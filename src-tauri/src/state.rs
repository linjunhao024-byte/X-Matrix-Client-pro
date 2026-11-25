//! Application state. Every field is the SINGLE owner of its data
//! (ARCHITECTURE.md §3). Nothing outside this module may write to it directly.
//!
//! Runtime state is deliberately *not* here: the engine runtime owns the
//! process handle and is managed as its own `Arc<EngineRuntime>`, so the
//! "what is running right now" question has exactly one answer.
//!
//! Durable state lives here, and every mutation goes through [`AppState::persist`]
//! so the file on disk can never drift from memory.

use std::collections::HashMap;

use parking_lot::RwLock;

use crate::domain::{
    Node, NodeId, NodeProbe, ProbeKind, ProxyGroup, RoutingRule, RuleSet, Settings, Subscription,
};
use crate::error::AppResult;
use crate::rulesets::RuleSetCache;
use crate::store::{Document, ProfileMeta, ProfileParts, Store, StoredProfile, SCHEMA_VERSION};

pub struct AppState {
    pub nodes: RwLock<Vec<Node>>,
    pub settings: RwLock<Settings>,
    pub rules: RwLock<Vec<RoutingRule>>,
    pub subscriptions: RwLock<Vec<Subscription>>,
    pub groups: RwLock<Vec<ProxyGroup>>,
    pub rule_sets: RwLock<Vec<RuleSet>>,
    /// Every profile, the active one included. The order is the order the user
    /// sees, so it must survive a restart untouched.
    pub profiles: RwLock<Vec<ProfileMeta>>,
    /// Id of the profile whose contents are in the fields above. Only the
    /// active profile has its contents inline; the others live in their own
    /// files, which keeps the hot path (start the kernel, list nodes) reading
    /// one document instead of fanning out per profile.
    pub active: RwLock<String>,
    /// Last latency measurement per node, keyed by node id. It is a note to
    /// the user, not an input to routing: nothing here can change how traffic
    /// flows, which is why a stale entry is harmless.
    pub probes: RwLock<HashMap<NodeId, NodeProbe>>,
    /// Cached rule-set lists (one file per set, next to the document).
    pub rule_set_files: RuleSetCache,
    store: Store,
}

impl AppState {
    /// Load whatever is on disk; a damaged file is reported but never fatal.
    pub fn load(store: Store) -> Self {
        let document = match store.load() {
            Ok(document) => document,
            Err(error) => {
                eprintln!(
                    "x-matrix: 无法读取 {}：{error}。以默认设置启动。",
                    store.path().display()
                );
                Document::default()
            }
        };

        // A profile whose file went missing is worth a line in the console, but
        // it must not stop the app: the other profiles still work, and the list
        // reports the missing one as an empty profile rather than pretending.
        for meta in document.profiles.iter().filter(|meta| meta.id != document.active) {
            match store.read_profile(&meta.id) {
                Ok(Some(_)) => {}
                Ok(None) => eprintln!("x-matrix: 配置 {} 的文件不见了", meta.id),
                Err(error) => eprintln!("x-matrix: 配置 {} 的文件读不出来：{error}", meta.id),
            }
        }

        Self {
            nodes: RwLock::new(document.nodes),
            settings: RwLock::new(document.settings),
            rules: RwLock::new(document.rules),
            subscriptions: RwLock::new(document.subscriptions),
            groups: RwLock::new(document.groups),
            rule_sets: RwLock::new(document.rule_sets),
            profiles: RwLock::new(document.profiles),
            active: RwLock::new(document.active),
            probes: RwLock::new(document.probes),
            rule_set_files: RuleSetCache::new(rule_sets_dir(&store)),
            store,
        }
    }

    /// In-memory state for tests and for the very first run.
    pub fn new() -> Self {
        Self::load(Store::new(default_store_path()))
    }

