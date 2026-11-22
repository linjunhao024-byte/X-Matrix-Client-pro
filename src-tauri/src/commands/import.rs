//! Importing share links and subscription documents.
//!
//! Parsing lives in [`crate::import`] and is pure. This module only decides what
//! to do with the result: preview it, or merge it into the node list.

use serde::Serialize;
use specta::Type;
use tauri::State;

use crate::documents;
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

/// Parse anything the user pasted: share links first, then whole documents.
///
/// `import::parse` understands one link per line, which is what a provider's
/// clipboard button gives you. A file is more likely to be a Clash or sing-box
/// config, and those are already parsed for subscriptions — so the fallback is
/// "if no link was recognised, try it as a document" rather than a second
/// import path in the UI. A pasted config must not silently produce nothing.
fn parse_anything(text: &str) -> ImportPreview {
    let report = import::parse(text);
    if !report.nodes.is_empty() {
        return ImportPreview {
            nodes: report.nodes,
            skipped: report.skipped,
        };
    }
    let document = documents::parse_document(text);
    if document.nodes.is_empty() {
        return ImportPreview {
            nodes: Vec::new(),
            skipped: report.skipped,
        };
    }
    ImportPreview {
        nodes: document.nodes,
        skipped: document.skipped,
    }
}

/// Parse without touching state.
#[tauri::command]
#[specta::specta]
pub fn import_preview(text: String) -> ImportPreview {
    parse_anything(&text)
}

/// Parse and merge into the node list, then persist.
#[tauri::command]
#[specta::specta]
pub fn import_nodes(state: State<'_, AppState>, text: String) -> AppResult<ImportOutcome> {
    let report = parse_anything(&text);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_share_link_still_takes_the_link_path() {
        let text = "vless://11111111-1111-1111-1111-111111111111@a.example.com:443?encryption=none&type=tcp&security=none#one";
        let preview = parse_anything(text);
        assert_eq!(preview.nodes.len(), 1);
        assert_eq!(preview.nodes[0].name, "one");
    }

    #[test]
    fn a_clash_file_is_imported_as_a_document() {
        // A raw string keeps the YAML readable: escapes here would be the only
        // thing under test.
        let text = r#"proxies:
  - name: clash-one
    type: ss
    server: b.example.com
    port: 8388
    cipher: aes-128-gcm
    password: secret
"#;
        let preview = parse_anything(text);
        assert_eq!(preview.nodes.len(), 1, "a Clash file must not import as nothing");
        assert_eq!(preview.nodes[0].name, "clash-one");
    }

    fn a_sing_box_file_is_imported_as_a_document() {
        let text = r#"{"outbounds":[{"type":"trojan","tag":"t","server":"c.example.com","server_port":443,"password":"pw"}]}"#;
        let preview = parse_anything(text);
        assert_eq!(preview.nodes.len(), 1);
        assert_eq!(preview.nodes[0].server, "c.example.com");
    }

    #[test]
    fn plain_prose_imports_nothing_instead_of_guessing() {
        let preview = parse_anything("hello, this is not a config");
        assert!(preview.nodes.is_empty());
    }
}
