//! sing-box renderer.
//!
//! Mirrors `docs/ENGINE-SPEC.md` §2. Two structural differences from Xray that
//! the renderer must respect:
//!
//! * the baseline outbounds are `proxy` + `direct` only — there is no
//!   `blackhole`, blocking is expressed with a route rule action;
//! * transport and TLS are flat objects on the outbound, not nested under
//!   `streamSettings`.
//!
//! Statistics come from the Clash API (`experimental.clash_api`), whose
//! `/traffic` websocket reports *rates*, unlike Xray's cumulative counters.

use std::collections::HashSet;

use serde_json::{json, Map, Value};

use crate::domain::group::GroupKind;
use crate::domain::node::{Node, Protocol, Transport};
use crate::error::{AppError, AppResult};

use super::render::{
    singbox_routing_rules, singbox_rule_set_declarations, singbox_tls, singbox_transport,
};
use super::{ensure_node_usable, Endpoints, Engine, EngineKind, RenderInput, StatsChannel};

pub struct SingBoxEngine;

impl Engine for SingBoxEngine {
    fn kind(&self) -> EngineKind {
        EngineKind::SingBox
    }

    fn binary_name(&self) -> &'static str {
        "sing-box.exe"
    }

    fn args(&self, config_path: &str) -> Vec<String> {
        // `--disable-color` keeps the captured log parseable; sing-box
        // otherwise emits ANSI escapes even when stdout is a pipe.
        vec![
            "run".into(),
            "-c".into(),
            config_path.into(),
            "--disable-color".into(),
        ]
    }

    fn stats_channel(&self, endpoints: Endpoints) -> StatsChannel {
        StatsChannel::ClashApi {
            port: endpoints.api_port,
        }
    }

    fn validate_node(&self, node: &Node) -> AppResult<()> {
        ensure_node_usable(node)?;
        if let Transport::Grpc { service_name } = &node.transport {
            if service_name.trim().is_empty() {
                return Err(AppError::InvalidInput(
                    "gRPC 传输必须填写 service name".to_string(),
                ));
            }
        }
        Ok(())
    }

    fn render_config(&self, input: &RenderInput<'_>) -> AppResult<Value> {
        self.validate_node(input.node)?;
        let endpoints = input.endpoints;
        let outbounds = singbox_outbounds(input)?;

        // Rule sets are a first-class object here, so a referenced set is
        // declared once and pointed at by tag. Omitted entirely when unused:
        // an empty array is a needless thing for a kernel to validate.
        let mut route = serde_json::Map::new();
        // With TUN on, the kernel sees IP packets instead of a socket API call,
        // so the destination domain is gone unless something reads it back off
        // the wire. The first rule asks sing-box to sniff it on every
        // connection; every later rule then matches the domain it always
        // matched. Nothing is added when TUN is off, so the table — and the
        // behaviour it was validated against — stays byte for byte the same.
        let mut rules = singbox_routing_rules(input);
        if input.tun_enabled {
            rules.insert(0, json!({ "action": "sniff" }));
        }
        route.insert("rules".into(), json!(rules));
        let rule_sets = singbox_rule_set_declarations(input);
        if !rule_sets.is_empty() {
            route.insert("rule_set".into(), json!(rule_sets));
        }
        // Without this the kernel leaves `metadata.processPath` empty and the
        // connections page can never name the program behind a connection —
        // the one thing people want when something unexpected is talking to
        // the network. Costs one syscall per new connection.
        route.insert("find_process".into(), json!(true));
        // sing-box 1.12 deprecated resolving outbound domains through the
        // system resolver; 1.14 makes it fatal. Once a `dns` section exists the
        // kernel demands an explicit default, so name our first server.
        if !input.dns_servers.is_empty() {
            route.insert("default_domain_resolver".into(), json!("dns-0"));
        }

        // sing-box 1.12 replaced the old `address` string with typed servers,
        // so this is the modern shape: every entry gets a type and a tag, and
        // `final` names the one used when nothing else matches.
        let dns = if input.dns_servers.is_empty() {
            Value::Null
        } else {
            let servers: Vec<Value> = input
                .dns_servers
                .iter()
                .enumerate()
                .map(|(index, entry)| {
                    let tag = format!("dns-{index}");
                    match entry {
                        crate::domain::settings::DnsEntry::Plain(host) => json!({
                            "type": "udp",
                            "tag": tag,
                            "server": host,
                        }),
                        crate::domain::settings::DnsEntry::Doh { host, path } => json!({
                            "type": "https",
                            "tag": tag,
                            "server": host,
                            "path": path,
                        }),
                        crate::domain::settings::DnsEntry::Dot { host } => json!({
                            "type": "tls",
                            "tag": tag,
                            "server": host,
                        }),
                    }
                })
                .collect();
            json!({
                "servers": servers,
                "final": "dns-0",
            })
        };

        Ok(json!({
            "log": {
                "level": input.log_level.as_singbox(),
                "timestamp": true,
            },
            "dns": dns,
            "inbounds": singbox_inbounds(input, endpoints),
            "outbounds": outbounds,
            "route": Value::Object(route),
            "experimental": {
                "clash_api": {
                    "external_controller": format!("127.0.0.1:{}", endpoints.api_port),
                },
            },
        }))
    }
}

