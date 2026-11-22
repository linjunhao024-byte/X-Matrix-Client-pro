//! One file a user can hand over when something is wrong.
//!
//! Support conversations stall on "what does your log say" — the engine log
//! lives three directories away and the generated config contains credentials
//! the user should not paste into a chat window. This module renders both into
//! a single markdown report with every secret replaced, so the answer to
//! "send me your diagnostics" is one button.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

use crate::engine::{EngineKind, EngineRuntime};
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::sysproxy::ProxyGuard;

/// Key names whose *values* never leave the machine.
///
/// Matched case-sensitively against the raw text, because kernels spell these
/// exactly this way in their JSON (`"uuid"`, `"password"`, `"publicKey"`).
/// Erring towards "masked too much" is deliberate: a report nobody can send is
/// useless, and a leaked UUID has to be rotated.
const SECRET_KEYS: [&str; 16] = [
    "uuid",
    "password",
    "passwd",
    "psk",
    "token",
    "auth",
    "secret",
    "secretKey",
    "secret_key",
    "publicKey",
    "public_key",
    "shortId",
    "short_id",
    "privateKey",
    "private_key",
    "id",
];

/// Replace the value that follows each secret-looking key with `***`.
///
/// Hand-rolled instead of a regex crate because the shapes are finite and this
/// runs once per export: `<key>` then optional quote/space then `:` or `=`
/// then optional quote, then the value up to the next delimiter.
pub fn redact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let Some((pos, key)) = next_secret(rest) else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..pos]);
        out.push_str(key);
        let after = &rest[pos + key.len()..];
        match mask_after_key(after) {
            Some((masked, tail)) => {
                out.push_str(&masked);
                rest = tail;
            }
            None => rest = after,
        }
    }
    out
}

/// Earliest secret key in `text`, with a word-boundary check on the left.
fn next_secret(text: &str) -> Option<(usize, &'static str)> {
    let mut best: Option<(usize, &'static str)> = None;
    for key in SECRET_KEYS {
        let mut from = 0;
        while let Some(found) = text[from..].find(key) {
            let at = from + found;
            let before = text[..at].chars().next_back();
            let clean_left = before.is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == '-'));
            if clean_left && after_key_is_separator(&text[at + key.len()..]) {
                if best.is_none_or(|(p, _)| at < p) {
                    best = Some((at, key));
                }
                break;
            }
            from = at + key.len();
        }
    }
    best
}

/// Is the text right after a key actually a separator (`: ` / `=` / `": `)?
fn after_key_is_separator(text: &str) -> bool {
    let trimmed = text.trim_start_matches([' ', '\t']);
    let trimmed = trimmed.strip_prefix('"').unwrap_or(trimmed);
    let trimmed = trimmed.trim_start_matches([' ', '\t']);
    trimmed.starts_with(':') || trimmed.starts_with('=')
}

/// Replace the value that follows a key, keeping the key's own punctuation.
///
/// The separator, its spacing and the opening quote stay as written, so
/// `"id": "***"` is still valid JSON, `password: ***` still valid YAML and
/// `?token=***&x=1` still a URL. The text after the value is handed back so
/// the caller can carry on with it.
fn mask_after_key(text: &str) -> Option<(String, &str)> {
    fn skip_spaces(text: &str, at: &mut usize) {
        while matches!(text.as_bytes().get(*at), Some(b' ') | Some(b'\t')) {
            *at += 1;
        }
    }
    let mut at = 0;
    skip_spaces(text, &mut at);
    // A JSON key brings its own closing quote: `"id": "x"`.
    if text.as_bytes().get(at) == Some(&b'"') {
        at += 1;
    }
    skip_spaces(text, &mut at);
    if !matches!(text.as_bytes().get(at), Some(b':') | Some(b'=')) {
        return None;
    }
    at += 1;
    skip_spaces(text, &mut at);
    let quoted = text.as_bytes().get(at) == Some(&b'"');
    if quoted {
        at += 1;
    }
    let value_start = at;
    let end = if quoted {
        value_start + text[value_start..].find('"')?
    } else {
        let rest = &text[value_start..];
        value_start
            + rest
                .find(['"', ',', '}', ']', '&', ' ', '\t', '\n', '\r', ')'])
                .unwrap_or(rest.len())
    };
    // Everything before the value is kept verbatim; a closing quote stays at
    // the head of the tail, so quotes remain balanced.
    Some((format!("{}***", &text[..value_start]), &text[end..]))
}

