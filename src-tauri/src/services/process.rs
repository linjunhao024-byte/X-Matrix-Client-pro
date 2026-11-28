//! The process list behind the rule editor's picker.
//!
//! Enumeration lives in [`crate::sysproc`] (the OS half) so this file is only
//! the contract: the WebView never sees a pid list it has to interpret, only
//! names and their counts. The Tauri command is a thin wrapper over
//! [`list_processes`].

use crate::error::AppResult;
use crate::services::EmptyArgs;
use crate::sysproc::{self, ProcessInfo};

/// Every running process, merged by executable name.
///
/// Takes the empty argument struct so it can be wired into the routing table
/// like any other command: `POST /process/list_processes` with no body.
pub fn list_processes(_args: EmptyArgs) -> AppResult<Vec<ProcessInfo>> {
    sysproc::list()
}