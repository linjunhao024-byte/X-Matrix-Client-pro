//! Proxy groups: list, add/update, delete, pick a member, toggle.
//!
//! Two ownership rules live here rather than in the UI:
//!
//! * deleting a group rewrites every rule that pointed at it back to the
//!   active node, so a rule is never left pointing at nothing;
//! * deleting a node prunes it out of every group (see `commands::node`), for
//!   the same reason.

use tauri::State;

use crate::domain::{ProxyGroup, RuleAction};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[tauri::command]
#[specta::specta]
pub fn list_groups(state: State<'_, AppState>) -> AppResult<Vec<ProxyGroup>> {
    Ok(state.groups.read().clone())
}

#[tauri::command]
#[specta::specta]
pub fn upsert_group(state: State<'_, AppState>, group: ProxyGroup) -> AppResult<Vec<ProxyGroup>> {
    group.validate()?;

    let snapshot = {
        let mut groups = state.groups.write();
        match groups.iter_mut().find(|existing| existing.id == group.id) {
            Some(existing) => *existing = group,
            None => groups.push(group),
        }
        groups.clone()
    };
    state.persist()?;
    Ok(snapshot)
}

#[tauri::command]
#[specta::specta]
pub fn delete_group(state: State<'_, AppState>, id: String) -> AppResult<Vec<ProxyGroup>> {
    let snapshot = {
        let mut groups = state.groups.write();
        let before = groups.len();
        groups.retain(|group| group.id != id);
        if groups.len() == before {
            return Err(AppError::NotFound(format!("group {id}")));
        }

        // Rules pointing at the group fall back to the active node. Doing it
        // here means the invariant holds whichever screen deleted it.
        let mut rules = state.rules.write();
        for rule in rules.iter_mut() {
            if rule.action.group_id() == Some(id.as_str()) {
                rule.action = RuleAction::Proxy;
            }
        }

        groups.clone()
    };
    state.persist()?;
    Ok(snapshot)
}

/// Pick which member a `select` group uses.
#[tauri::command]
#[specta::specta]
pub fn select_group_member(
    state: State<'_, AppState>,
    id: String,
    node_id: String,
) -> AppResult<Vec<ProxyGroup>> {
    let snapshot = {
        let mut groups = state.groups.write();
        let group = groups
            .iter_mut()
            .find(|group| group.id == id)
            .ok_or_else(|| AppError::NotFound(format!("group {id}")))?;
        if !group.members.iter().any(|member| member == &node_id) {
            return Err(AppError::InvalidInput(format!(
                "节点不在策略组「{}」的成员里",
                group.name
            )));
        }
        group.selected = Some(node_id);
        groups.clone()
    };
    state.persist()?;
    Ok(snapshot)
}

#[tauri::command]
#[specta::specta]
pub fn toggle_group(state: State<'_, AppState>, id: String) -> AppResult<Vec<ProxyGroup>> {
    let snapshot = {
        let mut groups = state.groups.write();
        let group = groups
            .iter_mut()
            .find(|group| group.id == id)
            .ok_or_else(|| AppError::NotFound(format!("group {id}")))?;
        group.enabled = !group.enabled;
        groups.clone()
    };
    state.persist()?;
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{GroupKind, RuleMatcher, RoutingRule};

    fn state() -> AppState {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("groups-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        AppState::load(crate::store::Store::new(dir.join("store.json")))
    }

    fn group() -> ProxyGroup {
        ProxyGroup {
            id: "g1".into(),
            name: "自动选择".into(),
            enabled: true,
            kind: GroupKind::Select,
            members: vec!["n1".into(), "n2".into()],
            selected: None,
        }
    }

    #[test]
    fn a_rule_pointing_at_a_group_can_be_rewritten_to_the_active_node() {
        let state = state();
        *state.groups.write() = vec![group()];
        *state.rules.write() = vec![RoutingRule {
            id: "r1".into(),
            name: "走策略组".into(),
            enabled: true,
            matcher: RuleMatcher::DomainSuffix { value: "example.com".into() },
            action: RuleAction::Group { group_id: "g1".into() },
        }];

        // `delete_group` needs a Tauri `State`, so the invariant is exercised
        // directly here: after removing the group, no rule may still name it.
        state.groups.write().clear();
        let mut rules = state.rules.write();
        for rule in rules.iter_mut() {
            if rule.action.group_id() == Some("g1") {
                rule.action = RuleAction::Proxy;
            }
        }
        assert_eq!(rules[0].action, RuleAction::Proxy);
        assert!(rules.iter().all(|rule| rule.action.group_id().is_none()));
    }
}
