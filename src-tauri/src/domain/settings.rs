use serde::{Deserialize, Serialize};
use specta::Type;

/// Backend-owned settings: everything here must survive an app restart.
/// Pure UI preferences (theme, sidebar state, table density) live in the
/// frontend only — see ARCHITECTURE.md §3.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
// `default` on the struct means a field added in a later version can be missing
// from an older on-disk document without making it unreadable.
#[serde(default)]
pub struct Settings {
    /// Local mixed inbound (HTTP + SOCKS) port.
    pub mixed_port: u16,
    /// HTTP inbound port when HTTP and SOCKS get their own listeners.
    ///
    /// `0` means "the same as [`Settings::mixed_port`]", which is the default
    /// and keeps the single mixed inbound. A non-zero value splits the two:
    /// software that only speaks `socks5://` and a system proxy that must be
    /// an HTTP address can then both be served without a second client.
    #[serde(default)]
    pub http_port: u16,
    /// SOCKS inbound port when the two protocols are split; `0` means "the
    /// same as [`Settings::mixed_port`]", exactly like [`Settings::http_port`].
    #[serde(default)]
    pub socks_port: u16,
    /// Control/API port used by the engine for stats & connections.
    pub api_port: u16,
    pub allow_lan: bool,
    pub mode: ProxyMode,
    /// Whether X-Matrix currently owns the Windows system proxy.
    pub system_proxy: bool,
    /// Write a PAC script and point `AutoConfigURL` at it instead of a fixed
    /// `ProxyServer`. Only meaningful while [`Settings::system_proxy`] is on:
    /// it is the *how*, not a second takeover. Off by default, because a script
    /// runs inside every application that reads the system proxy and the
    /// fixed-server path is the one users expect from a proxy client.
    pub pac_enabled: bool,
    pub auto_start: bool,
    pub language: Language,
    /// Start the last used node as soon as the app comes up. Off by default:
    /// an app that quietly opens a tunnel on launch is a surprise, and this
    /// machine may already have another client doing that.
    pub auto_connect: bool,
    /// The node and kernel that were running when the app last closed. Kept so
    /// "connect on launch" knows what to start, and so the UI can preselect it.
    pub last_node_id: Option<String>,
    /// Persisted as the kernel's own spelling (`xray` / `sing-box`) because the
    /// domain must not depend on the engine module.
    pub last_engine: Option<String>,
    /// Explicit Xray executable; empty means "the one in the engines folder".
    ///
    /// An override exists for portable installs (the kernel sits next to the
    /// app, not under `%APPDATA%`) and for people who already trust a specific
    /// build: they should not have to copy it into our data directory, where a
    /// later install could replace the binary they chose.
    #[serde(default)]
    pub xray_binary: String,
    /// Explicit sing-box executable; empty is the same as
    /// [`Settings::xray_binary`]. Resolved per kernel, because a user may keep
    /// the upstream Xray and a self-built sing-box side by side.
    #[serde(default)]
    pub singbox_binary: String,
    /// How chatty the kernel log should be. `Warn` keeps the file readable;
    /// `Debug` is what you switch to when a node will not connect.
    pub log_level: LogLevel,
    /// Re-test every node this often, in minutes. `0` means "only when I press
    /// the button": an app that starts hammering node endpoints on its own is
    /// something people notice on a metered connection.
    pub auto_test_minutes: u32,
    /// How long one latency probe may take, in milliseconds.
    ///
    /// Three seconds suits a healthy node on a decent line. A satellite link
    /// needs more, and a timeout that calls a working node "unreachable" is
    /// worse than a slow number, so the knob is a setting and not a constant.
    pub test_timeout_ms: u32,
    /// How many nodes "test all" probes at the same time.
    ///
    /// One is honest: parallel handshakes contend for the same uplink and the
    /// numbers then measure the queue rather than the nodes. Dozens of nodes
    /// make that tediously slow, so the knob exists with a low default.
    pub test_concurrency: u32,
    /// WebDAV collection the cloud backup writes into. Empty switches the
    /// feature off entirely: there is no default server to point at, and a
    /// guessed one would be someone else's disk.
    #[serde(default)]
    pub webdav_url: String,
    /// User name for that server. The password deliberately does *not* live
    /// here: it goes to the Windows Credential Manager (`syscreds`), because a
    /// `store.json` that carried it would leak it into every local export —
    /// including the one this feature uploads.
    #[serde(default)]
    pub webdav_username: String,
    /// Send the upload through the running tunnel, for servers that only answer
    /// from outside your ISP. Same trade-off as `subscription_via_proxy`: with
    /// no kernel running the upload refuses instead of silently going direct,
    /// because "backed up" is a claim the user will act on.
    #[serde(default)]
    pub webdav_via_proxy: bool,
    /// Bypass list written into the Windows proxy settings while X-Matrix owns
    /// them. Loopback and RFC1918 are in the default; users behind a corporate
    /// PAC or a local gateway usually need to add one more entry.
    pub proxy_bypass: String,
    /// What "real latency" and a url-test group probe. gstatic's 204 endpoint is
    /// the de-facto standard, but it is blocked on some networks, so the URL has
    /// to be editable or the feature is useless exactly where it is needed.
    pub test_url: String,
    /// `User-Agent` sent when fetching subscriptions. Empty means "our own".
    /// Plenty of providers only serve a Clash UA and answer anything else with
    /// an empty body or a 403, so this is the one header worth exposing.
    pub subscription_user_agent: String,
    /// Fetch subscriptions through the running tunnel instead of directly.
    ///
    /// A provider's panel is often unreachable from the network you need the
    /// subscription for — the classic case is a subscription URL that only
    /// answers from outside your ISP's reach. Off by default: sending your
    /// subscription URL through the tunnel means a broken node also breaks the
    /// refresh, and a failed refresh should never be the reason the app looks
    /// dead.
    pub subscription_via_proxy: bool,
    /// Comma separated resolver IPs. Empty means "let the kernel use the system
    /// resolver". Only literals are accepted: a hostname here would need the
    /// system resolver to resolve the resolver, which is how DNS loops start.
    pub dns_servers: String,
    /// Multiplex several logical connections over one kernel-to-server link.
    /// Off by default: it helps on lossy or heavily-shaped links and hurts on a
    /// clean one, so the user opts in rather than inheriting a surprise.
    #[serde(default)]
    pub mux_enabled: bool,
    /// How many streams one multiplexed link may carry. Only meaningful when
    /// `mux_enabled`; both kernels treat it as an upper bound, not a promise.
    #[serde(default = "default_mux_concurrency")]
    pub mux_concurrency: u16,
    /// Split the TLS ClientHello into small pieces before it leaves the machine.
    /// Xray renders this as `tlsSettings.fragment`; sing-box has no equivalent,
    /// so the setting is advertised as Xray-only. Off by default: it adds a
    /// little latency and is only useful against a middlebox that fingerprints
    /// the handshake.
    #[serde(default)]
    pub fragment_enabled: bool,
    /// Piece size for [`Settings::fragment_enabled`], written as an `a-b` range
    /// of bytes such as `100-200` — Xray's `tlsSettings.fragment.length` takes
    /// exactly this text. Only meaningful while the feature is enabled.
    #[serde(default = "default_fragment_length")]
    pub fragment_length: String,
    /// Delay between pieces for [`Settings::fragment_enabled`], written as an
    /// `a-b` range of milliseconds such as `10-20` — Xray's
    /// `tlsSettings.fragment.interval`. Only meaningful while enabled.
    #[serde(default = "default_fragment_interval")]
    pub fragment_interval: String,
    /// Come up with the window hidden, tray only. For people who launch this at
    /// login and never want to see it.
    pub start_minimized: bool,
    /// What the window's X button does. `true` (the default) hides to tray and
    /// keeps the tunnel up; `false` quits and restores the system proxy, which
    /// is what someone who thinks of X as "close the program" expects.
    pub close_to_tray: bool,
    /// 需要管理员权限的功能（TUN、安装服务）要不要先提示一次。默认开：
    /// 提权必须由用户点「是」，应用不能悄悄换个令牌重来。
    #[serde(default = "default_prompt_elevation")]
    pub prompt_elevation: bool,
    /// 窗口收进托盘时把内存目标降到低、让界面暂停刷新。默认关：
    /// 这是「省一点」的开关，不是所有人都想要界面在同一秒里失去响应。
    #[serde(default)]
    pub lite_mode: bool,
    /// Chord that starts or stops the tunnel from anywhere on the desktop,
    /// spelled the way the app spells it back (`Ctrl+Shift+X`). Empty switches
    /// the shortcut off: a chord nobody can press still looks registered in the
    /// settings page, which is worse than saying "off".
    #[serde(default = "default_hotkey_toggle")]
    pub hotkey_toggle: String,
    /// Chord that shows or hides the window, same spelling and same rule.
    #[serde(default = "default_hotkey_window")]
    pub hotkey_window: String,
}

