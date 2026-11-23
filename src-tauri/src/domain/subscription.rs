//! Subscription sources.
//!
//! A subscription is a URL that hands back a list of nodes, plus the policy for
//! when to fetch it again. The nodes themselves are ordinary [`Node`]s — they
//! are tagged with the source id so a refresh can replace exactly its own
//! nodes and leave hand-added ones alone.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::domain::node::Node;
use crate::error::{AppError, AppResult};

/// Tag prefix that marks a node as owned by a subscription.
pub const SOURCE_TAG_PREFIX: &str = "sub:";

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Subscription {
    pub id: String,
    pub name: String,
    pub url: String,
    /// Added in front of every node name this source brings in. Empty (or
    /// whitespace-only) means "leave the names alone". Applied once: a name
    /// that already starts with the prefix is not prefixed again, so a refresh
    /// never stacks `机场A · 机场A · 香港 01`.
    #[serde(default)]
    pub rename_prefix: String,
    /// Regex over the node name as the provider wrote it; when non-empty only
    /// matching nodes are kept. Empty keeps everything.
    #[serde(default)]
    pub include: String,
    /// Regex over the same name; when non-empty matching nodes are dropped.
    /// Runs after `include`, so it wins when both match.
    #[serde(default)]
    pub exclude: String,
    pub enabled: bool,
    /// Minutes between automatic refreshes. `0` means "only when I ask".
    #[specta(type = f64)]
    pub update_interval_minutes: u32,
    /// Unix milliseconds of the last successful fetch.
    #[specta(type = f64)]
    pub updated_at_ms: Option<f64>,
    #[specta(type = f64)]
    pub node_count: u32,
    /// Why the last fetch failed, kept so the UI can explain a stale list
    /// instead of silently showing yesterday's nodes.
    pub last_error: Option<String>,
    /// Bytes used so far, from the provider's `subscription-userinfo` header.
    #[specta(type = f64)]
    pub traffic_used: Option<f64>,
    /// Bytes in the plan, same header.
    #[specta(type = f64)]
    pub traffic_total: Option<f64>,
    /// Unix milliseconds when the plan expires, same header.
    #[specta(type = f64)]
    pub expires_at_ms: Option<f64>,
}

impl Subscription {
    pub fn new(name: String, url: String) -> Self {
        Self {
            id: format!("sub-{}", crate::import::unique_suffix()),
            name,
            url: url.trim().to_string(),
            rename_prefix: String::new(),
            include: String::new(),
            exclude: String::new(),
            enabled: true,
            update_interval_minutes: 360,
            updated_at_ms: None,
            node_count: 0,
            last_error: None,
            traffic_used: None,
            traffic_total: None,
            expires_at_ms: None,
        }
    }

    /// Read the `subscription-userinfo` header into the quota fields.
    ///
    /// The format is `key=value; key=value` with `upload`, `download`, `total`
    /// (bytes) and `expire` (unix **seconds**). Panels are sloppy about it:
    /// missing keys, `0` meaning "unlimited", and stray spaces all happen, so a
    /// header we cannot read leaves the fields alone rather than zeroing them.
    pub fn apply_userinfo(&mut self, raw: &str) {
        let mut upload: Option<f64> = None;
        let mut download: Option<f64> = None;
        let mut total: Option<f64> = None;
        let mut expire: Option<f64> = None;

        for part in raw.split(';') {
            let Some((key, value)) = part.split_once('=') else {
                continue;
            };
            let Ok(number) = value.trim().parse::<f64>() else {
                continue;
            };
            match key.trim().to_ascii_lowercase().as_str() {
                "upload" => upload = Some(number),
                "download" => download = Some(number),
                "total" => total = Some(number),
                "expire" => expire = Some(number),
                _ => {}
            }
        }

        if upload.is_some() || download.is_some() {
            self.traffic_used = Some(upload.unwrap_or(0.0) + download.unwrap_or(0.0));
        }
        // A total of 0 is how panels say "no limit"; showing "已用 12 GB / 0 B"
        // would be worse than showing nothing.
        if let Some(total) = total.filter(|total| *total > 0.0) {
            self.traffic_total = Some(total);
        }
        if let Some(expire) = expire.filter(|expire| *expire > 0.0) {
            self.expires_at_ms = Some(expire * 1000.0);
        }
    }

    pub fn validate(&self) -> AppResult<()> {
        if self.name.trim().is_empty() {
            return Err(crate::error::AppError::InvalidInput("订阅名称不能为空".into()));
        }
        if !(self.url.starts_with("http://") || self.url.starts_with("https://")) {
            return Err(crate::error::AppError::InvalidInput(
                "订阅地址必须以 http:// 或 https:// 开头".into(),
            ));
        }
        // Compile the two name filters now, while the user is still looking at
        // the field. A broken pattern that got saved would otherwise sit there
        // and either keep everything (silently wrong) or fail on every refresh.
        if let Err(error) = compile_filter(&self.include) {
            return Err(AppError::InvalidInput(format!(
                "只保留的名字正则写错了：{error}"
            )));
        }
        if let Err(error) = compile_filter(&self.exclude) {
            return Err(AppError::InvalidInput(format!(
                "排除的名字正则写错了：{error}"
            )));
        }
        Ok(())
    }

