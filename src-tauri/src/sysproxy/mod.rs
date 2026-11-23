//! Taking over (and giving back) the Windows system proxy.
//!
//! The important part is not writing the registry — it is *restoring* it. A
//! proxy client that leaves `ProxyEnable=1` behind after a crash cuts the
//! machine off the network, and one that blindly writes `0` on exit destroys
//! whatever the user had configured before. So:
//!
//! 1. before the first enable, the current values are copied to
//!    `proxy-snapshot.json`;
//! 2. disable restores that snapshot verbatim, and does nothing at all when we
//!    never took the proxy;
//! 3. the snapshot is kept on disk, so a crash between enable and disable is
//!    still recoverable on the next start ([`ProxyGuard::recover`]).
//!
//! Everything goes through the [`SystemProxy`] trait, so the lifecycle logic is
//! tested against a fake backend and never touches a real machine in tests.

use std::path::PathBuf;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::{AppError, AppResult};

/// The three registry values that make up "the system proxy".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ProxyState {
    pub enabled: bool,
    pub server: String,
    pub bypass: String,
}

impl ProxyState {
    /// A clean "no proxy" state. Only tests build it today, but it is the
    /// obvious value for any future "turn the proxy off" action.
    #[allow(dead_code)]
    pub fn off() -> Self {
        Self {
            enabled: false,
            server: String::new(),
            bypass: String::new(),
        }
    }
}

/// What the UI shows on the settings page.
#[derive(Debug, Clone, Serialize, Type)]
pub struct SystemProxyStatus {
    /// True when *we* are the ones holding the system proxy.
    pub active: bool,
    /// True when the system currently routes through some proxy (ours or not).
    pub enabled: bool,
    /// Whatever `ProxyServer` currently says.
    pub server: String,
    /// A leftover snapshot means the previous run died while holding it.
    pub snapshot_present: bool,
}

/// The platform side effect. One implementation per OS.
pub trait SystemProxy: Send + Sync {
    /// What is configured right now.
    fn read(&self) -> AppResult<ProxyState>;
    /// Write the given state and tell the system to reload it.
    fn write(&self, state: &ProxyState) -> AppResult<()>;
}

/// Default bypass list: loopback and local names never go through a proxy.
pub const DEFAULT_BYPASS: &str = "localhost;127.*;10.*;172.16.*;172.17.*;172.18.*;172.19.*;172.20.*;172.21.*;172.22.*;172.23.*;172.24.*;172.25.*;172.26.*;172.27.*;172.28.*;172.29.*;172.30.*;172.31.*;192.168.*;<local>";

/// Owns the snapshot lifecycle around a backend.
pub struct ProxyGuard {
    backend: Box<dyn SystemProxy>,
    snapshot_path: PathBuf,
    active: Mutex<bool>,
}

impl ProxyGuard {
    pub fn new(backend: Box<dyn SystemProxy>, snapshot_path: impl Into<PathBuf>) -> Self {
        Self {
            backend,
            snapshot_path: snapshot_path.into(),
            active: Mutex::new(false),
        }
    }

    /// Whether *we* are the ones currently holding the system proxy.
    pub fn is_active(&self) -> bool {
        *self.active.lock()
    }

    pub fn status(&self) -> AppResult<SystemProxyStatus> {
        let current = self.backend.read()?;
        Ok(SystemProxyStatus {
            active: self.is_active(),
            enabled: current.enabled,
            server: current.server,
            snapshot_present: self.snapshot_path.exists(),
        })
    }

    /// Point the system proxy at `host:port`.
    ///
    /// The snapshot is written *before* the change: if the process dies between
    /// the two writes, the previous state is still on disk.
    /// Point the system proxy at `host:port`, with a user-editable bypass list.
    ///
    /// An empty list falls back to the default instead of writing an empty
    /// `ProxyOverride`: clearing it would send loopback traffic through the
    /// tunnel, which breaks every local service the user runs.
    pub fn enable_with_bypass(&self, host: &str, port: u16, bypass: &str) -> AppResult<()> {
        let bypass = if bypass.trim().is_empty() { DEFAULT_BYPASS } else { bypass.trim() };
        let next = ProxyState {
            enabled: true,
            server: format!("{host}:{port}"),
            bypass: bypass.to_string(),
        };

        if self.is_active() {
            // Already ours: re-assert the values, never re-snapshot (that would
            // overwrite the user's real configuration with our own).
            self.backend.write(&next)?;
            return Ok(());
        }

        let previous = self.backend.read()?;
        self.save_snapshot(&previous)?;
        self.backend.write(&next)?;
        *self.active.lock() = true;
        Ok(())
    }

