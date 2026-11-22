//! Rule sets: a named list of domain or IP patterns that a routing rule can
//! point at, instead of repeating thousands of entries inside the rule itself.
//!
//! The text is kept verbatim on disk (see [`crate::store`]); this module only
//! turns it into entries and back into kernel vocabulary. Two list dialects are
//! understood, because that is what the ecosystem actually publishes:
//!
//! * **Clash provider** (`payload:` YAML, or the same lines without the header):
//!   `+.example.com`, `.example.com`, `example.com`, `DOMAIN-SUFFIX,x`,
//!   `DOMAIN,x`, `DOMAIN-KEYWORD,x`, `IP-CIDR,x/y`
//! * **Plain list** (sing-box / v2ray `domain-list`): one pattern per line,
//!   `#` or `//` starts a comment.
//!
//! Anything unrecognised is counted and skipped rather than failing the whole
//! set: a provider that adds a new rule type upstream must not break routing.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::{AppError, AppResult};

pub const RULE_SET_TAG_PREFIX: &str = "rs:";

/// One parsed line of a rule set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SetEntry {
    /// Matches the domain and every subdomain of it.
    DomainSuffix { value: String },
    /// Matches exactly this domain.
    DomainFull { value: String },
    /// Matches any domain containing this substring.
    DomainKeyword { value: String },
    /// CIDR block.
    IpCidr { value: String },
}

/// How the set is kept up to date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum RuleSetSource {
    /// Fetched from a URL on a timer.
    Remote,
    /// Pasted by the user; never refreshed.
    Local,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct RuleSet {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub source: RuleSetSource,
    /// Empty for `Local` sets.
    pub url: String,
    /// `0` means "only when I press refresh".
    #[specta(type = f64)]
    pub update_interval_minutes: u32,
    #[specta(type = f64)]
    pub updated_at_ms: Option<f64>,
    /// How many entries the last parse produced. Kept so the UI can show
    /// something before the file is read.
    #[specta(type = f64)]
    pub entry_count: u32,
    pub last_error: Option<String>,
}

impl RuleSet {
    pub fn new(name: String, url: String) -> Self {
        let source = if url.trim().is_empty() {
            RuleSetSource::Local
        } else {
            RuleSetSource::Remote
        };
        Self {
            id: format!("rs-{}", crate::import::unique_suffix()),
            name,
            enabled: true,
            source,
            url: url.trim().to_string(),
            update_interval_minutes: if source == RuleSetSource::Remote { 1440 } else { 0 },
            updated_at_ms: None,
            entry_count: 0,
            last_error: None,
        }
    }

    /// Tag used in generated configs; also the on-disk file stem.
    pub fn tag(&self) -> String {
        format!("{RULE_SET_TAG_PREFIX}{}", self.id)
    }

    pub fn validate(&self) -> AppResult<()> {
        if self.name.trim().is_empty() {
            return Err(AppError::InvalidInput("规则集需要名字".into()));
        }
        if self.source == RuleSetSource::Remote {
            let url = self.url.trim();
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                return Err(AppError::InvalidInput(
                    "远程规则集的地址必须是 http:// 或 https://".into(),
                ));
            }
        }
        Ok(())
    }

    pub fn is_due(&self, now_ms: f64) -> bool {
        if !self.enabled || self.source != RuleSetSource::Remote {
            return false;
        }
        if self.update_interval_minutes == 0 {
            return false;
        }
        match self.updated_at_ms {
            None => true,
            Some(last) => now_ms - last >= f64::from(self.update_interval_minutes) * 60_000.0,
        }
    }
}

/// Result of parsing one list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedSet {
    pub entries: Vec<SetEntry>,
    /// Lines that were not understood, trimmed to the first few so a broken
    /// provider does not turn into a wall of text in the UI.
    pub skipped: Vec<String>,
    pub skipped_total: usize,
}

const MAX_REPORTED_SKIPS: usize = 5;

/// Parse a provider file. Never fails: unparseable lines are counted.
pub fn parse_entries(text: &str) -> ParsedSet {
    let mut parsed = ParsedSet::default();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }
        // Clash YAML wrappers: `payload:` and list markers.
        if line == "payload:" || line == "rules:" {
            continue;
        }
        let line = line.strip_prefix("- ").unwrap_or(line);
        let line = line.trim_matches(|c| c == '"' || c == '\'');
        if line.is_empty() {
            continue;
        }
        match parse_line(line) {
            Some(entry) => parsed.entries.push(entry),
            None => {
                parsed.skipped_total += 1;
                if parsed.skipped.len() < MAX_REPORTED_SKIPS {
                    parsed.skipped.push(line.to_string());
                }
            }
        }
    }
    parsed
}

