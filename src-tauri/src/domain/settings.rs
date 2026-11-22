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
    /// Control/API port used by the engine for stats & connections.
    pub api_port: u16,
    pub allow_lan: bool,
    pub mode: ProxyMode,
    /// Whether X-Matrix currently owns the Windows system proxy.
    pub system_proxy: bool,
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
    /// How chatty the kernel log should be. `Warn` keeps the file readable;
    /// `Debug` is what you switch to when a node will not connect.
    pub log_level: LogLevel,
    /// Re-test every node this often, in minutes. `0` means "only when I press
    /// the button": an app that starts hammering node endpoints on its own is
    /// something people notice on a metered connection.
    pub auto_test_minutes: u32,
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
    /// Comma separated resolver IPs. Empty means "let the kernel use the system
    /// resolver". Only literals are accepted: a hostname here would need the
    /// system resolver to resolve the resolver, which is how DNS loops start.
    pub dns_servers: String,
    /// Come up with the window hidden, tray only. For people who launch this at
    /// login and never want to see it.
    pub start_minimized: bool,
    /// What the window's X button does. `true` (the default) hides to tray and
    /// keeps the tunnel up; `false` quits and restores the system proxy, which
    /// is what someone who thinks of X as "close the program" expects.
    pub close_to_tray: bool,
}

/// The probe used by every "is this node actually reachable" check. Kept in the
/// domain because it is a persisted default, and referenced by `net::latency`.
pub const DEFAULT_TEST_URL: &str = "http://www.gstatic.com/generate_204";

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
    En,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mixed_port: 7890,
            api_port: 9090,
            allow_lan: false,
            mode: ProxyMode::Rule,
            system_proxy: false,
            auto_start: false,
            language: Language::Zh,
            auto_connect: false,
            last_node_id: None,
            last_engine: None,
            log_level: LogLevel::Warn,
            auto_test_minutes: 0,
            proxy_bypass: crate::sysproxy::DEFAULT_BYPASS.to_string(),
            test_url: DEFAULT_TEST_URL.to_string(),
            subscription_user_agent: String::new(),
            dns_servers: String::new(),
            start_minimized: false,
            close_to_tray: true,
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
        self.dns_entries()?;
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
    fn closing_keeps_the_tunnel_unless_told_otherwise() {
        // The default has to match the tray's promise ("close hides, the tunnel
        // stays up"); a default of `false` would silently kill the proxy when
        // someone clicks X out of habit.
        let defaults = Settings::default();
        assert!(defaults.close_to_tray);
        assert!(!defaults.start_minimized);
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
}