/// The inbound listeners.
///
/// Without a split this is the single `mixed` listener, exactly the shape this
/// config has always had: SOCKS5 and HTTP on one port, tag `socks`. With a
/// split each protocol gets its own listener — `socks` (UDP relay on, because a
/// SOCKS5 client expects UDP ASSOCIATE to keep working) and `http` (no `udp`:
/// sing-box has no UDP semantics for an HTTP inbound).
fn singbox_inbounds(input: &RenderInput<'_>, endpoints: Endpoints) -> Vec<Value> {
    let mut inbounds = if !endpoints.split() {
        vec![json!({
            "type": "mixed",
            "tag": "socks",
            "listen": input.listen_address(),
            "listen_port": endpoints.proxy_port(),
        })]
    } else {
        vec![
            json!({
                "type": "socks",
                "tag": "socks",
                "listen": input.listen_address(),
                "listen_port": endpoints.socks_port,
                "udp": true,
            }),
            json!({
                "type": "http",
                "tag": "http",
                "listen": input.listen_address(),
                "listen_port": endpoints.http_port,
            }),
        ]
    };
    // The TUN listener goes last and only ever appends. The proxy inbounds keep
    // their tags, their order and their fields, so a config with TUN on is the
    // config the app already ships with one extra listener at the end — nothing
    // else moves, and turning TUN back off restores the old document exactly.
    if input.tun_enabled {
        inbounds.push(singbox_tun_inbound(input));
    }
    inbounds
}

/// The TUN inbound: take over the machine's traffic at the adapter level.
///
/// `172.19.0.1/30` is a two-host link-local-ish subnet that no home or office
/// LAN uses, so `auto_route` cannot collide with a real gateway. The kernel
/// creates the adapter itself; no WinTun DLL needs shipping alongside it.
///
/// `route_exclude_address` holds `100.64.0.0/10` out of the tunnel. That is the
/// CGNAT block Tailscale (and therefore the remote session this machine is
/// driven over) hands out; letting `auto_route` swallow it would cut the link
/// the app is being driven through. sing-box's own documentation excludes the
/// same range for the same reason.
///
/// `strict_route` stays off: it drops everything that does not go through the
/// adapter, which on Windows also kills LAN discovery and sharing — a proxy
/// client that breaks the local network is not what "take over my traffic"
/// means.
fn singbox_tun_inbound(input: &RenderInput<'_>) -> Value {
    json!({
        "type": "tun",
        "tag": "tun",
        "address": ["172.19.0.1/30"],
        "mtu": 9000,
        "auto_route": true,
        "strict_route": false,
        "stack": input.tun_stack,
        "route_exclude_address": ["100.64.0.0/10"],
    })
}

