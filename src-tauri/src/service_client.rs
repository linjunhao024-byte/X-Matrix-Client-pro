//! The GUI side of the kernel service (S4b).
//!
//! Everything here is "write a job file, wait for its result file" — see
//! [`crate::service`] for why the channel is files instead of a pipe. The one
//! thing worth reading twice is [`probe`]: the GUI only delegates when the
//! service is *installed and running*, and it asks the SCM every time instead
//! of caching, so stopping the service falls back to spawning the kernel here
//! without a restart of the app.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::engine::runtime::ProcessHandle;
use crate::engine::EngineKind;
use crate::error::{AppError, AppResult};

const RESULT_TIMEOUT: Duration = Duration::from_secs(10);
const RESULT_POLL: Duration = Duration::from_millis(100);
/// What a client says when its job file was never answered.
const NO_ANSWER: &str = "内核服务没有应答：检查服务是否还在运行（`sc query XMatrixKernel`）";

/// What the SCM says about our service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceStatus {
    pub installed: bool,
    pub running: bool,
    pub pid: Option<u32>,
    pub detail: String,
}

/// The one sentence the settings page shows. Pinned by the task book: four
/// states, nothing else. Extra hints belong in the install command's output,
/// not in this line.
fn describe(installed: bool, running: bool, pid: Option<u32>) -> String {
    match (installed, running, pid) {
        (false, _, _) => "没有安装".to_string(),
        (true, true, Some(pid)) => format!("已安装，正在运行，PID {pid}"),
        (true, true, None) => "已安装，正在运行".to_string(),
        (true, false, _) => "已安装，但没有运行".to_string(),
    }
}

impl ServiceStatus {
    fn missing() -> Self {
        Self {
            installed: false,
            running: false,
            pid: None,
            detail: describe(false, false, None),
        }
    }
}

/// Query the service and its recorded engines directory.
pub fn status() -> ServiceStatus {
    match query_status() {
        Ok(mut status) => {
            status.detail = describe(status.installed, status.running, status.pid);
            status
        }
        Err(error) => ServiceStatus {
            installed: false,
            running: false,
            pid: None,
            detail: format!("安装状态读不到：{error}"),
        },
    }
}

/// Only a running, installed service can host a kernel.
pub fn probe() -> Option<ServiceClient> {
    let status = status();
    if !(status.installed && status.running) {
        return None;
    }
    Some(ServiceClient {
        jobs_dir: crate::service::jobs_dir(),
        engines_dir_hint: crate::service::read_engines_dir()
            .map(|path| path.display().to_string())
            .unwrap_or_default(),
    })
}

/// A job request/response round trip. The two fields are the only things the
/// client knows about the service; the executable path never comes from here.
pub struct ServiceClient {
    jobs_dir: PathBuf,
    engines_dir_hint: String,
}

impl ServiceClient {
    /// Nonce for one request: now in nanoseconds plus the pid.
    pub fn nonce(&self) -> String {
        crate::service::make_nonce()
    }

    pub fn start(&self, kind: EngineKind, config: &str, ready_port: u16) -> AppResult<u32> {
        let nonce = self.nonce();
        let job = serde_json::json!({
            "op": "start",
            "kind": kind.as_str(),
            "ready_port": ready_port,
            "nonce": nonce,
            "config": config,
        });
        let answer = self.round_trip(&nonce, &job.to_string())?;
        match answer.pid {
            Some(pid) if answer.ok => Ok(pid),
            _ => {
                let mut message = answer
                    .error
                    .unwrap_or_else(|| "内核服务没有给出进程号".to_string());
                if message.contains("服务侧没有找到内核") && !self.engines_dir_hint.is_empty() {
                    message.push_str(&format!("（服务登记的内核目录：{}）", self.engines_dir_hint));
                }
                Err(AppError::Engine(message))
            }
        }
    }

    /// `Ok(Some(pid))` stopped one, `Ok(None)` there was nothing to stop.
    pub fn stop(&self, kind: EngineKind) -> AppResult<Option<u32>> {
        let nonce = self.nonce();
        let job = serde_json::json!({ "op": "stop", "kind": kind.as_str(), "nonce": nonce });
        let answer = self.round_trip(&nonce, &job.to_string())?;
        if answer.ok {
            Ok(answer.pid)
        } else {
            Err(AppError::Engine(
                answer
                    .error
                    .unwrap_or_else(|| "内核服务没有说清楚为什么停不掉".to_string()),
            ))
        }
    }

