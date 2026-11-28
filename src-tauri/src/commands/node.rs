//! Node commands — the Tauri door to the node business layer.
//!
//! The bodies live in [`crate::services::node`], shared with the headless core's
//! HTTP channel. This file keeps the command signatures, the tray refresh and
//! the "push the blocking part onto a thread" wrappers; anything a second
//! transport needs is a service call.

use std::sync::Arc;

use tauri::{AppHandle, State};

use super::engine::{run_blocking, stop_if_running_node, Proxy, Runtime};
use crate::domain::{Node, NodeId, NodeProbe};
use crate::engine::EngineRuntime;
use crate::error::AppResult;
use crate::services::node::LatencyProbe;
use crate::services::{node as svc, EmptyArgs};
use crate::state::AppState;
use std::collections::HashMap;

#[tauri::command]
#[specta::specta]
pub fn list_nodes(state: State<'_, AppState>) -> AppResult<Vec<Node>> {
    svc::list_nodes(&state, EmptyArgs {})
}

#[tauri::command]
#[specta::specta]
pub fn upsert_node(
    app: AppHandle,
    state: State<'_, AppState>,
    node: Node,
) -> AppResult<Vec<Node>> {
    let snapshot = svc::upsert_node(&state, svc::NodeArgs { node })?;
    crate::refresh_tray(&app);
    Ok(snapshot)
}

/// Add or remove one tag across a selection.
///
/// Tagging a batch is the whole point of having tags next to a selection mode:
/// a subscription gives you 40 nodes and you want to mark the ten you trust.
#[tauri::command]
#[specta::specta]
pub fn tag_nodes(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<NodeId>,
    tag: String,
    remove: bool,
) -> AppResult<Vec<Node>> {
    let snapshot = svc::tag_nodes(&state, svc::TagArgs { ids, tag, remove })?;
    crate::refresh_tray(&app);
    Ok(snapshot)
}

#[tauri::command]
#[specta::specta]
pub fn move_node(
    app: AppHandle,
    state: State<'_, AppState>,
    id: NodeId,
    delta: i32,
) -> AppResult<Vec<Node>> {
    let snapshot = svc::move_node(&state, svc::MoveArgs { id, delta })?;
    crate::refresh_tray(&app);
    Ok(snapshot)
}

#[tauri::command]
#[specta::specta]
pub async fn delete_node(
    app: AppHandle,
    state: State<'_, AppState>,
    runtime: Runtime<'_>,
    proxy: Proxy<'_>,
    id: NodeId,
) -> AppResult<Vec<Node>> {
    let gone = vec![id];
    let snapshot = svc::apply_delete(&state, &gone)?;
    stop_after_delete(runtime, proxy, gone).await?;
    crate::refresh_tray(&app);
    Ok(snapshot)
}

/// Stop the kernel when the node it runs was among those just removed.
///
/// Deleting the node under a running kernel would otherwise leave a kernel that
/// keeps proxying through something the UI can no longer render, plus a
/// `last_node_id` pointing at it. The stop itself lives in
/// [`stop_if_running_node`], which also hands the system proxy back.
async fn stop_after_delete(
    runtime: Runtime<'_>,
    proxy: Proxy<'_>,
    gone: Vec<NodeId>,
) -> AppResult<bool> {
    let runtime = Arc::clone(runtime.inner());
    let proxy = Arc::clone(proxy.inner());
    run_blocking(move || stop_if_running_node(&runtime, &proxy, &gone)).await
}

/// TCP handshake latency to a node, in milliseconds.
///
/// Runs off the UI thread: a node that silently blackholes packets costs the
/// full timeout, and the window must keep painting meanwhile.
#[tauri::command]
#[specta::specta]
pub async fn test_node_latency(state: State<'_, AppState>, id: NodeId) -> AppResult<u32> {
    let (node, timeout) = svc::latency_target(&state, &id)?;
    let target = node.id.clone();

    let outcome = run_blocking(move || Ok(svc::latency_measure(&node, timeout))).await?;

    // Success and failure are both worth remembering: "测不到" is a fact the
    // user asked for, and dropping it would make a restart look as if nothing
    // had ever been tried.
    svc::remember(&state, &target, outcome.as_ref().ok().copied());
    outcome
}

