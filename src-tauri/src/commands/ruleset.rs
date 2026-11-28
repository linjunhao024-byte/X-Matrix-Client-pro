//! Rule sets — the Tauri door to the rule-set business layer.
//!
//! The bodies live in [`crate::services::ruleset`], shared with the headless
//! core's HTTP channel; this file keeps the command signatures and the `///`
//! text specta copies into the IPC bindings.

use std::sync::Arc;

use tauri::State;

use crate::domain::ruleset::RuleSet;
use crate::engine::geo;
use crate::error::AppResult;
use crate::services::ruleset as svc;
use crate::services::ruleset::GeoUpdate;
use crate::services::EmptyArgs;
use crate::state::AppState;
use crate::EngineRuntime;

use crate::services::ruleset::{CreateRuleSetArgs, IdArgs, RuleSetArgs, SetTextArgs};

/// The two geo files a `geoip:`/`geosite:` rule depends on.
///
/// Reported even when they are absent: a missing data file is the usual reason
/// a geo rule looks broken, and the page has to be able to say so.
#[tauri::command]
#[specta::specta]
pub fn geo_assets(runtime: State<'_, Arc<EngineRuntime>>) -> Vec<geo::GeoAssetStatus> {
    svc::geo_assets(runtime.inner(), EmptyArgs {}).unwrap_or_default()
}

#[tauri::command]
#[specta::specta]
pub fn update_geo_assets(runtime: State<'_, Arc<EngineRuntime>>) -> AppResult<GeoUpdate> {
    svc::update_geo_assets(runtime.inner(), EmptyArgs {})
}

#[tauri::command]
#[specta::specta]
pub fn list_rule_sets(state: State<'_, AppState>) -> Vec<RuleSet> {
    svc::list_rule_sets(&state, EmptyArgs {})
}

#[tauri::command]
#[specta::specta]
pub fn create_rule_set(
    state: State<'_, AppState>,
    name: String,
    url: String,
) -> AppResult<Vec<RuleSet>> {
    svc::create_rule_set(&state, CreateRuleSetArgs { name, url })
}

#[tauri::command]
#[specta::specta]
pub fn upsert_rule_set(state: State<'_, AppState>, rule_set: RuleSet) -> AppResult<Vec<RuleSet>> {
    svc::upsert_rule_set(&state, RuleSetArgs { rule_set })
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
    svc::delete_rule_set(&state, IdArgs { id })
}

#[tauri::command]
#[specta::specta]
pub fn toggle_rule_set(state: State<'_, AppState>, id: String) -> AppResult<Vec<RuleSet>> {
    svc::toggle_rule_set(&state, IdArgs { id })
}

/// Replace a local set's list with pasted text.
#[tauri::command]
#[specta::specta]
pub fn set_rule_set_text(
    state: State<'_, AppState>,
    id: String,
    text: String,
) -> AppResult<RuleSet> {
    svc::set_rule_set_text(&state, SetTextArgs { id, text })
}

/// Fetch a remote set (or re-parse a local one) and store the result.
#[tauri::command]
#[specta::specta]
pub async fn refresh_rule_set(state: State<'_, AppState>, id: String) -> AppResult<RuleSet> {
    svc::refresh_rule_set(&state, IdArgs { id })
}
