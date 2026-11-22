//! Engine commands — the only door between the UI and a kernel process.
//!
//! Everything that can block (spawning, the readiness probe, an HTTP sample)
//! is pushed onto a blocking thread, so the webview keeps painting while a
//! kernel boots. The UI never learns about ports, config files or pids beyond
//! what [`EngineStatus`] reports.

use std::sync::Arc;

use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::domain::Node;
use crate::engine::stats::{self, TrafficSample};
use crate::net::latency;
use crate::engine::{
    Endpoints, EngineBinary, EngineKind, EngineRuntime, EngineStatus, RenderInput, RunOptions,
    StatsChannel,
};
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::sysproxy::ProxyGuard;

type Runtime<'a> = State<'a, Arc<EngineRuntime>>;
type Proxy<'a> = State<'a, Arc<ProxyGuard>>;

/// Resolve a node by id, or fail with a message the UI can show verbatim.
fn lookup_node(state: &AppState, node_id: &str) -> AppResult<Node> {
    state
        .nodes
        .read()
        .iter()
        .find(|node| node.id == node_id)
        .cloned()
        .ok_or_else(|| AppError::NotFound(format!("节点 {node_id} 不存在")))
}

/// Ports come from the user's settings, validated before anything is spawned.
fn endpoints_from(state: &AppState) -> AppResult<Endpoints> {
    let settings = state.settings.read().clone();
    Endpoints::new(settings.mixed_port, settings.api_port)
}

async fn run_blocking<T, F>(job: F) -> AppResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> AppResult<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(job)
        .await
        .map_err(|error| AppError::Internal(format!("后台任务失败：{error}")))?
}

/// Current runtime state. Cheap and non-blocking: safe to poll from the UI.
#[tauri::command]
#[specta::specta]
pub fn engine_status(runtime: Runtime<'_>) -> EngineStatus {
    runtime.status()
}

/// Which kernels this build knows how to drive.
#[tauri::command]
#[specta::specta]
pub fn engine_kinds() -> Vec<EngineKind> {
    EngineKind::all().to_vec()
}

/// Where each kernel binary is expected, and whether it is actually there.
#[tauri::command]
#[specta::specta]
pub fn engine_binaries(runtime: Runtime<'_>) -> Vec<EngineBinary> {
    runtime.binaries()
}

/// What version an installed kernel reports.
///
/// Runs `version` (no config, no ports, no traffic), so it is safe to call on
/// an idle machine and cheap enough for the home screen. The version is what
/// tells a user whether the binary they dropped in `engines/` is the one the
/// config format expects.
#[tauri::command]
#[specta::specta]
pub fn engine_version(runtime: Runtime<'_>, kind: EngineKind) -> AppResult<String> {
    // Deliberately synchronous: `version` is a ~30 ms self-report, and going
    // through `run_blocking` would move the `Runtime` borrow into a `'static`
    // closure it cannot escape.
    runtime.version(kind)
}

/// Download throughput through the running tunnel, in bytes per second.
///
/// Same probe address as the real-delay button, so the two numbers describe the
/// same path: one how long a round trip takes, the other how much it carries.
#[tauri::command]
#[specta::specta]
pub async fn proxy_speed(state: State<'_, AppState>, runtime: Runtime<'_>) -> AppResult<f64> {
    let port = {
        let status = runtime.status();
        if !status.running {
            return Err(AppError::Engine(
                "内核没在运行，测不到速度；先启动内核".to_string(),
            ));
        }
        status.mixed_port
    };
    let url = {
        let settings = state.settings.read();
        if settings.test_url.trim().is_empty() {
            latency::PROBE_URL.to_string()
        } else {
            settings.test_url.clone()
        }
    };
    let bytes_per_second = run_blocking(move || latency::download_speed_via_proxy(port, &url)).await?;
    Ok(bytes_per_second as f64)
}

/// The tail of a kernel log. Empty string means "that kernel never ran".
#[tauri::command]
#[specta::specta]
pub fn engine_log_tail(runtime: Runtime<'_>, kind: EngineKind, lines: u32) -> AppResult<String> {
    runtime.log_tail(kind, lines as usize)
}

/// One page of a kernel log, for the log page's poll loop.
///
/// Cheap on purpose: the caller passes back the `size` it last saw, so a quiet
/// kernel costs an empty string and a metadata call.
#[tauri::command]
#[specta::specta]
pub fn engine_log_delta(
    runtime: Runtime<'_>,
    kind: EngineKind,
    since: f64,
) -> AppResult<crate::engine::LogDelta> {
    // f64 on the boundary only: TypeScript has no u64, and specta refuses to
    // invent one. A cursor is a byte count, well inside f64's exact range.
    runtime.log_delta(kind, since.max(0.0) as u64)
}

/// Truncate a kernel's log.
#[tauri::command]
#[specta::specta]
pub fn engine_log_clear(runtime: Runtime<'_>, kind: EngineKind) -> AppResult<()> {
    runtime.log_clear(kind)
}

