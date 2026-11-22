//! Whole-document subscriptions.
//!
//! Three shapes show up in the wild: a base64 blob of share links (most 机场,
//! handled by [`crate::import`]), a Clash config with `proxies:` (YAML), and a
//! sing-box / Xray config with `outbounds:` (JSON). All three end in [`Node`]s.
//! Nothing here touches the network or the disk.

use serde_json::Value as Json;
use serde_yaml::Value as Yaml;

use crate::domain::node::{Node, Protocol, Security, Transport};
use crate::import::{unique_suffix, ParseReport};

/// Parse whatever a subscription URL handed back.
pub fn parse_document(text: &str) -> ParseReport {
    if let Some(report) = from_json_config(text) {
        return report;
    }
    if let Some(report) = from_clash_yaml(text) {
        return report;
    }
    crate::import::parse(text)
}

fn node(id_prefix: &str, name: String, server: String, port: u16, protocol: Protocol,
        transport: Transport, security: Security) -> Node {
    Node {
        id: format!("{id_prefix}-{}", unique_suffix()),
        name,
        server,
        port,
        protocol,
        transport,
        security,
        tags: vec![],
    }
}

// ------------------------------------------------------------ sing-box / Xray

fn from_json_config(text: &str) -> Option<ParseReport> {
    let value: Json = serde_json::from_str(text).ok()?;
    let outbounds = value.get("outbounds")?.as_array()?;
    if outbounds.is_empty() {
        return None;
    }
    let mut report = ParseReport::default();
    for outbound in outbounds {
        match node_from_outbound(outbound) {
            Some(node) => report.nodes.push(node),
            None => {
                let label = outbound.get("tag").and_then(Json::as_str).unwrap_or("未命名");
                report.skipped.push(format!("outbound「{label}」不是可识别的节点"));
            }
        }
    }
    Some(report)
}

fn node_from_outbound(outbound: &Json) -> Option<Node> {
    // sing-box spells it `type`, Xray spells it `protocol`.
    let kind = outbound.get("type").or_else(|| outbound.get("protocol"))?.as_str()?;
    let (server, port, protocol) = match kind {
        "vless" | "vmess" => v_outbound(outbound, kind)?,
        "trojan" | "shadowsocks" | "ss" => password_outbound(outbound, kind)?,
        "hysteria2" | "tuic" => quic_outbound(outbound, kind)?,
        _ => return None, // direct / block / selector are structural
    };
    let (transport, security) = match outbound.get("streamSettings") {
        Some(stream) => xray_stream(stream),
        None => singbox_stream(outbound),
    };
    let name = outbound
        .get("tag")
        .and_then(Json::as_str)
        .filter(|tag| !tag.is_empty())
        .unwrap_or("订阅节点")
        .to_string();
    Some(node("doc", name, server, port, protocol, transport, security))
}

fn v_outbound(outbound: &Json, kind: &str) -> Option<(String, u16, Protocol)> {
    // sing-box: flat fields.
    if let (Some(server), Some(port), Some(uuid)) = (
        outbound.get("server").and_then(Json::as_str),
        outbound.get("server_port").and_then(Json::as_u64),
        outbound.get("uuid").and_then(Json::as_str),
    ) {
        let protocol = if kind == "vless" {
            Protocol::Vless {
                uuid: uuid.into(),
                flow: outbound.get("flow").and_then(Json::as_str).map(str::to_string),
            }
        } else {
            Protocol::Vmess {
                uuid: uuid.into(),
                alter_id: outbound.get("alter_id").and_then(Json::as_u64).unwrap_or(0) as u16,
            }
        };
        return Some((server.into(), port as u16, protocol));
    }
    // Xray: settings.vnext[0].{address,port,users[0].{id,flow}}.
    let vnext = outbound.get("settings")?.get("vnext")?.as_array()?.first()?;
    let server = vnext.get("address").and_then(Json::as_str)?;
    let port = vnext.get("port").and_then(Json::as_u64)? as u16;
    let user = vnext.get("users")?.as_array()?.first()?;
    let uuid = user.get("id").and_then(Json::as_str)?;
    let protocol = if kind == "vless" {
        Protocol::Vless {
            uuid: uuid.into(),
            flow: user.get("flow").and_then(Json::as_str).map(str::to_string),
        }
    } else {
        Protocol::Vmess {
            uuid: uuid.into(),
            alter_id: user.get("alterId").and_then(Json::as_u64).unwrap_or(0) as u16,
        }
    };
    Some((server.into(), port, protocol))
}

