use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, State};

use super::engine::{
    lookup_node, probe_url, remember_real, run_blocking, stop_if_running_node, Proxy, Runtime,
};
use crate::domain::{Node, NodeId, NodeProbe, ProbeKind};
use crate::engine::temp::Attempt;
use crate::engine::{EngineKind, EngineRuntime};
use crate::error::{AppError, AppResult};
use crate::state::AppState;

#[tauri::command]
#[specta::specta]
pub fn list_nodes(state: State<'_, AppState>) -> AppResult<Vec<Node>> {
    Ok(state.nodes.read().clone())
}

#[tauri::command]
#[specta::specta]
pub fn upsert_node(
    app: AppHandle,
    state: State<'_, AppState>,
    mut node: Node,
) -> AppResult<Vec<Node>> {
    node.validate()?;
    node.check_user_tags()?;
    node.tags = node.normalized_tags();

    let snapshot = {
        let mut nodes = state.nodes.write();
        match nodes.iter_mut().find(|n| n.id == node.id) {
            Some(existing) => *existing = node,
            None => nodes.push(node),
        }
        nodes.clone()
    };
    state.persist()?;
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
    let snapshot = apply_tag(&state, &ids, &tag, remove)?;
    crate::refresh_tray(&app);
    Ok(snapshot)
}

/// The command above is a thin wrapper: `State<'_, AppState>` cannot be built
/// outside a running app, and this is the part worth testing.
pub fn apply_tag(
    state: &AppState,
    ids: &[NodeId],
    tag: &str,
    remove: bool,
) -> AppResult<Vec<Node>> {
    let tag = tag.trim().to_string();
    if tag.is_empty() {
        return Err(AppError::InvalidInput("标签不能为空".into()));
    }
    if tag.starts_with(crate::domain::subscription::SOURCE_TAG_PREFIX) {
        return Err(AppError::InvalidInput(format!(
            "标签不能以 {} 开头（那是订阅自己用的）",
            crate::domain::subscription::SOURCE_TAG_PREFIX
        )));
    }
    if ids.is_empty() {
        return Err(AppError::InvalidInput("没有选中任何节点".into()));
    }

    let snapshot = {
        let mut nodes = state.nodes.write();
        let mut touched = 0usize;
        for node in nodes.iter_mut() {
            if !ids.contains(&node.id) {
                continue;
            }
            touched += 1;
            if remove {
                node.tags.retain(|existing| existing != &tag);
            } else if !node.tags.iter().any(|existing| existing == &tag) {
                node.tags.push(tag.clone());
            }
            node.tags = node.normalized_tags();
        }
        if touched == 0 {
            return Err(AppError::NotFound("选中的节点都不存在".into()));
        }
        nodes.clone()
    };
    state.persist()?;
    Ok(snapshot)
}

