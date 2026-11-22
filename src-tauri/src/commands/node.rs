use tauri::{AppHandle, State};

use crate::domain::{Node, NodeId};
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
pub fn delete_node(
    app: AppHandle,
    state: State<'_, AppState>,
    id: NodeId,
) -> AppResult<Vec<Node>> {
    let snapshot = {
        let mut nodes = state.nodes.write();
        let before = nodes.len();
        nodes.retain(|n| n.id != id);
        if nodes.len() == before {
            return Err(AppError::NotFound(format!("node {id}")));
        }
        nodes.clone()
    };

    // A group that keeps a deleted member id would render an outbound that
    // points at nothing, so the node is pruned here rather than at render time.
    {
        let mut groups = state.groups.write();
        for group in groups.iter_mut() {
            group.members.retain(|member| member != &id);
            if group.selected.as_deref() == Some(id.as_str()) {
                group.selected = None;
            }
        }
    }

    state.persist()?;
    crate::refresh_tray(&app);
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

    tauri::async_runtime::spawn_blocking(move || {
        crate::net::latency::tcp_latency(&node.server, node.port)
    })
    .await
    .map_err(|error| AppError::Internal(format!("后台任务失败：{error}")))?
}

/// Delete several nodes in one call.
///
/// The UI offers a selection, so one round trip per node would be both slow and
/// half-applied if the window closed in the middle. Returns the remaining list,
/// like every other mutation here.
#[tauri::command]
#[specta::specta]
pub fn delete_nodes(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<NodeId>,
) -> AppResult<Vec<Node>> {
    if ids.is_empty() {
        return Err(AppError::InvalidInput("没有选中任何节点".into()));
    }

    let snapshot = {
        let mut nodes = state.nodes.write();
        let before = nodes.len();
        nodes.retain(|node| !ids.contains(&node.id));
        if nodes.len() == before {
            return Err(AppError::NotFound("选中的节点都不存在".into()));
        }
        nodes.clone()
    };

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

    state.persist()?;
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
