//! The routing table: the six P0 routes, plus one `POST /<module>/<command>`
//! route per command whose body now lives in [`crate::services`].
//!
//! Every handler is a plain function over [`CoreContext`], so the routing table
//! and the error envelope are testable without binding a socket. The server
//! thread only parses HTTP, checks the token and hands the request here.
//!
//! Two commands are deliberately *not* here: `pick_engine_binary` (a native
//! file dialog) and `engine_open_dir` (it opens Explorer). Both are shell
//! actions on the machine that owns the window; a headless core has no window
//! to show and no business popping one. They are named in [`NOT_ROUTED`].
//!
//! Error envelope (shared with the whole channel):
//!
//! ```json
//! {"error":{"code":"not_found","message":"节点 x 不存在"}}
//! ```
//!
//! The code is machine-readable (one per [`AppError`] variant, plus
//! `unauthorized` for the token gate), the message is the existing Chinese
//! wording from [`AppError`] verbatim — the UI shows it as-is, so inventing a
//! second vocabulary here would only make two texts for one failure.

use serde::Serialize;
use serde_json::{json, Value};

use crate::core::{http, CoreContext};
use crate::domain::node::Protocol;
use crate::domain::Node;
use crate::engine::{EngineKind, EngineStatus};
use crate::error::{AppError, AppResult};
use crate::services::engine as svc_engine;
use crate::services::sysproxy as svc_sysproxy;
use crate::services::{group as svc_group, import as svc_import, node as svc_node};
use crate::services::process as svc_process;
use crate::services::{Ctx, EmptyArgs, NOOP_NOTIFIER};

/// Check the token, then route. Authentication comes first on purpose: an
/// unknown path without a token is still a 401, because a caller that has not
/// proven it belongs here must not learn which paths exist.
pub fn dispatch(ctx: &CoreContext, token: &str, request: &http::Request) -> http::Response {
    if !token_matches(request.header("x-core-token"), token) {
        return failure(401, "unauthorized", "X-Core-Token 缺失或不匹配");
    }

    let route = ROUTES.iter().find(|route| {
        route.method == request.method.as_str() && route.path == request.path.as_str()
    });
    match route {
        Some(route) => (route.handler)(ctx, request),
        None => failure(
            404,
            "not_found",
            &format!("未知路由：{} {}", request.method, request.path),
        ),
    }
}

/// One row of the routing table.
pub struct Route {
    pub method: &'static str,
    pub path: &'static str,
    handler: Handler,
}

type Handler = fn(&CoreContext, &http::Request) -> http::Response;

/// Commands that intentionally have no route, with the reason they do not.
///
/// The coverage test treats these as covered, so the list cannot grow silently:
/// an entry only earns its place when the command is a shell action, not
/// business.
pub const NOT_ROUTED: &[(&str, &str)] = &[
    (
        "pick_engine_binary",
        "Tauri 原生文件选择对话框：属于窗口进程，headless core 没有窗口可弹",
    ),
    (
        "engine_open_dir",
        "在所属机器上打开 Explorer：属于窗口进程的壳动作，headless core 不该弹窗",
    ),
];

/// Everything the core answers, in one table so the coverage test has a single
/// source of truth. The six P0 routes stay exactly as they were.
pub const ROUTES: &[Route] = &[
    Route { method: "GET", path: "/health", handler: h_health },
    Route { method: "GET", path: "/status", handler: h_status },
    Route { method: "GET", path: "/nodes", handler: h_nodes },
    Route { method: "POST", path: "/engine/start", handler: h_legacy_engine_start },
    Route { method: "POST", path: "/engine/stop", handler: h_legacy_engine_stop },
    Route { method: "POST", path: "/shutdown", handler: h_shutdown },
    Route { method: "POST", path: "/engine/engine_status", handler: r_engine_status },
    Route { method: "POST", path: "/engine/engine_kinds", handler: r_engine_kinds },
    Route { method: "POST", path: "/engine/engine_binaries", handler: r_engine_binaries },
    Route { method: "POST", path: "/engine/engine_version", handler: r_engine_version },
    Route { method: "POST", path: "/engine/engine_log_tail", handler: r_engine_log_tail },
    Route { method: "POST", path: "/engine/engine_log_delta", handler: r_engine_log_delta },
    Route { method: "POST", path: "/engine/engine_log_clear", handler: r_engine_log_clear },
    Route { method: "POST", path: "/engine/proxy_latency", handler: r_proxy_latency },
    Route { method: "POST", path: "/engine/proxy_speed", handler: r_proxy_speed },
    Route { method: "POST", path: "/engine/group_delay", handler: r_group_delay },
    Route { method: "POST", path: "/engine/node_delay", handler: r_node_delay },
    Route { method: "POST", path: "/engine/close_connection", handler: r_close_connection },
    Route { method: "POST", path: "/engine/close_connections", handler: r_close_connections },
    Route { method: "POST", path: "/engine/engine_start", handler: r_engine_start },
    Route { method: "POST", path: "/engine/engine_apply", handler: r_engine_apply },
    Route { method: "POST", path: "/engine/engine_stop", handler: r_engine_stop },
    Route { method: "POST", path: "/engine/traffic_sample", handler: r_traffic_sample },
    Route { method: "POST", path: "/engine/traffic_history", handler: r_traffic_history },
    Route { method: "POST", path: "/engine/traffic_clear_history", handler: r_traffic_clear_history },
    Route { method: "POST", path: "/engine/engine_config_preview", handler: r_engine_config_preview },
    Route { method: "POST", path: "/sysproxy/system_proxy_status", handler: r_system_proxy_status },
    Route { method: "POST", path: "/sysproxy/system_proxy_disable", handler: r_system_proxy_disable },
    Route { method: "POST", path: "/node/list_nodes", handler: r_list_nodes },
    Route { method: "POST", path: "/node/upsert_node", handler: r_upsert_node },
    Route { method: "POST", path: "/node/delete_node", handler: r_delete_node },
    Route { method: "POST", path: "/node/move_node", handler: r_move_node },
    Route { method: "POST", path: "/node/delete_nodes", handler: r_delete_nodes },
    Route { method: "POST", path: "/node/tag_nodes", handler: r_tag_nodes },
    Route { method: "POST", path: "/node/export_node_links", handler: r_export_node_links },
    Route { method: "POST", path: "/node/test_node_latency", handler: r_test_node_latency },
    Route { method: "POST", path: "/node/test_nodes_latency", handler: r_test_nodes_latency },
    Route { method: "POST", path: "/node/node_probes", handler: r_node_probes },
    Route { method: "POST", path: "/node/clear_node_probes", handler: r_clear_node_probes },
    Route { method: "POST", path: "/node/node_delay_offline", handler: r_node_delay_offline },
    Route { method: "POST", path: "/group/list_groups", handler: r_list_groups },
    Route { method: "POST", path: "/group/upsert_group", handler: r_upsert_group },
    Route { method: "POST", path: "/group/delete_group", handler: r_delete_group },
    Route { method: "POST", path: "/group/select_group_member", handler: r_select_group_member },
    Route { method: "POST", path: "/group/toggle_group", handler: r_toggle_group },
    Route { method: "POST", path: "/process/list_processes", handler: r_list_processes },
    Route { method: "POST", path: "/import/import_preview", handler: r_import_preview },
    Route { method: "POST", path: "/import/import_nodes", handler: r_import_nodes },
];

/// The context a service function gets from the core.
fn command_ctx(ctx: &CoreContext) -> Ctx<'_> {
    Ctx {
        state: Some(&ctx.state),
        runtime: &ctx.runtime,
        proxy: Some(&ctx.proxy),
        notify: &NOOP_NOTIFIER,
    }
}

