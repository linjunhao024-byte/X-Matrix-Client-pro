//! Importing share links and subscription documents.
//!
//! Parsing lives in [`crate::import`] and is pure. This module only decides what
//! to do with the result: preview it, or merge it into the node list.

use serde::Serialize;
use specta::Type;
use tauri::State;

use crate::domain::Node;
use crate::error::AppResult;
use crate::import;
use crate::state::AppState;

/// What a paste would add, shown to the user before anything is written.
#[derive(Debug, Clone, Serialize, Type)]
pub struct ImportPreview {
    /// Nodes that parsed cleanly. `id`s are already assigned.
    pub nodes: Vec<Node>,
    /// One line per input line that could not be understood.
    pub skipped: Vec<String>,
}

/// What an import actually changed.
#[derive(Debug, Clone, Serialize, Type)]
pub struct ImportOutcome {
    /// Nodes added to the list.
    ///
    /// `f64` on the wire: specta refuses to export `usize`, and a node count is
    /// nowhere near the 2^53 where doubles stop counting exactly.
    #[specta(type = f64)]
    pub imported: usize,
    /// Nodes that were already present (same name + server + port).
    #[specta(type = f64)]
    pub duplicates: usize,
    pub skipped: Vec<String>,
    /// The full node list after the import.
    pub nodes: Vec<Node>,
}

/// Parse without touching state.
#[tauri::command]
#[specta::specta]
pub fn import_preview(text: String) -> ImportPreview {
    let report = import::parse(&text);
    ImportPreview {
        nodes: report.nodes,
        skipped: report.skipped,
    }
}

/// Parse and merge into the node list, then persist.
#[tauri::command]
#[specta::specta]
pub fn import_nodes(state: State<'_, AppState>, text: String) -> AppResult<ImportOutcome> {
    let report = import::parse(&text);

    let (snapshot, imported, duplicates) = {
        let mut nodes = state.nodes.write();
        let mut imported = 0;
        let mut duplicates = 0;
        for node in report.nodes {
            // Re-importing the same subscription is the common case; do not
            // turn it into a pile of duplicates. Name + server + port is what
            // the user sees in the table, so that is what identifies a node.
            let exists = nodes
                .iter()
                .any(|n| n.name == node.name && n.server == node.server && n.port == node.port);
            if exists {
                duplicates += 1;
                continue;
            }
            nodes.push(node);
            imported += 1;
        }
        (nodes.clone(), imported, duplicates)
    };

    if imported > 0 {
        state.persist()?;
    }

    Ok(ImportOutcome {
        imported,
        duplicates,
        skipped: report.skipped,
        nodes: snapshot,
    })
}
