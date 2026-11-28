//! The process list behind the rule editor's picker.
//!
//! Enumeration lives in `sysproc` (the OS half) and the shared body in
//! [`crate::services::process`], so this file is only the contract: the WebView
//! never sees a pid list it has to interpret, only names and their counts.

use crate::error::AppResult;
use crate::services::{process as svc, EmptyArgs};
use crate::sysproc::ProcessInfo;

/// Every running process, merged by executable name.
///
/// Async so the snapshot walk is dispatched on the async runtime instead of
/// blocking the thread that would otherwise deliver the next command.
#[tauri::command]
#[specta::specta]
pub async fn list_processes() -> AppResult<Vec<ProcessInfo>> {
    // The walk itself is synchronous but cheap enough to stay inline; the
    // command is `async` because the IPC layer expects the same shape it had
    // when this was a blocking call on the WebView thread.
    svc::list_processes(EmptyArgs {})
}