/// Prompting before elevation is the default: a silent `runas` would replace a
/// running session with a UAC dialog nobody asked for.
fn default_prompt_elevation() -> bool {
    true
}

/// The chords a fresh install starts with — the ones the app shipped since the
/// first build, so an existing user sees no change after upgrading.
fn default_hotkey_toggle() -> String {
    crate::hotkeys::DEFAULT_TOGGLE.to_string()
}

fn default_hotkey_window() -> String {
    crate::hotkeys::DEFAULT_WINDOW.to_string()
}

/// The probe used by every "is this node actually reachable" check. Kept in the
/// domain because it is a persisted default, and referenced by `net::latency`.
pub const DEFAULT_TEST_URL: &str = "http://www.gstatic.com/generate_204";

/// Three seconds: long enough for a slow-but-working node, short enough that a
/// dead one does not hold the whole "test all" run hostage.
pub const DEFAULT_TEST_TIMEOUT_MS: u32 = 3_000;

/// Two at a time. Sequential is the honest default, but a list of thirty nodes
/// at three seconds each is ninety seconds of watching nothing happen.
pub const DEFAULT_TEST_CONCURRENCY: u32 = 2;

/// Eight streams per multiplexed link when the user turns it on.
///
/// Both kernels default to a similar number, and it is a starting point rather
/// than a target: too few wastes the handshake that mux was meant to amortise,
/// too many lets one lossy link stall every stream at once.
fn default_mux_concurrency() -> u16 {
    8
}

