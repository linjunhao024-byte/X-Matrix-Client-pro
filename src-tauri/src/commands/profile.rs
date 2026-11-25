//! Profiles: several sets of nodes and rules, one active at a time.
//!
//! A profile is not a copy of the settings. Ports, language, hotkeys and the
//! tray belong to the machine, not to the set of nodes, and a profile that
//! moved the port would break every router rule the user wrote around it. So a
//! profile carries exactly [`ProfileParts`], and the active one keeps those
//! parts in the document's inline fields while the rest wait in their own
//! files — a switch is then a memory swap plus one file write, not a copy.
//!
//! The logic lives in free functions taking `&AppState` so the order that
//! matters (save the profile you are leaving *before* reading the one you are
//! entering) is testable without a Tauri runtime or a kernel.

use std::sync::Arc;

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, State};

use crate::commands::engine::{prepare_start, remember_session, run_blocking, start_engine};
use crate::commands::engine::{Proxy, Runtime};
use crate::domain::Node;
use crate::engine::EngineKind;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::store::{new_profile_id, ProfileMeta, ProfileParts};

/// One row in the profiles list.
#[derive(Debug, Clone, Serialize, Type)]
pub struct ProfileView {
    pub id: String,
    pub name: String,
    /// Whether this is the profile the running state belongs to.
    pub active: bool,
    // u32 rather than usize: specta renders usize as `number | null`, and the
    // UI shows this as a plain count.
    #[specta(type = f64)]
    pub nodes: u32,
    /// The profile's YAML overlay, whole text. The list carries it so the
    /// editor opens with what is actually stored instead of a second round
    /// trip that could disagree with the row it belongs to.
    pub merge: String,
}

/// What a switch did to the kernel, so the UI does not have to guess.
#[derive(Debug, Serialize, Type)]
pub struct SwitchReport {
    pub active: ProfileView,
    /// The kernel was running and now runs on the new profile.
    pub restarted: bool,
    /// The kernel was running and is now stopped, because the new profile has
    /// no node to run.
    pub stopped: bool,
    /// Why the restart did not happen, in the backend's own words.
    pub note: Option<String>,
}

impl ProfileView {
    /// One row. A profile whose file cannot be read shows zero nodes and an
    /// empty overlay: the list has to stay usable, and inventing a count or
    /// contents would be worse than a blank the switch later refuses to honour.
    fn of(state: &AppState, meta: &ProfileMeta, active: &str) -> Self {
        let is_active = meta.id == active;
        let stored = if is_active {
            None
        } else {
            // Read once: nodes and merge live in the same file, and reading it
            // twice could describe two different revisions.
            state.profile_parts(&meta.id).ok().flatten()
        };
        let nodes = if is_active {
            state.nodes.read().len() as u32
        } else {
            stored.as_ref().map(|parts| parts.nodes.len() as u32).unwrap_or(0)
        };
        let merge = if is_active {
            state.merge.read().clone()
        } else {
            stored.map(|parts| parts.merge).unwrap_or_default()
        };
        Self {
            id: meta.id.clone(),
            name: meta.name.clone(),
            active: is_active,
            nodes,
            merge,
        }
    }
}

/// Trim a name the user typed and refuse an empty one.
fn checked_name(name: &str) -> AppResult<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::InvalidInput("配置名不能为空".to_string()));
    }
    Ok(name.to_string())
}

fn find_meta(state: &AppState, id: &str) -> AppResult<ProfileMeta> {
    state
        .profiles
        .read()
        .iter()
        .find(|meta| meta.id == id)
        .cloned()
        .ok_or_else(|| AppError::NotFound(format!("配置 {id} 不存在")))
}

/// Every profile, active one included, in the order the user arranged them.
pub(crate) fn list_profiles(state: &AppState) -> Vec<ProfileView> {
    let active = state.active.read().clone();
    state
        .profiles
        .read()
        .iter()
        .map(|meta| ProfileView::of(state, meta, &active))
        .collect()
}