    /// Whether the node survives this source's `include`/`exclude`, and the
    /// name it enters the table under.
    ///
    /// Filtering reads the name *before* the prefix is added: a pattern like
    /// `^香港` should describe what the provider calls the node, not what this
    /// subscription renames it to — otherwise a prefix could make every node
    /// match, or none.
    pub fn tune(&self, node: &mut Node) -> bool {
        let include = compile_filter(&self.include).ok().flatten();
        if let Some(include) = include {
            if !include.is_match(&node.name) {
                return false;
            }
        }
        let exclude = compile_filter(&self.exclude).ok().flatten();
        if let Some(exclude) = exclude {
            if exclude.is_match(&node.name) {
                return false;
            }
        }
        node.name = self.prefixed_name(&node.name);
        true
    }

    /// The label a node gets on its way into the table.
    ///
    /// Empty or whitespace-only prefix means "leave it alone"; a name already
    /// starting with the prefix is returned untouched. Idempotent on purpose:
    /// the same document can be applied twice (a refresh, then an edit that
    /// triggers another), and the second pass must not stack prefixes.
    pub fn prefixed_name(&self, name: &str) -> String {
        let prefix = self.rename_prefix.as_str();
        if prefix.trim().is_empty() || name.starts_with(prefix) {
            return name.to_string();
        }
        format!("{prefix}{name}")
    }

    /// The tag that marks a node as coming from this subscription.
    pub fn tag(&self) -> String {
        format!("{SOURCE_TAG_PREFIX}{}", self.id)
    }

    /// Whether this subscription is due for a refresh at `now_ms`.
    pub fn is_due(&self, now_ms: f64) -> bool {
        if !self.enabled || self.update_interval_minutes == 0 {
            return false;
        }
        match self.updated_at_ms {
            None => true,
            Some(last) => now_ms - last >= f64::from(self.update_interval_minutes) * 60_000.0,
        }
    }
}

