//! The routing table — the Tauri door to the routing business layer.
//!
//! The bodies live in [`crate::services::routing`], shared with the headless
//! core's HTTP channel; this file keeps the command signatures and the `///`
//! text specta copies into the IPC bindings.

use tauri::State;

use crate::domain::RoutingRule;
use crate::error::AppResult;
use crate::services::routing as svc;
use crate::services::routing::RuleTest;
use crate::services::EmptyArgs;
use crate::state::AppState;

use crate::services::routing::{DomainArgs, IdArgs, MoveArgs, RuleArgs};

/// Which rule catches `domain`?
///
/// Rules are evaluated in order and the first hit wins, exactly as the kernel
/// does it, so this is a faithful dry run rather than a separate opinion.
#[tauri::command]
#[specta::specta]
pub fn test_domain(state: State<'_, AppState>, domain: String) -> AppResult<RuleTest> {
    svc::test_domain(&state, DomainArgs { domain })
}

#[tauri::command]
#[specta::specta]
pub fn list_rules(state: State<'_, AppState>) -> AppResult<Vec<RoutingRule>> {
    svc::list_rules(&state, EmptyArgs {})
}

#[tauri::command]
#[specta::specta]
pub fn upsert_rule(state: State<'_, AppState>, rule: RoutingRule) -> AppResult<Vec<RoutingRule>> {
    svc::upsert_rule(&state, RuleArgs { rule })
}

#[tauri::command]
#[specta::specta]
pub fn delete_rule(state: State<'_, AppState>, id: String) -> AppResult<Vec<RoutingRule>> {
    svc::delete_rule(&state, IdArgs { id })
}

/// Move one rule up or down by one position.
///
/// Separate from `upsert_rule` on purpose: reordering is not editing, and the
/// UI exposes it as a pair of arrows rather than a form field.
#[tauri::command]
#[specta::specta]
pub fn move_rule(state: State<'_, AppState>, id: String, delta: i32) -> AppResult<Vec<RoutingRule>> {
    svc::move_rule(&state, MoveArgs { id, delta })
}

/// Flip `enabled` without going through the editor form.
#[tauri::command]
#[specta::specta]
pub fn toggle_rule(state: State<'_, AppState>, id: String) -> AppResult<Vec<RoutingRule>> {
    svc::toggle_rule(&state, IdArgs { id })
}
