//! Engine layer — everything that turns a [`Node`] into a kernel configuration.
//!
//! M1 scope is deliberately *pure*: these functions take a node plus a port
//! plan and return a `serde_json::Value`. Nothing here spawns a process, opens
//! a socket, or touches the system proxy. Two reasons:
//!
//! 1. It is fully testable offline, with no kernel binaries present.
//! 2. A half-finished build must never fight with a proxy that is already
//!    running on the machine (that was an explicit requirement).
//!
//! Process lifecycle, readiness probing and the system proxy live in later
//! milestones and are layered on top of this module, not inside it.

pub mod geo;
mod job;
mod render;
// Crate-visible so command tests can reuse the runtime fakes instead of
// inventing a second set of them.
pub(crate) mod runtime;
mod singbox;
pub mod stats;
pub mod traffic_log;
mod xray;

pub use runtime::{EngineBinary, EngineRuntime, EngineStatus, LogDelta};
pub use traffic_log::{TrafficLog, TrafficPoint};
pub use singbox::SingBoxEngine;
pub use xray::XrayEngine;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::domain::group::ProxyGroup;
use crate::domain::node::Node;
use crate::domain::ruleset::{RuleSet, SetEntry};
use crate::domain::settings::{ProxyMode, Settings};
use crate::domain::routing::RoutingRule;
use crate::error::{AppError, AppResult};

/// A rule set together with its parsed entries.
///
/// The entries are loaded once per run (not per rule) because one set can be
/// referenced by several rules, and a provider file can hold tens of thousands
/// of lines.
#[derive(Debug, Clone)]
pub struct LoadedRuleSet {
    pub set: RuleSet,
    pub entries: Vec<SetEntry>,
}

impl LoadedRuleSet {
    /// Look a set up by id, ignoring disabled ones: a disabled set must not
    /// match anything, otherwise turning it off would have no effect.
    pub fn find<'a>(sets: &'a [LoadedRuleSet], id: &str) -> Option<&'a LoadedRuleSet> {
        sets.iter()
            .find(|loaded| loaded.set.id == id && loaded.set.enabled)
    }
}

/// Which kernel renders and runs the configuration.
// `Hash` is not for rendering: the runtime keys its per-kernel binary overrides
// by this value, so one map serves both kernels. Kept as a plain comment so it
// does not travel into the generated TypeScript contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum EngineKind {
    Xray,
    SingBox,
}

impl EngineKind {
    pub fn engine(self) -> Box<dyn Engine> {
        match self {
            EngineKind::Xray => Box::new(XrayEngine),
            EngineKind::SingBox => Box::new(SingBoxEngine),
        }
    }

    /// Stable identifier used for config and log file names.
    pub fn as_str(self) -> &'static str {
        match self {
            EngineKind::Xray => "xray",
            EngineKind::SingBox => "sing-box",
        }
    }

    /// Parse the persisted spelling back. Unknown values mean "the setting is
    /// from a newer build" — the caller falls back to doing nothing.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "xray" => Some(EngineKind::Xray),
            "sing-box" => Some(EngineKind::SingBox),
            _ => None,
        }
    }

    pub fn all() -> &'static [EngineKind] {
        &[EngineKind::Xray, EngineKind::SingBox]
    }
}

/// Ports handed to a kernel run. Kept separate from `Settings` because these
/// are *resolved* values (a port may be bumped when the preferred one is busy),
/// whereas settings hold the user's intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Endpoints {
    /// Local mixed inbound (SOCKS5 + HTTP on the same port).
    pub mixed_port: u16,
    /// Statistics / control endpoint: Xray expvar HTTP, or sing-box Clash API.
    pub api_port: u16,
}

impl Endpoints {
    pub fn new(mixed_port: u16, api_port: u16) -> AppResult<Self> {
        if mixed_port < 1024 {
            return Err(AppError::InvalidInput(format!(
                "入站端口 {mixed_port} 低于 1024，需要管理员权限才能监听"
            )));
        }
        if api_port < 1024 {
            return Err(AppError::InvalidInput(format!(
                "控制端口 {api_port} 低于 1024"
            )));
        }
        if mixed_port == api_port {
            return Err(AppError::InvalidInput(
                "入站端口与控制端口不能相同".to_string(),
            ));
        }
        Ok(Self {
            mixed_port,
            api_port,
        })
    }
}