    /// Give the system proxy back.
    ///
    /// Restores the snapshot when there is one; otherwise does nothing — we did
    /// not take it, so we must not touch it.
    pub fn disable(&self) -> AppResult<()> {
        let Some(previous) = self.load_snapshot() else {
            *self.active.lock() = false;
            return Ok(());
        };
        self.backend.write(&previous)?;
        self.remove_snapshot();
        *self.active.lock() = false;
        Ok(())
    }

    /// Give the proxy back only if we are the ones holding it.
    ///
    /// Called right after an unexpected kernel exit: the tunnel is gone, so the
    /// machine must stop pointing at it instead of waiting for the next launch to
    /// notice. Returns whether anything was actually released.
    pub fn release_if_active(&self) -> AppResult<bool> {
        if !self.is_active() {
            return Ok(false);
        }
        self.disable()?;
        Ok(true)
    }

    /// Called at start: if a snapshot exists, the previous run died while
    /// holding the proxy. Restore it now, so the machine is not left hijacked.
    pub fn recover(&self) -> AppResult<bool> {
        let Some(previous) = self.load_snapshot() else {
            return Ok(false);
        };
        self.backend.write(&previous)?;
        self.remove_snapshot();
        *self.active.lock() = false;
        Ok(true)
    }

    fn save_snapshot(&self, state: &ProxyState) -> AppResult<()> {
        if let Some(parent) = self.snapshot_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(state)
            .map_err(|error| AppError::Internal(error.to_string()))?;
        std::fs::write(&self.snapshot_path, json)?;
        Ok(())
    }

    fn load_snapshot(&self) -> Option<ProxyState> {
        let text = std::fs::read_to_string(&self.snapshot_path).ok()?;
        serde_json::from_str(&text).ok()
    }

    fn remove_snapshot(&self) {
        let _ = std::fs::remove_file(&self.snapshot_path);
    }
}

/// Last-resort safety net: if the app goes away while holding the proxy, the
/// process exit still restores the user's settings.
impl Drop for ProxyGuard {
    fn drop(&mut self) {
        if self.is_active() {
            let _ = self.disable();
        }
    }
}

#[cfg(windows)]
pub use windows_impl::WindowsProxy;

#[cfg(windows)]
mod windows_impl {
    use super::{ProxyState, SystemProxy};
    use crate::error::{AppError, AppResult};

    const KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";

    pub struct WindowsProxy;

    impl WindowsProxy {
        pub fn new() -> Self {
            Self
        }

        fn key() -> AppResult<winreg::RegKey> {
            use winreg::RegKey;
            use winreg::enums::{HKEY_CURRENT_USER, KEY_ALL_ACCESS};

            RegKey::predef(HKEY_CURRENT_USER)
                .open_subkey_with_flags(KEY, KEY_ALL_ACCESS)
                .map_err(|error| AppError::Io(error.to_string()))
        }
    }

    impl Default for WindowsProxy {
        fn default() -> Self {
            Self::new()
        }
    }

    impl SystemProxy for WindowsProxy {
        fn read(&self) -> AppResult<ProxyState> {
            let key = Self::key()?;
            let enabled = key.get_value::<u32, _>("ProxyEnable").unwrap_or(0) != 0;
            let server = key.get_value::<String, _>("ProxyServer").unwrap_or_default();
            let bypass = key
                .get_value::<String, _>("ProxyOverride")
                .unwrap_or_default();
            Ok(ProxyState {
                enabled,
                server,
                bypass,
            })
        }

        fn write(&self, state: &ProxyState) -> AppResult<()> {
            let key = Self::key()?;
            let io = |error: std::io::Error| AppError::Io(error.to_string());

            key.set_value("ProxyEnable", &u32::from(state.enabled))
                .map_err(io)?;
            if state.server.is_empty() {
                let _ = key.delete_value("ProxyServer");
            } else {
                key.set_value("ProxyServer", &state.server).map_err(io)?;
            }
            if state.bypass.is_empty() {
                let _ = key.delete_value("ProxyOverride");
            } else {
                key.set_value("ProxyOverride", &state.bypass).map_err(io)?;
            }

            notify_settings_changed();
            Ok(())
        }
    }

