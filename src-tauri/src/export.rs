//! Turning a node back into a share link.
//!
//! The importer is the source of truth for the format: whatever this module
//! writes, `crate::import::parse_link` must read back into the same node. The
//! round-trip tests at the bottom hold that promise for every protocol,
//! transport and security combination the editor can produce.

use base64::Engine as _;
use serde_json::json;

use crate::domain::{Node, Protocol, Security, Transport};
use crate::error::{AppError, AppResult};

/// The share link for one node — clipboard, QR code, or "send to a friend".
///
/// vmess is the odd one out: its name travels inside the base64 JSON (`ps`),
/// not in a `#fragment`, so appending one would corrupt the payload.
pub fn link(node: &Node) -> AppResult<String> {
    Ok(match &node.protocol {
        Protocol::Vless { uuid, flow } => with_name(vless(node, uuid, flow.as_deref()), &node.name),
        Protocol::Vmess { uuid, alter_id } => vmess(node, uuid, *alter_id)?,
        Protocol::Trojan { password } => with_name(trojan(node, password), &node.name),
        Protocol::Shadowsocks { method, password } => {
            with_name(shadowsocks(node, method, password)?, &node.name)
        }
    })
}

fn with_name(link: String, name: &str) -> String {
    format!("{link}#{}", encode(name))
}

fn vless(node: &Node, uuid: &str, flow: Option<&str>) -> String {
    let mut query = vec![("encryption".to_string(), "none".to_string())];
    if let Some(flow) = flow.filter(|value| !value.trim().is_empty()) {
        query.push(("flow".to_string(), flow.to_string()));
    }
    transport_query(node, &mut query);
    security_query(node, &mut query);
    format!(
        "vless://{}@{}?{}",
        encode(uuid),
        authority(node),
        render(&query)
    )
}

fn trojan(node: &Node, password: &str) -> String {
    let mut query = Vec::new();
    transport_query(node, &mut query);
    security_query(node, &mut query);
    format!(
        "trojan://{}@{}?{}",
        encode(password),
        authority(node),
        render(&query)
    )
}

/// vmess has no REALITY: the protocol's share link has no field for it, so
/// pretending otherwise would produce a link that silently drops the security
/// layer. Refusing is the only honest answer.
fn vmess(node: &Node, uuid: &str, alter_id: u16) -> AppResult<String> {
    let mut value = json!({
        "v": "2",
        "ps": node.name,
        "add": node.server,
        "port": node.port.to_string(),
        "id": uuid,
        "aid": alter_id.to_string(),
        "scy": "auto",
        "type": "none",
        "net": "tcp",
    });
    let map = value.as_object_mut().expect("object literal");

    match &node.transport {
        Transport::Tcp => {}
        Transport::Ws { path, host } => {
            map.insert("net".into(), json!("ws"));
            map.insert(
                "path".into(),
                json!(if path.is_empty() { "/" } else { path }),
            );
            if let Some(host) = host.as_deref().filter(|value| !value.is_empty()) {
                map.insert("host".into(), json!(host));
            }
        }
        Transport::Grpc { service_name } => {
            map.insert("net".into(), json!("grpc"));
            map.insert("path".into(), json!(service_name));
        }
    }

    match &node.security {
        Security::None => {}
        Security::Tls {
            sni,
            alpn,
            allow_insecure,
            pinned_cert_sha256,
        } => {
            map.insert("tls".into(), json!("tls"));
            if let Some(sni) = sni.as_deref().filter(|value| !value.is_empty()) {
                map.insert("sni".into(), json!(sni));
            }
            if !alpn.is_empty() {
                map.insert("alpn".into(), json!(alpn.join(",")));
            }
            if *allow_insecure {
                map.insert("allowInsecure".into(), json!("1"));
            }
            if !pinned_cert_sha256.is_empty() {
                map.insert(
                    "pinnedPeerCertSha256".into(),
                    json!(pinned_cert_sha256.join(",")),
                );
            }
        }
        Security::Reality { .. } => {
            return Err(AppError::InvalidInput(
                "vmess 链接无法表达 REALITY，请改用 vless".into(),
            ))
        }
    }

    let text = serde_json::to_string(&value)
        .map_err(|error| AppError::InvalidInput(format!("vmess 内容无法序列化：{error}")))?;
    Ok(format!(
        "vmess://{}",
        base64::engine::general_purpose::STANDARD_NO_PAD.encode(text.as_bytes())
    ))
}