/// Compile one name filter. `Ok(None)` means "not set" (empty or whitespace
/// only); an `Err` is a pattern the user has to fix before it can be applied.
fn compile_filter(pattern: &str) -> Result<Option<regex::Regex>, regex::Error> {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return Ok(None);
    }
    regex::Regex::new(pattern).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sub() -> Subscription {
        Subscription::new("机场 A".into(), "https://example.com/sub".into())
    }

    fn node(name: &str) -> Node {
        Node {
            id: "n1".into(),
            name: name.into(),
            server: "a.example.com".into(),
            port: 443,
            protocol: crate::domain::node::Protocol::Vless {
                uuid: "u-1".into(),
                flow: None,
            },
            transport: crate::domain::node::Transport::Tcp,
            security: crate::domain::node::Security::None,
            tags: vec![],
        }
    }

    #[test]
    fn the_quota_header_becomes_used_total_and_expiry() {
        let mut s = sub();
        s.apply_userinfo("upload=1000; download=2000; total=10000; expire=1767225600");
        assert_eq!(s.traffic_used, Some(3000.0));
        assert_eq!(s.traffic_total, Some(10000.0));
        assert_eq!(s.expires_at_ms, Some(1767225600000.0));
    }

    #[test]
    fn an_unlimited_plan_has_no_total() {
        let mut s = sub();
        // Panels write `total=0` when there is no cap; "已用 3 B / 0 B" would be
        // worse than showing nothing.
        s.apply_userinfo("upload=1; download=2; total=0; expire=0");
        assert_eq!(s.traffic_used, Some(3.0));
        assert_eq!(s.traffic_total, None);
        assert_eq!(s.expires_at_ms, None);
    }

    #[test]
    fn a_header_we_cannot_read_changes_nothing() {
        let mut s = sub();
        s.apply_userinfo("upload=10; download=20; total=100; expire=1767225600");
        let before = (s.traffic_used, s.traffic_total, s.expires_at_ms);
        s.apply_userinfo("garbage; upload=abc; expire=--");
        assert_eq!((s.traffic_used, s.traffic_total, s.expires_at_ms), before);
    }

    #[test]
    fn a_url_without_a_scheme_is_rejected() {
        let mut s = sub();
        s.url = "example.com/sub".into();
        assert!(s.validate().is_err());
    }

    #[test]
    fn an_empty_name_is_rejected() {
        let mut s = sub();
        s.name = "   ".into();
        assert!(s.validate().is_err());
    }

    #[test]
    fn a_never_fetched_subscription_is_due() {
        assert!(sub().is_due(1_000.0));
    }

    #[test]
    fn a_fresh_subscription_is_not_due_but_an_old_one_is() {
        let mut s = sub();
        s.updated_at_ms = Some(0.0);
        // 360 minutes is six hours.
        assert!(!s.is_due(60_000.0));
        assert!(s.is_due(7.0 * 3_600_000.0));
    }

    #[test]
    fn a_disabled_or_manual_subscription_is_never_due() {
        let mut s = sub();
        s.enabled = false;
        assert!(!s.is_due(f64::MAX / 2.0));
        let mut manual = sub();
        manual.update_interval_minutes = 0;
        assert!(!manual.is_due(f64::MAX / 2.0));
    }

    #[test]
    fn an_old_store_entry_without_the_three_knobs_still_loads() {
        // Bytes written before this feature existed: the fields are simply
        // absent, and `#[serde(default)]` has to turn that into "off".
        let json = r#"{
            "id": "sub-1",
            "name": "机场",
            "url": "https://example.com/sub",
            "enabled": true,
            "update_interval_minutes": 360,
            "updated_at_ms": null,
            "node_count": 0,
            "last_error": null,
            "traffic_used": null,
            "traffic_total": null,
            "expires_at_ms": null
        }"#;
        let parsed: Subscription = serde_json::from_str(json).expect("old entry must load");
        assert_eq!(parsed.rename_prefix, "");
        assert_eq!(parsed.include, "");
        assert_eq!(parsed.exclude, "");
    }

    #[test]
    fn a_prefix_is_added_once_even_when_the_name_already_has_it() {
        let mut s = sub();
        s.rename_prefix = "机场A · ".into();

        let mut first = node("香港 01");
        assert!(s.tune(&mut first));
        assert_eq!(first.name, "机场A · 香港 01");

        // Applying the same source twice must not stack the label. This is
        // what makes a refresh followed by an edit harmless.
        assert!(s.prefixed_name(&first.name) == first.name);
        assert!(s.tune(&mut first));
        assert_eq!(first.name, "机场A · 香港 01");
    }

    #[test]
    fn a_whitespace_only_prefix_is_off() {
        let mut s = sub();
        s.rename_prefix = "   ".into();
        let mut n = node("香港 01");
        assert!(s.tune(&mut n));
        assert_eq!(n.name, "香港 01", "a blank prefix is not a separator");
    }

    #[test]
    fn include_keeps_only_names_that_match() {
        let mut s = sub();
        s.include = "香港|台湾".into();

        let mut hk = node("香港 01");
        assert!(s.tune(&mut hk));
        let mut tw = node("台湾 02");
        assert!(s.tune(&mut tw));
        let mut us = node("美国 03");
        assert!(!s.tune(&mut us), "a name outside the pattern is dropped");
    }

    #[test]
    fn exclude_wins_over_include() {
        let mut s = sub();
        s.include = "香港".into();
        s.exclude = "过期".into();

        let mut fresh = node("香港 01");
        assert!(s.tune(&mut fresh));
        let mut stale = node("香港 02 已过期");
        assert!(
            !s.tune(&mut stale),
            "include said keep, exclude said drop, exclude runs last"
        );
        let mut other = node("美国 03");
        assert!(!s.tune(&mut other), "include still decides first");
    }

    #[test]
    fn filters_read_the_provider_name_not_the_prefixed_one() {
        // With a prefix in place, `include` must still be a statement about
        // what the panel calls the node. Otherwise `^香港` would match every
        // node of this source the moment a prefix is set.
        let mut s = sub();
        s.rename_prefix = "机场A · ".into();
        s.include = "^香港".into();

        let mut hk = node("香港 01");
        assert!(s.tune(&mut hk));
        assert_eq!(hk.name, "机场A · 香港 01");

        let mut us = node("美国 03");
        assert!(!s.tune(&mut us));
    }

    #[test]
    fn a_broken_filter_regex_is_rejected_with_the_field_name() {
        let mut s = sub();
        s.include = "(香港".into();
        let error = s.validate().expect_err("a dangling alternation is invalid");
        let text = error.to_string();
        assert!(text.contains("只保留"), "got {text}");
        assert!(text.contains("正则"), "got {text}");

        let mut t = sub();
        t.exclude = "[未闭合".into();
        let text = t.validate().expect_err("an unclosed class is invalid").to_string();
        assert!(text.contains("排除"), "got {text}");
    }

    #[test]
    fn an_empty_filter_matches_everything() {
        // Whitespace is what an empty text input actually sends.
        let mut s = sub();
        s.include = "  ".into();
        s.exclude = "\t".into();
        assert!(s.validate().is_ok());
        assert!(s.tune(&mut node("任意名字")));
    }

    #[test]
    fn renaming_does_not_change_the_fingerprint() {
        // Dedup compares endpoints, not labels: a source is allowed to rename
        // its nodes between refreshes without the list doubling.
        let mut s = sub();
        s.rename_prefix = "机场A · ".into();
        let mut n = node("香港 01");
        let before = n.fingerprint();
        assert!(s.tune(&mut n));
        assert_eq!(n.fingerprint(), before);
    }
}
