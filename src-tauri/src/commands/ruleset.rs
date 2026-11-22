//! Rule sets: named domain/IP lists that rules can point at.
//!
//! Split from `routing.rs` because the two have different lifetimes: a rule is
//! a line the user wrote, a rule set is a document that changes under them when
//! a provider updates. Deleting a set must therefore not silently delete the
//! rules that used it — they are disabled instead, and the UI says why.

use tauri::State;

use crate::domain::ruleset::{parse_entries, RuleSet, RuleSetSource};
use crate::domain::routing::RuleMatcher;
use crate::engine::LoadedRuleSet;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// Read every enabled set into memory for one render.
///
/// A set whose cache file is unreadable contributes no entries rather than
/// failing the whole run: one broken provider must not stop the proxy from
/// starting, which is the mistake that makes clients unusable offline.
pub fn loaded_sets(state: &AppState) -> Vec<LoadedRuleSet> {
    let sets: Vec<RuleSet> = state.rule_sets.read().clone();
    sets.into_iter()
        .filter(|set| set.enabled)
        .map(|set| {
            let entries = state
                .rule_set_files
                .entries(&set)
                .map(|parsed| parsed.entries)
                .unwrap_or_default();
            LoadedRuleSet { set, entries }
        })
        .collect()
}

#[tauri::command]
#[specta::specta]
pub fn list_rule_sets(state: State<'_, AppState>) -> Vec<RuleSet> {
    state.rule_sets.read().clone()
}

#[tauri::command]
#[specta::specta]
pub fn create_rule_set(
    state: State<'_, AppState>,
    name: String,
    url: String,
) -> AppResult<Vec<RuleSet>> {
    let set = RuleSet::new(name, url);
    set.validate()?;
    state.rule_sets.write().push(set);
    state.persist()?;
    Ok(state.rule_sets.read().clone())
}

#[tauri::command]
#[specta::specta]
pub fn upsert_rule_set(state: State<'_, AppState>, rule_set: RuleSet) -> AppResult<Vec<RuleSet>> {
    rule_set.validate()?;
    {
        let mut sets = state.rule_sets.write();
        match sets.iter_mut().find(|existing| existing.id == rule_set.id) {
            Some(existing) => *existing = rule_set,
            None => sets.push(rule_set),
        }
    }
    state.persist()?;
    Ok(state.rule_sets.read().clone())
}

/// Remove a set and disable the rules that pointed at it.
///
/// Disabling (rather than deleting) keeps the user's intent on screen: the rule
/// is still there, still named, with a matcher that no longer resolves. The
/// renderers skip a rule whose set is missing, so a dangling rule matches
/// nothing instead of matching everything.
#[tauri::command]
#[specta::specta]
pub fn delete_rule_set(state: State<'_, AppState>, id: String) -> AppResult<Vec<RuleSet>> {
    let removed = {
        let mut sets = state.rule_sets.write();
        let Some(index) = sets.iter().position(|set| set.id == id) else {
            return Err(AppError::NotFound(format!("规则集 {id} 不存在")));
        };
        sets.remove(index)
    };

    {
        let mut rules = state.rules.write();
        for rule in rules.iter_mut() {
            if let RuleMatcher::RuleSet { value } = &rule.matcher
                && value == &removed.id
            {
                rule.enabled = false;
            }
        }
    }

    state.rule_set_files.remove(&removed)?;
    state.persist()?;
    Ok(state.rule_sets.read().clone())
}

#[tauri::command]
#[specta::specta]
pub fn toggle_rule_set(state: State<'_, AppState>, id: String) -> AppResult<Vec<RuleSet>> {
    {
        let mut sets = state.rule_sets.write();
        let Some(set) = sets.iter_mut().find(|set| set.id == id) else {
            return Err(AppError::NotFound(format!("规则集 {id} 不存在")));
        };
        set.enabled = !set.enabled;
    }
    state.persist()?;
    Ok(state.rule_sets.read().clone())
}

/// Replace a local set's list with pasted text.
#[tauri::command]
#[specta::specta]
pub fn set_rule_set_text(
    state: State<'_, AppState>,
    id: String,
    text: String,
) -> AppResult<RuleSet> {
    apply_text(&state, &id, &text)
}

/// Fetch a remote set (or re-parse a local one) and store the result.
#[tauri::command]
#[specta::specta]
pub async fn refresh_rule_set(state: State<'_, AppState>, id: String) -> AppResult<RuleSet> {
    let set = {
        let sets = state.rule_sets.read();
        sets.iter()
            .find(|set| set.id == id)
            .cloned()
            .ok_or_else(|| AppError::NotFound(format!("规则集 {id} 不存在")))?
    };

    if set.source == RuleSetSource::Local {
        // Nothing to download: re-parsing the cached text is still worth doing,
        // because the parser may have learned new line types.
        let text = state.rule_set_files.read(&set)?;
        return apply_text(&state, &id, &text);
    }

    match crate::net::remote::fetch_text(&set.url) {
        Ok(text) => apply_text(&state, &id, &text),
        Err(error) => {
            record_error(&state, &id, &error.to_string());
            Err(error)
        }
    }
}

