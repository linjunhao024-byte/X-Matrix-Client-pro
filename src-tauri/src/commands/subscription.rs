//! Subscription sources — the Tauri door to the subscription business layer.
//!
//! The bodies live in [`crate::services::subscription`], shared with the
//! headless core's HTTP channel; this file keeps the command signatures and the
//! one shell-only side effect (`refresh_tray`). The blocking hop around the
//! network fetch also stays here, because it needs the Tauri async runtime.

use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::domain::Subscription;
use crate::engine::EngineRuntime;
use crate::error::AppResult;
use crate::services::subscription as svc;
use crate::services::EmptyArgs;
use crate::state::AppState;

use super::engine::{run_blocking, Proxy, Runtime};
use crate::services::subscription::{CreateSubscriptionArgs, RefreshOutcome, SubscriptionArgs};

#[tauri::command]
#[specta::specta]
pub fn list_subscriptions(state: State<'_, AppState>) -> Vec<Subscription> {
    svc::list_subscriptions(&state, EmptyArgs {})
}

/// Creating from the UI: the id is ours, not the user's.
#[tauri::command]
#[specta::specta]
pub fn create_subscription(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    url: String,
) -> AppResult<Vec<Subscription>> {
    let snapshot = svc::create_subscription(&state, CreateSubscriptionArgs { name, url })?;
    crate::refresh_tray(&app);
    Ok(snapshot)
}

#[tauri::command]
#[specta::specta]
pub fn upsert_subscription(
    app: AppHandle,
    state: State<'_, AppState>,
    subscription: Subscription,
) -> AppResult<Vec<Subscription>> {
    let snapshot = svc::upsert_subscription(&state, SubscriptionArgs { subscription })?;
    crate::refresh_tray(&app);
    Ok(snapshot)
}

/// Removing a source also removes the nodes it brought in.
#[tauri::command]
#[specta::specta]
pub async fn delete_subscription(
    app: AppHandle,
    state: State<'_, AppState>,
    runtime: Runtime<'_>,
    proxy: Proxy<'_>,
    id: String,
) -> AppResult<Vec<Subscription>> {
    let snapshot = svc::apply_delete_subscription(&state, runtime.inner(), proxy.inner(), &id)?;
    crate::refresh_tray(&app);
    Ok(snapshot)
}

/// Fetch one source and swap in its nodes.
#[tauri::command]
#[specta::specta]
pub async fn refresh_subscription(
    app: AppHandle,
    state: State<'_, AppState>,
    runtime: State<'_, Arc<EngineRuntime>>,
    proxy: Proxy<'_>,
    id: String,
) -> AppResult<RefreshOutcome> {
    let runtime = Arc::clone(runtime.inner());
    let proxy = Arc::clone(proxy.inner());

    // Plan and apply read state on this thread; only the fetch crosses to a
    // blocking worker, and it owns nothing but plain strings.
    let plan = svc::refresh_plan(&state, &id, runtime.status().running)?;
    let note = plan.note.clone();
    let fetched = run_blocking(move || svc::fetch_subscription(plan)).await?;
    let outcome = svc::apply_fetch(
        &state,
        &runtime,
        &proxy,
        &id,
        &fetched.body,
        fetched.subscription_userinfo.as_deref(),
        note,
    )?;

    // A refresh rewrites the node table, and the tray lists node names.
    crate::refresh_tray(&app);
    Ok(outcome)
}

/// Refresh every source that is due. Called by the background sweep and by the
/// "update all" button.
#[tauri::command]
#[specta::specta]
pub async fn refresh_due(
    app: AppHandle,
    state: State<'_, AppState>,
    runtime: State<'_, Arc<EngineRuntime>>,
    proxy: Proxy<'_>,
) -> AppResult<Vec<RefreshOutcome>> {
    let due = svc::due_subscription_ids(&state, svc::now_ms());

    let mut outcomes = Vec::new();
    for id in due {
        match refresh_subscription(
            app.clone(),
            state.clone(),
            runtime.clone(),
            proxy.clone(),
            id.clone(),
        )
        .await
        {
            Ok(outcome) => outcomes.push(outcome),
            Err(error) => svc::record_refresh_error(&state, &id, &error.to_string()),
        }
    }
    Ok(outcomes)
}