/// Where a running kernel publishes statistics.
///
/// The two kernels speak different protocols — Xray publishes cumulative byte
/// counters over expvar HTTP, sing-box publishes instantaneous rates over a
/// Clash API websocket. The abstraction keeps the *transport* in the enum and
/// leaves normalisation (cumulative vs. rate) to the stats reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatsChannel {
    /// `GET http://127.0.0.1:{port}/debug/vars` — cumulative counters.
    XrayExpVar { port: u16 },
    /// Clash API at `127.0.0.1:{port}` — `/traffic` websocket gives rates.
    ClashApi { port: u16 },
}

/// Verbosity lives in the domain (it is persisted with the settings); the
/// renderers translate it into each kernel's own spelling.
pub use crate::domain::settings::LogLevel;

/// Everything a renderer needs. Borrowed so callers keep ownership of the node.
pub struct RenderInput<'a> {
    pub node: &'a Node,
    pub endpoints: Endpoints,
    pub log_level: LogLevel,
    /// Enabled rules are rendered into `routing.rules` / `route.rules`.
    pub rules: &'a [RoutingRule],
    /// How the routing table is used: `rule` honours it, `global` and `direct`
    /// bypass it with a single catch-all. This is the difference between the
    /// three modes a proxy client offers, so it must reach the renderer.
    pub mode: ProxyMode,
    /// Listen on all interfaces so phones and other machines can use this
    /// client as their gateway. Off by default: an open inbound port on a
    /// laptop that joins random Wi-Fi networks is a real exposure.
    pub allow_lan: bool,
    /// Every node the user owns. Groups refer to members by id, so the
    /// renderer needs the catalogue to resolve them into outbounds.
    pub nodes: &'a [Node],
    /// Proxy groups. Rules may target them, and sing-box renders each one as
    /// a real `selector`/`urltest` outbound.
    pub groups: &'a [ProxyGroup],
    /// Rule sets, already parsed, so a rule can expand into their entries
    /// without the renderer touching the disk.
    pub rule_sets: &'a [LoadedRuleSet],
    /// Where sing-box rule-set files are written for this run. Xray has no
    /// rule-set concept and always inlines; `None` makes sing-box inline too,
    /// which keeps pure rendering (and its tests) free of any path.
    pub rule_set_dir: Option<&'a std::path::Path>,
    /// What a `urltest` group probes. Only sing-box has that outbound type, but
    /// the value is a setting, so it travels with the rest of the run options.
    pub probe_url: String,
    /// Resolvers the kernel should use instead of the system ones. Empty means
    /// "leave DNS alone", which is also what a fresh install gets.
    pub dns_servers: Vec<crate::domain::settings::DnsEntry>,
    /// Multiplex every connection over one link to the server. Off by default,
    /// because it is a trade: fewer handshakes on a lossy link, a queue on a
    /// clean one.
    pub mux_enabled: bool,
    /// Streams per multiplexed link; only read when [`RenderInput::mux_enabled`].
    pub mux_concurrency: u16,
    /// Fragment the TLS ClientHello into small pieces. Only Xray renders it,
    /// and only for TLS nodes; see [`RenderInput::fragment`].
    pub fragment_enabled: bool,
    /// Piece-size range (`a-b` bytes) for [`RenderInput::fragment`].
    pub fragment_length: String,
    /// Delay range (`a-b` milliseconds) for [`RenderInput::fragment`].
    pub fragment_interval: String,
}

impl<'a> RenderInput<'a> {
    pub fn new(node: &'a Node, endpoints: Endpoints) -> Self {
        Self {
            node,
            endpoints,
            log_level: LogLevel::Warn,
            rules: &[],
            mode: ProxyMode::Rule,
            allow_lan: false,
            nodes: &[],
            groups: &[],
            rule_sets: &[],
            rule_set_dir: None,
            probe_url: crate::domain::settings::DEFAULT_TEST_URL.to_string(),
            dns_servers: Vec::new(),
            mux_enabled: false,
            mux_concurrency: 8,
            fragment_enabled: false,
            fragment_length: "100-200".to_string(),
            fragment_interval: "10-20".to_string(),
        }
    }

