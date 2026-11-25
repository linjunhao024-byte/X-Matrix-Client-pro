//! Durable state: one JSON document, written atomically.
//!
//! Why a file and not SQLite: the whole dataset is a node list plus one settings
//! object — a few kilobytes, read once at start and written on every edit. A
//! single atomically-replaced document is simpler to reason about than a schema,
//! and it cannot end up half-migrated. If subscriptions ever grow into
//! thousands of nodes with history, this module is the only thing to swap out:
//! `Store` is the sole owner of the file format.
//!
//! Rules that make it safe:
//! * writes go to `<file>.tmp` and are renamed into place, so a crash never
//!   leaves a truncated document;
//! * a corrupt file is moved aside to `<file>.corrupt` instead of being silently
//!   deleted, and the app starts with defaults rather than refusing to open.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::domain::{Node, NodeId, NodeProbe, ProxyGroup, RoutingRule, RuleSet, Settings, Subscription};
use crate::error::{AppError, AppResult};

/// Bumped only when the on-disk shape changes incompatibly.
///
/// Version 2 introduced profiles. The active profile's contents stay in the
/// inline fields below, so a version 1 file upgrades in place instead of being
/// copied into a subfolder.
pub const SCHEMA_VERSION: u32 = 2;

/// The list-visible half of a profile. Contents live elsewhere: in the inline
/// fields of [`Document`] for the active one, in `profiles/<id>.json` for the
/// rest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileMeta {
    pub id: String,
    pub name: String,
}

/// Everything that switches together when the active profile changes.
///
/// Settings and probes are deliberately absent. Ports, language and hotkeys are
/// one machine's settings — a profile that changed the port a second time would
/// break every rule the user wrote for their router. A latency number says
/// something about the network at one moment, not about which set of nodes is
/// active, and keeping it would make every switch inherit a lie.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProfileParts {
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub rules: Vec<RoutingRule>,
    #[serde(default)]
    pub subscriptions: Vec<Subscription>,
    #[serde(default)]
    pub groups: Vec<ProxyGroup>,
    #[serde(default)]
    pub rule_sets: Vec<RuleSet>,
    /// YAML, deep-merged into the rendered kernel config. Empty = no merge.
    ///
    /// It belongs to the profile because it is a decision about *this* set of
    /// nodes and rules: an overlay that adds a DNS block for a company profile
    /// must not follow the user into their home profile.
    #[serde(default)]
    pub merge: String,
}

/// One profile file: metadata plus contents.
///
/// `parts` is flattened, so the file on disk is `{id, name, nodes, rules, …}`
/// rather than nesting the contents under a key — a profile file reads like a
/// small document, which is what it is.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredProfile {
    pub id: String,
    pub name: String,
    #[serde(flatten)]
    pub parts: ProfileParts,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub version: u32,
    /// Id of the profile the inline fields below belong to.
    #[serde(default)]
    pub active: String,
    /// Every profile, the active one included. The active profile has no file
    /// of its own while it is active: the inline fields are its only copy.
    #[serde(default)]
    pub profiles: Vec<ProfileMeta>,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub settings: Settings,
    /// Routing table, evaluated top to bottom by the kernel.
    #[serde(default)]
    pub rules: Vec<RoutingRule>,
    /// Subscription sources; their nodes carry the `sub:<id>` tag.
    #[serde(default)]
    pub subscriptions: Vec<Subscription>,
    /// Proxy groups: named sets of nodes that rules can target.
    #[serde(default)]
    pub groups: Vec<ProxyGroup>,
    /// Rule sets (domain / IP lists). Only the metadata lives here — the list
    /// itself is cached in its own file under `rulesets/`, because a provider
    /// can be tens of thousands of lines and this document must stay small
    /// enough to rewrite on every edit.
    #[serde(default)]
    pub rule_sets: Vec<RuleSet>,
    /// Last latency measurement per node. Purely informational: the kernel
    /// never reads it, so a stale or missing entry can only make the node list
    /// say "未测", never change how traffic flows.
    #[serde(default)]
    pub probes: HashMap<NodeId, NodeProbe>,
    /// Overlay for the active profile's rendered config. YAML, deep-merged into
    /// the kernel config after rendering; empty = leave the config alone.
    ///
    /// Stored inline here for the same reason the profile's contents are: the
    /// active profile has no file of its own while it is active.
    #[serde(default)]
    pub merge: String,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            version: SCHEMA_VERSION,
            active: String::new(),
            profiles: Vec::new(),
            nodes: Vec::new(),
            settings: Settings::default(),
            rules: Vec::new(),
            subscriptions: Vec::new(),
            groups: Vec::new(),
            rule_sets: Vec::new(),
            probes: HashMap::new(),
            merge: String::new(),
        }
    }
}

