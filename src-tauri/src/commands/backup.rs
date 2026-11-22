//! Configuration backup and restore.
//!
//! The durable state is one JSON document plus one file per rule set
//! (ARCHITECTURE.md §3), so a backup carries exactly those two things. Kernels,
//! logs and window positions are not configuration: a backup that quietly
//! shipped a 35 MB kernel binary would be a surprise, not a feature.
//!
//! Restore is a replace, never a merge. Merging two configuration sets produces
//! duplicate node ids and rules pointing at groups that only exist in one of
//! them — the kind of damage that only shows up days later.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::engine::EngineRuntime;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::store::Document;

/// Marker written into every backup, so a random JSON file is rejected with a
/// sentence the user can act on instead of a serde error.
const BACKUP_KIND: &str = "x-matrix-backup";

#[derive(Debug, Serialize, Deserialize)]
struct Backup {
    kind: String,
    document: Document,
    /// Rule-set id → raw list text. Remote sets could be fetched again, local
    /// ones cannot, so both travel with the backup.
    #[serde(default)]
    rule_sets: BTreeMap<String, String>,
}

/// What a restore actually brought back, so the UI can confirm it.
#[derive(Debug, Serialize, Type)]
pub struct BackupSummary {
    // u32 rather than usize: specta renders usize as `number | null`, and a
    // count that the UI has to null-check reads worse than it needs to.
    #[specta(type = f64)]
    pub nodes: u32,
    #[specta(type = f64)]
    pub subscriptions: u32,
    #[specta(type = f64)]
    pub rules: u32,
    #[specta(type = f64)]
    pub groups: u32,
    #[specta(type = f64)]
    pub rule_sets: u32,
}

fn stamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

/// Read and vet a backup document. Kept separate from the dialog so the rules
/// that matter (marker, node validity) are testable without a file picker.
fn parse_backup(text: &str) -> AppResult<Backup> {
    let backup: Backup = serde_json::from_str(text)
        .map_err(|error| AppError::InvalidInput(format!("这个文件不是配置备份：{error}")))?;
    if backup.kind != BACKUP_KIND {
        return Err(AppError::InvalidInput(
            "这个文件不是 X-Matrix 的配置备份".to_string(),
        ));
    }
    // Validate before anything is replaced: a half-applied restore is worse
    // than a rejected one.
    for node in &backup.document.nodes {
        node.validate()?;
    }
    Ok(backup)
}

/// Write the whole configuration to a file the user picks.
///
/// Returns the path, or `None` when the dialog was cancelled — cancelling is
/// not an error and must not paint a red banner.
#[tauri::command]
#[specta::specta]
pub async fn backup_save(app: AppHandle, state: State<'_, AppState>) -> AppResult<Option<String>> {
    let document = state.document();

    let mut rule_sets = BTreeMap::new();
    for set in &document.rule_sets {
        let text = state.rule_set_files.read(set)?;
        if !text.trim().is_empty() {
            rule_sets.insert(set.id.clone(), text);
        }
    }

    let backup = Backup {
        kind: BACKUP_KIND.to_string(),
        document,
        rule_sets,
    };
    let text = serde_json::to_string_pretty(&backup)
        .map_err(|error| AppError::Internal(format!("配置无法序列化：{error}")))?;

    let handle = app.clone();
    let name = format!("x-matrix-{}.json", stamp());
    tauri::async_runtime::spawn_blocking(move || {
        let picked = handle
            .dialog()
            .file()
            .set_title("导出配置")
            .set_file_name(name)
            .add_filter("X-Matrix 配置", &["json"])
            .blocking_save_file();

        let Some(path) = picked else {
            return Ok(None);
        };
        let path = path
            .into_path()
            .map_err(|error| AppError::InvalidInput(format!("所选路径不可用：{error}")))?;
        std::fs::write(&path, text)?;
        Ok(Some(path.display().to_string()))
    })
    .await
    .map_err(|error| AppError::Internal(format!("导出任务没有跑完：{error}")))?
}

/// Replace the whole configuration with the one in a file the user picks.
#[tauri::command]
#[specta::specta]
pub async fn backup_load(
    app: AppHandle,
    state: State<'_, AppState>,
    runtime: State<'_, Arc<EngineRuntime>>,
) -> AppResult<Option<BackupSummary>> {
    if runtime.status().running {
        return Err(AppError::Engine(
            "先停止内核再恢复配置：正在跑的内核还是按旧配置渲染的".to_string(),
        ));
    }

    let handle = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        handle
            .dialog()
            .file()
            .set_title("选择配置备份")
            .add_filter("X-Matrix 配置", &["json"])
            .blocking_pick_file()
    })
    .await
    .map_err(|error| AppError::Internal(format!("选择文件没有跑完：{error}")))?;

    let Some(path) = picked else {
        return Ok(None);
    };
    let path = path
        .into_path()
        .map_err(|error| AppError::InvalidInput(format!("所选路径不可用：{error}")))?;

    let text = std::fs::read_to_string(&path)?;
    let backup = parse_backup(&text)?;

    // Lists first, then the document: if a list fails to land, the state still
    // points at the old sets instead of at files that were never written.
    for set in &backup.document.rule_sets {
        if let Some(body) = backup.rule_sets.get(&set.id) {
            state.rule_set_files.write(set, body)?;
        }
    }

    let summary = BackupSummary {
        nodes: backup.document.nodes.len() as u32,
        subscriptions: backup.document.subscriptions.len() as u32,
        rules: backup.document.rules.len() as u32,
        groups: backup.document.groups.len() as u32,
        rule_sets: backup.document.rule_sets.len() as u32,
    };

    state.replace(backup.document)?;
    crate::refresh_tray(&app);
    Ok(Some(summary))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::node::{Node, Protocol, Security, Transport};

    fn node() -> Node {
        Node {
            id: "n1".into(),
            name: "东京".into(),
            server: "example.com".into(),
            port: 443,
            protocol: Protocol::Trojan {
                password: "pw".into(),
            },
            transport: Transport::Tcp,
            security: Security::None,
            tags: vec![],
        }
    }

    fn backup_with(document: Document) -> String {
        let backup = Backup {
            kind: BACKUP_KIND.to_string(),
            document,
            rule_sets: BTreeMap::new(),
        };
        serde_json::to_string(&backup).unwrap()
    }

    #[test]
    fn a_backup_round_trips_through_the_parser() {
        let mut document = Document::default();
        document.nodes.push(node());
        document.settings.mixed_port = 7999;

        let parsed = parse_backup(&backup_with(document)).unwrap();
        assert_eq!(parsed.document.nodes.len(), 1);
        assert_eq!(parsed.document.nodes[0].name, "东京");
        assert_eq!(parsed.document.settings.mixed_port, 7999);
    }

    #[test]
    fn a_foreign_json_file_is_rejected_with_a_sentence() {
        let mut document = Document::default();
        document.nodes.push(node());
        let text = backup_with(document).replace(BACKUP_KIND, "something-else");
        let error = parse_backup(&text).unwrap_err();
        assert!(matches!(error, AppError::InvalidInput(_)));
    }

    #[test]
    fn a_backup_with_a_broken_node_is_refused_before_anything_is_replaced() {
        let mut document = Document::default();
        let mut broken = node();
        broken.name = String::new();
        document.nodes.push(broken);

        assert!(parse_backup(&backup_with(document)).is_err());
    }

    #[test]
    fn plain_garbage_is_not_mistaken_for_a_backup() {
        assert!(parse_backup("not json at all").is_err());
    }
}