    fn round_trip(&self, nonce: &str, text: &str) -> AppResult<Answer> {
        let paths = crate::service::job_paths(&self.jobs_dir, nonce)?;
        let _ = std::fs::remove_file(&paths.result);
        std::fs::create_dir_all(&self.jobs_dir)?;
        crate::service::write_atomic(&paths.job, text.as_bytes())
            .map_err(|e| AppError::Engine(format!("写不了内核服务作业（{nonce}）：{e}")))?;

        let deadline = Instant::now() + RESULT_TIMEOUT;
        while Instant::now() < deadline {
            if let Ok(raw) = std::fs::read_to_string(&paths.result) {
                let parsed = serde_json::from_str::<Answer>(&raw).map_err(|e| {
                    AppError::Internal(format!("内核服务的回执不是合法 JSON：{e}"))
                })?;
                let _ = std::fs::remove_file(&paths.result);
                return Ok(parsed);
            }
            std::thread::sleep(RESULT_POLL);
        }
        let _ = std::fs::remove_file(&paths.job);
        Err(AppError::Engine(NO_ANSWER.to_string()))
    }
}

#[derive(Debug, Deserialize)]
struct Answer {
    #[serde(default)]
    ok: bool,
    #[serde(default)]
    pid: Option<u32>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    note: Option<String>,
}

/// A kernel owned by the service, presented to [`crate::engine::runtime`] as
/// if it were our own child. `try_wait` asks Windows whether the process is
/// still there, which is the honest answer for a process we cannot wait on.
pub struct ServiceProcess {
    pub pid: u32,
    pub kind: EngineKind,
    pub client: std::sync::Arc<Mutex<ServiceClient>>,
}

use parking_lot::Mutex;

impl ProcessHandle for ServiceProcess {
    fn pid(&self) -> u32 {
        self.pid
    }

    fn try_wait(&mut self) -> AppResult<Option<i32>> {
        if process_is_alive(self.pid) {
            Ok(None)
        } else {
            // We cannot see the exit code of a process we never spawned; -1 is
            // a visible "it died, reason unknown" rather than a fake zero.
            Ok(Some(-1))
        }
    }

    fn kill(&mut self) -> AppResult<()> {
        let client = self.client.lock();
        let _ = client.stop(self.kind);
        // The service may have been stopped under us; do not lie about it.
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if !process_is_alive(self.pid) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Err(AppError::Engine(format!(
            "内核服务说自己停了，但进程 {} 还在",
            self.pid
        )))
    }
}

// ---------------------------------------------------------------- Windows bits

