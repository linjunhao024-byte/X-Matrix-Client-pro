use serde::Serialize;
use specta::Type;
use std::sync::Arc;
use tauri::State;
use tauri::{AppHandle, Manager, WebviewWindow};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::engine::EngineRuntime;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::sysproxy::ProxyGuard;

#[derive(Debug, Clone, Serialize, Type)]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub platform: String,
    pub arch: String,
}

/// Where the app keeps its things. Shown on the About page so a user can find
/// their config without guessing at `%APPDATA%`.
#[derive(Debug, Clone, Serialize, Type)]
pub struct AppPaths {
    pub data_dir: String,
    pub engines_dir: String,
    pub config_dir: String,
    pub log_dir: String,
}

/// Cheap liveness probe for the contract layer: if this renders, the whole
/// Rust -> bindings.ts -> Vue chain is wired correctly.
#[tauri::command]
#[specta::specta]
pub fn app_info() -> AppResult<AppInfo> {
    Ok(AppInfo {
        name: "X-Matrix".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        platform: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
    })
}

#[tauri::command]
#[specta::specta]
pub fn app_paths(app: AppHandle, runtime: tauri::State<'_, std::sync::Arc<EngineRuntime>>) -> AppResult<AppPaths> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| AppError::Internal(format!("找不到数据目录：{error}")))?;
    Ok(AppPaths {
        data_dir: data_dir.display().to_string(),
        engines_dir: runtime.engines_dir().display().to_string(),
        config_dir: runtime.config_dir().display().to_string(),
        log_dir: runtime.log_dir().display().to_string(),
    })
}

/// Opens the data directory (the folder that holds `store.json`).
#[tauri::command]
#[specta::specta]
pub fn app_open_data_dir(app: AppHandle) -> AppResult<()> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| AppError::Internal(format!("找不到数据目录：{error}")))?;
    app.opener()
        .open_path(data_dir.display().to_string(), None::<&str>)
        .map_err(|error| AppError::Internal(format!("打不开目录：{error}")))
}

/// Opens the log directory (where `xray.log` / `sing-box.log` live). The log
/// dialog gets a button for it: "the kernel says something went wrong" is
/// exactly the moment you want the whole file, not the last 200 lines.
#[tauri::command]
#[specta::specta]
pub fn app_open_log_dir(app: AppHandle, runtime: tauri::State<'_, std::sync::Arc<EngineRuntime>>) -> AppResult<()> {
    let log_dir = runtime.log_dir().display().to_string();
    app.opener()
        .open_path(log_dir, None::<&str>)
        .map_err(|error| AppError::Internal(format!("打不开日志目录：{error}")))
}

/// Opens a page in the user's browser: currently only the release page the
/// update check handed back.
///
/// The scheme is checked here rather than trusted from the caller. The URL
/// comes from a network response, and "open whatever string a server sent"
/// is how a proxy app ends up launching something it should not.
#[tauri::command]
#[specta::specta]
pub fn app_open_url(app: AppHandle, url: String) -> AppResult<()> {
    let trimmed = url.trim();
    if !trimmed.starts_with("https://") && !trimmed.starts_with("http://") {
        return Err(AppError::InvalidInput(format!("只打开发布页这类 https 链接，不打开「{trimmed}」")));
    }
    app.opener()
        .open_url(trimmed, None::<&str>)
        .map_err(|error| AppError::Internal(format!("打不开链接：{error}")))
}

/// Save arbitrary text the UI produced (a CSV of connections, a list of share
/// links) through the native dialog.
///
/// Kept generic on purpose: the alternative is one command per file format,
/// and every one of them would repeat the same "ask for a path, write it,
/// report a cancelled dialog as `None`" dance.
#[tauri::command]
#[specta::specta]
pub async fn save_text_file(app: AppHandle, default_name: String, contents: String) -> AppResult<Option<String>> {
    let name = default_name.trim();
    if name.is_empty() {
        return Err(AppError::InvalidInput("文件名不能为空".into()));
    }
    // The extension decides the filter, and an unknown one just means "no
    // filter" rather than an error: the user picked the name, not us.
    let extension = name.rsplit_once('.').map(|(_, ext)| ext.to_string());
    let handle = app.clone();
    let name = name.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let mut dialog = handle.dialog().file().set_title("保存文件").set_file_name(name);
        if let Some(extension) = extension.as_deref() {
            dialog = dialog.add_filter(extension.to_uppercase(), &[extension]);
        }
        let Some(path) = dialog.blocking_save_file() else {
            return Ok(None);
        };
        let path = path
            .into_path()
            .map_err(|error| AppError::InvalidInput(format!("所选路径不可用：{error}")))?;
        std::fs::write(&path, contents)?;
        Ok(Some(path.display().to_string()))
    })
    .await
    .map_err(|error| AppError::Internal(format!("保存任务没有跑完：{error}")))?
}