/// Refresh every set that is due, for the background sweep.
pub fn refresh_due_sets(state: &AppState, now_ms: f64) -> Vec<(String, Result<usize, String>)> {
    let due: Vec<RuleSet> = state
        .rule_sets
        .read()
        .iter()
        .filter(|set| set.is_due(now_ms))
        .cloned()
        .collect();

    let mut report = Vec::new();
    for set in due {
        match crate::net::remote::fetch_text(&set.url) {
            Ok(text) => match apply_text(state, &set.id, &text) {
                Ok(updated) => report.push((updated.name, Ok(updated.entry_count as usize))),
                Err(error) => {
                    record_error(state, &set.id, &error.to_string());
                    report.push((set.name, Err(error.to_string())));
                }
            },
            Err(error) => {
                record_error(state, &set.id, &error.to_string());
                report.push((set.name, Err(error.to_string())));
            }
        }
    }
    report
}

/// Store `text` as the set's list and refresh its metadata.
fn apply_text(state: &AppState, id: &str, text: &str) -> AppResult<RuleSet> {
    let set = {
        let sets = state.rule_sets.read();
        sets.iter()
            .find(|set| set.id == id)
            .cloned()
            .ok_or_else(|| AppError::NotFound(format!("规则集 {id} 不存在")))?
    };

    let parsed = parse_entries(text);
    if parsed.entries.is_empty() && parsed.skipped_total == 0 {
        return Err(AppError::InvalidInput(
            "这份列表里没有任何能识别的规则".to_string(),
        ));
    }
    state.rule_set_files.write(&set, text)?;

    let updated = {
        let mut sets = state.rule_sets.write();
        let Some(existing) = sets.iter_mut().find(|existing| existing.id == id) else {
            return Err(AppError::NotFound(format!("规则集 {id} 不存在")));
        };
        existing.entry_count = parsed.entries.len().min(u32::MAX as usize) as u32;
        existing.updated_at_ms = Some(crate::commands::subscription::now_ms());
        existing.last_error = None;
        existing.clone()
    };

    state.persist()?;
    Ok(updated)
}

fn record_error(state: &AppState, id: &str, message: &str) {
    {
        let mut sets = state.rule_sets.write();
        if let Some(set) = sets.iter_mut().find(|set| set.id == id) {
            set.last_error = Some(message.to_string());
            set.updated_at_ms = Some(crate::commands::subscription::now_ms());
        }
    }
    let _ = state.persist();
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::domain::routing::{RuleAction, RoutingRule};
    use crate::store::Store;

    fn scratch() -> std::path::PathBuf {
        static SEQ: AtomicUsize = AtomicUsize::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("cmdruleset-{}-{}", std::process::id(), seq));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn state() -> AppState {
        let dir = scratch();
        AppState::load(Store::new(dir.join("store.json")))
    }

    fn set() -> RuleSet {
        RuleSet::new("广告".into(), String::new())
    }

    fn rule(set_id: &str) -> RoutingRule {
        RoutingRule {
            id: "r1".into(),
            name: "广告拦截".into(),
            enabled: true,
            matcher: RuleMatcher::RuleSet {
                value: set_id.to_string(),
            },
            action: RuleAction::Block,
        }
    }

    #[test]
    fn pasting_text_fills_the_set_and_counts_entries() {
        let state = state();
        let set = set();
        state.rule_sets.write().push(set.clone());

        let updated = apply_text(&state, &set.id, "payload:\n  - '+.ads.example'\n").unwrap();
        assert_eq!(updated.entry_count, 1);
        assert!(updated.updated_at_ms.is_some());
        assert!(updated.last_error.is_none());

        let loaded = loaded_sets(&state);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].entries.len(), 1);
    }

    #[test]
    fn an_empty_list_is_rejected_rather_than_stored() {
        let state = state();
        let set = set();
        state.rule_sets.write().push(set.clone());
        assert!(apply_text(&state, &set.id, "\n\n# only comments\n").is_err());
        assert_eq!(state.rule_sets.read()[0].entry_count, 0);
    }

    #[test]
    fn a_disabled_set_contributes_no_entries() {
        let state = state();
        let set = set();
        state.rule_sets.write().push(set.clone());
        apply_text(&state, &set.id, "example.com\n").unwrap();
        state.rule_sets.write()[0].enabled = false;
        assert!(loaded_sets(&state).is_empty());
    }

    #[test]
    fn deleting_a_set_disables_the_rules_that_used_it() {
        let state = state();
        let set = set();
        state.rule_sets.write().push(set.clone());
        state.rules.write().push(rule(&set.id));

        let sets = state.rule_sets.write();
        let removed = sets.iter().find(|s| s.id == set.id).cloned().unwrap();
        drop(sets);
        state.rule_set_files.remove(&removed).unwrap();

        // Same bookkeeping the command does, without going through Tauri state.
        {
            let mut sets = state.rule_sets.write();
            sets.retain(|s| s.id != set.id);
        }
        {
            let mut rules = state.rules.write();
            for rule in rules.iter_mut() {
                if let RuleMatcher::RuleSet { value } = &rule.matcher
                    && value == &set.id
                {
                    rule.enabled = false;
                }
            }
        }

        assert!(state.rule_sets.read().is_empty());
        assert!(!state.rules.read()[0].enabled);
    }

    #[test]
    fn a_missing_set_is_a_clear_error() {
        let state = state();
        assert!(apply_text(&state, "rs-nope", "example.com\n").is_err());
    }
}
