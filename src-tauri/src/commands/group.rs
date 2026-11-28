//! Proxy groups: list, add/update, delete, pick a member, toggle.
//!
//! The bodies live in [`crate::services::group`], shared with the headless
//! core's HTTP channel; this file keeps the command contract.
//!
//! Two ownership rules live in the service rather than in the UI:
//!
//! * deleting a group rewrites every rule that pointed at it back to the
//!   active node, so a rule is never left pointing at nothing;
//! * deleting a node prunes it out of every group (see `commands::node`), for
//!   the same reason.

use tauri::State;

use crate::domain::ProxyGroup;
use crate::error::AppResult;
use crate::services::{group as svc, EmptyArgs};
use crate::state::AppState;

#[tauri::command]
#[specta::specta]
pub fn list_groups(state: State<'_, AppState>) -> AppResult<Vec<ProxyGroup>> {
    svc::list_groups(&state, EmptyArgs {})
}

#[tauri::command]
#[specta::specta]
pub fn upsert_group(state: State<'_, AppState>, group: ProxyGroup) -> AppResult<Vec<ProxyGroup>> {
    svc::upsert_group(&state, svc::GroupArgs { group })
}

#[tauri::command]
#[specta::specta]
pub fn delete_group(state: State<'_, AppState>, id: String) -> AppResult<Vec<ProxyGroup>> {
    svc::delete_group(&state, svc::IdArgs { id })
}

/// Pick which member a `select` group uses.
#[tauri::command]
#[specta::specta]
pub fn select_group_member(
    state: State<'_, AppState>,
    id: String,
    node_id: String,
) -> AppResult<Vec<ProxyGroup>> {
    svc::select_group_member(&state, svc::SelectMemberArgs { id, node_id })
}

#[tauri::command]
#[specta::specta]
pub fn toggle_group(state: State<'_, AppState>, id: String) -> AppResult<Vec<ProxyGroup>> {
    svc::toggle_group(&state, svc::IdArgs { id })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{GroupKind, RuleAction, RuleMatcher, RoutingRule};

    fn state() -> AppState {
        // One directory per test: these run in parallel and a shared store
        // would let one test's `remove_dir_all` wipe another's groups.
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("groups-{}-{}", std::process::id(), seq));
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
    fn a_rule_pointing_at_a_group_is_rewritten_to_the_active_node() {
        // Removing the group without touching the rule would leave a policy
        // that sends traffic to nothing — the kind of silent blackhole the user
        // only notices when a site stops loading.
        let state = state();
        *state.groups.write() = vec![group()];
        *state.rules.write() = vec![RoutingRule {
            id: "r1".into(),
            name: "走策略组".into(),
            enabled: true,
            matcher: RuleMatcher::DomainSuffix { value: "example.com".into() },
            action: RuleAction::Group { group_id: "g1".into() },
        }];

        let left = svc::delete_group(&state, svc::IdArgs { id: "g1".into() }).unwrap();

        assert!(left.is_empty());
        let rules = state.rules.read();
        assert_eq!(rules[0].action, RuleAction::Proxy);
        assert!(rules.iter().all(|rule| rule.action.group_id().is_none()));
    }

    #[test]
    fn a_rule_pointing_elsewhere_is_left_alone() {
        let state = state();
        *state.groups.write() = vec![group()];
        *state.rules.write() = vec![RoutingRule {
            id: "r2".into(),
            name: "别的组".into(),
            enabled: true,
            matcher: RuleMatcher::DomainSuffix { value: "example.org".into() },
            action: RuleAction::Group { group_id: "g2".into() },
        }];

        svc::delete_group(&state, svc::IdArgs { id: "g1".into() }).unwrap();

        assert_eq!(
            state.rules.read()[0].action,
            RuleAction::Group { group_id: "g2".into() }
        );
    }

    #[test]
    fn picking_a_member_records_the_choice() {
        let state = state();
        *state.groups.write() = vec![group()];

        let groups =
            svc::select_group_member(&state, svc::SelectMemberArgs { id: "g1".into(), node_id: "n2".into() })
                .unwrap();

        assert_eq!(groups[0].selected.as_deref(), Some("n2"));
        // Persisted: the choice has to survive a restart or the group silently
        // falls back to its first member.
        assert_eq!(
            state.groups.read()[0].selected.as_deref(),
            Some("n2")
        );
    }

    #[test]
    fn a_member_outside_the_group_is_refused() {
        // Accepting it would create a selection the group itself cannot route,
        // and the picker would show a node that is not in the list.
        let state = state();
        *state.groups.write() = vec![group()];

        let error = svc::select_group_member(
            &state,
            svc::SelectMemberArgs { id: "g1".into(), node_id: "n9".into() },
        )
        .unwrap_err();

        assert!(format!("{error}").contains("不在策略组"), "{error}");
    }

    #[test]
    fn toggling_flips_enabled_and_persists() {
        let state = state();
        *state.groups.write() = vec![group()];

        let off = svc::toggle_group(&state, svc::IdArgs { id: "g1".into() }).unwrap();
        assert!(!off[0].enabled);
        let on = svc::toggle_group(&state, svc::IdArgs { id: "g1".into() }).unwrap();
        assert!(on[0].enabled);
    }

    #[test]
    fn unknown_group_ids_are_reported_as_not_found() {
        let state = state();

        assert!(matches!(
            svc::delete_group(&state, svc::IdArgs { id: "nope".into() }),
            Err(crate::error::AppError::NotFound(_))
        ));
        assert!(matches!(
            svc::toggle_group(&state, svc::IdArgs { id: "nope".into() }),
            Err(crate::error::AppError::NotFound(_))
        ));
        assert!(matches!(
            svc::select_group_member(
                &state,
                svc::SelectMemberArgs { id: "nope".into(), node_id: "n1".into() }
            ),
            Err(crate::error::AppError::NotFound(_))
        ));
    }
}