/// V2rayN's classic piece size. Xray expects this `a-b` text unchanged in
/// `tlsSettings.fragment.length`; the range is parsed (not normalised) at save
/// time so the kernel and the settings page see the same string.
fn default_fragment_length() -> String {
    "100-200".to_string()
}

/// Ten to twenty milliseconds between pieces, matching the usual client
/// default. Parsed like [`default_fragment_length`].
fn default_fragment_interval() -> String {
    "10-20".to_string()
}

/// Parse one `a-b` fragment range: two positive integers separated by a single
/// dash, surrounding whitespace allowed.
///
/// Xray rejects the whole config when a fragment range does not parse, so this
/// is the one parser both `validate()` and the tests rely on.
fn parse_positive_range(raw: &str) -> Option<(u32, u32)> {
    let (low, high) = raw.split_once('-')?;
    let low = low.trim().parse::<u32>().ok()?;
    let high = high.trim().parse::<u32>().ok()?;
    if low == 0 || high == 0 || low > high {
        return None;
    }
    Some((low, high))
}

/// One resolver the user asked for.
///
/// Three spellings are accepted, all of them IP literals: `1.1.1.1` (plain
/// UDP), `tls://1.1.1.1` (DNS-over-TLS) and `https://1.1.1.1/dns-query`
/// (DNS-over-HTTPS). A *hostname* such as `https://dns.google/dns-query` is
/// refused on purpose: resolving that name needs a resolver, and picking one
/// behind the user's back is how DNS loops and "my DNS ignores my settings"
/// bug reports start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DnsEntry {
    Plain(String),
    Doh { host: String, path: String },
    Dot { host: String },
}

impl DnsEntry {
    pub fn parse(raw: &str) -> Result<Self, String> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err("dns_servers 里有空项".into());
        }
        if let Some(rest) = raw.strip_prefix("https://") {
            let (host, path) = match rest.find('/') {
                Some(index) => (&rest[..index], &rest[index..]),
                None => (rest, "/dns-query"),
            };
            return Self::with_ip_host(DnsEntry::Doh {
                host: host.to_string(),
                path: path.to_string(),
            }, raw);
        }
        if let Some(rest) = raw.strip_prefix("tls://") {
            return Self::with_ip_host(
                DnsEntry::Dot {
                    host: rest.to_string(),
                },
                raw,
            );
        }
        Self::with_ip_host(DnsEntry::Plain(raw.to_string()), raw)
    }

    fn with_ip_host(entry: DnsEntry, raw: &str) -> Result<Self, String> {
        let host = match &entry {
            DnsEntry::Plain(host) | DnsEntry::Dot { host } => host,
            DnsEntry::Doh { host, .. } => host,
        };
        if host.parse::<std::net::IpAddr>().is_err() {
            return Err(format!(
                "dns_servers 里只能用 IP 字面量（域名要先把域名解析出来，正是 DNS 要干的事）：{raw}"
            ));
        }
        Ok(entry)
    }

    /// How Xray spells this resolver in `dns.servers`.
    pub fn xray_address(&self) -> String {
        match self {
            DnsEntry::Plain(host) => host.clone(),
            DnsEntry::Doh { host, path } => format!("https://{host}{path}"),
            DnsEntry::Dot { host } => format!("tls://{host}"),
        }
    }
}

/// Kernel log verbosity. Lives in the domain because it is persisted, and the
/// renderers translate it into each kernel's own spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Warn,
    Info,
    Debug,
}