fn password_outbound(outbound: &Json, kind: &str) -> Option<(String, u16, Protocol)> {
    let build = |password: &str, method: Option<&str>| {
        if kind == "trojan" {
            Protocol::Trojan { password: password.into() }
        } else {
            Protocol::Shadowsocks {
                method: method.unwrap_or("aes-256-gcm").into(),
                password: password.into(),
            }
        }
    };
    if let (Some(server), Some(port), Some(password)) = (
        outbound.get("server").and_then(Json::as_str),
        outbound.get("server_port").and_then(Json::as_u64),
        outbound.get("password").and_then(Json::as_str),
    ) {
        let method = outbound.get("method").and_then(Json::as_str);
        return Some((server.into(), port as u16, build(password, method)));
    }
    let entry = outbound.get("settings")?.get("servers")?.as_array()?.first()?;
    let server = entry.get("address").and_then(Json::as_str)?;
    let port = entry.get("port").and_then(Json::as_u64)? as u16;
    let password = entry.get("password").and_then(Json::as_str)?;
    let method = entry.get("method").and_then(Json::as_str);
    Some((server.into(), port, build(password, method)))
}

/// sing-box's QUIC outbounds. Both keep `password` at the top level, so the
/// only protocol-specific parts are the obfuscation (hysteria2) and the
/// congestion knobs (TUIC).
fn quic_outbound(outbound: &Json, kind: &str) -> Option<(String, u16, Protocol)> {
    let server = outbound.get("server").and_then(Json::as_str)?;
    let port = outbound.get("server_port").and_then(Json::as_u64)? as u16;
    let password = outbound.get("password").and_then(Json::as_str)?;
    let protocol = if kind == "hysteria2" {
        Protocol::Hysteria2 {
            password: password.into(),
            obfs_password: outbound
                .get("obfs")
                .and_then(|obfs| obfs.get("password"))
                .and_then(Json::as_str)
                .map(str::to_string),
            up_mbps: outbound
                .get("up_mbps")
                .and_then(Json::as_u64)
                .map(|value| value as u32),
            down_mbps: outbound
                .get("down_mbps")
                .and_then(Json::as_u64)
                .map(|value| value as u32),
        }
    } else {
        Protocol::Tuic {
            uuid: outbound.get("uuid").and_then(Json::as_str)?.into(),
            password: password.into(),
            congestion_control: outbound
                .get("congestion_control")
                .and_then(Json::as_str)
                .map(str::to_string),
            udp_relay_mode: outbound
                .get("udp_relay_mode")
                .and_then(Json::as_str)
                .map(str::to_string),
        }
    };
    Some((server.into(), port, protocol))
}