/// Create a profile, optionally seeded from another one.
///
/// The file is written before the metadata, so a crash in between leaves an
/// orphan file (invisible, harmless) rather than a listed profile with no
/// contents.
pub(crate) fn create_profile(
    state: &AppState,
    name: &str,
    from: Option<&str>,
) -> AppResult<ProfileView> {
    let name = checked_name(name)?;
    if state.profiles.read().iter().any(|meta| meta.name == name) {
        return Err(AppError::InvalidInput(format!(
            "已经有一个同名配置：{name}"
        )));
    }

    let parts = match from {
        None => ProfileParts::default(),
        Some(source) => {
            // The source must be a profile that exists, not merely a file that
            // happens to be lying there.
            find_meta(state, source)?;
            state
                .profile_parts(source)?
                .ok_or_else(|| AppError::NotFound(format!("配置文件读不出来：{source}")))?
        }
    };

    let id = new_profile_id();
    state.save_profile_parts(&id, &name, &parts)?;

    state.profiles.write().push(ProfileMeta {
        id: id.clone(),
        name: name.clone(),
    });
    if let Err(error) = state.persist() {
        // Undo the metadata and the file: a profile the document does not know
        // about would look like a phantom on the next launch.
        state.profiles.write().retain(|meta| meta.id != id);
        if let Err(cleanup) = state.drop_profile_file(&id) {
            eprintln!("x-matrix: 回滚新建配置 {id} 失败：{cleanup}");
        }
        return Err(error);
    }

    let active = state.active.read().clone();
    let meta = find_meta(state, &id)?;
    Ok(ProfileView::of(state, &meta, &active))
}

/// Rename one profile. Contents are never touched.
///
/// A non-active profile also carries its name inside its file, because that
/// file must stay readable on its own; the active one has no file, so the
/// document is its only record.
pub(crate) fn rename_profile(state: &AppState, id: &str, name: &str) -> AppResult<ProfileView> {
    let name = checked_name(name)?;
    if state
        .profiles
        .read()
        .iter()
        .any(|meta| meta.id != id && meta.name == name)
    {
        return Err(AppError::InvalidInput(format!(
            "已经有一个同名配置：{name}"
        )));
    }

    let active = state.active.read().clone();
    let meta = find_meta(state, id)?;

    if meta.id != active {
        // Name and contents travel in the same file: rename it in place so the
        // two cannot disagree.
        let parts = state
            .profile_parts(id)?
            .ok_or_else(|| AppError::NotFound(format!("配置文件读不出来：{id}")))?;
        state.save_profile_parts(id, &name, &parts)?;
    }

    {
        let mut profiles = state.profiles.write();
        if let Some(entry) = profiles.iter_mut().find(|entry| entry.id == id) {
            entry.name = name.clone();
        }
    }
    state.persist()?;

    let meta = find_meta(state, id)?;
    Ok(ProfileView::of(state, &meta, &active))
}

/// Delete a profile and its file.
///
/// Refuses the two cases that would leave the app without a usable active
/// profile: deleting the one in use, and deleting the last one.
pub(crate) fn delete_profile(state: &AppState, id: &str) -> AppResult<()> {
    let meta = find_meta(state, id)?;
    if *state.active.read() == id {
        return Err(AppError::InvalidInput(
            "不能删除正在使用的配置：先切到另一个".to_string(),
        ));
    }
    if state.profiles.read().len() <= 1 {
        return Err(AppError::InvalidInput("至少要留一个配置".to_string()));
    }

    state.profiles.write().retain(|entry| entry.id != id);
    if let Err(error) = state.persist() {
        // The document is the source of truth; put the row back exactly as it
        // was so a retry sees the same list.
        state.profiles.write().push(meta);
        return Err(error);
    }
    // A file that outlives its metadata is invisible and harmless, so a failure
    // here is noise in the console, not an error the user must solve.
    if let Err(error) = state.drop_profile_file(id) {
        eprintln!("x-matrix: 删掉配置文件 {id} 失败：{error}");
    }
    Ok(())
}

