//! Engine commands — the Tauri door to the engine business layer.
//!
//! Everything that can block (spawning, the readiness probe, an HTTP sample) is
//! still pushed onto a blocking thread, so the webview keeps painting while a
//! kernel boots. The bodies themselves live in [`crate::services::engine`],
//! shared with the headless core's HTTP channel; this file keeps the command
//! signatures, the native-dialog commands and the managed-state wiring.
//!
//! The UI never learns about ports, config files or pids beyond what
//! [`EngineStatus`] reports.

use std::sync::Arc;

use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::engine::{
    EngineBinary, EngineKind, EngineRuntime, EngineStatus, TrafficLog, TrafficPoint,
};
use crate::error::{AppError, AppResult};
use crate::services::engine as svc;
use crate::services::{Ctx, EmptyArgs, NOOP_NOTIFIER};
use crate::state::AppState;
use crate::sysproxy::ProxyGuard;

pub(crate) type Runtime<'a> = State<'a, Arc<EngineRuntime>>;
pub(crate) type Proxy<'a> = State<'a, Arc<ProxyGuard>>;
type Traffic<'a> = State<'a, Arc<TrafficLog>>;

/// The business helpers that used to live here are now in
/// [`crate::services::engine`]. Other command modules and the tray already
/// import them through this path, so the move keeps that surface stable
/// instead of rewriting every caller in the same batch.
pub(crate) use crate::services::engine::{
    prepare_start, remember_session, render_for_preview, start_engine, stop_if_running_node,
};

/// A context with only the runtime: enough for status, binaries, logs and the
/// config preview.
fn bare<'a, 'r>(_runtime: &'a Runtime<'r>) -> Ctx<'a> {
    Ctx {
        state: None,
        runtime: _runtime.inner(),
        proxy: None,
        notify: &NOOP_NOTIFIER,
    }
}

/// A context that also carries the application state.
fn with_state<'a, 'r>(state: &'a State<'r, AppState>, _runtime: &'a Runtime<'r>) -> Ctx<'a> {
    Ctx {
        state: Some(state.inner()),
        runtime: _runtime.inner(),
        proxy: None,
        notify: &NOOP_NOTIFIER,
    }
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
    svc::engine_status(&bare(&runtime), EmptyArgs {}).unwrap_or_default()
}

/// Which kernels this build knows how to drive.
#[tauri::command]
#[specta::specta]
pub fn engine_kinds() -> Vec<EngineKind> {
    svc::engine_kinds(EmptyArgs {}).unwrap_or_default()
}

/// Where each kernel binary is expected, and whether it is actually there.
#[tauri::command]
#[specta::specta]
pub fn engine_binaries(runtime: Runtime<'_>) -> Vec<EngineBinary> {
    svc::engine_binaries(&bare(&runtime), EmptyArgs {}).unwrap_or_default()
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
    svc::engine_version(&bare(&runtime), svc::KindArgs { kind })
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
//
// Shell-only: a native dialog belongs to the window process, so this command is
// listed in `NOT_ROUTED` and never becomes an HTTP route.
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
    let (port, url) = svc::proxy_speed_target(&with_state(&state, &runtime))?;
    let bytes_per_second = run_blocking(move || svc::proxy_speed_now(port, &url)).await?;
    Ok(bytes_per_second)
}

/// The tail of a kernel log. Empty string means "that kernel never ran".
#[tauri::command]
#[specta::specta]
pub fn engine_log_tail(runtime: Runtime<'_>, kind: EngineKind, lines: u32) -> AppResult<String> {
    svc::engine_log_tail(&bare(&runtime), svc::LogTailArgs { kind, lines })
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
    svc::engine_log_delta(&bare(&runtime), svc::LogDeltaArgs { kind, since })
}

/// Truncate a kernel's log.
#[tauri::command]
#[specta::specta]
pub fn engine_log_clear(runtime: Runtime<'_>, kind: EngineKind) -> AppResult<()> {
    svc::engine_log_clear(&bare(&runtime), svc::KindArgs { kind })
}