/// Parse `body` into the command's argument struct, call the service, and turn
/// either half into the shared envelope.
///
/// An empty body is `{}`, not an error: commands with no arguments are called
/// with `POST` and nothing to say. A body that will not deserialize is a 400 —
/// the caller sent the wrong shape, and retrying it unchanged will not help.
fn handle<A, T>(command: &str, body: &str, run: impl FnOnce(A) -> AppResult<T>) -> http::Response
where
    A: serde::de::DeserializeOwned,
    T: Serialize,
{
    let body = if body.trim().is_empty() { "{}" } else { body };
    let args: A = match serde_json::from_str(body) {
        Ok(args) => args,
        Err(error) => {
            return failure(
                400,
                "invalid_input",
                &format!("{command}: 参数不合法：{error}"),
            )
        }
    };
    match run(args) {
        Ok(value) => match serde_json::to_value(value) {
            Ok(value) => http::Response::json(200, value),
            Err(error) => failure(
                500,
                "internal",
                &format!("{command}: 结果序列化失败：{error}"),
            ),
        },
        Err(error) => from_error(&error),
    }
}

/// Wire one service function into the table. The argument type is spelled out
/// because it is also the wire contract: `camelCase` in the struct, `{}` for
/// no arguments.
macro_rules! ctx_route {
    ($handler:ident, $command:literal, $args:ty, $run:path) => {
        fn $handler(ctx: &CoreContext, request: &http::Request) -> http::Response {
            let context = command_ctx(ctx);
            handle($command, &request.body, |args: $args| $run(&context, args))
        }
    };
}

/// Wire a service function that only needs the state. Same shape as
/// [`ctx_route!`], minus the runtime and proxy the command never asked for.
macro_rules! state_route {
    ($handler:ident, $command:literal, $args:ty, $run:path) => {
        fn $handler(ctx: &CoreContext, request: &http::Request) -> http::Response {
            handle($command, &request.body, |args: $args| $run(&ctx.state, args))
        }
    };
}

ctx_route!(r_engine_status, "engine_status", EmptyArgs, svc_engine::engine_status);
ctx_route!(r_engine_binaries, "engine_binaries", EmptyArgs, svc_engine::engine_binaries);
ctx_route!(r_engine_version, "engine_version", svc_engine::KindArgs, svc_engine::engine_version);
ctx_route!(r_engine_log_tail, "engine_log_tail", svc_engine::LogTailArgs, svc_engine::engine_log_tail);
ctx_route!(r_engine_log_delta, "engine_log_delta", svc_engine::LogDeltaArgs, svc_engine::engine_log_delta);
ctx_route!(r_engine_log_clear, "engine_log_clear", svc_engine::KindArgs, svc_engine::engine_log_clear);
ctx_route!(r_proxy_latency, "proxy_latency", EmptyArgs, svc_engine::proxy_latency);
ctx_route!(r_proxy_speed, "proxy_speed", EmptyArgs, svc_engine::proxy_speed);
ctx_route!(r_group_delay, "group_delay", svc_engine::IdArgs, svc_engine::group_delay);
ctx_route!(r_node_delay, "node_delay", svc_engine::IdArgs, svc_engine::node_delay);
ctx_route!(r_close_connection, "close_connection", svc_engine::IdArgs, svc_engine::close_connection);
ctx_route!(r_close_connections, "close_connections", svc_engine::IdsArgs, svc_engine::close_connections);
ctx_route!(r_engine_start, "engine_start", svc_engine::StartArgs, svc_engine::engine_start);
ctx_route!(r_engine_apply, "engine_apply", svc_engine::StartArgs, svc_engine::engine_apply);
ctx_route!(r_engine_stop, "engine_stop", EmptyArgs, svc_engine::engine_stop);
ctx_route!(r_engine_config_preview, "engine_config_preview", svc_engine::PreviewArgs, svc_engine::engine_config_preview);

state_route!(r_list_nodes, "list_nodes", EmptyArgs, svc_node::list_nodes);
state_route!(r_upsert_node, "upsert_node", svc_node::NodeArgs, svc_node::upsert_node);
state_route!(r_move_node, "move_node", svc_node::MoveArgs, svc_node::move_node);
state_route!(r_tag_nodes, "tag_nodes", svc_node::TagArgs, svc_node::tag_nodes);
state_route!(r_export_node_links, "export_node_links", svc_node::IdsArgs, svc_node::export_node_links);
state_route!(r_test_node_latency, "test_node_latency", svc_node::IdArgs, svc_node::test_node_latency);
state_route!(r_test_nodes_latency, "test_nodes_latency", svc_node::IdsArgs, svc_node::test_nodes_latency);
state_route!(r_node_probes, "node_probes", EmptyArgs, svc_node::node_probes);
state_route!(r_clear_node_probes, "clear_node_probes", svc_node::IdsArgs, svc_node::clear_node_probes);
state_route!(r_list_groups, "list_groups", EmptyArgs, svc_group::list_groups);
state_route!(r_upsert_group, "upsert_group", svc_group::GroupArgs, svc_group::upsert_group);
state_route!(r_delete_group, "delete_group", svc_group::IdArgs, svc_group::delete_group);
state_route!(r_select_group_member, "select_group_member", svc_group::SelectMemberArgs, svc_group::select_group_member);
state_route!(r_toggle_group, "toggle_group", svc_group::IdArgs, svc_group::toggle_group);
/// The process list takes no state at all: it is an OS snapshot, like the
/// compile-time engine list.
fn r_list_processes(_ctx: &CoreContext, request: &http::Request) -> http::Response {
    handle("list_processes", &request.body, |args: EmptyArgs| {
        svc_process::list_processes(args)
    })
}
state_route!(r_import_nodes, "import_nodes", svc_import::TextArgs, svc_import::import_nodes);

/// Deleting a node may have to stop the kernel running it, so the handler hands
/// over the whole context rather than just the state.
fn r_delete_node(ctx: &CoreContext, request: &http::Request) -> http::Response {
    handle("delete_node", &request.body, |args: svc_node::IdArgs| {
        svc_node::delete_node(&ctx.state, &ctx.runtime, &ctx.proxy, args)
    })
}

fn r_delete_nodes(ctx: &CoreContext, request: &http::Request) -> http::Response {
    handle("delete_nodes", &request.body, |args: svc_node::IdsArgs| {
        svc_node::delete_nodes(&ctx.state, &ctx.runtime, &ctx.proxy, args)
    })
}

/// The offline delay needs the runtime as well: it refuses while a kernel runs.
fn r_node_delay_offline(ctx: &CoreContext, request: &http::Request) -> http::Response {
    handle("node_delay_offline", &request.body, |args: svc_node::IdArgs| {
        svc_node::node_delay_offline(&ctx.state, &ctx.runtime, args)
    })
}

/// Previewing never writes, so it cannot fail; the envelope still gets a value.
fn r_import_preview(_ctx: &CoreContext, request: &http::Request) -> http::Response {
    handle("import_preview", &request.body, |args: svc_import::TextArgs| {
        Ok::<_, AppError>(svc_import::import_preview(args))
    })
}

/// No runtime in this command's signature: the list is compile-time, so it is
/// served without a [`Ctx`].
fn r_engine_kinds(_ctx: &CoreContext, request: &http::Request) -> http::Response {
    handle("engine_kinds", &request.body, |args: EmptyArgs| {
        svc_engine::engine_kinds(args)
    })
}

/// The traffic commands use the traffic log, which is not part of the service
/// bundle, so they are wired to the log they actually use.
fn r_traffic_sample(ctx: &CoreContext, request: &http::Request) -> http::Response {
    handle("traffic_sample", &request.body, |_: EmptyArgs| {
        svc_engine::traffic_sample_now(&ctx.runtime, &ctx.traffic)
    })
}

fn r_traffic_history(ctx: &CoreContext, request: &http::Request) -> http::Response {
    handle("traffic_history", &request.body, |args: svc_engine::MinutesArgs| {
        svc_engine::traffic_history(&ctx.traffic, args)
    })
}

