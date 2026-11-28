//! Engine business logic, transport-free.
//!
//! This used to be the body of `commands/engine.rs`. The Tauri commands are now
//! thin: they build a [`Ctx`] from their managed state and call one function
//! here. The split exists so the headless core can run the same start sequence
//! (`POST /engine/engine_start`) without a webview.
//!
//! Nothing in this file knows about Tauri. That is not decoration: the core
//! binary is the same crate with a different entry point, and a stray
//! `tauri::AppHandle` here would make the HTTP channel impossible to compile.
//!
//! Functions that block (spawning, the readiness probe, an HTTP sample) are
//! synchronous on purpose. The Tauri side keeps its "push it onto a blocking
//! thread" wrappers around these same calls; the core runs them on its own
//! request thread.

use std::sync::Arc;

use crate::domain::{Node, ProbeKind};
use crate::engine::stats::{self, TrafficSample};
use crate::engine::{
    Endpoints, EngineBinary, EngineKind, EngineRuntime, EngineStatus, LoadedRuleSet, LogDelta,
    RenderInput, RunOptions, StatsChannel, TrafficLog, TrafficPoint,
};
use crate::error::{AppError, AppResult};
use crate::net::latency;
use crate::services::{Ctx, EmptyArgs};
use crate::state::AppState;
use crate::sysproxy::ProxyGuard;

/// Wall clock in unix seconds. The minute buckets are the user's minutes, so
/// they come from the same clock the UI formats.
pub fn now_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0)
}