/// All outbounds for this run.
///
/// sing-box is the kernel that can express groups natively, so each enabled
/// group becomes a real `selector` or `urltest` outbound and every member node
/// gets its own `node:<id>` outbound for the group to point at. A node shared
/// by two groups is emitted once.
///
/// `fallback` / `load-balance` are rendered as selectors, not as their Clash
/// counterparts: sing-box has neither, and the switching is done by this app
/// (see [`crate::engine::groupwatch`]).
fn singbox_outbounds(input: &RenderInput<'_>) -> AppResult<Vec<Value>> {
    let mux = input.mux_streams();
    let mut outbounds = vec![singbox_outbound(input.node, "proxy", mux)?];
    let mut emitted: HashSet<String> = HashSet::new();

    for group in input.active_groups() {
        let mut members: Vec<String> = Vec::new();
        for id in &group.members {
            let Some(node) = input.nodes.iter().find(|node| &node.id == id) else {
                // The node is gone; the group keeps whatever is left.
                continue;
            };
            let tag = format!("node:{}", node.id);
            if emitted.insert(tag.clone()) {
                outbounds.push(singbox_outbound(node, &tag, mux)?);
            }
            if !members.contains(&tag) {
                members.push(tag);
            }
        }
        if members.is_empty() {
            // An empty selector is a config error in sing-box, so a group whose
            // nodes all disappeared is simply not emitted.
            continue;
        }
        let mut entry = Map::new();
        entry.insert("tag".into(), json!(group.tag()));
        entry.insert("outbounds".into(), json!(members));
        match group.kind {
            GroupKind::Select => {
                entry.insert("type".into(), json!("selector"));
                if let Some(chosen) = group.chosen() {
                    entry.insert("default".into(), json!(format!("node:{chosen}")));
                }
            }
            GroupKind::UrlTest => {
                entry.insert("type".into(), json!("urltest"));
                entry.insert("url".into(), json!(input.probe_url.as_str()));
                entry.insert("interval".into(), json!("3m"));
                entry.insert("tolerance".into(), json!(50));
            }
            GroupKind::Fallback | GroupKind::LoadBalance => {
                // sing-box has no fallback or load-balance outbound. The
                // decision belongs to this app (see `engine::groupwatch`): it
                // probes every member and rewrites the selector through the
                // Clash API, so the kernel side must be a plain `selector` that
                // accepts an external switch. `default` is the first member
                // because `chosen()` already means "first member unless the
                // user pinned one"; the watcher takes over from there.
                entry.insert("type".into(), json!("selector"));
                if let Some(chosen) = group.chosen() {
                    entry.insert("default".into(), json!(format!("node:{chosen}")));
                }
            }
        }
        outbounds.push(Value::Object(entry));
    }

    outbounds.push(json!({ "type": "direct", "tag": "direct" }));
    Ok(outbounds)
}