impl LogLevel {
    /// Xray accepts `none|error|warning|info|debug`.
    pub fn as_xray(self) -> &'static str {
        match self {
            LogLevel::Warn => "warning",
            LogLevel::Info => "info",
            LogLevel::Debug => "debug",
        }
    }

    /// sing-box accepts `trace|debug|info|warn|error|fatal|panic`.
    pub fn as_singbox(self) -> &'static str {
        match self {
            LogLevel::Warn => "warn",
            LogLevel::Info => "info",
            LogLevel::Debug => "debug",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum ProxyMode {
    Rule,
    Global,
    Direct,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Zh,
    // Traditional Chinese with Taiwan wording. The rename is explicit because
    // `rename_all` would spell it `zhtw`, and the front end keys dictionaries
    // and `localStorage` by `zh-tw`.
    #[serde(rename = "zh-tw")]
    ZhTw,
    En,
}


impl Default for Settings {
    fn default() -> Self {
        Self {
            mixed_port: 7890,
            http_port: 0,
            socks_port: 0,
            api_port: 9090,
            allow_lan: false,
            mode: ProxyMode::Rule,
            system_proxy: false,
            pac_enabled: false,
            auto_start: false,
            language: Language::Zh,
            auto_connect: false,
            last_node_id: None,
            last_engine: None,
            xray_binary: String::new(),
            singbox_binary: String::new(),
            log_level: LogLevel::Warn,
            auto_test_minutes: 0,
            test_timeout_ms: DEFAULT_TEST_TIMEOUT_MS,
            test_concurrency: DEFAULT_TEST_CONCURRENCY,
            proxy_bypass: crate::sysproxy::DEFAULT_BYPASS.to_string(),
            test_url: DEFAULT_TEST_URL.to_string(),
            subscription_user_agent: String::new(),
            subscription_via_proxy: false,
            webdav_url: String::new(),
            webdav_username: String::new(),
            webdav_via_proxy: false,
            dns_servers: String::new(),
            mux_enabled: false,
            mux_concurrency: default_mux_concurrency(),
            fragment_enabled: false,
            fragment_length: default_fragment_length(),
            fragment_interval: default_fragment_interval(),
            start_minimized: false,
            close_to_tray: true,
            prompt_elevation: true,
            lite_mode: false,
            hotkey_toggle: default_hotkey_toggle(),
            hotkey_window: default_hotkey_window(),
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if self.mixed_port < 1024 {
            return Err("mixed_port must be >= 1024".into());
        }
        if self.api_port < 1024 {
            return Err("api_port must be >= 1024".into());
        }
        if self.mixed_port == self.api_port {
            return Err("mixed_port and api_port must differ".into());
        }
        // A split is optional, so `0` is not a port: it means "follow
        // mixed_port" and is only checked once resolved. Anything else has to
        // be a real port the kernel can bind without elevation.
        if self.http_port != 0 && self.http_port < 1024 {
            return Err("http_port must be >= 1024 (or 0 to follow mixed_port)".into());
        }
        if self.socks_port != 0 && self.socks_port < 1024 {
            return Err("socks_port must be >= 1024 (or 0 to follow mixed_port)".into());
        }
        // The same resolution `Endpoints::new` performs, so a document that
        // saves here cannot fail the engine's own check a moment later. Two
        // equal effective ports are not an error: that is simply "not split",
        // served by one mixed inbound like before.
        let http = if self.http_port == 0 { self.mixed_port } else { self.http_port };
        let socks = if self.socks_port == 0 { self.mixed_port } else { self.socks_port };
        if http == self.api_port {
            return Err("http_port and api_port must differ".into());
        }
        if socks == self.api_port {
            return Err("socks_port and api_port must differ".into());
        }
        let test_url = self.test_url.trim();
        if test_url.is_empty() {
            return Err("test_url must not be empty".into());
        }
        // The probe goes through our own loopback proxy, so it cannot be https:
        // adding a TLS stack to the client just to measure a round trip would be
        // a second implementation of what the kernel already does.
        if !test_url.starts_with("http://") {
            return Err("test_url must start with http://".into());
        }
        // A probe shorter than a second measures the clock, not the network;
        // anything past twenty seconds and the user has stopped waiting anyway.
        if !(500..=20_000).contains(&self.test_timeout_ms) {
            return Err("test_timeout_ms must be between 500 and 20000".into());
        }
        // Past sixteen the handshakes queue on the same uplink and the numbers
        // stop meaning anything, which is the one thing a latency test must not do.
        if !(1..=16).contains(&self.test_concurrency) {
            return Err("test_concurrency must be between 1 and 16".into());
        }
        // Xray accepts any positive number here, but 128 is where the per-link
        // bookkeeping stops being worth it and every stream inherits the same
        // stalled link. Zero would silently disable the feature the user just
        // switched on, so it is refused instead of reinterpreted.
        if !(1..=128).contains(&self.mux_concurrency) {
            return Err("mux_concurrency must be between 1 and 128".into());
        }
        // Xray refuses the entire config when a fragment range does not parse,
        // so a switched-on fragment with a typo has to block the save instead of
        // reaching the kernel on the next start. An empty string is refused by
        // the same parse: "enabled but blank" would otherwise silently disable
        // the feature the user just turned on.
        if self.fragment_enabled {
            if parse_positive_range(&self.fragment_length).is_none() {
                return Err("分片长度要写成 a-b 这样的范围，例如 100-200".to_string());
            }
            if parse_positive_range(&self.fragment_interval).is_none() {
                return Err("分片间隔要写成 a-b 这样的范围，例如 10-20".to_string());
            }
        }
        self.dns_entries()?;
        // Both chords are parsed here so a typo is refused at save time instead
        // of turning into a shortcut that silently does nothing. Empty text is
        // meaningful (switched off), which is why `parse_optional` exists.
        let toggle = crate::hotkeys::parse_optional(&self.hotkey_toggle)?;
        let window = crate::hotkeys::parse_optional(&self.hotkey_window)?;
        // Windows keys ids per combination: registering the same chord twice
        // leaves one action unreachable, and the settings page would show two
        // rows that claim to work.
        if let (Some(toggle), Some(window)) = (toggle, window) {
            if toggle == window {
                return Err(format!(
                    "两个快捷键不能相同（都是 {}）",
                    toggle.label()
                ));
            }
        }
        Ok(())
    }

    /// Parse [`Settings::dns_servers`] into typed entries. Kept on the settings
    /// type so the renderers and the validator cannot disagree about what
    /// counts as a server.
    pub fn dns_entries(&self) -> Result<Vec<DnsEntry>, String> {
        self.dns_servers
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(DnsEntry::parse)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_language_codes_match_the_front_end_dictionaries() {
        // `zh-tw` is spelled out in `#[serde(rename)]`; if that ever drifts, the
        // store would hold a code the dictionary loader rejects.
        assert_eq!(serde_json::to_string(&Language::Zh).unwrap(), "\"zh\"");
        assert_eq!(serde_json::to_string(&Language::ZhTw).unwrap(), "\"zh-tw\"");
        assert_eq!(serde_json::to_string(&Language::En).unwrap(), "\"en\"");
        let parsed: Language = serde_json::from_str("\"zh-tw\"").unwrap();
        assert_eq!(parsed, Language::ZhTw);
    }

    #[test]
    fn closing_keeps_the_tunnel_unless_told_otherwise() {
        // The default has to match the tray's promise ("close hides, the tunnel
        // stays up"); a default of `false` would silently kill the proxy when
        // someone clicks X out of habit.
        let defaults = Settings::default();
        assert!(defaults.close_to_tray);
        assert!(!defaults.start_minimized);
        // Lite mode is opt-in: it costs a visible refresh pause, so it must not
        // arrive turned on for people who never asked for it.
        assert!(!defaults.lite_mode);
    }

    #[test]
    fn the_launch_behaviour_survives_a_round_trip() {
        let settings = Settings {
            start_minimized: true,
            close_to_tray: false,
            ..Default::default()
        };
        let json = serde_json::to_string(&settings).expect("serialize");
        let back: Settings = serde_json::from_str(&json).expect("deserialize");
        assert!(back.start_minimized);
        assert!(!back.close_to_tray);
    }

    #[test]
    fn default_settings_are_valid() {
        assert!(Settings::default().validate().is_ok());
    }

    #[test]
    fn the_probe_url_must_be_plain_http() {
        let s = Settings { test_url: "https://example.com/".into(), ..Default::default() };
        assert!(s.validate().is_err());
        let s = Settings { test_url: "  ".into(), ..Default::default() };
        assert!(s.validate().is_err());
        let s = Settings { test_url: "http://cp.cloudflare.com/".into(), ..Default::default() };
        assert!(s.validate().is_ok());
    }

    #[test]
    fn dns_servers_must_be_ip_literals() {
        let s = Settings { dns_servers: "1.1.1.1, 8.8.8.8".into(), ..Default::default() };
        assert!(s.validate().is_ok());
        assert_eq!(
            s.dns_entries().unwrap(),
            vec![DnsEntry::Plain("1.1.1.1".into()), DnsEntry::Plain("8.8.8.8".into())]
        );
        let s = Settings { dns_servers: "dns.google".into(), ..Default::default() };
        assert!(s.validate().is_err());
        // Trailing commas are what people actually type; they must not fail.
        let s = Settings { dns_servers: "1.1.1.1,,".into(), ..Default::default() };
        assert_eq!(s.dns_entries().unwrap(), vec![DnsEntry::Plain("1.1.1.1".into())]);
        assert!(s.validate().is_ok());
    }

    #[test]
    fn encrypted_resolvers_are_understood() {
        let s = Settings {
            dns_servers: "1.1.1.1, tls://9.9.9.9, https://8.8.8.8/dns-query".into(),
            ..Default::default()
        };
        assert!(s.validate().is_ok());
        assert_eq!(
            s.dns_entries().unwrap(),
            vec![
                DnsEntry::Plain("1.1.1.1".into()),
                DnsEntry::Dot {
                    host: "9.9.9.9".into()
                },
                DnsEntry::Doh {
                    host: "8.8.8.8".into(),
                    path: "/dns-query".into()
                },
            ]
        );
    }

    #[test]
    fn a_doh_url_without_a_path_gets_the_default_one() {
        assert_eq!(
            DnsEntry::parse("https://1.1.1.1").unwrap(),
            DnsEntry::Doh {
                host: "1.1.1.1".into(),
                path: "/dns-query".into()
            }
        );
    }

    #[test]
    fn a_hostname_resolver_is_refused_even_with_a_scheme() {
        // The reason is not pedantry: resolving `dns.google` needs a resolver,
        // and silently borrowing the system one is exactly the bug people
        // report as "my DNS setting does nothing".
        assert!(DnsEntry::parse("https://dns.google/dns-query").is_err());
        assert!(DnsEntry::parse("tls://one.one.one.one").is_err());
        assert_eq!(DnsEntry::parse("tls://1.1.1.1").unwrap().xray_address(), "tls://1.1.1.1");
        assert_eq!(
            DnsEntry::parse("https://1.1.1.1/dns-query").unwrap().xray_address(),
            "https://1.1.1.1/dns-query"
        );
    }

    #[test]
    fn rejects_port_collision() {
        let s = Settings { api_port: 7890, ..Default::default() };
        assert!(s.validate().is_err());
    }

    #[test]
    fn the_default_chords_are_the_shipped_ones() {
        // An existing user must not notice anything after upgrading: the chords
        // the app has always registered are still what a fresh store holds.
        let defaults = Settings::default();
        assert_eq!(defaults.hotkey_toggle, "Ctrl+Shift+X");
        assert_eq!(defaults.hotkey_window, "Ctrl+Shift+W");
        assert!(defaults.validate().is_ok());
    }

    #[test]
    fn chords_are_validated_at_save_time() {
        // A typo has to be refused here rather than reach `RegisterHotKey`, where
        // the only symptom would be a chord that never fires.
        let s = Settings { hotkey_toggle: "Ctrl+Shift+Hyper".into(), ..Default::default() };
        let error = s.validate().unwrap_err();
        assert!(error.contains("Hyper"), "error: {error}");

        let s = Settings { hotkey_window: "Ctrl+Shift".into(), ..Default::default() };
        let error = s.validate().unwrap_err();
        assert!(error.contains("修饰键"), "error: {error}");

        // Both blank is a legitimate state: two shortcuts, both switched off.
        let s = Settings {
            hotkey_toggle: String::new(),
            hotkey_window: "  ".into(),
            ..Default::default()
        };
        assert!(s.validate().is_ok());
    }

    #[test]
    fn one_chord_cannot_own_two_actions() {
        let s = Settings {
            hotkey_toggle: "ctrl+shift+w".into(),
            hotkey_window: "Ctrl+Shift+W".into(),
            ..Default::default()
        };
        let error = s.validate().unwrap_err();
        assert!(error.contains("不能相同"), "error: {error}");
        // Different spelling of the same chord is still the same chord.
        let s = Settings {
            hotkey_toggle: "Alt+F5".into(),
            hotkey_window: "F5".into(),
            ..Default::default()
        };
        assert!(s.validate().is_ok());
    }

    #[test]
    fn an_older_store_without_the_kernel_overrides_still_loads() {
        // A store.json written before N11 has neither field. Both must fall
        // back to "use the engines folder" instead of making the file
        // unreadable, which would look like the app forgot every setting.
        let old = r#"{"mixed_port":7890,"api_port":9090,"allow_lan":false,
            "mode":"rule","system_proxy":false,"auto_start":false,"language":"zh",
            "auto_connect":false,"log_level":"warn","auto_test_minutes":0,
            "test_timeout_ms":3000,"test_concurrency":2,"proxy_bypass":"",
            "test_url":"http://www.gstatic.com/generate_204",
            "subscription_user_agent":"","subscription_via_proxy":false,
            "dns_servers":"","start_minimized":false,"close_to_tray":true}"#;
        let loaded: Settings = serde_json::from_str(old).expect("old store must still load");
        assert!(loaded.xray_binary.is_empty());
        assert!(loaded.singbox_binary.is_empty());
        // Added later still: an old document keeps the chords it effectively had.
        assert_eq!(loaded.hotkey_toggle, "Ctrl+Shift+X");
        assert_eq!(loaded.hotkey_window, "Ctrl+Shift+W");
    }

    #[test]
    fn an_older_store_without_mux_still_loads() {
        // Multiplexing is opt-in, so a store written before it existed must
        // read as "off, eight streams": a missing field must not turn the
        // feature on, and it must not make the whole document unreadable.
        let old = r#"{"mixed_port":7890,"api_port":9090,"allow_lan":false,
            "mode":"rule","system_proxy":false,"auto_start":false,"language":"zh",
            "auto_connect":false,"log_level":"warn","auto_test_minutes":0,
            "test_timeout_ms":3000,"test_concurrency":2,"proxy_bypass":"",
            "test_url":"http://www.gstatic.com/generate_204",
            "subscription_user_agent":"","subscription_via_proxy":false,
            "dns_servers":"","start_minimized":false,"close_to_tray":true}"#;
        let loaded: Settings = serde_json::from_str(old).expect("old store must still load");
        assert!(!loaded.mux_enabled);
        assert_eq!(loaded.mux_concurrency, 8);
        let defaults = Settings::default();
        assert!(!defaults.mux_enabled);
        assert_eq!(defaults.mux_concurrency, 8);
    }

    #[test]
    fn mux_concurrency_must_be_between_one_and_one_hundred_twenty_eight() {
        // Zero would leave the user with a feature that silently does nothing,
        // and past 128 the streams share one stalled link rather than gain.
        let s = Settings { mux_concurrency: 0, ..Default::default() };
        assert!(s.validate().is_err());
        let s = Settings { mux_concurrency: 129, ..Default::default() };
        assert!(s.validate().is_err());
        let s = Settings { mux_enabled: true, mux_concurrency: 8, ..Default::default() };
        assert!(s.validate().is_ok());
        let s = Settings { mux_concurrency: 128, ..Default::default() };
        assert!(s.validate().is_ok());
    }

    #[test]
    fn an_older_store_without_fragment_still_loads() {
        // Fragmenting arrived in the same build as the settings page for it, so
        // an older store has none of the three fields. The feature must read as
        // "off with the classic range", and the document must stay readable.
        let old = r#"{"mixed_port":7890,"api_port":9090,"allow_lan":false,
            "mode":"rule","system_proxy":false,"auto_start":false,"language":"zh",
            "auto_connect":false,"log_level":"warn","auto_test_minutes":0,
            "test_timeout_ms":3000,"test_concurrency":2,"proxy_bypass":"",
            "test_url":"http://www.gstatic.com/generate_204",
            "subscription_user_agent":"","subscription_via_proxy":false,
            "dns_servers":"","start_minimized":false,"close_to_tray":true}"#;
        let loaded: Settings = serde_json::from_str(old).expect("old store must still load");
        assert!(!loaded.fragment_enabled);
        assert_eq!(loaded.fragment_length, "100-200");
        assert_eq!(loaded.fragment_interval, "10-20");
        let defaults = Settings::default();
        assert!(!defaults.fragment_enabled);
        assert_eq!(defaults.fragment_length, "100-200");
        assert_eq!(defaults.fragment_interval, "10-20");
    }

    #[test]
    fn fragment_ranges_must_be_a_forward_range() {
        // "100" has no dash and "300-100" runs backwards — both are what a
        // half-remembered format produces, and Xray refuses the whole config
        // rather than just this node when a fragment range will not parse.
        let bad = |length: &str, interval: &str| Settings {
            fragment_enabled: true,
            fragment_length: length.into(),
            fragment_interval: interval.into(),
            ..Default::default()
        };
        assert_eq!(
            bad("100", "10-20").validate().unwrap_err(),
            "分片长度要写成 a-b 这样的范围，例如 100-200"
        );
        assert_eq!(
            bad("300-100", "10-20").validate().unwrap_err(),
            "分片长度要写成 a-b 这样的范围，例如 100-200"
        );
        assert_eq!(
            bad("100-200", "10").validate().unwrap_err(),
            "分片间隔要写成 a-b 这样的范围，例如 10-20"
        );
        assert_eq!(
            bad("100-200", "20-10").validate().unwrap_err(),
            "分片间隔要写成 a-b 这样的范围，例如 10-20"
        );
        // Blank values are the other half of "the switch is on but nothing is
        // set"; enabling must not be a way to silently turn the feature off.
        assert!(bad("", "10-20").validate().is_err());
        assert!(bad("100-200", "").validate().is_err());
        // Spaces around the numbers are what people actually type.
        assert!(bad(" 100 - 200 ", " 10 - 20 ").validate().is_ok());
        assert!(bad("0-200", "10-20").validate().is_err());
    }

    #[test]
    fn fragment_values_are_ignored_while_the_switch_is_off() {
        // The fields stay in the document, so garbage saved before the switch
        // was turned off must not block saving while nothing emits it.
        let s = Settings {
            fragment_enabled: false,
            fragment_length: "garbage".into(),
            fragment_interval: "-".into(),
            ..Default::default()
        };
        assert!(s.validate().is_ok());
    }

    #[test]
    fn an_older_store_without_split_ports_still_loads() {
        // Split ports arrived after this document was written, so both fields
        // are missing. They must read as `0` — "one mixed inbound, as before" —
        // instead of making the store unreadable or guessing a second port.
        let old = r#"{"mixed_port":7890,"api_port":9090,"allow_lan":false,
            "mode":"rule","system_proxy":false,"auto_start":false,"language":"zh",
            "auto_connect":false,"log_level":"warn","auto_test_minutes":0,
            "test_timeout_ms":3000,"test_concurrency":2,"proxy_bypass":"",
            "test_url":"http://www.gstatic.com/generate_204",
            "subscription_user_agent":"","subscription_via_proxy":false,
            "dns_servers":"","start_minimized":false,"close_to_tray":true}"#;
        let loaded: Settings = serde_json::from_str(old).expect("old store must still load");
        assert_eq!(loaded.http_port, 0);
        assert_eq!(loaded.socks_port, 0);
        // And a split document must survive the round trip with its values.
        let split = Settings {
            http_port: 7891,
            socks_port: 7892,
            ..Default::default()
        };
        let text = serde_json::to_string(&split).expect("serialise");
        let back: Settings = serde_json::from_str(&text).expect("read back");
        assert_eq!(back.http_port, 7891);
        assert_eq!(back.socks_port, 7892);
    }

    #[test]
    fn split_ports_are_validated_at_save_time() {
        // The defaults are a legal document: no split, one mixed inbound.
        assert!(Settings::default().validate().is_ok());

        // A real split is legal, and so is two equal non-zero ports — that is
        // just "not split", served by one listener on that port.
        let split = Settings { http_port: 7891, socks_port: 7892, ..Default::default() };
        assert!(split.validate().is_ok());
        let same = Settings { http_port: 7891, socks_port: 7891, ..Default::default() };
        assert!(same.validate().is_ok());

        // A port below 1024 needs elevation, so it must be refused before the
        // kernel tries and dies with only its own log to explain it. `0` keeps
        // its meaning of "follow mixed_port" and is never a privileged port.
        let low_http = Settings { http_port: 80, ..Default::default() };
        assert!(low_http.validate().is_err());
        let low_socks = Settings { socks_port: 80, ..Default::default() };
        assert!(low_socks.validate().is_err());

        // The control port is served by the same process, so a collision means
        // one of the two listeners cannot bind.
        let http_is_api = Settings { http_port: 9090, ..Default::default() };
        assert!(http_is_api.validate().is_err());
        let socks_is_api = Settings { socks_port: 9090, ..Default::default() };
        assert!(socks_is_api.validate().is_err());

        // Landing on the mixed port is the same as not splitting, not a clash.
        let http_is_mixed = Settings { http_port: 7890, ..Default::default() };
        assert!(http_is_mixed.validate().is_ok());
    }

    #[test]
    fn prompt_elevation_is_on_by_default() {
        // A store written before this field existed has no key at all, and the
        // answer for it has to be "prompt me", not "elevate silently".
        let legacy: Settings = serde_json::from_str("{}").expect("empty object is a valid store");
        assert!(legacy.prompt_elevation);

        // An explicit `false` is honoured, and it survives a round trip.
        let quiet = Settings { prompt_elevation: false, ..Default::default() };
        let text = serde_json::to_string(&quiet).expect("serialise");
        assert!(text.contains("\"prompt_elevation\":false"));
        let back: Settings = serde_json::from_str(&text).expect("read back");
        assert!(!back.prompt_elevation);
    }
}
