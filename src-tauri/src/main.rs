// Prevents an extra console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Headless core: no Tauri, no window, no tray. Entered before `run()` so
    // nothing in the GUI path can create a window in a `--serve` process.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--serve") {
        if let Err(error) = x_matrix_lib::core::serve(&args) {
            eprintln!("x-matrix core: 启动失败：{error}");
            std::process::exit(1);
        }
        return;
    }

    // Contract generation runs inside the app binary on purpose: a separate
    // `example` target fails to load on some Windows builds (loader entry-point
    // mismatch), while the app binary always links correctly.
    if let Some(path) = std::env::args().find_map(|a| a.strip_prefix("--export-contract=").map(str::to_owned)) {
        if let Err(e) = x_matrix_lib::export_contract(path) {
            eprintln!("contract export failed: {e}");
            std::process::exit(1);
        }
        return;
    }

    x_matrix_lib::run()
}
