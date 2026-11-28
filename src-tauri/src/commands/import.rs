//! Importing share links and subscription documents.
//!
//! Parsing lives in [`crate::import`] and is pure; the merge logic lives in
//! [`crate::services::import`], shared with the headless core's HTTP channel.
//! This file keeps the command contract and the tray refresh.

use tauri::{AppHandle, State};

use crate::error::AppResult;
use crate::services::import as svc;
use crate::state::AppState;

pub use crate::services::import::{ImportOutcome, ImportPreview};

/// Parse without touching state.
#[tauri::command]
#[specta::specta]
pub fn import_preview(text: String) -> ImportPreview {
    svc::import_preview(svc::TextArgs { text })
}

/// Parse and merge into the node list, then persist.
#[tauri::command]
#[specta::specta]
pub fn import_nodes(
    app: AppHandle,
    state: State<'_, AppState>,
    text: String,
) -> AppResult<ImportOutcome> {
    let outcome = svc::import_nodes(&state, svc::TextArgs { text })?;

    // A paste that changed nothing (all duplicates, no groups, no rules) must
    // not cost a tray rebuild: the counts already say what happened.
    if outcome.imported > 0 || outcome.groups > 0 || outcome.rules > 0 {
        crate::refresh_tray(&app);
    }
    Ok(outcome)
}
#[cfg(test)]
mod tests {
    use super::*;

    /// Everything goes through the command's own service entry, so a regression
    /// in the routing between "share link" and "document" is caught here rather
    /// than by the user pasting a Clash file and getting an empty list.
    fn preview(text: &str) -> ImportPreview {
        svc::import_preview(svc::TextArgs { text: text.to_string() })
    }

    #[test]
    fn a_share_link_still_takes_the_link_path() {
        let text = "vless://11111111-1111-1111-1111-111111111111@a.example.com:443?encryption=none&type=tcp&security=none#one";
        let preview = preview(text);
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
        let preview = preview(text);
        assert_eq!(preview.nodes.len(), 1, "a Clash file must not import as nothing");
        assert_eq!(preview.nodes[0].name, "clash-one");
    }

    #[test]
    fn a_sing_box_file_is_imported_as_a_document() {
        let text = r#"{"outbounds":[{"type":"trojan","tag":"t","server":"c.example.com","server_port":443,"password":"pw"}]}"#;
        let preview = preview(text);
        assert_eq!(preview.nodes.len(), 1);
        assert_eq!(preview.nodes[0].server, "c.example.com");
    }

    #[test]
    fn the_preview_names_the_groups_and_rules_a_config_carries() {
        // The user has to see "this import will also write routing" before
        // pressing the button, not discover it afterwards in the rules page.
        let text = r#"proxies:
  - name: one
    type: ss
    server: a.example.com
    port: 8388
    cipher: aes-128-gcm
    password: secret
proxy-groups:
  - name: 手动
    type: select
    proxies: [one]
rules:
  - DOMAIN-SUFFIX,example.com,手动
  - REJECT,geoip:cn,DIRECT
"#;
        let preview = preview(text);
        assert_eq!(preview.nodes.len(), 1);
        assert_eq!(preview.groups, vec!["手动".to_string()]);
        assert_eq!(preview.rules.len(), 1, "rules: {:?}", preview.rules);
        assert_eq!(preview.rules[0], "域名后缀 example.com → 手动");
    }

    #[test]
    fn plain_prose_imports_nothing_instead_of_guessing() {
        let preview = preview("hello, this is not a config");
        assert!(preview.nodes.is_empty());
    }
}
