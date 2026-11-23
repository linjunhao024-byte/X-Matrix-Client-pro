mod commands;
mod diagnostics;
mod documents;
mod export;
mod domain;
mod engine;
mod error;
mod hotkeys;
pub mod ipc;
mod import;
mod net;
mod rulesets;
mod state;
mod autostart;
mod store;
mod sysproxy;
mod tray;
mod update;

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
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
        // `--quit` is the second process being useful: it is the only way to ask
        // for a clean exit on Windows, where `taskkill /F` skips the tray
        // removal and leaves a dead icon behind.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            if wants_quit(&argv) {
                quit_cleanly(app);
                return;
            }
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
            // One line per minute, kept for a week: the live chart forgets
            // everything on restart, this does not.
            app.manage(std::sync::Arc::new(crate::engine::TrafficLog::new(
                base.join("traffic.jsonl"),
            )));
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

            // A `--quit` launch with nothing running has nothing to stop, but it
            // must not settle in: no window left on screen, no tray icon
            // registered. Its only job is to make sure no stale proxy snapshot
            // or kernel outlives it, so it leaves through the same clean path.
            // The window already exists here — Tauri builds configured windows
            // before this hook runs — so it is hidden rather than skipped.
            if wants_quit(std::env::args()) {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
                quit_cleanly(app.handle());
                return Ok(());
            }

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

            // Both chords are fixed for now. A shortcut the user cannot find is
            // the same as no shortcut, so the settings page lists them.
            // The chords live on their own message thread (see `hotkeys`):
            // the plugin route registered them but never received the events.
            install_hotkeys(app.handle());

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
        .build(tauri::generate_context!())
        .expect("error while building X-Matrix")
        .run(|app, event| {
            // Every normal way out has to take the icon away, including the
            // ones that never reach `quit_cleanly`: a session logout, a window
            // destroyed by the OS. Hiding an icon that is already hidden costs
            // nothing.
            if matches!(
                event,
                tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
            ) {
                remove_tray(app);
            }
        });
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
                // The tray was built before this thread ran, so its toggle row
                // still says "start"; the kernel is now up.
                refresh_tray(&app);
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
        let running = app
            .try_state::<std::sync::Arc<EngineRuntime>>()
            .map(|runtime| runtime.status().running)
            .unwrap_or(false);
        for id in due {
            match commands::subscription::refresh_one_blocking(&state, &id, running) {
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

/// The tray icon and its menu.
///
/// The icon is held in managed state on purpose: the icon leaves the
/// notification area when its last handle is dropped, so a local binding would
/// make the tray disappear the moment setup returned. The slot is an `Option`
/// so the exit path can take the icon out and drop it while the process is
/// still alive — that drop is the only thing that issues `NIM_DELETE`, whereas
/// `set_visible(false)` just tells the shell to hide a button it keeps.
struct TraySlot(Mutex<Option<TrayIcon<tauri::Wry>>>);

fn build_tray(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    // One line per process lifetime, so a log that shows two rows is proof that
    // something really did build a second icon.
    static TRAY_BUILDS: AtomicUsize = AtomicUsize::new(0);
    let build = TRAY_BUILDS.fetch_add(1, Ordering::Relaxed) + 1;
    eprintln!("x-matrix: 创建托盘图标 pid={} 第 {} 次", std::process::id(), build);

    let menu = tray::build(app.handle())?;
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
            // Everything else is a node or a proxy mode picked from a submenu.
            other => tray::on_row(app, other),
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

    app.manage(TraySlot(Mutex::new(Some(tray))));
    set_tray_tooltip(app.handle());
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
    let Some(tray) = app.tray_by_id("main") else {
        return;
    };

    // Rebuild rather than patch: the check marks on the node rows and the
    // start/stop wording all move together, and a half-updated menu is how you
    // end up clicking "stop" while the kernel is already stopped.
    match tray::build(app) {
        Ok(menu) => {
            let _ = tray.set_menu(Some(menu));
        }
        Err(error) => eprintln!("x-matrix: 托盘菜单重建失败：{error}"),
    }

    set_tray_tooltip(app);
}

/// The tooltip is the only part of the tray that is not rebuilt: it carries the
/// live node name and port, which change without the menu changing.
fn set_tray_tooltip(app: &AppHandle) {
    let Some(tray) = app.tray_by_id("main") else {
        return;
    };
    let status = app
        .try_state::<Arc<EngineRuntime>>()
        .map(|runtime| runtime.status())
        .unwrap_or_default();
    let tooltip = if status.running {
        format!("X-Matrix · {} · :{}", status.node_name, status.mixed_port)
    } else {
        "X-Matrix · 未连接".to_string()
    };
    let _ = tray.set_tooltip(Some(tooltip));
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

/// Wires both chords to the window: start/stop, and show/hide.
///
/// Registration is best effort — another proxy client may already own the
/// combination, and an app that refuses to start over a shortcut would be
/// worse than one without them. The failure is only logged.
fn install_hotkeys(app: &AppHandle) {
    let app = app.clone();
    hotkeys::install(move |chord| match chord {
        hotkeys::Chord::Toggle => toggle_kernel(&app),
        hotkeys::Chord::Window => toggle_window(&app),
    });
}

/// Bring the window back from the tray (or from minimised).
fn reveal(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Hide the window without touching the tunnel — the whole reason this runs
/// from a global chord is that the user is working in another app.
fn toggle_window(app: &AppHandle) {
    let Some(window) = app.get_webview_window("main") else {
        eprintln!("x-matrix: 显隐快捷键找不到主窗口");
        return;
    };
    eprintln!(
        "x-matrix: 显隐窗口 visible={:?} minimized={:?}",
        window.is_visible(),
        window.is_minimized()
    );
    let hidden = !window.is_visible().unwrap_or(true) || window.is_minimized().unwrap_or(false);
    if hidden {
        reveal(app);
    } else {
        let _ = window.hide();
    }
}

/// Whether this process was asked to quit rather than to run.
///
/// The flag is matched exactly: a path or a node name that merely contains
/// `--quit` must not turn a normal launch into a shutdown.
fn wants_quit<S: AsRef<str>>(args: impl IntoIterator<Item = S>) -> bool {
    args.into_iter().any(|arg| arg.as_ref() == "--quit")
}

/// Take the icon out of the notification area for good.
///
/// `set_visible(false)` is not enough: it asks the shell to hide a button it
/// keeps, so every restart leaves one more invisible entry behind, and Windows
/// keeps a dead icon in the tray until the pointer sweeps over it. A row of
/// those looks like the app spawned several copies. `TrayIcon` is reference
/// counted and the platform icon is deleted only when the last clone is
/// dropped, so both handles are released here: the one in managed state and
/// the one the resource table registered.
fn remove_tray(app: &AppHandle) {
    if let Some(slot) = app.try_state::<TraySlot>() {
        if let Ok(mut held) = slot.0.lock() {
            drop(held.take());
        }
    }
    drop(app.remove_tray_by_id("main"));
}

/// Set while an exit is under way. The window close path, the tray Quit row and
/// a `--quit` process talking to the running instance can all reach
/// `quit_cleanly` within the same moment; only the first one does the work.
static QUITTING: AtomicBool = AtomicBool::new(false);

/// Quit on purpose: give the system proxy back and stop the kernel *before*
/// the process disappears, rather than relying on destructors at exit.
fn quit_cleanly(app: &AppHandle) {
    // The Windows exit path and the `--quit` path both land here, so the work
    // happens once and the second caller just returns.
    if QUITTING.swap(true, Ordering::SeqCst) {
        return;
    }
    if let Some(guard) = app.try_state::<Arc<ProxyGuard>>() {
        let _ = guard.disable();
    }
    if let Some(runtime) = app.try_state::<Arc<EngineRuntime>>() {
        let _ = runtime.stop();
    }
    remove_tray(app);
    // `Shell_NotifyIconW(NIM_DELETE)` is a cross-process call into explorer and
    // the shell retires the button asynchronously: exiting in the same tick can
    // leave the icon behind. A short pause is enough for it to be processed.
    std::thread::sleep(std::time::Duration::from_millis(150));
    app.exit(0);
}

#[cfg(windows)]
fn windows_proxy() -> sysproxy::WindowsProxy {
    sysproxy::WindowsProxy::new()
}

#[cfg(test)]
mod tests {
    use super::wants_quit;

    #[test]
    fn a_quit_launch_is_recognised() {
        assert!(wants_quit(["X-Matrix.exe", "--quit"]));
        assert!(wants_quit(vec!["X-Matrix.exe".to_string(), "--quit".to_string()]));
    }

    #[test]
    fn a_plain_launch_does_not_quit() {
        assert!(!wants_quit(["X-Matrix.exe"]));
        assert!(!wants_quit(Vec::<String>::new()));
        assert!(!wants_quit(["X-Matrix.exe", "--quit-later"]));
    }

    #[test]
    fn a_quit_launch_elsewhere_on_the_command_line_is_not_the_flag() {
        assert!(!wants_quit([r"C:\--quit\x.exe"]));
        assert!(!wants_quit(["--quitter"]));
    }
}
