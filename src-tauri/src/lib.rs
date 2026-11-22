mod commands;
mod documents;
mod export;
mod domain;
mod engine;
mod error;
pub mod ipc;
mod import;
mod net;
mod rulesets;
mod state;
mod autostart;
mod store;
mod sysproxy;

use std::sync::Arc;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WindowEvent};

use engine::{EngineKind, EngineRuntime};
use state::AppState;
use store::Store;
use sysproxy::{ProxyGuard, SystemProxy};

/// Emit `src/services/bindings.ts` from the contract. Called by
/// `scripts/gen-contract.mjs` through the app binary (see `main.rs`).
pub fn export_contract(path: impl AsRef<std::path::Path>) -> Result<(), Box<dyn std::error::Error>> {
    ipc::builder().export(specta_typescript::Typescript::default(), path)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let specta_builder = ipc::builder();

    tauri::Builder::default()
        // First plugin on purpose: a second launch must hand its arguments to the
        // running instance and exit before it builds its own window and tray.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            reveal(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        // Remember where the user put the window. Only the three flags that
        // describe geometry: `visible` is ours (the "start hidden" setting),
        // and restoring a hidden/decorated state from disk would fight it.
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::SIZE
                        | tauri_plugin_window_state::StateFlags::POSITION
                        | tauri_plugin_window_state::StateFlags::MAXIMIZED,
                )
                .build(),
        )
        .setup(|app| {
            // Everything the engine layer needs lives under one app-owned
            // directory: kernels in `engines/`, generated configs in `configs/`,
            // kernel stdout/stderr in `logs/`.
            let base = app.path().app_data_dir()?;
            let engines_dir = base.join("engines");
            let config_dir = base.join("configs");
            let log_dir = base.join("logs");
            for dir in [&engines_dir, &config_dir, &log_dir] {
                std::fs::create_dir_all(dir)?;
            }
            app.manage(AppState::load(Store::new(base.join("store.json"))));
            app.manage(Arc::new(EngineRuntime::new(
                engines_dir,
                config_dir,
                log_dir,
            )));

            // If a snapshot survived the last run, that run died while holding
            // the system proxy. Put the user's settings back before doing
            // anything else.
            let guard = ProxyGuard::new(
                Box::new(windows_proxy()) as Box<dyn SystemProxy>,
                base.join("proxy-snapshot.json"),
            );
            match guard.recover() {
                Ok(true) => eprintln!("x-matrix: 已恢复上次异常退出前的系统代理设置"),
                Ok(false) => {}
                Err(error) => eprintln!("x-matrix: 恢复系统代理设置失败：{error}"),
            }
            app.manage(Arc::new(guard));

            // A tray that failed to appear must not make the window
            // unclosable: closing only hides when there is a tray to come back
            // from. Otherwise the user would have no way out.
            let tray_ok = match build_tray(app) {
                Ok(()) => true,
                Err(error) => {
                    eprintln!("x-matrix: 托盘创建失败，关窗将直接退出：{error}");
                    false
                }
            };
            app.manage(TrayAvailable(tray_ok));

            // Subscription sweep: one thread, one minute per tick. It only
            // fetches sources that are due, and a source with
            // `update_interval_minutes = 0` is never touched automatically.
            start_subscription_sweep(app.handle().clone());
            auto_connect(app.handle().clone());

            // "Start hidden" is applied after the window exists but before the
            // user sees it: hiding later would flash a window first.
            let start_hidden = app
                .state::<AppState>()
                .settings
                .read()
                .start_minimized;
            if let Some(window) = app.get_webview_window("main") {
                ensure_window_reachable(&window);
                if start_hidden {
                    let _ = window.hide();
                }
            }
            Ok(())
        })
        // What the X button means is a setting, because both readings are
        // legitimate: "get out of the way, keep my tunnel" (tray) and "close
        // the program" (quit, which must also hand the system proxy back).
        // Without a tray there is no way back, so X always quits then.
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let hide = tray_is_up(app) && app.state::<AppState>().settings.read().close_to_tray;
                api.prevent_close();
                if hide {
                    let _ = window.hide();
                } else {
                    quit_cleanly(app);
                }
            }
        })
        .invoke_handler(specta_builder.invoke_handler())
        .run(tauri::generate_context!())
        .expect("error while running X-Matrix");
}

