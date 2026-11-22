//! Shared rendering helpers.
//!
//! Transport and security are modelled once in [`crate::domain::node`], but the
//! two kernels spell them differently: Xray nests them under
//! `streamSettings.{wsSettings,tlsSettings,…}`, sing-box flattens them into
//! top-level `transport` / `tls` objects on the outbound. These functions are
//! the only place that translation happens.

use serde_json::{json, Map, Value};

use super::{LoadedRuleSet, RenderInput};
use crate::domain::node::{Security, Transport};
use crate::domain::routing::RuleMatcher;
use crate::domain::ruleset::SetEntry;
use crate::domain::settings::ProxyMode;

/// Default websocket path when the node does not specify one.
const DEFAULT_WS_PATH: &str = "/";

/// Xray `streamSettings` block for an outbound.
pub fn xray_stream_settings(transport: &Transport, security: &Security) -> Value {
    let mut stream = Map::new();

    match transport {
        Transport::Tcp => {
            stream.insert("network".into(), json!("tcp"));
        }
        Transport::Ws { path, host } => {
            stream.insert("network".into(), json!("ws"));
            let mut ws = Map::new();
            ws.insert(
                "path".into(),
                json!(if path.is_empty() { DEFAULT_WS_PATH } else { path.as_str() }),
            );
            if let Some(host) = host.as_deref().filter(|h| !h.is_empty()) {
                ws.insert("headers".into(), json!({ "Host": host }));
            }
            stream.insert("wsSettings".into(), Value::Object(ws));
        }
        Transport::Grpc { service_name } => {
            stream.insert("network".into(), json!("grpc"));
            stream.insert("grpcSettings".into(), json!({ "serviceName": service_name }));
        }
    }

    match security {
        Security::None => {
            stream.insert("security".into(), json!("none"));
        }
        Security::Tls {
            sni,
            alpn,
            pinned_cert_sha256,
            ..
        } => {
            stream.insert("security".into(), json!("tls"));
            let mut tls = Map::new();
            if let Some(sni) = sni.as_deref().filter(|s| !s.is_empty()) {
                tls.insert("serverName".into(), json!(sni));
            }
            if !alpn.is_empty() {
                tls.insert("alpn".into(), json!(alpn));
            }
            // Xray 26 dropped `allowInsecure` (it refuses to start when the
            // key is present), so pinning is the supported way to talk to a
            // self-signed server. Emitting `allowInsecure` here would break
            // the whole config, not just this node.
            if !pinned_cert_sha256.is_empty() {
                // Xray wants a single string here (a JSON array fails to
                // decode: "cannot unmarshal array into ... of type string"),
                // so several pins are joined with commas.
                tls.insert(
                    "pinnedPeerCertSha256".into(),
                    json!(pinned_cert_sha256.join(",")),
                );
            }
            stream.insert("tlsSettings".into(), Value::Object(tls));
        }
        Security::Reality {
            public_key,
            short_id,
            server_name,
            fingerprint,
            spider_x,
        } => {
            stream.insert("security".into(), json!("reality"));
            stream.insert(
                "realitySettings".into(),
                json!({
                    "serverName": server_name,
                    "publicKey": public_key,
                    "shortId": short_id,
                    // uTLS ClientHello fingerprint; Xray requires one, and the
                    // node's own value has to win or a server that only accepts
                    // a specific fingerprint would refuse the handshake.
                    "fingerprint": fingerprint,
                    "spiderX": spider_x,
                }),
            );
        }
    }

    Value::Object(stream)
}

/// sing-box `transport` object for an outbound, or `None` for plain TCP.
pub fn singbox_transport(transport: &Transport) -> Option<Value> {
    match transport {
        Transport::Tcp => None,
        Transport::Ws { path, host } => {
            let mut ws = Map::new();
            ws.insert("type".into(), json!("ws"));
            ws.insert(
                "path".into(),
                json!(if path.is_empty() { DEFAULT_WS_PATH } else { path.as_str() }),
            );
            if let Some(host) = host.as_deref().filter(|h| !h.is_empty()) {
                ws.insert("headers".into(), json!({ "Host": host }));
            }
            Some(Value::Object(ws))
        }
        Transport::Grpc { service_name } => Some(json!({
            "type": "grpc",
            "service_name": service_name,
        })),
    }
}