    /// The whole durable state, exactly as it would be written to disk.
    pub fn document(&self) -> Document {
        Document {
            version: SCHEMA_VERSION,
            active: self.active.read().clone(),
            profiles: self.profiles.read().clone(),
            nodes: self.nodes.read().clone(),
            settings: self.settings.read().clone(),
            rules: self.rules.read().clone(),
            subscriptions: self.subscriptions.read().clone(),
            groups: self.groups.read().clone(),
            rule_sets: self.rule_sets.read().clone(),
            probes: self.probes.read().clone(),
        }
    }

    /// Write the current nodes, settings and rules to disk.
    pub fn persist(&self) -> AppResult<()> {
        self.store.save(&self.document())
    }

    /// Remember one latency result and write it out.
    pub fn record_probe(
        &self,
        id: &NodeId,
        delay_ms: Option<u32>,
        kind: ProbeKind,
    ) -> AppResult<()> {
        self.record_probes(vec![(id.clone(), NodeProbe::measured(delay_ms, kind))])
    }

    /// Remember several results with a single write.
    ///
    /// A full sweep produces one record per node; rewriting the document per
    /// node would turn one cheap operation into dozens of file replacements.
    pub fn record_probes(&self, records: Vec<(NodeId, NodeProbe)>) -> AppResult<()> {
        if records.is_empty() {
            return Ok(());
        }
        {
            let mut probes = self.probes.write();
            for (id, probe) in records {
                probes.insert(id, probe);
            }
        }
        self.persist()
    }

    /// Drop records from memory only; the caller writes the document once it
    /// has finished the rest of its change (node deletion, for example).
    pub fn drop_probes(&self, ids: &[NodeId]) {
        let mut probes = self.probes.write();
        for id in ids {
            probes.remove(id);
        }
    }

    /// Replace everything in memory, then write it. Used by restore.
    ///
    /// Callers must stop the kernel first: a running kernel was rendered from
    /// the old settings, so swapping the state under it would leave the two
    /// disagreeing about ports and rules.
    pub fn replace(&self, document: Document) -> AppResult<()> {
        *self.nodes.write() = document.nodes;
        *self.settings.write() = document.settings;
        *self.rules.write() = document.rules;
        *self.subscriptions.write() = document.subscriptions;
        *self.groups.write() = document.groups;
        *self.rule_sets.write() = document.rule_sets;
        *self.profiles.write() = document.profiles;
        *self.active.write() = document.active;
        *self.probes.write() = document.probes;
        self.persist()
    }

    /// Contents of the active profile, straight from the inline fields.
    pub fn active_parts(&self) -> ProfileParts {
        ProfileParts {
            nodes: self.nodes.read().clone(),
            rules: self.rules.read().clone(),
            subscriptions: self.subscriptions.read().clone(),
            groups: self.groups.read().clone(),
            rule_sets: self.rule_sets.read().clone(),
        }
    }

    /// Swap the inline contents for another profile's, in memory only.
    ///
    /// The caller decides when to write: a switch saves the profile it is
    /// leaving first, and only then moves the contents over, so a failed write
    /// cannot leave the two profiles sharing one body.
    pub fn set_active_parts(&self, parts: ProfileParts) {
        *self.nodes.write() = parts.nodes;
        *self.rules.write() = parts.rules;
        *self.subscriptions.write() = parts.subscriptions;
        *self.groups.write() = parts.groups;
        *self.rule_sets.write() = parts.rule_sets;
    }

    /// Contents of one profile: inline for the active one, its file otherwise.
    ///
    /// `None` means the file is missing, which the list shows as a profile with
    /// no nodes rather than an error the user cannot act on.
    pub fn profile_parts(&self, id: &str) -> AppResult<Option<ProfileParts>> {
        if *self.active.read() == id {
            return Ok(Some(self.active_parts()));
        }
        Ok(self.store.read_profile(id)?.map(|profile| profile.parts))
    }

    /// Write one profile's contents to its file. `name` travels with it so the
    /// file is readable on its own.
    pub fn save_profile_parts(
        &self,
        id: &str,
        name: &str,
        parts: &ProfileParts,
    ) -> AppResult<()> {
        self.store.write_profile(&StoredProfile {
            id: id.to_string(),
            name: name.to_string(),
            parts: parts.clone(),
        })
    }