/// Connect on launch, if the user asked for it and the last session still
/// resolves to a node whose kernel is installed.
///
/// Runs on its own thread after a short pause so the window paints first: a
/// kernel booting during the first frame makes the app look frozen. Every
/// reason to skip is logged and silent — a launch that cannot connect must
/// still be a usable launch.
fn auto_connect(app: AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(1200));
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        let settings = state.settings.read().clone();
        if !settings.auto_connect {
            return;
        }
        let Some(node_id) = settings.last_node_id.clone() else {
            eprintln!("x-matrix: 自动连接跳过，还没有用过的节点");
            return;
        };
        let Some(kind) = settings.last_engine.as_deref().and_then(EngineKind::parse) else {
            eprintln!("x-matrix: 自动连接跳过，不知道上次用的是哪个内核");
            return;
        };
        let Some(runtime) = app.try_state::<std::sync::Arc<EngineRuntime>>() else {
            return;
        };
        let Some(proxy) = app.try_state::<std::sync::Arc<ProxyGuard>>() else {
            return;
        };
        if !runtime.binaries().iter().any(|b| b.kind == kind && b.present) {
            eprintln!("x-matrix: 自动连接跳过，内核目录里没有 {}", kind.as_str());
            return;
        }
        match commands::engine::prepare_start(&state, kind, &node_id)
            .and_then(|request| commands::engine::start_engine(&runtime, &proxy, request))
        {
            Ok(status) => {
                eprintln!("x-matrix: 已自动连接 {}", status.node_name);
                commands::engine::remember_session(&state, kind, &node_id);
            }
            Err(error) => eprintln!("x-matrix: 自动连接失败：{error}"),
        }
    });
}

/// Refresh due subscriptions in the background.
///
/// A plain thread, not an async task: `reqwest::blocking` inside is what the
/// refresh path already uses, and one minute of granularity does not need a
/// timer wheel.
fn start_subscription_sweep(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
        let Some(state) = app.try_state::<AppState>() else {
            continue;
        };
        let now = commands::subscription::now_ms();
        let due: Vec<String> = state
            .subscriptions
            .read()
            .iter()
            .filter(|subscription| subscription.is_due(now))
            .map(|subscription| subscription.id.clone())
            .collect();
        for id in due {
            match commands::subscription::refresh_one_blocking(&state, &id) {
                Ok(outcome) => {
                    println!(
                        "x-matrix: 订阅「{}」已更新，{} 个节点",
                        outcome.subscription.name, outcome.imported
                    );
                }
                Err(error) => {
                    eprintln!("x-matrix: 订阅 {id} 更新失败：{error}");
                    let mut subscriptions = state.subscriptions.write();
                    if let Some(existing) = subscriptions.iter_mut().find(|s| s.id == id) {
                        existing.last_error = Some(error.to_string());
                        existing.updated_at_ms = Some(commands::subscription::now_ms());
                    }
                }
            }
        }

        // Rule sets share the tick: they are the same kind of remote document,
        // and a second timer would only be another thing to reason about.
        for (name, outcome) in commands::ruleset::refresh_due_sets(&state, now) {
            match outcome {
                Ok(count) => println!("x-matrix: 规则集「{name}」已更新，{count} 条规则"),
                Err(error) => eprintln!("x-matrix: 规则集「{name}」更新失败：{error}"),
            }
        }
    });
}

/// Whether a tray icon exists, so closing the window can hide instead of quit.
struct TrayAvailable(bool);

fn tray_is_up(app: &AppHandle) -> bool {
    app.try_state::<TrayAvailable>()
        .map(|state| state.0)
        .unwrap_or(false)
}

/// Tray strings follow the saved language. Changing the language therefore
/// needs a restart for the tray to follow — the menu is built once, and
/// rebuilding it on every settings save would be a lot of machinery for a
/// label nobody reads twice.
fn tray_labels(language: crate::domain::settings::Language) -> [&'static str; 4] {
    match language {
        crate::domain::settings::Language::Zh => ["显示窗口", "启动内核", "停止内核", "退出 X-Matrix"],
        crate::domain::settings::Language::En => ["Show window", "Start kernel", "Stop kernel", "Quit X-Matrix"],
    }
}

/// The toggle menu item, kept so its label and the tooltip can follow the
/// kernel state without rebuilding the menu.
pub struct TrayUi {
    toggle: tauri::menu::MenuItem<tauri::Wry>,
    labels: [&'static str; 4],
}

/// The tray icon and its three verbs.
///
/// The icon is kept in managed state on purpose: a `TrayIcon` that is dropped
/// is removed from the notification area, so a local binding would make the
/// tray disappear the moment setup returned.
fn build_tray(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let labels = tray_labels(
        app.try_state::<AppState>()
            .map(|state| state.settings.read().language)
            .unwrap_or(crate::domain::settings::Language::Zh),
    );
    let show = MenuItem::with_id(app, "show", labels[0], true, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle", labels[1], true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", labels[3], true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &toggle, &quit])?;

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or("打包里没有应用图标，托盘做不出来")?;