/// sing-box `tls` object for an outbound, or `None` when security is off.
pub fn singbox_tls(security: &Security) -> Option<Value> {
    match security {
        Security::None => None,
        Security::Tls {
            sni,
            alpn,
            allow_insecure,
            ..
        } => {
            let mut tls = Map::new();
            tls.insert("enabled".into(), json!(true));
            if let Some(sni) = sni.as_deref().filter(|s| !s.is_empty()) {
                tls.insert("server_name".into(), json!(sni));
            }
            if !alpn.is_empty() {
                tls.insert("alpn".into(), json!(alpn));
            }
            if *allow_insecure {
                tls.insert("insecure".into(), json!(true));
            }
            Some(Value::Object(tls))
        }
        Security::Reality {
            public_key,
            short_id,
            server_name,
            fingerprint,
            ..
        } => Some(json!({
            "enabled": true,
            "server_name": server_name,
            "utls": {
                "enabled": true,
                "fingerprint": fingerprint,
            },
            "reality": {
                "enabled": true,
                "public_key": public_key,
                "short_id": short_id,
            },
        })),
    }
}

/// Xray expresses every matcher as one `field` rule with an `outboundTag`.
/// The routing block Xray should use for this run.
///
/// `global` and `direct` are not "no rules": they are one catch-all rule that
/// outranks the user's table. Keeping them here (rather than clearing the
/// table) means a user can switch modes without losing their rules.
pub fn xray_routing_rules(input: &RenderInput<'_>) -> Vec<Value> {
    match input.mode {
        ProxyMode::Rule => xray_route_rules(input),
        ProxyMode::Global => {
            vec![json!({ "type": "field", "network": "tcp,udp", "outboundTag": "proxy" })]
        }
        ProxyMode::Direct => {
            vec![json!({ "type": "field", "network": "tcp,udp", "outboundTag": "direct" })]
        }
    }
}

/// The routing block sing-box should use for this run.
pub fn singbox_routing_rules(input: &RenderInput<'_>) -> Vec<Value> {
    match input.mode {
        ProxyMode::Rule => singbox_route_rules(input),
        ProxyMode::Global => vec![json!({ "outbound": "proxy" })],
        ProxyMode::Direct => vec![json!({ "outbound": "direct" })],
    }
}

pub fn xray_route_rules(input: &RenderInput<'_>) -> Vec<Value> {
    let mut out = Vec::new();
    for rule in input.rules.iter().filter(|rule| rule.enabled) {
        // Xray has no rule-set object: the entries have to be inlined. A set
        // may mix domains and CIDRs, and Xray *and*s the fields inside one
        // rule — so a mixed set becomes two rules, never one rule with both
        // keys (that would match nothing).
        if let RuleMatcher::RuleSet { value } = &rule.matcher {
            let Some(loaded) = LoadedRuleSet::find(input.rule_sets, value) else {
                // The set is gone or disabled: skip the rule rather than
                // emitting an empty matcher, which Xray would treat as "match
                // everything".
                continue;
            };
            let (domains, ips) = split_entries(&loaded.entries);
            if !domains.is_empty() {
                out.push(json!({
                    "type": "field",
                    "domain": domains,
                    "outboundTag": rule.action.xray_tag(),
                }));
            }
            if !ips.is_empty() {
                out.push(json!({
                    "type": "field",
                    "ip": ips,
                    "outboundTag": rule.action.xray_tag(),
                }));
            }
            continue;
        }

        let mut entry = Map::new();
        entry.insert("type".into(), json!("field"));
        match &rule.matcher {
            // Xray spells a suffix match `domain:` — there is no `suffix:`
            // prefix, and an unknown prefix is read as a literal keyword, so a
            // rule written that way never matches anything. (Found on a real
            // kernel: the rule stayed silent while traffic went to the proxy.)
            RuleMatcher::DomainSuffix { value } => {
                entry.insert("domain".into(), json!([format!("domain:{value}")]));
            }
            RuleMatcher::DomainKeyword { value } => {
                entry.insert("domain".into(), json!([format!("keyword:{value}")]));
            }
            RuleMatcher::DomainFull { value } => {
                entry.insert("domain".into(), json!([format!("full:{value}")]));
            }
            RuleMatcher::IpCidr { value } => {
                entry.insert("ip".into(), json!([value]));
            }
            RuleMatcher::GeoIp { value } => {
                entry.insert("ip".into(), json!([format!("geoip:{value}")]));
            }
            // Xray takes the port as a string or an array of strings.
            RuleMatcher::Port { value } => {
                entry.insert("port".into(), json!(value.to_string()));
            }
            RuleMatcher::ProcessName { value } => {
                entry.insert("process".into(), json!([value]));
            }
            RuleMatcher::RuleSet { .. } => unreachable!("handled above"),
        }
        entry.insert("outboundTag".into(), json!(rule.action.xray_tag()));
        out.push(Value::Object(entry));
    }
    out
}

