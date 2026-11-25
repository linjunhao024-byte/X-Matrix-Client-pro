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
use crate::store::{Document, StoredProfile};

/// Marker written into every backup, so a random JSON file is rejected with a
/// sentence the user can act on instead of a serde error.
const BACKUP_KIND: &str = "x-matrix-backup";

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Backup {
    kind: String,
    document: Document,
    /// Rule-set id → raw list text. Remote sets could be fetched again, local
    /// ones cannot, so both travel with the backup.
    #[serde(default)]
    rule_sets: BTreeMap<String, String>,
    /// The profiles that are not active. The active one is inside `document`
    /// already, and files for it do not exist on disk. Older backups have no
    /// such key and still restore, as the single「默认」profile.
    #[serde(default)]
    profiles: Vec<StoredProfile>,
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
    #[specta(type = f64)]
    pub profiles: u32,
}

fn stamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

/// Render the whole configuration as the JSON text a backup file holds.
///
/// Shared with the WebDAV upload, so the bytes sent to a server are exactly the
/// bytes a local export would contain - one backup format, not two.
pub(crate) fn collect(state: &AppState) -> AppResult<String> {
    let document = state.document();

    let mut rule_sets = BTreeMap::new();
    for set in &document.rule_sets {
        let text = state.rule_set_files.read(set)?;
        if !text.trim().is_empty() {
            rule_sets.insert(set.id.clone(), text);
        }
    }

    // Every profile, not just the active one: a backup that silently dropped
    // the profiles the user was not currently using would be a data-loss bug
    // wearing an export button. A missing file is refused here instead of
    // shipped as an empty profile, because the difference only shows up on the
    // day the user restores.
    let mut profiles = Vec::new();
    for meta in document
        .profiles
        .iter()
        .filter(|meta| meta.id != document.active)
    {
        let parts = state.profile_parts(&meta.id)?.ok_or_else(|| {
            AppError::InvalidInput(format!(
                "配置「{}」的文件不见了，导出会少一份配置；先删掉它再导出",
                meta.name
            ))
        })?;
        profiles.push(StoredProfile {
            id: meta.id.clone(),
            name: meta.name.clone(),
            parts,
        });
    }

    let backup = Backup {
        kind: BACKUP_KIND.to_string(),
        document,
        rule_sets,
        profiles,
    };
    serde_json::to_string_pretty(&backup)
        .map_err(|error| AppError::Internal(format!("配置无法序列化：{error}")))
}

/// Put a vetted backup into the live state.
///
/// Lists first, then the document: if a list fails to land, the state still
/// points at the old sets instead of at files that were never written. Shared
/// with the WebDAV restore, which must replace exactly what a file restore does.
pub(crate) fn apply(state: &AppState, backup: Backup) -> AppResult<BackupSummary> {
    for set in &backup.document.rule_sets {
        if let Some(body) = backup.rule_sets.get(&set.id) {
            state.rule_set_files.write(set, body)?;
        }
    }

    // A backup written before profiles existed has neither a profile list nor
    // an active id. Normalising here makes it the single「默认」profile, so an
    // old file restores into a usable app instead of a profile-less state.
    let mut document = backup.document;
    crate::store::normalize(&mut document);

    // Profile files before the document, for the same reason as rule sets: a
    // state pointing at files that were never written is the shape of data loss.
    for profile in &backup.profiles {
        state.save_profile_parts(&profile.id, &profile.name, &profile.parts)?;
    }

    let summary = BackupSummary {
        nodes: document.nodes.len() as u32,
        subscriptions: document.subscriptions.len() as u32,
        rules: document.rules.len() as u32,
        groups: document.groups.len() as u32,
        rule_sets: document.rule_sets.len() as u32,
        profiles: document.profiles.len() as u32,
    };

    // Files of profiles that the backup does not contain would linger and be
    // invisible in the list; a restore is a replace, so they go. Done after the
    // replace so a failure leaves the old profiles readable.
    let previous: Vec<String> = state.profiles.read().iter().map(|meta| meta.id.clone()).collect();
    let kept: Vec<String> = document.profiles.iter().map(|meta| meta.id.clone()).collect();

    state.replace(document)?;

    for id in previous {
        if !kept.contains(&id) {
            if let Err(error) = state.drop_profile_file(&id) {
                eprintln!("x-matrix: 恢复配置后删不掉旧配置文件 {id}：{error}");
            }
        }
    }
    Ok(summary)
}