fn r_traffic_clear_history(ctx: &CoreContext, request: &http::Request) -> http::Response {
    handle("traffic_clear_history", &request.body, |args: EmptyArgs| {
        svc_engine::traffic_clear_history(&ctx.traffic, args)
    })
}

fn r_system_proxy_status(ctx: &CoreContext, request: &http::Request) -> http::Response {
    handle("system_proxy_status", &request.body, |args: EmptyArgs| {
        svc_sysproxy::system_proxy_status(&ctx.proxy, args)
    })
}

fn r_system_proxy_disable(ctx: &CoreContext, request: &http::Request) -> http::Response {
    handle("system_proxy_disable", &request.body, |args: EmptyArgs| {
        svc_sysproxy::system_proxy_disable(&ctx.proxy, args)
    })
}

fn h_health(_ctx: &CoreContext, _request: &http::Request) -> http::Response {
    health()
}

fn h_status(ctx: &CoreContext, _request: &http::Request) -> http::Response {
    status(ctx)
}

fn h_nodes(ctx: &CoreContext, _request: &http::Request) -> http::Response {
    nodes(ctx)
}

fn h_legacy_engine_start(ctx: &CoreContext, request: &http::Request) -> http::Response {
    legacy_engine_start(ctx, request)
}

fn h_legacy_engine_stop(ctx: &CoreContext, _request: &http::Request) -> http::Response {
    legacy_engine_stop(ctx)
}

fn h_shutdown(ctx: &CoreContext, _request: &http::Request) -> http::Response {
    shutdown(ctx)
}

/// Turn a handler failure into the shared envelope. `NotFound` is a 404, bad
/// input is a 400, everything else is a 500 — the split the UI uses to decide
/// between "you asked for the wrong thing" and "the core broke".
pub fn from_error(error: &AppError) -> http::Response {
    let (status, code) = match error {
        AppError::NotFound(_) => (404, "not_found"),
        AppError::InvalidInput(_) => (400, "invalid_input"),
        AppError::Io(_) => (500, "io"),
        AppError::Engine(_) => (500, "engine"),
        AppError::Internal(_) => (500, "internal"),
    };
    failure(status, code, &error.to_string())
}

fn failure(status: u16, code: &str, message: &str) -> http::Response {
    http::Response::json(
        status,
        json!({"error": {"code": code, "message": message}}),
    )
}

/// Constant-time comparison. The token never leaves loopback, but a comparison
/// that returns early is a habit worth not having in a file that also handles
/// remote-looking input.
fn token_matches(provided: Option<&str>, expected: &str) -> bool {
    let Some(provided) = provided else {
        return false;
    };
    let (provided, expected) = (provided.as_bytes(), expected.as_bytes());
    if provided.len() != expected.len() {
        return false;
    }
    let mut difference = 0u8;
    for (left, right) in provided.iter().zip(expected) {
        difference |= left ^ right;
    }
    difference == 0
}

fn health() -> http::Response {
    http::Response::json(
        200,
        json!({"ok": true, "version": env!("CARGO_PKG_VERSION")}),
    )
}

/// What the UI needs from the proxy backend, without the registry-only fields
/// (`bypass`, `pac_url`) it does not render.
#[derive(Serialize)]
struct ProxyView {
    /// True when *this core* is the one holding the system proxy.
    active: bool,
    enabled: bool,
    server: String,
    snapshot_present: bool,
    /// Present only when reading the machine's proxy state failed; the UI shows
    /// "读不到" instead of a convincing-looking false.
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// `/status` is the existing [`EngineStatus`] shape, flattened, plus the proxy
/// block. Flattening keeps the field names identical to the Tauri command, so
/// the C# model can mirror it without a translation table.
#[derive(Serialize)]
struct StatusView {
    #[serde(flatten)]
    engine: EngineStatus,
    proxy: ProxyView,
}

fn status(ctx: &CoreContext) -> http::Response {
    let proxy = match ctx.proxy.status() {
        Ok(status) => ProxyView {
            active: status.active,
            enabled: status.enabled,
            server: status.server,
            snapshot_present: status.snapshot_present,
            error: None,
        },
        Err(error) => ProxyView {
            active: ctx.proxy.is_active(),
            enabled: false,
            server: String::new(),
            snapshot_present: false,
            error: Some(error.to_string()),
        },
    };
    match serde_json::to_value(StatusView {
        engine: ctx.runtime.status(),
        proxy,
    }) {
        Ok(value) => http::Response::json(200, value),
        Err(error) => failure(500, "internal", &format!("状态序列化失败：{error}")),
    }
}

/// One node as the UI lists it: no uuid, no password, no subscription body.
#[derive(Serialize)]
struct NodeView {
    id: String,
    name: String,
    /// Lowercase protocol slug (`vless`, `vmess`, …) — the same spelling
    /// `EngineKind` uses for kernels, so the UI can branch without a second
    /// naming scheme.
    protocol: &'static str,
    /// The server address. Named `host` here because that is what the UI calls
    /// it; the domain keeps the more explicit `server`.
    host: String,
    port: u16,
}

fn protocol_slug(protocol: &Protocol) -> &'static str {
    match protocol {
        Protocol::Vless { .. } => "vless",
        Protocol::Vmess { .. } => "vmess",
        Protocol::Trojan { .. } => "trojan",
        Protocol::Shadowsocks { .. } => "shadowsocks",
        Protocol::Hysteria2 { .. } => "hysteria2",
        Protocol::Tuic { .. } => "tuic",
    }
}

fn nodes(ctx: &CoreContext) -> http::Response {
    let list: Vec<NodeView> = ctx
        .state
        .nodes
        .read()
        .iter()
        .map(|node: &Node| NodeView {
            id: node.id.clone(),
            name: node.name.clone(),
            protocol: protocol_slug(&node.protocol),
            host: node.server.clone(),
            port: node.port,
        })
        .collect();
    match serde_json::to_value(list) {
        Ok(value) => http::Response::json(200, value),
        Err(error) => failure(500, "internal", &format!("节点列表序列化失败：{error}")),
    }
}

/// `POST /engine/start` — the same prepare/start path the Tauri command takes,
/// so the headless core cannot drift from the window it will serve.
fn legacy_engine_start(ctx: &CoreContext, request: &http::Request) -> http::Response {
    let body: Value = match serde_json::from_str(&request.body) {
        Ok(value) => value,
        Err(error) => {
            return failure(
                400,
                "invalid_input",
                &format!("请求体不是合法 JSON：{error}"),
            )
        }
    };
    let Some(kind) = body
        .get("kind")
        .and_then(Value::as_str)
        .and_then(EngineKind::parse)
    else {
        return failure(
            400,
            "invalid_input",
            "kind 必须是 \"xray\" 或 \"sing-box\"",
        );
    };
    let Some(node_id) = body.get("nodeId").and_then(Value::as_str) else {
        return failure(400, "invalid_input", "缺少 nodeId");
    };

    let prepared: AppResult<_> =
        crate::commands::engine::prepare_start(&ctx.state, kind, node_id);
    let request = match prepared {
        Ok(request) => request,
        Err(error) => return from_error(&error),
    };
    match crate::commands::engine::start_engine(&ctx.runtime, &ctx.proxy, request) {
        Ok(_) => {
            crate::commands::engine::remember_session(&ctx.state, kind, node_id);
            http::Response::json(200, json!({"ok": true}))
        }
        Err(error) => from_error(&error),
    }
}

/// `POST /engine/stop` — proxy back first, then the kernel, exactly the order
/// [`crate::commands::engine::engine_stop`] uses: the machine must never point
/// at a port that is already gone.
fn legacy_engine_stop(ctx: &CoreContext) -> http::Response {
    if let Err(error) = ctx.proxy.disable() {
        return from_error(&error);
    }
    if let Err(error) = ctx.runtime.stop() {
        return from_error(&error);
    }
    http::Response::json(200, json!({"ok": true}))
}