    /// Apply the settings-and-routing side of a run in one step.
    pub fn with_options(mut self, options: RunOptions<'a>) -> Self {
        self.rules = options.rules;
        self.mode = options.mode;
        self.allow_lan = options.allow_lan;
        self.nodes = options.nodes;
        self.groups = options.groups;
        self.rule_sets = options.rule_sets;
        self.log_level = options.log_level;
        self.probe_url = options.probe_url;
        self.dns_servers = options.dns_servers;
        self.mux_enabled = options.mux_enabled;
        self.mux_concurrency = options.mux_concurrency;
        self.fragment_enabled = options.fragment_enabled;
        self.fragment_length = options.fragment_length;
        self.fragment_interval = options.fragment_interval;
        self
    }

    /// Tell the renderer where per-run rule-set files live (sing-box only).
    pub fn with_rule_set_dir(mut self, dir: &'a std::path::Path) -> Self {
        self.rule_set_dir = Some(dir);
        self
    }

    /// The node a group currently resolves to, if it is still in the catalogue.
    ///
    /// A group can outlive the node it points at (the node was deleted, or a
    /// subscription replaced it), so this returns `Option` and the renderers
    /// simply skip a group they cannot resolve instead of emitting a broken
    /// outbound.
    pub fn member_node(&self, group: &ProxyGroup) -> Option<&'a Node> {
        let wanted = group.chosen()?;
        self.nodes.iter().find(|node| node.id == wanted)
    }

    /// Enabled groups, in the order the user arranged them.
    pub fn active_groups(&self) -> impl Iterator<Item = &ProxyGroup> {
        self.groups.iter().filter(|group| group.enabled)
    }

    /// Inbound listen address, honouring the LAN-sharing opt-in.
    pub fn listen_address(&self) -> &'static str {
        if self.allow_lan { "0.0.0.0" } else { "127.0.0.1" }
    }

    /// The stream count one multiplexed outbound may carry, or `None` when the
    /// user left multiplexing off.
    ///
    /// Both kernels distinguish "no mux block" from `enabled: false`, so the
    /// renderers take this one value and omit the block entirely when it is
    /// `None`. Keeping the decision here means the two renderers cannot spell
    /// the off case differently.
    pub fn mux_streams(&self) -> Option<u16> {
        if self.mux_enabled { Some(self.mux_concurrency) } else { None }
    }

    /// The fragment `(length, interval)` the Xray renderer should emit, or
    /// `None` when the user left the switch off.
    ///
    /// This is the single place that decides whether the feature is on: the
    /// renderer takes this value instead of testing `fragment_enabled` itself,
    /// so the settings page and the emitted config cannot disagree. The values
    /// are returned as the user wrote them (`a-b`) because that is exactly the
    /// text Xray's `tlsSettings.fragment` expects.
    pub fn fragment(&self) -> Option<(String, String)> {
        if self.fragment_enabled {
            Some((self.fragment_length.clone(), self.fragment_interval.clone()))
        } else {
            None
        }
    }
}

/// Everything besides the node itself that shapes a rendered config.
///
/// Kept separate from [`RenderInput`] so the command layer can build one from
/// the current settings without knowing what a renderer needs internally.
#[derive(Clone)]
pub struct RunOptions<'a> {
    pub rules: &'a [RoutingRule],
    pub mode: ProxyMode,
    pub allow_lan: bool,
    pub nodes: &'a [Node],
    pub groups: &'a [ProxyGroup],
    pub rule_sets: &'a [LoadedRuleSet],
    /// Persisted verbosity, so the kernel log matches the setting the user sees.
    pub log_level: LogLevel,
    /// Probe target for `urltest` groups, taken from the settings.
    pub probe_url: String,
    /// Custom resolvers, taken from the settings.
    pub dns_servers: Vec<crate::domain::settings::DnsEntry>,
    /// The settings text the resolvers were parsed from. The UI compares it
    /// with the saved value to tell an applied change from a pending one, so it
    /// must survive exactly as the user wrote it — the parsed list cannot
    /// round-trip whitespace or a trailing comma.
    pub dns_servers_raw: String,
    /// Multiplex every connection over one link to the server.
    pub mux_enabled: bool,
    /// Streams per multiplexed link; only read when [`RunOptions::mux_enabled`].
    pub mux_concurrency: u16,
    /// Fragment the TLS ClientHello. Only Xray renders it; see
    /// [`RenderInput::fragment`].
    pub fragment_enabled: bool,
    /// Piece-size range (`a-b` bytes) for [`RenderInput::fragment`].
    pub fragment_length: String,
    /// Delay range (`a-b` milliseconds) for [`RenderInput::fragment`].
    pub fragment_interval: String,
}

