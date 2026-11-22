//! Subscription sources: CRUD, refresh, and the scheduled sweep.
//!
//! A subscription owns the nodes tagged `sub:<id>` — nothing else. Refreshing
//! replaces exactly that slice, so a hand-added node can never be wiped by a
//! provider changing its mind.

use std::collections::HashSet;

use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::documents::parse_document;
use crate::domain::subscription::SOURCE_TAG_PREFIX;
use crate::domain::{Node, Subscription};
use crate::error::{AppError, AppResult};
use crate::engine::EngineRuntime;
use crate::net::remote;
use crate::state::AppState;

/// What one refresh did, so the UI can say something specific.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct RefreshOutcome {
    pub subscription: Subscription,
    #[specta(type = f64)]
    pub imported: usize,
    #[specta(type = f64)]
    pub removed: usize,
    pub skipped: Vec<String>,
    /// Something true about this refresh that is not about the body: which
    /// route it took, for instance. Kept apart from `skipped` because that list
    /// counts lines the parser could not read, and a route note is not a line.
    pub note: Option<String>,
}

/// Where a fetch should go, and what to tell the user about the route.
///
/// "Through the tunnel" is a setting, not a guess: a subscription whose panel is
/// unreachable from here can only be refreshed through the tunnel, and a
/// subscription that is reachable should not depend on a node being up. When the
/// setting is on but nothing is running we still fetch directly and say so in
/// `skipped`, because a refresh that silently takes a different route than the
/// one the user asked for is exactly the kind of quiet surprise that makes a
/// client untrustworthy.
fn fetch_route(state: &AppState, engine_running: bool) -> (Option<u16>, Option<String>) {
    let settings = state.settings.read();
    if !settings.subscription_via_proxy {
        return (None, None);
    }
    if engine_running {
        return (Some(settings.mixed_port), None);
    }
    (
        None,
        Some("订阅设置为走代理，但内核没有运行，这一次是直连拉的".to_string()),
    )
}

#[tauri::command]
#[specta::specta]
pub fn list_subscriptions(state: State<'_, AppState>) -> Vec<Subscription> {
    state.subscriptions.read().clone()
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
    let subscription = Subscription::new(name, url);
    subscription.validate()?;
    state.subscriptions.write().push(subscription);
    let snapshot = state.subscriptions.read().clone();
    state.persist()?;
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
    subscription.validate()?;
    {
        let mut subscriptions = state.subscriptions.write();
        match subscriptions.iter_mut().find(|existing| existing.id == subscription.id) {
            Some(existing) => *existing = subscription,
            None => subscriptions.push(subscription),
        }
    }
    let snapshot = state.subscriptions.read().clone();
    state.persist()?;
    crate::refresh_tray(&app);
    Ok(snapshot)
}

