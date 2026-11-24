//! Xray-core renderer.
//!
//! Mirrors the structure v2rayN generates (see `docs/ENGINE-SPEC.md` §1), but
//! writes Xray's *native* outbound shapes (`settings.vnext[]` /
//! `settings.servers[]`) directly instead of reproducing v2rayN's flat
//! intermediate model.
//!
//! Statistics use `metrics` + `policy.system.statsOutbound*`, i.e. the expvar
//! endpoint at `GET /debug/vars`. Xray is *not* driven over gRPC here.

use serde_json::{json, Value};

use crate::domain::node::{Node, Protocol};
use crate::error::AppResult;

use super::render::{xray_routing_rules, xray_stream_settings};
use super::{ensure_node_usable, Endpoints, Engine, EngineKind, RenderInput, StatsChannel};

pub struct XrayEngine;

impl Engine for XrayEngine {
    fn kind(&self) -> EngineKind {
        EngineKind::Xray
    }

    fn binary_name(&self) -> &'static str {
        "xray.exe"
    }

    fn args(&self, config_path: &str) -> Vec<String> {
        vec!["run".into(), "-c".into(), config_path.into()]
    }

    fn stats_channel(&self, endpoints: Endpoints) -> StatsChannel {
        StatsChannel::XrayExpVar {
            port: endpoints.api_port,
        }
    }

    fn validate_node(&self, node: &Node) -> AppResult<()> {
        ensure_node_usable(node)?;
        // Xray has no hysteria2 or TUIC outbound at all. Saying so here beats
        // rendering a config that quietly leaves the node out, which would
        // look like a working tunnel that sends everything direct.
        if node.protocol.is_quic() {
            return Err(crate::error::AppError::InvalidInput(format!(
                "Xray 内核不支持 {} 协议，请在首页把内核切换为 sing-box 后再启动",
                node.protocol.label()
            )));
        }
        // Xray refuses an empty Shadowsocks method outright; catching it here
        // gives the UI a field-level error instead of a kernel stderr dump.
        if let Protocol::Shadowsocks { method, .. } = &node.protocol {
            if method.trim().is_empty() {
                return Err(crate::error::AppError::InvalidInput(
                    "Shadowsocks 加密方式不能为空".to_string(),
                ));
            }
        }
        Ok(())
    }

    fn render_config(&self, input: &RenderInput<'_>) -> AppResult<Value> {
        self.validate_node(input.node)?;
        let endpoints = input.endpoints;
        let outbounds = xray_outbounds(input)?;

        Ok(json!({
            "log": {
                // Log files are a privacy liability by default; the app reads
                // stdout instead. Opt-in logging comes with a settings toggle.
                "access": null,
                "error": null,
                "loglevel": input.log_level.as_xray(),
            },
            "inbounds": [{
                "tag": "socks",
                "port": endpoints.mixed_port,
                "protocol": "mixed",
                "listen": input.listen_address(),
                "settings": {
                    "auth": "noauth",
                    "udp": true,
                    "allowTransparent": false,
                },
                "sniffing": {
                    "enabled": true,
                    "destOverride": ["http", "tls"],
                    "routeOnly": false,
                },
            }],
            "outbounds": outbounds,
            "routing": {
                "domainStrategy": "IPIfNonMatch",
                "rules": xray_routing_rules(input),
            },
            // Custom resolvers, when the user asked for them. Xray accepts bare
            // IPs, `tls://` and `https://` spellings; the domain form is
            // refused in settings, because a hostname here would have to be
            // resolved by the resolver itself.
            "dns": if input.dns_servers.is_empty() {
                Value::Null
            } else {
                json!({
                    "servers": input
                        .dns_servers
                        .iter()
                        .map(|entry| entry.xray_address())
                        .collect::<Vec<_>>()
                })
            },
            "stats": {},
            "metrics": {
                "listen": format!("127.0.0.1:{}", endpoints.api_port),
            },
            "policy": {
                "system": {
                    "statsOutboundDownlink": true,
                    "statsOutboundUplink": true,
                },
            },
        }))
    }
}

