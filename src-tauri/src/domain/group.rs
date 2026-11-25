//! Proxy groups: a named set of nodes that a rule can target.
//!
//! Clash calls these "proxy groups", and they are the feature that makes a
//! rule table readable: instead of pointing every rule at one node, a rule
//! points at "auto" and the group decides which node that means.
//!
//! The two kernels are not equally capable here, and pretending otherwise
//! would produce configs that silently do the wrong thing:
//!
//! * sing-box has native `selector` and `urltest` outbounds, so a group is a
//!   real outbound that owns its members and can re-probe them.
//! * Xray has no selector outbound. The renderer therefore duplicates the
//!   chosen member outbound under the group tag, which means an Xray group is
//!   always static. The UI says so out loud.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::{AppError, AppResult};

/// Prefix of the outbound tag a group occupies in a rendered config.
pub const GROUP_TAG_PREFIX: &str = "group:";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum GroupKind {
    /// Always the member the user picked.
    Select,
    /// Probe the members and use the fastest one.
    UrlTest,
    /// Use the first member that answers, in the order the user arranged.
    ///
    /// Neither kernel has a `fallback` outbound, so this is *app-driven*: the
    /// renderer emits a plain `selector` (sing-box) or a static copy (Xray) and
    /// [`crate::engine::groupwatch`] flips the selector when the head member
    /// stops answering.
    Fallback,
    /// Rotate through the healthy members, one per probe round.
    ///
    /// Rotation is by time, not per connection — neither kernel can spread
    /// individual connections across outbounds. Like [`GroupKind::Fallback`],
    /// the renderer emits a selector and the watcher does the rotating.
    LoadBalance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ProxyGroup {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub kind: GroupKind,
    /// Node ids, in the order the user arranged them.
    pub members: Vec<String>,
    /// Which member `Select` uses. `None` means the first member.
    pub selected: Option<String>,
}

impl ProxyGroup {
    /// The outbound tag this group occupies.
    pub fn tag(&self) -> String {
        format!("{GROUP_TAG_PREFIX}{}", self.id)
    }

    /// The member a static rendering should use.
    pub fn chosen(&self) -> Option<&str> {
        match self.selected.as_deref() {
            Some(id) if self.members.iter().any(|member| member == id) => Some(id),
            _ => self.members.first().map(String::as_str),
        }
    }

    pub fn validate(&self) -> AppResult<()> {
        if self.name.trim().is_empty() {
            return Err(AppError::InvalidInput("策略组名称不能为空".to_string()));
        }
        if self.members.is_empty() {
            return Err(AppError::InvalidInput(format!(
                "策略组「{}」至少要有一个节点",
                self.name
            )));
        }
        if let Some(selected) = self.selected.as_deref() {
            if !self.members.iter().any(|member| member == selected) {
                return Err(AppError::InvalidInput(format!(
                    "策略组「{}」选中的节点不在成员里",
                    self.name
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(members: Vec<&str>, selected: Option<&str>) -> ProxyGroup {
        ProxyGroup {
            id: "g1".into(),
            name: "自动选择".into(),
            enabled: true,
            kind: GroupKind::Select,
            members: members.into_iter().map(str::to_string).collect(),
            selected: selected.map(str::to_string),
        }
    }

    #[test]
    fn a_group_without_members_is_rejected() {
        assert!(group(vec![], None).validate().is_err());
    }

    #[test]
    fn a_selection_outside_the_members_is_rejected() {
        assert!(group(vec!["a"], Some("b")).validate().is_err());
        assert!(group(vec!["a"], Some("a")).validate().is_ok());
    }

    #[test]
    fn chosen_falls_back_to_the_first_member() {
        assert_eq!(group(vec!["a", "b"], None).chosen(), Some("a"));
        assert_eq!(group(vec!["a", "b"], Some("b")).chosen(), Some("b"));
    }

    #[test]
    fn the_tag_carries_the_id() {
        assert_eq!(group(vec!["a"], None).tag(), "group:g1");
    }
}
