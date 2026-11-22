use tauri::State;

use crate::autostart::{AutoStart, WindowsAutoStart};
use crate::domain::Settings;
use crate::error::{AppError, AppResult};
use crate::state::AppState;

/// Whether the launch-at-login entry is currently registered.
///
/// Read from the registry, not from the saved settings: the user (or an
/// uninstaller) can change it outside the app, and the switch should not lie.
#[tauri::command]
#[specta::specta]
pub fn autostart_status() -> AppResult<bool> {
    #[cfg(windows)]
    {
        WindowsAutoStart.enabled()
    }
    #[cfg(not(windows))]
    {
        Ok(false)
    }
}

#[tauri::command]
#[specta::specta]
pub fn get_settings(state: State<'_, AppState>) -> AppResult<Settings> {
    Ok(state.settings.read().clone())
}

#[tauri::command]
#[specta::specta]
pub fn save_settings(state: State<'_, AppState>, settings: Settings) -> AppResult<Settings> {
    settings.validate().map_err(AppError::InvalidInput)?;

    // Launch-at-login is a side effect of the setting, not a separate switch:
    // the user flips one thing and the registry follows. Applying it before we
    // persist means a failure leaves the old value in the file too.
    #[cfg(windows)]
    WindowsAutoStart.set(settings.auto_start)?;

    *state.settings.write() = settings.clone();
    state.persist()?;
    Ok(settings)
}
