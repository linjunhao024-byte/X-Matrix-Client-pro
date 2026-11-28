//! Business layer shared by the two transports.
//!
//! A command used to *be* the business: the Tauri wrapper read the managed
//! state, rendered a config, touched the kernel and called back into the
//! webview. P1 splits that in two — [`commands`](crate::commands) keeps the
//! Tauri signature and the shell-only bits (file dialogs, Explorer, tray), and
//! everything else lives here.
//!
//! The layer depends on no Tauri type. [`Ctx`] borrows the same `Arc`s the app
//! manages, and the handful of commands that legitimately need the shell stay
//! in the command file and are listed in [`crate::core::endpoints::NOT_ROUTED`].
//!
//! Events are the one thing a library cannot do by itself: a Tauri build emits
//! them through `AppHandle`, and the headless core has no window to tell. Both
//! sides go through [`Notifier`], and the core passes [`NoopNotifier`]. Batch A
//! has no emit sites — the pre-split `engine.rs` and `sysproxy.rs` had none —
//! so only the no-op end is wired; the `AppHandle` implementation arrives with
//! the first batch that actually emits.

use std::sync::Arc;

use crate::engine::EngineRuntime;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::sysproxy::ProxyGuard;

pub mod engine;
pub mod group;
pub mod import;
pub mod node;
pub mod process;
pub mod routing;
pub mod ruleset;
pub mod subscription;
pub mod sysproxy;

/// The one side effect that cannot move into the library: telling the UI that
/// something happened. Methods here are fire-and-forget, exactly like `emit`.
pub trait Notifier: Send + Sync {
    fn emit(&self, event: &str, payload: serde_json::Value);
}

/// The core's notifier. There is no window on the other end of an HTTP channel,
/// and P1 keeps the UI on its existing event stream, so this does nothing on
/// purpose. P2 turns the UI into a poller and this stays empty.
pub struct NoopNotifier;

impl Notifier for NoopNotifier {
    fn emit(&self, _event: &str, _payload: serde_json::Value) {}
}

/// A `'static` instance, so a [`Ctx`] can point at it without a temporary.
pub static NOOP_NOTIFIER: NoopNotifier = NoopNotifier;

/// Empty request body for commands that take no arguments.
///
/// `{}` deserializes into it and an empty body is normalized to `{}` before
/// parsing, so `POST /engine/stop` with no body is still a valid call.
#[derive(Debug, Default, serde::Deserialize)]
pub struct EmptyArgs {}

/// What a service function is allowed to see, borrowed from the caller.
///
/// Every field except the runtime is optional because the two transports do not
/// own the same things: the Tauri process manages a `ProxyGuard` and a
/// `TrafficLog`, while a service unit test can leave them out. A function that
/// needs one asks through [`Ctx::state`]/[`Ctx::proxy`] and
/// gets a readable [`AppError::Internal`] if the caller forgot to wire it — that
/// is a programming mistake, not a user-facing failure.
pub struct Ctx<'a> {
    pub state: Option<&'a AppState>,
    pub runtime: &'a Arc<EngineRuntime>,
    pub proxy: Option<&'a Arc<ProxyGuard>>,
    pub notify: &'a dyn Notifier,
}

impl<'a> Ctx<'a> {
    /// A context with only the runtime: enough for the kernel's own status,
    /// logs, binaries and config preview.
    pub fn new(runtime: &'a Arc<EngineRuntime>, notify: &'a dyn Notifier) -> Self {
        Self {
            state: None,
            runtime,
            proxy: None,
            notify,
        }
    }

    pub fn state(&self) -> AppResult<&'a AppState> {
        self.state
            .ok_or_else(|| AppError::Internal("该命令需要应用状态，但调用方没有提供".into()))
    }

    pub fn proxy(&self) -> AppResult<&'a Arc<ProxyGuard>> {
        self.proxy
            .ok_or_else(|| AppError::Internal("该命令需要系统代理，但调用方没有提供".into()))
    }
}