/// Resolve a node by id, or fail with a message the UI can show verbatim.
pub fn lookup_node(state: &AppState, node_id: &str) -> AppResult<Node> {
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
pub fn endpoints_from(state: &AppState) -> AppResult<Endpoints> {
    let settings = state.settings.read().clone();
    Endpoints::new(
        settings.mixed_port,
        settings.http_port,
        settings.socks_port,
        settings.api_port,
    )
}

// ---------------------------------------------------------------------------
// HTTP-facing entry points. One per route; argument structs are camelCase so
// the wire body matches the Tauri argument names the frontend already sends.
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize)]
pub struct KindArgs {
    pub kind: EngineKind,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartArgs {
    pub kind: EngineKind,
    pub node_id: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogTailArgs {
    pub kind: EngineKind,
    pub lines: u32,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogDeltaArgs {
    pub kind: EngineKind,
    pub since: f64,
}

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewArgs {
    pub kind: EngineKind,
    pub node_id: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct IdArgs {
    pub id: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct IdsArgs {
    pub ids: Vec<String>,
}

#[derive(Debug, serde::Deserialize)]
pub struct MinutesArgs {
    pub minutes: f64,
}

/// Current runtime state. Cheap and non-blocking: safe to poll from the UI.
pub fn engine_status(ctx: &Ctx<'_>, _args: EmptyArgs) -> AppResult<EngineStatus> {
    Ok(ctx.runtime.status())
}

/// Which kernels this build knows how to drive.
///
/// Takes no [`Ctx`]: the answer is a compile-time list, and a caller with no
/// runtime (the command signature has none either) can still ask.
pub fn engine_kinds(_args: EmptyArgs) -> AppResult<Vec<EngineKind>> {
    Ok(EngineKind::all().to_vec())
}

/// Where each kernel binary is expected, and whether it is actually there.
pub fn engine_binaries(ctx: &Ctx<'_>, _args: EmptyArgs) -> AppResult<Vec<EngineBinary>> {
    Ok(ctx.runtime.binaries())
}

/// What version an installed kernel reports.
///
/// Runs `version` (no config, no ports, no traffic), so it is safe to call on
/// an idle machine and cheap enough for the home screen.
pub fn engine_version(ctx: &Ctx<'_>, args: KindArgs) -> AppResult<String> {
    ctx.runtime.version(args.kind)
}

/// The tail of a kernel log. Empty string means "that kernel never ran".
pub fn engine_log_tail(ctx: &Ctx<'_>, args: LogTailArgs) -> AppResult<String> {
    ctx.runtime.log_tail(args.kind, args.lines as usize)
}

/// One page of a kernel log, for the log page's poll loop.
pub fn engine_log_delta(ctx: &Ctx<'_>, args: LogDeltaArgs) -> AppResult<LogDelta> {
    // f64 on the boundary only: TypeScript has no u64, and specta refuses to
    // invent one. A cursor is a byte count, well inside f64's exact range.
    ctx.runtime.log_delta(args.kind, args.since.max(0.0) as u64)
}

/// Truncate a kernel's log.
pub fn engine_log_clear(ctx: &Ctx<'_>, args: KindArgs) -> AppResult<()> {
    ctx.runtime.log_clear(args.kind)
}

/// The exact document that *would* be handed to the kernel.
pub fn engine_config_preview(ctx: &Ctx<'_>, args: PreviewArgs) -> AppResult<String> {
    render_preview(args.kind, &args.node_id, ctx.state()?, ctx.runtime)
}

/// Port + probe URL for a real-delay measurement.
///
/// Split from [`proxy_latency_now`] so an async caller can do the cheap state
/// reads inline and push only the network call onto a blocking thread.
pub fn proxy_latency_target(ctx: &Ctx<'_>) -> AppResult<(u16, String)> {
    let status = ctx.runtime.status();
    if !status.running {
        return Err(AppError::Engine(
            "内核没在运行，测不到真实延迟；先启动内核".to_string(),
        ));
    }
    // The HTTP inbound, not the configured mixed port: with a split the mixed
    // port has no listener and the probe would fail on a healthy kernel.
    Ok((status.http_port, probe_url(ctx.state()?)))
}

/// Real delay: one HTTP request through the running engine's HTTP inbound.
pub fn proxy_latency_now(port: u16, url: &str) -> AppResult<u32> {
    latency::http_latency_via_proxy(port, url)
}

pub fn proxy_latency(ctx: &Ctx<'_>, _args: EmptyArgs) -> AppResult<u32> {
    let (port, url) = proxy_latency_target(ctx)?;
    proxy_latency_now(port, &url)
}

/// Port + probe URL for a throughput measurement.
pub fn proxy_speed_target(ctx: &Ctx<'_>) -> AppResult<(u16, String)> {
    let status = ctx.runtime.status();
    if !status.running {
        return Err(AppError::Engine(
            "内核没在运行，测不到速度；先启动内核".to_string(),
        ));
    }
    Ok((status.http_port, probe_url(ctx.state()?)))
}

/// Bytes per second through the running tunnel.
pub fn proxy_speed_now(port: u16, url: &str) -> AppResult<f64> {
    Ok(latency::download_speed_via_proxy(port, url)? as f64)
}

pub fn proxy_speed(ctx: &Ctx<'_>, _args: EmptyArgs) -> AppResult<f64> {
    let (port, url) = proxy_speed_target(ctx)?;
    proxy_speed_now(port, &url)
}

/// The tag and probe URL a group's delay is measured with.
pub fn group_delay_target(ctx: &Ctx<'_>, id: &str) -> AppResult<(String, String)> {
    let state = ctx.state()?;
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
    Ok((group.tag(), probe_url(state)))
}

/// Real delay of one strategy group, measured by the running sing-box.
pub fn group_delay_now(runtime: &EngineRuntime, tag: &str, url: &str) -> AppResult<u32> {
    delay_of(runtime, tag, url)
}

pub fn group_delay(ctx: &Ctx<'_>, args: IdArgs) -> AppResult<u32> {
    let (tag, url) = group_delay_target(ctx, &args.id)?;
    group_delay_now(ctx.runtime, &tag, &url)
}

/// Node id, outbound tag and probe URL a node's delay is measured with.
pub fn node_delay_target(ctx: &Ctx<'_>, id: &str) -> AppResult<(String, String, String)> {
    let state = ctx.state()?;
    let node = lookup_node(state, id)?;
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
    let url = probe_url(state);
    Ok((tag, url, node.id.clone()))
}

/// One real-delay attempt against the running kernel.
pub fn node_delay_probe(runtime: &EngineRuntime, tag: &str, url: &str) -> DelayAttempt {
    probe_delay(runtime, tag, url)
}

/// Turn an attempt into either a number or the right stored state.
pub fn node_delay_finish(
    state: &AppState,
    node_id: &str,
    outcome: DelayAttempt,
) -> AppResult<u32> {
    let delay_ms = match outcome {
        DelayAttempt::Measured(ms) => Some(ms),
        // The attempt happened and produced nothing: that is exactly the
        // "测不到" state, worth keeping until the next attempt replaces it.
        DelayAttempt::Failed(error) => {
            remember_real(state, node_id, None);
            return Err(error);
        }
        // Not a measurement at all, so the stored number stays as it was.
        DelayAttempt::Refused(error) => return Err(error),
    };
    remember_real(state, node_id, delay_ms);
    Ok(delay_ms.unwrap_or_default())
}

/// Real delay of one node, measured by the running sing-box.
pub fn node_delay(ctx: &Ctx<'_>, args: IdArgs) -> AppResult<u32> {
    let (tag, url, node_id) = node_delay_target(ctx, &args.id)?;
    let outcome = node_delay_probe(ctx.runtime, &tag, &url);
    node_delay_finish(ctx.state()?, &node_id, outcome)
}

/// Drop one live connection (sing-box only — Xray has no connection table).
pub fn close_connection(ctx: &Ctx<'_>, args: IdArgs) -> AppResult<()> {
    let status = ctx.runtime.status();
    if !status.running {
        return Err(AppError::Engine("内核没在运行，没有连接可断开".to_string()));
    }
    if status.kind != Some(EngineKind::SingBox) {
        return Err(AppError::Engine(
            "Xray 不提供连接列表，断不开单个连接".to_string(),
        ));
    }
    stats::close_connection(status.api_port, &args.id)
}

/// Drop several connections at once; an empty list means all of them.
pub fn close_connections(ctx: &Ctx<'_>, args: IdsArgs) -> AppResult<()> {
    let status = ctx.runtime.status();
    if !status.running {
        return Err(AppError::Engine("内核没在运行，没有连接可断开".to_string()));
    }
    if status.kind != Some(EngineKind::SingBox) {
        return Err(AppError::Engine(
            "Xray 不提供连接列表，断不开连接".to_string(),
        ));
    }
    stats::close_connections(status.api_port, &args.ids)
}

/// One statistics sample. `None` means "nothing is running".
pub fn traffic_sample_now(
    runtime: &EngineRuntime,
    log: &TrafficLog,
) -> AppResult<Option<TrafficSample>> {
    let Some(channel) = runtime.stats_channel() else {
        return Ok(None);
    };
    let sample = match channel {
        StatsChannel::XrayExpVar { port } => stats::read_xray(port)?,
        StatsChannel::ClashApi { port } => stats::read_clash(port)?,
    };
    // Every live sample is also history in the making. Doing it here means the
    // record is exactly as complete as the chart the user is watching.
    log.observe(sample.up_total, sample.down_total, now_seconds());
    Ok(Some(sample))
}

/// Per-minute history, oldest first. `minutes <= 0` returns everything kept.
pub fn traffic_history(log: &TrafficLog, args: MinutesArgs) -> AppResult<Vec<TrafficPoint>> {
    Ok(log.points(args.minutes as i64, now_seconds()))
}

pub fn traffic_clear_history(log: &TrafficLog, _args: EmptyArgs) -> AppResult<()> {
    log.clear()
}

/// Start `node_id` on `kind`, remembering the session afterwards.
pub fn engine_start(ctx: &Ctx<'_>, args: StartArgs) -> AppResult<EngineStatus> {
    let state = ctx.state()?;
    let request = prepare_start(state, args.kind, &args.node_id)?;
    let status = start_engine(ctx.runtime, ctx.proxy()?, request)?;
    remember_session(state, args.kind, &args.node_id);
    Ok(status)
}

/// Swap what is running without making the user stop and start it.
pub fn engine_apply(ctx: &Ctx<'_>, args: StartArgs) -> AppResult<EngineStatus> {
    apply_swap(
        args.kind,
        &args.node_id,
        ctx.state()?,
        Arc::clone(ctx.runtime),
        Arc::clone(ctx.proxy()?),
    )
}

/// Stop the kernel, giving the system proxy back first.
pub fn engine_stop(ctx: &Ctx<'_>, _args: EmptyArgs) -> AppResult<EngineStatus> {
    engine_stop_now(ctx.runtime, ctx.proxy()?)
}

pub fn engine_stop_now(
    runtime: &Arc<EngineRuntime>,
    proxy: &Arc<ProxyGuard>,
) -> AppResult<EngineStatus> {
    // Order matters: hand the proxy back before the port disappears.
    proxy.disable()?;
    runtime.stop()?;
    Ok(runtime.status())
}

// ---------------------------------------------------------------------------
// Start / swap sequence. Shared by the window, the tray, auto-connect and the
// core's HTTP channel — there must be exactly one place that decides the
// config, the proxy order and the rollback.
// ---------------------------------------------------------------------------

/// Everything the start sequence needs, read off the shared state.
pub struct StartRequest {
    pub kind: EngineKind,
    pub node: Node,
    pub endpoints: Endpoints,
    pub settings: crate::domain::Settings,
    pub rules: Vec<crate::domain::RoutingRule>,
    pub nodes: Vec<Node>,
    pub groups: Vec<crate::domain::ProxyGroup>,
    pub rule_sets: Vec<LoadedRuleSet>,
    /// The active profile's YAML overlay. Read here rather than inside
    /// `start_engine` so every entry point starts from the same document.
    pub merge: String,
}

/// Read the shared state into a [`StartRequest`]. Cheap: read locks only.
pub fn prepare_start(state: &AppState, kind: EngineKind, node_id: &str) -> AppResult<StartRequest> {
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
pub(crate) fn tun_permission_note(elevated: bool, has_service: bool) -> Option<&'static str> {
    if elevated || has_service {
        None
    } else {
        Some(
            "开 TUN 需要管理员权限：先在「权限」里点「以管理员身份重启」，或者装上内核服务让它以 SYSTEM 启动。",
        )
    }
}

/// Spawn the kernel, then point the system proxy at it.
pub fn start_engine(
    runtime: &Arc<EngineRuntime>,
    proxy: &Arc<ProxyGuard>,
    request: StartRequest,
) -> AppResult<EngineStatus> {
    // TUN has to be refused here, before a config is rendered or a process is
    // spawned. Every start path comes through this function, so one gate
    // covers all of them.
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
    // switch.
    if !runtime.status().running {
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

/// Stop the current kernel (if any) and start the requested one.
///
/// Behaviour is unchanged from the old `apply_engine`: same prepare, same
/// stop-then-start when something is already running, same session remember.
/// The caller refreshes the tray; that is a Tauri side effect.
pub fn apply_swap(
    kind: EngineKind,
    node_id: &str,
    state: &AppState,
    runtime: Arc<EngineRuntime>,
    proxy: Arc<ProxyGuard>,
) -> AppResult<EngineStatus> {
    let request = prepare_start(state, kind, node_id)?;
    let was_running = runtime.status().running;
    if was_running {
        runtime.stop()?;
    }
    let status = start_engine(&runtime, &proxy, request)?;
    remember_session(state, kind, node_id);
    Ok(status)
}

/// Stop the kernel if it is running one of these nodes, handing the proxy back
/// first so the machine is never left pointing at a dead port.
///
/// Returns whether a kernel was stopped: `false` means nothing was running, or
/// the running node is not among `gone`, and the proxy was therefore not touched.
pub fn stop_if_running_node(
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

/// The same document for the node the app would start next.
///
/// The diagnostics report needs the config the user is *actually* about to run,
/// which is whatever the engine selection and last node say.
pub fn render_for_preview(state: &AppState, runtime: &EngineRuntime) -> AppResult<String> {
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

pub fn render_preview(
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

/// What came back from one real-delay attempt, with the distinction that
/// matters once results are remembered.
///
/// A refusal (no kernel running, or a kernel without a Clash API) says nothing
/// about the node's latency — writing it down as "测不到" would replace a real
/// measurement with a fact about the app's own state. A failure does say the
/// node answered nothing, so it belongs in the record.
pub enum DelayAttempt {
    Measured(u32),
    Failed(AppError),
    Refused(AppError),
}

/// How long the kernel takes to reach the probe through one outbound.
///
/// This goes through the Clash API instead of the mixed port, which is what
/// makes a *group* measurable: the kernel dials through the group's own tag.
/// Only sing-box exposes the endpoint.
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
pub fn probe_url(state: &AppState) -> String {
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

/// Store a real-delay result without letting a disk problem hide the number the
/// measurement already produced.
pub fn remember_real(state: &AppState, id: &str, delay_ms: Option<u32>) {
    if let Err(error) = state.record_probe(&id.to_string(), delay_ms, ProbeKind::Real) {
        eprintln!("记不住真实延迟：{error}");
    }
}