/// Write a redacted markdown report and let the user keep it.
///
/// The report is built *before* the dialog opens: it reads several locks and
/// runs both kernels' `version`, and doing that while a modal file dialog owns
/// the window would look like a freeze.
#[tauri::command]
#[specta::specta]
pub async fn export_diagnostics(
    app: AppHandle,
    state: State<'_, AppState>,
    runtime: State<'_, Arc<EngineRuntime>>,
    proxy: State<'_, Arc<ProxyGuard>>,
) -> AppResult<Option<String>> {
    let snapshot = crate::diagnostics::collect(&state, &runtime, &proxy);
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(path) = handle
            .dialog()
            .file()
            .set_title("导出诊断报告")
            .set_file_name("x-matrix-diagnostics.md")
            .add_filter("Markdown", &["md"])
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let path = path
            .into_path()
            .map_err(|error| AppError::InvalidInput(format!("所选路径不可用：{error}")))?;
        crate::diagnostics::write_to(&path, &snapshot)?;
        Ok(Some(path.display().to_string()))
    })
    .await
    .map_err(|error| AppError::Internal(format!("诊断报告没有写完：{error}")))?
}

/// Ask the release feed whether a newer build exists.
///
/// Off the IPC thread (`spawn_blocking`) because the whole point of a timeout is
/// that the window stays usable while the answer is late.
#[tauri::command]
#[specta::specta]
pub async fn check_update() -> AppResult<crate::update::UpdateStatus> {
    let current = env!("CARGO_PKG_VERSION");
    tauri::async_runtime::spawn_blocking(move || crate::update::check(current))
        .await
        .map_err(|error| AppError::Internal(format!("更新检查没有返回：{error}")))?
}

/// The address the outside world sees, and how the request reached it.
///
/// `via` is the honest half of the answer: an address read while the tunnel is
/// down belongs to this machine, and showing it without saying so would look
/// like traffic went through a proxy when it did not.
#[derive(Debug, Clone, Serialize, Type)]
pub struct ExitIp {
    /// The address exactly as the echo service answered it.
    pub ip: String,
    /// `"proxy"` (through the local mixed inbound) or `"direct"`.
    pub via: String,
}

/// The hosts tried in order until one answers.
///
/// A single hard-coded host turns "this network blocks that one service" into
/// "no exit IP at all": on the test machine `api.ipify.org` is unreachable
/// while all three below answer. The order is a decision — icanhazip first
/// (plain body, just the address), then ifconfig.me, then ipinfo.io — and it is
/// what the failure summary reports, so it is not incidental.
const EXIT_IP_CANDIDATES: [&str; 3] = [
    "https://ipv4.icanhazip.com",
    "https://ifconfig.me/ip",
    "https://ipinfo.io/ip",
];

/// Long enough for a slow mobile link, short enough that a request blocked by
/// the network is over before the user gives up on the button.
const EXIT_IP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);

/// How much of a bad answer we quote back before it stops being readable.
const EXIT_IP_ECHO_MAX: usize = 120;

/// Accept only something that really is an address.
///
/// A captive portal answers 200 with a login page, and some public endpoints
/// answer an HTML error page with a 200 as well; putting that body on the home
/// screen would present junk as "your exit IP". Whitespace is collapsed so a
/// multi-line body stays one readable line, and the echo is capped.
fn parse_exit_ip(body: &str) -> AppResult<String> {
    let text = body.trim();
    if text.is_empty() {
        return Err(AppError::Engine("出口 IP 的回答看不懂：空内容".into()));
    }
    if let Ok(address) = text.parse::<std::net::IpAddr>() {
        return Ok(address.to_string());
    }
    let single_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut echo: String = single_line.chars().take(EXIT_IP_ECHO_MAX).collect();
    if single_line.chars().count() > EXIT_IP_ECHO_MAX {
        echo.push('…');
    }
    Err(AppError::Engine(format!("出口 IP 的回答看不懂：{echo}")))
}