/// Open the engines folder in Explorer, so the user can drop a kernel in.
#[tauri::command]
#[specta::specta]
pub fn engine_open_dir(app: AppHandle, runtime: Runtime<'_>) -> AppResult<()> {
    let dir = runtime.engines_dir().display().to_string();
    app.opener()
        .open_path(dir, None::<&str>)
        .map_err(|error| AppError::Internal(error.to_string()))
}

/// Start `node_id` on `kind`. Stops whatever was running first.
///
/// If the user asked for the system proxy, it is pointed at the kernel *after*
/// the readiness probe passes — never before, or every request during boot
/// would hit a port with no listener. A failed proxy takeover tears the kernel
/// back down rather than leaving the UI claiming something that is not true.
/// Everything the start sequence needs, read off the shared state.
///
/// Pulled out of the command so the same sequence can run without a webview
/// asking for it — that is how "connect on launch" works, and it keeps exactly
/// one code path that decides the config, the proxy order and the rollback.
pub struct StartRequest {
    pub kind: EngineKind,
    pub node: Node,
    pub endpoints: Endpoints,
    pub settings: crate::domain::Settings,
    pub rules: Vec<crate::domain::RoutingRule>,
    pub nodes: Vec<Node>,
    pub groups: Vec<crate::domain::ProxyGroup>,
    pub rule_sets: Vec<crate::engine::LoadedRuleSet>,
}

/// Read the shared state into a [`StartRequest`]. Cheap: read locks only.
pub fn prepare_start(
    state: &AppState,
    kind: EngineKind,
    node_id: &str,
) -> AppResult<StartRequest> {
    Ok(StartRequest {
        kind,
        node: lookup_node(state, node_id)?,
        endpoints: endpoints_from(state)?,
        settings: state.settings.read().clone(),
        rules: state.rules.read().clone(),
        nodes: state.nodes.read().clone(),
        groups: state.groups.read().clone(),
        rule_sets: crate::commands::ruleset::loaded_sets(state),
    })
}

/// Spawn the kernel, then point the system proxy at it.
///
/// The proxy is taken over *after* the readiness probe passes — never before,
/// or every request during boot would hit a port with no listener. A failed
/// takeover tears the kernel back down rather than leaving the UI claiming
/// something that is not true.
pub fn start_engine(
    runtime: &Arc<EngineRuntime>,
    proxy: &Arc<ProxyGuard>,
    request: StartRequest,
) -> AppResult<EngineStatus> {
    // Only check when we are idle: while our own kernel runs, it *is* the
    // thing holding the port, and preflighting then would refuse every node
    // switch. Checking from idle is the case that matters anyway — the user
    // just closed another client, or forgot to.
    if !runtime.status().running {
        crate::net::ports::ensure_free(request.endpoints.mixed_port, "混合端口")?;
        crate::net::ports::ensure_free(request.endpoints.api_port, "控制端口")?;
    }
    let options = RunOptions::from_settings(&request.settings, &request.rules)
        .with_catalog(&request.nodes, &request.groups)
        .with_rule_sets(&request.rule_sets);
    let status = runtime.start(request.kind, &request.node, request.endpoints, options)?;
    if request.settings.system_proxy
        && let Err(error) =
            proxy.enable_with_bypass("127.0.0.1", request.endpoints.mixed_port, &request.settings.proxy_bypass)
    {
        let _ = runtime.stop();
        return Err(error);
    }
    Ok(status)
}

/// Remember what is running, so a later launch can offer to reconnect and the
/// UI can preselect the same node.
pub fn remember_session(state: &AppState, kind: EngineKind, node_id: &str) {
    {
        let mut settings = state.settings.write();
        settings.last_node_id = Some(node_id.to_string());
        settings.last_engine = Some(kind.as_str().to_string());
    }
    if let Err(error) = state.persist() {
        eprintln!("x-matrix: 记住上次使用的节点失败：{error}");
    }
}

/// Start `node_id` on `kind`. Stops whatever was running first.
#[tauri::command]
#[specta::specta]
pub async fn engine_start(
    app: AppHandle,
    kind: EngineKind,
    node_id: String,
    state: State<'_, AppState>,
    runtime: Runtime<'_>,
    proxy: Proxy<'_>,
) -> AppResult<EngineStatus> {
    let request = prepare_start(&state, kind, &node_id)?;
    let runtime = Arc::clone(runtime.inner());
    let proxy = Arc::clone(proxy.inner());
    let status = run_blocking(move || start_engine(&runtime, &proxy, request)).await?;
    remember_session(&state, kind, &node_id);
    // The tray's toggle says what the next click does, so it has to follow.
    crate::refresh_tray(&app);
    Ok(status)
}

/// Stop the kernel, giving the system proxy back first.
#[tauri::command]
#[specta::specta]
pub async fn engine_stop(
    app: AppHandle,
    runtime: Runtime<'_>,
    proxy: Proxy<'_>,
) -> AppResult<EngineStatus> {
    let runtime = Arc::clone(runtime.inner());
    let proxy = Arc::clone(proxy.inner());
    let status = run_blocking(move || {
        // Order matters: hand the proxy back before the port disappears.
        proxy.disable()?;
        runtime.stop()?;
        Ok(runtime.status())
    })
    .await?;
    crate::refresh_tray(&app);
    Ok(status)
}

