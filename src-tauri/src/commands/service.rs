//! Service installation commands (S4b).
//!
//! Three commands, no more: read the state, install, uninstall. Installing is
//! the only place the service's engines directory is decided — it is written to
//! `HKLM` while we hold an elevated token, and the service re-reads it for every
//! job, which is what keeps a normal user from pointing SYSTEM at an arbitrary
//! executable.

use std::sync::Arc;

use serde::Serialize;
use specta::Type;
use tauri::State;

use crate::engine::EngineRuntime;
use crate::error::{AppError, AppResult};
use crate::service_client::ServiceStatus;

/// What the settings page shows. The four fields are pinned by the task book;
/// the pid already lives in `detail` where it belongs (one sentence, not a
/// separate value the UI has to format).
#[derive(Debug, Clone, Serialize, Type)]
pub struct ServiceState {
    pub installed: bool,
    pub running: bool,
    pub engines_dir: Option<String>,
    pub detail: String,
}

impl From<ServiceStatus> for ServiceState {
    fn from(status: ServiceStatus) -> Self {
        let engines_dir = crate::service::read_engines_dir()
            .ok()
            .map(|dir| dir.to_string_lossy().into_owned());
        Self {
            installed: status.installed,
            running: status.running,
            engines_dir,
            detail: status.detail,
        }
    }
}

#[tauri::command]
#[specta::specta]
pub fn service_state() -> ServiceState {
    ServiceState::from(crate::service_client::status())
}

#[tauri::command]
#[specta::specta]
pub fn service_install(runtime: State<'_, Arc<EngineRuntime>>) -> AppResult<ServiceState> {
    if !crate::elevate::is_elevated() {
        return Err(AppError::Engine(
            "安装服务需要管理员权限：先在「权限」里点「以管理员身份重启」".to_string(),
        ));
    }
    let engines_dir = runtime.engines_dir().to_path_buf();
    let warning = crate::service::install(&engines_dir)?;
    runtime.refresh_service();
    let mut state = ServiceState::from(crate::service_client::status());
    // The service is installed and running at this point, so the warning rides
    // along with that sentence instead of replacing it.
    if let Some(warning) = warning {
        state.detail = format!("{}（{warning}）", state.detail);
    }
    Ok(state)
}

#[tauri::command]
#[specta::specta]
pub fn service_uninstall(runtime: State<'_, Arc<EngineRuntime>>) -> AppResult<ServiceState> {
    if !crate::elevate::is_elevated() {
        return Err(AppError::Engine(
            "卸载服务需要管理员权限：先在「权限」里点「以管理员身份重启」".to_string(),
        ));
    }
    crate::service::uninstall()?;
    runtime.refresh_service();
    Ok(ServiceState::from(crate::service_client::status()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_state_without_a_service_says_so() {
        let state = ServiceState::from(ServiceStatus {
            installed: false,
            running: false,
            pid: None,
            detail: "没有安装".to_string(),
        });
        assert!(!state.installed && !state.running);
        assert!(state.engines_dir.is_none());
        assert_eq!(state.detail, "没有安装");
    }

    #[test]
    fn a_running_state_keeps_the_status_sentence() {
        let state = ServiceState::from(ServiceStatus {
            installed: true,
            running: true,
            pid: Some(4242),
            detail: "已安装，正在运行，PID 4242".to_string(),
        });
        assert!(state.installed && state.running);
        assert_eq!(state.detail, "已安装，正在运行，PID 4242");
    }
}