/// Split parsed entries into Xray's two vocabulary sets.
fn split_entries(entries: &[SetEntry]) -> (Vec<String>, Vec<String>) {
    let mut domains = Vec::new();
    let mut ips = Vec::new();
    for entry in entries {
        match entry {
            SetEntry::DomainSuffix { value } => domains.push(format!("domain:{value}")),
            SetEntry::DomainKeyword { value } => domains.push(format!("keyword:{value}")),
            SetEntry::DomainFull { value } => domains.push(format!("full:{value}")),
            SetEntry::IpCidr { value } => ips.push(value.clone()),
        }
    }
    (domains, ips)
}

/// Rule-set declarations for sing-box's `route.rule_set`.
///
/// sing-box is the only kernel with a native rule-set object, so a set is
/// referenced by tag and its patterns live in a generated file. `format:
/// "source"` means the file is a JSON document (`version` + `rules`), which is
/// what [`singbox_rule_set_source`] writes.
pub fn singbox_rule_set_declarations(input: &RenderInput<'_>) -> Vec<Value> {
    let Some(dir) = input.rule_set_dir else {
        return Vec::new();
    };
    if input.mode != ProxyMode::Rule {
        return Vec::new();
    }

    let mut declared: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for rule in input.rules.iter().filter(|rule| rule.enabled) {
        let RuleMatcher::RuleSet { value } = &rule.matcher else {
            continue;
        };
        let Some(loaded) = LoadedRuleSet::find(input.rule_sets, value) else {
            continue;
        };
        // An empty set has nothing to match, and sing-box rejects a rule that
        // points at a rule-set nobody declared, so both sides skip it together.
        if loaded.entries.is_empty() {
            continue;
        }
        let tag = loaded.set.tag();
        if declared.contains(&tag) {
            continue;
        }
        declared.push(tag.clone());
        out.push(json!({
            "type": "local",
            "tag": tag,
            "format": "source",
            "path": dir.join(format!("{}.json", loaded.set.id)).display().to_string(),
        }));
    }
    out
}