/// Move a node one slot up or down in the list.
///
/// The stored order *is* the manual order — there is no `sort_index` to keep in
/// sync, and every other view sorts a copy. Moving past either end is a no-op
/// rather than an error: the button is disabled there, so an out-of-range delta
/// only happens on a double click, which should not produce a toast.
pub fn apply_move(state: &AppState, id: &str, delta: i32) -> AppResult<Vec<Node>> {
    let snapshot = {
        let mut nodes = state.nodes.write();
        let index = nodes
            .iter()
            .position(|node| node.id == id)
            .ok_or_else(|| AppError::NotFound(format!("node {id}")))?;
        let target = index as i64 + delta as i64;
        if target < 0 || target >= nodes.len() as i64 {
            return Ok(nodes.clone());
        }
        nodes.swap(index, target as usize);
        nodes.clone()
    };
    state.persist()?;
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
    let snapshot = apply_move(&state, &id, delta)?;
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
    let snapshot = apply_delete(&state, &gone)?;
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

/// Delete nodes and everything that points at them.
///
/// The command wrappers exist to fetch the `AppHandle` for the tray; the part
/// worth testing is here. Groups keep member ids, and a group that keeps a
/// deleted id would render an outbound pointing at nothing, so the node is
/// pruned from them rather than at render time. The remembered latency result
/// goes too: it would be a record nobody can see or clear.
pub fn apply_delete(state: &AppState, ids: &[NodeId]) -> AppResult<Vec<Node>> {
    if ids.is_empty() {
        return Err(AppError::InvalidInput("没有选中任何节点".into()));
    }

    let snapshot = {
        let mut nodes = state.nodes.write();
        let before = nodes.len();
        nodes.retain(|node| !ids.contains(&node.id));
        if nodes.len() == before {
            // One id gets the message the single-node button can show verbatim;
            // a selection just says none of them were there.
            return Err(if ids.len() == 1 {
                AppError::NotFound(format!("node {}", ids[0]))
            } else {
                AppError::NotFound("选中的节点都不存在".into())
            });
        }
        nodes.clone()
    };

    // Settings and nodes are separate locks, and this one is only taken once the
    // nodes lock is released, so the two can never dead-lock. A `last_node_id`
    // naming a node that no longer exists would make the app claim it is running
    // something it cannot even render.
    {
        let mut settings = state.settings.write();
        if let Some(last) = settings.last_node_id.clone() {
            if ids.iter().any(|id| *id == last) {
                settings.last_node_id = None;
            }
        }
    }

    {
        let mut groups = state.groups.write();
        for group in groups.iter_mut() {
            group.members.retain(|member| !ids.contains(member));
            if let Some(selected) = group.selected.as_deref() {
                if ids.iter().any(|id| id == selected) {
                    group.selected = None;
                }
            }
        }
    }

    state.drop_probes(ids);
    state.persist()?;
    Ok(snapshot)
}

/// Why a TCP handshake cannot measure this node, if it cannot.
///
/// A QUIC node speaks UDP: there is no TCP handshake to time, so the number
/// would be an error message pretending to be a measurement.
pub(crate) fn tcp_handshake_refusal(node: &Node) -> Option<AppError> {
    if !node.protocol.is_quic() {
        return None;
    }
    Some(AppError::InvalidInput(format!(
        "{} 走 QUIC，TCP 握手测不出延迟；启动内核后用首页的「真实延迟」",
        node.protocol.label()
    )))
}

/// TCP handshake latency to a node, in milliseconds.
///
/// Runs off the UI thread: a node that silently blackholes packets costs the
/// full timeout, and the window must keep painting meanwhile.
#[tauri::command]
#[specta::specta]
pub async fn test_node_latency(state: State<'_, AppState>, id: NodeId) -> AppResult<u32> {
    let node = state
        .nodes
        .read()
        .iter()
        .find(|node| node.id == id)
        .cloned()
        .ok_or_else(|| AppError::NotFound(format!("节点 {id} 不存在")))?;

    if let Some(refusal) = tcp_handshake_refusal(&node) {
        return Err(refusal);
    }

    let timeout = Duration::from_millis(u64::from(state.settings.read().test_timeout_ms));
    let target = node.id.clone();

    let outcome = tauri::async_runtime::spawn_blocking(move || {
        crate::net::latency::tcp_latency_with_timeout(&node.server, node.port, timeout)
    })
    .await
    .map_err(|error| AppError::Internal(format!("后台任务失败：{error}")))?;

    // Success and failure are both worth remembering: "测不到" is a fact the
    // user asked for, and dropping it would make a restart look as if nothing
    // had ever been tried.
    remember(state.inner(), &target, outcome.as_ref().ok().copied());
    outcome
}

/// One row of a batch latency run.
///
/// The UI needs both outcomes at once: a node that answered and a node that did
/// not are the same kind of fact, and rowing them through `Result` per node
/// would make the frontend throw away half of the answer to keep the other.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct LatencyProbe {
    pub id: NodeId,
    pub ms: Option<u32>,
    pub error: Option<String>,
}

fn probe_one(node: &Node, timeout: Duration) -> LatencyProbe {
    let id = node.id.clone();
    if let Some(refusal) = tcp_handshake_refusal(node) {
        return LatencyProbe { id, ms: None, error: Some(refusal.to_string()) };
    }
    match crate::net::latency::tcp_latency_with_timeout(&node.server, node.port, timeout) {
        Ok(ms) => LatencyProbe { id, ms: Some(ms), error: None },
        Err(error) => LatencyProbe { id, ms: None, error: Some(error.to_string()) },
    }
}

/// Probe every node with a bounded number of worker threads.
///
/// Results come back in the order the nodes were handed in, whatever order the
/// threads finished in: the caller renders them against the same ids it asked
/// about, and a shuffled answer would look like the cards rearranging themselves.
fn probe_all(nodes: &[Node], timeout_ms: u32, workers: usize) -> Vec<LatencyProbe> {
    let timeout = Duration::from_millis(u64::from(timeout_ms));
    let slots: Mutex<Vec<Option<LatencyProbe>>> = Mutex::new(vec![None; nodes.len()]);
    let next = AtomicUsize::new(0);
    let workers = workers.clamp(1, 16).min(nodes.len());

    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                if index >= nodes.len() {
                    break;
                }
                let probe = probe_one(&nodes[index], timeout);
                let mut slots = slots.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                slots[index] = Some(probe);
            });
        }
    });

    slots
        .into_inner()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .into_iter()
        .flatten()
        .collect()
}

/// Store one measured delay — or its absence — for a node.
///
/// A disk problem must not turn a finished measurement into a failed command:
/// the number is already correct in memory, so the write error is logged rather
/// than handed to a user who cannot act on it.
fn remember(state: &AppState, id: &NodeId, delay_ms: Option<u32>) {
    if let Err(error) = state.record_probe(id, delay_ms, ProbeKind::Tcp) {
        eprintln!("记不住测速结果：{error}");
    }
}

