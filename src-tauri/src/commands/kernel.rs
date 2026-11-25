//! Kernel updates — the only place that downloads a binary and puts it where
//! the runtime will spawn it.
//!
//! Three rules shape the flow:
//!
//! 1. Nothing in `engines/` changes until the new binary has run
//!    `<incoming> version` and printed a line. A truncated download, a wrong
//!    architecture or a zip that unpacked to something else all fail there,
//!    while the working kernel is still untouched.
//! 2. The old file is moved to `<name>.old`, never deleted. A failed update
//!    leaves a bootable kernel behind; the user can put it back by hand.
//! 3. A kernel that was running is stopped first (proxy handed back before the
//!    port disappears) and started again on the same node afterwards. If the
//!    restart fails the new binary stays: rolling back would trade a working
//!    file for a broken one, which is the worse of the two outcomes.
//!
//! The download shares the subscription fetch path, so the "through the tunnel"
//! setting applies here too — a machine that can only reach GitHub through the
//! running kernel can still fetch the next kernel.

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, State};

use crate::engine::release::{self, ReleaseInfo};
use crate::engine::EngineKind;
use crate::error::{AppError, AppResult};
use crate::net::remote;
use crate::state::AppState;

use super::engine::{prepare_start, run_blocking, start_engine, Proxy, Runtime};

/// A 17–33 MB zip on a slow link. The budget is for the download, not for
/// hanging the UI: the whole flow runs on a blocking thread.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(120);

/// The release JSON and the `.dgst` are a few hundred bytes; they get a short
/// budget so a dead network fails in seconds, not minutes.
const METADATA_TIMEOUT: Duration = Duration::from_secs(30);

/// What the settings page needs to draw one kernel row: the newest official
/// release, where it comes from, and what this machine has right now.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct KernelRelease {
    pub kind: EngineKind,
    /// Upstream tag as published (`v26.11.9`).
    pub tag: String,
    /// The tag without its `v` — the spelling the UI compares.
    pub version: String,
    pub url: String,
    /// `f64` because specta cannot carry `u64`; the same rule `GeoAssetStatus`
    /// follows for byte counts.
    pub bytes: f64,
    /// Version the installed binary reports, or `None` when there is none to
    /// ask. A missing kernel is not an error here — "not installed yet" is a
    /// state the settings page must be able to show.
    pub current: Option<String>,
    /// Something the caller should say out loud: e.g. the fetch went direct
    /// because the kernel was not running. `None` when the route was the one
    /// the settings asked for.
    pub note: Option<String>,
}

/// The outcome of one update, one field per question the UI asks about it.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct KernelUpdate {
    pub kind: EngineKind,
    pub version: String,
    pub bytes: f64,
    /// sha256 of the downloaded archive — the same value that was compared
    /// against upstream, so a later "was this the file we checked" question has
    /// an answer.
    pub sha256: String,
    /// True only when an upstream sha256 was fetched *and* matched. No digest
    /// means `false`, never "the file looked fine".
    pub verified: bool,
    /// Whether there was a previous binary to move aside.
    pub replaced: bool,
    /// Whether a running kernel was stopped and brought back on the same node.
    pub restarted: bool,
    pub note: Option<String>,
}

/// Where the download should go, and what to tell the user about the route.
///
/// This is the same rule the subscription fetch uses, deliberately: one setting
/// (`subscription_via_proxy`) says "this machine can only reach the outside
/// world through the tunnel", and a kernel update is exactly the other moment
/// where that matters. When the setting is on but nothing is running the fetch
/// still goes direct and says so, because silently choosing a different route
/// than the one the user asked for is the kind of surprise that ends in a
/// support ticket.
fn fetch_route(state: &AppState, engine_running: bool) -> (Option<u16>, Option<String>) {
    let settings = state.settings.read();
    if !settings.subscription_via_proxy {
        return (None, None);
    }
    if engine_running {
        // An HTTP client needs the HTTP inbound; with a split the configured
        // mixed port has no listener. `resolve_port` is the same `0 = follow
        // mixed_port` rule the renderer and the takeover use.
        let port = crate::engine::Endpoints::resolve_port(settings.mixed_port, settings.http_port);
        return (Some(port), None);
    }
    (
        None,
        Some("订阅设置为走代理，但内核没有运行，这一次是直连拉的".to_string()),
    )
}

/// Fetch `releases/latest` and reduce it to the asset this build can use.
fn fetch_release(kind: EngineKind, proxy: Option<u16>) -> AppResult<ReleaseInfo> {
    let (status, body) = remote::fetch_bytes_through(
        release::feed_url(kind),
        METADATA_TIMEOUT,
        proxy,
        "内核发布页",
    )?;
    if !(200..300).contains(&status) {
        return Err(AppError::Engine(format!("内核发布页返回 HTTP {status}")));
    }
    release::parse_release(kind, &String::from_utf8_lossy(&body))
}