pub struct Store {
    path: PathBuf,
}

impl Store {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Read the document, or hand back defaults.
    ///
    /// A missing file is normal on first run. A corrupt one is *not* fatal:
    /// it gets moved aside so the user can still open the app and see what
    /// happened, instead of facing a window that never appears.
    ///
    /// Whatever comes back has at least one profile. Upgrading a version 1 file
    /// rewrites it once, here, so no other module has to know that profiles
    /// arrived in the middle of the app's life.
    pub fn load(&self) -> AppResult<Document> {
        let mut document = self.read()?;
        if normalize(&mut document) {
            // A read-only data directory must not keep the app from opening:
            // the migration is also in memory, so the UI still has a profile.
            if let Err(error) = self.save(&document) {
                eprintln!("x-matrix: 迁移到多配置后写盘失败：{error}");
            }
        }
        Ok(document)
    }

    fn read(&self) -> AppResult<Document> {
        let raw = match std::fs::read_to_string(&self.path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Document::default())
            }
            Err(error) => return Err(AppError::Io(error.to_string())),
        };

        if raw.trim().is_empty() {
            return Ok(Document::default());
        }

        // A UTF-8 BOM is what Notepad and PowerShell 5's `Set-Content` write by
        // default, and serde_json refuses it. Treating that as corruption would
        // move a perfectly good config aside and silently start from defaults,
        // which is exactly what happened during acceptance testing.
        let body = raw.trim_start_matches('\u{feff}');

        match serde_json::from_str::<Document>(body) {
            Ok(document) => Ok(document),
            Err(_) => {
                let aside = self.path.with_extension("corrupt");
                let _ = std::fs::rename(&self.path, &aside);
                Ok(Document::default())
            }
        }
    }

    /// Where the non-active profiles live: beside the document.
    pub fn profiles_dir(&self) -> PathBuf {
        self.path
            .parent()
            .map(|dir| dir.join("profiles"))
            .unwrap_or_else(|| PathBuf::from("profiles"))
    }

    /// File for one profile.
    ///
    /// The id is validated first, because it comes from a JSON file a user can
    /// edit: without [`clean_id`] an id of `../../evil` would write outside the
    /// data directory.
    pub fn profile_path(&self, id: &str) -> AppResult<PathBuf> {
        Ok(self.profiles_dir().join(format!("{}.json", clean_id(id)?)))
    }

    /// Read one profile file.
    ///
    /// `Ok(None)` means the file is simply not there. Callers decide whether
    /// that is "deleted" (the list) or "unreadable" (a switch): the two look
    /// identical on disk but deserve different sentences.
    pub fn read_profile(&self, id: &str) -> AppResult<Option<StoredProfile>> {
        let path = self.profile_path(id)?;
        let raw = match std::fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(AppError::Io(format!("{}：{error}", path.display()))),
        };
        let body = raw.trim_start_matches('\u{feff}');
        let profile = serde_json::from_str::<StoredProfile>(body).map_err(|error| {
            AppError::Io(format!("{} 不是有效的配置文件：{error}", path.display()))
        })?;
        Ok(Some(profile))
    }

    /// Write one profile file atomically.
    pub fn write_profile(&self, profile: &StoredProfile) -> AppResult<()> {
        let path = self.profile_path(&profile.id)?;
        let json = serde_json::to_string_pretty(profile)
            .map_err(|error| AppError::Internal(error.to_string()))?;
        write_atomic(&path, &json)
    }

    /// Delete one profile file. A file that is already gone is not an error:
    /// the caller is about to drop the metadata that points at it anyway.
    pub fn remove_profile(&self, id: &str) -> AppResult<()> {
        match std::fs::remove_file(self.profile_path(id)?) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(AppError::Io(error.to_string())),
        }
    }

    /// Replace the document atomically.
    pub fn save(&self, document: &Document) -> AppResult<()> {
        let json = serde_json::to_string_pretty(document)
            .map_err(|error| AppError::Internal(error.to_string()))?;
        write_atomic(&self.path, &json)
    }
}

