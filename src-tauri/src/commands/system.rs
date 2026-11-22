use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Manager, WebviewWindow};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::engine::EngineRuntime;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Type)]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub platform: String,
    pub arch: String,
}

/// Where the app keeps its things. Shown on the About page so a user can find
/// their config without guessing at `%APPDATA%`.
#[derive(Debug, Clone, Serialize, Type)]
pub struct AppPaths {
    pub data_dir: String,
    pub engines_dir: String,
    pub config_dir: String,
    pub log_dir: String,
}

/// Cheap liveness probe for the contract layer: if this renders, the whole
/// Rust -> bindings.ts -> Vue chain is wired correctly.
#[tauri::command]
#[specta::specta]
pub fn app_info() -> AppResult<AppInfo> {
    Ok(AppInfo {
        name: "X-Matrix".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        platform: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
    })
}

#[tauri::command]
#[specta::specta]
pub fn app_paths(app: AppHandle, runtime: tauri::State<'_, std::sync::Arc<EngineRuntime>>) -> AppResult<AppPaths> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| AppError::Internal(format!("找不到数据目录：{error}")))?;
    Ok(AppPaths {
        data_dir: data_dir.display().to_string(),
        engines_dir: runtime.engines_dir().display().to_string(),
        config_dir: runtime.config_dir().display().to_string(),
        log_dir: runtime.log_dir().display().to_string(),
    })
}

/// Opens the data directory (the folder that holds `store.json`).
#[tauri::command]
#[specta::specta]
pub fn app_open_data_dir(app: AppHandle) -> AppResult<()> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| AppError::Internal(format!("找不到数据目录：{error}")))?;
    app.opener()
        .open_path(data_dir.display().to_string(), None::<&str>)
        .map_err(|error| AppError::Internal(format!("打不开目录：{error}")))
}

/// Opens the log directory (where `xray.log` / `sing-box.log` live). The log
/// dialog gets a button for it: "the kernel says something went wrong" is
/// exactly the moment you want the whole file, not the last 200 lines.
#[tauri::command]
#[specta::specta]
pub fn app_open_log_dir(app: AppHandle, runtime: tauri::State<'_, std::sync::Arc<EngineRuntime>>) -> AppResult<()> {
    let log_dir = runtime.log_dir().display().to_string();
    app.opener()
        .open_path(log_dir, None::<&str>)
        .map_err(|error| AppError::Internal(format!("打不开日志目录：{error}")))
}

/// Save arbitrary text the UI produced (a CSV of connections, a list of share
/// links) through the native dialog.
///
/// Kept generic on purpose: the alternative is one command per file format,
/// and every one of them would repeat the same "ask for a path, write it,
/// report a cancelled dialog as `None`" dance.
#[tauri::command]
#[specta::specta]
pub async fn save_text_file(app: AppHandle, default_name: String, contents: String) -> AppResult<Option<String>> {
    let name = default_name.trim();
    if name.is_empty() {
        return Err(AppError::InvalidInput("文件名不能为空".into()));
    }
    // The extension decides the filter, and an unknown one just means "no
    // filter" rather than an error: the user picked the name, not us.
    let extension = name.rsplit_once('.').map(|(_, ext)| ext.to_string());
    let handle = app.clone();
    let name = name.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let mut dialog = handle.dialog().file().set_title("保存文件").set_file_name(name);
        if let Some(extension) = extension.as_deref() {
            dialog = dialog.add_filter(extension.to_uppercase(), &[extension]);
        }
        let Some(path) = dialog.blocking_save_file() else {
            return Ok(None);
        };
        let path = path
            .into_path()
            .map_err(|error| AppError::InvalidInput(format!("所选路径不可用：{error}")))?;
        std::fs::write(&path, contents)?;
        Ok(Some(path.display().to_string()))
    })
    .await
    .map_err(|error| AppError::Internal(format!("保存任务没有跑完：{error}")))?
}

/// Ask for a text file and return its contents.
///
/// Subscription files, Clash configs and exported link lists are all just text,
/// so the importer can stay one code path: read the file, hand the string to
/// `import_preview`, done. The size cap keeps a mis-click on a 2 GB log from
/// freezing the window.
const MAX_IMPORT_BYTES: u64 = 4 * 1024 * 1024;

#[tauri::command]
#[specta::specta]
pub async fn open_text_file(app: AppHandle) -> AppResult<Option<String>> {
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let picked = handle
            .dialog()
            .file()
            .add_filter("文本文件", &["txt", "json", "yaml", "yml", "conf", "list"])
            .blocking_pick_file();
        let Some(path) = picked else {
            return Ok(None);
        };
        let path = path
            .into_path()
            .map_err(|error| AppError::InvalidInput(format!("这个路径读不出来：{error}")))?;
        let size = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
        if size > MAX_IMPORT_BYTES {
            return Err(AppError::InvalidInput(format!(
                "文件有 {} MB，超过 4 MB 上限；这么大的东西不是订阅",
                size / 1024 / 1024
            )));
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|error| AppError::InvalidInput(format!("读不了这个文件：{error}")))?;
        Ok(Some(text))
    })
    .await
    .map_err(|error| AppError::Internal(format!("读取任务没有跑完：{error}")))?
}

/// The title bar is drawn by the OS, so CSS cannot theme it: a dark app with a
/// white frame looks like a bug. The UI calls this whenever the theme resolves.
#[tauri::command]
#[specta::specta]
pub fn ui_set_window_theme(window: WebviewWindow, dark: bool) -> AppResult<()> {
    let theme = if dark {
        tauri::Theme::Dark
    } else {
        tauri::Theme::Light
    };
    window
        .set_theme(Some(theme))
        .map_err(|error| AppError::Engine(error.to_string()))
}