/// One statistics sample. `None` means "nothing is running", which the UI shows
/// as an empty state rather than an error.
#[tauri::command]
#[specta::specta]
pub async fn traffic_sample(runtime: Runtime<'_>) -> AppResult<Option<TrafficSample>> {
    let runtime = Arc::clone(runtime.inner());
    run_blocking(move || {
        let Some(channel) = runtime.stats_channel() else {
            return Ok(None);
        };
        let sample = match channel {
            StatsChannel::XrayExpVar { port } => stats::read_xray(port)?,
            StatsChannel::ClashApi { port } => stats::read_clash(port)?,
        };
        Ok(Some(sample))
    })
    .await
}

/// The exact document that *would* be handed to the kernel.
///
/// Rendering is pure, so this is safe to call with nothing installed and
/// nothing running — it is how the UI can explain a node without spawning it.
#[tauri::command]
#[specta::specta]
pub fn engine_config_preview(
    kind: EngineKind,
    node_id: String,
    state: State<'_, AppState>,
    runtime: Runtime<'_>,
) -> AppResult<String> {
    render_preview(kind, &node_id, &state, &runtime)
}

/// The same document for the node the app would start next.
///
/// The diagnostics report needs the config the user is *actually* about to run,
/// which is whatever the engine selection and last node say — not a node id the
/// caller had to invent.
pub(crate) fn render_for_preview(
    state: &AppState,
    runtime: &EngineRuntime,
) -> AppResult<String> {
    let (kind, node_id) = {
        let settings = state.settings.read();
        let kind = settings
            .last_engine
            .as_deref()
            .and_then(EngineKind::parse)
            .unwrap_or(EngineKind::Xray);
        let node_id = settings
            .last_node_id
            .clone()
            .or_else(|| state.nodes.read().first().map(|node| node.id.clone()));
        (kind, node_id)
    };
    let node_id = node_id.ok_or_else(|| AppError::InvalidInput("还没有任何节点".into()))?;
    render_preview(kind, &node_id, state, runtime)
}

fn render_preview(
    kind: EngineKind,
    node_id: &str,
    state: &AppState,
    runtime: &EngineRuntime,
) -> AppResult<String> {
    let node = lookup_node(state, node_id)?;
    let endpoints = endpoints_from(state)?;
    let engine = kind.engine();
    engine.validate_node(&node)?;
    let rules = state.rules.read().clone();
    let settings = state.settings.read().clone();
    let nodes = state.nodes.read().clone();
    let groups = state.groups.read().clone();
    let rule_sets = crate::commands::ruleset::loaded_sets(state);
    let options = RunOptions::from_settings(&settings, &rules)
        .with_catalog(&nodes, &groups)
        .with_rule_sets(&rule_sets);

    let mut input = RenderInput::new(&node, endpoints).with_options(options);
    // Show the same document the kernel would get, including the rule-set paths
    // sing-box will be pointed at.
    let rule_set_dir = (kind == EngineKind::SingBox).then(|| runtime.rule_set_dir());
    if let Some(dir) = rule_set_dir.as_deref() {
        input = input.with_rule_set_dir(dir);
    }
    let config = engine.render_config(&input)?;
    serde_json::to_string_pretty(&config).map_err(|error| AppError::Internal(error.to_string()))
}

/// Real delay: one HTTP request through the running engine's mixed port.
///
/// This is the only latency number that includes the tunnel, so it only exists
/// while a kernel is running. The cheaper TCP handshake in `test_node_latency`
/// stays available for the pre-start case.
#[tauri::command]
#[specta::specta]
pub async fn proxy_latency(
    state: State<'_, AppState>,
    runtime: Runtime<'_>,
) -> AppResult<u32> {
    let status = runtime.status();
    if !status.running {
        return Err(AppError::Engine(
            "内核没在运行，测不到真实延迟；先启动内核".to_string(),
        ));
    }
    let port = status.mixed_port;
    // The probe URL is a setting because gstatic is blocked on some networks —
    // a latency button that always fails is worse than no button.
    let url = {
        let settings = state.settings.read();
        if settings.test_url.trim().is_empty() {
            latency::PROBE_URL.to_string()
        } else {
            settings.test_url.clone()
        }
    };
    tauri::async_runtime::spawn_blocking(move || latency::http_latency_via_proxy(port, &url))
    .await
    .map_err(|error| AppError::Internal(format!("测速任务没有跑完：{error}")))?
}

/// Drop one live connection (sing-box only — Xray has no connection table).
#[tauri::command]
#[specta::specta]
pub fn close_connection(runtime: Runtime<'_>, id: String) -> AppResult<()> {
    let status = runtime.status();
    if !status.running {
        return Err(AppError::Engine("内核没在运行，没有连接可断开".to_string()));
    }
    if status.kind != Some(EngineKind::SingBox) {
        return Err(AppError::Engine(
            "Xray 不提供连接列表，断不开单个连接".to_string(),
        ));
    }
    stats::close_connection(status.api_port, &id)
}
