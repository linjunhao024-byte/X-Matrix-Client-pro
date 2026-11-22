//! Routing rules: which traffic goes where.
//!
//! A rule is a *matcher* plus an *action*. Keeping the matcher a tagged enum
//! means a rule can never exist in a half-filled state (a domain rule with no
//! domain, a port rule with no port), which is exactly the class of bug that
//! used to reach the kernel as an invalid config.
//!
//! The two kernels express these differently — Xray groups everything into
//! `field` rules with `outboundTag`, sing-box uses one key per matcher and
//! `action: "reject"` for blocking. That translation lives in
//! [`crate::engine::render`], not here.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::{AppError, AppResult};

/// What a rule looks at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum RuleMatcher {
    /// `example.com` and every subdomain.
    DomainSuffix { value: String },
    /// Any domain containing the text.
    DomainKeyword { value: String },
    /// Exactly this domain.
    DomainFull { value: String },
    /// An IP range, e.g. `192.168.0.0/16` or `10.0.0.1`.
    IpCidr { value: String },
    /// A geo database code, e.g. `cn`, `private`.
    GeoIp { value: String },
    /// Destination port.
    Port { value: u16 },
    /// Executable name on this machine, e.g. `chrome.exe`.
    ProcessName { value: String },
    /// Every pattern in a rule set. `value` is the set id, not the set name:
    /// renaming a set must not break the rules that point at it.
    RuleSet { value: String },
}

impl RuleMatcher {
    /// The text the user typed, whatever the matcher kind.
    pub fn value(&self) -> &str {
        match self {
            Self::DomainSuffix { value }
            | Self::DomainKeyword { value }
            | Self::DomainFull { value }
            | Self::IpCidr { value }
            | Self::GeoIp { value }
            | Self::ProcessName { value }
            | Self::RuleSet { value } => value,
            // Port has no text form; callers that need it match on the variant.
            Self::Port { .. } => "",
        }
    }

    /// Short human label used by the UI and by error messages.
    pub fn label(&self) -> &'static str {
        match self {
            Self::DomainSuffix { .. } => "域名后缀",
            Self::DomainKeyword { .. } => "域名关键词",
            Self::DomainFull { .. } => "完整域名",
            Self::IpCidr { .. } => "IP 段",
            Self::GeoIp { .. } => "地区",
            Self::Port { .. } => "端口",
            Self::ProcessName { .. } => "进程",
            Self::RuleSet { .. } => "规则集",
        }
    }

    fn validate(&self) -> AppResult<()> {
        match self {
            Self::Port { value } => {
                if *value == 0 {
                    return Err(AppError::InvalidInput("端口必须大于 0".to_string()));
                }
                Ok(())
            }
            Self::IpCidr { value } => {
                if value.trim().is_empty() {
                    return Err(AppError::InvalidInput("IP 段不能为空".to_string()));
                }
                // Deliberately loose: a full CIDR parser would be nice, but the
                // kernel is the authority on what it accepts. We only reject
                // what is certainly not an address.
                if value.contains(char::is_whitespace) {
                    return Err(AppError::InvalidInput(
                        "IP 段不能包含空格".to_string(),
                    ));
                }
                Ok(())
            }
            Self::GeoIp { value } => {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    return Err(AppError::InvalidInput("地区代码不能为空".to_string()));
                }
                if !trimmed.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                    return Err(AppError::InvalidInput(
                        "地区代码只能包含字母、数字和连字符".to_string(),
                    ));
                }
                Ok(())
            }
            Self::ProcessName { value } => {
                if value.trim().is_empty() {
                    return Err(AppError::InvalidInput("进程名不能为空".to_string()));
                }
                Ok(())
            }
            _ => {
                if self.value().trim().is_empty() {
                    return Err(AppError::InvalidInput(format!("{}不能为空", self.label())));
                }
                Ok(())
            }
        }
    }
}

/// Where matching traffic goes.
///
/// Tagged rather than a plain enum because `Group` carries the id of the
/// group it targets: a rule can never point at a group without naming one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum RuleAction {
    /// Through the selected node.
    Proxy,
    /// Straight out, without the proxy.
    Direct,
    /// Dropped.
    Block,
    /// Through a proxy group; the id refers to a [`crate::domain::group::ProxyGroup`].
    Group { group_id: String },
}

impl RuleAction {
    /// The Xray outbound tag this action maps to.
    pub fn xray_tag(&self) -> String {
        match self {
            Self::Proxy => "proxy".to_string(),
            Self::Direct => "direct".to_string(),
            Self::Block => "block".to_string(),
            Self::Group { group_id } => format!("group:{group_id}"),
        }
    }

    /// The sing-box outbound tag, or `None` when the action is a rejection.
    pub fn singbox_tag(&self) -> Option<String> {
        match self {
            // sing-box has no blackhole outbound in our baseline config;
            // blocking is a route action.
            Self::Block => None,
            other => Some(other.xray_tag()),
        }
    }

    /// The group this action targets, when it targets one.
    pub fn group_id(&self) -> Option<&str> {
        match self {
            Self::Group { group_id } => Some(group_id),
            _ => None,
        }
    }
}

/// One rule, evaluated top to bottom.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct RoutingRule {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub matcher: RuleMatcher,
    pub action: RuleAction,
}

impl RoutingRule {
    pub fn validate(&self) -> AppResult<()> {
        if self.name.trim().is_empty() {
            return Err(AppError::InvalidInput("规则名称不能为空".to_string()));
        }
        if let Some(group_id) = self.action.group_id() {
            if group_id.trim().is_empty() {
                return Err(AppError::InvalidInput(
                    "指向策略组的规则必须指定组".to_string(),
                ));
            }
        }
        self.matcher.validate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(matcher: RuleMatcher) -> RoutingRule {
        RoutingRule {
            id: "r1".into(),
            name: "规则".into(),
            enabled: true,
            matcher,
            action: RuleAction::Proxy,
        }
    }

    #[test]
    fn an_empty_domain_is_rejected() {
        let error = rule(RuleMatcher::DomainSuffix { value: "  ".into() })
            .validate()
            .unwrap_err();
        assert!(error.to_string().contains("域名后缀"));
    }

    #[test]
    fn a_zero_port_is_rejected() {
        assert!(rule(RuleMatcher::Port { value: 0 }).validate().is_err());
        assert!(rule(RuleMatcher::Port { value: 443 }).validate().is_ok());
    }

    #[test]
    fn a_geo_code_must_be_plain() {
        assert!(rule(RuleMatcher::GeoIp {
            value: "cn".into()
        })
        .validate()
        .is_ok());
        assert!(rule(RuleMatcher::GeoIp {
            value: "c n".into()
        })
        .validate()
        .is_err());
    }

    #[test]
    fn block_maps_to_a_tag_for_xray_and_to_a_reject_for_singbox() {
        assert_eq!(RuleAction::Block.xray_tag(), "block".to_string());
        assert_eq!(RuleAction::Block.singbox_tag(), None);
        assert_eq!(RuleAction::Proxy.singbox_tag(), Some("proxy".to_string()));
    }
}
