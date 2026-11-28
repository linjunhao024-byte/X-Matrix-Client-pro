//! Rule sets: named domain/IP lists that rules can point at.
//!
//! Split from `routing.rs` because the two have different lifetimes: a rule is
//! a line the user wrote, a rule set is a document that changes under them when
//! a provider updates. Deleting a set must therefore not silently delete the
//! rules that used it — they are disabled instead, and the UI says why.
//!
//! This used to be the body of `commands/ruleset.rs`; the Tauri commands are
//! now thin wrappers over these functions, shared with the headless core's HTTP
//! channel.

use std::time::Duration;

use crate::domain::ruleset::{parse_entries, RuleSet, RuleSetSource};
use crate::domain::routing::RuleMatcher;
use crate::engine::geo;
use crate::engine::LoadedRuleSet;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::EngineRuntime;

use crate::services::EmptyArgs;

// ---------------------------------------------------------------------------
// HTTP-facing entry points. One per route; the wire body matches the Tauri
// argument names the frontend already sends.
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize)]
pub struct IdArgs {
    pub id: String,
}

/// Creating from the UI carries the two fields the form has, not a whole
/// [`RuleSet`]: the id is ours to mint in [`RuleSet::new`].
#[derive(Debug, serde::Deserialize)]
pub struct CreateRuleSetArgs {
    pub name: String,
    pub url: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleSetArgs {
    pub rule_set: RuleSet,
}

#[derive(Debug, serde::Deserialize)]
pub struct SetTextArgs {
    pub id: String,
    pub text: String,
}

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

/// The two geo files a `geoip:`/`geosite:` rule depends on.
///
/// Reported even when they are absent: a missing data file is the usual reason
/// a geo rule looks broken, and the page has to be able to say so.
pub fn geo_assets(
    runtime: &EngineRuntime,
    _args: EmptyArgs,
) -> AppResult<Vec<geo::GeoAssetStatus>> {
    Ok(geo::status(&geo::asset_dir(runtime.engines_dir())))
}

/// Download both `geoip.dat` and `geosite.dat`, one line per file.
///
/// A kernel without these files still starts, so nothing else in the app would
/// notice: the rules just stop matching. Downloads go to a temp file and are
/// renamed into place, so an interrupted update leaves the old data usable.
// Each asset has several interchangeable sources (`geo::candidates`): GitHub
// itself is unreachable from some networks that still reach its mirrors, so a
// failed attempt moves on to the next address instead of ending the update.
// Only when every source failed does the line become a failure, and it names
// each attempt's reason in address order so the UI can show something a user
// can act on. (Kept as a plain comment: specta copies `///` text into the
// generated IPC bindings, and changing a sentence there is not this task's.)
/// What a geo update did, and whether a running kernel still has to be told.
///
/// The data files are read once, when a kernel starts: replacing them under a
/// live kernel changes nothing until it restarts. `updated` therefore decides
/// whether the page has to say "kernel is still using the old data".
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct GeoUpdate {
    /// One line per asset, in `geo::ASSET_NAMES` order, success or failure.
    pub lines: Vec<String>,
    /// How many files were actually replaced on disk.
    #[specta(type = f64)]
    pub updated: u32,
    /// True when at least one file changed while a kernel was running.
    pub restart_needed: bool,
}

pub fn update_geo_assets(runtime: &EngineRuntime, _args: EmptyArgs) -> AppResult<GeoUpdate> {
    // Geo data is a few megabytes; the ordinary 10 s fetch timeout would cut
    // off a slow but healthy link.
    const GEO_TIMEOUT: Duration = Duration::from_secs(180);
    let dir = geo::asset_dir(runtime.engines_dir());
    let mut lines = Vec::with_capacity(geo::ASSET_NAMES.len());
    let mut failures = Vec::new();
    let mut updated = 0u32;

    for name in geo::ASSET_NAMES {
        let mut attempts = Vec::new();
        let mut installed = false;

        for url in geo::candidates(name) {
            match crate::net::remote::fetch_bytes(url, GEO_TIMEOUT, name) {
                Ok((status, body)) if (200..300).contains(&status) => {
                    match geo::install(&dir, name, &body) {
                        Ok(()) => {
                            lines.push(format!(
                                "{name} 已更新（{}）",
                                geo::human_bytes(body.len() as u64)
                            ));
                            installed = true;
                            updated += 1;
                            break;
                        }
                        // The bytes arrived but the disk refused them; the next
                        // source serves the same file, so it is worth one more try.
                        Err(error) => attempts.push(format!("写入失败：{error}")),
                    }
                }
                Ok((status, _)) => attempts.push(format!("HTTP {status}")),
                Err(error) => attempts.push(error.to_string()),
            }
        }

        if !installed {
            let line = failures_line(name, &attempts);
            failures.push(line.clone());
            lines.push(line);
        }
    }

    if failures.len() == geo::ASSET_NAMES.len() {
        return Err(AppError::Engine(failures.join("；")));
    }
    let restart_needed = stale_data_needs_restart(updated, runtime.status().running);
    Ok(GeoUpdate {
        lines,
        updated,
        restart_needed,
    })
}

/// The one line shown when every source for `name` failed.
///
/// Each attempt contributes its own short reason ("HTTP 403", a connection
/// error), joined in the same order as the candidate list, so the reader can
/// tell which mirrors were tried and how each one broke.
/// Does a finished geo update leave a running kernel with stale data?
///
/// Only if a file was actually replaced *and* a kernel is running: nothing to
/// reload when nothing changed, and no kernel to tell when none runs.
fn stale_data_needs_restart(updated: u32, running: bool) -> bool {
    updated > 0 && running
}

fn failures_line(name: &str, attempts: &[String]) -> String {
    format!("{name} 下载失败：{}", attempts.join("；"))
}

pub fn list_rule_sets(state: &AppState, _args: EmptyArgs) -> Vec<RuleSet> {
    state.rule_sets.read().clone()
}

pub fn create_rule_set(state: &AppState, args: CreateRuleSetArgs) -> AppResult<Vec<RuleSet>> {
    let set = RuleSet::new(args.name, args.url);
    set.validate()?;
    state.rule_sets.write().push(set);
    state.persist()?;
    Ok(state.rule_sets.read().clone())
}

pub fn upsert_rule_set(state: &AppState, args: RuleSetArgs) -> AppResult<Vec<RuleSet>> {
    let rule_set = args.rule_set;
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
pub fn delete_rule_set(state: &AppState, args: IdArgs) -> AppResult<Vec<RuleSet>> {
    let id = args.id;
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

pub fn toggle_rule_set(state: &AppState, args: IdArgs) -> AppResult<Vec<RuleSet>> {
    let id = args.id;
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
pub fn set_rule_set_text(state: &AppState, args: SetTextArgs) -> AppResult<RuleSet> {
    apply_text(state, &args.id, &args.text)
}

/// Fetch a remote set (or re-parse a local one) and store the result.
///
/// Blocking on purpose: the Tauri command that fronts this one is `async` but
/// already fetched inline before the split, so the thread behaviour is
/// unchanged. The background sweep calls [`refresh_due_sets`] directly.
pub fn refresh_rule_set(state: &AppState, args: IdArgs) -> AppResult<RuleSet> {
    let id = args.id;
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
        return apply_text(state, &id, &text);
    }

    match crate::net::remote::fetch_text(&set.url) {
        Ok(text) => apply_text(state, &id, &text),
        Err(error) => {
            record_error(state, &id, &error.to_string());
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
        existing.updated_at_ms = Some(crate::services::subscription::now_ms());
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
            set.updated_at_ms = Some(crate::services::subscription::now_ms());
        }
    }
    let _ = state.persist();
}

#[cfg(test)]
mod tests {
    /// A running kernel only has to be told when a file really changed.
    #[test]
    fn only_a_real_change_needs_a_restart() {
        assert!(stale_data_needs_restart(1, true));
        assert!(stale_data_needs_restart(2, true));
        assert!(!stale_data_needs_restart(0, true), "nothing changed, nothing to reload");
        assert!(!stale_data_needs_restart(2, false), "no kernel, nothing to tell");
    }

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
            .join(format!("svcruleset-{}-{}", std::process::id(), seq));
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

        delete_rule_set(
            &state,
            IdArgs {
                id: set.id.clone(),
            },
        )
        .unwrap();

        assert!(state.rule_sets.read().is_empty());
        assert!(!state.rules.read()[0].enabled);
    }

    #[test]
    fn one_failed_attempt_is_reported_after_the_asset_name() {
        let line = failures_line("geoip.dat", &["HTTP 404".to_string()]);
        assert_eq!(line, "geoip.dat 下载失败：HTTP 404");
    }

    #[test]
    fn every_failed_attempt_appears_in_candidate_order() {
        let attempts = vec![
            "error sending request for url (https://cdn.jsdelivr.net/…)：连接被拒绝".to_string(),
            "HTTP 403".to_string(),
            "HTTP 404".to_string(),
        ];
        let line = failures_line("geosite.dat", &attempts);
        assert!(line.starts_with("geosite.dat 下载失败："), "{line}");
        let body = line.trim_start_matches("geosite.dat 下载失败：");
        let parts: Vec<&str> = body.split('；').collect();
        assert_eq!(
            parts,
            vec![
                "error sending request for url (https://cdn.jsdelivr.net/…)：连接被拒绝",
                "HTTP 403",
                "HTTP 404",
            ]
        );
    }

    #[test]
    fn a_missing_set_is_a_clear_error() {
        let state = state();
        assert!(apply_text(&state, "rs-nope", "example.com\n").is_err());
    }
}