/// Read and vet a backup document. Kept separate from the dialog so the rules
/// that matter (marker, node validity) are testable without a file picker.
pub(crate) fn parse_backup(text: &str) -> AppResult<Backup> {
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
    let text = collect(&state)?;

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

    let summary = apply(&state, backup)?;
    crate::refresh_tray(&app);
    Ok(Some(summary))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::node::{Node, Protocol, Security, Transport};
    use crate::store::{ProfileMeta, ProfileParts, Store};

    fn scratch() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static SEQ: AtomicUsize = AtomicUsize::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("backup-{}-{}", std::process::id(), seq));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn state_at(dir: &std::path::Path) -> AppState {
        AppState::load(Store::new(dir.join("store.json")))
    }

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
            profiles: Vec::new(),
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

    #[test]
    fn an_export_carries_every_profile_not_just_the_active_one() {
        let dir = scratch();
        let state = state_at(&dir);
        state.nodes.write().push(node());
        state.profiles.write().push(ProfileMeta {
            id: "p-work".into(),
            name: "公司".into(),
        });
        state.persist().unwrap();

        // The non-active profile has its own file; the active one only lives
        // inline.
        let parts = ProfileParts {
            nodes: vec![],
            ..ProfileParts::default()
        };
        state.save_profile_parts("p-work", "公司", &parts).unwrap();

        let text = collect(&state).unwrap();
        let parsed = parse_backup(&text).unwrap();
        assert_eq!(parsed.document.profiles.len(), 2);
        assert_eq!(parsed.document.active, "default");
        assert_eq!(parsed.profiles.len(), 1, "only the non-active one travels as a file");
        assert_eq!(parsed.profiles[0].id, "p-work");
        assert_eq!(parsed.profiles[0].name, "公司");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_restore_writes_the_profile_files_before_pointing_at_them() {
        let dir = scratch();
        let state = state_at(&dir);
        let mut document = Document::default();
        document.nodes.push(node());
        crate::store::normalize(&mut document);
        document.profiles.push(ProfileMeta {
            id: "p-work".into(),
            name: "公司".into(),
        });

        let parts = ProfileParts {
            nodes: vec![node()],
            ..ProfileParts::default()
        };
        let backup = Backup {
            kind: BACKUP_KIND.to_string(),
            document,
            rule_sets: BTreeMap::new(),
            profiles: vec![StoredProfile {
                id: "p-work".into(),
                name: "公司".into(),
                parts: parts.clone(),
            }],
        };

        let summary = apply(&state, backup).unwrap();
        assert_eq!(summary.profiles, 2);
        assert_eq!(summary.nodes, 1);
        assert_eq!(state.profiles.read().len(), 2);
        let restored = state
            .profile_parts("p-work")
            .unwrap()
            .expect("its file was written before the state pointed at it");
        assert_eq!(restored.nodes.len(), 1);
        assert_eq!(restored.nodes[0].name, "东京");
        // The old「默认」profile is still the active one, inline.
        assert_eq!(*state.active.read(), "default");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn both_halves_of_a_backup_carry_the_profile_overlays() {
        let dir = scratch();
        let state = state_at(&dir);
        state.nodes.write().push(node());
        state.persist().unwrap();
        // The active profile's overlay lives in the document...
        crate::commands::profile::set_merge(&state, "default", "a: 1\n".into()).unwrap();
        // ...and a non-active one lives in its own file, next to its nodes.
        state.profiles.write().push(ProfileMeta {
            id: "p-work".into(),
            name: "公司".into(),
        });
        state.persist().unwrap();
        state
            .save_profile_parts(
                "p-work",
                "公司",
                &ProfileParts {
                    merge: "b: 2\n".into(),
                    ..ProfileParts::default()
                },
            )
            .unwrap();

        let text = collect(&state).unwrap();
        let parsed = parse_backup(&text).unwrap();
        assert_eq!(parsed.document.merge, "a: 1\n", "the active overlay travels in the document");
        assert_eq!(
            parsed.profiles[0].parts.merge, "b: 2\n",
            "the other one travels in the profile file"
        );

        // Restoring into an empty installation must bring both back: an overlay
        // that survived export but not import would be a silent loss.
        let fresh_dir = scratch();
        let fresh = state_at(&fresh_dir);
        apply(&fresh, parsed).unwrap();
        assert_eq!(fresh.merge.read().as_str(), "a: 1\n");
        assert_eq!(
            fresh.profile_parts("p-work").unwrap().unwrap().merge,
            "b: 2\n"
        );
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&fresh_dir).ok();
    }

    #[test]
    fn an_old_backup_without_profiles_restores_as_the_default_profile() {
        // Exactly the shape this build's predecessor wrote: no profile keys at
        // all, at the top level or inside the document.
        let mut document = Document::default();
        document.nodes.push(node());
        let mut value = serde_json::to_value(Backup {
            kind: BACKUP_KIND.to_string(),
            document,
            rule_sets: BTreeMap::new(),
            profiles: Vec::new(),
        })
        .unwrap();
        let outer = value.as_object_mut().unwrap();
        outer.remove("profiles");
        let inner = outer["document"].as_object_mut().unwrap();
        inner.remove("profiles");
        inner.remove("active");
        inner.insert("version".into(), serde_json::json!(1));
        let text = serde_json::to_string(&value).unwrap();

        let parsed = parse_backup(&text).expect("an old backup is still a backup");
        assert!(parsed.profiles.is_empty());

        let dir = scratch();
        let state = state_at(&dir);
        let summary = apply(&state, parsed).unwrap();
        assert_eq!(summary.profiles, 1, "an old file becomes one「默认」profile");
        assert_eq!(*state.active.read(), "default");
        assert_eq!(state.profiles.read().len(), 1);
        assert_eq!(state.profiles.read()[0].name, "默认");
        assert_eq!(state.nodes.read().len(), 1, "its contents are not lost");
        std::fs::remove_dir_all(&dir).ok();
    }
}
