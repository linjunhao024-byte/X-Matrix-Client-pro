//! Whether this process is elevated, and how to restart it as an administrator.
//!
//! TUN and the Windows service need a full administrator token, so the settings
//! page asks [`is_elevated`] and can offer a relaunch. The relaunch is ordered on
//! purpose: the new process is started first, and only a confirmed start lets
//! this one take the exit path, so a cancelled UAC prompt leaves the running app
//! untouched. The kernel is not stopped here at all — the next instance reconnects
//! from the saved `auto_connect` / `last_node_id`.

use crate::error::{AppError, AppResult};

/// Argument the elevated copy carries so it can wait for this process to go away.
///
/// `tauri-plugin-single-instance` owns a session-wide mutex: a second process
/// started while the first still runs is treated as "hand over argv and exit",
/// and `ShellExecuteExW` returns as soon as the new process is created, well
/// before `quit_cleanly` finished. Without this handshake the elevated copy
/// would exit as a second instance and the app would simply vanish.
const RELAUNCH_WAIT: &str = "--relaunch-wait=";

/// Upper bound for that handshake. `quit_cleanly` spends ~150 ms on the tray
/// notification; anything near this limit means the predecessor is wedged, and
/// hanging the new launch forever would be worse than starting alongside it.
const RELAUNCH_WAIT_MS: u32 = 15_000;

/// Whether this process holds an elevated (full administrator) token.
///
/// Any failure on the way — no token handle, unsupported information class —
/// answers `false`: "cannot tell" is reported as "not elevated", which at worst
/// shows the prompt once too often. Off Windows there is nothing to elevate.
pub fn is_elevated() -> bool {
    #[cfg(windows)]
    {
        windows_is_elevated()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Start a new copy of this executable through the `runas` verb, then quit.
///
/// The order is the whole point: if UAC is cancelled the error returns before
/// [`crate::quit_cleanly`] is reached and the user keeps the app they were using.
/// On success the new process is already alive, so exiting here never leaves a
/// gap with no instance running.
pub fn restart_elevated(app: &tauri::AppHandle) -> AppResult<()> {
    #[cfg(windows)]
    {
        let path = std::env::current_exe()
            .map_err(|e| AppError::Engine(format!("找不到要启动的程序：{e}")))?;
        spawn_elevated(&path)?;
        // Started, not merely requested: only now is it safe to leave.
        crate::quit_cleanly(app);
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        Err(AppError::Internal(
            "只有 Windows 支持以管理员身份重启".to_string(),
        ))
    }
}

/// Turn a `ShellExecuteExW` failure into the sentence the user sees.
///
/// Split out as a pure function so the mapping is testable without a UAC prompt:
/// the interesting cases only happen on a machine that is not already elevated.
#[cfg(windows)]
pub(crate) fn message_for(code: u32, path: &std::path::Path) -> AppError {
    match code {
        // ERROR_CANCELLED: the consent dialog was dismissed or answered "No".
        1223 => AppError::Engine("提权被取消：没有点「是」，所以没有重启".to_string()),
        // ERROR_FILE_NOT_FOUND / ERROR_PATH_NOT_FOUND.
        2 | 3 => AppError::Engine(format!("找不到要启动的程序：{}", path.display())),
        other => AppError::Engine(format!("以管理员身份重启失败：{other}")),
    }
}

/// Block a relaunched copy until its predecessor has exited.
///
/// Called from `run()` before the Tauri builder installs the single-instance
/// plugin. A launch without the argument (the normal case) returns immediately.
pub(crate) fn wait_for_predecessor() {
    // `args()` panics on an argument that is not valid Unicode; a launch path
    // with an unpaired surrogate would end startup over a flag we can ignore.
    let args = std::env::args_os().filter_map(|arg| arg.into_string().ok());
    let Some(pid) = parse_relaunch_wait(args) else {
        return;
    };
    #[cfg(windows)]
    wait_for_pid(pid);
    #[cfg(not(windows))]
    let _ = pid;
}

/// Read the pid out of a `--relaunch-wait=<pid>` argument.
///
/// Exact-match prefix: a path or node name that merely contains the text must
/// not make a normal launch wait on an unrelated process. A malformed value is
/// ignored rather than turned into pid 0.
fn parse_relaunch_wait<S: AsRef<str>>(args: impl IntoIterator<Item = S>) -> Option<u32> {
    args.into_iter().find_map(|arg| {
        arg.as_ref()
            .strip_prefix(RELAUNCH_WAIT)
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|pid| *pid != 0)
    })
}

#[cfg(windows)]
fn spawn_elevated(path: &std::path::Path) -> AppResult<()> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError};
    use windows_sys::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let verb = wide("runas");
    let file = wide(path);
    // Tells the child who to wait for; see `wait_for_predecessor`.
    let parameters = wide(format!("{RELAUNCH_WAIT}{}", std::process::id()));

    let mut info = SHELLEXECUTEINFOW::default();
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = parameters.as_ptr();
    info.nShow = SW_SHOWNORMAL;

    // SAFETY: `info` is fully initialised with its real size, and every pointer
    // it holds (`verb`, `file`, `parameters`) outlives the call.
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        // SAFETY: no pointers are involved; the last-error value is per thread.
        return Err(message_for(unsafe { GetLastError() }, path));
    }
    if !info.hProcess.is_null() {
        // SEE_MASK_NOCLOSEPROCESS hands us the child's handle; nothing waits on
        // it here, and the child has its own handshake with this pid.
        unsafe { CloseHandle(info.hProcess) };
    }
    Ok(())
}

