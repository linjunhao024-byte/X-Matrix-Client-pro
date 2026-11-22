//! Turning share links into nodes.
//!
//! Everything here is pure: text in, nodes out. No network, no disk, no state.
//! That keeps the messy part (four link dialects, base64 in two flavours,
//! percent-encoded names) testable without a server, and lets the UI show a
//! preview before anything is written.
//!
//! Supported: `vless://`, `vmess://`, `trojan://`, `ss://`, either bare (one per
//! line) or wrapped in a base64 subscription document.

use serde::Serialize;

use crate::domain::node::{Node, Protocol, Security, Transport};
use crate::error::{AppError, AppResult};

/// Result of a parse attempt: what could be read, and what was skipped.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ParseReport {
    pub nodes: Vec<Node>,
    /// One human-readable line per line that could not be understood.
    pub skipped: Vec<String>,
}

/// Parse a blob of text: a subscription body, a file, or a pasted list.
pub fn parse(text: &str) -> ParseReport {
    let mut report = ParseReport::default();

    // A subscription is usually the whole list, base64-encoded. Decode only if
    // the text does not already look like links.
    let body = if text.contains("://") {
        text.to_string()
    } else {
        match decode_base64(text.trim()) {
            Some(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            None => text.to_string(),
        }
    };

    let mut index = 0usize;
    for raw in body.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }
        match parse_link(line, index) {
            Ok(node) => {
                index += 1;
                report.nodes.push(node);
            }
            Err(error) => report.skipped.push(format!("第 {} 行：{error}", report.skipped.len() + 1)),
        }
    }

    report
}

/// Parse exactly one link.
pub fn parse_link(line: &str, index: usize) -> AppResult<Node> {
    let lower = line.to_ascii_lowercase();
    if lower.starts_with("vless://") {
        parse_vless(line, index)
    } else if lower.starts_with("vmess://") {
        parse_vmess(line, index)
    } else if lower.starts_with("trojan://") {
        parse_trojan(line, index)
    } else if lower.starts_with("ss://") {
        parse_shadowsocks(line, index)
    } else {
        Err(AppError::InvalidInput(format!(
            "不支持的链接类型：{}",
            line.split("://").next().unwrap_or("?")
        )))
    }
}

fn parse_vless(line: &str, index: usize) -> AppResult<Node> {
    let rest = strip_scheme(line, "vless://");
    let (head, query, name) = split_link(rest);
    let (user, host, port) = split_authority(head)?;
    if user.is_empty() {
        return Err(AppError::InvalidInput("vless 链接缺少 uuid".into()));
    }

    Ok(Node {
        id: make_id("vless", index),
        name: name.unwrap_or_else(|| format!("vless-{}", index + 1)),
        server: host,
        port,
        protocol: Protocol::Vless {
            uuid: user,
            flow: non_empty(query.get("flow")),
        },
        transport: transport_from_query(&query),
        security: security_from_query(&query),
        tags: vec![],
    })
}

fn parse_trojan(line: &str, index: usize) -> AppResult<Node> {
    let rest = strip_scheme(line, "trojan://");
    let (head, query, name) = split_link(rest);
    let (password, host, port) = split_authority(head)?;
    if password.is_empty() {
        return Err(AppError::InvalidInput("trojan 链接缺少密码".into()));
    }

    // Trojan is TLS by definition, so a link without `security=` still means
    // TLS — only an explicit `security=none` turns it off.
    let security = match query.get("security").as_deref() {
        None => Security::Tls {
            sni: query.get("sni").or_else(|| query.get("host")),
            alpn: vec![],
            allow_insecure: insecure_flag(&query),
            pinned_cert_sha256: cert_pins(&query),
        },
        Some(_) => security_from_query(&query),
    };

    Ok(Node {
        id: make_id("trojan", index),
        name: name.unwrap_or_else(|| format!("trojan-{}", index + 1)),
        server: host,
        port,
        protocol: Protocol::Trojan { password },
        transport: transport_from_query(&query),
        security,
        tags: vec![],
    })
}