/// Run `<path> version` and return the first non-empty line.
///
/// The staging file is asked directly, before it has a chance to become the
/// kernel: this is the one check that proves the download is an executable this
/// machine can actually start.
fn probe_version(path: &Path) -> AppResult<String> {
    let output = Command::new(path)
        .arg("version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| AppError::Io(format!("{} 跑不起来：{error}", path.display())))?;
    if !output.status.success() {
        let code = output.status.code().map(|code| code.to_string()).unwrap_or_else(|| "未知".into());
        return Err(AppError::Engine(format!(
            "{} version 退出码 {code}",
            path.display()
        )));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
        .ok_or_else(|| AppError::Engine(format!("{} version 没有任何输出", path.display())))
}

/// What a finished download phase has to hand back to the replace phase.
struct Staged {
    info: ReleaseInfo,
    sha256: String,
    verified: bool,
    notes: Vec<String>,
}

/// Download, verify and unpack one release into `engines/` as a staged file.
///
/// Everything up to and including the version probe happens here; the caller
/// only has to decide whether the running kernel may be stopped for the swap.
fn download_and_stage(
    kind: EngineKind,
    dir: &Path,
    binary_name: &str,
    proxy: Option<u16>,
) -> AppResult<Staged> {
    let info = fetch_release(kind, proxy)?;
    let mut notes: Vec<String> = Vec::new();

    let (status, zip_bytes) = remote::fetch_bytes_through(
        &info.url,
        DOWNLOAD_TIMEOUT,
        proxy,
        "内核压缩包",
    )?;
    if !(200..300).contains(&status) {
        return Err(AppError::Engine(format!("内核压缩包返回 HTTP {status}")));
    }
    if zip_bytes.is_empty() {
        return Err(AppError::Engine("内核压缩包是空的（0 字节）".into()));
    }
    let sha256 = release::sha256_hex(&zip_bytes);

    // Verification is best-effort *only* in the sense that a missing digest
    // does not block the update; a digest that exists and disagrees does, and
    // it blocks before anything in `engines/` is touched.
    let mut verified = false;
    match info.digest_url.as_deref() {
        Some(digest_url) => {
            match remote::fetch_bytes_through(digest_url, METADATA_TIMEOUT, proxy, "内核校验文件") {
                Ok((status, body)) if (200..300).contains(&status) => {
                    match release::parse_digest(&String::from_utf8_lossy(&body)) {
                        Some(expected) if expected == sha256 => verified = true,
                        Some(expected) => {
                            let expected_short = expected.get(..12).unwrap_or(&expected);
                            let actual_short = sha256.get(..12).unwrap_or(&sha256);
                            return Err(AppError::Engine(format!(
                                "sha256 不符，已丢弃这份下载、没有替换任何文件：上游 {expected_short}…，下载到的是 {actual_short}…"
                            )));
                        }
                        None => notes.push("上游的校验文件里没有 SHA2-256 这一行".to_string()),
                    }
                }
                Ok((status, _)) => {
                    notes.push(format!("上游的校验文件没取到（HTTP {status}）"));
                }
                Err(error) => {
                    notes.push(format!("上游的校验文件没取到（{error}）"));
                }
            }
        }
        None => notes.push("上游没有提供 sha256 校验文件".to_string()),
    }

    let exe = release::extract_exe(&zip_bytes, binary_name)?;
    let incoming = release::stage(dir, binary_name, &exe)?;
    if let Err(error) = probe_version(&incoming) {
        // The old file was never touched, so discarding the staged one is all
        // that is needed to leave the machine exactly as it was.
        release::discard(dir, binary_name);
        return Err(AppError::Engine(format!(
            "新内核跑不起来，已保留原文件：{error}"
        )));
    }
    Ok(Staged {
        info,
        sha256,
        verified,
        notes,
    })
}

/// Check what upstream has, and what this machine has, without touching either.
#[tauri::command]
#[specta::specta]
pub async fn kernel_check(
    state: State<'_, AppState>,
    runtime: Runtime<'_>,
    kind: EngineKind,
) -> AppResult<KernelRelease> {
    let running = runtime.status().running;
    let (proxy_port, route_note) = fetch_route(&state, running);
    let runtime = Arc::clone(runtime.inner());
    run_blocking(move || {
        let info = fetch_release(kind, proxy_port)?;
        // The kernels print a sentence, not a version ("Xray 26.11.9 (…)").
        // Normalise it so the UI compares numbers, but keep the raw line when
        // the format is not one this build recognises — showing the truth is
        // better than showing nothing.
        let current = runtime
            .version(kind)
            .ok()
            .map(|line| release::version_from_self_report(&line).unwrap_or(line));
        Ok(KernelRelease {
            kind,
            tag: info.tag,
            version: info.version,
            url: info.url,
            bytes: info.bytes,
            current,
            note: route_note,
        })
    })
    .await
}

/// Download the newest official kernel and replace the one in `engines/`.
#[tauri::command]
#[specta::specta]
pub async fn kernel_install(
    app: AppHandle,
    state: State<'_, AppState>,
    runtime: Runtime<'_>,
    proxy: Proxy<'_>,
    kind: EngineKind,
) -> AppResult<KernelUpdate> {
    // Refuse before the network: with a custom binary path the file in
    // `engines/` is not the file the app runs, so "update the kernel" would
    // replace something nobody uses and leave the real kernel stale. Costing
    // the user a 17 MB download just to say so would be rude.
    let custom = {
        let settings = state.settings.read();
        match kind {
            EngineKind::Xray => settings.xray_binary.clone(),
            EngineKind::SingBox => settings.singbox_binary.clone(),
        }
    };
    if !custom.trim().is_empty() {
        return Err(AppError::InvalidInput(format!(
            "{} 用的是自定义内核路径（{}）；这个按钮只更新内核目录里的官方文件，先用「恢复默认」再更新",
            kind.as_str(),
            custom.trim()
        )));
    }

    let (proxy_port, route_note) = fetch_route(&state, runtime.status().running);
    let dir = runtime.engines_dir().to_path_buf();
    let binary_name = kind.engine().binary_name();
    let mut notes: Vec<String> = Vec::new();
    if let Some(note) = route_note {
        notes.push(note);
    }

    let staged = {
        let dir = dir.clone();
        run_blocking(move || download_and_stage(kind, &dir, binary_name, proxy_port)).await?
    };
    notes.extend(staged.notes.iter().cloned());

    // Re-read the runtime: the download can take a minute and the user may have
    // started or stopped a kernel meanwhile. What is true now is what gets
    // stopped — not what was true when the download began.
    let before = runtime.status();
    let stop_this = before.running && before.kind == Some(kind);
    let node_id = before.node_id.clone();

    // Rebuilding the session is decided before anything is stopped. If the
    // node the running kernel uses has since been deleted, there is nothing to
    // restart on: replacing the binary would leave a stopped kernel and a
    // binary the user never asked to run. Refuse, and keep the old file.
    let mut restart_request = None;
    if stop_this {
        let id = node_id.clone().unwrap_or_default();
        if id.is_empty() {
            release::discard(&dir, binary_name);
            return Err(AppError::Engine(
                "内核正在运行，但状态里没有节点 id，不敢在换内核后让它起不来；这次没有替换任何文件"
                    .into(),
            ));
        }
        match prepare_start(&state, kind, &id) {
            Ok(request) => restart_request = Some(request),
            Err(error) => {
                release::discard(&dir, binary_name);
                return Err(AppError::Engine(format!(
                    "内核在运行，但重启它要用的配置读不出来，这次没有替换任何文件：{error}"
                )));
            }
        }
    }

    let runtime_swap = Arc::clone(runtime.inner());
    let proxy_swap = Arc::clone(proxy.inner());
    let dir_swap = dir.clone();
    let replaced = {
        let dir = dir_swap;
        run_blocking(move || {
            let result = (|| -> AppResult<bool> {
                if stop_this {
                    // The order `engine_stop` uses: hand the proxy back before
                    // the port it points at disappears.
                    proxy_swap.disable()?;
                    runtime_swap.stop()?;
                }
                release::promote(&dir, binary_name)
            })();
            if result.is_err() {
                // A failed swap must not leave a staged binary for the next
                // attempt to trip over.
                release::discard(&dir, binary_name);
            }
            result
        })
        .await?
    };

    let mut restarted = false;
    if let Some(request) = restart_request {
        let runtime_start = Arc::clone(runtime.inner());
        let proxy_start = Arc::clone(proxy.inner());
        match run_blocking(move || start_engine(&runtime_start, &proxy_start, request)).await {
            Ok(_) => restarted = true,
            Err(error) => notes.push(format!("替换成功，但内核没起来：{error}")),
        }
    }

    // The tray's toggle says what the next click does; a swap changes that.
    crate::refresh_tray(&app);

    Ok(KernelUpdate {
        kind,
        version: staged.info.version,
        bytes: staged.info.bytes,
        sha256: staged.sha256,
        verified: staged.verified,
        replaced,
        restarted,
        note: if notes.is_empty() {
            None
        } else {
            Some(notes.join("；"))
        },
    })
}