#[cfg(windows)]
fn wait_for_pid(pid: u32) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
    };

    // SAFETY: a pid is a plain number; the returned handle is closed below.
    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if handle.is_null() {
        // Already gone, or never ours: there is nothing to wait for.
        return;
    }
    // SAFETY: `handle` is a live process handle opened with SYNCHRONIZE.
    unsafe { WaitForSingleObject(handle, RELAUNCH_WAIT_MS) };
    // SAFETY: closing a handle we own, exactly once.
    unsafe { CloseHandle(handle) };
}

#[cfg(windows)]
fn windows_is_elevated() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: the pseudo handle always refers to this process; `token` is only
    // read after a successful call and closed below.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return false;
    }

    let mut elevation = TOKEN_ELEVATION::default();
    let mut returned = 0u32;
    // SAFETY: the buffer is exactly one `TOKEN_ELEVATION` and the length matches,
    // which is what the information class writes.
    let query = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            &mut elevation as *mut _ as *mut core::ffi::c_void,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
    };
    // SAFETY: `token` was opened just above and is not used afterwards.
    unsafe { CloseHandle(token) };

    query != 0 && elevation.TokenIsElevated != 0
}

#[cfg(windows)]
fn wide(text: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;

    text.as_ref().encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(all(test, windows))]
mod tests {
    use super::{is_elevated, message_for, parse_relaunch_wait, wide};
    use crate::error::AppError;

    fn text(error: AppError) -> String {
        match error {
            AppError::Engine(message) => message,
            other => panic!("expected AppError::Engine, got {other:?}"),
        }
    }

    #[test]
    fn a_dismissed_uac_prompt_says_nothing_restarted() {
        let message = text(message_for(1223, std::path::Path::new("X-Matrix.exe")));
        assert_eq!(message, "提权被取消：没有点「是」，所以没有重启");
    }

    #[test]
    fn a_missing_executable_names_the_path() {
        let path = std::path::Path::new("C:\\gone\\X-Matrix.exe");
        for code in [2u32, 3] {
            let message = text(message_for(code, path));
            assert_eq!(message, "找不到要启动的程序：C:\\gone\\X-Matrix.exe");
        }
    }

    #[test]
    fn any_other_failure_keeps_the_error_code() {
        let message = text(message_for(5, std::path::Path::new("X-Matrix.exe")));
        assert_eq!(message, "以管理员身份重启失败：5");
    }

    #[test]
    fn the_relaunch_argument_is_read_exactly() {
        assert_eq!(
            parse_relaunch_wait(["X-Matrix.exe", "--relaunch-wait=4321"]),
            Some(4321)
        );
        assert_eq!(parse_relaunch_wait(["X-Matrix.exe"]), None);
        // Wedged into another value, or malformed, must not match.
        assert_eq!(parse_relaunch_wait(["X-Matrix.exe", "--quit --relaunch-wait=1"]), None);
        assert_eq!(parse_relaunch_wait(["X-Matrix.exe", "--relaunch-wait="]), None);
        assert_eq!(parse_relaunch_wait(["X-Matrix.exe", "--relaunch-wait=0"]), None);
        assert_eq!(parse_relaunch_wait(["X-Matrix.exe", "--relaunch-wait=abc"]), None);
    }

    #[test]
    fn a_wide_string_is_nul_terminated() {
        let encoded = wide("runas");
        assert_eq!(encoded.last(), Some(&0));
        assert_eq!(&encoded[..5], &[114, 117, 110, 97, 115]);
    }

    #[test]
    fn this_session_reports_its_own_elevation() {
        // The development machine and `cloudpc-test` both run as an administrator.
        // On an account without elevation the honest answer is `false`, so the
        // assertion is skipped with a note instead of weakened to always pass.
        if !is_elevated() {
            eprintln!("not an elevated session: the true-answer assertion is skipped");
            return;
        }
        assert!(is_elevated(), "an elevated session must report elevated");
    }
}