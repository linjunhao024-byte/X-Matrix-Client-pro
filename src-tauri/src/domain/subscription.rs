//! Subscription sources.
//!
//! A subscription is a URL that hands back a list of nodes, plus the policy for
//! when to fetch it again. The nodes themselves are ordinary [`Node`]s — they
//! are tagged with the source id so a refresh can replace exactly its own
//! nodes and leave hand-added ones alone.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::AppResult;

/// Tag prefix that marks a node as owned by a subscription.
pub const SOURCE_TAG_PREFIX: &str = "sub:";

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Subscription {
    pub id: String,
    pub name: String,
    pub url: String,
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
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sub() -> Subscription {
        Subscription::new("机场 A".into(), "https://example.com/sub".into())
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
}