/// Bring any document up to the multi-profile shape, in memory.
///
/// Returns whether the caller should write it back. A version 1 file keeps its
/// inline nodes and rules untouched: they are what the user had, so they become
/// the active「默认」profile instead of being copied somewhere and duplicated.
/// The same rule repairs a hand-edited file whose `active` names no profile,
/// which is why this runs on every load rather than only once.
///
/// Shared with restore, because a backup written before profiles existed has no
/// profile list either.
pub(crate) fn normalize(document: &mut Document) -> bool {
    let upgraded = document.version < SCHEMA_VERSION;
    let mut changed = upgraded;

    if document.profiles.is_empty() {
        document.profiles.push(ProfileMeta {
            id: "default".to_string(),
            name: "默认".to_string(),
        });
        changed = true;
    }

    if !document
        .profiles
        .iter()
        .any(|meta| meta.id == document.active)
    {
        document.active = document.profiles[0].id.clone();
        changed = true;
    }

    document.version = SCHEMA_VERSION;
    if upgraded {
        // stderr, not stdout: the packaged app has no console, and stderr is
        // what the launcher redirects to a file during acceptance.
        eprintln!("x-matrix: 已把旧的单配置迁移成「默认」配置");
    }
    changed
}

/// Profile ids become file names, so only a narrow alphabet is allowed.
///
/// `.`, `/` and `\` cannot survive a set that only takes lowercase hex digits
/// and `-`, which is why there is no separate `..` check to forget later.
pub(crate) fn clean_id(id: &str) -> AppResult<&str> {
    let valid = !id.is_empty()
        && id.len() <= 40
        && id
            .chars()
            .all(|each| each.is_ascii_lowercase() || each.is_ascii_digit() || each == '-');
    if !valid {
        return Err(AppError::InvalidInput(format!("配置 id 不合法：{id}")));
    }
    Ok(id)
}

/// `p-` plus twelve hex digits: time plus a counter, so two profiles created in
/// the same millisecond still differ. The alphabet stays inside [`clean_id`].
pub(crate) fn new_profile_id() -> String {
    let raw = crate::import::unique_suffix();
    let tail = if raw.len() > 12 {
        &raw[raw.len() - 12..]
    } else {
        raw.as_str()
    };
    format!("p-{tail}")
}

