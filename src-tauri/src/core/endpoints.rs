//! The five routes the P0 UI needs, plus `POST /shutdown`.
//!
//! Every handler is a plain function over [`CoreContext`], so the routing table
//! and the error envelope are testable without binding a socket. The server
//! thread only parses HTTP, checks the token and hands the request here.
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

/// Check the token, then route. Authentication comes first on purpose: an
/// unknown path without a token is still a 401, because a caller that has not
/// proven it belongs here must not learn which paths exist.
pub fn dispatch(ctx: &CoreContext, token: &str, request: &http::Request) -> http::Response {
    if !token_matches(request.header("x-core-token"), token) {
        return failure(401, "unauthorized", "X-Core-Token 缺失或不匹配");
    }

    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/health") => health(),
        ("GET", "/status") => status(ctx),
        ("GET", "/nodes") => nodes(ctx),
        ("POST", "/engine/start") => engine_start(ctx, request),
        ("POST", "/engine/stop") => engine_stop(ctx),
        ("POST", "/shutdown") => shutdown(ctx),
        (method, path) => failure(
            404,
            "not_found",
            &format!("未知路由：{method} {path}"),
        ),
    }
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
fn engine_start(ctx: &CoreContext, request: &http::Request) -> http::Response {
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
fn engine_stop(ctx: &CoreContext) -> http::Response {
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
    use crate::domain::{Protocol, Security, Transport};
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
        let dir = temp_dir("routes");
        let state = AppState::load(Store::new(dir.join("store.json")));
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
        CoreContext::new(state, runtime, proxy)
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
}