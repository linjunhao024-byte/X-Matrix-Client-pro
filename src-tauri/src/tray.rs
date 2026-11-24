//! The tray menu: what it lists, and what a click on each row does.
//!
//! Every state change rebuilds the menu instead of patching rows in place.
//! A subscription can add nodes at any moment, and a menu holding stale ids is
//! worse than a menu built twice — building it is a few dozen allocations and
//! only happens when the node list, the mode, or the kernel state changed.
//!
//! The rows are produced as plain data first ([`node_rows`] / [`mode_rows`])
//! because a tray menu cannot be inspected in a test, and the interesting part
//! (which node is checked, which ids exist) is not the menu widget itself.

use std::sync::Arc;

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Manager, Wry};

use crate::domain::settings::{Language, ProxyMode};
use crate::engine::EngineKind;
use crate::engine::EngineRuntime;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::sysproxy::ProxyGuard;

/// A subscription can carry hundreds of nodes; a menu taller than the screen
/// is unusable, so the tray lists the first few and the window lists all.
pub const NODE_LIMIT: usize = 20;

/// One row of the menu, as data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub label: String,
    pub checked: bool,
}

/// Menu wording follows the saved language. Like the rest of the tray, it is
/// read when the menu is built, so a language change reaches it on the next
/// rebuild (which any state change triggers).
pub struct Labels {
    pub show: &'static str,
    pub start: &'static str,
    pub stop: &'static str,
    pub quit: &'static str,
    pub nodes: &'static str,
    pub modes: &'static str,
    pub empty_nodes: &'static str,
    pub rule: &'static str,
    pub global: &'static str,
    pub direct: &'static str,
}

pub fn labels(language: Language) -> Labels {
    match language {
        Language::Zh => Labels {
            show: "显示窗口",
            start: "启动内核",
            stop: "停止内核",
            quit: "退出 X-Matrix",
            nodes: "节点",
            modes: "代理模式",
            empty_nodes: "还没有节点",
            rule: "规则",
            global: "全局",
            direct: "直连",
        },
        Language::ZhTw => Labels {
            show: "顯示視窗",
            start: "啟動內核",
            stop: "停止內核",
            quit: "結束 X-Matrix",
            nodes: "節點",
            modes: "代理模式",
            empty_nodes: "還沒有節點",
            rule: "規則",
            global: "全域",
            direct: "直連",
        },
        Language::En => Labels {
            show: "Show window",
            start: "Start kernel",
            stop: "Stop kernel",
            quit: "Quit X-Matrix",
            nodes: "Nodes",
            modes: "Proxy mode",
            empty_nodes: "No nodes yet",
            rule: "Rule",
            global: "Global",
            direct: "Direct",
        },
    }
}

impl Labels {
    fn mode(&self, mode: ProxyMode) -> &'static str {
        match mode {
            ProxyMode::Rule => self.rule,
            ProxyMode::Global => self.global,
            ProxyMode::Direct => self.direct,
        }
    }
}

/// `(id, name)` pairs, in the order the window lists them.
pub fn node_rows(nodes: &[(String, String)], current: Option<&str>) -> Vec<Row> {
    nodes
        .iter()
        .take(NODE_LIMIT)
        .map(|(id, name)| Row {
            id: format!("node:{id}"),
            label: name.clone(),
            // Comparing through `Some` because a node that was never started —
            // or one that was deleted while the tray was up — leaves no check
            // mark anywhere, which is the honest picture.
            checked: current == Some(id.as_str()),
        })
        .collect()
}

pub fn mode_rows(language: Language, mode: ProxyMode) -> Vec<Row> {
    let labels = labels(language);
    [ProxyMode::Rule, ProxyMode::Global, ProxyMode::Direct]
        .into_iter()
        .map(|each| Row {
            id: format!("mode:{}", slug(each)),
            label: labels.mode(each).to_string(),
            checked: each == mode,
        })
        .collect()
}

fn slug(mode: ProxyMode) -> &'static str {
    match mode {
        ProxyMode::Rule => "rule",
        ProxyMode::Global => "global",
        ProxyMode::Direct => "direct",
    }
}

fn parse_mode(text: &str) -> Option<ProxyMode> {
    match text {
        "rule" => Some(ProxyMode::Rule),
        "global" => Some(ProxyMode::Global),
        "direct" => Some(ProxyMode::Direct),
        _ => None,
    }
}