/// The short name used in the failure summary: the URL without its scheme.
///
/// Three full URLs make a 200-character error nobody reads, and the host is
/// what tells the user which service their network cannot reach.
fn candidate_host(url: &str) -> &str {
    url.strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url)
}

/// The one line shown when every candidate failed: each host once, in the order
/// it was tried, with the reason that host gave.
///
/// Pure, so the wording is testable without opening a socket.
fn attempts_line(attempts: &[(String, String)]) -> String {
    let joined = attempts
        .iter()
        .map(|(host, reason)| format!("{host} {reason}"))
        .collect::<Vec<_>>()
        .join("；");
    format!("出口 IP 都问不到：{joined}")
}

/// Ask a public echo service which address it saw.
///
/// Off the IPC thread (`spawn_blocking`) for the same reason `check_update` is:
/// the window has to keep painting while the timeout runs out.
///
/// The route is decided before the request. A running kernel means the mixed
/// inbound is up and the answer has to come through it; otherwise the same
/// request leaves the machine on its own, and `via` says which happened.
#[tauri::command]
#[specta::specta]
pub async fn exit_ip(app: AppHandle) -> AppResult<ExitIp> {
    // Read what the request needs while still on the IPC thread; no lock guard
    // has to survive the `await`.
    let (via, proxy) = {
        let status = app.state::<Arc<EngineRuntime>>().status();
        // The running kernel's HTTP inbound: that is the port that speaks HTTP,
        // and with a split it is not the configured mixed port. Taken from the
        // status, not the saved settings, so an unsaved port edit cannot send
        // this probe into a port nothing listens on.
        if status.running {
            ("proxy", Some(status.http_port))
        } else {
            ("direct", None)
        }
    };

    tauri::async_runtime::spawn_blocking(move || {
        let mut attempts: Vec<(String, String)> = Vec::new();
        for url in EXIT_IP_CANDIDATES {
            let answer =
                crate::net::remote::fetch_answer(url, EXIT_IP_TIMEOUT, None, proxy, "出口 IP");
            let reason = match answer {
                // Only a 2xx body counts: some services answer 403 with a page
                // that says nothing about the address.
                Ok(answer) if (200..300).contains(&answer.status) => {
                    match parse_exit_ip(&answer.body) {
                        // First accepted answer wins; the remaining hosts are
                        // not asked, so a working lookup stays one request.
                        Ok(ip) => return Ok(ExitIp { ip, via: via.to_string() }),
                        Err(error) => {
                            let text = error.to_string();
                            // "出口 IP 的回答看不懂" is already the summary's opening.
                            text.strip_prefix("出口 IP ").unwrap_or(&text).to_string()
                        }
                    }
                }
                Ok(answer) => format!("返回 HTTP {}", answer.status),
                Err(error) => {
                    let text = error.to_string();
                    // "拉取出口 IP失败" repeats what the summary says; keep "拉取失败：…".
                    format!("拉取失败：{}", text.strip_prefix("拉取出口 IP失败：").unwrap_or(&text))
                }
            };
            attempts.push((candidate_host(url).to_string(), reason));
        }
        Err(AppError::Engine(attempts_line(&attempts)))
    })
    .await
    .map_err(|error| AppError::Internal(format!("出口 IP 查询没有返回：{error}")))?
}

/// Ask for a text file and return its contents.
/// Subscription files, Clash configs and exported link lists are all just text,
/// so the importer can stay one code path: read the file, hand the string to
/// `import_preview`, done. The size cap keeps a mis-click on a 2 GB log from
/// freezing the window.
const MAX_IMPORT_BYTES: u64 = 4 * 1024 * 1024;