#[cfg(windows)]
mod win {
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_SERVICE_DOES_NOT_EXIST, HANDLE};
    use windows_sys::Win32::System::Services::{
        CloseServiceHandle, OpenSCManagerW, OpenServiceW, QueryServiceStatusEx,
        SC_STATUS_PROCESS_INFO, SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_STATUS_PROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, TerminateProcess, PROCESS_TERMINATE,
    };

    use super::ServiceStatus;
    use crate::elevate::wide;

    pub fn query_status() -> Result<ServiceStatus, String> {
        let scm = unsafe { OpenSCManagerW(std::ptr::null(), std::ptr::null(), 0) };
        if scm.is_null() {
            return Err("打不开服务管理器（OpenSCManager 失败）".to_string());
        }
        let name = wide(crate::service::SERVICE_NAME);
        let service = unsafe { OpenServiceW(scm, name.as_ptr(), SERVICE_QUERY_STATUS) };
        if service.is_null() {
            let code = unsafe { windows_sys::Win32::Foundation::GetLastError() };
            unsafe { CloseServiceHandle(scm) };
            if code == ERROR_SERVICE_DOES_NOT_EXIST {
                return Ok(ServiceStatus::missing());
            }
            return Err(format!("打不开服务（错误 {code}）"));
        }

        let mut status: SERVICE_STATUS_PROCESS = unsafe { std::mem::zeroed() };
        let mut needed = 0u32;
        let ok = unsafe {
            QueryServiceStatusEx(
                service,
                SC_STATUS_PROCESS_INFO,
                &mut status as *mut _ as *mut u8,
                std::mem::size_of::<SERVICE_STATUS_PROCESS>() as u32,
                &mut needed,
            )
        };
        unsafe {
            CloseServiceHandle(service);
            CloseServiceHandle(scm);
        }
        if ok == 0 {
            return Err("服务状态读不出来（QueryServiceStatusEx 失败）".to_string());
        }
        let running = status.dwCurrentState == SERVICE_RUNNING;
        Ok(ServiceStatus {
            installed: true,
            running,
            pid: running.then_some(status.dwProcessId),
            detail: String::new(),
        })
    }

    pub fn process_is_alive(pid: u32) -> bool {
        if pid == 0 {
            return false;
        }
        // A process list from the kernel (Toolhelp) sees SYSTEM processes too;
        // OpenProcess from this ordinary-token GUI can be denied for them, which
        // would turn a live kernel into a fake "it died" report.
        pid_name(pid).is_some()
    }

    /// True when `pid` is one of our kernels. The service only terminates such
    /// processes when cleaning up after its own restart: killing an arbitrary
    /// pid would be a way to make a SYSTEM service murder other programs.
    pub fn process_belongs_to_service_kernel(pid: u32) -> bool {
        let Some(name) = pid_name(pid) else {
            return false;
        };
        let lowered = name.to_ascii_lowercase();
        lowered.ends_with("xray.exe") || lowered.ends_with("sing-box.exe")
    }

    fn pid_name(pid: u32) -> Option<String> {
        crate::sysproc::list()
            .ok()?
            .into_iter()
            .find(|row| row.pids.contains(&pid))
            .map(|row| row.name)
    }

    pub fn terminate(pid: u32) {
        if pid == 0 {
            return;
        }
        let handle: HANDLE = unsafe { OpenProcess(PROCESS_TERMINATE, 0, pid) };
        if handle.is_null() {
            return;
        }
        unsafe {
            TerminateProcess(handle, 1);
            CloseHandle(handle);
        }
    }

}

#[cfg(windows)]
pub use win::{process_belongs_to_service_kernel, process_is_alive, query_status, terminate};

#[cfg(not(windows))]
pub fn process_is_alive(_pid: u32) -> bool {
    false
}

#[cfg(not(windows))]
pub fn process_belongs_to_service_kernel(_pid: u32) -> bool {
    false
}

#[cfg(not(windows))]
pub fn terminate(_pid: u32) {}

#[cfg(not(windows))]
pub fn query_status() -> Result<ServiceStatus, String> {
    Err("只有 Windows 实现了内核服务".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_service_describes_itself() {
        let status = ServiceStatus::missing();
        assert!(!status.installed && !status.running && status.pid.is_none());
        assert_eq!(status.detail, "没有安装");
    }

    #[test]
    fn the_four_detail_sentences_are_pinned() {
        assert_eq!(describe(false, false, None), "没有安装");
        assert_eq!(describe(true, true, Some(1234)), "已安装，正在运行，PID 1234");
        assert_eq!(describe(true, true, None), "已安装，正在运行");
        assert_eq!(describe(true, false, None), "已安装，但没有运行");
        assert!(NO_ANSWER.contains("检查服务是否还在运行"), "{NO_ANSWER}");
    }

    #[test]
    fn an_answer_without_ok_is_a_failure() {
        let parsed: Answer = serde_json::from_str(r#"{"error":"内核没起来"}"#).unwrap();
        assert!(!parsed.ok);
        assert_eq!(parsed.error.as_deref(), Some("内核没起来"));
    }

    #[test]
    fn a_ping_answer_carries_no_pid() {
        let parsed: Answer = serde_json::from_str(r#"{"ok":true,"note":"pong"}"#).unwrap();
        assert!(parsed.ok && parsed.pid.is_none());
    }

    #[cfg(windows)]
    #[test]
    fn the_current_process_is_alive_and_has_no_kernel_name() {
        let pid = std::process::id();
        assert!(process_is_alive(pid));
        // This test binary is not a kernel; the service cleanup must leave it alone.
        assert!(!process_belongs_to_service_kernel(pid));
    }

}