/// Build the whole menu from the current state.
pub fn build(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let (labels, settings, nodes) = {
        let state = app.state::<AppState>();
        let settings = state.settings.read().clone();
        let nodes: Vec<(String, String)> = state
            .nodes
            .read()
            .iter()
            .map(|node| (node.id.clone(), node.name.clone()))
            .collect();
        (labels(settings.language), settings, nodes)
    };

    let running = app
        .try_state::<Arc<EngineRuntime>>()
        .map(|runtime| runtime.status().running)
        .unwrap_or(false);

    // One line per build: a tray menu cannot be inspected from a test, and
    // "what did the tray believe at that moment" is the first question when a
    // row does the wrong thing.
    eprintln!(
        "x-matrix: 托盘菜单 {} 个节点 · 模式 {} · 内核{}",
        nodes.len(),
        slug(settings.mode),
        if running { "运行中" } else { "未运行" }
    );

    let show = MenuItem::with_id(app, "show", labels.show, true, None::<&str>)?;
    let toggle = MenuItem::with_id(
        app,
        "toggle",
        if running { labels.stop } else { labels.start },
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", labels.quit, true, None::<&str>)?;

    let node_items: Vec<CheckMenuItem<Wry>> = node_rows(&nodes, settings.last_node_id.as_deref())
        .into_iter()
        .map(|row| CheckMenuItem::with_id(app, row.id, row.label, true, row.checked, None::<&str>))
        .collect::<tauri::Result<Vec<_>>>()?;
    let nodes_menu = if node_items.is_empty() {
        // An empty submenu looks broken; a disabled row says why.
        let empty = MenuItem::with_id(app, "noop", labels.empty_nodes, false, None::<&str>)?;
        Submenu::with_items(app, labels.nodes, true, &[&empty as &dyn tauri::menu::IsMenuItem<Wry>])?
    } else {
        let refs: Vec<&dyn tauri::menu::IsMenuItem<Wry>> =
            node_items.iter().map(|item| item as &dyn tauri::menu::IsMenuItem<Wry>).collect();
        Submenu::with_items(app, labels.nodes, true, &refs)?
    };

    let mode_items: Vec<CheckMenuItem<Wry>> = mode_rows(settings.language, settings.mode)
        .into_iter()
        .map(|row| CheckMenuItem::with_id(app, row.id, row.label, true, row.checked, None::<&str>))
        .collect::<tauri::Result<Vec<_>>>()?;
    let mode_refs: Vec<&dyn tauri::menu::IsMenuItem<Wry>> =
        mode_items.iter().map(|item| item as &dyn tauri::menu::IsMenuItem<Wry>).collect();
    let modes_menu = Submenu::with_items(app, labels.modes, true, &mode_refs)?;

    let separator = PredefinedMenuItem::separator(app)?;

    Menu::with_items(
        app,
        &[
            &show,
            &toggle,
            &separator,
            &nodes_menu,
            &modes_menu,
            &separator,
            &quit,
        ],
    )
}

enum Target {
    Node(String),
    Mode(ProxyMode),
}

/// Handle a row that is not one of the three fixed verbs.
///
/// Menu callbacks run on the event loop, so the work (stopping and starting a
/// kernel) is handed to a thread — blocking here would freeze the tray.
pub fn on_row(app: &AppHandle, id: &str) {
    let target = if let Some(node) = id.strip_prefix("node:") {
        Some(Target::Node(node.to_string()))
    } else {
        id.strip_prefix("mode:")
            .and_then(parse_mode)
            .map(Target::Mode)
    };
    let Some(target) = target else { return };
    let app = app.clone();
    std::thread::spawn(move || {
        if let Err(error) = apply(&app, target) {
            eprintln!("x-matrix: 托盘切换失败：{error}");
        }
        crate::refresh_tray(&app);
    });
}

/// Save the choice, then make the running kernel match it.
fn apply(app: &AppHandle, target: Target) -> AppResult<()> {
    let Some(state) = app.try_state::<AppState>() else {
        return Ok(());
    };
    let Some(runtime) = app.try_state::<Arc<EngineRuntime>>() else {
        return Ok(());
    };
    let Some(proxy) = app.try_state::<Arc<ProxyGuard>>() else {
        return Ok(());
    };
    let runtime = Arc::clone(runtime.inner());
    let proxy = Arc::clone(proxy.inner());

    // What the menu showed, before this click changed anything.
    let (was_node, was_mode) = {
        let settings = state.settings.read();
        (settings.last_node_id.clone(), settings.mode)
    };

    match &target {
        Target::Node(id) => {
            let known = state.nodes.read().iter().any(|node| &node.id == id);
            if !known {
                return Err(AppError::NotFound(format!("节点 {id} 已经不在了")));
            }
            state.settings.write().last_node_id = Some(id.clone());
        }
        Target::Mode(mode) => {
            state.settings.write().mode = *mode;
        }
    }
    state.persist()?;

    // Nothing running: the choice is saved and the next start picks it up.
    if !runtime.status().running {
        return Ok(());
    }

    // Clicking the row that is already in effect must not bounce the kernel:
    // a restart drops every live connection, and a check-marked row is exactly
    // the one a user is most likely to click twice.
    let unchanged = match &target {
        Target::Node(id) => was_node.as_deref() == Some(id.as_str()),
        Target::Mode(mode) => was_mode == *mode,
    };
    if unchanged {
        return Ok(());
    }

    let settings = state.settings.read().clone();
    let kind = settings
        .last_engine
        .as_deref()
        .and_then(EngineKind::parse)
        .unwrap_or(EngineKind::Xray);
    let Some(node_id) = settings.last_node_id.clone() else {
        return Ok(());
    };
    let request = crate::commands::engine::prepare_start(&state, kind, &node_id)?;
    // Stop before start: one kernel owns the ports, so the new one cannot bind
    // while the old one still holds them. The system proxy is left alone —
    // it keeps pointing at the same mixed port, which is what makes the switch
    // invisible to the browser.
    runtime.stop()?;
    crate::commands::engine::start_engine(&runtime, &proxy, request)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nodes() -> Vec<(String, String)> {
        vec![
            ("a".into(), "AAA".into()),
            ("b".into(), "BBB".into()),
        ]
    }

    #[test]
    fn every_language_names_the_menu_in_its_own_words() {
        // The three menus must actually differ: a missing arm would fall back to
        // English or Chinese and look like a half-translated tray.
        let zh = labels(Language::Zh);
        let tw = labels(Language::ZhTw);
        let en = labels(Language::En);
        assert_eq!(zh.show, "显示窗口");
        assert_eq!(tw.show, "顯示視窗");
        assert_eq!(en.show, "Show window");
        assert_eq!(tw.global, "全域");
        assert_eq!(tw.quit, "結束 X-Matrix");
        // Nothing may repeat another language's word for the same slot.
        assert_ne!(tw.start, zh.start);
        assert_ne!(tw.start, en.start);
    }

    #[test]
    fn the_current_node_is_the_one_with_a_check_mark() {
        let rows = node_rows(&nodes(), Some("b"));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, "node:a");
        assert_eq!(rows[0].label, "AAA");
        assert!(!rows[0].checked);
        assert!(rows[1].checked);
    }

    #[test]
    fn no_current_node_checks_nothing() {
        let rows = node_rows(&nodes(), None);
        assert!(rows.iter().all(|row| !row.checked));
    }

    #[test]
    fn a_long_subscription_is_cut_at_the_limit() {
        let many: Vec<(String, String)> = (0..NODE_LIMIT + 5)
            .map(|i| (format!("n{i}"), format!("node {i}")))
            .collect();
        assert_eq!(node_rows(&many, None).len(), NODE_LIMIT);
    }

    #[test]
    fn mode_rows_check_exactly_one_and_carry_the_slug() {
        let rows = mode_rows(Language::Zh, ProxyMode::Global);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].id, "mode:global");
        assert!(rows[1].checked);
        assert!(!rows[0].checked && !rows[2].checked);
    }

    #[test]
    fn every_mode_slug_parses_back() {
        for mode in [ProxyMode::Rule, ProxyMode::Global, ProxyMode::Direct] {
            assert_eq!(parse_mode(slug(mode)), Some(mode));
        }
        assert_eq!(parse_mode("nonsense"), None);
    }
}
