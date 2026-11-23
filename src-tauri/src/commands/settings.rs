use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::autostart::{AutoStart, WindowsAutoStart};
use crate::domain::Settings;
use crate::engine::{EngineKind, EngineRuntime};
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

/// Push the kernel-binary overrides from the settings into the runtime.
///
/// Both the startup path (after the store is loaded, before auto-connect) and
/// `save_settings` call this, so the runtime never starts a kernel the settings
/// file no longer names. An empty value means "use the engines directory" and
/// clears the override instead of becoming a path of its own.
///
/// The path is not checked here: the settings page reports whether the file is
/// there, and a kernel the user moves back into place should start working
/// without re-saving anything.
pub(crate) fn apply_engine_binaries(runtime: &EngineRuntime, settings: &Settings) {
    for (kind, value) in [
        (EngineKind::Xray, &settings.xray_binary),
        (EngineKind::SingBox, &settings.singbox_binary),
    ] {
        let trimmed = value.trim();
        runtime.set_binary(
            kind,
            match trimmed.is_empty() {
                true => None,
                false => Some(PathBuf::from(trimmed)),
            },
        );
    }
}

#[tauri::command]
#[specta::specta]
pub fn get_settings(state: State<'_, AppState>) -> AppResult<Settings> {
    Ok(state.settings.read().clone())
}

#[tauri::command]
#[specta::specta]
pub fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    runtime: State<'_, Arc<EngineRuntime>>,
    settings: Settings,
) -> AppResult<Settings> {
    settings.validate().map_err(AppError::InvalidInput)?;

    // Launch-at-login is a side effect of the setting, not a separate switch:
    // the user flips one thing and the registry follows. Applying it before we
    // persist means a failure leaves the old value in the file too.
    #[cfg(windows)]
    WindowsAutoStart.set(settings.auto_start)?;

    *state.settings.write() = settings.clone();
    state.persist()?;
    // The runtime caches the executable per kernel; a saved path must take
    // effect for the next start without an app restart.
    apply_engine_binaries(&runtime, &settings);
    // The tray lists the node names and checks the current mode, so a saved
    // setting is a reason to rebuild it.
    crate::refresh_tray(&app);
    Ok(settings)
}