/// Write via a temporary file plus rename.
///
/// `fs::rename` replaces an existing file on Windows only when the target is not
/// open; retrying once covers the common case of an antivirus or indexer
/// holding a handle for a few milliseconds.
fn write_atomic(path: &Path, contents: &str) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, contents)?;
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(first) => {
            std::thread::sleep(std::time::Duration::from_millis(40));
            std::fs::rename(&tmp, path).map_err(|_| AppError::Io(first.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::domain::node::{Protocol, Security, Transport};
    use crate::domain::ProbeKind;

    fn scratch() -> PathBuf {
        static SEQ: AtomicUsize = AtomicUsize::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("store-{}-{}", std::process::id(), seq));
        std::fs::create_dir_all(&dir).unwrap();
        dir
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
            tags: vec!["tag".into()],
        }
    }

    #[test]
    fn a_missing_file_is_defaults_not_an_error() {
        let dir = scratch();
        let store = Store::new(dir.join("store.json"));
        let document = store.load().unwrap();
        assert!(document.nodes.is_empty());
        assert_eq!(document.version, SCHEMA_VERSION);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn nodes_and_settings_survive_a_round_trip() {
        let dir = scratch();
        let store = Store::new(dir.join("nested/store.json"));
        let mut document = Document::default();
        document.nodes.push(node("香港"));
        document.settings.mixed_port = 7897;
        store.save(&document).unwrap();

        let back = store.load().unwrap();
        assert_eq!(back.nodes.len(), 1);
        assert_eq!(back.nodes[0].name, "香港");
        assert_eq!(back.settings.mixed_port, 7897);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_utf8_bom_does_not_count_as_corruption() {
        // Windows editors add a BOM by habit; the document is still valid JSON.
        let dir = scratch();
        let path = dir.join("store.json");
        let mut document = Document::default();
        document.settings.mixed_port = 1234;
        let json = serde_json::to_string(&document).unwrap();
        std::fs::write(&path, format!("\u{feff}{json}")).unwrap();

        let loaded = Store::new(&path).load().expect("bom must not break the read");
        assert_eq!(loaded.settings.mixed_port, 1234);
        assert!(!path.with_extension("corrupt").exists(), "nothing may be moved aside");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_corrupt_file_is_moved_aside_and_replaced_by_defaults() {
        let dir = scratch();
        let path = dir.join("store.json");
        std::fs::write(&path, "{ not json").unwrap();

        let store = Store::new(&path);
        let document = store.load().unwrap();
        assert!(document.nodes.is_empty());

        // The evidence is kept, and the original name is free for the next save.
        assert!(path.with_extension("corrupt").exists());
        store.save(&document).unwrap();
        assert!(store.load().is_ok());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn saving_leaves_no_temporary_file_behind() {
        let dir = scratch();
        let store = Store::new(dir.join("store.json"));
        store.save(&Document::default()).unwrap();
        assert!(!dir.join("store.tmp").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_probe_record_survives_a_round_trip() {
        let dir = scratch();
        let store = Store::new(dir.join("store.json"));
        let mut document = Document::default();
        document.nodes.push(node("香港"));
        document.probes.insert(
            "id-香港".into(),
            NodeProbe::measured(Some(120), ProbeKind::Tcp),
        );
        // A failed probe is a fact about the node, not a missing entry: the
        // list must be able to say "测不到" instead of "未测".
        document.probes.insert(
            "id-东京".into(),
            NodeProbe::measured(None, ProbeKind::Real),
        );
        store.save(&document).unwrap();

        let back = store.load().unwrap();
        assert_eq!(back.probes.len(), 2);
        let fast = &back.probes["id-香港"];
        assert_eq!(fast.delay_ms, Some(120));
        assert_eq!(fast.kind, ProbeKind::Tcp);
        assert!(fast.at > 0);
        let failed = &back.probes["id-东京"];
        assert_eq!(failed.delay_ms, None);
        assert_eq!(failed.kind, ProbeKind::Real);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_older_document_without_probes_loads_with_an_empty_map() {
        // Exactly what the previous version wrote: no `probes` key at all.
        let dir = scratch();
        let path = dir.join("store.json");
        std::fs::write(
            &path,
            r#"{"version":1,"nodes":[],"settings":{},"rules":[],"subscriptions":[],"groups":[],"rule_sets":[]}"#,
        )
        .unwrap();

        let document = Store::new(&path).load().expect("old file must still open");
        assert!(document.probes.is_empty());
        assert!(!path.with_extension("corrupt").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unknown_fields_from_a_newer_build_are_ignored() {
        let dir = scratch();
        let path = dir.join("store.json");
        std::fs::write(
            &path,
            r#"{"version":1,"nodes":[],"settings":{},"something_new":true}"#,
        )
        .unwrap();
        // Forward compatibility: a newer build adding a field must not brick
        // an older one.
        assert!(Store::new(&path).load().is_ok());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_old_single_profile_document_becomes_the_default_profile() {
        // Exactly what version 1 wrote, node included: the migration must keep
        // the contents in place instead of dropping or duplicating them.
        let dir = scratch();
        let path = dir.join("store.json");
        let mut source = Document::default();
        source.nodes.push(node("东京"));
        source.settings.mixed_port = 7897;
        // Start from a real document so this test cannot rot when a field is
        // added to `Node`, then strip exactly what version 1 did not have.
        let mut old = serde_json::to_value(&source).unwrap();
        let fields = old.as_object_mut().unwrap();
        fields.remove("active");
        fields.remove("profiles");
        fields.insert("version".into(), serde_json::json!(1));
        std::fs::write(&path, serde_json::to_string(&old).unwrap()).unwrap();

        let store = Store::new(&path);
        let document = store.load().expect("an old file must upgrade, not fail");
        assert_eq!(document.version, SCHEMA_VERSION);
        assert_eq!(document.active, "default");
        assert_eq!(
            document.profiles,
            vec![ProfileMeta {
                id: "default".into(),
                name: "默认".into()
            }]
        );
        assert_eq!(document.nodes.len(), 1);
        assert_eq!(document.nodes[0].name, "东京");
        assert_eq!(document.settings.mixed_port, 7897);

        // And the upgrade is on disk, so the next launch does not redo it.
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("\"active\": \"default\""));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_migration_happens_once() {
        let dir = scratch();
        let path = dir.join("store.json");
        std::fs::write(&path, r#"{"version":1,"nodes":[]}"#).unwrap();

        let store = Store::new(&path);
        let first = store.load().unwrap();
        assert_eq!(first.profiles.len(), 1);
        let after_first = std::fs::read_to_string(&path).unwrap();

        let second = store.load().unwrap();
        assert_eq!(second.profiles.len(), 1, "a second load must not add a profile");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            after_first,
            "nothing changed, so nothing may be rewritten"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_profile_file_round_trips_through_the_store() {
        let dir = scratch();
        let store = Store::new(dir.join("store.json"));
        let parts = ProfileParts {
            nodes: vec![node("香港")],
            ..ProfileParts::default()
        };
        let stored = StoredProfile {
            id: "p-0123456789ab".into(),
            name: "家里".into(),
            parts: parts.clone(),
        };
        store.write_profile(&stored).unwrap();

        // One file per profile, beside the document, named after the id.
        let path = store.profile_path(&stored.id).unwrap();
        assert!(path.starts_with(dir.join("profiles")));
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("\"nodes\""), "the contents are flattened, not nested");
        assert!(!raw.contains("\"parts\""));

        let back = store.read_profile(&stored.id).unwrap().expect("the file is there");
        assert_eq!(back.id, stored.id);
        assert_eq!(back.name, "家里");
        assert_eq!(back.parts.nodes.len(), 1);
        assert_eq!(back.parts.nodes[0].name, "香港");

        store.remove_profile(&stored.id).unwrap();
        assert!(store.read_profile(&stored.id).unwrap().is_none());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_overlay_survives_a_round_trip_and_an_older_file_loads_without_one() {
        let dir = scratch();
        let store = Store::new(dir.join("store.json"));

        // A document written before overlays existed: the key is simply absent.
        // It has to load with "no merge", not fail — the file is the user's.
        let mut document = Document::default();
        document.nodes.push(node("东京"));
        let mut raw = serde_json::to_value(&document).unwrap();
        raw.as_object_mut().unwrap().remove("merge");
        std::fs::write(store.path(), raw.to_string()).unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.merge, "");
        assert_eq!(loaded.nodes.len(), 1, "the rest of the file still counts");

        // Writing it back keeps what the user typed, overlay and all.
        let mut document = loaded;
        document.merge = "dns:\n  servers:\n    - https://1.1.1.1/dns-query\n".into();
        store.save(&document).unwrap();
        assert_eq!(store.load().unwrap().merge, document.merge);

        // The same for a profile file, whose contents are flattened.
        let path = store.profile_path("p-old").unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{\"id\":\"p-old\",\"name\":\"旧\",\"nodes\":[]}").unwrap();
        let old = store.read_profile("p-old").unwrap().unwrap();
        assert_eq!(old.parts.merge, "", "a profile file without the key has no overlay");

        let with_overlay = StoredProfile {
            parts: ProfileParts {
                merge: "log:\n  loglevel: debug\n".into(),
                ..ProfileParts::default()
            },
            ..old
        };
        store.write_profile(&with_overlay).unwrap();
        assert_eq!(
            store.read_profile("p-old").unwrap().unwrap().parts.merge,
            "log:\n  loglevel: debug\n"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_profile_file_reads_as_none() {
        // The list has to survive a profile file deleted behind its back.
        let dir = scratch();
        let store = Store::new(dir.join("store.json"));
        assert!(store.read_profile("default").unwrap().is_none());
        // Deleting a file that is already gone is a no-op, not an error: the
        // caller is about to drop the metadata that points at it.
        store.remove_profile("default").unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn profile_ids_that_could_escape_the_data_directory_are_rejected() {
        // The id becomes a file name, so anything that can climb out of the
        // directory, or that Windows treats specially, must be refused.
        for bad in ["../x", "", "a/b", "a\\b", "..", "PROFILE", "配置"] {
            let error = clean_id(bad).expect_err("this id must be refused");
            assert!(
                matches!(error, AppError::InvalidInput(_)),
                "{bad:?} must be an InvalidInput, got {error:?}"
            );
        }
        let too_long = "a".repeat(41);
        assert!(clean_id(&too_long).is_err(), "an id longer than 40 chars is a file name problem");
        assert!(clean_id("a".repeat(40).as_str()).is_ok(), "40 is still allowed");
        assert_eq!(clean_id("default").unwrap(), "default");
        assert_eq!(clean_id("p-0123456789ab").unwrap(), "p-0123456789ab");

        let generated = new_profile_id();
        assert!(generated.starts_with("p-"), "{generated}");
        assert_eq!(generated.len(), 14, "p- plus twelve hex digits: {generated}");
        assert!(clean_id(&generated).is_ok());
    }
}