    let tray = TrayIconBuilder::with_id("main")
        .icon(icon)
        .tooltip("X-Matrix")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => reveal(app),
            "toggle" => toggle_kernel(app),
            "quit" => quit_cleanly(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                reveal(tray.app_handle());
            }
        })
        .build(app)?;

    app.manage(tray);
    app.manage(TrayUi { toggle, labels });
    refresh_tray(app.handle());
    Ok(())
}

/// Start or stop the kernel from the tray.
///
/// Same sequence as the buttons in the window (start = kernel then proxy,
/// stop = proxy then kernel); the only difference is that there is no UI to
/// report an error to, so failures go to the console and the tooltip.
fn toggle_kernel(app: &AppHandle) {
    let Some(runtime) = app.try_state::<Arc<EngineRuntime>>() else {
        return;
    };
    let runtime = Arc::clone(runtime.inner());
    let Some(proxy) = app.try_state::<Arc<ProxyGuard>>() else {
        return;
    };
    let proxy = Arc::clone(proxy.inner());
    let app = app.clone();
    std::thread::spawn(move || {
        let result = if runtime.status().running {
            proxy
                .disable()
                .and_then(|()| runtime.stop())
                .map(|_| ())
        } else {
            match app.try_state::<AppState>() {
                Some(state) => {
                    let settings = state.settings.read().clone();
                    let node_id = settings.last_node_id.clone().unwrap_or_default();
                    let kind = settings
                        .last_engine
                        .as_deref()
                        .and_then(EngineKind::parse)
                        .unwrap_or(EngineKind::Xray);
                    if node_id.is_empty() {
                        Err(crate::error::AppError::InvalidInput(
                            "还没有选过节点，先在窗口里选一个".into(),
                        ))
                    } else {
                        commands::engine::prepare_start(&state, kind, &node_id)
                            .and_then(|request| {
                                commands::engine::start_engine(&runtime, &proxy, request)
                            })
                            .map(|_| ())
                    }
                }
                None => Ok(()),
            }
        };
        if let Err(error) = result {
            eprintln!("x-matrix: 托盘操作失败：{error}");
        }
        refresh_tray(&app);
    });
}

/// Make the toggle label and the tooltip describe what is actually running.
pub fn refresh_tray(app: &AppHandle) {
    let Some(ui) = app.try_state::<TrayUi>() else {
        return;
    };
    let status = app
        .try_state::<Arc<EngineRuntime>>()
        .map(|runtime| runtime.status())
        .unwrap_or_default();
    let label = if status.running { ui.labels[2] } else { ui.labels[1] };
    let _ = ui.toggle.set_text(label);
    if let Some(tray) = app.tray_by_id("main") {
        let tooltip = if status.running {
            format!("X-Matrix · {} · :{}", status.node_name, status.mixed_port)
        } else {
            "X-Matrix · 未连接".to_string()
        };
        let _ = tray.set_tooltip(Some(tooltip));
    }
}

/// Keep a restored window reachable.
///
/// The geometry plugin replays the last position verbatim, and a saved position
/// can point at a monitor that is gone — unplugged dock, projector, VM session
/// with a different resolution. Restoring an invisible window is worse than
/// forgetting where it was, so a window that lands on no monitor at all is
/// re-centred on the primary one.
fn ensure_window_reachable(window: &tauri::WebviewWindow) {
    let Ok(position) = window.outer_position() else {
        return;
    };
    let Ok(size) = window.outer_size() else {
        return;
    };
    let monitors = window.available_monitors().unwrap_or_default();
    if monitors.is_empty() {
        return;
    }
    let overlaps_any = monitors.iter().any(|monitor| {
        let origin = monitor.position();
        let area = monitor.size();
        let left = position.x.max(origin.x);
        let top = position.y.max(origin.y);
        let right = (position.x + size.width as i32).min(origin.x + area.width as i32);
        let bottom = (position.y + size.height as i32).min(origin.y + area.height as i32);
        right > left && bottom > top
    });
    if !overlaps_any {
        let _ = window.center();
    }
}

/// Bring the window back from the tray (or from minimised).
fn reveal(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Quit on purpose: give the system proxy back and stop the kernel *before*
/// the process disappears, rather than relying on destructors at exit.
fn quit_cleanly(app: &AppHandle) {
    if let Some(guard) = app.try_state::<Arc<ProxyGuard>>() {
        let _ = guard.disable();
    }
    if let Some(runtime) = app.try_state::<Arc<EngineRuntime>>() {
        let _ = runtime.stop();
    }
    // Take the icon out of the notification area on the way out. Windows keeps
    // a dead icon in the tray until the pointer sweeps over it, and a row of
    // those looks like the app spawned several copies, so we hide it while we
    // still can.
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_visible(false);
    }
    app.exit(0);
}

#[cfg(windows)]
fn windows_proxy() -> sysproxy::WindowsProxy {
    sysproxy::WindowsProxy::new()
}