fn xray_stream(stream: &Json) -> (Transport, Security) {
    let transport = match stream.get("network").and_then(Json::as_str).unwrap_or("tcp") {
        "ws" => {
            let ws = stream.get("wsSettings");
            Transport::Ws {
                path: ws.and_then(|ws| ws.get("path")).and_then(Json::as_str).unwrap_or("/").into(),
                host: ws.and_then(|ws| ws.get("headers"))
                    .and_then(|headers| headers.get("Host"))
                    .and_then(Json::as_str)
                    .map(str::to_string),
            }
        }
        "grpc" => Transport::Grpc {
            service_name: stream.get("grpcSettings")
                .and_then(|grpc| grpc.get("serviceName"))
                .and_then(Json::as_str)
                .unwrap_or("")
                .into(),
        },
        _ => Transport::Tcp,
    };

    let security = match stream.get("security").and_then(Json::as_str).unwrap_or("none") {
        "tls" => Security::Tls {
            sni: stream.get("tlsSettings")
                .and_then(|tls| tls.get("serverName"))
                .and_then(Json::as_str)
                .map(str::to_string),
            alpn: vec![],
            allow_insecure: stream
                .get("tlsSettings")
                .and_then(|tls| tls.get("allowInsecure"))
                .and_then(Json::as_bool)
                .unwrap_or(false),
            pinned_cert_sha256: stream
                .get("tlsSettings")
                .and_then(|tls| tls.get("pinnedPeerCertSha256"))
                .and_then(Json::as_array)
                .map(|pins| {
                    pins.iter()
                        .filter_map(Json::as_str)
                        .map(|pin| pin.trim().replace(|c| c == ':' || c == ' ', "").to_lowercase())
                        .filter(|pin| !pin.is_empty())
                        .collect()
                })
                .unwrap_or_default(),
        },
        "reality" => {
            let reality = stream.get("realitySettings");
            Security::Reality {
                public_key: reality.and_then(|r| r.get("publicKey")).and_then(Json::as_str).unwrap_or("").into(),
                short_id: reality.and_then(|r| r.get("shortId")).and_then(Json::as_str).unwrap_or("").into(),
                server_name: reality.and_then(|r| r.get("serverName")).and_then(Json::as_str).unwrap_or("").into(),
                fingerprint: reality
                    .and_then(|r| r.get("fingerprint"))
                    .and_then(Json::as_str)
                    .filter(|value| !value.is_empty())
                    .unwrap_or("chrome")
                    .into(),
                spider_x: reality
                    .and_then(|r| r.get("spiderX"))
                    .and_then(Json::as_str)
                    .filter(|value| !value.is_empty())
                    .unwrap_or("/")
                    .into(),
            }
        }
        _ => Security::None,
    };
    (transport, security)
}

fn singbox_stream(outbound: &Json) -> (Transport, Security) {
    let transport = match outbound.get("transport") {
        Some(t) => match t.get("type").and_then(Json::as_str).unwrap_or("tcp") {
            "ws" => Transport::Ws {
                path: t.get("path").and_then(Json::as_str).unwrap_or("/").into(),
                host: t.get("headers").and_then(|h| h.get("Host")).and_then(Json::as_str).map(str::to_string),
            },
            "grpc" => Transport::Grpc {
                service_name: t.get("service_name").and_then(Json::as_str).unwrap_or("").into(),
            },
            _ => Transport::Tcp,
        },
        None => Transport::Tcp,
    };

    let tls = outbound.get("tls");
    let enabled = tls.and_then(|t| t.get("enabled")).and_then(Json::as_bool).unwrap_or(false);
    let security = if !enabled {
        Security::None
    } else if let Some(reality) = tls.and_then(|t| t.get("reality")) {
        Security::Reality {
            public_key: reality.get("public_key").and_then(Json::as_str).unwrap_or("").into(),
            short_id: reality.get("short_id").and_then(Json::as_str).unwrap_or("").into(),
            server_name: tls.and_then(|t| t.get("server_name")).and_then(Json::as_str).unwrap_or("").into(),
            fingerprint: tls
                .and_then(|t| t.get("utls"))
                .and_then(|u| u.get("fingerprint"))
                .and_then(Json::as_str)
                .filter(|value| !value.is_empty())
                .unwrap_or("chrome")
                .into(),
            spider_x: "/".into(),
        }
    } else {
        Security::Tls {
            sni: tls.and_then(|t| t.get("server_name")).and_then(Json::as_str).map(str::to_string),
            alpn: vec![],
            allow_insecure: tls
                .and_then(|t| t.get("insecure"))
                .and_then(Json::as_bool)
                .unwrap_or(false),
            pinned_cert_sha256: vec![],
        }
    };
    (transport, security)
}

// ------------------------------------------------------------------- Clash

fn from_clash_yaml(text: &str) -> Option<ParseReport> {
    let document: Yaml = serde_yaml::from_str(text).ok()?;
    let proxies = document.get("proxies")?.as_sequence()?;
    if proxies.is_empty() {
        return None;
    }
    let mut report = ParseReport::default();
    for proxy in proxies {
        match node_from_clash(proxy) {
            Some(node) => report.nodes.push(node),
            None => {
                let name = yaml_str(proxy, "name").unwrap_or_else(|| "未命名 proxy".into());
                report.skipped.push(format!("proxy「{name}」不是可识别的节点"));
            }
        }
    }
    Some(report)
}