/// Everything the report says, already flattened.
///
/// Collected here (not formatted here) so the layout can be tested against a
/// fixed snapshot without a running app, a data directory or a kernel.
#[derive(Debug, Default, Clone)]
pub struct Snapshot {
    pub version: String,
    pub os: String,
    pub arch: String,
    pub generated_unix: u64,
    pub engines: Vec<EngineRow>,
    pub dirs: Vec<(String, String)>,
    pub rows: Vec<(String, Vec<(String, String)>)>,
    pub logs: Vec<(String, String)>,
    pub config: String,
}

#[derive(Debug, Clone)]
pub struct EngineRow {
    pub kind: String,
    pub installed: bool,
    pub path: String,
    pub bytes: Option<u64>,
    pub version: Option<String>,
}

impl Snapshot {
    /// Render markdown. Pure: takes the snapshot, returns a string.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str("# X-Matrix 诊断报告\n\n");
        out.push_str(
            "这份报告是给排查问题用的，凭据已经打码，可以直接发给别人。\n\n",
        );
        out.push_str(&format!(
            "- 版本：{}（{} / {}）\n- 生成时间：{}\n\n",
            self.version,
            self.os,
            self.arch,
            format_unix(self.generated_unix),
        ));

        out.push_str("## 内核\n\n");
        out.push_str("| 内核 | 已安装 | 大小 | 版本 | 路径 |\n|---|---|---|---|---|\n");
        for engine in &self.engines {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                engine.kind,
                if engine.installed { "是" } else { "否" },
                engine
                    .bytes
                    .map(format_bytes)
                    .unwrap_or_else(|| "—".into()),
                engine.version.as_deref().unwrap_or("未安装或无法执行"),
                engine.path,
            ));
        }

        out.push_str("\n## 目录\n\n");
        for (label, value) in &self.dirs {
            out.push_str(&format!("- {label}：`{value}`\n"));
        }

        for (title, rows) in &self.rows {
            out.push_str(&format!("\n## {title}\n\n"));
            for (key, value) in rows {
                // An empty value renders as a dash: a key with nothing after it
                // reads like the collector broke, not like the list is empty.
                let value = if value.trim().is_empty() {
                    "—".to_string()
                } else {
                    redact(value)
                };
                out.push_str(&format!("- `{key}`：{value}\n"));
            }
        }

        for (title, text) in &self.logs {
            out.push_str(&format!("\n## {title}\n\n```\n{}\n```\n", redact(text)));
        }

        if !self.config.is_empty() {
            out.push_str("\n## 生成配置（已打码）\n\n```json\n");
            out.push_str(&redact(&self.config));
            out.push_str("\n```\n");
        }

        out.push_str("\n---\n\n本报告已打码，可直接分享。\n");
        out
    }
}