/// Every remembered latency result, keyed by node id.
#[tauri::command]
#[specta::specta]
pub fn node_probes(state: State<'_, AppState>) -> HashMap<NodeId, NodeProbe> {
    svc::node_probes(&state, EmptyArgs {})
        .unwrap_or_default()
}

/// Forget remembered results for the given ids; an empty list clears them all.
///
/// Clearing is cheap and honest: the next sweep fills the map again, and a user
/// who moved to a different network does not want yesterday's numbers.
#[tauri::command]
#[specta::specta]
pub fn clear_node_probes(
    state: State<'_, AppState>,
    ids: Vec<NodeId>,
) -> AppResult<HashMap<NodeId, NodeProbe>> {
    svc::clear_node_probes(&state, svc::IdsArgs { ids })
}

// Real delay of one node, measured through a throwaway kernel.
//
// The running kernel is the better instrument — it already holds the node — so
// this refuses while one runs instead of competing with it for ports and nodes.
// Without a kernel this is the only path that measures the tunnel rather than
// the TCP handshake, which is why it starts a kernel, sends one request through
// it and kills it again.
#[tauri::command]
#[specta::specta]
pub async fn node_delay_offline(
    state: State<'_, AppState>,
    runtime: Runtime<'_>,
    id: String,
) -> AppResult<u32> {
    delay_offline(&state, runtime.inner(), &id).await
}

// The body of `node_delay_offline`, free of `State` so the refusal paths are
// testable without spinning up a Tauri app.
pub(crate) async fn delay_offline(
    state: &AppState,
    runtime: &EngineRuntime,
    id: &str,
) -> AppResult<u32> {
    let request = svc::delay_offline_target(state, runtime, id)?;
    let outcome = run_blocking(move || Ok(svc::delay_offline_probe(&request))).await?;
    svc::delay_offline_finish(state, id, outcome)
}

/// Pick the targets, probe them and remember the numbers — the round behind
/// [`test_nodes_latency`].
///
/// Lives outside the command so the startup probe and the hotkey can run the
/// exact same round: `State` only exists inside a command, and duplicating the
/// pick/probe/remember steps is how the two paths would drift apart.
pub(crate) async fn probe_nodes(
    state: &AppState,
    ids: Vec<NodeId>,
) -> AppResult<Vec<LatencyProbe>> {
    let (targets, timeout_ms, workers) = svc::probe_targets(state, &ids)?;

    if targets.is_empty() {
        return Ok(Vec::new());
    }

    let probes = run_blocking(move || Ok(svc::probe_all(&targets, timeout_ms, workers))).await?;

    svc::remember_batch(state, &probes);
    Ok(probes)
}

/// TCP handshake latency for a list of nodes, in milliseconds.
///
/// An empty list means every node: that is what "test all" wants, and asking the
/// frontend to enumerate the same rows it is about to render only creates a
/// chance for the two lists to disagree.
#[tauri::command]
#[specta::specta]
pub async fn test_nodes_latency(
    state: State<'_, AppState>,
    ids: Vec<NodeId>,
) -> AppResult<Vec<LatencyProbe>> {
    probe_nodes(&state, ids).await
}

/// Delete several nodes in one call.
///
/// The UI offers a selection, so one round trip per node would be both slow and
/// half-applied if the window closed in the middle. Returns the remaining list,
/// like every other mutation here.
#[tauri::command]
#[specta::specta]
pub async fn delete_nodes(
    app: AppHandle,
    state: State<'_, AppState>,
    runtime: Runtime<'_>,
    proxy: Proxy<'_>,
    ids: Vec<NodeId>,
) -> AppResult<Vec<Node>> {
    let snapshot = svc::apply_delete(&state, &ids)?;
    stop_after_delete(runtime, proxy, ids).await?;
    crate::refresh_tray(&app);
    Ok(snapshot)
}

