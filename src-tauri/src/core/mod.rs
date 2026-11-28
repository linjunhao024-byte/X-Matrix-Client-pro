//! The headless core: the same state, runtime and proxy guard the Tauri window
//! owns, minus the window, exposed over a token-guarded loopback HTTP channel.
//!
//! Why a separate process at all (from `docs/REWRITE-PLAN.md`): the UI crash and
//! the kernel lifetime must stop being the same event. A window that dies while
//! the user is browsing should not take the tunnel with it, and an Avalonia
//! window should not have to link Tauri to start a kernel. So the core owns the
//! long-lived things ([`AppState`], [`EngineRuntime`], [`ProxyGuard`]) and the
//! window is a client of the routes in [`endpoints`].
//!
//! What this module deliberately does *not* do: call `ProxyGuard::recover()`.
//! Recovery writes the system proxy registry, and a core that can be started
//! from a terminal (or a test) must not touch that on the developer machine.
//! Recovery stays in `lib.rs`, where a real window launch means a real user.
//!
//! The endpoint file (`%APPDATA%\com.xmatrix.client\core.endpoint`) is how the
//! UI finds a running core: `{"port":47100,"token":"…","pid":1234}`. It is
//! written with a temporary file and a rename so a reader never sees half a
//! document.

pub mod endpoints;
pub mod http;
pub mod server;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::domain::Settings;
use crate::engine::{EngineRuntime, TrafficLog};
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::store::Store;
use crate::sysproxy::{ProxyGuard, SystemProxy};

/// Everything a request may touch. One instance for the whole process; the
/// connection threads share it behind an `Arc`.
pub struct CoreContext {
    pub state: AppState,
    pub runtime: Arc<EngineRuntime>,
    pub proxy: Arc<ProxyGuard>,
    /// One line per minute, kept across restarts. The traffic routes read the
    /// same file the window's chart writes, so both show one history.
    pub traffic: Arc<TrafficLog>,
    /// Set by `POST /shutdown`; the accept loop polls it and then drains.
    shutdown: AtomicBool,
}

impl CoreContext {
    pub fn new(
        state: AppState,
        runtime: Arc<EngineRuntime>,
        proxy: Arc<ProxyGuard>,
        traffic: Arc<TrafficLog>,
    ) -> Self {
        Self {
            state,
            runtime,
            proxy,
            traffic,
            shutdown: AtomicBool::new(false),
        }
    }

    pub fn request_shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
    }

    pub fn is_shutting_down(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }
}

/// Run the core in the foreground. Blocks until `POST /shutdown`, a listen
/// failure, or the process is killed.
///
/// `args` is `std::env::args().skip(1)`; `--port n` is the only flag it reads
/// (0 = let the OS pick, and the chosen port is what the endpoint file says).
pub fn serve(args: &[String]) -> AppResult<()> {
    let port = port_from_args(args)?;
    let base = app_data_dir()?;
    for directory in ["engines", "configs", "logs"] {
        let path = base.join(directory);
        std::fs::create_dir_all(&path)
            .map_err(|error| AppError::Io(format!("无法创建 {}：{error}", path.display())))?;
    }

    // Same order as `lib.rs`'s setup, minus the Tauri handles: state first, then
    // the binaries the store points at, then the service probe, then the guard.
    let state = AppState::load(Store::new(base.join("store.json")));
    let settings: Settings = state.settings.read().clone();
    let runtime = Arc::new(EngineRuntime::new(
        base.join("engines"),
        base.join("configs"),
        base.join("logs"),
    ));
    crate::commands::settings::apply_engine_binaries(&runtime, &settings);
    runtime.refresh_service();

    let proxy = Arc::new(ProxyGuard::new(
        proxy_backend(),
        base.join("proxy-snapshot.json"),
        base.join("x-matrix.pac"),
    ));

    let traffic = Arc::new(TrafficLog::new(base.join("traffic.jsonl")));
    let context = Arc::new(CoreContext::new(state, runtime, proxy, traffic));
    server::serve(&context, port, &base.join("core.endpoint"))
}

/// Read `--port n`; absent means 0, which is the documented way to ask the OS
/// for a free port. Unknown flags are an error rather than ignored, so a typo
/// cannot silently start the core on the wrong port.
fn port_from_args(args: &[String]) -> AppResult<u16> {
    let mut port = 0u16;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--serve" => index += 1,
            "--port" => {
                let Some(value) = args.get(index + 1) else {
                    return Err(AppError::InvalidInput("--port 后面缺少端口号".to_string()));
                };
                port = value.parse::<u16>().map_err(|_| {
                    AppError::InvalidInput(format!("端口必须是 0~65535 的整数：{value:?}"))
                })?;
                index += 2;
            }
            other => {
                return Err(AppError::InvalidInput(format!(
                    "core 不认识这个参数：{other:?}（只支持 --serve 与 --port n）"
                )))
            }
        }
    }
    Ok(port)
}

/// `%APPDATA%\com.xmatrix.client` — the same directory Tauri's `app_data_dir`
/// resolves for identifier `com.xmatrix.client`, without needing Tauri.
pub fn app_data_dir() -> AppResult<PathBuf> {
    let app_data = std::env::var_os("APPDATA").ok_or_else(|| {
        AppError::Internal("环境变量 APPDATA 不存在，无法定位 core 数据目录".to_string())
    })?;
    Ok(Path::new(&app_data).join("com.xmatrix.client"))
}

/// The real Windows registry backend in production; a loud refusal everywhere
/// else, so a cross-platform build still compiles and its `/status` says why
/// the proxy block is unavailable instead of pretending it is off.
fn proxy_backend() -> Box<dyn SystemProxy> {
    #[cfg(windows)]
    {
        Box::new(crate::sysproxy::WindowsProxy::new())
    }
    #[cfg(not(windows))]
    {
        Box::new(UnsupportedProxy)
    }
}

#[cfg(not(windows))]
struct UnsupportedProxy;

#[cfg(not(windows))]
impl SystemProxy for UnsupportedProxy {
    fn read(&self) -> AppResult<crate::sysproxy::ProxyState> {
        Err(AppError::Internal(
            "当前平台没有系统代理实现；仅 Windows 可用".to_string(),
        ))
    }

    fn write(&self, _state: &crate::sysproxy::ProxyState) -> AppResult<()> {
        Err(AppError::Internal(
            "当前平台没有系统代理实现；仅 Windows 可用".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn the_port_defaults_to_os_assigned() {
        assert_eq!(port_from_args(&strings(&["--serve"])).unwrap(), 0);
    }

    #[test]
    fn an_explicit_port_is_read() {
        assert_eq!(
            port_from_args(&strings(&["--serve", "--port", "47100"])).unwrap(),
            47100
        );
    }

    #[test]
    fn a_missing_or_impossible_port_is_a_400_style_input_error() {
        for args in [
            strings(&["--serve", "--port"]),
            strings(&["--serve", "--port", "70000"]),
            strings(&["--serve", "--port", "abc"]),
            strings(&["--serve", "--window"]),
        ] {
            let error = port_from_args(&args).unwrap_err();
            assert!(
                matches!(error, AppError::InvalidInput(_)),
                "{args:?} should be invalid input, got {error:?}"
            );
        }
    }
}