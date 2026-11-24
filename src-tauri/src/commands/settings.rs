use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::autostart::{AutoStart, WindowsAutoStart};
use crate::commands::engine::Proxy;
use crate::domain::settings::Language;
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

/// What saving the settings means for a proxy we may already be holding.
#[derive(Debug, PartialEq, Eq)]
enum Takeover {
    /// Neither proxy field changed: the machine already matches the document.
    None,
    /// Point the system proxy at the running kernel (first take, or re-assert
    /// with an edited bypass list or a switched PAC mode).
    Take,
    /// Give the system proxy back: the user just turned the takeover off.
    Release,
}

/// Compare the saved settings before and after a save.
///
/// Only the two proxy fields are read: everything else a save changes is baked
/// into the config of the *next* start anyway. Callers check that a kernel is
/// running first — with nothing listening, there is nothing to take over or
/// give back.
fn takeover_change(before: &Settings, after: &Settings) -> Takeover {
    if before.system_proxy != after.system_proxy {
        return match after.system_proxy {
            true => Takeover::Take,
            false => Takeover::Release,
        };
    }
    // Still on, but the bypass list or the PAC switch moved: both change what
    // the registry must say, so only re-asserting makes the edit real.
    if after.system_proxy
        && (before.proxy_bypass != after.proxy_bypass || before.pac_enabled != after.pac_enabled)
    {
        return Takeover::Take;
    }
    Takeover::None
}

#[tauri::command]
#[specta::specta]
pub fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    runtime: State<'_, Arc<EngineRuntime>>,
    proxy: Proxy<'_>,
    settings: Settings,
) -> AppResult<Settings> {
    settings.validate().map_err(AppError::InvalidInput)?;

    // The proxy we may be holding was taken over from the old values, so the
    // decision below needs the document as it stood before this save.
    let before = state.settings.read().clone();

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

    // The system proxy is the one setting that points at a process, so it only
    // has a machine side effect while that process runs. Turning the takeover
    // off is a safety action: it must release the machine now, not at the next
    // start, or the user saves "off" and stays hijacked.
    let status = runtime.status();
    if status.running {
        match takeover_change(&before, &settings) {
            Takeover::Take => {
                // The *running* kernel's port, never the saved one: a port
                // edited in the same save only takes effect on the next start,
                // and pointing the registry at it now would send every request
                // into a port nothing listens on.
                let port = match status.mixed_port {
                    0 => settings.mixed_port,
                    port => port,
                };
                // The settings are already saved when this fails: the error
                // tells the user why the machine state did not follow, instead
                // of silently rolling their change back.
                if settings.pac_enabled {
                    // Rendered from the rules as they are saved now: a rule
                    // edited in this same save is part of the new script.
                    let script = crate::sysproxy::pac::render(
                        port,
                        &settings.proxy_bypass,
                        &crate::sysproxy::pac::direct_patterns(&state.rules.read()),
                    );
                    proxy.enable_pac(port, &settings.proxy_bypass, &script)?;
                } else {
                    proxy.enable_with_bypass("127.0.0.1", port, &settings.proxy_bypass)?;
                }
            }
            Takeover::Release => proxy.disable()?,
            Takeover::None => {}
        }
    }

    // A chord is a machine-side registration, like the proxy key: saving a new
    // one has to take effect now, and saving an empty one has to release the old
    // combination, or the page would claim a shortcut the desktop no longer has.
    crate::hotkeys::apply(
        crate::hotkeys::parse_optional(&settings.hotkey_toggle).ok().flatten(),
        crate::hotkeys::parse_optional(&settings.hotkey_window).ok().flatten(),
    );

    // The tray lists the node names and checks the current mode, so a saved
    // setting is a reason to rebuild it.
    crate::refresh_tray(&app);
    Ok(settings)
}

/// Change only the language.
///
/// The settings page applies a language the moment it is clicked, so it cannot
/// route through `save_settings`: that command takes the whole document, and
/// sending a form that still holds unsaved edits (a port half typed, say) would
/// save those too. This writes the one field and rebuilds the tray, whose menu
/// wording follows it.
#[tauri::command]
#[specta::specta]
pub fn set_language(
    app: AppHandle,
    state: State<'_, AppState>,
    language: Language,
) -> AppResult<Language> {
    {
        let mut settings = state.settings.write();
        if settings.language != language {
            settings.language = language;
        }
    }
    state.persist()?;
    crate::refresh_tray(&app);
    Ok(language)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Settings differing from the defaults only in the two proxy fields.
    fn proxy_settings(system_proxy: bool, bypass: &str) -> Settings {
        Settings {
            system_proxy,
            proxy_bypass: bypass.to_string(),
            ..Settings::default()
        }
    }

    #[test]
    fn unchanged_proxy_settings_are_none() {
        let before = proxy_settings(true, "localhost");
        let after = before.clone();
        assert_eq!(takeover_change(&before, &after), Takeover::None);
    }

    #[test]
    fn turning_system_proxy_on_takes() {
        let before = proxy_settings(false, "localhost");
        let after = proxy_settings(true, "localhost");
        assert_eq!(takeover_change(&before, &after), Takeover::Take);
    }

    #[test]
    fn turning_system_proxy_off_releases() {
        let before = proxy_settings(true, "localhost");
        let after = proxy_settings(false, "localhost");
        assert_eq!(takeover_change(&before, &after), Takeover::Release);
    }

    #[test]
    fn editing_bypass_while_on_takes_again() {
        let before = proxy_settings(true, "localhost");
        let after = proxy_settings(true, "localhost;*.corp.example");
        assert_eq!(takeover_change(&before, &after), Takeover::Take);
    }

    #[test]
    fn editing_bypass_while_off_does_nothing() {
        let before = proxy_settings(false, "localhost");
        let after = proxy_settings(false, "localhost;*.corp.example");
        assert_eq!(takeover_change(&before, &after), Takeover::None);
    }

    #[test]
    fn switching_off_and_editing_bypass_releases() {
        // Both fields moved at once: giving the proxy back wins, because the
        // takeover is off and nothing should be re-asserted.
        let before = proxy_settings(true, "localhost");
        let after = proxy_settings(false, "localhost;*.corp.example");
        assert_eq!(takeover_change(&before, &after), Takeover::Release);
    }
}