    /// Remove one profile's file. Called when it becomes active (the inline
    /// fields become its only copy) and when it is deleted.
    pub fn drop_profile_file(&self, id: &str) -> AppResult<()> {
        self.store.remove_profile(id)
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Fallback location used only when no explicit path was provided.
fn default_store_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("state")
        .join("store.json")
}

/// Rule-set lists live beside the document, in their own folder.
fn rule_sets_dir(store: &Store) -> std::path::PathBuf {
    store
        .path()
        .parent()
        .map(|dir| dir.join("rulesets"))
        .unwrap_or_else(|| std::path::PathBuf::from("rulesets"))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::domain::node::{Protocol, Security, Transport};

    fn scratch() -> std::path::PathBuf {
        static SEQ: AtomicUsize = AtomicUsize::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("state-{}-{}", std::process::id(), seq));
        std::fs::create_dir_all(&dir).unwrap();
        dir
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

    #[test]
    fn state_survives_a_restart() {
        let dir = scratch();
        let path = dir.join("store.json");

        {
            let state = AppState::load(Store::new(&path));
            state.nodes.write().push(node());
            state.settings.write().mixed_port = 7999;
            state.persist().unwrap();
        }

        let reloaded = AppState::load(Store::new(&path));
        assert_eq!(reloaded.nodes.read().len(), 1);
        assert_eq!(reloaded.nodes.read()[0].name, "东京");
        assert_eq!(reloaded.settings.read().mixed_port, 7999);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_probe_and_a_failed_probe_both_survive_a_restart() {
        let dir = scratch();
        let path = dir.join("store.json");
        {
            let state = AppState::load(Store::new(&path));
            state.nodes.write().push(node());
            state
                .record_probe(&"n1".to_string(), Some(120), ProbeKind::Tcp)
                .unwrap();
            // A failure is recorded as `None`, never as an absent entry.
            state
                .record_probe(&"n2".to_string(), None, ProbeKind::Real)
                .unwrap();
        }

        let reloaded = AppState::load(Store::new(&path));
        let probes = reloaded.probes.read();
        assert_eq!(probes.len(), 2);
        assert_eq!(probes["n1"].delay_ms, Some(120));
        assert_eq!(probes["n1"].kind, ProbeKind::Tcp);
        assert_eq!(probes["n2"].delay_ms, None);
        assert_eq!(probes["n2"].kind, ProbeKind::Real);
        drop(probes);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn dropping_a_probe_removes_it_from_memory_and_disk() {
        let dir = scratch();
        let path = dir.join("store.json");
        let state = AppState::load(Store::new(&path));
        state.nodes.write().push(node());
        state
            .record_probe(&"n1".to_string(), Some(80), ProbeKind::Tcp)
            .unwrap();

        state.drop_probes(&["n1".to_string()]);
        state.persist().unwrap();
        assert!(state.probes.read().is_empty());

        let reloaded = AppState::load(Store::new(&path));
        assert!(reloaded.probes.read().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_broken_file_still_opens_the_app() {
        let dir = scratch();
        let path = dir.join("store.json");
        std::fs::write(&path, "garbage").unwrap();

        let state = AppState::load(Store::new(&path));
        assert!(state.nodes.read().is_empty());
        assert_eq!(state.settings.read().mixed_port, 7890);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_non_active_profile_reads_from_its_own_file() {
        let dir = scratch();
        let path = dir.join("store.json");
        let state = AppState::load(Store::new(&path));

        // The inline fields belong to the active profile only.
        let other = ProfileParts {
            nodes: vec![node()],
            ..ProfileParts::default()
        };
        state.save_profile_parts("p-other", "公司", &other).unwrap();
        let read = state.profile_parts("p-other").unwrap().expect("its file is there");
        assert_eq!(read.nodes.len(), 1);
        assert_eq!(read.nodes[0].name, "东京");
        assert!(state.active_parts().nodes.is_empty());

        // And the active id answers from memory instead of a file that is not
        // supposed to exist.
        let active = state.active.read().clone();
        state.set_active_parts(other);
        assert_eq!(state.profile_parts(&active).unwrap().unwrap().nodes.len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }
}