/// Replace one profile's YAML overlay.
///
/// The text is validated here with the same function the renderer uses, so a
/// bad overlay is refused when it is saved instead of when the kernel next
/// starts. An empty string is a valid overlay: it means "no merge at all",
/// which is also what a profile that never had one carries.
pub(crate) fn set_merge(state: &AppState, id: &str, text: String) -> AppResult<ProfileView> {
    let meta = find_meta(state, id)?;
    // Same rules as `Engine::render_config` will apply: parse, then insist on
    // a mapping at the top. `{}` as the base is enough because nothing is kept
    // from it — only the overlay's own validity is being judged.
    crate::engine::merge::merge_text(&serde_json::json!({}), &text)?;

    let active = state.active.read().clone();
    if meta.id == active {
        // The running state is the document, so the overlay is swapped in
        // memory first and written immediately: a profile the user has to
        // press "save" twice for is worse than a write that reports its error.
        let previous = {
            let mut merge = state.merge.write();
            std::mem::replace(&mut *merge, text)
        };
        if let Err(error) = state.persist() {
            // Keep memory and disk saying the same thing: the document still
            // holds the old overlay, so the old overlay goes back.
            *state.merge.write() = previous;
            return Err(error);
        }
    } else {
        // A non-active profile keeps its overlay in its own file, beside the
        // nodes it applies to. `save_profile_parts` writes that file atomically
        // and touches no in-memory field, so a failure needs no rollback.
        let mut parts = state
            .profile_parts(id)?
            .ok_or_else(|| AppError::NotFound(format!("配置文件读不出来：{id}")))?;
        parts.merge = text;
        state.save_profile_parts(id, &meta.name, &parts)?;
    }

    let meta = find_meta(state, id)?;
    Ok(ProfileView::of(state, &meta, &active))
}

/// What a switch has to do, decided before anything is written.
///
/// `Away` carries the contents already in memory: reading the target file is
/// step 3, and doing it here keeps the read off the async path.
#[derive(Debug)]
pub(crate) enum SwitchPrep {
    /// The requested profile is the one in use; nothing to write.
    Already(ProfileView),
    /// The profile being left is saved; move the inline fields to `target`.
    Away { target: ProfileParts },
}

/// Steps 1–3 of a switch: check, save what is being left, read what is wanted.
///
/// The order is the whole point. Saving first means a failure can only ever
/// lose the new profile, never the one the user is still on.
pub(crate) fn prepare_switch(state: &AppState, id: &str) -> AppResult<SwitchPrep> {
    let meta = find_meta(state, id)?;
    let active = state.active.read().clone();
    if meta.id == active {
        return Ok(SwitchPrep::Already(ProfileView::of(state, &meta, &active)));
    }

    let leaving = find_meta(state, &active)?;
    state.save_profile_parts(&active, &leaving.name, &state.active_parts())?;

    let target = state
        .profile_parts(id)?
        .ok_or_else(|| AppError::NotFound(format!("配置文件读不出来：{id}")))?;
    Ok(SwitchPrep::Away { target })
}

/// Step 5 of a switch: make the new profile active on disk and in memory.
///
/// The memory swap happens before the write, so a failed write is rolled back
/// instead of leaving the app showing one profile and the file saying another.
pub(crate) fn commit_switch(
    state: &AppState,
    id: &str,
    target: ProfileParts,
    leaving: &str,
) -> AppResult<()> {
    let previous = state.active_parts();
    state.set_active_parts(target);
    *state.active.write() = id.to_string();
    if let Err(error) = state.persist() {
        state.set_active_parts(previous);
        *state.active.write() = leaving.to_string();
        return Err(error);
    }
    // The inline fields are now this profile's only copy; a duplicate file
    // would be a stale copy nobody reads until the day it silently wins.
    if let Err(error) = state.drop_profile_file(id) {
        eprintln!("x-matrix: 切到 {id} 后删不掉旧文件：{error}");
    }
    Ok(())
}