fn parse_line(line: &str) -> Option<SetEntry> {
    // Clash `TYPE,value` form.
    if let Some((kind, rest)) = line.split_once(',') {
        // Trailing flags such as `,no-resolve` are policy hints for Clash,
        // not part of the pattern.
        let value = rest
            .split(',')
            .next()
            .unwrap_or("")
            .trim()
            .trim_matches(|c| c == '"' || c == '\'')
            .to_string();
        if value.is_empty() {
            return None;
        }
        return match kind.trim().to_ascii_uppercase().as_str() {
            "DOMAIN-SUFFIX" | "DOMAIN-SUFFIX-LIST" => Some(SetEntry::DomainSuffix { value }),
            "DOMAIN" | "DOMAIN-LIST" => Some(SetEntry::DomainFull { value }),
            "DOMAIN-KEYWORD" => Some(SetEntry::DomainKeyword { value }),
            "IP-CIDR" | "IP-CIDR6" => Some(SetEntry::IpCidr { value }),
            _ => None,
        };
    }

    // v2ray `domain:` / `full:` / `keyword:` prefixes.
    if let Some(rest) = line.strip_prefix("domain:") {
        return Some(SetEntry::DomainSuffix {
            value: rest.trim().to_string(),
        });
    }
    if let Some(rest) = line.strip_prefix("full:") {
        return Some(SetEntry::DomainFull {
            value: rest.trim().to_string(),
        });
    }
    if let Some(rest) = line.strip_prefix("keyword:") {
        return Some(SetEntry::DomainKeyword {
            value: rest.trim().to_string(),
        });
    }

    // Bare pattern: `+.example.com` and `.example.com` mean "and subdomains".
    if let Some(rest) = line.strip_prefix("+.") {
        return Some(SetEntry::DomainSuffix {
            value: rest.trim().to_string(),
        });
    }
    if let Some(rest) = line.strip_prefix('.') {
        return Some(SetEntry::DomainSuffix {
            value: rest.trim().to_string(),
        });
    }

    // Bare CIDR.
    if line.contains('/') && line.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        return Some(SetEntry::IpCidr {
            value: line.to_string(),
        });
    }

    // Bare domain.
    if line.contains('.') && !line.contains(' ') {
        return Some(SetEntry::DomainFull {
            value: line.to_string(),
        });
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_clash_provider_with_yaml_wrappers() {
        let text = "payload:\n  - '+.google.com'\n  - DOMAIN,example.org\n  - DOMAIN-KEYWORD,ads\n  - IP-CIDR,10.0.0.0/8,no-resolve\n";
        let parsed = parse_entries(text);
        assert_eq!(
            parsed.entries,
            vec![
                SetEntry::DomainSuffix {
                    value: "google.com".into()
                },
                SetEntry::DomainFull {
                    value: "example.org".into()
                },
                SetEntry::DomainKeyword {
                    value: "ads".into()
                },
                SetEntry::IpCidr {
                    value: "10.0.0.0/8".into()
                },
            ]
        );
        assert_eq!(parsed.skipped_total, 0);
    }

    #[test]
    fn reads_a_plain_list_with_prefixes_and_comments() {
        let text = "# comment\n// another\ndomain:example.com\nfull:www.example.com\nkeyword:track\n1.1.1.0/24\nplain.example.net\n";
        let parsed = parse_entries(text);
        assert_eq!(parsed.entries.len(), 5);
        assert!(matches!(parsed.entries[0], SetEntry::DomainSuffix { .. }));
        assert!(matches!(parsed.entries[3], SetEntry::IpCidr { .. }));
        assert!(matches!(parsed.entries[4], SetEntry::DomainFull { .. }));
    }

    #[test]
    fn unknown_lines_are_counted_not_fatal() {
        let text = "example.com\nGEOSITE,cn\nPROCESS-NAME,curl\n";
        let parsed = parse_entries(text);
        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.skipped_total, 2);
        assert_eq!(parsed.skipped.len(), 2);
    }

    #[test]
    fn a_remote_set_needs_an_http_url() {
        let set = RuleSet::new("x".into(), "file:///etc/passwd".into());
        assert!(set.validate().is_err());
        let ok = RuleSet::new("x".into(), "https://example.com/list.txt".into());
        assert!(ok.validate().is_ok());
    }

    #[test]
    fn a_local_set_is_never_due() {
        let mut set = RuleSet::new("local".into(), String::new());
        set.update_interval_minutes = 60;
        assert_eq!(set.source, RuleSetSource::Local);
        assert!(!set.is_due(9_999_999_999.0));
    }

    #[test]
    fn a_remote_set_is_due_once_the_interval_passed() {
        let mut set = RuleSet::new("remote".into(), "https://example.com/l.txt".into());
        set.update_interval_minutes = 60;
        assert!(set.is_due(1_000_000.0));
        set.updated_at_ms = Some(1_000_000.0);
        assert!(!set.is_due(1_000_000.0 + 60_000.0 * 59.0));
        assert!(set.is_due(1_000_000.0 + 60_000.0 * 61.0));
    }
}
