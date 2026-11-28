//! The routing table: list, add/update, delete, reorder.
//!
//! Order matters — the kernel evaluates rules top to bottom and the first match
//! wins — so the list is stored as an array and `move_rule` is the only way to
//! change the order.
//!
//! This used to be the body of `commands/routing.rs`; the Tauri commands are
//! now thin wrappers over these functions, shared with the headless core's HTTP
//! channel.

use serde::Serialize;
use specta::Type;

use crate::domain::{RuleAction, RoutingRule};
use crate::error::{AppError, AppResult};
use crate::services::EmptyArgs;
use crate::state::AppState;

// ---------------------------------------------------------------------------
// HTTP-facing entry points. One per route; the wire body matches the Tauri
// argument names the frontend already sends.
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize)]
pub struct IdArgs {
    pub id: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct DomainArgs {
    pub domain: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct RuleArgs {
    pub rule: RoutingRule,
}

#[derive(Debug, serde::Deserialize)]
pub struct MoveArgs {
    pub id: String,
    pub delta: i32,
}

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
pub fn test_domain(state: &AppState, args: DomainArgs) -> AppResult<RuleTest> {
    let host = args.domain.trim();
    if host.is_empty() {
        return Err(AppError::InvalidInput("先填一个域名".to_string()));
    }

    let rules = state.rules.read().clone();
    let sets = crate::services::ruleset::loaded_sets(state);
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

pub fn list_rules(state: &AppState, _args: EmptyArgs) -> AppResult<Vec<RoutingRule>> {
    Ok(state.rules.read().clone())
}

pub fn upsert_rule(state: &AppState, args: RuleArgs) -> AppResult<Vec<RoutingRule>> {
    let rule = args.rule;
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

pub fn delete_rule(state: &AppState, args: IdArgs) -> AppResult<Vec<RoutingRule>> {
    let id = args.id;
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
pub fn move_rule(state: &AppState, args: MoveArgs) -> AppResult<Vec<RoutingRule>> {
    let MoveArgs { id, delta } = args;
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
pub fn toggle_rule(state: &AppState, args: IdArgs) -> AppResult<Vec<RoutingRule>> {
    let id = args.id;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::routing::RuleMatcher;
    use crate::store::Store;

    fn scratch() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static SEQ: AtomicUsize = AtomicUsize::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("svcrouting-{}-{}", std::process::id(), seq));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn state() -> AppState {
        let dir = scratch();
        AppState::load(Store::new(dir.join("store.json")))
    }

    fn domain_rule(name: &str) -> RoutingRule {
        RoutingRule {
            id: "r1".into(),
            name: name.into(),
            enabled: true,
            matcher: RuleMatcher::DomainSuffix {
                value: "example.com".into(),
            },
            action: RuleAction::Block,
        }
    }

    #[test]
    fn a_domain_rule_catches_its_suffix() {
        let state = state();
        state.rules.write().push(domain_rule("广告"));

        let hit = test_domain(
            &state,
            DomainArgs {
                domain: "ads.example.com".into(),
            },
        )
        .unwrap();
        assert_eq!(hit.rule.as_deref(), Some("广告"));
        assert_eq!(hit.skipped, 0);
    }

    #[test]
    fn an_empty_domain_is_refused() {
        let state = state();
        let error = test_domain(
            &state,
            DomainArgs {
                domain: "   ".into(),
            },
        )
        .expect_err("empty domain");
        assert!(matches!(error, AppError::InvalidInput(_)), "got {error:?}");
    }

    #[test]
    fn moving_past_the_end_is_a_no_op() {
        let state = state();
        state.rules.write().push(domain_rule("广告"));

        let unchanged = move_rule(
            &state,
            MoveArgs {
                id: "r1".into(),
                delta: -1,
            },
        )
        .unwrap();
        assert_eq!(unchanged.len(), 1);
    }
}