/// Share links for the given nodes, one per line — what "copy" hands to the
/// clipboard. A node that cannot be expressed as a link fails the whole call
/// rather than producing a silently truncated list.
#[tauri::command]
#[specta::specta]
pub fn export_node_links(state: State<'_, AppState>, ids: Vec<NodeId>) -> AppResult<String> {
    svc::export_node_links(&state, svc::IdsArgs { ids })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ProbeKind, Protocol, Security, Transport};
    use crate::engine::runtime::tests::{harness, Harness};
    use crate::engine::{EngineKind, Endpoints, RunOptions};
    use crate::error::AppError;
    use crate::services::node::{
        apply_delete, apply_move, apply_tag, probe_all, remember, remember_batch,
        tcp_handshake_refusal,
    };
    use crate::store::Store;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn scratch() -> std::path::PathBuf {
        static SEQ: AtomicUsize = AtomicUsize::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("node-tags-{}-{}", std::process::id(), seq));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn node(id: &str, tags: &[&str]) -> Node {
        Node {
            id: id.into(),
            name: id.into(),
            server: "example.com".into(),
            port: 443,
            protocol: Protocol::Vless {
                uuid: "u-1".into(),
                flow: None,
            },
            transport: Transport::Tcp,
            security: Security::None,
            tags: tags.iter().map(|tag| tag.to_string()).collect(),
        }
    }

    fn state_with(nodes: Vec<Node>) -> AppState {
        let state = AppState::load(Store::new(scratch().join("store.json")));
        state.nodes.write().extend(nodes);
        state
    }

    /// The runtime fakes with a kernel that reports itself as running. No real
    /// process exists: `FakeSpawner` records the spawn and answers with a pid.
    fn running_runtime() -> Harness {
        let harness = harness(true, false);
        harness
            .runtime
            .start(
                EngineKind::Xray,
                &crate::engine::runtime::tests::node(),
                Endpoints::new(7890, 0, 0, 9090).unwrap(),
                RunOptions::new(),
            )
            .unwrap();
        harness
    }

    fn cleanup(harness: Harness) {
        let dir = harness.dir.clone();
        drop(harness);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn offline_delay_refuses_while_the_main_kernel_runs() {
        // A second kernel for the same node would fight the running one for
        // ports and ports-to-nothing: the honest answer is "use the one you
        // already have".
        let state = state_with(vec![node("n1", &[])]);
        let harness = running_runtime();
        assert!(harness.runtime.status().running);

        let error = tauri::async_runtime::block_on(delay_offline(&state, &harness.runtime, "n1"))
            .unwrap_err();
        assert!(matches!(&error, AppError::InvalidInput(_)), "{error}");
        assert!(error.to_string().contains("内核正在运行"), "{error}");
        cleanup(harness);
    }

    #[test]
    fn a_refusal_keeps_the_remembered_real_delay() {
        // A refusal measured nothing, so overwriting a real number with
        // "测不到" would destroy a fact to report an error.
        let state = state_with(vec![node("n1", &[])]);
        state
            .record_probe(&"n1".to_string(), Some(321), ProbeKind::Real)
            .unwrap();
        let harness = running_runtime();

        assert!(
            tauri::async_runtime::block_on(delay_offline(&state, &harness.runtime, "n1")).is_err()
        );

        let probes = state.probes.read();
        assert_eq!(probes["n1"].delay_ms, Some(321));
        assert_eq!(probes["n1"].kind, ProbeKind::Real);
        drop(probes);
        cleanup(harness);
    }

    #[test]
    fn a_quic_node_is_refused_a_tcp_handshake_number() {
        // Printing a millisecond figure for a node we never timed would be a
        // measurement that measured nothing.
        let mut quic = node("q", &[]);
        quic.protocol = Protocol::Hysteria2 {
            password: "p".into(),
            obfs_password: None,
            up_mbps: None,
            down_mbps: None,
        };
        let refusal = tcp_handshake_refusal(&quic).expect("QUIC node must be refused");
        let text = format!("{refusal}");
        assert!(text.contains("QUIC"), "message says why: {text}");
        assert!(text.contains("真实延迟"), "message says what to use: {text}");
        assert!(tcp_handshake_refusal(&node("t", &[])).is_none());
    }

    #[test]
    fn tags_are_trimmed_and_deduplicated() {
        let dirty = node("a", &["  home ", "home", "", "  "]);
        assert_eq!(dirty.normalized_tags(), vec!["home".to_string()]);
    }

    #[test]
    fn a_hand_made_subscription_tag_is_refused() {
        // Refresh deletes nodes by tag, so a tag the user typed that looks like
        // a subscription tag would be silently deleted later.
        let error = node("a", &["sub:sub-1234"]).check_user_tags().unwrap_err();
        assert!(format!("{error}").contains("sub:"));
    }

    #[test]
    fn tagging_a_selection_touches_only_the_selected_nodes() {
        let state = state_with(vec![node("a", &[]), node("b", &[]), node("c", &["keep"])]);

        let all = apply_tag(&state, &["a".into(), "b".into()], "  trusted  ", false).unwrap();

        assert_eq!(all.len(), 3);
        assert_eq!(all[0].tags, vec!["trusted".to_string()]);
        assert_eq!(all[1].tags, vec!["trusted".to_string()]);
        assert_eq!(all[2].tags, vec!["keep".to_string()]);
    }

    #[test]
    fn tagging_twice_does_not_duplicate_the_tag() {
        let state = state_with(vec![node("a", &[])]);

        apply_tag(&state, &["a".into()], "trusted", false).unwrap();
        let all = apply_tag(&state, &["a".into()], "trusted", false).unwrap();

        assert_eq!(all[0].tags, vec!["trusted".to_string()]);
    }

    #[test]
    fn removing_a_tag_leaves_the_others_alone() {
        let state = state_with(vec![node("a", &["trusted", "fast"])]);

        let all = apply_tag(&state, &["a".into()], "trusted", true).unwrap();

        assert_eq!(all[0].tags, vec!["fast".to_string()]);
    }

    #[test]
    fn tagging_nothing_or_an_empty_tag_is_an_error() {
        let state = state_with(vec![node("a", &[])]);

        assert!(apply_tag(&state, &[], "trusted", false).is_err());
        assert!(apply_tag(&state, &["a".into()], "   ", false).is_err());
        assert!(apply_tag(&state, &["missing".into()], "trusted", false).is_err());
    }

    #[test]
    fn a_batch_keep_the_order_it_was_asked_in() {
        // A shuffled answer would look like the cards rearranging themselves,
        // and the frontend pairs the probes back by index.
        let nodes: Vec<Node> = (0..8).map(|index| node(&format!("n{index}"), &[])).collect();
        let probes = probe_all(&nodes, 100, 4);

        assert_eq!(
            probes.iter().map(|probe| probe.id.as_str()).collect::<Vec<_>>(),
            nodes.iter().map(|item| item.id.as_str()).collect::<Vec<_>>()
        );
        assert_eq!(probes.len(), nodes.len());
    }

    #[test]
    fn a_quic_node_in_a_batch_says_why_instead_of_going_missing() {
        // The row has to exist either way: a node with no answer at all reads
        // as "the app forgot about it", which is a different bug report.
        let mut quic = node("q", &[]);
        quic.protocol = Protocol::Hysteria2 {
            password: "p".into(),
            obfs_password: None,
            up_mbps: None,
            down_mbps: None,
        };

        let probes = probe_all(&[quic], 100, 1);

        assert_eq!(probes.len(), 1);
        assert!(probes[0].ms.is_none());
        assert!(probes[0].error.as_deref().unwrap_or("").contains("QUIC"));
    }

    #[test]
    fn one_worker_for_a_dozen_nodes_is_still_the_right_answer() {
        // The clamp exists so a setting of 0 cannot spawn zero threads and
        // deadlock the wait, and so more workers than nodes is not an error.
        let nodes: Vec<Node> = (0..3).map(|index| node(&format!("n{index}"), &[])).collect();

        assert_eq!(probe_all(&nodes, 100, 0).len(), 3);
        assert_eq!(probe_all(&nodes, 100, 64).len(), 3);
    }

    #[test]
    fn deleting_a_node_forgets_its_probe() {
        let state = state_with(vec![node("a", &[]), node("b", &[])]);
        state
            .record_probe(&"a".to_string(), Some(90), ProbeKind::Tcp)
            .unwrap();
        state
            .record_probe(&"b".to_string(), None, ProbeKind::Tcp)
            .unwrap();

        let left = apply_delete(&state, &["a".to_string()]).unwrap();

        assert_eq!(
            left.iter().map(|item| item.id.as_str()).collect::<Vec<_>>(),
            vec!["b"]
        );
        // A record for a node nobody can see or clear would live on forever.
        let probes = state.probes.read();
        assert!(!probes.contains_key("a"));
        assert!(probes.contains_key("b"));
        drop(probes);

        assert!(apply_delete(&state, &[]).is_err());
        assert!(matches!(
            apply_delete(&state, &["nope".to_string()]),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn deleting_the_node_the_app_was_using_forgets_it() {
        let state = state_with(vec![node("a", &[]), node("b", &[])]);
        state.settings.write().last_node_id = Some("a".to_string());

        apply_delete(&state, &["a".to_string()]).unwrap();

        // An id that outlives its node would make the app preselect nothing, or
        // claim a kernel is running something that cannot be rendered.
        assert_eq!(state.settings.read().last_node_id, None);
    }

    #[test]
    fn deleting_another_node_keeps_the_remembered_one() {
        let state = state_with(vec![node("a", &[]), node("b", &[])]);
        state.settings.write().last_node_id = Some("a".to_string());

        apply_delete(&state, &["b".to_string()]).unwrap();

        assert_eq!(state.settings.read().last_node_id.as_deref(), Some("a"));
    }

    #[test]
    fn a_failed_probe_is_stored_as_no_result() {
        // "测不到" has to be a stored fact, not a missing entry: otherwise a
        // restart cannot tell a failed sweep from a node that was never tested.
        let state = state_with(vec![node("a", &[])]);

        remember(&state, &"a".to_string(), None);

        let probes = state.probes.read();
        let probe = probes.get("a").expect("a failed probe still leaves a record");
        assert_eq!(probe.delay_ms, None);
        assert_eq!(probe.kind, ProbeKind::Tcp);
        assert!(probe.at > 0);
    }

    #[test]
    fn a_quic_refusal_does_not_overwrite_a_measurement() {
        // The refusal row exists so the card can explain itself; it must not be
        // written back as if the node had just been measured and failed.
        let mut quic = node("q", &[]);
        quic.protocol = Protocol::Hysteria2 {
            password: "p".into(),
            obfs_password: None,
            up_mbps: None,
            down_mbps: None,
        };
        let state = state_with(vec![quic.clone()]);
        state
            .record_probe(&"q".to_string(), Some(55), ProbeKind::Real)
            .unwrap();

        let rows = probe_all(std::slice::from_ref(&quic), 100, 1);
        remember_batch(&state, &rows);

        let probes = state.probes.read();
        let stored = probes.get("q").expect("old record survives");
        assert_eq!(stored.delay_ms, Some(55));
        assert_eq!(stored.kind, ProbeKind::Real);
    }

    #[test]
    fn moving_a_node_swaps_it_with_its_neighbour() {
        let state = state_with(vec![node("a", &[]), node("b", &[])]);

        let moved = apply_move(&state, "b", -1).unwrap();
        assert_eq!(
            moved.iter().map(|item| item.id.as_str()).collect::<Vec<_>>(),
            vec!["b", "a"]
        );

        // Past the top: nothing happens, and it is not an error.
        let same = apply_move(&state, "b", -1).unwrap();
        assert_eq!(
            same.iter().map(|item| item.id.as_str()).collect::<Vec<_>>(),
            vec!["b", "a"]
        );

        // Unknown id is a real error, unlike "already at the end".
        assert!(matches!(apply_move(&state, "nope", 1), Err(AppError::NotFound(_))));
    }
}