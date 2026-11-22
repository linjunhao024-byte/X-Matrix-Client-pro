//! The routing table: list, add/update, delete, reorder.
//!
//! Order matters — the kernel evaluates rules top to bottom and the first match
//! wins — so the list is stored as an array and `move_rule` is the only way to
//! change the order.

use tauri::State;

use crate::domain::RoutingRule;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[tauri::command]
#[specta::specta]
pub fn list_rules(state: State<'_, AppState>) -> AppResult<Vec<RoutingRule>> {
    Ok(state.rules.read().clone())
}

#[tauri::command]
#[specta::specta]
pub fn upsert_rule(state: State<'_, AppState>, rule: RoutingRule) -> AppResult<Vec<RoutingRule>> {
    rule.validate()?;

    let snapshot = {
        let mut rules = state.rules.write();
        match rules.iter_mut().find(|existing| existing.id == rule.id) {
            Some(existing) => *existing = rule,
            None => rules.push(rule),
        }
        rules.clone()
    };
    state.persist()?;
    Ok(snapshot)
}

#[tauri::command]
#[specta::specta]
pub fn delete_rule(state: State<'_, AppState>, id: String) -> AppResult<Vec<RoutingRule>> {
    let snapshot = {
        let mut rules = state.rules.write();
        let before = rules.len();
        rules.retain(|rule| rule.id != id);
        if rules.len() == before {
            return Err(AppError::NotFound(format!("rule {id}")));
        }
        rules.clone()
    };
    state.persist()?;
    Ok(snapshot)
}

/// Move one rule up or down by one position.
///
/// Separate from `upsert_rule` on purpose: reordering is not editing, and the
/// UI exposes it as a pair of arrows rather than a form field.
#[tauri::command]
#[specta::specta]
pub fn move_rule(state: State<'_, AppState>, id: String, delta: i32) -> AppResult<Vec<RoutingRule>> {
    let snapshot = {
        let mut rules = state.rules.write();
        let index = rules
            .iter()
            .position(|rule| rule.id == id)
            .ok_or_else(|| AppError::NotFound(format!("rule {id}")))?;
        let target = index as i64 + delta as i64;
        if target < 0 || target >= rules.len() as i64 {
            // Already at an end: not an error, just nothing to do.
            return Ok(rules.clone());
        }
        rules.swap(index, target as usize);
        rules.clone()
    };
    state.persist()?;
    Ok(snapshot)
}

/// Flip `enabled` without going through the editor form.
#[tauri::command]
#[specta::specta]
pub fn toggle_rule(state: State<'_, AppState>, id: String) -> AppResult<Vec<RoutingRule>> {
    let snapshot = {
        let mut rules = state.rules.write();
        let rule = rules
            .iter_mut()
            .find(|rule| rule.id == id)
            .ok_or_else(|| AppError::NotFound(format!("rule {id}")))?;
        rule.enabled = !rule.enabled;
        rules.clone()
    };
    state.persist()?;
    Ok(snapshot)
}