fn parse_vmess(line: &str, index: usize) -> AppResult<Node> {
    // Some panels append `#name` to a vmess link even though the name belongs
    // in the JSON payload; base64 cannot contain `#`, so dropping the fragment
    // is unambiguous.
    let payload = strip_scheme(line, "vmess://");
    let payload = payload.split('#').next().unwrap_or(payload);
    let bytes = decode_base64(payload)
        .ok_or_else(|| AppError::InvalidInput("vmess 链接不是合法的 base64".into()))?;
    let text = String::from_utf8_lossy(&bytes);
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|_| AppError::InvalidInput("vmess 内容不是合法的 JSON".into()))?;

    let field = |key: &str| -> String {
        match value.get(key) {
            Some(serde_json::Value::String(text)) => text.clone(),
            Some(serde_json::Value::Number(number)) => number.to_string(),
            _ => String::new(),
        }
    };

    let server = field("add");
    let port = field("port")
        .parse::<u16>()
        .map_err(|_| AppError::InvalidInput("vmess 端口不是数字".into()))?;
    let uuid = field("id");
    if server.is_empty() || uuid.is_empty() {
        return Err(AppError::InvalidInput("vmess 缺少服务器或 uuid".into()));
    }

    let alter_id = field("aid").parse::<u16>().unwrap_or(0);
    let network = field("net");
    let path = field("path");
    let host = field("host");
    let tls = field("tls");
    let sni = field("sni");

    let transport = match network.as_str() {
        "ws" | "websocket" => Transport::Ws {
            path: if path.is_empty() { "/".into() } else { path },
            host: non_empty(Some(host)),
        },
        "grpc" => Transport::Grpc {
            service_name: if path.is_empty() {
                "grpc".into()
            } else {
                path
            },
        },
        _ => Transport::Tcp,
    };

    let security = if tls.eq_ignore_ascii_case("tls") || tls.eq_ignore_ascii_case("reality") {
        Security::Tls {
            sni: non_empty(Some(sni)),
            alpn: vec![],
            allow_insecure: field("allowInsecure") == "true" || field("allowInsecure") == "1",
            pinned_cert_sha256: field("pinnedPeerCertSha256")
                .split(',')
                .map(|item| item.trim().replace(|c| c == ':' || c == ' ', "").to_lowercase())
                .filter(|item| !item.is_empty())
                .collect(),
        }
    } else {
        Security::None
    };

    let name = field("ps");

    Ok(Node {
        id: make_id("vmess", index),
        name: if name.is_empty() {
            format!("vmess-{}", index + 1)
        } else {
            name
        },
        server,
        port,
        protocol: Protocol::Vmess { uuid, alter_id },
        transport,
        security,
        tags: vec![],
    })
}

fn parse_shadowsocks(line: &str, index: usize) -> AppResult<Node> {
    let rest = strip_scheme(line, "ss://");
    let (head, _query, name) = split_link(rest);

    // SIP002: `base64(method:password)@host:port`, or the legacy form where the
    // whole `method:password@host:port` is base64-encoded.
    let (credentials, host, port) = if head.contains('@') {
        let (user, host, port) = split_authority(head)?;
        let decoded = decode_base64(&user)
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or(user);
        (decoded, host, port)
    } else {
        let bytes = decode_base64(head)
            .ok_or_else(|| AppError::InvalidInput("ss 链接不是合法的 base64".into()))?;
        let decoded = String::from_utf8_lossy(&bytes).into_owned();
        let (user, host, port) = split_authority(&decoded)?;
        (user, host, port)
    };

    let (method, password) = credentials
        .split_once(':')
        .ok_or_else(|| AppError::InvalidInput("ss 凭据缺少加密方式或密码".into()))?;
    if method.is_empty() || password.is_empty() {
        return Err(AppError::InvalidInput("ss 凭据不完整".into()));
    }

    Ok(Node {
        id: make_id("ss", index),
        name: name.unwrap_or_else(|| format!("ss-{}", index + 1)),
        server: host,
        port,
        protocol: Protocol::Shadowsocks {
            method: method.to_string(),
            password: password.to_string(),
        },
        transport: Transport::Tcp,
        security: Security::None,
        tags: vec![],
    })
}