/// shadowsocks carries no transport and no TLS in its share link; a node that
/// has them is not an ss node any more.
fn shadowsocks(node: &Node, method: &str, password: &str) -> AppResult<String> {
    if !matches!(node.transport, Transport::Tcp) {
        return Err(AppError::InvalidInput(
            "shadowsocks 链接不支持 ws / grpc 传输".into(),
        ));
    }
    if !matches!(node.security, Security::None) {
        return Err(AppError::InvalidInput(
            "shadowsocks 链接不支持 TLS / REALITY".into(),
        ));
    }
    let credentials = base64::engine::general_purpose::STANDARD_NO_PAD
        .encode(format!("{method}:{password}").as_bytes());
    Ok(format!("ss://{}@{}", credentials, authority(node)))
}

fn transport_query(node: &Node, query: &mut Vec<(String, String)>) {
    match &node.transport {
        Transport::Tcp => query.push(("type".into(), "tcp".into())),
        Transport::Ws { path, host } => {
            query.push(("type".into(), "ws".into()));
            query.push((
                "path".into(),
                if path.is_empty() {
                    "/".into()
                } else {
                    path.clone()
                },
            ));
            if let Some(host) = host.as_deref().filter(|value| !value.is_empty()) {
                query.push(("host".into(), host.to_string()));
            }
        }
        Transport::Grpc { service_name } => {
            query.push(("type".into(), "grpc".into()));
            query.push(("serviceName".into(), service_name.clone()));
        }
    }
}

fn security_query(node: &Node, query: &mut Vec<(String, String)>) {
    match &node.security {
        Security::None => query.push(("security".into(), "none".into())),
        Security::Tls {
            sni,
            alpn,
            allow_insecure,
            pinned_cert_sha256,
        } => {
            query.push(("security".into(), "tls".into()));
            if let Some(sni) = sni.as_deref().filter(|value| !value.is_empty()) {
                query.push(("sni".into(), sni.to_string()));
            }
            if !alpn.is_empty() {
                query.push(("alpn".into(), alpn.join(",")));
            }
            if *allow_insecure {
                query.push(("allowInsecure".into(), "1".into()));
            }
            if !pinned_cert_sha256.is_empty() {
                query.push(("pcs".into(), pinned_cert_sha256.join(",")));
            }
        }
        Security::Reality {
            public_key,
            short_id,
            server_name,
            fingerprint,
            spider_x,
        } => {
            query.push(("security".into(), "reality".into()));
            query.push(("pbk".into(), public_key.clone()));
            query.push(("sid".into(), short_id.clone()));
            query.push(("sni".into(), server_name.clone()));
            query.push(("fp".into(), fingerprint.clone()));
            query.push(("spx".into(), spider_x.clone()));
        }
    }
}

fn authority(node: &Node) -> String {
    if node.server.contains(':') && !node.server.starts_with('[') {
        format!("[{}]:{}", node.server, node.port)
    } else {
        format!("{}:{}", node.server, node.port)
    }
}