/// Pick the node a restarted kernel should use: the remembered one when the new
/// profile still has it, otherwise its first node.
fn restart_node(state: &AppState) -> Option<Node> {
    let wanted = state.settings.read().last_node_id.clone();
    let nodes = state.nodes.read();
    wanted
        .and_then(|id| nodes.iter().find(|node| node.id == id).cloned())
        .or_else(|| nodes.first().cloned())
}

fn engine_kind(state: &AppState) -> EngineKind {
    state
        .settings
        .read()
        .last_engine
        .as_deref()
        .and_then(EngineKind::parse)
        .unwrap_or(EngineKind::Xray)
}

#[tauri::command]
#[specta::specta]
pub fn profiles_list(state: State<'_, AppState>) -> Vec<ProfileView> {
    list_profiles(&state)
}

#[tauri::command]
#[specta::specta]
pub fn profile_create(
    state: State<'_, AppState>,
    name: String,
    from: Option<String>,
) -> AppResult<ProfileView> {
    create_profile(&state, &name, from.as_deref())
}

#[tauri::command]
#[specta::specta]
pub fn profile_rename(
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> AppResult<ProfileView> {
    rename_profile(&state, &id, &name)
}

#[tauri::command]
#[specta::specta]
pub fn profile_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    delete_profile(&state, &id)
}

/// Save the YAML overlay of one profile. Singular `profile_` prefix, like the
/// other per-row commands.
#[tauri::command]
#[specta::specta]
pub fn profile_merge_set(
    state: State<'_, AppState>,
    id: String,
    text: String,
) -> AppResult<ProfileView> {
    set_merge(&state, &id, text)
}

