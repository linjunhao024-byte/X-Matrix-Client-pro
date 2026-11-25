//! Engine commands — the only door between the UI and a kernel process.
//!
//! Everything that can block (spawning, the readiness probe, an HTTP sample)
//! is pushed onto a blocking thread, so the webview keeps painting while a
//! kernel boots. The UI never learns about ports, config files or pids beyond
//! what [`EngineStatus`] reports.

use std::sync::Arc;

use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::domain::{Node, ProbeKind};
use crate::engine::stats::{self, TrafficSample};
use crate::engine::{TrafficLog, TrafficPoint};
use crate::net::latency;
use crate::engine::{
    Endpoints, EngineBinary, EngineKind, EngineRuntime, EngineStatus, RenderInput, RunOptions,
    StatsChannel,
};
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::sysproxy::ProxyGuard;

pub(crate) type Runtime<'a> = State<'a, Arc<EngineRuntime>>;
pub(crate) type Proxy<'a> = State<'a, Arc<ProxyGuard>>;
type Traffic<'a> = State<'a, Arc<TrafficLog>>;

/// Wall clock in unix seconds. The minute buckets are the user's minutes, so
/// they come from the same clock the UI formats.
fn now_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

/// Resolve a node by id, or fail with a message the UI can show verbatim.
pub(crate) fn lookup_node(state: &AppState, node_id: &str) -> AppResult<Node> {
    state
        .nodes
        .read()
        .iter()
        .find(|node| node.id == node_id)
        .cloned()
        .ok_or_else(|| AppError::NotFound(format!("节点 {node_id} 不存在")))
}

/// Ports come from the user's settings, validated before anything is spawned.
///
/// The `0 = follow mixed_port` fallback lives in [`Endpoints::new`], not here:
/// rendering, takeover and the port preflight then all read the same resolved
/// number instead of each redoing the fallback.
fn endpoints_from(state: &AppState) -> AppResult<Endpoints> {
    let settings = state.settings.read().clone();
    Endpoints::new(
        settings.mixed_port,
        settings.http_port,
        settings.socks_port,
        settings.api_port,
    )
}

pub(crate) async fn run_blocking<T, F>(job: F) -> AppResult<T>
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

// Opens the native file picker for one kernel and returns the chosen path.
//
// Written as `//` on purpose: specta copies `///` doc comments into the
// generated bindings, and `check:contract` then reports the hand-off text as a
// drift. The signature is the contract; the explanation lives here.
//
// `None` means the user cancelled, which is not a failure — the settings page
// keeps whatever path was already there. A dialog that cannot even be shown
// panics inside the blocking call, and `spawn_blocking` turns that into a
// `JoinError` we can hand back as a readable message instead of losing the app.
#[tauri::command]
#[specta::specta]
pub async fn pick_engine_binary(app: AppHandle, kind: EngineKind) -> AppResult<Option<String>> {
    let chosen = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("可执行文件", &["exe"])
            .blocking_pick_file()
    })
    .await
    .map_err(|error| {
        AppError::NotFound(format!(
            "打不开 {} 内核的选择文件对话框：{error}",
            kind.as_str()
        ))
    })?;

    let Some(path) = chosen else {
        return Ok(None);
    };
    let path = path.into_path().map_err(|error| {
        AppError::InvalidInput(format!("所选路径不可用：{error}"))
    })?;
    Ok(Some(path.display().to_string()))
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
        // The HTTP inbound: that is what an HTTP probe (and the system proxy)
        // talks to. It equals `mixed_port` unless the two protocols are split.
        status.http_port
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
    /// The active profile's YAML overlay. Read here rather than inside
    /// `start_engine` so every entry point (the window, the tray, reconnect on
    /// launch) starts from the same document, and an overlay that fails to
    /// merge stops the launch before anything is spawned.
    pub merge: String,
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
        merge: state.merge.read().clone(),
    })
}