/// The file sing-box reads for a set, in its *source* format.
///
/// This is JSON — `{"version": 1, "rules": [...]}` — not a list of
/// `domain_suffix:example.org` lines. Getting that wrong is not a cosmetic
/// mistake: sing-box refuses to boot with
/// `parse rule-set[0]: invalid character 'd' looking for beginning of value`,
/// which takes the whole kernel down over one provider file.
///
/// Each kind gets its own rule because fields inside one rule are ANDed, so
/// mixing suffixes and CIDRs in a single object would mean "a request that is
/// both". Separate objects are ORed by the rule-set matcher, which is what a
/// domain/IP list means.
pub fn singbox_rule_set_source(entries: &[SetEntry]) -> String {
    let mut suffixes: Vec<&str> = Vec::new();
    let mut keywords: Vec<&str> = Vec::new();
    let mut fulls: Vec<&str> = Vec::new();
    let mut ips: Vec<&str> = Vec::new();
    for entry in entries {
        match entry {
            SetEntry::DomainSuffix { value } => suffixes.push(value),
            SetEntry::DomainKeyword { value } => keywords.push(value),
            SetEntry::DomainFull { value } => fulls.push(value),
            SetEntry::IpCidr { value } => ips.push(value),
        }
    }

    let mut rules = Vec::new();
    for (key, values) in [
        ("domain_suffix", suffixes),
        ("domain_keyword", keywords),
        ("domain", fulls),
        ("ip_cidr", ips),
    ] {
        if values.is_empty() {
            continue;
        }
        let mut rule = Map::new();
        rule.insert(key.to_string(), json!(values));
        rules.push(Value::Object(rule));
    }

    let document = json!({ "version": 1, "rules": rules });
    serde_json::to_string_pretty(&document).unwrap_or_else(|_| "{\"version\":1,\"rules\":[]}".into())
}

/// sing-box uses one key per matcher, and blocking is an `action`, not an
/// outbound: the baseline config only has `proxy` and `direct`.
pub fn singbox_route_rules(input: &RenderInput<'_>) -> Vec<Value> {
    let mut out = Vec::new();
    for rule in input.rules.iter().filter(|rule| rule.enabled) {
        let mut entry = Map::new();

        // With a place to write the files, a set stays a set: sing-box resolves
        // it natively and the config stays small. Without one (unit tests, or a
        // preview that must not touch the disk) the entries are inlined, which
        // sing-box also understands.
        if let RuleMatcher::RuleSet { value } = &rule.matcher {
            let Some(loaded) = LoadedRuleSet::find(input.rule_sets, value) else {
                continue;
            };
            if input.rule_set_dir.is_some() {
                if loaded.entries.is_empty() {
                    continue;
                }
                entry.insert("rule_set".into(), json!([loaded.set.tag()]));
            } else {
                let (domains, ips) = split_singbox_entries(&loaded.entries);
                if !domains.is_empty() {
                    entry.insert("domain".into(), json!(domains));
                }
                if !ips.is_empty() {
                    entry.insert("ip_cidr".into(), json!(ips));
                }
                if entry.is_empty() {
                    continue;
                }
            }
        } else {
            match &rule.matcher {
                RuleMatcher::DomainSuffix { value } => {
                    entry.insert("domain_suffix".into(), json!([value]));
                }
                RuleMatcher::DomainKeyword { value } => {
                    entry.insert("domain_keyword".into(), json!([value]));
                }
                RuleMatcher::DomainFull { value } => {
                    entry.insert("domain".into(), json!([value]));
                }
                RuleMatcher::IpCidr { value } => {
                    entry.insert("ip_cidr".into(), json!([value]));
                }
                RuleMatcher::GeoIp { value } => {
                    entry.insert("geoip".into(), json!([value]));
                }
                RuleMatcher::Port { value } => {
                    entry.insert("port".into(), json!([value]));
                }
                RuleMatcher::ProcessName { value } => {
                    entry.insert("process_name".into(), json!([value]));
                }
                RuleMatcher::RuleSet { .. } => unreachable!("handled above"),
            }
        }

        match rule.action.singbox_tag() {
            Some(tag) => {
                entry.insert("outbound".into(), json!(tag));
            }
            None => {
                entry.insert("action".into(), json!("reject"));
            }
        }
        out.push(Value::Object(entry));
    }
    out
}