    /// WinINet caches the proxy settings; without these two calls the change
    /// only takes effect for applications started later.
    fn notify_settings_changed() {
        use windows_sys::Win32::Networking::WinInet::{
            INTERNET_OPTION_REFRESH, INTERNET_OPTION_SETTINGS_CHANGED, InternetSetOptionW,
        };
        // SAFETY: both calls pass a null handle and no buffer, which is the
        // documented way to broadcast a settings change.
        unsafe {
            InternetSetOptionW(
                std::ptr::null_mut(),
                INTERNET_OPTION_SETTINGS_CHANGED as _,
                std::ptr::null_mut(),
                0,
            );
            InternetSetOptionW(
                std::ptr::null_mut(),
                INTERNET_OPTION_REFRESH as _,
                std::ptr::null_mut(),
                0,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    struct FakeProxy {
        state: Mutex<ProxyState>,
        writes: Mutex<Vec<ProxyState>>,
    }

    impl FakeProxy {
        fn with(state: ProxyState) -> Self {
            Self {
                state: Mutex::new(state),
                writes: Mutex::new(Vec::new()),
            }
        }

        fn writes(&self) -> Vec<ProxyState> {
            self.writes.lock().clone()
        }
    }

    impl SystemProxy for Arc<FakeProxy> {
        fn read(&self) -> AppResult<ProxyState> {
            Ok(self.state.lock().clone())
        }

        fn write(&self, state: &ProxyState) -> AppResult<()> {
            *self.state.lock() = state.clone();
            self.writes.lock().push(state.clone());
            Ok(())
        }
    }

    /// A path inside the crate's target dir: the system temp dir is not
    /// writable from every shell on this machine.
    fn scratch(name: &str) -> PathBuf {
        static SEQ: AtomicUsize = AtomicUsize::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join("sysproxy");
        std::fs::create_dir_all(&base).unwrap();
        base.join(format!("{name}-{}-{seq}.json", std::process::id()))
    }

    fn custom() -> ProxyState {
        ProxyState {
            enabled: true,
            server: "10.0.0.1:8080".into(),
            bypass: "old".into(),
        }
    }

    #[test]
    fn enabling_snapshots_the_previous_state_first() {
        let before = custom();
        let fake = Arc::new(FakeProxy::with(before.clone()));
        let path = scratch("snapshot");
        let guard = ProxyGuard::new(Box::new(fake.clone()), &path);

        guard.enable_with_bypass("127.0.0.1", 7890, DEFAULT_BYPASS).unwrap();

        let snapshot: ProxyState =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(snapshot, before);
        assert_eq!(fake.state.lock().server, "127.0.0.1:7890");
        assert!(guard.is_active());
        std::mem::forget(guard);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_custom_bypass_list_is_written_as_given() {
        let fake = Arc::new(FakeProxy::with(custom()));
        let path = scratch("bypass");
        let guard = ProxyGuard::new(Box::new(fake.clone()), &path);

        guard
            .enable_with_bypass("127.0.0.1", 7890, "localhost;*.corp.example")
            .unwrap();

        assert_eq!(fake.state.lock().bypass, "localhost;*.corp.example");
        std::mem::forget(guard);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_empty_bypass_list_falls_back_to_the_default() {
        // Writing an empty ProxyOverride would push loopback traffic through the
        // tunnel and break every local service.
        let fake = Arc::new(FakeProxy::with(custom()));
        let path = scratch("bypass-empty");
        let guard = ProxyGuard::new(Box::new(fake.clone()), &path);

        guard.enable_with_bypass("127.0.0.1", 7890, "   ").unwrap();

        assert_eq!(fake.state.lock().bypass, DEFAULT_BYPASS);
        std::mem::forget(guard);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn disabling_restores_what_was_there_before() {
        let before = custom();
        let fake = Arc::new(FakeProxy::with(before.clone()));
        let path = scratch("restore");
        let guard = ProxyGuard::new(Box::new(fake.clone()), &path);

        guard.enable_with_bypass("127.0.0.1", 7890, DEFAULT_BYPASS).unwrap();
        guard.disable().unwrap();

        assert_eq!(*fake.state.lock(), before);
        assert!(!guard.is_active());
        assert!(!path.exists(), "the snapshot must not outlive the restore");
    }

    #[test]
    fn disabling_when_nothing_was_taken_is_a_no_op() {
        // The user's own proxy settings must survive us poking around.
        let before = custom();
        let fake = Arc::new(FakeProxy::with(before.clone()));
        let path = scratch("no-snapshot");
        let _ = std::fs::remove_file(&path);
        let guard = ProxyGuard::new(Box::new(fake.clone()), &path);

        guard.disable().unwrap();

        assert!(fake.writes().is_empty());
        assert_eq!(*fake.state.lock(), before);
    }

    #[test]
    fn releasing_an_active_guard_restores_the_previous_state() {
        // The crash path: the kernel died while we held the proxy, so the only
        // safe move is to put the user's own settings back immediately.
        let before = custom();
        let fake = Arc::new(FakeProxy::with(before.clone()));
        let path = scratch("release-active");
        let guard = ProxyGuard::new(Box::new(fake.clone()), &path);

        guard.enable_with_bypass("127.0.0.1", 7890, DEFAULT_BYPASS).unwrap();
        assert!(guard.is_active());

        assert!(guard.release_if_active().unwrap());
        assert_eq!(*fake.state.lock(), before);
        assert!(!guard.is_active());
        assert!(!path.exists(), "the snapshot must not outlive the release");
    }

    #[test]
    fn releasing_an_inactive_guard_never_writes() {
        // We must not hand back a proxy we never took: doing so would overwrite
        // whatever the user configured on their own.
        let fake = Arc::new(FakeProxy::with(custom()));
        let path = scratch("release-inactive");
        let _ = std::fs::remove_file(&path);
        let guard = ProxyGuard::new(Box::new(fake.clone()), &path);
        let writes_before = fake.writes().len();

        assert!(!guard.release_if_active().unwrap());
        assert_eq!(fake.writes().len(), writes_before);
    }

    #[test]
    fn a_leftover_snapshot_is_restored_on_recovery() {
        // Simulates a crash while the proxy was hijacked.
        let before = ProxyState::off();
        let fake = Arc::new(FakeProxy::with(ProxyState {
            enabled: true,
            server: "127.0.0.1:7890".into(),
            bypass: DEFAULT_BYPASS.into(),
        }));
        let path = scratch("recover");
        std::fs::write(&path, serde_json::to_string(&before).unwrap()).unwrap();

        let guard = ProxyGuard::new(Box::new(fake.clone()), &path);
        assert!(guard.recover().unwrap());

        assert_eq!(*fake.state.lock(), before);
        assert!(!path.exists());
    }

    #[test]
    fn recovery_is_a_no_op_when_nothing_was_left_behind() {
        let fake = Arc::new(FakeProxy::with(ProxyState::off()));
        let path = scratch("nothing");
        let _ = std::fs::remove_file(&path);
        let guard = ProxyGuard::new(Box::new(fake.clone()), &path);

        assert!(!guard.recover().unwrap());
        assert!(fake.writes().is_empty());
    }

    #[test]
    fn enabling_twice_does_not_overwrite_the_snapshot_with_our_own_values() {
        let before = custom();
        let fake = Arc::new(FakeProxy::with(before.clone()));
        let path = scratch("twice");
        let guard = ProxyGuard::new(Box::new(fake.clone()), &path);

        guard.enable_with_bypass("127.0.0.1", 7890, DEFAULT_BYPASS).unwrap();
        guard.enable_with_bypass("127.0.0.1", 7891, DEFAULT_BYPASS).unwrap();

        let snapshot: ProxyState =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            snapshot, before,
            "the snapshot must stay the pre-X-Matrix state"
        );
        assert_eq!(fake.state.lock().server, "127.0.0.1:7891");
        std::mem::forget(guard);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn status_reports_whether_we_hold_it() {
        let fake = Arc::new(FakeProxy::with(ProxyState::off()));
        let path = scratch("status");
        let _ = std::fs::remove_file(&path);
        let guard = ProxyGuard::new(Box::new(fake.clone()), &path);

        let before = guard.status().unwrap();
        assert!(!before.active);
        assert!(!before.enabled);
        assert!(!before.snapshot_present);

        guard.enable_with_bypass("127.0.0.1", 7890, DEFAULT_BYPASS).unwrap();
        let after = guard.status().unwrap();
        assert!(after.active);
        assert!(after.enabled);
        assert_eq!(after.server, "127.0.0.1:7890");
        assert!(after.snapshot_present);
        std::mem::forget(guard);
        let _ = std::fs::remove_file(&path);
    }
}