/// Why a TUN start needs something this process does not have, if it does.
///
/// Pulled out of [`start_engine`] so the answer can be tested without a UAC
/// token: the machine that runs the tests is usually already elevated, and a
/// test that only ever sees `true` proves nothing about the refusal.
fn tun_permission_note(elevated: bool, has_service: bool) -> Option<&'static str> {
    if elevated || has_service {
        None
    } else {
        Some(
            "开 TUN 需要管理员权限：先在「权限」里点「以管理员身份重启」，或者装上内核服务让它以 SYSTEM 启动。",
        )
    }
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
    // TUN has to be refused here, before a config is rendered or a process is
    // spawned. The kernel cannot make any of these three true once it is up:
    // a config with a TUN inbound and a system proxy set routes every request
    // back through loopback, Xray has no TUN inbound at all, and an
    // unelevated TUN start dies inside the kernel with only its own log to
    // explain why. Every start path — commands, tray, auto-connect — comes
    // through this function, so one gate covers all of them.
    if request.settings.tun_enabled {
        if request.settings.system_proxy {
            return Err(AppError::InvalidInput(
                "TUN 和系统代理不能同时开：TUN 已经在网卡层接管，系统代理只会把流量再绕回本机一遍。关掉其中一个再启动。"
                    .into(),
            ));
        }
        if request.kind != EngineKind::SingBox {
            return Err(AppError::InvalidInput(
                "Xray 内核没有 TUN 入站：TUN 只对 sing-box 做了支持。切到 sing-box，或者关掉 TUN。"
                    .into(),
            ));
        }
        if let Some(note) =
            tun_permission_note(crate::elevate::is_elevated(), runtime.has_service())
        {
            return Err(AppError::InvalidInput(note.into()));
        }
    }
    // Only check when we are idle: while our own kernel runs, it *is* the
    // thing holding the port, and preflighting then would refuse every node
    // switch. Checking from idle is the case that matters anyway — the user
    // just closed another client, or forgot to.
    if !runtime.status().running {
        // A split gives the kernel two inbound ports, so the preflight has to
        // cover both — and each refusal names the port it actually refused,
        // instead of telling the user about a "mixed port" that no longer
        // exists.
        if request.endpoints.split() {
            crate::net::ports::ensure_free(request.endpoints.http_port, "HTTP 端口")?;
            crate::net::ports::ensure_free(request.endpoints.socks_port, "SOCKS 端口")?;
        } else {
            crate::net::ports::ensure_free(request.endpoints.mixed_port, "混合端口")?;
        }
        crate::net::ports::ensure_free(request.endpoints.api_port, "控制端口")?;
    }
    let options = RunOptions::from_settings(&request.settings, &request.rules)
        .with_catalog(&request.nodes, &request.groups)
        .with_rule_sets(&request.rule_sets)
        .with_merge(request.merge);
    let status = runtime.start(request.kind, &request.node, request.endpoints, options)?;
    if request.settings.system_proxy {
        let bypass = request.settings.proxy_bypass.as_str();
        let applied = if request.settings.pac_enabled {
            // The script encodes the port itself, so it is rendered from the
            // endpoint that is actually listening — not from the saved setting.
            // With a split that endpoint is the HTTP one: a PAC script hands
            // the browser `PROXY host:port`, which it resolves over HTTP.
            let script = crate::sysproxy::pac::render(
                request.endpoints.proxy_port(),
                bypass,
                &crate::sysproxy::pac::direct_patterns(&request.rules),
            );
            proxy.enable_pac(request.endpoints.proxy_port(), bypass, &script)
        } else {
            proxy.enable_with_bypass("127.0.0.1", request.endpoints.proxy_port(), bypass)
        };
        if let Err(error) = applied {
            let _ = runtime.stop();
            return Err(error);
        }
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

/// The swap [`engine_apply`] performs, callable outside a command.
///
/// `State` wrappers only exist inside a command; the hotkey path starts from an
/// `AppHandle` and the managed `Arc`s, so the body lives here once instead of
/// being copied. Behaviour is unchanged: same prepare, same stop-then-start
/// when something is already running, same session remember and tray refresh.
pub(crate) async fn apply_engine(
    app: &AppHandle,
    kind: EngineKind,
    node_id: &str,
    state: &AppState,
    runtime: Arc<EngineRuntime>,
    proxy: Arc<ProxyGuard>,
) -> AppResult<EngineStatus> {
    let request = prepare_start(state, kind, node_id)?;
    let was_running = runtime.status().running;
    let status = run_blocking(move || {
        if was_running {
            runtime.stop()?;
        }
        start_engine(&runtime, &proxy, request)
    })
    .await?;
    remember_session(state, kind, node_id);
    crate::refresh_tray(app);
    Ok(status)
}

/// Swap what is running without making the user stop and start it: same ports,
/// same proxy takeover, a kernel replaced underneath.
///
/// The system proxy stays enabled across the swap on purpose. Handing it back
/// first would send traffic direct for the gap — a leak, and a longer one
/// (registry rewrite) than the second a kernel needs to bind a port that never
/// went away.
#[tauri::command]
#[specta::specta]
pub async fn engine_apply(
    app: AppHandle,
    kind: EngineKind,
    node_id: String,
    state: State<'_, AppState>,
    runtime: Runtime<'_>,
    proxy: Proxy<'_>,
) -> AppResult<EngineStatus> {
    let runtime = Arc::clone(runtime.inner());
    let proxy = Arc::clone(proxy.inner());
    apply_engine(&app, kind, &node_id, &state, runtime, proxy).await
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

/// Stop the kernel if it is running one of these nodes, and hand the system
/// proxy back first so the machine is never left pointing at a dead port.
///
/// Called when a node the kernel was started with is about to stop existing —
/// deleted by the user, or dropped by a subscription refresh that replaced the
/// batch it came from. Left alone, the kernel would keep proxying through a node
/// the UI can neither render nor switch to.
///
/// Returns whether a kernel was stopped: `false` means nothing was running, or
/// the running node is not among `gone`, and the proxy was therefore not touched.
pub(crate) fn stop_if_running_node(
    runtime: &EngineRuntime,
    proxy: &ProxyGuard,
    gone: &[String],
) -> AppResult<bool> {
    let Some(running_id) = runtime.status().node_id else {
        return Ok(false);
    };
    if !gone.iter().any(|id| id == &running_id) {
        return Ok(false);
    }
    // Order matters, as in `engine_stop`: hand the proxy back before the port
    // it points at disappears.
    proxy.disable()?;
    runtime.stop()?;
    Ok(true)
}

/// One statistics sample. `None` means "nothing is running", which the UI shows
/// as an empty state rather than an error.
#[tauri::command]
#[specta::specta]
pub async fn traffic_sample(
    runtime: Runtime<'_>,
    log: Traffic<'_>,
) -> AppResult<Option<TrafficSample>> {
    let runtime = Arc::clone(runtime.inner());
    let log = Arc::clone(log.inner());
    run_blocking(move || {
        let Some(channel) = runtime.stats_channel() else {
            return Ok(None);
        };
        let sample = match channel {
            StatsChannel::XrayExpVar { port } => stats::read_xray(port)?,
            StatsChannel::ClashApi { port } => stats::read_clash(port)?,
        };
        // Every live sample is also history in the making. Doing it here means
        // the record is exactly as complete as the chart the user is watching —
        // there is no second timer that could miss minutes.
        log.observe(sample.up_total, sample.down_total, now_seconds());
        Ok(Some(sample))
    })
    .await
}

/// Per-minute history, oldest first. `minutes <= 0` returns everything kept
/// (seven days at most).
#[tauri::command]
#[specta::specta]
pub fn traffic_history(log: Traffic<'_>, minutes: f64) -> Vec<TrafficPoint> {
    log.points(minutes as i64, now_seconds())
}

#[tauri::command]
#[specta::specta]
pub fn traffic_clear_history(log: Traffic<'_>) -> AppResult<()> {
    log.clear()
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
        .with_rule_sets(&rule_sets)
        // The preview claims to show what the kernel would get, so it must
        // carry the overlay the start path carries.
        .with_merge(state.merge.read().clone());

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
    // The HTTP inbound, not the configured mixed port: with a split the mixed
    // port has no listener and the probe would fail on a healthy kernel.
    let port = status.http_port;
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

/// Drop several connections at once; an empty list means all of them.
///
/// Same two preconditions as `close_connection`, phrased for the plural case:
/// "close everything" on Xray is not a missing feature of the button, it is a
/// kernel that has no connection table to clear.
#[tauri::command]
#[specta::specta]
pub fn close_connections(runtime: Runtime<'_>, ids: Vec<String>) -> AppResult<()> {
    let status = runtime.status();
    if !status.running {
        return Err(AppError::Engine("内核没在运行，没有连接可断开".to_string()));
    }
    if status.kind != Some(EngineKind::SingBox) {
        return Err(AppError::Engine(
            "Xray 不提供连接列表，断不开连接".to_string(),
        ));
    }
    stats::close_connections(status.api_port, &ids)
}

/// What came back from one real-delay attempt, with the distinction that
/// matters once results are remembered.
///
/// A refusal (no kernel running, or a kernel without a Clash API) says nothing
/// about the node's latency — writing it down as "测不到" would replace a real
/// measurement with a fact about the app's own state. A failure does say the
/// node answered nothing, so it belongs in the record.
enum DelayAttempt {
    Measured(u32),
    Failed(AppError),
    Refused(AppError),
}

/// How long the kernel takes to reach the probe through one outbound.
///
/// This goes through the Clash API instead of the mixed port, which is what
/// makes a *group* measurable: the kernel dials through the group's own tag,
/// so the number describes the path that group would take, not the path the
/// running config happens to use. Only sing-box exposes the endpoint.
fn probe_delay(runtime: &EngineRuntime, tag: &str, url: &str) -> DelayAttempt {
    let status = runtime.status();
    if !status.running {
        return DelayAttempt::Refused(AppError::Engine(
            "内核没在运行，测不到真实延迟；先启动内核".to_string(),
        ));
    }
    if status.kind != Some(EngineKind::SingBox) {
        return DelayAttempt::Refused(AppError::Engine(
            "Xray 没有 Clash API，分组延迟只能在 sing-box 内核下测".to_string(),
        ));
    }
    match stats::clash_delay(status.api_port, tag, url, DELAY_BUDGET_MS) {
        Ok(ms) => DelayAttempt::Measured(ms),
        Err(error) => DelayAttempt::Failed(error),
    }
}

/// The same attempt, for callers that only care whether it worked.
fn delay_of(runtime: &EngineRuntime, tag: &str, url: &str) -> AppResult<u32> {
    match probe_delay(runtime, tag, url) {
        DelayAttempt::Measured(ms) => Ok(ms),
        DelayAttempt::Failed(error) | DelayAttempt::Refused(error) => Err(error),
    }
}

/// The probe URL, or the built-in one when the setting is blank.
pub(crate) fn probe_url(state: &AppState) -> String {
    let settings = state.settings.read();
    if settings.test_url.trim().is_empty() {
        latency::PROBE_URL.to_string()
    } else {
        settings.test_url.clone()
    }
}

/// Kernel-side delay budget. Long enough for a slow overseas hop, short enough
/// that a dead node answers instead of hanging the button forever.
const DELAY_BUDGET_MS: u64 = 5000;

/// Real delay of one strategy group, measured by the running sing-box.
#[tauri::command]
#[specta::specta]
pub async fn group_delay(
    state: State<'_, AppState>,
    runtime: Runtime<'_>,
    id: String,
) -> AppResult<u32> {
    let group = state
        .groups
        .read()
        .iter()
        .find(|group| group.id == id)
        .cloned()
        .ok_or_else(|| AppError::NotFound(format!("策略组 {id} 不存在")))?;
    if group.members.is_empty() {
        return Err(AppError::InvalidInput(format!(
            "策略组「{}」没有成员，没什么可测的",
            group.name
        )));
    }
    let tag = group.tag();
    let url = probe_url(&state);
    let runtime = Arc::clone(runtime.inner());
    run_blocking(move || delay_of(&runtime, &tag, &url)).await
}

/// Real delay of one node, measured by the running sing-box.
///
/// A node only has a tag when the running config emits it: the active node is
/// `proxy`, a group member is `node:<id>`. Anything else has no outbound to
/// dial through, and saying so is better than measuring someone else's path.
#[tauri::command]
#[specta::specta]
pub async fn node_delay(
    state: State<'_, AppState>,
    runtime: Runtime<'_>,
    id: String,
) -> AppResult<u32> {
    let node = lookup_node(&state, &id)?;
    let active = state.settings.read().last_node_id.clone();
    let in_group = state
        .groups
        .read()
        .iter()
        .any(|group| group.enabled && group.members.iter().any(|member| member == &node.id));
    let tag = if active.as_deref() == Some(node.id.as_str()) {
        "proxy".to_string()
    } else if in_group {
        format!("node:{}", node.id)
    } else {
        return Err(AppError::InvalidInput(format!(
            "「{}」既不是当前节点，也不在任何启用的策略组里，内核里没有它的出站可测；先用首页把它设为当前节点",
            node.name
        )));
    };
    let url = probe_url(&state);
    let runtime = Arc::clone(runtime.inner());
    let node_id = node.id.clone();
    let outcome = run_blocking(move || Ok(probe_delay(&runtime, &tag, &url))).await?;

    let delay_ms = match outcome {
        DelayAttempt::Measured(ms) => Some(ms),
        // The attempt happened and produced nothing: that is exactly the
        // "测不到" state, worth keeping until the next attempt replaces it.
        DelayAttempt::Failed(error) => {
            remember_real(&state, &node_id, None);
            return Err(error);
        }
        // Not a measurement at all, so the stored number stays as it was.
        DelayAttempt::Refused(error) => return Err(error),
    };
    remember_real(&state, &node_id, delay_ms);
    Ok(delay_ms.unwrap_or_default())
}

/// Store a real-delay result without letting a disk problem hide the number the
/// measurement already produced.
pub(crate) fn remember_real(state: &AppState, id: &str, delay_ms: Option<u32>) {
    if let Err(error) = state.record_probe(&id.to_string(), delay_ms, ProbeKind::Real) {
        eprintln!("记不住真实延迟：{error}");
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use parking_lot::Mutex;

    use super::{start_engine, stop_if_running_node, tun_permission_note, StartRequest};
    use crate::engine::runtime::tests::{harness, node, Harness};
    use crate::engine::{Endpoints, EngineKind, RunOptions};
    use crate::error::AppResult;
    use crate::sysproxy::{ProxyGuard, ProxyState, SystemProxy};

    /// A backend that only remembers what it was told. `ProxyGuard` runs its
    /// whole snapshot/restore dance against it, so the test sees "handed back"
    /// without the real system proxy ever being read or written.
    pub(crate) struct RecordingProxy {
        pub(crate) state: Mutex<ProxyState>,
        pub(crate) writes: Mutex<Vec<ProxyState>>,
    }

    impl SystemProxy for Arc<RecordingProxy> {
        fn read(&self) -> AppResult<ProxyState> {
            Ok(self.state.lock().clone())
        }

        fn write(&self, state: &ProxyState) -> AppResult<()> {
            *self.state.lock() = state.clone();
            self.writes.lock().push(state.clone());
            Ok(())
        }
    }

    /// A snapshot path inside the crate's target dir; the system temp dir is not
    /// writable from every shell on this machine.
    fn scratch(name: &str) -> PathBuf {
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join("engine-commands");
        std::fs::create_dir_all(&base).unwrap();
        base.join(format!("{name}-{}-{seq}.json", std::process::id()))
    }

    pub(crate) fn proxy() -> (ProxyGuard, Arc<RecordingProxy>, PathBuf) {
        let fake = Arc::new(RecordingProxy {
            state: Mutex::new(ProxyState::off()),
            writes: Mutex::new(Vec::new()),
        });
        let path = scratch("snapshot");
        let guard = ProxyGuard::new(Box::new(fake.clone()), &path, scratch("pac").with_extension("pac"));
        (guard, fake, path)
    }

    pub(crate) fn start_kernel(h: &Harness) {
        h.runtime
            .start(
                EngineKind::Xray,
                &node(),
                Endpoints::new(7890, 0, 0, 9090).unwrap(),
                RunOptions::new(),
            )
            .unwrap();
    }

    #[test]
    fn a_deleted_running_node_stops_the_kernel_and_hands_the_proxy_back() {
        let h = harness(true, false);
        let (guard, fake, path) = proxy();
        start_kernel(&h);
        guard.enable_with_bypass("127.0.0.1", 7890, "").unwrap();
        assert!(guard.is_active());

        let stopped = stop_if_running_node(&h.runtime, &guard, &["n1".to_string()]).unwrap();

        assert!(stopped);
        // No kernel is left running a node that no longer exists ...
        assert!(!h.runtime.status().running);
        // ... and the machine is not left pointing at the port it was using.
        assert!(!guard.is_active());
        assert!(!fake.state.lock().enabled);
        assert_eq!(fake.writes.lock().len(), 2, "enable, then restore");

        std::fs::remove_dir_all(&h.dir).ok();
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn deleting_some_other_node_leaves_the_kernel_and_the_proxy_alone() {
        let h = harness(true, false);
        let (guard, fake, path) = proxy();
        start_kernel(&h);
        guard.enable_with_bypass("127.0.0.1", 7890, "").unwrap();

        let stopped = stop_if_running_node(&h.runtime, &guard, &["n2".to_string()]).unwrap();

        assert!(!stopped);
        assert!(h.runtime.status().running);
        assert!(guard.is_active());
        assert_eq!(fake.writes.lock().len(), 1, "only the enable");

        std::fs::remove_dir_all(&h.dir).ok();
        let _ = std::fs::remove_file(&path);
    }

    /// A port the OS says is free right now.
    ///
    /// `start_engine` preflights by really binding, so a hard-coded 7891 would
    /// make these tests flaky on a machine where something else listens there.
    fn free_port() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    }

    fn split_request(http_port: u16, socks_port: u16, pac: bool) -> StartRequest {
        let mut settings = crate::domain::Settings::default();
        settings.system_proxy = true;
        settings.pac_enabled = pac;
        StartRequest {
            kind: EngineKind::Xray,
            node: node(),
            endpoints: Endpoints::new(7890, http_port, socks_port, free_port()).unwrap(),
            settings,
            rules: vec![],
            nodes: vec![node()],
            groups: vec![],
            rule_sets: vec![],
            merge: String::new(),
        }
    }

    #[test]
    fn a_split_takeover_points_the_system_proxy_at_the_http_port() {
        let h = harness(true, false);
        let (guard, fake, path) = proxy();
        // start_engine takes the guard by Arc: the app state owns one too.
        let guard = Arc::new(guard);
        let http_port = free_port();
        let socks_port = free_port();

        let status = start_engine(&h.runtime, &guard, split_request(http_port, socks_port, false)).unwrap();

        // The status still reports what the user configured ...
        assert_eq!(status.mixed_port, 7890);
        // ... and the effective ports the kernel really bound.
        assert_eq!(status.http_port, http_port);
        assert_eq!(status.socks_port, socks_port);

        let writes = fake.writes.lock();
        assert_eq!(writes.len(), 1, "one takeover write");
        assert!(writes[0].enabled);
        // The system proxy speaks HTTP, so it must be pointed at the HTTP
        // inbound, never at the mixed port that no longer listens.
        assert_eq!(writes[0].server, format!("127.0.0.1:{http_port}"));
        assert!(!writes[0].server.contains(&socks_port.to_string()));

        std::fs::remove_dir_all(&h.dir).ok();
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_split_pac_takeover_encodes_the_http_port() {
        let h = harness(true, false);
        let fake = Arc::new(RecordingProxy {
            state: Mutex::new(ProxyState::off()),
            writes: Mutex::new(Vec::new()),
        });
        let snapshot = scratch("snapshot-pac");
        let pac_path = scratch("pac").with_extension("pac");
        let guard = Arc::new(ProxyGuard::new(Box::new(fake.clone()), &snapshot, pac_path.clone()));
        let http_port = free_port();
        let socks_port = free_port();

        let status = start_engine(&h.runtime, &guard, split_request(http_port, socks_port, true)).unwrap();
        assert_eq!(status.http_port, http_port);

        // `enable_pac` already refuses a script whose PROXY directive names a
        // different port, so reaching the file at all proves agreement; read it
        // to pin *which* port the pair agreed on.
        let script = std::fs::read_to_string(&pac_path).unwrap();
        assert!(
            script.contains(&format!("PROXY 127.0.0.1:{http_port}; DIRECT")),
            "PAC script must forward into the HTTP inbound: {script}"
        );
        assert!(!script.contains(&socks_port.to_string()));

        let writes = fake.writes.lock();
        assert_eq!(writes.len(), 1, "one takeover write");
        assert!(!writes[0].server.contains(':'), "PAC mode uses AutoConfigURL");

        std::fs::remove_dir_all(&h.dir).ok();
        let _ = std::fs::remove_file(&snapshot);
        let _ = std::fs::remove_file(&pac_path);
    }

    #[test]
    fn a_stop_with_nothing_running_touches_nothing() {
        let h = harness(true, false);
        let (guard, fake, path) = proxy();

        let stopped = stop_if_running_node(&h.runtime, &guard, &["n1".to_string()]).unwrap();

        assert!(!stopped);
        // The proxy was never ours, so a delete has no business writing it.
        assert!(fake.writes.lock().is_empty());
        assert!(!guard.is_active());

        std::fs::remove_dir_all(&h.dir).ok();
        let _ = std::fs::remove_file(&path);
    }

    /// A start request that asks for TUN, on the given kernel, with the system
    /// proxy on or off. The ports never get bound: every refusal below happens
    /// before the preflight, which is the point.
    fn tun_request(kind: EngineKind, system_proxy: bool) -> StartRequest {
        let mut settings = crate::domain::Settings::default();
        settings.tun_enabled = true;
        settings.system_proxy = system_proxy;
        StartRequest {
            kind,
            node: node(),
            endpoints: Endpoints::new(7890, 0, 0, 9090).unwrap(),
            settings,
            rules: vec![],
            nodes: vec![node()],
            groups: vec![],
            rule_sets: vec![],
            merge: String::new(),
        }
    }

    #[test]
    fn tun_and_system_proxy_are_rejected_together() {
        let h = harness(true, false);
        let (guard, fake, path) = proxy();
        let guard = Arc::new(guard);

        let message = start_engine(&h.runtime, &guard, tun_request(EngineKind::SingBox, true))
            .unwrap_err()
            .to_string();

        assert!(message.contains("不能同时开"), "message: {message}");
        // Refused before the kernel ran and before the registry was touched:
        // the whole point of checking here is that nothing half-starts.
        assert!(!h.runtime.status().running);
        assert!(fake.writes.lock().is_empty());

        std::fs::remove_dir_all(&h.dir).ok();
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn tun_on_xray_is_rejected() {
        let h = harness(true, false);
        let (guard, fake, path) = proxy();
        let guard = Arc::new(guard);

        let message = start_engine(&h.runtime, &guard, tun_request(EngineKind::Xray, false))
            .unwrap_err()
            .to_string();

        assert!(message.contains("只对 sing-box"), "message: {message}");
        assert!(!h.runtime.status().running);
        assert!(fake.writes.lock().is_empty());

        std::fs::remove_dir_all(&h.dir).ok();
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn tun_needs_either_an_elevated_process_or_the_service() {
        // Elevated, or the service (which runs as SYSTEM): both are enough.
        assert!(tun_permission_note(true, false).is_none());
        assert!(tun_permission_note(false, true).is_none());

        // Neither: the refusal has to name both ways out, because the user
        // cannot tell from the symptom (a kernel that exits) what to do.
        let note = tun_permission_note(false, false).expect("unelevated, serviceless start must be refused");
        assert!(note.contains("管理员权限"), "note: {note}");
        assert!(note.contains("以管理员身份重启"), "note: {note}");
        assert!(note.contains("SYSTEM"), "note: {note}");
    }
}