#[tauri::command]
#[specta::specta]
pub async fn open_text_file(app: AppHandle) -> AppResult<Option<String>> {
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let picked = handle
            .dialog()
            .file()
            .add_filter("文本文件", &["txt", "json", "yaml", "yml", "conf", "list"])
            .blocking_pick_file();
        let Some(path) = picked else {
            return Ok(None);
        };
        let path = path
            .into_path()
            .map_err(|error| AppError::InvalidInput(format!("这个路径读不出来：{error}")))?;
        let size = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
        if size > MAX_IMPORT_BYTES {
            return Err(AppError::InvalidInput(format!(
                "文件有 {} MB，超过 4 MB 上限；这么大的东西不是订阅",
                size / 1024 / 1024
            )));
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|error| AppError::InvalidInput(format!("读不了这个文件：{error}")))?;
        Ok(Some(text))
    })
    .await
    .map_err(|error| AppError::Internal(format!("读取任务没有跑完：{error}")))?
}

/// The title bar is drawn by the OS, so CSS cannot theme it: a dark app with a
/// white frame looks like a bug. The UI calls this whenever the theme resolves.
#[tauri::command]
#[specta::specta]
pub fn ui_set_window_theme(window: WebviewWindow, dark: bool) -> AppResult<()> {
    let theme = if dark {
        tauri::Theme::Dark
    } else {
        tauri::Theme::Light
    };
    window
        .set_theme(Some(theme))
        .map_err(|error| AppError::Engine(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fallback order is a decision (see the const): a harmless-looking
    /// reordering changes which service a user's first request hits.
    #[test]
    fn the_exit_ip_candidates_keep_their_documented_order() {
        assert_eq!(
            EXIT_IP_CANDIDATES,
            [
                "https://ipv4.icanhazip.com",
                "https://ifconfig.me/ip",
                "https://ipinfo.io/ip",
            ]
        );
    }

    /// A failed lookup has to say which services were tried and what each one
    /// answered, in order, with every host named exactly once.
    #[test]
    fn the_attempts_line_names_each_host_once_in_order() {
        let one = attempts_line(&[("ipinfo.io/ip".into(), "回答看不懂：<html>".into())]);
        assert_eq!(one, "出口 IP 都问不到：ipinfo.io/ip 回答看不懂：<html>");
        assert!(!one.contains('；'), "a single attempt needs no separator: {one}");

        let three = attempts_line(&[
            ("ipv4.icanhazip.com".into(), "拉取失败：连接超时".into()),
            ("ifconfig.me/ip".into(), "返回 HTTP 403".into()),
            ("ipinfo.io/ip".into(), "回答看不懂：<html>".into()),
        ]);
        assert!(three.starts_with("出口 IP 都问不到："), "{three}");
        assert_eq!(three.matches('；').count(), 2, "three attempts, two separators: {three}");
        for url in EXIT_IP_CANDIDATES {
            let host = candidate_host(url);
            assert_eq!(three.matches(host).count(), 1, "each host appears exactly once: {three}");
        }
        assert!(three.ends_with("ipinfo.io/ip 回答看不懂：<html>"), "{three}");
    }

    /// The common case: ipify answers one address and a trailing newline.
    #[test]
    fn an_ipv4_answer_loses_its_trailing_newline() {
        assert_eq!(parse_exit_ip("203.0.113.7\n").expect("valid IPv4"), "203.0.113.7");
    }

    #[test]
    fn an_ipv6_answer_is_kept_as_an_address() {
        assert_eq!(parse_exit_ip(" 2001:db8::1 \n").expect("valid IPv6"), "2001:db8::1");
    }

    /// A captive portal or an HTML error page must never be shown as an IP.
    #[test]
    fn a_garbage_answer_is_reported_instead_of_shown() {
        let error = parse_exit_ip("<html><body>Sign in to Wi-Fi</body></html>")
            .expect_err("HTML is not an address");
        assert!(
            error.to_string().contains("看不懂"),
            "the message must say the answer was unreadable: {error}"
        );
    }

    #[test]
    fn an_empty_answer_is_reported_too() {
        let error = parse_exit_ip("   \n\t").expect_err("blank body is not an address");
        assert!(
            error.to_string().contains("看不懂"),
            "the message must say the answer was unreadable: {error}"
        );
    }
}