/// `pcs` / `pinnedPeerCertSha256` carry one or more SHA-256 fingerprints of the
/// server certificate, comma separated. Colons and spaces are stripped because
/// every client prints fingerprints differently, and Xray expects bare hex.
fn cert_pins(query: &Query) -> Vec<String> {
    ["pcs", "pinnedPeerCertSha256", "pinSHA256"]
        .iter()
        .filter_map(|key| query.get(*key))
        .flat_map(|raw| {
            raw.split(',')
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .map(|item| item.trim().replace(|c| c == ':' || c == ' ', "").to_lowercase())
        .filter(|item| !item.is_empty())
        .collect()
}

/// `allowInsecure=1` (Xray) and `insecure=1` (some panels) both mean "do not
/// verify the certificate". Anything else, including a missing value, means
/// verification stays on — a link must opt in explicitly.
fn insecure_flag(query: &Query) -> bool {
    ["allowInsecure", "insecure", "allowinsecure"]
        .iter()
        .any(|key| matches!(query.get(*key).as_deref(), Some("1") | Some("true") | Some("yes")))
}

fn transport_from_query(query: &Query) -> Transport {
    match query.get("type").as_deref() {
        Some("ws") | Some("websocket") => Transport::Ws {
            path: query.get("path").unwrap_or_else(|| "/".to_string()),
            host: query.get("host"),
        },
        Some("grpc") => Transport::Grpc {
            service_name: query
                .get("serviceName")
                .or_else(|| query.get("service"))
                .unwrap_or_else(|| "grpc".to_string()),
        },
        _ => Transport::Tcp,
    }
}

fn security_from_query(query: &Query) -> Security {
    match query.get("security").as_deref() {
        Some("tls") => Security::Tls {
            sni: query.get("sni").or_else(|| query.get("host")),
            allow_insecure: insecure_flag(query),
            pinned_cert_sha256: cert_pins(query),
            alpn: query
                .get("alpn")
                .map(|raw| {
                    raw.split(',')
                        .map(|item| item.trim().to_string())
                        .filter(|item| !item.is_empty())
                        .collect()
                })
                .unwrap_or_default(),
        },
        Some("reality") => Security::Reality {
            public_key: query.get("pbk").unwrap_or_default(),
            short_id: query.get("sid").unwrap_or_default(),
            server_name: query.get("sni").unwrap_or_default(),
            // `fp` is the uTLS fingerprint and `spx` the spider path. Links
            // from other clients often omit them; chrome + "/" is what those
            // clients default to, so an absent value must not mean "none".
            fingerprint: query
                .get("fp")
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "chrome".to_string()),
            spider_x: query
                .get("spx")
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "/".to_string()),
        },
        _ => Security::None,
    }
}

/// Query-string parameters, already percent-decoded.
#[derive(Debug, Default)]
struct Query(std::collections::HashMap<String, String>);

impl Query {
    fn new() -> Self {
        Self::default()
    }

    fn insert(&mut self, key: String, value: String) {
        self.0.insert(key, value);
    }

    fn get(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }
}

fn strip_scheme<'a>(line: &'a str, scheme: &str) -> &'a str {
    // The scheme was matched case-insensitively, so strip by length.
    &line[scheme.len().min(line.len())..]
}

/// Split `authority?query#name` into its three parts.
fn split_link(rest: &str) -> (&str, Query, Option<String>) {
    let (before_fragment, name) = match rest.split_once('#') {
        Some((head, fragment)) => (head, Some(percent_decode(fragment))),
        None => (rest, None),
    };
    let (head, query) = match before_fragment.split_once('?') {
        Some((head, raw)) => (head, parse_query(raw)),
        None => (before_fragment, Query::new()),
    };
    let name = name.filter(|value| !value.trim().is_empty());
    (head, query, name)
}

fn parse_query(raw: &str) -> Query {
    let mut map = Query::new();
    for pair in raw.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = match pair.split_once('=') {
            Some((key, value)) => (key, value),
            None => (pair, ""),
        };
        map.insert(percent_decode(key), percent_decode(value));
    }
    map
}

/// `user@host:port` → its three parts. Tolerates brackets around IPv6 hosts.
fn split_authority(head: &str) -> AppResult<(String, String, u16)> {
    // The userinfo is percent-encoded in links (`ss://`/`trojan://` passwords
    // and some panels' uuids), so decode it here — otherwise a password with
    // `#` or `@` arrives mangled.
    let (user, host_port) = match head.rsplit_once('@') {
        Some((user, host_port)) => (percent_decode(user), host_port),
        None => (String::new(), head),
    };

    let (host, port) = if let Some(rest) = host_port.strip_prefix('[') {
        // [::1]:443
        match rest.split_once("]:") {
            Some((host, port)) => (host.to_string(), port),
            None => return Err(AppError::InvalidInput("IPv6 地址缺少端口".into())),
        }
    } else {
        match host_port.rsplit_once(':') {
            Some((host, port)) => (host.to_string(), port),
            None => return Err(AppError::InvalidInput("地址缺少端口".into())),
        }
    };

    let port = port
        .parse::<u16>()
        .map_err(|_| AppError::InvalidInput(format!("端口 {port} 不是合法数字")))?;
    if host.trim().is_empty() {
        return Err(AppError::InvalidInput("地址为空".into()));
    }

    Ok((percent_decode(&user), host, port))
}