/// Read the live app state into a snapshot.
pub fn collect(state: &AppState, runtime: &EngineRuntime, proxy: &ProxyGuard) -> Snapshot {
    let settings = state.settings.read().clone();
    let nodes = state.nodes.read().clone();
    let subscriptions = state.subscriptions.read().clone();
    let rules = state.rules.read().clone();
    let groups = state.groups.read().clone();
    let rule_sets = state.rule_sets.read().clone();

    let engines = runtime
        .binaries()
        .into_iter()
        .map(|binary| {
            let path = std::path::PathBuf::from(&binary.path);
            let bytes = std::fs::metadata(&path).ok().map(|meta| meta.len());
            let version = runtime
                .version(binary.kind)
                .ok()
                .map(|line| line.trim().to_string());
            EngineRow {
                kind: binary.kind.as_str().to_string(),
                installed: binary.present,
                path: binary.path,
                bytes,
                version,
            }
        })
        .collect();

    let mut protocols: BTreeMap<String, usize> = BTreeMap::new();
    for node in &nodes {
        // The enum's variant name is the protocol name the user sees; `{:?}`
        // adds the payload, so keep only the first word.
        let name = format!("{:?}", node.protocol);
        let name = name.split_whitespace().next().unwrap_or("?").to_string();
        *protocols.entry(name).or_default() += 1;
    }

    let status = runtime.status();
    let proxy_status = proxy.status().ok();

    let mut rows: Vec<(String, Vec<(String, String)>)> = Vec::new();
    rows.push((
        "设置".into(),
        to_rows(&serde_json::to_value(&settings).unwrap_or(Value::Null)),
    ));
    rows.push((
        "规模".into(),
        vec![
            ("节点".into(), nodes.len().to_string()),
            (
                "协议".into(),
                protocols
                    .iter()
                    .map(|(name, count)| format!("{name} {count}"))
                    .collect::<Vec<_>>()
                    .join(" · "),
            ),
            (
                "订阅".into(),
                subscriptions
                    .iter()
                    .map(|item| {
                        format!(
                            "{}（{} 个节点，{}）",
                            item.name,
                            item.node_count,
                            if item.enabled { "启用" } else { "停用" }
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("；"),
            ),
            ("路由规则".into(), rules.len().to_string()),
            (
                "策略组".into(),
                groups
                    .iter()
                    .map(|group| format!("{}（{} 个成员）", group.name, group.members.len()))
                    .collect::<Vec<_>>()
                    .join("；"),
            ),
            (
                "规则集".into(),
                rule_sets
                    .iter()
                    .map(|set| format!("{}（{} 条）", set.name, set.entry_count))
                    .collect::<Vec<_>>()
                    .join("；"),
            ),
        ],
    ));
    rows.push((
        "引擎状态".into(),
        vec![
            ("运行中".into(), status.running.to_string()),
            (
                "内核".into(),
                status
                    .kind
                    .map(|kind| kind.as_str().to_string())
                    .unwrap_or_else(|| "—".into()),
            ),
            ("节点".into(), status.node_name.clone()),
            ("入站端口".into(), status.mixed_port.to_string()),
            ("控制端口".into(), status.api_port.to_string()),
            (
                "上次退出码".into(),
                status
                    .last_exit_code
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "—".into()),
            ),
        ],
    ));
    rows.push((
        "系统代理".into(),
        match proxy_status {
            Some(status) => vec![
                ("由本程序持有".into(), status.active.to_string()),
                ("系统已启用".into(), status.enabled.to_string()),
                ("服务器".into(), status.server),
                ("残留快照".into(), status.snapshot_present.to_string()),
            ],
            None => vec![("读取失败".into(), "—".into())],
        },
    ));

    let logs = EngineKind::all()
        .iter()
        .map(|kind| {
            let text = runtime
                .log_tail(*kind, 200)
                .unwrap_or_else(|error| format!("（读不到日志：{error}）"));
            (
                format!("{} 日志（最后 200 行）", kind.as_str()),
                text,
            )
        })
        .collect();

    let config = crate::commands::engine::render_for_preview(&state, runtime)
        .unwrap_or_else(|error| format!("（渲染配置失败：{error}）"));

    Snapshot {
        version: env!("CARGO_PKG_VERSION").to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        generated_unix: unix_now(),
        engines,
        dirs: vec![
            (
                "engines".into(),
                runtime.engines_dir().display().to_string(),
            ),
            ("configs".into(), runtime.config_dir().display().to_string()),
            ("logs".into(), runtime.log_dir().display().to_string()),
            (
                "rulesets".into(),
                runtime.rule_set_dir().display().to_string(),
            ),
        ],
        rows,
        logs,
        config,
    }
}

/// Flatten a JSON object into `(key, value)` rows, one level deep.
fn to_rows(value: &Value) -> Vec<(String, String)> {
    let Some(object) = value.as_object() else {
        return vec![("原始".into(), value.to_string())];
    };
    object
        .iter()
        .map(|(key, value)| {
            let text = match value {
                Value::String(text) => text.clone(),
                other => other.to_string(),
            };
            (key.clone(), if text.is_empty() { "—".into() } else { text })
        })
        .collect()
}

/// Write the report to `path`, creating the parent directory if needed.
pub fn write_to(path: &Path, snapshot: &Snapshot) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|error| AppError::Io(format!("建不了目录 {}：{error}", parent.display())))?;
        }
    }
    std::fs::write(path, snapshot.render())
        .map_err(|error| AppError::Io(format!("写不进 {}：{error}", path.display())))
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

