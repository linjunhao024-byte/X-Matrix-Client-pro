//! The routing table: list, add/update, delete, reorder.
//!
//! Order matters — the kernel evaluates rules top to bottom and the first match
//! wins — so the list is stored as an array and `move_rule` is the only way to
//! change the order.

use serde::Serialize;
use specta::Type;
use tauri::State;

use crate::commands::ruleset::loaded_sets;
use crate::domain::{RuleAction, RoutingRule};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// What the rule table would do with one hostname.
#[derive(Debug, Clone, Serialize, Type)]
pub struct RuleTest {
    /// Name of the rule that caught it, or `None` when nothing did.
    pub rule: Option<String>,
    /// The action that rule takes; `None` means "fell through to the default".
    pub action: Option<RuleAction>,
    /// Rules that could not be judged from a hostname alone (geoip / port /
    /// process) and were skipped. Reporting them is the difference between
    /// "your rules do not cover this" and "I could not tell".
    #[specta(type = f64)]
    pub skipped: usize,
}

/// Which rule catches `domain`?
///
/// Rules are evaluated in order and the first hit wins, exactly as the kernel
/// does it, so this is a faithful dry run rather than a separate opinion.
#[tauri::command]
#[specta::specta]
pub fn test_domain(state: State<'_, AppState>, domain: String) -> AppResult<RuleTest> {
    let host = domain.trim();
    if host.is_empty() {
        return Err(AppError::InvalidInput("先填一个域名".to_string()));
    }

    let rules = state.rules.read().clone();
    let sets = loaded_sets(&state);
    let mut skipped = 0;

    for rule in rules.iter().filter(|rule| rule.enabled) {
        let entries = match &rule.matcher {
            crate::domain::RuleMatcher::RuleSet { value } => sets
                .iter()
                .find(|loaded| &loaded.set.id == value)
                .map(|loaded| loaded.entries.as_slice()),
            _ => None,
        };
        match rule.matcher.matches_domain(host, entries) {
            Some(true) => {
                return Ok(RuleTest {
                    rule: Some(rule.name.clone()),
                    action: Some(rule.action.clone()),
                    skipped,
                })
            }
            Some(false) => {}
            None => skipped += 1,
        }
    }

    Ok(RuleTest {
        rule: None,
        action: None,
        skipped,
    })
}

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
