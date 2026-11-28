//! Proxy groups: list, add/update, delete, pick a member, toggle.
//!
//! Two ownership rules live here rather than in the UI:
//!
//! * deleting a group rewrites every rule that pointed at it back to the
//!   active node, so a rule is never left pointing at nothing;
//! * deleting a node prunes it out of every group (see
//!   [`crate::services::node`]), for the same reason.
//!
//! This used to be the body of `commands/group.rs`; the Tauri commands are now
//! thin wrappers over these functions.

use crate::domain::{ProxyGroup, RuleAction};
use crate::error::{AppError, AppResult};
use crate::services::EmptyArgs;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// HTTP-facing entry points. One per route; the wire body matches the Tauri
// argument names the frontend already sends.
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize)]
pub struct GroupArgs {
    pub group: ProxyGroup,
}

#[derive(Debug, serde::Deserialize)]
pub struct IdArgs {
    pub id: String,
}

/// Picking a member takes two ids; `camelCase` keeps `node_id` the `nodeId`
/// the Tauri command already calls it.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectMemberArgs {
    pub id: String,
    pub node_id: String,
}

pub fn list_groups(state: &AppState, _args: EmptyArgs) -> AppResult<Vec<ProxyGroup>> {
    Ok(state.groups.read().clone())
}

pub fn upsert_group(state: &AppState, args: GroupArgs) -> AppResult<Vec<ProxyGroup>> {
    let group = args.group;
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

pub fn delete_group(state: &AppState, args: IdArgs) -> AppResult<Vec<ProxyGroup>> {
    let id = args.id;
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
pub fn select_group_member(
    state: &AppState,
    args: SelectMemberArgs,
) -> AppResult<Vec<ProxyGroup>> {
    let snapshot = {
        let mut groups = state.groups.write();
        let group = groups
            .iter_mut()
            .find(|group| group.id == args.id)
            .ok_or_else(|| AppError::NotFound(format!("group {}", args.id)))?;
        if !group.members.iter().any(|member| member == &args.node_id) {
            return Err(AppError::InvalidInput(format!(
                "节点不在策略组「{}」的成员里",
                group.name
            )));
        }
        group.selected = Some(args.node_id);
        groups.clone()
    };
    state.persist()?;
    Ok(snapshot)
}

pub fn toggle_group(state: &AppState, args: IdArgs) -> AppResult<Vec<ProxyGroup>> {
    let snapshot = {
        let mut groups = state.groups.write();
        let group = groups
            .iter_mut()
            .find(|group| group.id == args.id)
            .ok_or_else(|| AppError::NotFound(format!("group {}", args.id)))?;
        group.enabled = !group.enabled;
        groups.clone()
    };
    state.persist()?;
    Ok(snapshot)
}
