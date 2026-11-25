//! Cloud backup: the same document, kept in a WebDAV folder.
//!
//! A local export needs a person with a file dialog; this needs only a running
//! app. The two share one format on purpose — the bytes uploaded are exactly
//! the bytes a local export would contain — so a restore from either source is
//! the same operation.
//!
//! The password is not part of the document: it lives in the Windows Credential
//! Manager (see `syscreds`). Everything else here is a thin wrapper around
//! `net::webdav`, with the two decisions that belong to the app rather than to
//! HTTP: whether the kernel is allowed to be running, and how a status code
//! becomes a sentence.

use std::sync::Arc;

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, State};

use crate::commands::backup::{self, BackupSummary};
use crate::domain::Settings;
use crate::engine::{Endpoints, EngineRuntime};
use crate::error::{AppError, AppResult};
use crate::net::webdav;
use crate::state::AppState;
use crate::syscreds;

/// What the settings page needs to render the WebDAV section.
#[derive(Debug, Serialize, Type)]
pub struct WebdavState {
    pub url: String,
    pub username: String,
    pub via_proxy: bool,
    /// Whether a password is stored. The password itself never crosses the IPC
    /// boundary in either direction: the page only needs to know if one exists,
    /// and "leave it alone" is expressed by not sending a new one.
    pub has_password: bool,
}

/// What an upload actually did, so the page can say it with numbers.
#[derive(Debug, Serialize, Type)]
pub struct WebdavPush {
    pub url: String,
    #[specta(type = f64)]
    pub bytes: u32,
    pub status: u16,
}

fn snapshot(state: &AppState) -> AppResult<WebdavState> {
    let settings = state.settings.read().clone();
    Ok(WebdavState {
        url: settings.webdav_url,
        username: settings.webdav_username,
        via_proxy: settings.webdav_via_proxy,
        has_password: matches!(syscreds::get_secret(syscreds::WEBDAV_TARGET)?, Some(secret) if !secret.is_empty()),
    })
}

/// The port the upload should go through, or `None` for a direct connection.
///
/// "Via proxy" with nothing listening is a refusal, not a silent fallback: a
/// backup the user believes happened is worse than one that said it could not.
fn proxy_port(settings: &Settings, runtime: &EngineRuntime, verb: &str) -> AppResult<Option<u16>> {
    if !settings.webdav_via_proxy {
        return Ok(None);
    }
    let status = runtime.status();
    if !status.running {
        return Err(AppError::Engine(format!(
            "设置了走代理{verb}，但内核没有运行：先在首页启动内核，或者关掉这个开关"
        )));
    }
    // The running kernel's HTTP inbound, resolved the same way the renderer
    // resolves it. `0` only happens if that invariant breaks; the saved pair is
    // still a better guess than port 0.
    Ok(Some(match status.http_port {
        0 => Endpoints::resolve_port(settings.mixed_port, settings.http_port),
        port => port,
    }))
}

/// Turn a status code into something a person can act on.
///
/// `net::webdav` deliberately reports the code instead of deciding: 401 on an
/// upload and 401 on a restore are the same sentence, but 404 means "your URL
/// is wrong" for one and "there is no backup yet" for the other.
fn expect(status: u16, verb: &str) -> AppResult<()> {
    match status {
        200..=299 => Ok(()),
        401 | 403 => Err(AppError::InvalidInput(format!(
            "服务器拒绝认证（HTTP {status}）：检查用户名和密码"
        ))),
        404 if verb == "上传" => Err(AppError::InvalidInput(format!(
            "服务器上说没有这个地址（HTTP 404）：地址要写到文件，或者以 / 结尾"
        ))),
        404 => Err(AppError::InvalidInput(
            "云端还没有备份文件（HTTP 404）：先上传一次，或者检查地址".to_string(),
        )),
        409 => Err(AppError::InvalidInput(
            "服务器上没有这个目录（HTTP 409）：先在 WebDAV 服务器上把目录建好".to_string(),
        )),
        _ => Err(AppError::Engine(format!("服务器返回 HTTP {status}"))),
    }
}

fn resolved_url(settings: &Settings) -> AppResult<String> {
    let url = settings.webdav_url.trim();
    if url.is_empty() {
        return Err(AppError::InvalidInput(
            "先填一个 WebDAV 地址".to_string(),
        ));
    }
    webdav::resolve_target(url)
}

#[tauri::command]
#[specta::specta]
pub fn webdav_state(state: State<'_, AppState>) -> AppResult<WebdavState> {
    snapshot(&state)
}

