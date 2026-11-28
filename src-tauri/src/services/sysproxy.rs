//! System proxy business, transport-free.
//!
//! Deliberately narrow: the UI can *read* the state and *give back* what we
//! took, but it cannot seize the system proxy on a bare button press. Taking it
//! is bound to "a kernel is running *and* the saved setting says so" in
//! [`crate::services::engine::start_engine`] and in `save_settings`.
//!
//! These two take the guard directly instead of a [`crate::services::Ctx`]:
//! they are the only commands in this batch with no engine runtime in their
//! signature, and inventing one just to satisfy a bundle would be a lie.

use std::sync::Arc;

use crate::error::AppResult;
use crate::services::EmptyArgs;
use crate::sysproxy::{ProxyGuard, SystemProxyStatus};

/// What the system proxy looks like right now, and whether we are holding it.
pub fn system_proxy_status(
    guard: &Arc<ProxyGuard>,
    _args: EmptyArgs,
) -> AppResult<SystemProxyStatus> {
    guard.status()
}

/// Restore whatever was configured before X-Matrix took over.
///
/// Safe to call at any time: it does nothing unless a snapshot says we took it.
pub fn system_proxy_disable(
    guard: &Arc<ProxyGuard>,
    _args: EmptyArgs,
) -> AppResult<SystemProxyStatus> {
    guard.disable()?;
    guard.status()
}