/// Switch profiles, restarting the kernel on the new one when it was running.
///
/// The kernel is stopped before the state is swapped (a running kernel was
/// rendered from the old profile) and started again only after the new profile
/// is actually the active one. When the new profile has no node, the honest
/// outcome is a stopped kernel plus a note, not a kernel started on a node that
/// is not in the profile the user picked.
#[tauri::command]
#[specta::specta]
pub async fn profile_switch(
    app: AppHandle,
    state: State<'_, AppState>,
    runtime: Runtime<'_>,
    proxy: Proxy<'_>,
    id: String,
) -> AppResult<SwitchReport> {
    let target = match prepare_switch(&state, &id)? {
        SwitchPrep::Already(view) => {
            return Ok(SwitchReport {
                active: view,
                restarted: false,
                stopped: false,
                note: None,
            })
        }
        SwitchPrep::Away { target, .. } => target,
    };

    let leaving = state.active.read().clone();
    let was_running = runtime.status().running;

    if was_running {
        let runtime = Arc::clone(runtime.inner());
        let proxy = Arc::clone(proxy.inner());
        run_blocking(move || {
            // Hand the proxy back before the port disappears, same order as
            // `engine_stop`.
            if proxy.is_active() {
                proxy.disable()?;
            }
            runtime.stop()?;
            Ok::<(), AppError>(())
        })
        .await?;
    }

    commit_switch(&state, &id, target, &leaving)?;

    let mut restarted = false;
    let mut stopped = false;
    let mut note = None;

    if was_running {
        let kind = engine_kind(&state);
        match restart_node(&state) {
            None => {
                stopped = true;
                note = Some("新配置里没有节点，内核保持停止".to_string());
            }
            Some(node) => match prepare_start(&state, kind, &node.id) {
                Err(error) => note = Some(error.to_string()),
                Ok(request) => {
                    let runtime = Arc::clone(runtime.inner());
                    let proxy = Arc::clone(proxy.inner());
                    let outcome = run_blocking(move || start_engine(&runtime, &proxy, request)).await;
                    match outcome {
                        Ok(_) => {
                            restarted = true;
                            remember_session(&state, kind, &node.id);
                        }
                        Err(error) => note = Some(error.to_string()),
                    }
                }
            },
        }
    }

    crate::refresh_tray(&app);

    let active = state.active.read().clone();
    let meta = find_meta(&state, &active)?;
    Ok(SwitchReport {
        active: ProfileView::of(&state, &meta, &active),
        restarted,
        stopped,
        note,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::domain::node::{Protocol, Security, Transport};
    use crate::store::Store;

    fn scratch() -> PathBuf {
        static SEQ: AtomicUsize = AtomicUsize::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("profile-{}-{}", std::process::id(), seq));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn state_at(dir: &std::path::Path) -> AppState {
        AppState::load(Store::new(dir.join("store.json")))
    }

    fn node(name: &str) -> Node {
        Node {
            id: format!("id-{name}"),
            name: name.into(),
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

    #[test]
    fn a_fresh_state_has_exactly_one_active_profile() {
        let dir = scratch();
        let state = state_at(&dir);
        let rows = list_profiles(&state);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "default");
        assert!(rows[0].active);
        assert_eq!(rows[0].name, "默认");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn creating_a_profile_with_a_name_that_is_taken_is_refused() {
        let dir = scratch();
        let state = state_at(&dir);
        create_profile(&state, "公司", None).unwrap();
        let error = create_profile(&state, " 公司 ", None).unwrap_err();
        assert!(
            matches!(&error, AppError::InvalidInput(message) if message == "已经有一个同名配置：公司"),
            "{error:?}"
        );
        assert_eq!(list_profiles(&state).len(), 2, "nothing was added");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_profile_can_start_as_a_copy_of_another_one() {
        let dir = scratch();
        let state = state_at(&dir);
        state.nodes.write().push(node("东京"));
        state.persist().unwrap();

        let copied = create_profile(&state, "副本", Some("default")).unwrap();
        assert_eq!(copied.nodes, 1);
        // It starts inactive, so its contents are already in its own file.
        assert!(!copied.active);
        assert_eq!(state.profile_parts(&copied.id).unwrap().unwrap().nodes.len(), 1);

        // And the copy is independent: changing one does not change the other.
        state.nodes.write().push(node("香港"));
        state.persist().unwrap();
        assert_eq!(state.profile_parts(&copied.id).unwrap().unwrap().nodes.len(), 1);
        assert_eq!(list_profiles(&state)[0].nodes, 2);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_profile_in_use_cannot_be_deleted() {
        let dir = scratch();
        let state = state_at(&dir);
        create_profile(&state, "公司", None).unwrap();
        let error = delete_profile(&state, "default").unwrap_err();
        assert!(
            matches!(&error, AppError::InvalidInput(message) if message == "不能删除正在使用的配置：先切到另一个"),
            "{error:?}"
        );
        assert_eq!(list_profiles(&state).len(), 2);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_last_profile_cannot_be_deleted() {
        // The only way to reach this branch: the active id names a profile that
        // is not the one being deleted. A hand-edited document can do that, and
        // an empty app is exactly what the guard exists to prevent.
        let dir = scratch();
        let state = state_at(&dir);
        *state.active.write() = "somewhere-else".to_string();
        let error = delete_profile(&state, "default").unwrap_err();
        assert!(
            matches!(&error, AppError::InvalidInput(message) if message == "至少要留一个配置"),
            "{error:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn renaming_a_profile_keeps_its_contents_and_updates_its_file() {
        let dir = scratch();
        let state = state_at(&dir);
        let created = create_profile(&state, "公司", None).unwrap();

        let renamed = rename_profile(&state, &created.id, " 办公室 ").unwrap();
        assert_eq!(renamed.name, "办公室");
        assert!(!renamed.active);
        // The file's own name field has to agree, or a later export would ship
        // the old name.
        let stored = Store::new(dir.join("store.json"))
            .read_profile(&created.id)
            .unwrap()
            .unwrap();
        assert_eq!(stored.name, "办公室");

        // The active profile is renamed in the document only, and keeps its
        // nodes.
        state.nodes.write().push(node("东京"));
        state.persist().unwrap();
        rename_profile(&state, "default", "家里").unwrap();
        assert_eq!(state.active.read().as_str(), "default");
        assert_eq!(state.nodes.read().len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn switching_saves_the_profile_it_leaves_and_loads_the_target() {
        let dir = scratch();
        let state = state_at(&dir);
        state.nodes.write().push(node("东京"));
        state.persist().unwrap();

        let created = create_profile(&state, "公司", None).unwrap();
        // Give the new profile its own contents, as the node list UI would.
        state
            .save_profile_parts(
                &created.id,
                "公司",
                &ProfileParts {
                    nodes: vec![node("香港"), node("新加坡")],
                    ..ProfileParts::default()
                },
            )
            .unwrap();

        let SwitchPrep::Away { target, .. } = prepare_switch(&state, &created.id).unwrap() else {
            panic!("a different profile must not report itself as already active");
        };
        // Before the commit, the file of the profile being left holds its
        // contents — that copy is what a crash would fall back to.
        let left = Store::new(dir.join("store.json"))
            .read_profile("default")
            .unwrap()
            .unwrap();
        assert_eq!(left.parts.nodes.len(), 1);

        commit_switch(&state, &created.id, target, "default").unwrap();
        assert_eq!(state.active.read().as_str(), created.id.as_str());
        assert_eq!(state.nodes.read().len(), 2);
        assert_eq!(state.nodes.read()[0].name, "香港");
        assert_eq!(state.active_parts().nodes.len(), 2);

        // The new active profile's file is gone: the inline fields are its only
        // copy.
        assert!(Store::new(dir.join("store.json"))
            .read_profile(&created.id)
            .unwrap()
            .is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn asking_for_the_profile_already_in_use_changes_nothing() {
        let dir = scratch();
        let state = state_at(&dir);
        state.nodes.write().push(node("东京"));
        state.persist().unwrap();
        let before = std::fs::read(dir.join("store.json")).unwrap();

        let SwitchPrep::Already(view) = prepare_switch(&state, "default").unwrap() else {
            panic!("the active profile must be reported as already active");
        };
        assert!(view.active);
        assert_eq!(view.nodes, 1);
        assert_eq!(
            std::fs::read(dir.join("store.json")).unwrap(),
            before,
            "an already-active profile must not touch the disk"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_profile_file_is_a_sentence_not_a_panic() {
        let dir = scratch();
        let state = state_at(&dir);
        state.profiles.write().push(ProfileMeta {
            id: "p-gone".into(),
            name: "丢了".into(),
        });
        state.persist().unwrap();

        // The list still answers, with a zero instead of a count it cannot know.
        let rows = list_profiles(&state);
        let gone = rows.iter().find(|row| row.id == "p-gone").unwrap();
        assert_eq!(gone.nodes, 0);
        assert!(!gone.active);

        // Switching to it fails with something the user can read.
        let error = prepare_switch(&state, "p-gone").unwrap_err();
        assert!(
            matches!(&error, AppError::NotFound(message) if message == "配置文件读不出来：p-gone"),
            "{error:?}"
        );
        // And the failure left the active profile exactly as it was.
        assert_eq!(state.active.read().as_str(), "default");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_active_profiles_overlay_lives_in_the_document() {
        let dir = scratch();
        let state = state_at(&dir);
        let text = "dns:\n  servers:\n    - https://1.1.1.1/dns-query\n".to_string();

        let view = set_merge(&state, "default", text.clone()).unwrap();
        assert_eq!(view.merge, text);
        assert!(view.active);
        assert_eq!(state.merge.read().as_str(), text.as_str());

        // The document is where the active profile lives, so that is where the
        // overlay must be — otherwise a switch would lose it.
        let document = Store::new(dir.join("store.json")).load().unwrap();
        assert_eq!(document.merge, text);

        // And it survives a restart: a new process reads it back from the file.
        let again = state_at(&dir);
        assert_eq!(again.merge.read().as_str(), text.as_str());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_non_active_profiles_overlay_lives_in_its_own_file() {
        let dir = scratch();
        let state = state_at(&dir);
        let created = create_profile(&state, "公司", None).unwrap();
        let text = "log:\n  loglevel: debug\n".to_string();

        let view = set_merge(&state, &created.id, text.clone()).unwrap();
        assert_eq!(view.merge, text);
        assert!(!view.active);
        // The running profile is untouched: an overlay belongs to one profile.
        assert_eq!(state.merge.read().as_str(), "");
        let stored = Store::new(dir.join("store.json"))
            .read_profile(&created.id)
            .unwrap()
            .unwrap();
        assert_eq!(stored.parts.merge, text);
        // The list already carries it, so opening the editor needs no second call.
        let row = list_profiles(&state)
            .into_iter()
            .find(|row| row.id == created.id)
            .unwrap();
        assert_eq!(row.merge, text);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_overlay_that_cannot_be_a_mapping_is_refused_without_changing_anything() {
        let dir = scratch();
        let state = state_at(&dir);
        set_merge(&state, "default", "log:\n  loglevel: debug\n".to_string()).unwrap();
        let before = std::fs::read(dir.join("store.json")).unwrap();

        for (text, prefix) in [
            ("- a\n- b\n", "覆盖内容的顶层必须是一个映射（key: value），现在是 数组"),
            ("dns: [unclosed\n", "覆盖内容不是合法 YAML："),
        ] {
            let error = set_merge(&state, "default", text.to_string()).unwrap_err();
            assert!(
                matches!(&error, AppError::InvalidInput(message) if message.starts_with(prefix)),
                "{error:?}"
            );
        }
        // The bad text is the renderer's error verbatim, and nothing moved:
        // still the old overlay, still the same bytes on disk.
        assert_eq!(state.merge.read().as_str(), "log:\n  loglevel: debug\n");
        assert_eq!(std::fs::read(dir.join("store.json")).unwrap(), before);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_empty_overlay_is_how_you_clear_one() {
        let dir = scratch();
        let state = state_at(&dir);
        set_merge(&state, "default", "log:\n  loglevel: debug\n".into()).unwrap();
        let cleared = set_merge(&state, "default", "  \n".into()).unwrap();
        assert_eq!(cleared.merge, "  \n");
        // Stored as the user left it; `merge_text` treats blank as "no merge",
        // so the rendering outcome is the same as an empty string.
        assert_eq!(state.merge.read().as_str(), "  \n");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_unknown_profile_cannot_have_an_overlay_set() {
        let dir = scratch();
        let state = state_at(&dir);
        let error = set_merge(&state, "nope", "log:\n  loglevel: debug\n".into()).unwrap_err();
        assert!(
            matches!(&error, AppError::NotFound(message) if message == "配置 nope 不存在"),
            "{error:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn switching_carries_the_overlay_with_the_profile_it_belongs_to() {
        let dir = scratch();
        let state = state_at(&dir);
        state.nodes.write().push(node("东京"));
        state.persist().unwrap();
        set_merge(&state, "default", "a: 1\n".into()).unwrap();

        let created = create_profile(&state, "公司", None).unwrap();
        let target_text = "b: 2\n".to_string();
        set_merge(&state, &created.id, target_text.clone()).unwrap();

        let SwitchPrep::Away { target } = prepare_switch(&state, &created.id).unwrap() else {
            panic!("a different profile must not report itself as already active");
        };
        commit_switch(&state, &created.id, target, "default").unwrap();

        // The new active profile brought its own overlay...
        assert_eq!(state.merge.read().as_str(), target_text.as_str());
        // ...and the one it left keeps its overlay in its file, ready for the
        // switch back.
        let left = Store::new(dir.join("store.json"))
            .read_profile("default")
            .unwrap()
            .unwrap();
        assert_eq!(left.parts.merge, "a: 1\n");
        assert_eq!(
            Store::new(dir.join("store.json")).load().unwrap().merge,
            target_text
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}