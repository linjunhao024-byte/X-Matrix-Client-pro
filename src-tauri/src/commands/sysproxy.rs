//! System proxy commands.
//!
//! Deliberately narrow: the UI can *read* the state and *give back* what we
//! took, but it cannot seize the system proxy on a button press. Taking it is
//! bound to starting a kernel with "system proxy" enabled in settings — the
//! same contract v2rayN and Clash Verge use, and the one place where the
//! ownership question has an obvious answer.

use std::sync::Arc;

use tauri::State;

use crate::error::AppResult;
use crate::sysproxy::{ProxyGuard, SystemProxyStatus};

type Guard<'a> = State<'a, Arc<ProxyGuard>>;

/// What the system proxy looks like right now, and whether we are holding it.
#[tauri::command]
#[specta::specta]
pub fn system_proxy_status(guard: Guard<'_>) -> AppResult<SystemProxyStatus> {
    guard.status()
}

/// Restore whatever was configured before X-Matrix took over.
///
/// Safe to call at any time: it does nothing unless a snapshot says we took it.
#[tauri::command]
#[specta::specta]
pub fn system_proxy_disable(guard: Guard<'_>) -> AppResult<SystemProxyStatus> {
    guard.disable()?;
    guard.status()
}
