use tauri::AppHandle;

use crate::elevate;
use crate::error::AppResult;

/// Whether this process runs with an administrator token.
///
/// The settings page calls this once on mount: `true` / `false` are answers, so
/// the status line and the restart button can be decided from them.
#[tauri::command]
#[specta::specta]
pub fn app_elevated() -> bool {
    elevate::is_elevated()
}

/// Restart the app with administrator rights.
///
/// Returns only on failure. On success the new process has already started and
/// this one is on its way out through the normal clean-exit path, so the caller
/// should treat a resolved promise as "gone".
#[tauri::command]
#[specta::specta]
pub fn restart_elevated(app: AppHandle) -> AppResult<()> {
    elevate::restart_elevated(&app)
}