/// Open the engines folder in Explorer, so the user can drop a kernel in.
//
// Shell-only: it opens a window on the machine that owns the process, which a
// headless core has no business doing. Listed in `NOT_ROUTED`.
#[tauri::command]
#[specta::specta]
pub fn engine_open_dir(app: AppHandle, runtime: Runtime<'_>) -> AppResult<()> {
    let dir = runtime.engines_dir().display().to_string();
    app.opener()
        .open_path(dir, None::<&str>)
        .map_err(|error| AppError::Internal(error.to_string()))
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
    let request = svc::prepare_start(state.inner(), kind, &node_id)?;
    let runtime = Arc::clone(runtime.inner());
    let proxy = Arc::clone(proxy.inner());
    let status = run_blocking(move || svc::start_engine(&runtime, &proxy, request)).await?;
    svc::remember_session(state.inner(), kind, &node_id);
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
    let request = svc::prepare_start(state, kind, node_id)?;
    let was_running = runtime.status().running;
    let status = run_blocking(move || {
        if was_running {
            runtime.stop()?;
        }
        svc::start_engine(&runtime, &proxy, request)
    })
    .await?;
    svc::remember_session(state, kind, node_id);
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
    apply_engine(&app, kind, &node_id, state.inner(), runtime, proxy).await
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
    let status = run_blocking(move || svc::engine_stop_now(&runtime, &proxy)).await?;
    crate::refresh_tray(&app);
    Ok(status)
}

/// One statistics sample. `None` means "nothing is running", which the UI shows
/// as an empty state rather than an error.
#[tauri::command]
#[specta::specta]
pub async fn traffic_sample(
    runtime: Runtime<'_>,
    log: Traffic<'_>,
) -> AppResult<Option<crate::engine::stats::TrafficSample>> {
    let runtime = Arc::clone(runtime.inner());
    let log = Arc::clone(log.inner());
    run_blocking(move || svc::traffic_sample_now(&runtime, &log)).await
}

/// Per-minute history, oldest first. `minutes <= 0` returns everything kept
/// (seven days at most).
#[tauri::command]
#[specta::specta]
pub fn traffic_history(log: Traffic<'_>, minutes: f64) -> Vec<TrafficPoint> {
    svc::traffic_history(log.inner(), svc::MinutesArgs { minutes }).unwrap_or_default()
}

#[tauri::command]
#[specta::specta]
pub fn traffic_clear_history(log: Traffic<'_>) -> AppResult<()> {
    svc::traffic_clear_history(log.inner(), EmptyArgs {})
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
    svc::engine_config_preview(
        &with_state(&state, &runtime),
        svc::PreviewArgs { kind, node_id },
    )
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
    let (port, url) = svc::proxy_latency_target(&with_state(&state, &runtime))?;
    run_blocking(move || svc::proxy_latency_now(port, &url)).await
}

/// Drop one live connection (sing-box only — Xray has no connection table).
#[tauri::command]
#[specta::specta]
pub fn close_connection(runtime: Runtime<'_>, id: String) -> AppResult<()> {
    svc::close_connection(&bare(&runtime), svc::IdArgs { id })
}

/// Drop several connections at once; an empty list means all of them.
///
/// Same two preconditions as `close_connection`, phrased for the plural case:
/// "close everything" on Xray is not a missing feature of the button, it is a
/// kernel that has no connection table to clear.
#[tauri::command]
#[specta::specta]
pub fn close_connections(runtime: Runtime<'_>, ids: Vec<String>) -> AppResult<()> {
    svc::close_connections(&bare(&runtime), svc::IdsArgs { ids })
}

/// Real delay of one strategy group, measured by the running sing-box.
#[tauri::command]
#[specta::specta]
pub async fn group_delay(
    state: State<'_, AppState>,
    runtime: Runtime<'_>,
    id: String,
) -> AppResult<u32> {
    let (tag, url) = svc::group_delay_target(&with_state(&state, &runtime), &id)?;
    let runtime = Arc::clone(runtime.inner());
    run_blocking(move || svc::group_delay_now(&runtime, &tag, &url)).await
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
    let (tag, url, node_id) = svc::node_delay_target(&with_state(&state, &runtime), &id)?;
    let runtime = Arc::clone(runtime.inner());
    let outcome = run_blocking(move || Ok(svc::node_delay_probe(&runtime, &tag, &url))).await?;
    svc::node_delay_finish(state.inner(), &node_id, outcome)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use parking_lot::Mutex;

    use super::{start_engine, stop_if_running_node};
    use crate::services::engine::{tun_permission_note, StartRequest};
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