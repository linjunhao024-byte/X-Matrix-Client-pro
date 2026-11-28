//! Node-list business: CRUD, tags, order, share links, latency and probes.
//!
//! This used to be the body of `commands/node.rs`. The Tauri commands are now
//! thin: they build nothing, hand the managed state in, and keep only the tray
//! refresh and the "push the blocking part onto a thread" wrapper. The split
//! exists so the headless core can answer the same calls over HTTP.
//!
//! Nothing here knows about Tauri. The one side effect the commands keep is
//! [`crate::refresh_tray`], because a tray belongs to the window process, not
//! to the business layer.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use crate::domain::{Node, NodeId, NodeProbe, ProbeKind};
use crate::engine::temp::Attempt;
use crate::engine::{EngineKind, EngineRuntime};
use crate::error::{AppError, AppResult};
use crate::services::engine::{lookup_node, probe_url, remember_real, stop_if_running_node};
use crate::services::EmptyArgs;
use crate::state::AppState;
use crate::sysproxy::ProxyGuard;

// ---------------------------------------------------------------------------
// HTTP-facing entry points. One per route; argument structs use the same wire
// names the Tauri command already takes, so the frontend sends one body shape
// to either transport.
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize)]
pub struct NodeArgs {
    pub node: Node,
}

#[derive(Debug, serde::Deserialize)]
pub struct TagArgs {
    pub ids: Vec<NodeId>,
    pub tag: String,
    pub remove: bool,
}

#[derive(Debug, serde::Deserialize)]
pub struct MoveArgs {
    pub id: NodeId,
    pub delta: i32,
}