#[tauri::command]
#[specta::specta]
pub async fn webdav_upload(
    state: State<'_, AppState>,
    runtime: State<'_, Arc<EngineRuntime>>,
) -> AppResult<WebdavPush> {
    let settings = state.settings.read().clone();
    let url = resolved_url(&settings)?;
    let text = backup::collect(&state)?;
    let password = syscreds::get_secret(syscreds::WEBDAV_TARGET)?.unwrap_or_default();
    let proxy = proxy_port(&settings, &runtime, "上传")?;

    let user = settings.webdav_username.clone();
    let bytes = text.len() as u32;
    let body = text.into_bytes();
    let target = url.clone();
    let timeout = webdav::TIMEOUT;
    let status = tauri::async_runtime::spawn_blocking(move || {
        webdav::put_bytes(&target, &user, &password, &body, timeout, proxy)
    })
    .await
    .map_err(|error| AppError::Internal(format!("上传任务没有跑完：{error}")))??;

    expect(status, "上传")?;
    Ok(WebdavPush { url, bytes, status })
}

/// Replace the configuration with the one in the cloud.
///
/// Refused while a kernel runs, for the same reason a file restore is: the
/// kernel is still rendering the document being replaced. The check happens
/// before the download, so a refused restore costs no traffic.
#[tauri::command]
#[specta::specta]
pub async fn webdav_restore(
    app: AppHandle,
    state: State<'_, AppState>,
    runtime: State<'_, Arc<EngineRuntime>>,
) -> AppResult<Option<BackupSummary>> {
    if runtime.status().running {
        return Err(AppError::Engine(
            "先停止内核再恢复配置：正在跑的内核还是按旧配置渲染的".to_string(),
        ));
    }

    let settings = state.settings.read().clone();
    let url = resolved_url(&settings)?;
    let password = syscreds::get_secret(syscreds::WEBDAV_TARGET)?.unwrap_or_default();
    let proxy = proxy_port(&settings, &runtime, "恢复")?;

    let user = settings.webdav_username.clone();
    let timeout = webdav::TIMEOUT;
    let (status, body) = tauri::async_runtime::spawn_blocking(move || {
        webdav::get_bytes(&url, &user, &password, timeout, proxy)
    })
    .await
    .map_err(|error| AppError::Internal(format!("恢复任务没有跑完：{error}")))??;

    let text = if (200..=299).contains(&status) {
        String::from_utf8(body)
            .map_err(|error| AppError::InvalidInput(format!("云端备份不是 UTF-8 文本：{error}")))?
    } else {
        String::new()
    };
    expect(status, "恢复")?;

    let backup = backup::parse_backup(&text)?;
    let summary = backup::apply(&state, backup)?;
    crate::refresh_tray(&app);
    Ok(Some(summary))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_status_code_becomes_a_sentence_that_differs_by_direction() {
        assert!(expect(201, "上传").is_ok());
        assert!(expect(200, "恢复").is_ok());
        // The 404 case is the one that must not be shared: it means different
        // things depending on which way the bytes were going.
        let up = expect(404, "上传").unwrap_err().to_string();
        let down = expect(404, "恢复").unwrap_err().to_string();
        assert!(up.contains("404") && up.contains("地址"));
        assert!(down.contains("404") && down.contains("备份"));
        assert_ne!(up, down);
    }

    #[test]
    fn an_auth_failure_says_where_to_look() {
        for status in [401, 403] {
            let error = expect(status, "上传").unwrap_err().to_string();
            assert!(error.contains("用户名和密码"), "{error}");
        }
    }

    #[test]
    fn an_empty_url_is_refused_before_any_traffic() {
        let settings = Settings::default();
        assert!(matches!(
            resolved_url(&settings),
            Err(AppError::InvalidInput(_))
        ));
    }

    #[test]
    fn a_url_without_a_file_name_gains_one() {
        let settings = Settings {
            webdav_url: "https://dav.example.com/backups/".into(),
            ..Default::default()
        };
        assert_eq!(
            resolved_url(&settings).unwrap(),
            format!("https://dav.example.com/backups/{}", webdav::BACKUP_FILE_NAME)
        );
    }

    #[test]
    fn a_plain_http_url_keeps_its_own_file_name() {
        let settings = Settings {
            webdav_url: "http://127.0.0.1:8099/dav/my-backup.json".into(),
            ..Default::default()
        };
        assert_eq!(
            resolved_url(&settings).unwrap(),
            "http://127.0.0.1:8099/dav/my-backup.json"
        );
    }
}