/// Removing a source also removes the nodes it brought in.
#[tauri::command]
#[specta::specta]
pub fn delete_subscription(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> AppResult<Vec<Subscription>> {
    let tag = {
        let subscriptions = state.subscriptions.read();
        subscriptions
            .iter()
            .find(|existing| existing.id == id)
            .map(|existing| existing.tag())
            .ok_or_else(|| AppError::NotFound(format!("订阅 {id} 不存在")))?
    };
    {
        let mut subscriptions = state.subscriptions.write();
        let before = subscriptions.len();
        subscriptions.retain(|existing| existing.id != id);
        if subscriptions.len() == before {
            return Err(AppError::NotFound(format!("订阅 {id} 不存在")));
        }
    }
    state.nodes.write().retain(|node| !node.tags.iter().any(|t| t == &tag));
    let snapshot = state.subscriptions.read().clone();
    state.persist()?;
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
    id: String,
) -> AppResult<RefreshOutcome> {
    let subscription = state
        .subscriptions
        .read()
        .iter()
        .find(|existing| existing.id == id)
        .cloned()
        .ok_or_else(|| AppError::NotFound(format!("订阅 {id} 不存在")))?;

    // The UA override travels with the request: some providers gate on it.
    let agent = state.settings.read().subscription_user_agent.clone();
    let (proxy, note) = fetch_route(&state, runtime.status().running);
    let fetched = tauri::async_runtime::spawn_blocking(move || {
        let fetched =
            remote::fetch_through(&subscription.url, remote::FETCH_TIMEOUT, Some(&agent), proxy)?;
        Ok::<_, AppError>(fetched)
    })
    .await
    .map_err(|error| AppError::Internal(format!("后台任务失败：{error}")))??;

    let mut outcome = apply_document(
        &state,
        &id,
        &fetched.body,
        fetched.subscription_userinfo.as_deref(),
    )?;
    outcome.note = note;
    // A refresh rewrites the node table, and the tray lists node names.
    crate::refresh_tray(&app);
    Ok(outcome)
}

/// Blocking refresh, for the background sweep (which owns no async runtime).
pub fn refresh_one_blocking(
    state: &AppState,
    id: &str,
    engine_running: bool,
) -> AppResult<RefreshOutcome> {
    let subscription = state
        .subscriptions
        .read()
        .iter()
        .find(|existing| existing.id == id)
        .cloned()
        .ok_or_else(|| AppError::NotFound(format!("订阅 {id} 不存在")))?;
    let agent = state.settings.read().subscription_user_agent.clone();
    let (proxy, note) = fetch_route(state, engine_running);
    let fetched = remote::fetch_through(&subscription.url, remote::FETCH_TIMEOUT, Some(&agent), proxy)?;
    let mut outcome = apply_document(
        state,
        id,
        &fetched.body,
        fetched.subscription_userinfo.as_deref(),
    )?;
    outcome.note = note;
    Ok(outcome)
}

/// The pure half: a document in, the node table rewritten.
///
/// Split out so the refresh path can be tested without a network and without
/// a running app.
pub fn apply_document(
    state: &AppState,
    id: &str,
    text: &str,
    userinfo: Option<&str>,
) -> AppResult<RefreshOutcome> {
    let report = parse_document(text);

    let mut subscription = {
        let subscriptions = state.subscriptions.read();
        subscriptions
            .iter()
            .find(|existing| existing.id == id)
            .cloned()
            .ok_or_else(|| AppError::NotFound(format!("订阅 {id} 不存在")))?
    };
    let tag = subscription.tag();

    let mut imported_nodes: Vec<Node> = report
        .nodes
        .into_iter()
        .map(|mut node| {
            node.tags = vec![tag.clone()];
            node
        })
        .collect();
    imported_nodes.retain(|node| {
        // A provider can hand back junk; the same rules as manual import apply.
        node.validate().is_ok()
    });

    // Two subscriptions (or one provider repeating itself) listing the same
    // server must not double the list, so the batch is compared against what
    // *other subscriptions* already brought in. Hand-made nodes are excluded
    // from that comparison: a user who pasted the same server twice on purpose
    // owns both copies, and a refresh has no business deleting one. This
    // batch's own old copies are not competition either — they are what we are
    // about to delete. Anything dropped is reported by name so the count in the
    // UI never silently disagrees with the provider.
    let mut seen: HashSet<String> = {
        let nodes = state.nodes.read();
        nodes
            .iter()
            .filter(|node| {
                node.tags
                    .iter()
                    .any(|t| t.starts_with(SOURCE_TAG_PREFIX) && t != &tag)
            })
            .map(|node| node.fingerprint())
            .collect()
    };
    let mut duplicates: Vec<String> = Vec::new();
    imported_nodes.retain(|node| {
        if seen.insert(node.fingerprint()) {
            true
        } else {
            duplicates.push(node.name.clone());
            false
        }
    });

    let removed = {
        let mut nodes = state.nodes.write();
        let before = nodes.len();
        nodes.retain(|node| !node.tags.iter().any(|t| t == &tag));
        let removed = before - nodes.len();
        nodes.extend(imported_nodes.iter().cloned());
        removed
    };

    subscription.node_count = imported_nodes.len() as u32;
    subscription.updated_at_ms = Some(now_ms());
    subscription.last_error = None;
    // The quota header only arrives with a real fetch; a provider that omits it
    // leaves the previous numbers alone rather than blanking them.
    if let Some(userinfo) = userinfo {
        subscription.apply_userinfo(userinfo);
    }

    {
        let mut subscriptions = state.subscriptions.write();
        if let Some(existing) = subscriptions.iter_mut().find(|existing| existing.id == id) {
            *existing = subscription.clone();
        }
    }
    state.persist()?;

    Ok(RefreshOutcome {
        subscription,
        imported: imported_nodes.len(),
        removed,
        note: None,
        skipped: {
            let mut skipped = report.skipped;
            // Duplicates are not an error, but they are why the count is lower
            // than what the provider sent — say so instead of hiding it.
            for name in duplicates {
                skipped.push(format!("与已有节点重复，已跳过：{name}"));
            }
            skipped
        },
    })
}

/// Refresh every source that is due. Called by the background sweep and by the
/// "update all" button.
#[tauri::command]
#[specta::specta]
pub async fn refresh_due(
    app: AppHandle,
    state: State<'_, AppState>,
    runtime: State<'_, Arc<EngineRuntime>>,
) -> AppResult<Vec<RefreshOutcome>> {
    let due: Vec<String> = {
        let now = now_ms();
        state
            .subscriptions
            .read()
            .iter()
            .filter(|subscription| subscription.is_due(now))
            .map(|subscription| subscription.id.clone())
            .collect()
    };

    let mut outcomes = Vec::new();
    for id in due {
        match refresh_subscription(app.clone(), state.clone(), runtime.clone(), id.clone()).await {
            Ok(outcome) => outcomes.push(outcome),
            Err(error) => {
                // Record the failure on the source itself: a silently dead
                // subscription is worse than a loud one.
                let mut subscriptions = state.subscriptions.write();
                if let Some(existing) = subscriptions.iter_mut().find(|existing| existing.id == id) {
                    existing.last_error = Some(error.to_string());
                    existing.updated_at_ms = Some(now_ms());
                }
            }
        }
    }
    Ok(outcomes)
}

pub fn now_ms() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as f64)
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::domain::subscription::Subscription;
    use crate::store::Store;

    fn scratch() -> std::path::PathBuf {
        static SEQ: AtomicUsize = AtomicUsize::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("sub-{}-{}", std::process::id(), seq));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn state_with(subscription: Subscription) -> AppState {
        let dir = scratch();
        let state = AppState::load(Store::new(dir.join("store.json")));
        state.subscriptions.write().push(subscription);
        state.persist().unwrap();
        state
    }

    const CLASH: &str = r#"