/// sing-box spells domain matchers as three separate keys, so the inlined form
/// of a mixed set is a single rule with `domain` + `ip_cidr` (sing-box *or*s
/// the keys of one rule, unlike Xray).
fn split_singbox_entries(entries: &[SetEntry]) -> (Vec<String>, Vec<String>) {
    let mut domains = Vec::new();
    let mut ips = Vec::new();
    for entry in entries {
        match entry {
            SetEntry::DomainSuffix { value } => domains.push(value.clone()),
            SetEntry::DomainKeyword { value } => domains.push(value.clone()),
            SetEntry::DomainFull { value } => domains.push(value.clone()),
            SetEntry::IpCidr { value } => ips.push(value.clone()),
        }
    }
    (domains, ips)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xray_tcp_without_tls_has_no_transport_blocks() {
        let v = xray_stream_settings(&Transport::Tcp, &Security::None);
        assert_eq!(v["network"], json!("tcp"));
        assert_eq!(v["security"], json!("none"));
        assert!(v.get("wsSettings").is_none());
        assert!(v.get("tlsSettings").is_none());
    }

    #[test]
    fn xray_ws_defaults_path_and_keeps_host_header() {
        let v = xray_stream_settings(
            &Transport::Ws {
                path: String::new(),
                host: Some("cdn.example.com".into()),
            },
            &Security::None,
        );
        assert_eq!(v["network"], json!("ws"));
        assert_eq!(v["wsSettings"]["path"], json!("/"));
        assert_eq!(v["wsSettings"]["headers"]["Host"], json!("cdn.example.com"));
    }

    #[test]
    fn xray_reality_always_carries_a_fingerprint() {
        let v = xray_stream_settings(
            &Transport::Grpc {
                service_name: "svc".into(),
            },
            &Security::Reality {
                public_key: "pk".into(),
                short_id: "ab".into(),
                server_name: "sni.example.com".into(),
                fingerprint: "firefox".into(),
                spider_x: "/spider".into(),
            },
        );
        assert_eq!(v["security"], json!("reality"));
        assert_eq!(v["grpcSettings"]["serviceName"], json!("svc"));
        assert_eq!(v["realitySettings"]["publicKey"], json!("pk"));
        assert_eq!(v["realitySettings"]["fingerprint"], json!("firefox"));
        assert_eq!(v["realitySettings"]["spiderX"], json!("/spider"));
    }

    #[test]
    fn singbox_omits_transport_for_tcp_and_tls_when_off() {
        assert!(singbox_transport(&Transport::Tcp).is_none());
        assert!(singbox_tls(&Security::None).is_none());
    }

    #[test]
    fn singbox_reality_nests_under_tls() {
        let tls = singbox_tls(&Security::Reality {
            public_key: "pk".into(),
            short_id: "ab".into(),
            server_name: "sni".into(),
            fingerprint: "firefox".into(),
            spider_x: "/".into(),
        })
        .expect("reality must produce a tls block");
        assert_eq!(tls["enabled"], json!(true));
        assert_eq!(tls["reality"]["public_key"], json!("pk"));
        assert_eq!(tls["reality"]["short_id"], json!("ab"));
        assert_eq!(tls["utls"]["fingerprint"], json!("firefox"));
    }
}

#[cfg(test)]
mod rule_tests {
    use super::*;
    use crate::domain::node::{Node, Protocol};
    use crate::domain::ruleset::{RuleSet, RuleSetSource};
    use crate::domain::routing::{RuleAction, RoutingRule};
    use crate::engine::Endpoints;

    fn rule(id: &str, matcher: RuleMatcher, action: RuleAction, enabled: bool) -> RoutingRule {
        RoutingRule {
            id: id.into(),
            name: id.into(),
            enabled,
            matcher,
            action,
        }
    }

    /// A node the renderers never look at; only the routing table matters here.
    fn node() -> Node {
        Node {
            id: "n1".into(),
            name: "测试".into(),
            server: "example.com".into(),
            port: 443,
            protocol: Protocol::Trojan {
                password: "pw".into(),
            },
            transport: Transport::Tcp,
            security: Security::None,
            tags: vec![],
        }
    }

    fn input<'a>(node: &'a Node, rules: &'a [RoutingRule]) -> RenderInput<'a> {
        let mut input = RenderInput::new(node, Endpoints::new(7890, 9090).unwrap());
        input.rules = rules;
        input
    }