fn yaml_str(value: &Yaml, key: &str) -> Option<String> {
    value.get(key).and_then(Yaml::as_str).map(str::to_string)
}

fn yaml_u64(value: &Yaml, key: &str) -> Option<u64> {
    value.get(key).and_then(Yaml::as_u64)
}

fn yaml_bool(value: &Yaml, key: &str) -> bool {
    value.get(key).and_then(Yaml::as_bool).unwrap_or(false)
}

fn node_from_clash(proxy: &Yaml) -> Option<Node> {
    let kind = yaml_str(proxy, "type")?;
    let server = yaml_str(proxy, "server").filter(|s| !s.is_empty())?;
    let port = yaml_u64(proxy, "port")? as u16;

    let protocol = match kind.as_str() {
        "vless" => Protocol::Vless { uuid: yaml_str(proxy, "uuid")?, flow: yaml_str(proxy, "flow") },
        "vmess" => Protocol::Vmess {
            uuid: yaml_str(proxy, "uuid")?,
            alter_id: yaml_u64(proxy, "alterId").unwrap_or(0) as u16,
        },
        "trojan" => Protocol::Trojan { password: yaml_str(proxy, "password")? },
        "ss" | "shadowsocks" => Protocol::Shadowsocks {
            method: yaml_str(proxy, "cipher").unwrap_or_else(|| "aes-256-gcm".into()),
            password: yaml_str(proxy, "password")?,
        },
        "hysteria2" | "hy2" => Protocol::Hysteria2 {
            password: yaml_str(proxy, "password")?,
            // Clash only obfuscates when `obfs: salamander` is present; an
            // `obfs-password` on its own means nothing.
            obfs_password: yaml_str(proxy, "obfs")
                .filter(|obfs| obfs.eq_ignore_ascii_case("salamander"))
                .and_then(|_| yaml_str(proxy, "obfs-password")),
            up_mbps: yaml_u64(proxy, "up").map(|value| value as u32),
            down_mbps: yaml_u64(proxy, "down").map(|value| value as u32),
        },
        "tuic" => Protocol::Tuic {
            uuid: yaml_str(proxy, "uuid")?,
            password: yaml_str(proxy, "password")?,
            congestion_control: yaml_str(proxy, "congestion-controller"),
            udp_relay_mode: yaml_str(proxy, "udp-relay-mode"),
        },
        _ => return None,
    };

    let transport = match yaml_str(proxy, "network").unwrap_or_else(|| "tcp".into()).as_str() {
        "ws" => Transport::Ws {
            path: proxy.get("ws-opts").and_then(|ws| yaml_str(ws, "path")).unwrap_or_else(|| "/".into()),
            host: proxy.get("ws-opts").and_then(|ws| ws.get("headers")).and_then(|h| yaml_str(h, "Host")),
        },
        "grpc" => Transport::Grpc {
            service_name: proxy.get("grpc-opts")
                .and_then(|grpc| yaml_str(grpc, "grpc-service-name"))
                .unwrap_or_default(),
        },
        _ => Transport::Tcp,
    };

    let security = if let Some(reality) = proxy.get("reality-opts") {
        Security::Reality {
            public_key: yaml_str(reality, "public-key").unwrap_or_default(),
            short_id: yaml_str(reality, "short-id").unwrap_or_default(),
            server_name: yaml_str(proxy, "servername").or_else(|| yaml_str(proxy, "sni")).unwrap_or_default(),
            fingerprint: yaml_str(proxy, "client-fingerprint").unwrap_or_else(|| "chrome".to_string()),
            spider_x: "/".to_string(),
        }
    } else if yaml_bool(proxy, "tls") {
        Security::Tls {
            sni: yaml_str(proxy, "servername").or_else(|| yaml_str(proxy, "sni")),
            alpn: vec![],
            allow_insecure: yaml_bool(proxy, "skip-cert-verify"),
            // Clash's `fingerprint` is the certificate SHA-256, i.e. pinning.
            pinned_cert_sha256: yaml_str(proxy, "fingerprint")
                .map(|pin| {
                    pin.trim()
                        .replace(|c| c == ':' || c == ' ', "")
                        .to_lowercase()
                })
                .filter(|pin| !pin.is_empty())
                .into_iter()
                .collect(),
        }
    } else {
        Security::None
    };

    let name = yaml_str(proxy, "name").unwrap_or_else(|| format!("{server}:{port}"));
    Some(node("clash", name, server, port, protocol, transport, security))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLASH: &str = r#"
proxies:
  - name: "香港 01"
    type: vless
    server: hk.example.com
    port: 443
    uuid: 11111111-2222-3333-4444-555555555555
    flow: xtls-rprx-vision
    tls: true
    servername: example.com
    network: ws
    ws-opts:
      path: /ray
      headers:
        Host: cdn.example.com
  - name: "ss 02"
    type: ss
    server: 1.2.3.4
    port: 8388
    cipher: chacha20-ietf-poly1305
    password: secret
  - name: "坏节点"
    type: ss
    server: ""
    port: 1
"#;

    #[test]
    fn a_clash_document_becomes_nodes() {
        let report = parse_document(CLASH);
        assert_eq!(report.nodes.len(), 2, "skipped: {:?}", report.skipped);
        let first = &report.nodes[0];
        assert_eq!(first.name, "香港 01");
        assert_eq!(first.server, "hk.example.com");
        assert_eq!(first.port, 443);
        assert!(matches!(first.transport, Transport::Ws { .. }));
        assert!(matches!(first.security, Security::Tls { .. }));
        assert!(matches!(first.protocol, Protocol::Vless { .. }));
    }

    #[test]
    fn a_clash_ss_proxy_keeps_its_cipher() {
        let report = parse_document(CLASH);
        match &report.nodes[1].protocol {
            Protocol::Shadowsocks { method, password } => {
                assert_eq!(method, "chacha20-ietf-poly1305");
                assert_eq!(password, "secret");
            }
            other => panic!("expected shadowsocks, got {other:?}"),
        }
    }

    #[test]
    fn a_singbox_document_becomes_nodes() {
        let text = r#"{
          "outbounds": [
            { "type": "vless", "tag": "东京", "server": "jp.example.com", "server_port": 443,
              "uuid": "abc", "flow": "xtls-rprx-vision",
              "tls": { "enabled": true, "server_name": "example.com",
                       "reality": { "enabled": true, "public_key": "PK", "short_id": "01" } },
              "transport": { "type": "grpc", "service_name": "grpc-svc" } },
            { "type": "direct", "tag": "direct" }
          ]
        }"#;
        let report = parse_document(text);
        assert_eq!(report.nodes.len(), 1, "skipped: {:?}", report.skipped);
        let node = &report.nodes[0];
        assert_eq!(node.name, "东京");
        assert!(matches!(node.security, Security::Reality { .. }));
        assert!(matches!(node.transport, Transport::Grpc { .. }));
        assert!(report.skipped.iter().any(|line| line.contains("direct")));
    }

    #[test]
    fn an_xray_document_is_understood_too() {
        let text = r#"{
          "outbounds": [
            { "protocol": "vmess", "tag": "老节点",
              "settings": { "vnext": [{ "address": "old.example.com", "port": 10086,
                "users": [{ "id": "uuid-x", "alterId": 4 }] }] },
              "streamSettings": { "network": "tcp", "security": "none" } }
          ]
        }"#;
        let report = parse_document(text);
        assert_eq!(report.nodes.len(), 1);
        assert_eq!(report.nodes[0].server, "old.example.com");
        assert_eq!(report.nodes[0].port, 10086);
        assert!(matches!(report.nodes[0].protocol, Protocol::Vmess { alter_id: 4, .. }));
    }

    #[test]
    fn a_share_link_list_falls_through_to_the_link_parser() {
        let report = parse_document("trojan://pass@example.com:443#节点");
        assert_eq!(report.nodes.len(), 1);
        assert_eq!(report.nodes[0].name, "节点");
    }
}