#[derive(Debug, serde::Deserialize)]
pub struct IdArgs {
    pub id: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct IdsArgs {
    pub ids: Vec<NodeId>,
}

pub fn list_nodes(state: &AppState, _args: EmptyArgs) -> AppResult<Vec<Node>> {
    Ok(state.nodes.read().clone())
}

pub fn upsert_node(state: &AppState, args: NodeArgs) -> AppResult<Vec<Node>> {
    let mut node = args.node;
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
    Ok(snapshot)
}

/// Add or remove one tag across a selection.
///
/// Tagging a batch is the whole point of having tags next to a selection mode:
/// a subscription gives you 40 nodes and you want to mark the ten you trust.
pub fn tag_nodes(state: &AppState, args: TagArgs) -> AppResult<Vec<Node>> {
    apply_tag(state, &args.ids, &args.tag, args.remove)
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

pub fn move_node(state: &AppState, args: MoveArgs) -> AppResult<Vec<Node>> {
    apply_move(state, &args.id, args.delta)
}

pub fn delete_node(
    state: &AppState,
    runtime: &EngineRuntime,
    proxy: &ProxyGuard,
    args: IdArgs,
) -> AppResult<Vec<Node>> {
    let gone = vec![args.id];
    let snapshot = apply_delete(state, &gone)?;
    stop_if_running_node(runtime, proxy, &gone)?;
    Ok(snapshot)
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

pub fn delete_nodes(
    state: &AppState,
    runtime: &EngineRuntime,
    proxy: &ProxyGuard,
    args: IdsArgs,
) -> AppResult<Vec<Node>> {
    let snapshot = apply_delete(state, &args.ids)?;
    stop_if_running_node(runtime, proxy, &args.ids)?;
    Ok(snapshot)
}

/// Share links for the given nodes, one per line — what "copy" hands to the
/// clipboard. A node that cannot be expressed as a link fails the whole call
/// rather than producing a silently truncated list.
pub fn export_node_links(state: &AppState, args: IdsArgs) -> AppResult<String> {
    if args.ids.is_empty() {
        return Err(AppError::InvalidInput("没有选中任何节点".into()));
    }

    let nodes = state.nodes.read().clone();
    let mut lines = Vec::with_capacity(args.ids.len());
    for id in args.ids {
        let node = nodes
            .iter()
            .find(|node| node.id == id)
            .ok_or_else(|| AppError::NotFound(format!("节点 {id} 不存在")))?;
        lines.push(crate::export::link(node)?);
    }
    Ok(lines.join("\n"))
}

/// Why a TCP handshake cannot measure this node, if it cannot.
///
/// A QUIC node speaks UDP: there is no TCP handshake to time, so the number
/// would be an error message pretending to be a measurement.
pub fn tcp_handshake_refusal(node: &Node) -> Option<AppError> {
    if !node.protocol.is_quic() {
        return None;
    }
    Some(AppError::InvalidInput(format!(
        "{} 走 QUIC，TCP 握手测不出延迟；启动内核后用首页的「真实延迟」",
        node.protocol.label()
    )))
}

/// Resolve the node and the timeout a TCP handshake runs with.
///
/// Split from the measurement itself so the Tauri side can move the blocking
/// connect onto its own thread while the `State` borrow stays in the command.
pub fn latency_target(state: &AppState, id: &str) -> AppResult<(Node, Duration)> {
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
    Ok((node, timeout))
}

/// The blocking part: TCP handshake latency, in milliseconds.
pub fn latency_measure(node: &Node, timeout: Duration) -> AppResult<u32> {
    crate::net::latency::tcp_latency_with_timeout(&node.server, node.port, timeout)
}

/// TCP handshake latency to a node, in milliseconds.
///
/// Synchronous on purpose: the core runs it on its request thread, and the
/// Tauri command wraps [`latency_measure`] in `spawn_blocking`.
pub fn test_node_latency(state: &AppState, args: IdArgs) -> AppResult<u32> {
    let (node, timeout) = latency_target(state, &args.id)?;
    let target = node.id.clone();

    let outcome = latency_measure(&node, timeout);

    // Success and failure are both worth remembering: "测不到" is a fact the
    // user asked for, and dropping it would make a restart look as if nothing
    // had ever been tried.
    remember(state, &target, outcome.as_ref().ok().copied());
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
pub fn probe_all(nodes: &[Node], timeout_ms: u32, workers: usize) -> Vec<LatencyProbe> {
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
pub fn remember(state: &AppState, id: &NodeId, delay_ms: Option<u32>) {
    if let Err(error) = state.record_probe(id, delay_ms, ProbeKind::Tcp) {
        eprintln!("记不住测速结果：{error}");
    }
}

/// Store a whole batch with one write.
///
/// Rows refused by the TCP handshake (QUIC nodes) never measured anything, so
/// they are skipped: writing "测不到" over an older real number would destroy a
/// fact to record a refusal.
pub fn remember_batch(state: &AppState, probes: &[LatencyProbe]) {
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

/// Pick the nodes a batch run measures, with the stored timeout and concurrency.
///
/// An empty list means every node: that is what "test all" wants, and asking the
/// frontend to enumerate the same rows it is about to render only creates a
/// chance for the two lists to disagree.
pub fn probe_targets(state: &AppState, ids: &[NodeId]) -> AppResult<(Vec<Node>, u32, usize)> {
    let nodes = state.nodes.read();
    let settings = state.settings.read();
    let targets = if ids.is_empty() {
        nodes.clone()
    } else {
        let mut picked = Vec::with_capacity(ids.len());
        for id in ids {
            let node = nodes
                .iter()
                .find(|node| &node.id == id)
                .cloned()
                .ok_or_else(|| AppError::NotFound(format!("节点 {id} 不存在")))?;
            picked.push(node);
        }
        picked
    };
    Ok((
        targets,
        settings.test_timeout_ms,
        settings.test_concurrency as usize,
    ))
}

/// TCP handshake latency for a list of nodes, in milliseconds.
pub fn test_nodes_latency(state: &AppState, args: IdsArgs) -> AppResult<Vec<LatencyProbe>> {
    let (targets, timeout_ms, workers) = probe_targets(state, &args.ids)?;
    if targets.is_empty() {
        return Ok(Vec::new());
    }
    let probes = probe_all(&targets, timeout_ms, workers);
    remember_batch(state, &probes);
    Ok(probes)
}

/// Every remembered latency result, keyed by node id.
pub fn node_probes(state: &AppState, _args: EmptyArgs) -> AppResult<HashMap<NodeId, NodeProbe>> {
    Ok(state.probes.read().clone())
}

/// Forget remembered results for the given ids; an empty list clears them all.
///
/// Clearing is cheap and honest: the next sweep fills the map again, and a user
/// who moved to a different network does not want yesterday's numbers.
pub fn clear_node_probes(
    state: &AppState,
    args: IdsArgs,
) -> AppResult<HashMap<NodeId, NodeProbe>> {
    {
        let mut probes = state.probes.write();
        if args.ids.is_empty() {
            probes.clear();
        } else {
            for id in &args.ids {
                probes.remove(id);
            }
        }
    }
    state.persist()?;
    Ok(state.probes.read().clone())
}

// ---------------------------------------------------------------------------
// Real delay, measured through a throwaway kernel.
// ---------------------------------------------------------------------------

/// Everything one offline attempt needs, resolved while the state borrow lasts.
///
/// The attempt itself takes owned data only: it runs on a blocking thread (in
/// Tauri) or the request thread (in the core), and neither may hold a borrow of
/// the app state.
pub struct OfflineDelayRequest {
    pub node: Node,
    pub kind: EngineKind,
    pub engines_dir: std::path::PathBuf,
    pub config_dir: std::path::PathBuf,
    pub url: String,
    pub timeout: Duration,
}

/// The running kernel is the better instrument — it already holds the node — so
/// this refuses while one runs instead of competing with it for ports and nodes.
pub fn delay_offline_target(
    state: &AppState,
    runtime: &EngineRuntime,
    id: &str,
) -> AppResult<OfflineDelayRequest> {
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
    Ok(OfflineDelayRequest {
        engines_dir: runtime.engines_dir().to_path_buf(),
        config_dir: runtime.config_dir().to_path_buf(),
        node,
        kind,
        url,
        timeout,
    })
}

/// The blocking part: start a throwaway kernel, send one request, kill it.
pub(crate) fn delay_offline_probe(request: &OfflineDelayRequest) -> Attempt {
    crate::engine::temp::attempt(
        &request.node,
        request.kind,
        &request.engines_dir,
        &request.config_dir,
        &request.url,
        request.timeout,
    )
}

/// Turn an attempt into either a number or the right stored state.
pub(crate) fn delay_offline_finish(
    state: &AppState,
    node_id: &str,
    outcome: Attempt,
) -> AppResult<u32> {
    match outcome {
        Attempt::Measured(ms) => {
            remember_real(state, node_id, Some(ms));
            Ok(ms)
        }
        // The kernel started and the request failed: the node is what did not
        // answer, and that belongs in the record exactly like a failed probe
        // through the running kernel.
        Attempt::Failed(error) => {
            remember_real(state, node_id, None);
            Err(error)
        }
        // No measurement took place — nothing to remember.
        Attempt::NotStarted(error) => Err(error),
    }
}

/// Real delay of one node, measured through a throwaway kernel.
pub fn node_delay_offline(
    state: &AppState,
    runtime: &EngineRuntime,
    args: IdArgs,
) -> AppResult<u32> {
    let request = delay_offline_target(state, runtime, &args.id)?;
    let node_id = request.node.id.clone();
    let outcome = delay_offline_probe(&request);
    delay_offline_finish(state, &node_id, outcome)
}