    /// A loaded rule set with a fixed id, so a rule can point at it by name.
    fn loaded(id: &str, entries: Vec<SetEntry>) -> LoadedRuleSet {
        LoadedRuleSet {
            set: RuleSet {
                id: id.into(),
                name: id.into(),
                enabled: true,
                source: RuleSetSource::Local,
                url: String::new(),
                update_interval_minutes: 0,
                updated_at_ms: None,
                entry_count: entries.len() as u32,
                last_error: None,
            },
            entries,
        }
    }

    /// Global and direct replace the user's table with one catch-all rule; a
    /// regression here silently sends traffic the wrong way, so pin it.
    #[test]
    fn global_mode_is_one_catch_all_rule_to_the_proxy() {
        let node = node();
        let rules = sample();
        let mut global = input(&node, &rules);
        global.mode = ProxyMode::Global;
        assert_eq!(
            xray_routing_rules(&global),
            vec![json!({ "type": "field", "network": "tcp,udp", "outboundTag": "proxy" })]
        );
        assert_eq!(singbox_routing_rules(&global), vec![json!({ "outbound": "proxy" })]);

        let mut direct = input(&node, &rules);
        direct.mode = ProxyMode::Direct;
        assert_eq!(
            xray_routing_rules(&direct)[0]["outboundTag"],
            json!("direct")
        );
        assert_eq!(
            singbox_routing_rules(&direct),
            vec![json!({ "outbound": "direct" })]
        );
    }

    /// Switching modes must not eat the user's table: rule mode still renders it.
    #[test]
    fn rule_mode_still_renders_the_users_table() {
        let node = node();
        let rules = sample();
        let mut routed = input(&node, &rules);
        routed.mode = ProxyMode::Rule;
        let rendered = xray_routing_rules(&routed);
        assert_eq!(rendered.len(), 2, "the disabled rule stays out");
        assert_eq!(rendered[0]["outboundTag"], json!("direct"));
    }

    fn sample() -> Vec<RoutingRule> {
        vec![
            rule(
                "direct-lan",
                RuleMatcher::IpCidr {
                    value: "192.168.0.0/16".into(),
                },
                RuleAction::Direct,
                true,
            ),
            rule(
                "block-ads",
                RuleMatcher::DomainSuffix {
                    value: "ads.example.com".into(),
                },
                RuleAction::Block,
                true,
            ),
            rule(
                "off",
                RuleMatcher::DomainKeyword {
                    value: "ignored".into(),
                },
                RuleAction::Proxy,
                false,
            ),
        ]
    }

    #[test]
    fn xray_rules_use_field_shape_and_skip_disabled() {
        let node = node();
        let rules = xray_route_rules(&input(&node, &sample()));
        assert_eq!(rules.len(), 2, "disabled rules must not be rendered");
        assert_eq!(rules[0]["type"], json!("field"));
        assert_eq!(rules[0]["ip"][0], json!("192.168.0.0/16"));
        assert_eq!(rules[0]["outboundTag"], json!("direct"));
        assert_eq!(rules[1]["domain"][0], json!("domain:ads.example.com"));
        assert_eq!(rules[1]["outboundTag"], json!("block"));
    }