impl<'a> RunOptions<'a> {
    pub fn new() -> Self {
        Self {
            rules: &[],
            mode: ProxyMode::Rule,
            allow_lan: false,
            nodes: &[],
            groups: &[],
            rule_sets: &[],
            log_level: LogLevel::Warn,
            probe_url: crate::domain::settings::DEFAULT_TEST_URL.to_string(),
            dns_servers: Vec::new(),
            dns_servers_raw: String::new(),
            mux_enabled: false,
            mux_concurrency: 8,
            fragment_enabled: false,
            fragment_length: "100-200".to_string(),
            fragment_interval: "10-20".to_string(),
        }
    }

    /// Take the parts of the saved settings that change the generated config.
    pub fn from_settings(settings: &Settings, rules: &'a [RoutingRule]) -> Self {
        Self {
            rules,
            mode: settings.mode,
            allow_lan: settings.allow_lan,
            nodes: &[],
            groups: &[],
            rule_sets: &[],
            log_level: settings.log_level,
            probe_url: settings.test_url.clone(),
            dns_servers: settings.dns_entries().unwrap_or_default(),
            dns_servers_raw: settings.dns_servers.clone(),
            mux_enabled: settings.mux_enabled,
            mux_concurrency: settings.mux_concurrency,
            fragment_enabled: settings.fragment_enabled,
            fragment_length: settings.fragment_length.clone(),
            fragment_interval: settings.fragment_interval.clone(),
        }
    }

    /// Hand the catalogue over so group members can be resolved.
    pub fn with_catalog(mut self, nodes: &'a [Node], groups: &'a [ProxyGroup]) -> Self {
        self.nodes = nodes;
        self.groups = groups;
        self
    }

    /// Hand the parsed rule sets over so `RuleSet` matchers can expand.
    pub fn with_rule_sets(mut self, rule_sets: &'a [LoadedRuleSet]) -> Self {
        self.rule_sets = rule_sets;
        self
    }
}

impl Default for RunOptions<'_> {
    fn default() -> Self {
        Self::new()
    }
}

/// The one interface the rest of the app uses to talk to a kernel.
///
/// Adding a third kernel (mihomo, hysteria, …) means implementing this trait;
/// it must not require touching the UI, the stores, or the command layer.
pub trait Engine: Send + Sync {
    fn kind(&self) -> EngineKind;

    /// Executable file name, resolved against the engine directory at runtime.
    fn binary_name(&self) -> &'static str;

    /// Arguments that run `config_path` in the foreground.
    fn args(&self, config_path: &str) -> Vec<String>;

    /// Build the full config document. Pure: no disk writes, no side effects.
    fn render_config(&self, input: &RenderInput<'_>) -> AppResult<serde_json::Value>;

    fn stats_channel(&self, endpoints: Endpoints) -> StatsChannel;

    /// Reject node/kernel combinations this kernel cannot honour *before*
    /// anything is written to disk or spawned.
    fn validate_node(&self, node: &Node) -> AppResult<()>;
}

/// Shared guard used by both renderers so the error text stays identical.
fn ensure_node_usable(node: &Node) -> AppResult<()> {
    if node.server.trim().is_empty() {
        return Err(AppError::InvalidInput("服务器地址不能为空".to_string()));
    }
    if node.server.contains(char::is_whitespace) {
        return Err(AppError::InvalidInput(
            "服务器地址不能包含空格".to_string(),
        ));
    }
    if node.port == 0 {
        return Err(AppError::InvalidInput("端口必须大于 0".to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_reject_privileged_ports() {
        assert!(Endpoints::new(7890, 9090).is_ok());
        assert!(Endpoints::new(80, 9090).is_err());
        assert!(Endpoints::new(7890, 80).is_err());
    }

    #[test]
    fn endpoints_reject_identical_ports() {
        // The most common real mistake: one port doing double duty.
        assert!(Endpoints::new(7890, 7890).is_err());
    }

    #[test]
    fn every_kind_has_an_engine_with_a_binary() {
        for kind in EngineKind::all() {
            let engine = kind.engine();
            assert_eq!(engine.kind(), *kind);
            assert!(!engine.binary_name().is_empty());
        }
    }
}