/// `POST /shutdown` — only the token holder can ask core to leave. The server
/// loop finishes in-flight responses before returning, so the client gets its
/// `{"ok":true}` rather than a dropped connection.
fn shutdown(ctx: &CoreContext) -> http::Response {
    ctx.request_shutdown();
    http::Response::json(200, json!({"ok": true}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::CoreContext;
    use crate::domain::{GroupKind, ProbeKind, Protocol, RuleAction, RuleMatcher, RoutingRule, Security, Transport};
    use crate::engine::EngineRuntime;
    use crate::error::AppResult;
    use crate::state::AppState;
    use crate::store::Store;
    use crate::sysproxy::{ProxyGuard, ProxyState, SystemProxy};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// A proxy backend that refuses to write: the tests run on the development
    /// machine, where touching the real registry is forbidden. A `write` that
    /// errors (instead of silently succeeding) proves no route tried.
    struct OffProxy;

    impl SystemProxy for OffProxy {
        fn read(&self) -> AppResult<ProxyState> {
            Ok(ProxyState::off())
        }

        fn write(&self, _state: &ProxyState) -> AppResult<()> {
            Err(AppError::Internal(
                "测试不得写系统代理注册表".to_string(),
            ))
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        // The other suites' convention: scratch space under the crate's
        // `target/`, because the sandbox refuses writes to %TEMP%.
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("xmatrix-core-tests");
        std::fs::create_dir_all(&root).unwrap();
        let dir = root.join(format!(
            "routes-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn context() -> CoreContext {
        build_context(false)
    }

    /// A context that also knows one node, for the routes that resolve a node
    /// before they can do anything (preview, start).
    fn context_with_node() -> CoreContext {
        build_context(true)
    }

    fn build_context(with_node: bool) -> CoreContext {
        let dir = temp_dir("routes");
        let state = AppState::load(Store::new(dir.join("store.json")));
        if with_node {
            state
                .nodes
                .write()
                .push(node("n1", "测试节点", "example.com", 443));
        }
        let runtime = Arc::new(EngineRuntime::new(
            dir.join("engines"),
            dir.join("configs"),
            dir.join("logs"),
        ));
        let proxy = Arc::new(ProxyGuard::new(
            Box::new(OffProxy),
            dir.join("proxy-snapshot.json"),
            dir.join("x-matrix.pac"),
        ));
        let traffic = Arc::new(crate::engine::TrafficLog::new(dir.join("traffic.jsonl")));
        CoreContext::new(state, runtime, proxy, traffic)
    }

    /// A context wired to the fake engine runtime: spawning, probing and port
    /// binding are all injected, so no kernel process or listening socket is
    /// created by these tests.
    fn harness_context() -> CoreContext {
        let seed = crate::engine::runtime::tests::harness(true, false);
        let state = AppState::load(Store::new(seed.dir.join("store.json")));
        state.nodes.write().push(crate::engine::runtime::tests::node());
        // Default ports belong to a real user's session; the fake runtime never
        // binds them, but `ensure_free` still refuses a port that is taken.
        {
            let mut settings = state.settings.write();
            settings.mixed_port = free_port();
            settings.api_port = free_port();
        }
        let proxy = Arc::new(ProxyGuard::new(
            Box::new(OffProxy),
            seed.dir.join("proxy-snapshot.json"),
            seed.dir.join("x-matrix.pac"),
        ));
        let traffic = Arc::new(crate::engine::TrafficLog::new(seed.dir.join("traffic.jsonl")));
        CoreContext::new(state, Arc::clone(&seed.runtime), proxy, traffic)
    }

    fn free_port() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    }

    fn node(id: &str, name: &str, server: &str, port: u16) -> Node {
        Node {
            id: id.to_string(),
            name: name.to_string(),
            server: server.to_string(),
            port,
            protocol: Protocol::Vless {
                uuid: "11111111-2222-3333-4444-555555555555".to_string(),
                flow: None,
            },
            transport: Transport::Tcp,
            security: Security::None,
            tags: vec![],
        }
    }

    fn get(path: &str, token: &str) -> http::Request {
        http::Request::new("GET", path, &[("X-Core-Token", token)], "")
    }

    fn post(path: &str, token: &str, body: &str) -> http::Request {
        http::Request::new(
            "POST",
            path,
            &[("X-Core-Token", token), ("Content-Type", "application/json")],
            body,
        )
    }

    fn body(response: &http::Response) -> Value {
        serde_json::from_str(&response.body).unwrap()
    }

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn health_answers_with_ok_and_the_crate_version() {
        let ctx = context();
        let response = dispatch(&ctx, TOKEN, &get("/health", TOKEN));
        assert_eq!(response.status, 200);
        let value = body(&response);
        assert_eq!(value["ok"], json!(true));
        assert_eq!(value["version"], json!(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn a_missing_token_is_401() {
        let ctx = context();
        let request = http::Request::new("GET", "/health", &[], "");
        let response = dispatch(&ctx, TOKEN, &request);
        assert_eq!(response.status, 401);
        assert_eq!(body(&response)["error"]["code"], json!("unauthorized"));
    }

    #[test]
    fn a_wrong_token_is_401_even_for_an_unknown_path() {
        let ctx = context();
        let response = dispatch(&ctx, TOKEN, &get("/no-such-route", "wrong"));
        assert_eq!(response.status, 401);
        assert_eq!(body(&response)["error"]["code"], json!("unauthorized"));
    }

    #[test]
    fn an_unknown_route_is_404_and_keeps_the_error_shape() {
        let ctx = context();
        let response = dispatch(&ctx, TOKEN, &get("/no-such-route", TOKEN));
        assert_eq!(response.status, 404);

        let value = body(&response);
        // Exactly one top-level key, exactly two keys inside `error`: a UI that
        // validates the envelope can rely on it.
        assert_eq!(value.as_object().unwrap().keys().collect::<Vec<_>>(), vec!["error"]);
        let error = value["error"].as_object().unwrap();
        assert_eq!(error.len(), 2);
        assert_eq!(error["code"], json!("not_found"));
        assert!(
            error["message"].as_str().is_some_and(|text| !text.is_empty()),
            "the message must be a non-empty string: {value:?}"
        );
    }

    #[test]
    fn nodes_are_the_five_public_fields_and_nothing_secret() {
        let ctx = context();
        ctx.state
            .nodes
            .write()
            .extend([node("n1", "东京 01", "jp.example.com", 443), node("n2", "新加坡 02", "sg.example.com", 8443)]);

        let response = dispatch(&ctx, TOKEN, &get("/nodes", TOKEN));
        assert_eq!(response.status, 200);
        let list = body(&response);
        let list = list.as_array().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0]["id"], json!("n1"));
        assert_eq!(list[0]["name"], json!("东京 01"));
        assert_eq!(list[0]["protocol"], json!("vless"));
        assert_eq!(list[0]["host"], json!("jp.example.com"));
        assert_eq!(list[0]["port"], json!(443));
        for entry in list {
            let keys: Vec<_> = entry.as_object().unwrap().keys().cloned().collect();
            assert_eq!(keys, vec!["host", "id", "name", "port", "protocol"]);
        }
        // The uuid from the protocol must not leak into a list response.
        assert!(!response.body.contains("11111111"));
    }

    #[test]
    fn status_is_the_engine_shape_plus_the_proxy_block() {
        let ctx = context();
        let response = dispatch(&ctx, TOKEN, &get("/status", TOKEN));
        assert_eq!(response.status, 200);
        let value = body(&response);
        assert_eq!(value["running"], json!(false));
        assert_eq!(value["kind"], Value::Null);
        assert_eq!(value["pid"], json!(0));
        assert_eq!(value["proxy"]["active"], json!(false));
        // No proxy error on a backend that reads cleanly.
        assert!(value["proxy"]["error"].is_null());
    }

    #[test]
    fn engine_start_rejects_an_unknown_kind_with_400() {
        let ctx = context();
        let response = dispatch(
            &ctx,
            TOKEN,
            &post("/engine/start", TOKEN, r#"{"kind":"v2ray","nodeId":"n1"}"#),
        );
        assert_eq!(response.status, 400);
        assert_eq!(body(&response)["error"]["code"], json!("invalid_input"));
    }

    #[test]
    fn engine_start_for_a_missing_node_is_404_and_spawns_nothing() {
        let ctx = context();
        let response = dispatch(
            &ctx,
            TOKEN,
            &post("/engine/start", TOKEN, r#"{"kind":"xray","nodeId":"missing"}"#),
        );
        assert_eq!(response.status, 404);
        let value = body(&response);
        assert_eq!(value["error"]["code"], json!("not_found"));
        assert!(
            value["error"]["message"]
                .as_str()
                .is_some_and(|text| text.contains("missing")),
            "the message should name the node: {value:?}"
        );
        assert!(!ctx.runtime.status().running);
    }

    #[test]
    fn engine_stop_with_nothing_running_still_succeeds() {
        let ctx = context();
        let response = dispatch(&ctx, TOKEN, &post("/engine/stop", TOKEN, ""));
        assert_eq!(response.status, 200);
        assert_eq!(body(&response), json!({"ok": true}));
    }

    #[test]
    fn shutdown_is_recorded_and_answers_ok() {
        let ctx = context();
        assert!(!ctx.is_shutting_down());
        let response = dispatch(&ctx, TOKEN, &post("/shutdown", TOKEN, ""));
        assert_eq!(response.status, 200);
        assert_eq!(body(&response), json!({"ok": true}));
        assert!(ctx.is_shutting_down());
    }

    #[test]
    fn a_malformed_json_body_is_400_with_the_envelope() {
        let ctx = context();
        let response = dispatch(&ctx, TOKEN, &post("/engine/start", TOKEN, "{nope"));
        assert_eq!(response.status, 400);
        assert_eq!(body(&response)["error"]["code"], json!("invalid_input"));
    }

    // ------------------------------------------------------------------
    // P1 batch A: every migrated command answers on
    // `POST /<module>/<command_name>`.
    //
    // Coverage is progressive on purpose. The rewrite moves one command
    // module per batch, so a route is only demanded for the modules named in
    // `COVERED_MODULES`. Modules not named there are still pure-Tauri and are
    // skipped, not silently "passed": when a later batch migrates one, its
    // name is added below and this test starts failing until every command it
    // registers owns a `ROUTES` row (or an explicit `NOT_ROUTED` entry with a
    // reason). A red run is fixed by adding the missing routes — never by
    // deleting a module name from the array.
    // ------------------------------------------------------------------

    const COVERED_MODULES: &[&str] = &["engine", "sysproxy", "node", "group", "process", "import"];

    /// `(module, command)` for every `commands::<module>::<command>` in
    /// `ipc.rs`. Read as text because that list is the registry the Tauri
    /// layer really uses; a second hand-maintained list here would drift.
    fn registered_commands(source: &str) -> Vec<(String, String)> {
        let mut commands = Vec::new();
        for line in source.lines() {
            let Some(rest) = line.trim().strip_prefix("commands::") else {
                continue;
            };
            let mut parts = rest.split("::");
            let module = parts.next().unwrap_or("").trim();
            let command = parts.next().unwrap_or("").trim_end_matches(',').trim();
            if module.is_empty() || command.is_empty() {
                continue;
            }
            commands.push((module.to_string(), command.to_string()));
        }
        commands
    }

    #[test]
    fn every_ipc_command_has_a_route() {
        let registered = registered_commands(include_str!("../ipc.rs"));
        assert!(
            registered.len() >= 24,
            "解析 ipc.rs 只得到 {} 条命令，解析规则可能已过期",
            registered.len()
        );

        // The whitelist must not outlive its commands: a stale entry would
        // hide a missing route just as well as a fake pass.
        for (command, reason) in NOT_ROUTED {
            assert!(
                !reason.trim().is_empty(),
                "NOT_ROUTED 里的 {command} 必须写清为什么没有路由"
            );
            assert!(
                registered.iter().any(|(_, name)| name == command),
                "NOT_ROUTED 里的 {command} 不在 ipc.rs 的注册表中"
            );
        }

        let mut checked = 0usize;
        for (module, command) in &registered {
            if !COVERED_MODULES.contains(&module.as_str()) {
                continue;
            }
            checked += 1;
            if NOT_ROUTED.iter().any(|(name, _)| name == command) {
                continue;
            }
            let path = format!("/{module}/{command}");
            assert!(
                ROUTES
                    .iter()
                    .any(|route| route.method == "POST" && route.path == path),
                "命令 {module}::{command} 已注册，但 POST {path} 没有路由：\
                 在 ROUTES 里补一条，或（仅限壳动作）写进 NOT_ROUTED 并说明理由"
            );
        }
        assert_eq!(
            checked, 44,
            "本批覆盖 engine 22 + sysproxy 2 + node 12 + group 5 + process 1 + import 2 = 44 条命令，实际检查 {checked} 条"
        );
    }

    #[test]
    fn no_two_routes_share_a_method_and_path() {
        for (index, route) in ROUTES.iter().enumerate() {
            for other in &ROUTES[index + 1..] {
                assert!(
                    !(route.method == other.method && route.path == other.path),
                    "重复路由：{} {}",
                    route.method,
                    route.path
                );
            }
        }
    }

    #[test]
    fn engine_status_route_answers_the_idle_status() {
        let ctx = context();
        let response = dispatch(&ctx, TOKEN, &post("/engine/engine_status", TOKEN, ""));
        assert_eq!(response.status, 200, "{}", response.body);
        let value = body(&response);
        assert_eq!(value["running"], json!(false));
        assert_eq!(value["kind"], Value::Null);
    }

    #[test]
    fn engine_kinds_route_lists_both_kernels() {
        let ctx = context();
        let response = dispatch(&ctx, TOKEN, &post("/engine/engine_kinds", TOKEN, ""));
        assert_eq!(response.status, 200, "{}", response.body);
        assert_eq!(body(&response), json!(["xray", "sing-box"]));
    }

    #[test]
    fn engine_binaries_route_reports_paths_and_absence() {
        let ctx = context();
        let response = dispatch(&ctx, TOKEN, &post("/engine/engine_binaries", TOKEN, ""));
        assert_eq!(response.status, 200, "{}", response.body);
        let value = body(&response);
        let list = value.as_array().unwrap();
        assert_eq!(list.len(), 2);
        for entry in list {
            assert!(entry["path"].is_string());
            assert_eq!(entry["present"], json!(false));
        }
    }

    #[test]
    fn engine_version_for_a_missing_binary_is_404() {
        let ctx = context();
        let response = dispatch(
            &ctx,
            TOKEN,
            &post("/engine/engine_version", TOKEN, r#"{"kind":"xray"}"#),
        );
        assert_eq!(response.status, 404);
        assert_eq!(body(&response)["error"]["code"], json!("not_found"));
    }

    #[test]
    fn engine_log_tail_route_reads_an_empty_log() {
        let ctx = context();
        let response = dispatch(
            &ctx,
            TOKEN,
            &post("/engine/engine_log_tail", TOKEN, r#"{"kind":"xray","lines":20}"#),
        );
        assert_eq!(response.status, 200, "{}", response.body);
        assert_eq!(body(&response), json!(""));
    }

    #[test]
    fn engine_log_delta_route_returns_the_zero_delta() {
        let ctx = context();
        let response = dispatch(
            &ctx,
            TOKEN,
            &post("/engine/engine_log_delta", TOKEN, r#"{"kind":"xray","since":0}"#),
        );
        assert_eq!(response.status, 200, "{}", response.body);
        let value = body(&response);
        assert_eq!(value["text"], json!(""));
        assert_eq!(value["reset"], json!(false));
    }

    #[test]
    fn engine_log_clear_route_is_ok_without_a_log_file() {
        let ctx = context();
        let response = dispatch(
            &ctx,
            TOKEN,
            &post("/engine/engine_log_clear", TOKEN, r#"{"kind":"xray"}"#),
        );
        assert_eq!(response.status, 200, "{}", response.body);
        assert_eq!(body(&response), Value::Null);
    }

    #[test]
    fn engine_config_preview_route_renders_for_a_known_node() {
        let ctx = context_with_node();
        let response = dispatch(
            &ctx,
            TOKEN,
            &post(
                "/engine/engine_config_preview",
                TOKEN,
                r#"{"kind":"xray","nodeId":"n1"}"#,
            ),
        );
        assert_eq!(response.status, 200, "{}", response.body);
        let rendered = body(&response);
        let text = rendered.as_str().unwrap_or("");
        assert!(text.contains("example.com"), "预览应包含节点服务器：{text}");
    }

    #[test]
    fn proxy_latency_route_without_a_kernel_is_a_500_engine_error() {
        let ctx = context();
        let response = dispatch(&ctx, TOKEN, &post("/engine/proxy_latency", TOKEN, ""));
        assert_eq!(response.status, 500);
        assert_eq!(body(&response)["error"]["code"], json!("engine"));
    }

    #[test]
    fn proxy_speed_route_without_a_kernel_is_a_500_engine_error() {
        let ctx = context();
        let response = dispatch(&ctx, TOKEN, &post("/engine/proxy_speed", TOKEN, ""));
        assert_eq!(response.status, 500);
        assert_eq!(body(&response)["error"]["code"], json!("engine"));
    }

    #[test]
    fn close_connection_route_without_a_kernel_is_a_500_engine_error() {
        let ctx = context();
        let response = dispatch(
            &ctx,
            TOKEN,
            &post("/engine/close_connection", TOKEN, r#"{"id":"c1"}"#),
        );
        assert_eq!(response.status, 500);
        assert_eq!(body(&response)["error"]["code"], json!("engine"));
    }

    #[test]
    fn close_connections_route_without_a_kernel_is_a_500_engine_error() {
        let ctx = context();
        let response = dispatch(
            &ctx,
            TOKEN,
            &post("/engine/close_connections", TOKEN, r#"{"ids":["c1"]}"#),
        );
        assert_eq!(response.status, 500);
        assert_eq!(body(&response)["error"]["code"], json!("engine"));
    }

    #[test]
    fn group_delay_for_a_missing_group_is_404() {
        let ctx = context();
        let response = dispatch(
            &ctx,
            TOKEN,
            &post("/engine/group_delay", TOKEN, r#"{"id":"missing"}"#),
        );
        assert_eq!(response.status, 404);
        assert_eq!(body(&response)["error"]["code"], json!("not_found"));
    }

    #[test]
    fn node_delay_for_a_missing_node_is_404() {
        let ctx = context();
        let response = dispatch(
            &ctx,
            TOKEN,
            &post("/engine/node_delay", TOKEN, r#"{"id":"missing"}"#),
        );
        assert_eq!(response.status, 404);
        assert_eq!(body(&response)["error"]["code"], json!("not_found"));
    }

    #[test]
    fn traffic_sample_route_is_null_without_a_stats_channel() {
        let ctx = context();
        let response = dispatch(&ctx, TOKEN, &post("/engine/traffic_sample", TOKEN, ""));
        assert_eq!(response.status, 200, "{}", response.body);
        assert_eq!(body(&response), Value::Null);
    }

    #[test]
    fn traffic_history_route_starts_empty() {
        let ctx = context();
        let response = dispatch(
            &ctx,
            TOKEN,
            &post("/engine/traffic_history", TOKEN, r#"{"minutes":10}"#),
        );
        assert_eq!(response.status, 200, "{}", response.body);
        assert_eq!(body(&response), json!([]));
    }

    #[test]
    fn traffic_clear_history_route_answers_null() {
        let ctx = context();
        let response = dispatch(
            &ctx,
            TOKEN,
            &post("/engine/traffic_clear_history", TOKEN, ""),
        );
        assert_eq!(response.status, 200, "{}", response.body);
        assert_eq!(body(&response), Value::Null);
    }

    #[test]
    fn engine_start_and_stop_routes_drive_the_fake_runtime() {
        let ctx = harness_context();
        let start = dispatch(
            &ctx,
            TOKEN,
            &post("/engine/engine_start", TOKEN, r#"{"kind":"xray","nodeId":"n1"}"#),
        );
        assert_eq!(start.status, 200, "{}", start.body);
        assert_eq!(body(&start)["running"], json!(true));
        assert_eq!(body(&start)["node_id"], json!("n1"));

        let stop = dispatch(&ctx, TOKEN, &post("/engine/engine_stop", TOKEN, ""));
        assert_eq!(stop.status, 200, "{}", stop.body);
        assert_eq!(body(&stop)["running"], json!(false));
        assert!(!ctx.runtime.status().running);
    }

    #[test]
    fn engine_apply_route_starts_the_node() {
        let ctx = harness_context();
        let response = dispatch(
            &ctx,
            TOKEN,
            &post("/engine/engine_apply", TOKEN, r#"{"kind":"xray","nodeId":"n1"}"#),
        );
        assert_eq!(response.status, 200, "{}", response.body);
        assert_eq!(body(&response)["running"], json!(true));
        assert_eq!(ctx.runtime.status().node_id.as_deref(), Some("n1"));
    }

    #[test]
    fn system_proxy_status_route_reports_inactive() {
        let ctx = context();
        let response = dispatch(
            &ctx,
            TOKEN,
            &post("/sysproxy/system_proxy_status", TOKEN, ""),
        );
        assert_eq!(response.status, 200, "{}", response.body);
        assert_eq!(body(&response)["active"], json!(false));
    }

    #[test]
    fn system_proxy_disable_route_is_idempotent() {
        let ctx = context();
        let response = dispatch(
            &ctx,
            TOKEN,
            &post("/sysproxy/system_proxy_disable", TOKEN, ""),
        );
        assert_eq!(response.status, 200, "{}", response.body);
        assert_eq!(body(&response)["active"], json!(false));
    }

    #[test]
    fn a_missing_kind_on_engine_log_tail_is_400() {
        let ctx = context();
        let response = dispatch(
            &ctx,
            TOKEN,
            &post("/engine/engine_log_tail", TOKEN, r#"{"lines":10}"#),
        );
        assert_eq!(response.status, 400);
        let value = body(&response);
        assert_eq!(value["error"]["code"], json!("invalid_input"));
        assert!(
            value["error"]["message"]
                .as_str()
                .is_some_and(|text| text.contains("engine_log_tail")),
            "消息应点名命令：{value:?}"
        );
    }

    #[test]
    fn ids_must_be_a_list_on_close_connections() {
        let ctx = context();
        let response = dispatch(
            &ctx,
            TOKEN,
            &post("/engine/close_connections", TOKEN, r#"{"ids":"c1"}"#),
        );
        assert_eq!(response.status, 400);
        assert_eq!(body(&response)["error"]["code"], json!("invalid_input"));
    }

    #[test]
    fn a_non_object_body_is_400_even_for_a_no_arg_command() {
        let ctx = context();
        let response = dispatch(&ctx, TOKEN, &post("/engine/engine_status", TOKEN, "[1]"));
        assert_eq!(response.status, 400);
        assert_eq!(body(&response)["error"]["code"], json!("invalid_input"));
    }

// ------------------------------------------------------------------
    // P1 batch B: node / group / process / import.
    //
    // Every test builds its own context, so the fake runtime and the
    // registry-refusing proxy are the only OS touched. Nothing here starts a
    // kernel: the latency tests listen on a loopback port this process owns,
    // and the offline-delay tests stop at the refusal path before any temp
    // kernel could be written.
    // ------------------------------------------------------------------

    fn node_json(id: &str, name: &str, server: &str, port: u16) -> Value {
        serde_json::to_value(node(id, name, server, port)).unwrap()
    }

    #[test]
    fn node_list_route_answers_the_node_list() {
        let ctx = context_with_node();
        let response = dispatch(&ctx, TOKEN, &post("/node/list_nodes", TOKEN, ""));
        assert_eq!(response.status, 200, "{}", response.body);
        let value = body(&response);
        assert_eq!(value.as_array().unwrap().len(), 1);
        assert_eq!(value[0]["name"], json!("测试节点"));
    }

    #[test]
    fn upsert_node_route_adds_then_updates() {
        let ctx = context();
        let payload = json!({"node": node_json("n1", "甲", "a.example", 443)}).to_string();
        let response = dispatch(&ctx, TOKEN, &post("/node/upsert_node", TOKEN, &payload));
        assert_eq!(response.status, 200, "{}", response.body);
        assert_eq!(body(&response)[0]["name"], json!("甲"));

        let payload = json!({"node": node_json("n1", "乙", "a.example", 443)}).to_string();
        let response = dispatch(&ctx, TOKEN, &post("/node/upsert_node", TOKEN, &payload));
        assert_eq!(response.status, 200, "{}", response.body);
        let value = body(&response);
        assert_eq!(value.as_array().unwrap().len(), 1, "same id replaces, not appends");
        assert_eq!(value[0]["name"], json!("乙"));
    }

    #[test]
    fn tag_nodes_route_tags_the_selection() {
        let ctx = context_with_node();
        let payload = json!({"ids": ["n1"], "tag": "  trusted  ", "remove": false}).to_string();
        let response = dispatch(&ctx, TOKEN, &post("/node/tag_nodes", TOKEN, &payload));
        assert_eq!(response.status, 200, "{}", response.body);
        assert_eq!(body(&response)[0]["tags"], json!(["trusted"]));
    }

    #[test]
    fn an_empty_tag_on_tag_nodes_is_400() {
        let ctx = context_with_node();
        let payload = json!({"ids": ["n1"], "tag": "   ", "remove": false}).to_string();
        let response = dispatch(&ctx, TOKEN, &post("/node/tag_nodes", TOKEN, &payload));
        assert_eq!(response.status, 400);
        let value = body(&response);
        assert_eq!(value["error"]["code"], json!("invalid_input"));
        assert!(
            value["error"]["message"]
                .as_str()
                .is_some_and(|text| text.contains("标签")),
            "{value:?}"
        );
    }

    #[test]
    fn move_node_route_swaps_neighbours() {
        let ctx = context();
        ctx.state
            .nodes
            .write()
            .extend([node("n1", "一", "a.example", 443), node("n2", "二", "b.example", 443)]);

        let response = dispatch(&ctx, TOKEN, &post("/node/move_node", TOKEN, r#"{"id":"n2","delta":-1}"#));

        assert_eq!(response.status, 200, "{}", response.body);
        let value = body(&response);
        assert_eq!(value[0]["id"], json!("n2"));
        assert_eq!(value[1]["id"], json!("n1"));
    }

    #[test]
    fn delete_node_route_removes_it_and_answers_the_rest() {
        let ctx = context_with_node();
        let response = dispatch(&ctx, TOKEN, &post("/node/delete_node", TOKEN, r#"{"id":"n1"}"#));
        assert_eq!(response.status, 200, "{}", response.body);
        assert_eq!(body(&response), json!([]));
        assert!(ctx.state.nodes.read().is_empty());
    }

    #[test]
    fn deleting_a_missing_node_is_404() {
        let ctx = context();
        let response = dispatch(&ctx, TOKEN, &post("/node/delete_node", TOKEN, r#"{"id":"nope"}"#));
        assert_eq!(response.status, 404);
        assert_eq!(body(&response)["error"]["code"], json!("not_found"));
    }

    #[test]
    fn delete_nodes_route_removes_a_selection() {
        let ctx = context();
        ctx.state
            .nodes
            .write()
            .extend([node("n1", "一", "a.example", 443), node("n2", "二", "b.example", 443)]);

        let response = dispatch(&ctx, TOKEN, &post("/node/delete_nodes", TOKEN, r#"{"ids":["n1","n2"]}"#));

        assert_eq!(response.status, 200, "{}", response.body);
        assert_eq!(body(&response), json!([]));
    }

    #[test]
    fn export_node_links_route_writes_one_link_per_line() {
        let ctx = context_with_node();
        let response = dispatch(&ctx, TOKEN, &post("/node/export_node_links", TOKEN, r#"{"ids":["n1"]}"#));
        assert_eq!(response.status, 200, "{}", response.body);
        let links = body(&response);
        let text = links.as_str().expect("a single string");
        assert_eq!(text.lines().count(), 1);
        assert!(text.starts_with("vless://"), "{text}");
    }

    #[test]
    fn export_node_links_without_a_selection_is_400() {
        let ctx = context();
        let response = dispatch(&ctx, TOKEN, &post("/node/export_node_links", TOKEN, r#"{"ids":[]}"#));
        assert_eq!(response.status, 400);
        assert_eq!(body(&response)["error"]["code"], json!("invalid_input"));
    }

    #[test]
    fn test_node_latency_route_measures_a_local_listener() {
        // A listening socket this process owns: the measurement is real, but it
        // never leaves loopback and there is no proxy or kernel involved.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let ctx = context();
        ctx.state
            .nodes
            .write()
            .push(node("n1", "本机", "127.0.0.1", port));

        let response = dispatch(&ctx, TOKEN, &post("/node/test_node_latency", TOKEN, r#"{"id":"n1"}"#));

        assert_eq!(response.status, 200, "{}", response.body);
        assert!(body(&response).is_number(), "{:?}", body(&response));
    }

    #[test]
    fn test_node_latency_for_a_missing_node_is_404() {
        let ctx = context();
        let response = dispatch(&ctx, TOKEN, &post("/node/test_node_latency", TOKEN, r#"{"id":"nope"}"#));
        assert_eq!(response.status, 404);
        assert_eq!(body(&response)["error"]["code"], json!("not_found"));
    }

    #[test]
    fn test_nodes_latency_route_returns_one_row_per_target() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let ctx = context();
        ctx.state
            .nodes
            .write()
            .push(node("n1", "本机", "127.0.0.1", port));

        let response = dispatch(&ctx, TOKEN, &post("/node/test_nodes_latency", TOKEN, r#"{"ids":["n1"]}"#));

        assert_eq!(response.status, 200, "{}", response.body);
        let value = body(&response);
        assert_eq!(value.as_array().unwrap().len(), 1);
        assert_eq!(value[0]["id"], json!("n1"));
        assert!(value[0]["ms"].is_number(), "{value:?}");
        // Remembered as a side effect, so a restart can show the number.
        assert!(ctx.state.probes.read().contains_key("n1"));
    }

    #[test]
    fn node_probes_route_starts_empty() {
        let ctx = context();
        let response = dispatch(&ctx, TOKEN, &post("/node/node_probes", TOKEN, ""));
        assert_eq!(response.status, 200, "{}", response.body);
        assert_eq!(body(&response), json!({}));
    }

    #[test]
    fn clear_node_probes_route_forgets_the_ids() {
        let ctx = context_with_node();
        ctx.state
            .record_probe(&"n1".to_string(), Some(12), ProbeKind::Tcp)
            .unwrap();

        let response = dispatch(&ctx, TOKEN, &post("/node/clear_node_probes", TOKEN, r#"{"ids":["n1"]}"#));

        assert_eq!(response.status, 200, "{}", response.body);
        assert_eq!(body(&response), json!({}));
        assert!(ctx.state.probes.read().is_empty());
    }

    #[test]
    fn node_delay_offline_for_a_missing_node_is_404() {
        // Resolving the node comes first, so this fails before any throwaway
        // kernel could be configured or spawned.
        let ctx = context();
        let response = dispatch(&ctx, TOKEN, &post("/node/node_delay_offline", TOKEN, r#"{"id":"missing"}"#));
        assert_eq!(response.status, 404);
        let value = body(&response);
        assert_eq!(value["error"]["code"], json!("not_found"));
        assert!(
            value["error"]["message"]
                .as_str()
                .is_some_and(|text| text.contains("missing")),
            "{value:?}"
        );
    }

    #[test]
    fn node_delay_offline_refuses_while_the_kernel_runs() {
        let ctx = harness_context();
        let started = dispatch(
            &ctx,
            TOKEN,
            &post("/engine/engine_start", TOKEN, r#"{"kind":"xray","nodeId":"n1"}"#),
        );
        assert_eq!(started.status, 200, "{}", started.body);

        let response = dispatch(&ctx, TOKEN, &post("/node/node_delay_offline", TOKEN, r#"{"id":"n1"}"#));

        assert_eq!(response.status, 400, "{}", response.body);
        let value = body(&response);
        assert_eq!(value["error"]["code"], json!("invalid_input"));
        assert!(
            value["error"]["message"]
                .as_str()
                .is_some_and(|text| text.contains("内核正在运行")),
            "{value:?}"
        );
    }

    #[test]
    fn group_list_route_starts_empty() {
        let ctx = context();
        let response = dispatch(&ctx, TOKEN, &post("/group/list_groups", TOKEN, ""));
        assert_eq!(response.status, 200, "{}", response.body);
        assert_eq!(body(&response), json!([]));
    }

    fn group_json() -> Value {
        json!({
            "id": "g1",
            "name": "手动",
            "enabled": true,
            "kind": "select",
            "members": ["n1", "n2"],
            "selected": null
        })
    }

    #[test]
    fn upsert_group_route_adds_a_group() {
        let ctx = context();
        let payload = json!({"group": group_json()}).to_string();
        let response = dispatch(&ctx, TOKEN, &post("/group/upsert_group", TOKEN, &payload));
        assert_eq!(response.status, 200, "{}", response.body);
        let value = body(&response);
        assert_eq!(value[0]["name"], json!("手动"));
        assert_eq!(value[0]["kind"], json!("select"));
    }

    #[test]
    fn delete_group_route_rewrites_rules_to_the_active_node() {
        let ctx = context();
        ctx.state.groups.write().push(crate::domain::ProxyGroup {
            id: "g1".into(),
            name: "手动".into(),
            enabled: true,
            kind: GroupKind::Select,
            members: vec!["n1".into()],
            selected: None,
        });
        ctx.state.rules.write().push(RoutingRule {
            id: "r1".into(),
            name: "走组".into(),
            enabled: true,
            matcher: RuleMatcher::DomainSuffix { value: "example.com".into() },
            action: RuleAction::Group { group_id: "g1".into() },
        });

        let response = dispatch(&ctx, TOKEN, &post("/group/delete_group", TOKEN, r#"{"id":"g1"}"#));

        assert_eq!(response.status, 200, "{}", response.body);
        assert_eq!(body(&response), json!([]));
        assert_eq!(ctx.state.rules.read()[0].action, RuleAction::Proxy);
    }

    #[test]
    fn select_group_member_route_picks_the_member() {
        let ctx = context();
        ctx.state.groups.write().push(crate::domain::ProxyGroup {
            id: "g1".into(),
            name: "手动".into(),
            enabled: true,
            kind: GroupKind::Select,
            members: vec!["n1".into(), "n2".into()],
            selected: None,
        });

        let response = dispatch(
            &ctx,
            TOKEN,
            &post("/group/select_group_member", TOKEN, r#"{"id":"g1","nodeId":"n2"}"#),
        );

        assert_eq!(response.status, 200, "{}", response.body);
        assert_eq!(body(&response)[0]["selected"], json!("n2"));
    }

    #[test]
    fn selecting_a_member_outside_the_group_is_400() {
        let ctx = context();
        ctx.state.groups.write().push(crate::domain::ProxyGroup {
            id: "g1".into(),
            name: "手动".into(),
            enabled: true,
            kind: GroupKind::Select,
            members: vec!["n1".into()],
            selected: None,
        });

        let response = dispatch(
            &ctx,
            TOKEN,
            &post("/group/select_group_member", TOKEN, r#"{"id":"g1","nodeId":"n9"}"#),
        );

        assert_eq!(response.status, 400);
        assert_eq!(body(&response)["error"]["code"], json!("invalid_input"));
    }

    #[test]
    fn toggle_group_route_flips_enabled() {
        let ctx = context();
        ctx.state.groups.write().push(crate::domain::ProxyGroup {
            id: "g1".into(),
            name: "手动".into(),
            enabled: true,
            kind: GroupKind::Select,
            members: vec!["n1".into()],
            selected: None,
        });

        let response = dispatch(&ctx, TOKEN, &post("/group/toggle_group", TOKEN, r#"{"id":"g1"}"#));

        assert_eq!(response.status, 200, "{}", response.body);
        assert_eq!(body(&response)[0]["enabled"], json!(false));
    }

    #[test]
    fn deleting_a_missing_group_is_404() {
        let ctx = context();
        let response = dispatch(&ctx, TOKEN, &post("/group/delete_group", TOKEN, r#"{"id":"nope"}"#));
        assert_eq!(response.status, 404);
        assert_eq!(body(&response)["error"]["code"], json!("not_found"));
    }

    #[test]
    fn import_preview_route_parses_a_share_link() {
        let ctx = context();
        let payload = json!({"text": "vless://uuid-1@example.com:443?security=tls&sni=a.example.com"}).to_string();
        let response = dispatch(&ctx, TOKEN, &post("/import/import_preview", TOKEN, &payload));
        assert_eq!(response.status, 200, "{}", response.body);
        let value = body(&response);
        assert_eq!(value["nodes"].as_array().unwrap().len(), 1);
        assert_eq!(value["nodes"][0]["server"], json!("example.com"));
        assert_eq!(value["skipped"], json!([]));
    }

    #[test]
    fn import_nodes_route_counts_duplicates_on_the_second_paste() {
        let ctx = context();
        let payload = json!({"text": "vless://uuid-1@example.com:443?security=tls&sni=a.example.com"}).to_string();

        let first = dispatch(&ctx, TOKEN, &post("/import/import_nodes", TOKEN, &payload));
        assert_eq!(first.status, 200, "{}", first.body);
        let value = body(&first);
        assert_eq!(value["imported"], json!(1));
        assert_eq!(value["duplicates"], json!(0));

        let second = dispatch(&ctx, TOKEN, &post("/import/import_nodes", TOKEN, &payload));
        assert_eq!(second.status, 200, "{}", second.body);
        let value = body(&second);
        assert_eq!(value["imported"], json!(0));
        assert_eq!(value["duplicates"], json!(1));
        assert_eq!(ctx.state.nodes.read().len(), 1);
    }

    #[test]
    fn list_processes_route_answers_a_list() {
        // Enumeration only: no kernel, no registry, no network.
        let ctx = context();
        let response = dispatch(&ctx, TOKEN, &post("/process/list_processes", TOKEN, ""));
        assert_eq!(response.status, 200, "{}", response.body);
        assert!(body(&response).is_array());
    }
}