fn render(query: &[(String, String)]) -> String {
    query
        .iter()
        .map(|(key, value)| format!("{key}={}", encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Percent-encode everything outside the unreserved set. `import::percent_decode`
/// is the inverse, so a name with `#`, `&` or a space survives the round trip.
fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::node::{Node, Protocol, Security, Transport};
    use crate::import::parse_link;

    fn node(protocol: Protocol, transport: Transport, security: Security) -> Node {
        Node {
            id: "n1".into(),
            name: "测试 节点 #1".into(),
            server: "example.com".into(),
            port: 443,
            protocol,
            transport,
            security,
            tags: vec![],
        }
    }

    fn reality() -> Security {
        Security::Reality {
            public_key: "PJOhHocICbFBWR9T7nV7OWzOHA6UnQ-mPFhGuPedJCU".into(),
            short_id: "00256d".into(),
            server_name: "www.sony.com".into(),
            fingerprint: "chrome".into(),
            spider_x: "/".into(),
        }
    }

    /// Every combination must survive `link` → `parse_link` unchanged. The id
    /// and the tags are ours, not the link's, so they are compared separately.
    fn round_trip(original: &Node) {
        let text = link(original).expect("link");
        let parsed = parse_link(&text, 0).expect("parse back");
        assert_eq!(parsed.name, original.name, "name: {text}");
        assert_eq!(parsed.server, original.server, "server: {text}");
        assert_eq!(parsed.port, original.port, "port: {text}");
        assert_eq!(parsed.protocol, original.protocol, "protocol: {text}");
        assert_eq!(parsed.transport, original.transport, "transport: {text}");
        assert_eq!(parsed.security, original.security, "security: {text}");
    }

    #[test]
    fn vless_over_tcp_with_reality_survives_the_round_trip() {
        round_trip(&node(
            Protocol::Vless {
                uuid: "11111111-2222-3333-4444-555555555555".into(),
                flow: Some("xtls-rprx-vision".into()),
            },
            Transport::Tcp,
            reality(),
        ));
    }

    #[test]
    fn vless_over_ws_with_tls_survives_the_round_trip() {
        round_trip(&node(
            Protocol::Vless {
                uuid: "11111111-2222-3333-4444-555555555555".into(),
                flow: None,
            },
            Transport::Ws {
                path: "/ws".into(),
                host: Some("cdn.example.com".into()),
            },
            Security::Tls {
                sni: Some("cdn.example.com".into()),
                alpn: vec!["h2".into(), "http/1.1".into()],
                allow_insecure: true,
                pinned_cert_sha256: vec![],
            },
        ));
    }

    #[test]
    fn vless_over_grpc_with_a_pinned_certificate_survives_the_round_trip() {
        round_trip(&node(
            Protocol::Vless {
                uuid: "11111111-2222-3333-4444-555555555555".into(),
                flow: None,
            },
            Transport::Grpc {
                service_name: "gun".into(),
            },
            Security::Tls {
                sni: Some("local.test".into()),
                alpn: vec![],
                allow_insecure: false,
                pinned_cert_sha256: vec![
                    "93c559a5d66054530f6831870c92dd7e85f2b1942507507cd12b685d838c04de".into(),
                ],
            },
        ));
    }

    #[test]
    fn vmess_over_ws_survives_the_round_trip() {
        round_trip(&node(
            Protocol::Vmess {
                uuid: "11111111-2222-3333-4444-555555555555".into(),
                alter_id: 4,
            },
            Transport::Ws {
                path: "/vm".into(),
                host: None,
            },
            Security::Tls {
                sni: Some("example.com".into()),
                alpn: vec![],
                allow_insecure: false,
                pinned_cert_sha256: vec![],
            },
        ));
    }

    #[test]
    fn trojan_with_a_password_that_needs_escaping_survives_the_round_trip() {
        round_trip(&node(
            Protocol::Trojan {
                password: "p@ss word#1".into(),
            },
            Transport::Tcp,
            Security::Tls {
                sni: Some("example.com".into()),
                alpn: vec![],
                allow_insecure: false,
                pinned_cert_sha256: vec![],
            },
        ));
    }

    #[test]
    fn shadowsocks_survives_the_round_trip() {
        round_trip(&node(
            Protocol::Shadowsocks {
                method: "aes-256-gcm".into(),
                password: "hunter2".into(),
            },
            Transport::Tcp,
            Security::None,
        ));
    }

    #[test]
    fn an_ipv6_server_gets_brackets() {
        let mut original = node(
            Protocol::Trojan {
                password: "secret".into(),
            },
            Transport::Tcp,
            Security::None,
        );
        original.server = "2001:db8::1".into();
        let text = link(&original).expect("link");
        assert!(text.contains("@[2001:db8::1]:443"), "{text}");
        round_trip(&original);
    }

    #[test]
    fn vmess_refuses_reality_instead_of_dropping_it() {
        let error = link(&node(
            Protocol::Vmess {
                uuid: "11111111-2222-3333-4444-555555555555".into(),
                alter_id: 0,
            },
            Transport::Tcp,
            reality(),
        ))
        .unwrap_err();
        assert!(format!("{error}").contains("REALITY"), "{error}");
    }

    #[test]
    fn shadowsocks_refuses_a_transport_it_cannot_express() {
        let error = link(&node(
            Protocol::Shadowsocks {
                method: "aes-256-gcm".into(),
                password: "hunter2".into(),
            },
            Transport::Ws {
                path: "/ws".into(),
                host: None,
            },
            Security::None,
        ))
        .unwrap_err();
        assert!(format!("{error}").contains("ws"), "{error}");
    }
}