/// All outbounds for this run.
///
/// Xray has no selector outbound, so a proxy group becomes a *static* copy of
/// its chosen member under the group's tag. That is the honest mapping: an
/// Xray `urltest` group behaves like `select` pinned to the first member, and
/// the UI says so.
fn xray_outbounds(input: &RenderInput<'_>) -> AppResult<Vec<Value>> {
    let mux = input.mux_streams();
    // One decision, taken by `RenderInput::fragment`, shared by the active node
    // and every group copy: two call sites testing the switch themselves could
    // drift apart.
    let fragment = input.fragment();
    let fragment = fragment
        .as_ref()
        .map(|(length, interval)| (length.as_str(), interval.as_str()));
    let mut outbounds = vec![xray_outbound(input.node, "proxy", mux, fragment)?];
    for group in input.active_groups() {
        // A group whose nodes were all deleted is skipped rather than emitted
        // pointing at nothing — the kernel would refuse the whole config.
        if let Some(node) = input.member_node(group) {
            outbounds.push(xray_outbound(node, &group.tag(), mux, fragment)?);
        }
    }
    outbounds.push(json!({ "protocol": "freedom", "tag": "direct" }));
    outbounds.push(json!({ "protocol": "blackhole", "tag": "block" }));
    Ok(outbounds)
}