fn non_empty(value: Option<String>) -> Option<String> {
    let trimmed = value.unwrap_or_default();
    let trimmed = trimmed.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// A suffix that is unique across imports in one session.
///
/// Nanoseconds plus a counter: two imports in the same millisecond must not
/// produce the same node id, or the second silently overwrites the first.
pub fn unique_suffix() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static SEQ: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    format!("{nanos:x}{:x}", SEQ.fetch_add(1, Ordering::Relaxed))
}

fn make_id(prefix: &str, index: usize) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("{prefix}-{nanos:x}-{index}")
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = &value[index + 1..index + 3];
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Base64 in either alphabet, with or without padding.
fn decode_base64(input: &str) -> Option<Vec<u8>> {
    use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
    use base64::Engine as _;

    let cleaned: String = input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '\n' && *c != '\r')
        .collect();
    if cleaned.is_empty() {
        return None;
    }

    // `base64::Engine` is not dyn-compatible, so try the four alphabets in turn.
    if let Ok(bytes) = STANDARD.decode(cleaned.as_bytes()) {
        return Some(bytes);
    }
    if let Ok(bytes) = STANDARD_NO_PAD.decode(cleaned.as_bytes()) {
        return Some(bytes);
    }
    if let Ok(bytes) = URL_SAFE.decode(cleaned.as_bytes()) {
        return Some(bytes);
    }
    if let Ok(bytes) = URL_SAFE_NO_PAD.decode(cleaned.as_bytes()) {
        return Some(bytes);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_vless_reality_link() {
        let report = parse(
            "vless://11111111-2222-3333-4444-555555555555@example.com:443?type=tcp&security=reality&pbk=PUBKEY&sid=ab12&sni=www.example.com&flow=xtls-rprx-vision&fp=firefox&spx=%2Fspider#%E9%A6%99%E6%B8%AF",
        );
        assert_eq!(report.nodes.len(), 1, "{:?}", report.skipped);
        let node = &report.nodes[0];
        assert_eq!(node.name, "香港");
        assert_eq!(node.server, "example.com");
        assert_eq!(node.port, 443);
        match &node.protocol {
            Protocol::Vless { uuid, flow } => {
                assert_eq!(uuid, "11111111-2222-3333-4444-555555555555");
                assert_eq!(flow.as_deref(), Some("xtls-rprx-vision"));
            }
            other => panic!("unexpected protocol {other:?}"),
        }
        match &node.security {
            Security::Reality {
                public_key,
                short_id,
                server_name,
                fingerprint,
                spider_x,
            } => {
                assert_eq!(public_key, "PUBKEY");
                assert_eq!(short_id, "ab12");
                assert_eq!(server_name, "www.example.com");
                // The link carries fp/spx, and they must survive the trip.
                assert_eq!(fingerprint, "firefox");
                assert_eq!(spider_x, "/spider");
            }
            other => panic!("unexpected security {other:?}"),
        }
    }

    #[test]
    fn a_reality_link_without_fp_or_spx_falls_back_to_the_client_defaults() {
        // Most share links omit both; an empty fingerprint would make Xray
        // fall back to its own default and an empty spider path is invalid.
        let report = parse("vless://uuid-1@example.com:443?security=reality&pbk=PK&sid=ab&sni=www.example.com");
        match &report.nodes[0].security {
            Security::Reality {
                fingerprint,
                spider_x,
                ..
            } => {
                assert_eq!(fingerprint, "chrome");
                assert_eq!(spider_x, "/");
            }
            other => panic!("unexpected security {other:?}"),
        }
    }

    #[test]
    fn an_insecure_tls_link_opts_out_of_verification() {
        let report = parse("vless://uuid-1@example.com:443?security=tls&sni=a.example.com&allowInsecure=1");
        match &report.nodes[0].security {
            Security::Tls {
                allow_insecure, ..
            } => assert!(*allow_insecure),
            other => panic!("unexpected security {other:?}"),
        }
    }

    #[test]
    fn a_tls_link_verifies_the_certificate_by_default() {
        let report = parse("vless://uuid-1@example.com:443?security=tls&sni=a.example.com");
        match &report.nodes[0].security {
            Security::Tls {
                allow_insecure, ..
            } => assert!(!*allow_insecure),
            other => panic!("unexpected security {other:?}"),
        }
    }

    #[test]
    fn reads_a_trojan_ws_link() {
        let report = parse("trojan://secret@example.com:8443?type=ws&path=%2Fws&host=cdn.example.com&sni=cdn.example.com#Trojan");
        let node = &report.nodes[0];
        assert_eq!(node.name, "Trojan");
        assert_eq!(node.port, 8443);
        match &node.transport {
            Transport::Ws { path, host } => {
                assert_eq!(path, "/ws");
                assert_eq!(host.as_deref(), Some("cdn.example.com"));
            }
            other => panic!("unexpected transport {other:?}"),
        }
        match &node.security {
            Security::Tls { sni, .. } => assert_eq!(sni.as_deref(), Some("cdn.example.com")),
            other => panic!("unexpected security {other:?}"),
        }
    }

    #[test]
    fn reads_a_vmess_json_link() {
        // {"v":"2","ps":"VM","add":"example.com","port":"443","id":"uuid-1","aid":"0","net":"ws","path":"/ray","host":"cdn","tls":"tls","sni":"cdn"}
        let payload = "eyJ2IjoiMiIsInBzIjoiVk0iLCJhZGQiOiJleGFtcGxlLmNvbSIsInBvcnQiOiI0NDMiLCJpZCI6InV1aWQtMSIsImFpZCI6IjAiLCJuZXQiOiJ3cyIsInBhdGgiOiIvcmF5IiwiaG9zdCI6ImNkbiIsInRscyI6InRscyIsInNuaSI6ImNkbiJ9";
        let report = parse(&format!("vmess://{payload}"));
        assert_eq!(report.nodes.len(), 1, "{:?}", report.skipped);
        let node = &report.nodes[0];
        assert_eq!(node.name, "VM");
        assert_eq!(node.server, "example.com");
        match &node.protocol {
            Protocol::Vmess { uuid, alter_id } => {
                assert_eq!(uuid, "uuid-1");
                assert_eq!(*alter_id, 0);
            }
            other => panic!("unexpected protocol {other:?}"),
        }
        match &node.transport {
            Transport::Ws { path, .. } => assert_eq!(path, "/ray"),
            other => panic!("unexpected transport {other:?}"),
        }
    }

    #[test]
    fn reads_sip002_shadowsocks() {
        // base64("aes-256-gcm:pw") = YWVzLTI1Ni1nY206cHc=
        let report = parse("ss://YWVzLTI1Ni1nY206cHc=@example.com:8388#SS%20Node");
        assert_eq!(report.nodes.len(), 1, "{:?}", report.skipped);
        let node = &report.nodes[0];
        assert_eq!(node.name, "SS Node");
        match &node.protocol {
            Protocol::Shadowsocks { method, password } => {
                assert_eq!(method, "aes-256-gcm");
                assert_eq!(password, "pw");
            }
            other => panic!("unexpected protocol {other:?}"),
        }
    }

    #[test]
    fn reads_a_base64_subscription_with_several_links() {
        // Two links, base64-encoded as a whole — what a subscription URL returns.
        let plain = "trojan://a@a.example.com:443#A\ntrojan://b@b.example.com:443#B\n";
        let encoded = {
            use base64::engine::general_purpose::STANDARD;
            use base64::Engine as _;
            STANDARD.encode(plain)
        };
        let report = parse(&encoded);
        assert_eq!(report.nodes.len(), 2, "{:?}", report.skipped);
        assert_eq!(report.nodes[1].name, "B");
    }

    #[test]
    fn unknown_lines_are_reported_not_fatal() {
        let report = parse("trojan://a@a.example.com:443#A\nhttp://not-a-proxy\n");
        assert_eq!(report.nodes.len(), 1);
        assert_eq!(report.skipped.len(), 1);
        assert!(report.skipped[0].contains("不支持的链接类型"));
    }

    #[test]
    fn an_ipv6_host_keeps_its_brackets_off_the_server_field() {
        let report = parse("trojan://pw@[2001:db8::1]:443#v6");
        let node = &report.nodes[0];
        assert_eq!(node.server, "2001:db8::1");
        assert_eq!(node.port, 443);
    }

    #[test]
    fn a_link_without_a_port_is_skipped_with_a_reason() {
        let report = parse("trojan://pw@example.com#no-port");
        assert!(report.nodes.is_empty());
        assert!(report.skipped[0].contains("端口"));
    }
}