proxies:
  - name: "甲"
    type: vless
    server: a.example.com
    port: 443
    uuid: u-1
  - name: "乙"
    type: trojan
    server: b.example.com
    port: 8443
    password: pw
"#;

    #[test]
    fn a_refresh_installs_the_documents_nodes() {
        let subscription = Subscription::new("机场".into(), "https://example.com/sub".into());
        let state = state_with(subscription.clone());

        let outcome = apply_document(&state, &subscription.id, CLASH, None).unwrap();

        assert_eq!(outcome.imported, 2);
        assert_eq!(outcome.removed, 0);
        assert_eq!(state.nodes.read().len(), 2);
        assert!(state.nodes.read().iter().all(|node| node.tags == vec![subscription.tag()]));
        let stored = state.subscriptions.read()[0].clone();
        assert_eq!(stored.node_count, 2);
        assert!(stored.updated_at_ms.is_some());
        assert!(stored.last_error.is_none());
    }

    #[test]
    fn a_node_two_subscriptions_share_is_kept_once() {
        let first = Subscription::new("甲机场".into(), "https://a.example.com/sub".into());
        let second = Subscription::new("乙机场".into(), "https://b.example.com/sub".into());
        let state = state_with(first.clone());
        state.subscriptions.write().push(second.clone());

        apply_document(&state, &first.id, CLASH, None).unwrap();
        let outcome = apply_document(&state, &second.id, CLASH, None).unwrap();

        assert_eq!(outcome.imported, 0, "both nodes are already here");
        assert_eq!(outcome.skipped.len(), 2, "and the user is told why");
        assert_eq!(state.nodes.read().len(), 2, "the list did not double");
    }

    #[test]
    fn a_manual_copy_of_a_subscription_node_is_not_deduplicated() {
        let subscription = Subscription::new("机场".into(), "https://example.com/sub".into());
        let state = state_with(subscription.clone());
        apply_document(&state, &subscription.id, CLASH, None).unwrap();

        let mut manual = state.nodes.read()[0].clone();
        manual.id = "manual".into();
        manual.tags = vec![];
        state.nodes.write().push(manual);

        let outcome = apply_document(&state, &subscription.id, CLASH, None).unwrap();

        assert_eq!(outcome.imported, 2, "our own batch is replaced, not deduplicated");
        assert!(state.nodes.read().iter().any(|node| node.id == "manual"));
    }

    #[test]
    fn a_second_refresh_replaces_the_previous_batch_and_keeps_manual_nodes() {
        let subscription = Subscription::new("机场".into(), "https://example.com/sub".into());
        let state = state_with(subscription.clone());
        apply_document(&state, &subscription.id, CLASH, None).unwrap();

        let mut manual = state.nodes.read()[0].clone();
        manual.id = "manual".into();
        manual.name = "自己加的".into();
        manual.tags = vec![];
        state.nodes.write().push(manual);

        let outcome = apply_document(&state, &subscription.id, CLASH, None).unwrap();

        assert_eq!(outcome.removed, 2, "the old batch is gone");
        assert_eq!(outcome.imported, 2);
        let nodes = state.nodes.read();
        assert_eq!(nodes.len(), 3, "manual node survived");
        assert!(nodes.iter().any(|node| node.id == "manual"));
    }

    #[test]
    fn a_broken_document_keeps_the_old_nodes() {
        let subscription = Subscription::new("机场".into(), "https://example.com/sub".into());
        let state = state_with(subscription.clone());
        apply_document(&state, &subscription.id, CLASH, None).unwrap();

        let outcome = apply_document(&state, &subscription.id, "not a document at all", None).unwrap();

        assert_eq!(outcome.imported, 0);
        assert_eq!(outcome.removed, 2, "a document with no nodes clears the batch");
        assert_eq!(state.nodes.read().len(), 0);
    }

    #[test]
    fn refreshing_an_unknown_source_is_an_error() {
        let subscription = Subscription::new("机场".into(), "https://example.com/sub".into());
        let state = state_with(subscription);
        let error = apply_document(&state, "sub-missing", CLASH, None).expect_err("unknown id");
        assert!(matches!(error, AppError::NotFound(_)), "got {error:?}");
    }

    #[test]
    fn a_fetch_goes_direct_unless_the_setting_asks_for_the_tunnel() {
        let subscription = Subscription::new("机场".into(), "https://example.com/sub".into());
        let state = state_with(subscription);

        assert_eq!(fetch_route(&state, true), (None, None), "off means off");
    }

    #[test]
    fn a_running_kernel_is_the_route_the_setting_asked_for() {
        let subscription = Subscription::new("机场".into(), "https://example.com/sub".into());
        let state = state_with(subscription);
        {
            let mut settings = state.settings.write();
            settings.subscription_via_proxy = true;
            settings.mixed_port = 7897;
        }

        assert_eq!(fetch_route(&state, true), (Some(7897), None));
    }

    #[test]
    fn wanting_the_tunnel_and_having_no_kernel_says_so() {
        let subscription = Subscription::new("机场".into(), "https://example.com/sub".into());
        let state = state_with(subscription);
        state.settings.write().subscription_via_proxy = true;

        let (proxy, note) = fetch_route(&state, false);
        assert_eq!(proxy, None, "a dead port is not a route");
        let note = note.expect("the user is told which route was taken");
        assert!(note.contains("直连"), "got {note}");
    }

    #[test]
    fn a_source_is_due_only_when_its_interval_has_passed() {
        let mut subscription = Subscription::new("机场".into(), "https://example.com/sub".into());
        subscription.update_interval_minutes = 60;
        assert!(subscription.is_due(now_ms()), "never fetched counts as due");

        subscription.updated_at_ms = Some(now_ms());
        assert!(!subscription.is_due(now_ms()));

        subscription.enabled = false;
        assert!(!subscription.is_due(now_ms() + 10.0 * 3_600_000.0));
    }
}
