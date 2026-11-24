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
use crate::sysproxy::ProxyGuard;

use super::engine::{run_blocking, stop_if_running_node, Proxy, Runtime};

/// What one refresh did, so the UI can say something specific.
///
/// `imported`, `removed`, `filtered` and `skipped` are four different
/// stories: nodes that made it in, nodes the previous batch lost, nodes this
/// source's own rename/filter policy asked us to drop, and lines the provider
/// sent that could not be used (unreadable, or a duplicate of another source).
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct RefreshOutcome {
    pub subscription: Subscription,
    #[specta(type = f64)]
    pub imported: usize,
    #[specta(type = f64)]
    pub removed: usize,
    /// Nodes the subscription's `include`/`exclude` patterns kept out of the
    /// table. Kept apart from duplicates on purpose: "the provider sent 300,
    /// 200 matched your filter" is not the same complaint as "100 were already
    /// here", and the UI says which one happened.
    #[specta(type = f64)]
    pub filtered: u32,
    pub skipped: Vec<String>,
    /// Something true about this refresh that is not about the body: which
    /// route it took, for instance. Kept apart from `skipped` because that list
    /// counts lines the parser could not read, and a route note is not a line.
    pub note: Option<String>,
}

/// What applying a document did to the kernel's world, as opposed to the table.
///
/// Deliberately not part of [`RefreshOutcome`]: the UI has no business seeing
/// it. It only decides whether the kernel must stop, and whether its remembered
/// node id has to follow a row that was re-imported under a new id.
#[derive(Debug, Clone, Default)]
pub(crate) struct RefreshEffects {
    /// Remembered ids that name no row any more and have no replacement: the
    /// server really went away, so a kernel running it must stop.
    pub gone: Vec<String>,
    /// The remembered id and the id that now names the same server. A rename,
    /// not a disappearance: the kernel keeps running under the new id.
    pub adopted: Option<(String, String)>,
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
        // A subscription fetch is an HTTP client, so it has to go through the
        // HTTP inbound; with a split the configured mixed port has no listener
        // at all. `resolve_port` is the same `0 = follow mixed_port` rule the
        // renderer uses.
        let port = crate::engine::Endpoints::resolve_port(settings.mixed_port, settings.http_port);
        return (Some(port), None);
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
pub async fn delete_subscription(
    app: AppHandle,
    state: State<'_, AppState>,
    runtime: Runtime<'_>,
    proxy: Proxy<'_>,
    id: String,
) -> AppResult<Vec<Subscription>> {
    let snapshot = apply_delete_subscription(&state, runtime.inner(), proxy.inner(), &id)?;
    crate::refresh_tray(&app);
    Ok(snapshot)
}

/// The command's body: `State<'_, _>` cannot be built outside a running app,
/// and this is the part worth testing.
///
/// Dropping a source drops its nodes, and a kernel running one of them would
/// otherwise keep proxying through a node the UI can no longer render, with the
/// system proxy still pointing at its port. The stop itself lives in
/// [`stop_if_running_node`], which also hands the proxy back.
pub fn apply_delete_subscription(
    state: &AppState,
    runtime: &EngineRuntime,
    proxy: &ProxyGuard,
    id: &str,
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
    // Nodes that came from this source go with it, and so do the latency
    // records that describe them: a record for a node nobody can click is
    // dead weight in every future document write.
    let dropped = {
        let mut nodes = state.nodes.write();
        let mut dropped: Vec<String> = Vec::new();
        nodes.retain(|node| {
            let stale = node.tags.iter().any(|t| t == &tag);
            if stale {
                dropped.push(node.id.clone());
            }
            !stale
        });
        dropped
    };
    state.drop_probes(&dropped);
    // Settings and nodes are separate locks, and this one is only taken once the
    // nodes lock is released, so the two can never dead-lock. A `last_node_id`
    // naming a node that no longer exists would make the app claim it is running
    // something it cannot even render.
    {
        let mut settings = state.settings.write();
        if settings
            .last_node_id
            .as_ref()
            .is_some_and(|last| dropped.iter().any(|id| id == last))
        {
            settings.last_node_id = None;
        }
    }
    state.persist()?;
    // After the durable change, exactly like deleting a single node: the record
    // is consistent even if handing the system proxy back fails.
    stop_if_running_node(runtime, proxy, &dropped)?;
    Ok(state.subscriptions.read().clone())
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
    let subscription = state
        .subscriptions
        .read()
        .iter()
        .find(|existing| existing.id == id)
        .cloned()
        .ok_or_else(|| AppError::NotFound(format!("订阅 {id} 不存在")))?;

    // The UA override travels with the request: some providers gate on it.
    let agent = state.settings.read().subscription_user_agent.clone();
    let (tunnel, note) = fetch_route(&state, runtime.status().running);
    let fetched = tauri::async_runtime::spawn_blocking(move || {
        let fetched = remote::fetch_through(
            &subscription.url,
            remote::FETCH_TIMEOUT,
            Some(&agent),
            tunnel,
        )?;
        Ok::<_, AppError>(fetched)
    })
    .await
    .map_err(|error| AppError::Internal(format!("后台任务失败：{error}")))??;

    // What the settings named before the document was applied is read inside
    // `apply_document` itself: the effects it reports are the only ones that
    // know whether the remembered node was replaced by the same server or lost.
    let (mut outcome, effects) = apply_document(
        &state,
        &id,
        &fetched.body,
        fetched.subscription_userinfo.as_deref(),
    )?;
    outcome.note = note;
    // The same server coming back under a new id is a rename, not a delete: the
    // kernel keeps running, and its id follows the row so a later delete of that
    // node still finds it.
    if let Some((from, to)) = &effects.adopted {
        runtime.adopt_node_id(from, to);
    }
    if !effects.gone.is_empty() {
        let runtime = Arc::clone(runtime.inner());
        let proxy = Arc::clone(proxy.inner());
        let gone = effects.gone;
        run_blocking(move || stop_if_running_node(&runtime, &proxy, &gone)).await?;
    }
    // A refresh rewrites the node table, and the tray lists node names.
    crate::refresh_tray(&app);
    Ok(outcome)
}

/// Blocking refresh, for the background sweep (which owns no async runtime).
pub fn refresh_one_blocking(
    state: &AppState,
    runtime: &EngineRuntime,
    proxy: &ProxyGuard,
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
    let (tunnel, note) = fetch_route(state, engine_running);
    let fetched =
        remote::fetch_through(&subscription.url, remote::FETCH_TIMEOUT, Some(&agent), tunnel)?;
    let (mut outcome, effects) = apply_document(
        state,
        id,
        &fetched.body,
        fetched.subscription_userinfo.as_deref(),
    )?;
    outcome.note = note;
    // Same adoption as the command path: a re-imported node is a rename, so the
    // kernel follows the new id instead of stopping.
    if let Some((from, to)) = &effects.adopted {
        runtime.adopt_node_id(from, to);
    }
    // The sweep thread cannot ask the UI anything, so it stops the kernel here
    // and lets the page find out through its normal status polling.
    if !effects.gone.is_empty() {
        stop_if_running_node(runtime, proxy, &effects.gone)?;
    }
    Ok(outcome)
}

/// The pure half: a document in, the node table rewritten.
///
/// Split out so the refresh path can be tested without a network and without
/// a running app. The [`RefreshEffects`] it returns are, together with the
/// [`RefreshOutcome`], everything a caller needs: the outcome is for the user,
/// the effects are for the kernel.
pub fn apply_document(
    state: &AppState,
    id: &str,
    text: &str,
    userinfo: Option<&str>,
) -> AppResult<(RefreshOutcome, RefreshEffects)> {
    let report = parse_document(text);

    let mut subscription = {
        let subscriptions = state.subscriptions.read();
        subscriptions
            .iter()
            .find(|existing| existing.id == id)
            .cloned()
            .ok_or_else(|| AppError::NotFound(format!("订阅 {id} 不存在")))?
    };
    // A store from before this feature, or one edited by hand, can carry a
    // pattern that does not compile. Refuse the refresh instead of quietly
    // keeping every node the user asked to filter out.
    subscription.validate()?;
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

    // The source's own policy runs before the dedup pass. A node the filters
    // rejected is not a duplicate of anything, and counting it as one would
    // tell the user their provider repeated itself when it did not. The prefix
    // is applied here too, so the name stored is the name the user will see.
    let mut filtered: u32 = 0;
    imported_nodes.retain_mut(|node| {
        if subscription.tune(node) {
            true
        } else {
            filtered += 1;
            false
        }
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

    // The node the settings name may be running in the kernel, and a refresh
    // mints a fresh id for every row it imports. What that row *is* has to be
    // taken before the swap: afterwards the id is the only evidence left, and
    // its fingerprint is how a re-imported server is told apart from a server
    // that really went away.
    let remembered = state.settings.read().last_node_id.clone();
    let remembered_fingerprint = remembered.as_ref().and_then(|id| {
        state
            .nodes
            .read()
            .iter()
            .find(|node| &node.id == id)
            .map(|node| node.fingerprint())
    });

    let (removed, dropped) = {
        let mut nodes = state.nodes.write();
        let before = nodes.len();
        let mut dropped: Vec<String> = Vec::new();
        nodes.retain(|node| {
            let stale = node.tags.iter().any(|t| t == &tag);
            if stale {
                dropped.push(node.id.clone());
            }
            !stale
        });
        let removed = before - nodes.len();
        nodes.extend(imported_nodes.iter().cloned());
        (removed, dropped)
    };
    state.drop_probes(&dropped);

    // The settings remember which node the app is on, and that node may just
    // have been replaced. Clearing or re-pointing the id needs its own lock, so
    // it is taken only now, with the node table released. A remembered id that
    // names no row is either a rename — the same server under a fresh id — or a
    // disappearance; only the second must take the kernel down with it, which is
    // why the fingerprint was captured before the swap.
    let mut effects = RefreshEffects::default();
    if let Some(old) = remembered {
        if !state.nodes.read().iter().any(|node| node.id == old) {
            let replacement = remembered_fingerprint
                .as_deref()
                .and_then(|fingerprint| replacement_for(state, &tag, fingerprint, &old));
            let mut settings = state.settings.write();
            match replacement {
                Some(new) => {
                    settings.last_node_id = Some(new.clone());
                    effects.adopted = Some((old, new));
                }
                None => {
                    settings.last_node_id = None;
                    effects.gone.push(old);
                }
            }
        }
    }

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

    Ok((
        RefreshOutcome {
            subscription,
            imported: imported_nodes.len(),
            removed,
            filtered,
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
        },
        effects,
    ))
}

/// The row that now names the same server as one that is gone.
///
/// This source's own fresh copy wins: a provider that renames nodes must not
/// hand the kernel to an identical node owned by somebody else. Falling back to
/// any match keeps the promise that one server is one node, even when the same
/// server moved from one subscription to another.
fn replacement_for(state: &AppState, tag: &str, fingerprint: &str, old: &str) -> Option<String> {
    let nodes = state.nodes.read();
    let own_copy = nodes.iter().find(|node| {
        node.id != old && node.tags.iter().any(|t| t == tag) && node.fingerprint() == fingerprint
    });
    own_copy
        .or_else(|| {
            nodes
                .iter()
                .find(|node| node.id != old && node.fingerprint() == fingerprint)
        })
        .map(|node| node.id.clone())
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
    use crate::commands::engine::tests::proxy;
    use crate::domain::subscription::Subscription;
    use crate::engine::runtime::tests::harness;
    use crate::engine::{Endpoints, EngineKind, RunOptions};
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

    /// The same provider after 甲 went away: only 乙 is still listed.
    const TROJAN_ONLY: &str = r#"
proxies:
  - name: "乙"
    type: trojan
    server: b.example.com
    port: 8443
    password: pw
"#;

    #[test]
    fn a_refresh_that_reimports_the_remembered_node_adopts_the_new_id() {
        let subscription = Subscription::new("机场".into(), "https://example.com/sub".into());
        let state = state_with(subscription.clone());
        apply_document(&state, &subscription.id, CLASH, None).unwrap();
        let first = state.nodes.read()[0].id.clone();
        state.settings.write().last_node_id = Some(first.clone());

        let (_, effects) = apply_document(&state, &subscription.id, CLASH, None).unwrap();

        // Every refreshed row carries a fresh id, but the server behind it is the
        // same one: this is a rename, so the settings follow it and the kernel is
        // left running instead of being stopped for a routine sweep.
        let (from, to) = effects.adopted.clone().expect("the same server came back");
        assert_eq!(from, first);
        assert_ne!(to, first);
        assert!(effects.gone.is_empty(), "a rename must not stop the kernel");
        assert!(state.nodes.read().iter().any(|node| node.id == to));
        assert_eq!(state.settings.read().last_node_id.as_deref(), Some(to.as_str()));
    }

    #[test]
    fn a_refresh_whose_server_is_really_gone_reports_it_and_forgets_it() {
        let subscription = Subscription::new("机场".into(), "https://example.com/sub".into());
        let state = state_with(subscription.clone());
        apply_document(&state, &subscription.id, CLASH, None).unwrap();
        let first = state.nodes.read()[0].id.clone();
        state.settings.write().last_node_id = Some(first.clone());

        let (_, effects) = apply_document(&state, &subscription.id, TROJAN_ONLY, None).unwrap();

        // 甲 is not in the document any more and no row has its fingerprint: the
        // kernel is running something the user cannot select, so it has to stop.
        assert_eq!(effects.gone, vec![first], "the caller is told which id to stop");
        assert!(effects.adopted.is_none());
        assert_eq!(state.settings.read().last_node_id, None);
    }

    #[test]
    fn a_remembered_node_the_refresh_did_not_touch_is_left_alone() {
        let subscription = Subscription::new("机场".into(), "https://example.com/sub".into());
        let state = state_with(subscription.clone());
        apply_document(&state, &subscription.id, CLASH, None).unwrap();

        // A hand-added node belongs to nobody's subscription, so the swap leaves
        // it where it is — and the settings keep naming it.
        let mut manual = state.nodes.read()[0].clone();
        manual.id = "manual".into();
        manual.tags = vec![];
        manual.server = "manual.example.com".into();
        state.nodes.write().push(manual);
        state.settings.write().last_node_id = Some("manual".into());

        let (_, effects) = apply_document(&state, &subscription.id, CLASH, None).unwrap();

        assert!(effects.gone.is_empty(), "the remembered row is still here");
        assert!(effects.adopted.is_none(), "and it did not move");
        assert_eq!(state.settings.read().last_node_id.as_deref(), Some("manual"));
    }

    #[test]
    fn deleting_a_subscription_stops_the_kernel_on_its_node_and_forgets_it() {
        let subscription = Subscription::new("机场".into(), "https://example.com/sub".into());
        let state = state_with(subscription.clone());
        apply_document(&state, &subscription.id, CLASH, None).unwrap();
        // 乙 is the trojan entry, so it can be handed to the Xray fake as-is.
        let running = state.nodes.read()[1].clone();
        state.settings.write().last_node_id = Some(running.id.clone());

        let h = harness(true, false);
        h.runtime
            .start(
                EngineKind::Xray,
                &running,
                Endpoints::new(7890, 0, 0, 9090).unwrap(),
                RunOptions::new(),
            )
            .unwrap();
        let (guard, fake, path) = proxy();
        guard.enable_with_bypass("127.0.0.1", 7890, "").unwrap();
        assert!(guard.is_active());

        let remaining =
            apply_delete_subscription(&state, &h.runtime, &guard, &subscription.id).unwrap();

        assert!(remaining.is_empty(), "the source is gone");
        assert!(state.nodes.read().is_empty(), "and so are the nodes it owned");
        // The kernel was running a node nobody can select any more.
        assert!(!h.runtime.status().running);
        assert!(!guard.is_active(), "and the machine is handed back");
        assert!(!fake.state.lock().enabled);
        assert_eq!(state.settings.read().last_node_id, None);

        std::fs::remove_dir_all(&h.dir).ok();
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn deleting_a_subscription_leaves_a_kernel_on_another_node_alone() {
        let subscription = Subscription::new("机场".into(), "https://example.com/sub".into());
        let state = state_with(subscription.clone());
        apply_document(&state, &subscription.id, CLASH, None).unwrap();
        // A hand-added node survives the delete, and so must its kernel.
        let mut manual = state.nodes.read()[0].clone();
        manual.id = "manual".into();
        manual.tags = vec![];
        manual.server = "manual.example.com".into();
        state.nodes.write().push(manual.clone());
        state.settings.write().last_node_id = Some("manual".into());

        let h = harness(true, false);
        h.runtime
            .start(
                EngineKind::Xray,
                &manual,
                Endpoints::new(7890, 0, 0, 9090).unwrap(),
                RunOptions::new(),
            )
            .unwrap();
        let (guard, fake, path) = proxy();
        guard.enable_with_bypass("127.0.0.1", 7890, "").unwrap();

        apply_delete_subscription(&state, &h.runtime, &guard, &subscription.id).unwrap();

        assert_eq!(state.nodes.read().len(), 1, "only the manual node is left");
        assert!(h.runtime.status().running, "its kernel keeps running");
        assert!(guard.is_active(), "and the proxy is untouched");
        assert_eq!(fake.writes.lock().len(), 1, "only the enable");
        assert_eq!(state.settings.read().last_node_id.as_deref(), Some("manual"));

        std::fs::remove_dir_all(&h.dir).ok();
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_refresh_installs_the_documents_nodes() {
        let subscription = Subscription::new("机场".into(), "https://example.com/sub".into());
        let state = state_with(subscription.clone());

        let (outcome, _) = apply_document(&state, &subscription.id, CLASH, None).unwrap();

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
        let (outcome, _) = apply_document(&state, &second.id, CLASH, None).unwrap();

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

        let (outcome, _) = apply_document(&state, &subscription.id, CLASH, None).unwrap();

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

        let (outcome, _) = apply_document(&state, &subscription.id, CLASH, None).unwrap();

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

        let (outcome, _) = apply_document(&state, &subscription.id, "not a document at all", None).unwrap();

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

    #[test]
    fn filtered_nodes_are_counted_apart_from_duplicates() {
        let mut subscription = Subscription::new("机场".into(), "https://example.com/sub".into());
        subscription.include = "甲".into();
        let state = state_with(subscription.clone());

        let (outcome, _) = apply_document(&state, &subscription.id, CLASH, None).unwrap();

        assert_eq!(outcome.imported, 1);
        assert_eq!(outcome.filtered, 1);
        assert!(
            outcome.skipped.is_empty(),
            "a filtered node is not a duplicate: {:?}",
            outcome.skipped
        );
        assert_eq!(state.nodes.read().len(), 1);
        assert_eq!(state.nodes.read()[0].name, "甲");
    }

    #[test]
    fn exclude_drops_nodes_and_the_rest_keep_their_prefix() {
        let mut subscription = Subscription::new("机场".into(), "https://example.com/sub".into());
        subscription.rename_prefix = "机场A · ".into();
        subscription.exclude = "乙".into();
        let state = state_with(subscription.clone());

        let (outcome, _) = apply_document(&state, &subscription.id, CLASH, None).unwrap();

        assert_eq!(outcome.imported, 1);
        assert_eq!(outcome.filtered, 1);
        let nodes = state.nodes.read();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].name, "机场A · 甲");
    }

    #[test]
    fn a_prefix_does_not_hide_a_duplicate_from_another_source() {
        // Dedup compares endpoints, not labels, so two sources that rename the
        // same servers differently still collapse into one entry.
        let mut first = Subscription::new("甲机场".into(), "https://a.example.com/sub".into());
        first.rename_prefix = "甲 · ".into();
        let mut second = Subscription::new("乙机场".into(), "https://b.example.com/sub".into());
        second.rename_prefix = "乙 · ".into();
        let state = state_with(first.clone());
        state.subscriptions.write().push(second.clone());

        apply_document(&state, &first.id, CLASH, None).unwrap();
        let (outcome, _) = apply_document(&state, &second.id, CLASH, None).unwrap();

        assert_eq!(outcome.imported, 0);
        assert_eq!(outcome.filtered, 0);
        assert_eq!(outcome.skipped.len(), 2, "both are reported as duplicates");
        assert_eq!(state.nodes.read().len(), 2, "the list did not double");
        assert!(state.nodes.read().iter().all(|node| node.name.starts_with("甲 · ")));
    }

    #[test]
    fn a_stored_pattern_that_does_not_compile_refuses_the_refresh() {
        // `upsert_subscription` validates, so this can only come from an older
        // or hand-edited store. Keeping every node silently would be a lie.
        let mut subscription = Subscription::new("机场".into(), "https://example.com/sub".into());
        subscription.include = "(香港".into();
        let state = state_with(subscription.clone());

        let error = apply_document(&state, &subscription.id, CLASH, None)
            .expect_err("a broken filter must not be ignored");
        assert!(matches!(error, AppError::InvalidInput(_)), "got {error:?}");
        assert!(state.nodes.read().is_empty(), "nothing was installed");
    }
}
