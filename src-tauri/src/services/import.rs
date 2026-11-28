//! Importing share links and subscription documents.
//!
//! Parsing lives in [`crate::import`] and is pure. This module only decides what
//! to do with the result: preview it, or merge it into the node list. The Tauri
//! commands keep only the tray refresh, so the headless core can import the same
//! document over HTTP.

use serde::Serialize;
use specta::Type;

use crate::documents;
use crate::domain::group::ProxyGroup;
use crate::domain::routing::{RuleAction, RuleMatcher, RoutingRule};
use crate::domain::Node;
use crate::error::AppResult;
use crate::import::{self, unique_suffix, ParsedRule};
use crate::state::AppState;

/// What a paste would add, shown to the user before anything is written.
#[derive(Debug, Clone, Serialize, Type)]
pub struct ImportPreview {
    /// Nodes that parsed cleanly. `id`s are already assigned.
    pub nodes: Vec<Node>,
    /// One line per input line that could not be understood.
    pub skipped: Vec<String>,
    /// Names of the proxy groups the document declares.
    ///
    /// A Clash config is not only a server list: its groups say how the nodes
    /// are meant to be used together. The preview names them so the user knows
    /// an import will also write routing, not just nodes.
    pub groups: Vec<String>,
    /// One line per rule the document declares, in file order.
    pub rules: Vec<String>,
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
    /// Proxy groups added.
    #[specta(type = f64)]
    pub groups: usize,
    /// Routing rules added.
    #[specta(type = f64)]
    pub rules: usize,
    pub skipped: Vec<String>,
    /// The full node list after the import.
    pub nodes: Vec<Node>,
}

/// The one argument both import commands take.
#[derive(Debug, serde::Deserialize)]
pub struct TextArgs {
    pub text: String,
}

/// Parse anything the user pasted: share links first, then whole documents.
///
/// `import::parse` understands one link per line, which is what a provider's
/// clipboard button gives you. A file is more likely to be a Clash or sing-box
/// config, and those are already parsed for subscriptions — so the fallback is
/// "if no link was recognised, try it as a document" rather than a second
/// import path in the UI. A pasted config must not silently produce nothing.
///
/// The whole report comes back, groups and rules included: dropping them here
/// would mean the merge step could never see the routing a config shipped with.
fn parse_report(text: &str) -> import::ParseReport {
    let report = import::parse(text);
    if !report.nodes.is_empty() {
        return report;
    }
    let document = documents::parse_document(text);
    if document.nodes.is_empty() {
        return report;
    }
    document
}

fn preview_of(report: import::ParseReport) -> ImportPreview {
    ImportPreview {
        groups: report.groups.iter().map(|group| group.name.clone()).collect(),
        rules: report.rules.iter().map(describe_rule).collect(),
        nodes: report.nodes,
        skipped: report.skipped,
    }
}

/// One rule, as a line a person can read in the preview.
fn describe_rule(rule: &ParsedRule) -> String {
    let value = match &rule.matcher {
        RuleMatcher::Port { value } => value.to_string(),
        other => other.value().to_string(),
    };
    format!("{} {value} → {}", rule.matcher.label(), rule.target)
}

/// Parse without touching state.
pub fn import_preview(args: TextArgs) -> ImportPreview {
    preview_of(parse_report(&args.text))
}

/// Parse and merge into the node list, then persist.
pub fn import_nodes(state: &AppState, args: TextArgs) -> AppResult<ImportOutcome> {
    let report = parse_report(&args.text);
    let mut skipped = report.skipped;

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

    // Groups first: rules point at them, so a rule imported before its group
    // would have to be rewritten afterwards.
    let groups_added = {
        let mut groups = state.groups.write();
        let mut added = 0;
        for parsed in &report.groups {
            if groups.iter().any(|existing| existing.name == parsed.name) {
                skipped.push(format!("策略组「{}」已经存在，没有覆盖", parsed.name));
                continue;
            }
            // Members are names in the document and ids here. A name that is
            // not in the list is a node the document mentions but we could not
            // parse, and a group that silently shrank is worse than one that
            // says so.
            let members: Vec<String> = parsed
                .members
                .iter()
                .filter_map(|name| snapshot.iter().find(|node| &node.name == name))
                .map(|node| node.id.clone())
                .collect();
            if members.is_empty() {
                skipped.push(format!("策略组「{}」的成员都不在导入结果里，已跳过", parsed.name));
                continue;
            }
            if members.len() < parsed.members.len() {
                // Clash lets a group hold `DIRECT` and `REJECT` as members: they
                // are outbounds of the config, not nodes of this app, so the
                // group keeps only the nodes that made it in. Name the ones that
                // did not, otherwise "1 member was skipped" reads like a bug.
                let missing: Vec<&str> = parsed
                    .members
                    .iter()
                    .filter(|name| !snapshot.iter().any(|node| &&node.name == name))
                    .map(String::as_str)
                    .collect();
                skipped.push(format!(
                    "策略组「{}」的 {} 个成员没能导入（{}），已略过",
                    parsed.name,
                    missing.len(),
                    missing.join("、")
                ));
            }
            groups.push(ProxyGroup {
                id: format!("group-{}", unique_suffix()),
                name: parsed.name.clone(),
                enabled: true,
                kind: parsed.kind,
                members,
                selected: None,
            });
            added += 1;
        }
        added
    };

    let rules_added = {
        let mut rules = state.rules.write();
        if report.rules.is_empty() {
            0
        } else if !rules.is_empty() {
            // Rules are evaluated top to bottom, so appending someone else's
            // table would put every imported rule after the user's catch-alls
            // and change nothing — or worse, reorder what they built.
            skipped.push(format!(
                "已经有时 {} 条规则，配置里的 {} 条没有导入（不覆盖你排好的顺序）",
                rules.len(),
                report.rules.len()
            ));
            0
        } else {
            let groups = state.groups.read();
            let mut added = Vec::new();
            for parsed in &report.rules {
                let action = match parsed.target.as_str() {
                    "DIRECT" => RuleAction::Direct,
                    "REJECT" | "REJECT-DROP" => RuleAction::Block,
                    "PROXY" => RuleAction::Proxy,
                    name => match groups.iter().find(|group| group.name == name) {
                        Some(group) => RuleAction::Group { group_id: group.id.clone() },
                        None => {
                            skipped.push(format!(
                                "规则指向「{name}」，它不是本次导入的策略组，已跳过"
                            ));
                            continue;
                        }
                    },
                };
                let value = match &parsed.matcher {
                    RuleMatcher::Port { value } => value.to_string(),
                    other => other.value().to_string(),
                };
                added.push(RoutingRule {
                    id: format!("rule-{}", unique_suffix()),
                    name: format!("{} {value}", parsed.matcher.label()),
                    enabled: true,
                    matcher: parsed.matcher.clone(),
                    action,
                });
            }
            let count = added.len();
            rules.extend(added);
            count
        }
    };

    if imported > 0 || groups_added > 0 || rules_added > 0 {
        state.persist()?;
    }

    Ok(ImportOutcome {
        imported,
        duplicates,
        groups: groups_added,
        rules: rules_added,
        skipped,
        nodes: snapshot,
    })
}