/// Store a whole batch with one write.
///
/// Rows refused by the TCP handshake (QUIC nodes) never measured anything, so
/// they are skipped: writing "测不到" over an older real number would destroy a
/// fact to record a refusal.
fn remember_batch(state: &AppState, probes: &[LatencyProbe]) {
    let records = {
        let nodes = state.nodes.read();
        probes
            .iter()
            .filter(|probe| {
                nodes
                    .iter()
                    .find(|node| node.id == probe.id)
                    .map(|node| tcp_handshake_refusal(node).is_none())
                    .unwrap_or(false)
            })
            .map(|probe| {
                (
                    probe.id.clone(),
                    NodeProbe::measured(probe.ms, ProbeKind::Tcp),
                )
            })
            .collect()
    };
    if let Err(error) = state.record_probes(records) {
        eprintln!("记不住测速结果：{error}");
    }
}

/// Every remembered latency result, keyed by node id.
#[tauri::command]
#[specta::specta]
pub fn node_probes(state: State<'_, AppState>) -> HashMap<NodeId, NodeProbe> {
    state.probes.read().clone()
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
    {
        let mut probes = state.probes.write();
        if ids.is_empty() {
            probes.clear();
        } else {
            for id in &ids {
                probes.remove(id);
            }
        }
    }
    state.persist()?;
    Ok(state.probes.read().clone())
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
    delay_offline(&state, &runtime, &id).await
}

// The body of `node_delay_offline`, free of `State` so the refusal paths are
// testable without spinning up a Tauri app.
pub(crate) async fn delay_offline(
    state: &AppState,
    runtime: &EngineRuntime,
    id: &str,
) -> AppResult<u32> {
    if runtime.status().running {
        return Err(AppError::InvalidInput(
            "内核正在运行：直接用「真实延迟」经它测，不必另起一个".to_string(),
        ));
    }
    // Everything below is a refusal too: an unknown id, a protocol the chosen
    // kernel cannot run, a probe URL that makes no sense. None of them measured
    // anything, so none of them may touch the stored result.
    let node = lookup_node(state, id)?;
    let settings = state.settings.read().clone();
    let kind = settings
        .last_engine
        .as_deref()
        .and_then(EngineKind::parse)
        .unwrap_or(EngineKind::Xray);
    // Refuse here rather than in the kernel: a QUIC node under Xray fails deep
    // inside a config the user never sees, with a message about a field name.
    kind.engine().validate_node(&node)?;
    let url = probe_url(state);
    let timeout = Duration::from_millis(u64::from(settings.test_timeout_ms));
    let engines_dir = runtime.engines_dir().to_path_buf();
    let config_dir = runtime.config_dir().to_path_buf();
    let target = node.id.clone();

    let outcome = run_blocking(move || {
        Ok(crate::engine::temp::attempt(
            &node,
            kind,
            &engines_dir,
            &config_dir,
            &url,
            timeout,
        ))
    })
    .await?;

    match outcome {
        Attempt::Measured(ms) => {
            remember_real(state, &target, Some(ms));
            Ok(ms)
        }
        // The kernel started and the request failed: the node is what did not
        // answer, and that belongs in the record exactly like a failed probe
        // through the running kernel.
        Attempt::Failed(error) => {
            remember_real(state, &target, None);
            Err(error)
        }
        // No measurement took place — nothing to remember.
        Attempt::NotStarted(error) => Err(error),
    }
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
    let (targets, timeout_ms, workers) = {
        let nodes = state.nodes.read();
        let settings = state.settings.read();
        let targets = if ids.is_empty() {
            nodes.clone()
        } else {
            let mut picked = Vec::with_capacity(ids.len());
            for id in &ids {
                let node = nodes
                    .iter()
                    .find(|node| &node.id == id)
                    .cloned()
                    .ok_or_else(|| AppError::NotFound(format!("节点 {id} 不存在")))?;
                picked.push(node);
            }
            picked
        };
        (targets, settings.test_timeout_ms, settings.test_concurrency as usize)
    };

    if targets.is_empty() {
        return Ok(Vec::new());
    }

    let probes = tauri::async_runtime::spawn_blocking(move || {
        probe_all(&targets, timeout_ms, workers)
    })
    .await
    .map_err(|error| AppError::Internal(format!("后台任务失败：{error}")))?;

    remember_batch(state.inner(), &probes);
    Ok(probes)
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
    let snapshot = apply_delete(&state, &ids)?;
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
    if ids.is_empty() {
        return Err(AppError::InvalidInput("没有选中任何节点".into()));
    }

    let nodes = state.nodes.read().clone();
    let mut lines = Vec::with_capacity(ids.len());
    for id in ids {
        let node = nodes
            .iter()
            .find(|node| node.id == id)
            .ok_or_else(|| AppError::NotFound(format!("节点 {id} 不存在")))?;
        lines.push(crate::export::link(node)?);
    }
    Ok(lines.join("
"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Protocol, Security, Transport};
    use crate::engine::runtime::tests::{harness, Harness};
    use crate::engine::{Endpoints, RunOptions};
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
                Endpoints::new(7890, 9090).unwrap(),
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
