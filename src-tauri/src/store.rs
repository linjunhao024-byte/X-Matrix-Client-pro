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

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::domain::{Node, ProxyGroup, RoutingRule, RuleSet, Settings, Subscription};
use crate::error::{AppError, AppResult};

/// Bumped only when the on-disk shape changes incompatibly.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub version: u32,
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
}

impl Default for Document {
    fn default() -> Self {
        Self {
            version: SCHEMA_VERSION,
            nodes: Vec::new(),
            settings: Settings::default(),
            rules: Vec::new(),
            subscriptions: Vec::new(),
            groups: Vec::new(),
            rule_sets: Vec::new(),
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
    pub fn load(&self) -> AppResult<Document> {
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

    /// Replace the document atomically.
    pub fn save(&self, document: &Document) -> AppResult<()> {
        let json = serde_json::to_string_pretty(document)
            .map_err(|error| AppError::Internal(error.to_string()))?;
        write_atomic(&self.path, &json)
    }
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
}
