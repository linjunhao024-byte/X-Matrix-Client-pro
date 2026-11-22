//! Application state. Every field is the SINGLE owner of its data
//! (ARCHITECTURE.md §3). Nothing outside this module may write to it directly.
//!
//! Runtime state is deliberately *not* here: the engine runtime owns the
//! process handle and is managed as its own `Arc<EngineRuntime>`, so the
//! "what is running right now" question has exactly one answer.
//!
//! Durable state lives here, and every mutation goes through [`AppState::persist`]
//! so the file on disk can never drift from memory.

use parking_lot::RwLock;

use crate::domain::{Node, ProxyGroup, RoutingRule, RuleSet, Settings, Subscription};
use crate::error::AppResult;
use crate::rulesets::RuleSetCache;
use crate::store::{Document, Store, SCHEMA_VERSION};

pub struct AppState {
    pub nodes: RwLock<Vec<Node>>,
    pub settings: RwLock<Settings>,
    pub rules: RwLock<Vec<RoutingRule>>,
    pub subscriptions: RwLock<Vec<Subscription>>,
    pub groups: RwLock<Vec<ProxyGroup>>,
    pub rule_sets: RwLock<Vec<RuleSet>>,
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

        Self {
            nodes: RwLock::new(document.nodes),
            settings: RwLock::new(document.settings),
            rules: RwLock::new(document.rules),
            subscriptions: RwLock::new(document.subscriptions),
            groups: RwLock::new(document.groups),
            rule_sets: RwLock::new(document.rule_sets),
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
            nodes: self.nodes.read().clone(),
            settings: self.settings.read().clone(),
            rules: self.rules.read().clone(),
            subscriptions: self.subscriptions.read().clone(),
            groups: self.groups.read().clone(),
            rule_sets: self.rule_sets.read().clone(),
        }
    }

    /// Write the current nodes, settings and rules to disk.
    pub fn persist(&self) -> AppResult<()> {
        self.store.save(&self.document())
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
        self.persist()
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
    fn a_broken_file_still_opens_the_app() {
        let dir = scratch();
        let path = dir.join("store.json");
        std::fs::write(&path, "garbage").unwrap();

        let state = AppState::load(Store::new(&path));
        assert!(state.nodes.read().is_empty());
        assert_eq!(state.settings.read().mixed_port, 7890);
        std::fs::remove_dir_all(&dir).ok();
    }
}