/// One proxy outbound. `tag` is `proxy` for the active node and `group:<id>`
/// for a group's copy of a member.
///
/// `mux_streams` is `Some(n)` only when the user turned multiplexing on; Xray
/// treats a missing `mux` block and `"mux": {"enabled": false}` differently
/// enough in the logs that emitting nothing is the honest "off".
///
/// `fragment` is the TLS fragmentation range, or `None` when it is off; it is
/// only forwarded to `streamSettings`, never to the `mux` block.
fn xray_outbound(
    node: &Node,
    tag: &str,
    mux_streams: Option<u16>,
    fragment: Option<(&str, &str)>,
) -> AppResult<Value> {
    let (protocol, settings) = match &node.protocol {
        Protocol::Vless { uuid, flow } => {
            let mut user = json!({ "id": uuid, "encryption": "none" });
            if let Some(flow) = flow.as_deref().filter(|f| !f.is_empty()) {
                user["flow"] = json!(flow);
            }
            (
                "vless",
                json!({
                    "vnext": [{
                        "address": node.server,
                        "port": node.port,
                        "users": [user],
                    }],
                }),
            )
        }
        Protocol::Vmess { uuid, alter_id } => (
            "vmess",
            json!({
                "vnext": [{
                    "address": node.server,
                    "port": node.port,
                    "users": [{
                        "id": uuid,
                        "alterId": alter_id,
                        "security": "auto",
                    }],
                }],
            }),
        ),
        Protocol::Trojan { password } => (
            "trojan",
            json!({
                "servers": [{
                    "address": node.server,
                    "port": node.port,
                    "password": password,
                }],
            }),
        ),
        Protocol::Shadowsocks { method, password } => (
            "shadowsocks",
            json!({
                "servers": [{
                    "address": node.server,
                    "port": node.port,
                    "method": method,
                    "password": password,
                }],
            }),
        ),
        // Unreachable: `validate_node` rejects QUIC protocols before any
        // outbound is built. The arm exists so the match stays exhaustive
        // and a future protocol cannot slip through unnoticed.
        Protocol::Hysteria2 { .. } | Protocol::Tuic { .. } => {
            return Err(crate::error::AppError::InvalidInput(format!(
                "Xray 内核不支持 {} 协议",
                node.protocol.label()
            )))
        }
    };

    let mut outbound = json!({
        "tag": tag,
        "protocol": protocol,
        "settings": settings,
        "streamSettings": xray_stream_settings(&node.transport, &node.security, fragment),
    });
    // `mux` sits beside `settings`, not inside it: Xray's outbound schema puts
    // the multiplexing policy at the same level as the protocol payload.
    if let Some(streams) = mux_streams {
        outbound["mux"] = json!({ "enabled": true, "concurrency": streams });
    }
    Ok(outbound)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::node::{Security, Transport};
    use crate::domain::settings::ProxyMode;
    use crate::engine::RunOptions;

    fn node(protocol: Protocol, transport: Transport, security: Security) -> Node {
        Node {
            id: "n1".into(),
            name: "测试节点".into(),
            server: "example.com".into(),
            port: 443,
            protocol,
            transport,
            security,
            tags: vec![],
        }
    }

    fn render(node: &Node) -> Value {
        let endpoints = Endpoints::new(7890, 9090).unwrap();
        XrayEngine.render_config(&RenderInput::new(node, endpoints)).unwrap()
    }

    fn plain_node() -> Node {
        node(
            Protocol::Vless {
                uuid: "uuid-1".into(),
                flow: None,
            },
            Transport::Tcp,
            Security::None,
        )
    }

    fn render_with(node: &Node, mode: ProxyMode, allow_lan: bool) -> Value {
        let endpoints = Endpoints::new(7890, 9090).unwrap();
        let input = RenderInput::new(node, endpoints).with_options(RunOptions {
            mode,
            allow_lan,
            ..RunOptions::new()
        });
        XrayEngine.render_config(&input).unwrap()
    }

    #[test]
    fn global_mode_ignores_the_rule_table() {
        let cfg = render_with(&plain_node(), ProxyMode::Global, false);
        assert_eq!(cfg["routing"]["rules"][0]["outboundTag"], json!("proxy"));
    }

    #[test]
    fn direct_mode_ignores_the_rule_table() {
        let cfg = render_with(&plain_node(), ProxyMode::Direct, false);
        assert_eq!(cfg["routing"]["rules"][0]["outboundTag"], json!("direct"));
    }

    #[test]
    fn lan_sharing_moves_the_inbound_off_loopback() {
        let shared = render_with(&plain_node(), ProxyMode::Rule, true);
        assert_eq!(shared["inbounds"][0]["listen"], json!("0.0.0.0"));
        let local = render_with(&plain_node(), ProxyMode::Rule, false);
        assert_eq!(local["inbounds"][0]["listen"], json!("127.0.0.1"));
    }

    #[test]
    fn vless_ws_tls_renders_native_vnext_shape() {
        let n = node(
            Protocol::Vless {
                uuid: "uuid-1".into(),
                flow: None,
            },
            Transport::Ws {
                path: "/ray".into(),
                host: Some("cdn.example.com".into()),
            },
            Security::Tls {
                sni: Some("sni.example.com".into()),
                alpn: vec!["h2".into(), "http/1.1".into()],
                allow_insecure: false,
                pinned_cert_sha256: vec![],
            },
        );
        let cfg = render(&n);
        let out = &cfg["outbounds"][0];
        assert_eq!(out["tag"], json!("proxy"));
        assert_eq!(out["protocol"], json!("vless"));
        assert_eq!(out["settings"]["vnext"][0]["address"], json!("example.com"));
        assert_eq!(out["settings"]["vnext"][0]["users"][0]["id"], json!("uuid-1"));
        // Xray rejects a vless user without an explicit encryption value.
        assert_eq!(
            out["settings"]["vnext"][0]["users"][0]["encryption"],
            json!("none")
        );
        assert_eq!(out["streamSettings"]["wsSettings"]["path"], json!("/ray"));
        assert_eq!(out["streamSettings"]["tlsSettings"]["alpn"][0], json!("h2"));
    }

    #[test]
    fn vless_flow_is_written_only_when_present() {
        let with_flow = node(
            Protocol::Vless {
                uuid: "u".into(),
                flow: Some("xtls-rprx-vision".into()),
            },
            Transport::Tcp,
            Security::None,
        );
        let cfg = render(&with_flow);
        assert_eq!(
            cfg["outbounds"][0]["settings"]["vnext"][0]["users"][0]["flow"],
            json!("xtls-rprx-vision")
        );

        let no_flow = node(
            Protocol::Vless {
                uuid: "u".into(),
                flow: Some(String::new()),
            },
            Transport::Tcp,
            Security::None,
        );
        let cfg = render(&no_flow);
        assert!(
            cfg["outbounds"][0]["settings"]["vnext"][0]["users"][0]
                .get("flow")
                .is_none(),
            "an empty flow must not be emitted"
        );
    }

    #[test]
    fn trojan_and_shadowsocks_use_the_servers_array() {
        let trojan = render(&node(
            Protocol::Trojan {
                password: "pw".into(),
            },
            Transport::Grpc {
                service_name: "svc".into(),
            },
            Security::Tls {
                sni: None,
                alpn: vec![],
                allow_insecure: false,
                pinned_cert_sha256: vec![],
            },
        ));
        assert_eq!(trojan["outbounds"][0]["protocol"], json!("trojan"));
        assert_eq!(trojan["outbounds"][0]["settings"]["servers"][0]["password"], json!("pw"));

        let ss = render(&node(
            Protocol::Shadowsocks {
                method: "aes-256-gcm".into(),
                password: "pw".into(),
            },
            Transport::Tcp,
            Security::None,
        ));
        assert_eq!(ss["outbounds"][0]["protocol"], json!("shadowsocks"));
        assert_eq!(
            ss["outbounds"][0]["settings"]["servers"][0]["method"],
            json!("aes-256-gcm")
        );
    }

    #[test]
    fn direct_and_block_outbounds_are_preserved() {
        let cfg = render(&node(
            Protocol::Trojan {
                password: "pw".into(),
            },
            Transport::Tcp,
            Security::None,
        ));
        let tags: Vec<&str> = cfg["outbounds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| o["tag"].as_str().unwrap())
            .collect();
        assert_eq!(tags, vec!["proxy", "direct", "block"]);
    }

    #[test]
    fn stats_wiring_points_at_the_api_port() {
        let cfg = render(&node(
            Protocol::Trojan {
                password: "pw".into(),
            },
            Transport::Tcp,
            Security::None,
        ));
        assert_eq!(cfg["metrics"]["listen"], json!("127.0.0.1:9090"));
        assert_eq!(
            cfg["policy"]["system"]["statsOutboundUplink"],
            json!(true)
        );
        // `stats` must be an empty object, not null/absent.
        assert_eq!(cfg["stats"], json!({}));
    }

    #[test]
    fn mixed_inbound_listens_on_the_planned_port() {
        let cfg = render(&node(
            Protocol::Trojan {
                password: "pw".into(),
            },
            Transport::Tcp,
            Security::None,
        ));
        assert_eq!(cfg["inbounds"][0]["protocol"], json!("mixed"));
        assert_eq!(cfg["inbounds"][0]["port"], json!(7890));
        assert_eq!(cfg["inbounds"][0]["listen"], json!("127.0.0.1"));
    }

    #[test]
    fn empty_server_is_rejected_before_rendering() {
        let mut n = node(
            Protocol::Trojan {
                password: "pw".into(),
            },
            Transport::Tcp,
            Security::None,
        );
        n.server = "  ".into();
        let endpoints = Endpoints::new(7890, 9090).unwrap();
        assert!(XrayEngine.render_config(&RenderInput::new(&n, endpoints)).is_err());
    }

    #[test]
    fn args_run_the_config_in_the_foreground() {
        assert_eq!(
            XrayEngine.args("C:/cfg/config.json"),
            vec!["run", "-c", "C:/cfg/config.json"]
        );
    }

    #[test]
    fn stats_channel_is_expvar_on_the_api_port() {
        let endpoints = Endpoints::new(7890, 9090).unwrap();
        assert_eq!(
            XrayEngine.stats_channel(endpoints),
            StatsChannel::XrayExpVar { port: 9090 }
        );
    }

    fn two_nodes() -> (Node, Node) {
        let first = Node {
            id: "n1".into(),
            name: "一号".into(),
            server: "one.example.com".into(),
            port: 443,
            protocol: Protocol::Trojan { password: "pw".into() },
            transport: Transport::Tcp,
            security: Security::None,
            tags: vec![],
        };
        let mut second = first.clone();
        second.id = "n2".into();
        second.name = "二号".into();
        second.server = "two.example.com".into();
        (first, second)
    }

    #[test]
    fn the_persisted_log_level_reaches_the_kernel() {
        // A debug setting that never reaches the config is a switch that lies,
        // so this walks the whole path: settings → RunOptions → renderer.
        let (node, _) = two_nodes();
        let settings = crate::domain::Settings {
            log_level: crate::domain::settings::LogLevel::Debug,
            ..Default::default()
        };
        let options = crate::engine::RunOptions::from_settings(&settings, &[]);
        let input = crate::engine::RenderInput::new(&node, Endpoints::new(17890, 19090).unwrap())
            .with_options(options);
        let config = XrayEngine.render_config(&input).unwrap();
        assert_eq!(config["log"]["loglevel"], "debug");
    }

    #[test]
    fn a_group_becomes_a_static_copy_of_its_chosen_member() {
        let (first, second) = two_nodes();
        let group = crate::domain::group::ProxyGroup {
            id: "g1".into(),
            name: "自动选择".into(),
            enabled: true,
            kind: crate::domain::group::GroupKind::Select,
            members: vec!["n1".into(), "n2".into()],
            selected: Some("n2".into()),
        };
        let nodes = vec![first.clone(), second.clone()];
        let groups = vec![group];
        let endpoints = Endpoints::new(7890, 9090).unwrap();
        let options = RunOptions::new().with_catalog(&nodes, &groups);
        let input = RenderInput::new(&first, endpoints).with_options(options);

        let cfg = XrayEngine.render_config(&input).unwrap();
        let outbounds = cfg["outbounds"].as_array().unwrap();
        let tags: Vec<&str> = outbounds
            .iter()
            .map(|outbound| outbound["tag"].as_str().unwrap())
            .collect();
        assert_eq!(tags, vec!["proxy", "group:g1", "direct", "block"]);
        // The group copy must carry the *chosen* member's server, not the
        // active node's: that is the whole point of a group.
        assert_eq!(outbounds[1]["settings"]["servers"][0]["address"], json!("two.example.com"));
    }

    #[test]
    fn a_group_whose_members_are_gone_is_skipped() {
        let (first, _second) = two_nodes();
        let group = crate::domain::group::ProxyGroup {
            id: "g1".into(),
            name: "空组".into(),
            enabled: true,
            kind: crate::domain::group::GroupKind::Select,
            members: vec!["missing".into()],
            selected: None,
        };
        let nodes = vec![first.clone()];
        let groups = vec![group];
        let endpoints = Endpoints::new(7890, 9090).unwrap();
        let input = RenderInput::new(&first, endpoints)
            .with_options(RunOptions::new().with_catalog(&nodes, &groups));

        let cfg = XrayEngine.render_config(&input).unwrap();
        let tags: Vec<&str> = cfg["outbounds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|outbound| outbound["tag"].as_str().unwrap())
            .collect();
        assert_eq!(tags, vec!["proxy", "direct", "block"]);
    }

    #[test]
    fn custom_resolvers_land_in_the_dns_section() {
        let node = plain_node();
        let endpoints = Endpoints::new(7890, 9090).unwrap();
        let mut input = RenderInput::new(&node, endpoints);
        input.dns_servers = vec![
            crate::domain::settings::DnsEntry::Plain("1.1.1.1".into()),
            crate::domain::settings::DnsEntry::Plain("8.8.8.8".into()),
        ];
        let cfg = XrayEngine.render_config(&input).unwrap();
        assert_eq!(cfg["dns"]["servers"], json!(["1.1.1.1", "8.8.8.8"]));
    }

    #[test]
    fn encrypted_resolvers_keep_their_scheme_for_xray() {
        let node = plain_node();
        let endpoints = Endpoints::new(7890, 9090).unwrap();
        let mut input = RenderInput::new(&node, endpoints);
        input.dns_servers = vec![
            crate::domain::settings::DnsEntry::Dot {
                host: "9.9.9.9".into(),
            },
            crate::domain::settings::DnsEntry::Doh {
                host: "8.8.8.8".into(),
                path: "/dns-query".into(),
            },
        ];
        let cfg = XrayEngine.render_config(&input).unwrap();
        assert_eq!(
            cfg["dns"]["servers"],
            json!(["tls://9.9.9.9", "https://8.8.8.8/dns-query"])
        );
    }

    #[test]
    fn no_resolvers_means_no_dns_section() {
        let cfg = render(&plain_node());
        assert!(cfg["dns"].is_null());
    }

    /// Xray has no hysteria2 or TUIC outbound. Rendering a config that dropped
    /// the node would look like a working tunnel that sends everything direct,
    /// so the renderer refuses and names the fix.
    #[test]
    fn a_quic_node_is_refused_with_a_pointer_to_sing_box() {
        let endpoints = Endpoints::new(7890, 9090).unwrap();
        let node = node(
            Protocol::Hysteria2 {
                password: "pw".into(),
                obfs_password: None,
                up_mbps: None,
                down_mbps: None,
            },
            Transport::Tcp,
            Security::None,
        );
        let error = XrayEngine
            .render_config(&RenderInput::new(&node, endpoints))
            .expect_err("xray cannot carry hysteria2");
        let text = format!("{error:?}");
        assert!(text.contains("Hysteria2"), "{text}");
        assert!(text.contains("sing-box"), "{text}");
    }
}