    #[test]
    fn singbox_block_is_a_reject_action_not_an_outbound() {
        let node = node();
        let rules = singbox_route_rules(&input(&node, &sample()));
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0]["ip_cidr"][0], json!("192.168.0.0/16"));
        assert_eq!(rules[0]["outbound"], json!("direct"));
        assert_eq!(rules[1]["domain_suffix"][0], json!("ads.example.com"));
        assert_eq!(rules[1]["action"], json!("reject"));
        assert!(
            rules[1].get("outbound").is_none(),
            "a reject rule must not name an outbound"
        );
    }

    #[test]
    fn xray_port_is_a_string_and_singbox_port_is_a_number() {
        let node = node();
        let rules = vec![rule(
            "p",
            RuleMatcher::Port { value: 443 },
            RuleAction::Proxy,
            true,
        )];
        let shared = input(&node, &rules);
        assert_eq!(xray_route_rules(&shared)[0]["port"], json!("443"));
        assert_eq!(singbox_route_rules(&shared)[0]["port"][0], json!(443));
    }

    #[test]
    fn geoip_is_prefixed_for_xray_and_bare_for_singbox() {
        let node = node();
        let rules = vec![rule(
            "cn",
            RuleMatcher::GeoIp {
                value: "cn".into(),
            },
            RuleAction::Direct,
            true,
        )];
        let shared = input(&node, &rules);
        assert_eq!(xray_route_rules(&shared)[0]["ip"][0], json!("geoip:cn"));
        assert_eq!(singbox_route_rules(&shared)[0]["geoip"][0], json!("cn"));
    }

    #[test]
    fn a_rule_set_becomes_two_xray_rules_when_it_mixes_kinds() {
        let node = node();
        let rules = vec![rule(
            "ads",
            RuleMatcher::RuleSet {
                value: "rs-1".into(),
            },
            RuleAction::Block,
            true,
        )];
        let loaded = loaded(
            "rs-1",
            vec![
                SetEntry::DomainSuffix {
                    value: "ads.example".into(),
                },
                SetEntry::IpCidr {
                    value: "10.0.0.0/8".into(),
                },
            ],
        );
        let mut shared = input(&node, &rules);
        shared.rule_sets = std::slice::from_ref(&loaded);

        let rendered = xray_route_rules(&shared);
        assert_eq!(
            rendered.len(),
            2,
            "Xray ANDs the fields of one rule, so a mixed set needs two rules"
        );
        assert_eq!(rendered[0]["domain"][0], json!("domain:ads.example"));
        assert_eq!(rendered[1]["ip"][0], json!("10.0.0.0/8"));
        assert_eq!(rendered[1]["outboundTag"], json!("block"));
    }

    #[test]
    fn a_rule_pointing_at_a_missing_set_is_dropped() {
        let node = node();
        let rules = vec![rule(
            "gone",
            RuleMatcher::RuleSet {
                value: "rs-missing".into(),
            },
            RuleAction::Proxy,
            true,
        )];
        let shared = input(&node, &rules);
        assert!(
            xray_route_rules(&shared).is_empty(),
            "an empty matcher would match everything, so the rule must vanish"
        );
    }

    #[test]
    fn singbox_references_a_rule_set_file_when_it_has_somewhere_to_write() {
        let node = node();
        let rules = vec![rule(
            "ads",
            RuleMatcher::RuleSet {
                value: "rs-1".into(),
            },
            RuleAction::Block,
            true,
        )];
        let loaded = loaded(
            "rs-1",
            vec![SetEntry::DomainSuffix {
                value: "ads.example".into(),
            }],
        );
        let dir = std::path::Path::new("C:/tmp/rule-sets");
        let mut shared = input(&node, &rules);
        shared.rule_sets = std::slice::from_ref(&loaded);
        shared.rule_set_dir = Some(dir);

        let rendered = singbox_route_rules(&shared);
        assert_eq!(rendered[0]["rule_set"][0], json!("rs:rs-1"));
        assert_eq!(rendered[0]["action"], json!("reject"));

        let declared = singbox_rule_set_declarations(&shared);
        assert_eq!(declared.len(), 1);
        assert_eq!(declared[0]["format"], json!("source"));
        assert!(
            declared[0]["path"]
                .as_str()
                .unwrap()
                .ends_with("rs-1.json")
        );
    }

    #[test]
    fn an_empty_rule_set_is_skipped_by_both_sides() {
        // Declaring a set with no file and referencing it anyway is a config
        // error in sing-box; skipping only one side would break the kernel.
        let node = node();
        let rules = vec![rule(
            "ads",
            RuleMatcher::RuleSet {
                value: "rs-1".into(),
            },
            RuleAction::Block,
            true,
        )];
        let loaded = loaded("rs-1", Vec::new());
        let mut shared = input(&node, &rules);
        shared.rule_sets = std::slice::from_ref(&loaded);
        shared.rule_set_dir = Some(std::path::Path::new("C:/tmp/rule-sets"));

        assert!(singbox_route_rules(&shared).is_empty());
        assert!(singbox_rule_set_declarations(&shared).is_empty());
    }

    #[test]
    fn the_singbox_rule_set_file_is_source_format_json() {
        // The real kernel rejects a line list here, so pin the document shape.
        let entries = vec![
            SetEntry::DomainSuffix {
                value: "example.org".into(),
            },
            SetEntry::DomainKeyword {
                value: "adserver".into(),
            },
            SetEntry::DomainFull {
                value: "foo.example.net".into(),
            },
            SetEntry::IpCidr {
                value: "203.0.113.0/24".into(),
            },
        ];
        let text = singbox_rule_set_source(&entries);
        let parsed: Value = serde_json::from_str(&text).expect("source format must be JSON");
        assert_eq!(parsed["version"], json!(1));
        let rules = parsed["rules"].as_array().expect("rules array");
        assert_eq!(rules.len(), 4, "one object per kind: fields in one rule are ANDed");
        assert_eq!(rules[0]["domain_suffix"], json!(["example.org"]));
        assert_eq!(rules[1]["domain_keyword"], json!(["adserver"]));
        assert_eq!(rules[2]["domain"], json!(["foo.example.net"]));
        assert_eq!(rules[3]["ip_cidr"], json!(["203.0.113.0/24"]));
    }

    #[test]
    fn singbox_inlines_a_rule_set_when_it_has_no_directory() {
        let node = node();
        let rules = vec![rule(
            "ads",
            RuleMatcher::RuleSet {
                value: "rs-1".into(),
            },
            RuleAction::Proxy,
            true,
        )];
        let loaded = loaded(
            "rs-1",
            vec![SetEntry::DomainFull {
                value: "ads.example".into(),
            }],
        );
        let mut shared = input(&node, &rules);
        shared.rule_sets = std::slice::from_ref(&loaded);

        let rendered = singbox_route_rules(&shared);
        assert_eq!(rendered[0]["domain"][0], json!("ads.example"));
        assert_eq!(rendered[0]["outbound"], json!("proxy"));
        assert!(singbox_rule_set_declarations(&shared).is_empty());
    }
}