/// Seconds since the epoch, spelled as UTC. Local time would need a timezone
/// database; a wrong-looking timestamp is better than a wrong one, so say UTC.
fn format_unix(seconds: u64) -> String {
    let days = seconds / 86_400;
    let rest = seconds % 86_400;
    let (year, month, day) = civil_from_days(days as i64);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        rest / 3600,
        (rest % 3600) / 60,
        rest % 60
    )
}

/// Days since 1970-01-01 → civil date (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { y + 1 } else { y }, month, day)
}

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_json_credentials() {
        let text = r#"{"id":"8f1c-uuid","password":"hunter2","server":"example.com","port":443}"#;
        let masked = redact(text);
        assert!(!masked.contains("8f1c-uuid"), "{masked}");
        assert!(!masked.contains("hunter2"), "{masked}");
        // Address and port stay: they are what the reader needs to help.
        assert!(masked.contains("example.com"));
        assert!(masked.contains("443"));
    }

    #[test]
    fn masks_colon_and_equals_forms() {
        assert!(!redact("password: hunter2").contains("hunter2"));
        assert!(!redact("uuid = 55e1-aaaa").contains("55e1-aaaa"));
        assert!(!redact("https://example.com/sub?token=abc123&x=1").contains("abc123"));
    }

    #[test]
    fn masking_keeps_the_punctuation_that_keeps_it_valid() {
        // JSON stays parseable, and the neighbours of the masked value survive.
        assert_eq!(
            redact(r#"{"id": "abc-def", "port": 443}"#),
            r#"{"id": "***", "port": 443}"#
        );
        assert_eq!(redact("password: hunter2\nrest"), "password: ***\nrest");
        assert_eq!(
            redact("https://example.com/sub?token=abc&x=1"),
            "https://example.com/sub?token=***&x=1"
        );
        // The key itself is never swallowed.
        assert!(redact(r#""uuid": "x""#).starts_with(r#""uuid""#));
    }

    #[test]
    fn an_empty_row_reads_as_empty_not_as_a_failure() {
        let mut snapshot = Snapshot::default();
        snapshot.rows.push(("规模".into(), vec![("订阅".into(), String::new())]));
        assert!(snapshot.render().contains("- `订阅`：—"));
    }

    #[test]
    fn leaves_ordinary_text_alone() {
        let text = "节点 HK-01 127.0.0.1:443 运行中";
        assert_eq!(redact(text), text);
        // "valid" contains "id" but must not be treated as a key.
        assert_eq!(redact("valid: yes"), "valid: yes");
    }

    #[test]
    fn renders_a_report_from_an_empty_snapshot() {
        let snapshot = Snapshot {
            version: "0.1.0".into(),
            os: "windows".into(),
            arch: "x86_64".into(),
            generated_unix: 0,
            ..Snapshot::default()
        };
        let text = snapshot.render();
        assert!(text.contains("# X-Matrix 诊断报告"));
        assert!(text.contains("1970-01-01"));
        assert!(text.contains("本报告已打码"));
    }

    #[test]
    fn renders_engine_rows() {
        let snapshot = Snapshot {
            engines: vec![EngineRow {
                kind: "xray".into(),
                installed: false,
                path: "C:/x/engines/xray.exe".into(),
                bytes: None,
                version: None,
            }],
            ..Snapshot::default()
        };
        let text = snapshot.render();
        assert!(text.contains("未安装或无法执行"));
        assert!(text.contains("xray"));
        assert!(text.contains("8,315,392") == false);
    }

    #[test]
    fn civil_dates_round_trip_known_days() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_000), (2022, 1, 8));
        assert!(format_unix(0).starts_with("1970-01-01"));
    }
}