/// One proxy outbound: `proxy` for the active node, `node:<id>` for a group
/// member.
///
/// `mux_streams` is `Some(n)` only when the user turned multiplexing on;
/// sing-box spells the feature `multiplex` and its stream cap `max_streams`.
fn singbox_outbound(node: &Node, tag: &str, mux_streams: Option<u16>) -> AppResult<Value> {
    let mut out = Map::new();
    out.insert("tag".into(), json!(tag));
    out.insert("server".into(), json!(node.server));
    out.insert("server_port".into(), json!(node.port));

    match &node.protocol {
        Protocol::Vless { uuid, flow } => {
            out.insert("type".into(), json!("vless"));
            out.insert("uuid".into(), json!(uuid));
            if let Some(flow) = flow.as_deref().filter(|f| !f.is_empty()) {
                out.insert("flow".into(), json!(flow));
            }
        }
        Protocol::Vmess { uuid, alter_id } => {
            out.insert("type".into(), json!("vmess"));
            out.insert("uuid".into(), json!(uuid));
            out.insert("alter_id".into(), json!(alter_id));
            out.insert("security".into(), json!("auto"));
        }
        Protocol::Trojan { password } => {
            out.insert("type".into(), json!("trojan"));
            out.insert("password".into(), json!(password));
        }
        Protocol::Shadowsocks { method, password } => {
            out.insert("type".into(), json!("shadowsocks"));
            out.insert("method".into(), json!(method));
            out.insert("password".into(), json!(password));
        }
        Protocol::Hysteria2 {
            password,
            obfs_password,
            up_mbps,
            down_mbps,
        } => {
            out.insert("type".into(), json!("hysteria2"));
            out.insert("password".into(), json!(password));
            if let Some(obfs) = obfs_password.as_deref().filter(|o| !o.trim().is_empty()) {
                out.insert(
                    "obfs".into(),
                    json!({ "type": "salamander", "password": obfs }),
                );
            }
            // Zero means "no hint"; sending 0 makes the kernel treat the
            // link as having no bandwidth at all.
            if let Some(up) = up_mbps.filter(|v| *v > 0) {
                out.insert("up_mbps".into(), json!(up));
            }
            if let Some(down) = down_mbps.filter(|v| *v > 0) {
                out.insert("down_mbps".into(), json!(down));
            }
        }
        Protocol::Tuic {
            uuid,
            password,
            congestion_control,
            udp_relay_mode,
        } => {
            out.insert("type".into(), json!("tuic"));
            out.insert("uuid".into(), json!(uuid));
            out.insert("password".into(), json!(password));
            if let Some(cc) = congestion_control.as_deref().filter(|c| !c.trim().is_empty()) {
                out.insert("congestion_control".into(), json!(cc));
            }
            if let Some(mode) = udp_relay_mode.as_deref().filter(|m| !m.trim().is_empty()) {
                out.insert("udp_relay_mode".into(), json!(mode));
            }
        }
    }

    if let Some(transport) = singbox_transport(&node.transport) {
        out.insert("transport".into(), transport);
    }
    if let Some(tls) = singbox_tls(&node.security) {
        out.insert("tls".into(), tls);
    } else if node.protocol.is_quic() {
        // hysteria2 and TUIC carry their own TLS handshake: sing-box refuses
        // the outbound unless `tls.enabled` is true, even when the user
        // imported a share link that names no TLS at all.
        out.insert("tls".into(), json!({ "enabled": true }));
    }

    // `multiplex` is a flat sibling of `tls` on the outbound. `smux` is the
    // wire protocol both kernels speak, and sing-box names the stream cap
    // `max_streams` rather than Xray's `concurrency`.
    if let Some(streams) = mux_streams {
        out.insert(
            "multiplex".into(),
            json!({ "enabled": true, "protocol": "smux", "max_streams": streams }),
        );
    }

    Ok(Value::Object(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::node::Security;
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
        let endpoints = Endpoints::new(7890, 0, 0, 9090).unwrap();
        SingBoxEngine
            .render_config(&RenderInput::new(node, endpoints))
            .unwrap()
    }

    #[test]
    fn vless_reality_grpc_renders_flat_outbound() {
        let cfg = render(&node(
            Protocol::Vless {
                uuid: "uuid-1".into(),
                flow: Some("xtls-rprx-vision".into()),
            },
            Transport::Grpc {
                service_name: "svc".into(),
            },
            Security::Reality {
                public_key: "pk".into(),
                short_id: "ab".into(),
                server_name: "sni.example.com".into(),
                fingerprint: "firefox".into(),
                spider_x: "/".into(),
            },
        ));
        let out = &cfg["outbounds"][0];
        assert_eq!(out["type"], json!("vless"));
        assert_eq!(out["tag"], json!("proxy"));
        assert_eq!(out["server"], json!("example.com"));
        assert_eq!(out["server_port"], json!(443));
        assert_eq!(out["flow"], json!("xtls-rprx-vision"));
        assert_eq!(out["transport"]["type"], json!("grpc"));
        assert_eq!(out["transport"]["service_name"], json!("svc"));
        assert_eq!(out["tls"]["reality"]["public_key"], json!("pk"));
        // sing-box has no `streamSettings` nesting — the whole point of the
        // separate renderer.
        assert!(out.get("streamSettings").is_none());
    }

    #[test]
    fn split_inbounds_give_socks_and_http_their_own_listeners() {
        let endpoints = Endpoints::new(7890, 7891, 7892, 9090).unwrap();
        let n = node(
            Protocol::Trojan {
                password: "pw".into(),
            },
            Transport::Tcp,
            Security::None,
        );
        let cfg = SingBoxEngine
            .render_config(&RenderInput::new(&n, endpoints))
            .unwrap();
        let inbounds = cfg["inbounds"].as_array().unwrap();
        assert_eq!(inbounds.len(), 2);
        // The two entries are the SOCKS and HTTP listeners, tags unchanged so
        // rules that referenced `socks` keep working.
        assert_eq!(inbounds[0]["type"], json!("socks"));
        assert_eq!(inbounds[0]["tag"], json!("socks"));
        assert_eq!(inbounds[0]["listen_port"], json!(7892));
        assert_eq!(inbounds[0]["udp"], json!(true));
        assert_eq!(inbounds[1]["type"], json!("http"));
        assert_eq!(inbounds[1]["tag"], json!("http"));
        assert_eq!(inbounds[1]["listen_port"], json!(7891));
        assert!(inbounds[1].get("udp").is_none());
    }

    #[test]
    fn plain_tcp_without_tls_omits_both_blocks() {
        let cfg = render(&node(
            Protocol::Trojan {
                password: "pw".into(),
            },
            Transport::Tcp,
            Security::None,
        ));
        let out = &cfg["outbounds"][0];
        assert!(out.get("transport").is_none());
        assert!(out.get("tls").is_none());
        assert_eq!(out["password"], json!("pw"));
    }

    #[test]
    fn shadowsocks_and_vmess_map_their_own_fields() {
        let ss = render(&node(
            Protocol::Shadowsocks {
                method: "chacha20-ietf-poly1305".into(),
                password: "pw".into(),
            },
            Transport::Tcp,
            Security::None,
        ));
        assert_eq!(ss["outbounds"][0]["type"], json!("shadowsocks"));
        assert_eq!(
            ss["outbounds"][0]["method"],
            json!("chacha20-ietf-poly1305")
        );

        let vmess = render(&node(
            Protocol::Vmess {
                uuid: "u".into(),
                alter_id: 0,
            },
            Transport::Tcp,
            Security::None,
        ));
        assert_eq!(vmess["outbounds"][0]["type"], json!("vmess"));
        assert_eq!(vmess["outbounds"][0]["alter_id"], json!(0));
        assert_eq!(vmess["outbounds"][0]["security"], json!("auto"));
    }

    #[test]
    fn baseline_outbounds_are_proxy_then_direct_only() {
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
        assert_eq!(tags, vec!["proxy", "direct"]);
    }

    #[test]
    fn clash_api_is_wired_to_the_api_port() {
        let cfg = render(&node(
            Protocol::Trojan {
                password: "pw".into(),
            },
            Transport::Tcp,
            Security::None,
        ));
        assert_eq!(
            cfg["experimental"]["clash_api"]["external_controller"],
            json!("127.0.0.1:9090")
        );
        assert_eq!(cfg["inbounds"][0]["type"], json!("mixed"));
        assert_eq!(cfg["inbounds"][0]["listen_port"], json!(7890));
    }

    #[test]
    fn grpc_without_service_name_is_rejected() {
        let n = node(
            Protocol::Trojan {
                password: "pw".into(),
            },
            Transport::Grpc {
                service_name: "   ".into(),
            },
            Security::None,
        );
        let endpoints = Endpoints::new(7890, 0, 0, 9090).unwrap();
        assert!(SingBoxEngine
            .render_config(&RenderInput::new(&n, endpoints))
            .is_err());
    }

    #[test]
    fn args_disable_colour_for_captured_logs() {
        assert_eq!(
            SingBoxEngine.args("C:/cfg/config.json"),
            vec!["run", "-c", "C:/cfg/config.json", "--disable-color"]
        );
    }

    #[test]
    fn stats_channel_is_the_clash_api() {
        let endpoints = Endpoints::new(7890, 0, 0, 9090).unwrap();
        assert_eq!(
            SingBoxEngine.stats_channel(endpoints),
            StatsChannel::ClashApi { port: 9090 }
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

    fn group(id: &str, kind: GroupKind, members: &[&str]) -> crate::domain::group::ProxyGroup {
        crate::domain::group::ProxyGroup {
            id: id.into(),
            name: "组".into(),
            enabled: true,
            kind,
            members: members.iter().map(|member| member.to_string()).collect(),
            selected: None,
        }
    }

    #[test]
    fn hysteria2_gets_a_tls_block_even_when_the_node_has_no_security() {
        let cfg = render(&node(
            Protocol::Hysteria2 {
                password: "pw".into(),
                obfs_password: Some("salt".into()),
                up_mbps: Some(120),
                down_mbps: Some(0),
            },
            Transport::Tcp,
            Security::None,
        ));
        let out = &cfg["outbounds"][0];
        assert_eq!(out["type"], json!("hysteria2"));
        assert_eq!(out["password"], json!("pw"));
        assert_eq!(
            out["obfs"],
            json!({ "type": "salamander", "password": "salt" })
        );
        assert_eq!(out["up_mbps"], json!(120));
        assert!(out.get("down_mbps").is_none(), "a zero hint is not a hint");
        // sing-box refuses the outbound without it.
        assert_eq!(out["tls"]["enabled"], json!(true));
    }

    #[test]
    fn tuic_keeps_both_halves_of_the_credential_and_its_knobs() {
        let cfg = render(&node(
            Protocol::Tuic {
                uuid: "uuid-1".into(),
                password: "pw".into(),
                congestion_control: Some("bbr".into()),
                udp_relay_mode: Some("quic".into()),
            },
            Transport::Tcp,
            Security::Tls {
                sni: Some("tu.example.com".into()),
                alpn: vec![],
                allow_insecure: true,
                pinned_cert_sha256: vec![],
            },
        ));
        let out = &cfg["outbounds"][0];
        assert_eq!(out["type"], json!("tuic"));
        assert_eq!(out["uuid"], json!("uuid-1"));
        assert_eq!(out["password"], json!("pw"));
        assert_eq!(out["congestion_control"], json!("bbr"));
        assert_eq!(out["udp_relay_mode"], json!("quic"));
        assert_eq!(out["tls"]["server_name"], json!("tu.example.com"));
        assert_eq!(out["tls"]["insecure"], json!(true));
    }

    #[test]
    fn a_select_group_becomes_a_native_selector() {
        let (first, second) = two_nodes();
        let nodes = vec![first.clone(), second.clone()];
        let groups = vec![group("g1", GroupKind::Select, &["n1", "n2"])];
        let endpoints = Endpoints::new(7890, 0, 0, 9090).unwrap();
        let input = RenderInput::new(&first, endpoints)
            .with_options(RunOptions::new().with_catalog(&nodes, &groups));

        let cfg = SingBoxEngine.render_config(&input).unwrap();
        let outbounds = cfg["outbounds"].as_array().unwrap();
        let tags: Vec<&str> = outbounds
            .iter()
            .map(|outbound| outbound["tag"].as_str().unwrap())
            .collect();
        assert_eq!(tags, vec!["proxy", "node:n1", "node:n2", "group:g1", "direct"]);
        assert_eq!(outbounds[3]["type"], json!("selector"));
        assert_eq!(outbounds[3]["outbounds"], json!(["node:n1", "node:n2"]));
        assert_eq!(outbounds[3]["default"], json!("node:n1"));
    }

    #[test]
    fn a_fallback_group_is_a_selector_the_watcher_can_switch() {
        let (first, second) = two_nodes();
        let nodes = vec![first.clone(), second.clone()];
        let groups = vec![group("g1", GroupKind::Fallback, &["n1", "n2"])];
        let endpoints = Endpoints::new(7890, 0, 0, 9090).unwrap();
        let input = RenderInput::new(&first, endpoints)
            .with_options(RunOptions::new().with_catalog(&nodes, &groups));

        let cfg = SingBoxEngine.render_config(&input).unwrap();
        let group_out = &cfg["outbounds"][3];
        // sing-box has no fallback outbound: the app switches this selector.
        assert_eq!(group_out["type"], json!("selector"));
        assert_eq!(group_out["outbounds"], json!(["node:n1", "node:n2"]));
        assert_eq!(group_out["default"], json!("node:n1"));
    }

    #[test]
    fn a_load_balance_group_is_also_a_selector() {
        let (first, second) = two_nodes();
        let nodes = vec![first.clone(), second.clone()];
        let groups = vec![group("g1", GroupKind::LoadBalance, &["n1", "n2"])];
        let endpoints = Endpoints::new(7890, 0, 0, 9090).unwrap();
        let input = RenderInput::new(&first, endpoints)
            .with_options(RunOptions::new().with_catalog(&nodes, &groups));

        let cfg = SingBoxEngine.render_config(&input).unwrap();
        let group_out = &cfg["outbounds"][3];
        // Rotation is app-side too; the kernel only sees a selector.
        assert_eq!(group_out["type"], json!("selector"));
        assert_eq!(group_out["default"], json!("node:n1"));
    }

    #[test]
    fn a_url_test_group_becomes_a_native_urltest() {
        let (first, second) = two_nodes();
        let nodes = vec![first.clone(), second.clone()];
        let groups = vec![group("g1", GroupKind::UrlTest, &["n1", "n2"])];
        let endpoints = Endpoints::new(7890, 0, 0, 9090).unwrap();
        let input = RenderInput::new(&first, endpoints)
            .with_options(RunOptions::new().with_catalog(&nodes, &groups));

        let cfg = SingBoxEngine.render_config(&input).unwrap();
        let group_out = &cfg["outbounds"][3];
        assert_eq!(group_out["type"], json!("urltest"));
        assert_eq!(group_out["interval"], json!("3m"));
    }

    #[test]
    fn a_url_test_group_probes_the_configured_url() {
        let nodes = vec![node(
            Protocol::Trojan { password: "pw".into() },
            Transport::Tcp,
            Security::None,
        )];
        let groups = vec![group("g1", GroupKind::UrlTest, &["n1"])];
        let options = RunOptions {
            probe_url: "http://cp.cloudflare.com/".into(),
            ..RunOptions::new().with_catalog(&nodes, &groups)
        };
        let endpoints = Endpoints::new(7890, 0, 0, 9090).unwrap();
        let config = SingBoxEngine
            .render_config(&RenderInput::new(&nodes[0], endpoints).with_options(options))
            .unwrap();

        let outbounds = config["outbounds"].as_array().unwrap();
        let urltest = outbounds
            .iter()
            .find(|outbound| outbound["type"] == "urltest")
            .expect("url-test group is rendered");
        assert_eq!(urltest["url"], "http://cp.cloudflare.com/");
    }

    #[test]
    fn a_node_shared_by_two_groups_is_emitted_once() {
        let (first, second) = two_nodes();
        let nodes = vec![first.clone(), second.clone()];
        let groups = vec![
            group("g1", GroupKind::Select, &["n1", "n2"]),
            group("g2", GroupKind::Select, &["n1"]),
        ];
        let endpoints = Endpoints::new(7890, 0, 0, 9090).unwrap();
        let input = RenderInput::new(&first, endpoints)
            .with_options(RunOptions::new().with_catalog(&nodes, &groups));

        let cfg = SingBoxEngine.render_config(&input).unwrap();
        let tags: Vec<&str> = cfg["outbounds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|outbound| outbound["tag"].as_str().unwrap())
            .collect();
        assert_eq!(tags, vec!["proxy", "node:n1", "node:n2", "group:g1", "group:g2", "direct"]);
    }

    #[test]
    fn custom_resolvers_use_the_typed_server_shape() {
        let first = node(
            Protocol::Vless {
                uuid: "uuid-1".into(),
                flow: None,
            },
            Transport::Tcp,
            Security::None,
        );
        let endpoints = Endpoints::new(7890, 0, 0, 9090).unwrap();
        let mut input = RenderInput::new(&first, endpoints);
        input.dns_servers = vec![
            crate::domain::settings::DnsEntry::Plain("1.1.1.1".into()),
            crate::domain::settings::DnsEntry::Plain("8.8.8.8".into()),
        ];
        let cfg = SingBoxEngine.render_config(&input).unwrap();
        assert_eq!(cfg["dns"]["servers"][0]["type"], json!("udp"));
        assert_eq!(cfg["dns"]["servers"][0]["tag"], json!("dns-0"));
        assert_eq!(cfg["dns"]["servers"][1]["server"], json!("8.8.8.8"));
        assert_eq!(cfg["dns"]["final"], json!("dns-0"));
        // sing-box 1.14 refuses to start without this once `dns` is present.
        assert_eq!(cfg["route"]["default_domain_resolver"], json!("dns-0"));
        // The connections page names the owning program only when the kernel
        // was asked to look it up.
        assert_eq!(cfg["route"]["find_process"], json!(true));
    }

    #[test]
    fn encrypted_resolvers_use_the_typed_server_shapes() {
        let first = node(
            Protocol::Vless {
                uuid: "uuid-1".into(),
                flow: None,
            },
            Transport::Tcp,
            Security::None,
        );
        let endpoints = Endpoints::new(7890, 0, 0, 9090).unwrap();
        let mut input = RenderInput::new(&first, endpoints);
        input.dns_servers = vec![
            crate::domain::settings::DnsEntry::Dot {
                host: "9.9.9.9".into(),
            },
            crate::domain::settings::DnsEntry::Doh {
                host: "8.8.8.8".into(),
                path: "/dns-query".into(),
            },
        ];
        let cfg = SingBoxEngine.render_config(&input).unwrap();
        assert_eq!(cfg["dns"]["servers"][0]["type"], json!("tls"));
        assert_eq!(cfg["dns"]["servers"][0]["server"], json!("9.9.9.9"));
        assert_eq!(cfg["dns"]["servers"][1]["type"], json!("https"));
        assert_eq!(cfg["dns"]["servers"][1]["path"], json!("/dns-query"));
    }

    #[test]
    fn no_resolvers_means_no_dns_section_in_sing_box() {
        let first = node(
            Protocol::Vless {
                uuid: "uuid-1".into(),
                flow: None,
            },
            Transport::Tcp,
            Security::None,
        );
        assert!(render(&first)["dns"].is_null());
    }

    #[test]
    fn tun_inbound_is_appended_and_nothing_else_changes() {
        let endpoints = Endpoints::new(7890, 0, 0, 9090).unwrap();
        let n = node(
            Protocol::Trojan {
                password: "pw".into(),
            },
            Transport::Tcp,
            Security::None,
        );
        let plain = SingBoxEngine
            .render_config(&RenderInput::new(&n, endpoints))
            .unwrap();
        let mut wanting = RenderInput::new(&n, endpoints);
        wanting.tun_enabled = true;
        let tun = SingBoxEngine.render_config(&wanting).unwrap();

        // Off is the old document, byte for byte: one listener, no sniff rule.
        let plain_inbounds = plain["inbounds"].as_array().unwrap();
        assert_eq!(plain_inbounds.len(), 1);
        assert_eq!(plain_inbounds[0]["type"], json!("mixed"));
        assert!(plain_inbounds[0].get("address").is_none());
        let plain_rules = plain["route"]["rules"].as_array().unwrap();
        assert!(!plain_rules.iter().any(|rule| rule == &json!({"action": "sniff"})));

        // On adds exactly one listener, at the end, with the settings' stack.
        let inbounds = tun["inbounds"].as_array().unwrap();
        assert_eq!(inbounds.len(), 2);
        assert_eq!(inbounds[0], plain_inbounds[0]);
        let t = &inbounds[1];
        assert_eq!(t["type"], json!("tun"));
        assert_eq!(t["tag"], json!("tun"));
        assert_eq!(t["address"], json!(["172.19.0.1/30"]));
        assert_eq!(t["mtu"], json!(9000));
        assert_eq!(t["auto_route"], json!(true));
        assert_eq!(t["strict_route"], json!(false));
        assert_eq!(t["stack"], json!("mixed"));
        assert_eq!(t["route_exclude_address"], json!(["100.64.0.0/10"]));
        assert!(t.get("listen_port").is_none());

        // Sniffing is what gives the domain rules something to match once the
        // traffic arrives as packets, so it goes first.
        let rules = tun["route"]["rules"].as_array().unwrap();
        assert_eq!(rules[0], json!({"action": "sniff"}));
        assert_eq!(rules.len(), plain_rules.len() + 1);

        // And nothing else moved: same outbounds, DNS, API.
        assert_eq!(tun["outbounds"], plain["outbounds"]);
        assert_eq!(tun["dns"], plain["dns"]);
        assert_eq!(tun["log"], plain["log"]);
        assert_eq!(tun["experimental"], plain["experimental"]);

        // With a split the two proxy listeners stay in place and the tun is
        // still last: "appended", not "first".
        let split = Endpoints::new(7890, 7891, 7892, 9090).unwrap();
        let mut wanting = RenderInput::new(&n, split);
        wanting.tun_enabled = true;
        let cfg = SingBoxEngine.render_config(&wanting).unwrap();
        let inbounds = cfg["inbounds"].as_array().unwrap();
        assert_eq!(inbounds.len(), 3);
        assert_eq!(inbounds[0]["tag"], json!("socks"));
        assert_eq!(inbounds[1]["tag"], json!("http"));
        assert_eq!(inbounds[2]["tag"], json!("tun"));
    }

    #[test]
    fn tun_stack_is_written_through() {
        let endpoints = Endpoints::new(7890, 0, 0, 9090).unwrap();
        let n = node(
            Protocol::Trojan {
                password: "pw".into(),
            },
            Transport::Tcp,
            Security::None,
        );
        for stack in ["system", "gvisor", "mixed"] {
            let mut input = RenderInput::new(&n, endpoints);
            input.tun_enabled = true;
            input.tun_stack = stack.to_string();
            let cfg = SingBoxEngine.render_config(&input).unwrap();
            let last = cfg["inbounds"].as_array().unwrap().last().unwrap();
            assert_eq!(last["stack"], json!(stack));
        }
    }

}