#[cfg(test)]
mod insecure_tests {
    use super::*;
    use crate::domain::{Security, Transport};

    fn insecure() -> Security {
        Security::Tls {
            sni: Some("a.example.com".into()),
            alpn: vec![],
            allow_insecure: true,
            pinned_cert_sha256: vec![],
        }
    }

    fn pinned() -> Security {
        Security::Tls {
            sni: Some("a.example.com".into()),
            alpn: vec![],
            allow_insecure: false,
            pinned_cert_sha256: vec!["93c559a5d6605453".into()],
        }
    }

    #[test]
    fn xray_never_emits_allow_insecure() {
        // Xray 26 removed the key and refuses to start when it appears, so the
        // whole config dies — not just this node.
        let stream = xray_stream_settings(&Transport::Tcp, &insecure());
        assert!(stream["tlsSettings"].get("allowInsecure").is_none());
    }

    #[test]
    fn xray_pins_the_certificate_when_one_is_given() {
        let stream = xray_stream_settings(&Transport::Tcp, &pinned());
        assert_eq!(
            stream["tlsSettings"]["pinnedPeerCertSha256"],
            serde_json::json!("93c559a5d6605453")
        );
        let plain = xray_stream_settings(&Transport::Tcp, &insecure());
        assert!(plain["tlsSettings"].get("pinnedPeerCertSha256").is_none());
    }

    #[test]
    fn singbox_skips_verification_only_when_asked() {
        let tls = singbox_tls(&insecure()).expect("tls object");
        assert_eq!(tls["insecure"], serde_json::json!(true));
        let strict = singbox_tls(&Security::Tls {
            sni: None,
            alpn: vec![],
            allow_insecure: false,
            pinned_cert_sha256: vec![],
        })
        .expect("tls object");
        assert!(strict.get("insecure").is_none());
    }
}
