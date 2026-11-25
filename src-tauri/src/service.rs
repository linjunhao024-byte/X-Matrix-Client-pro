//! The kernel-hosting Windows service (S4b).
//!
//! Why a service at all: a TUN inbound needs an administrator token. Asking
//! for UAC on every start would make the feature useless, and giving the GUI a
//! permanent elevated token is worse. So the *kernel* runs as SYSTEM, started
//! by a service; the GUI keeps its own ordinary token and only talks to that
//! service.
//!
//! **The channel is a directory of job files, not a named pipe.** A pipe with
//! a hand-written security descriptor is one mistake away from letting any
//! local process ask a SYSTEM service to do things; a directory hands that job
//! to the filesystem ACL, which Windows already gets right. The two rules that
//! make the file channel safe:
//!
//! * the client sends a *config*, never an executable path — what runs is
//!   always `<EnginesDir>\<kernel>.exe`, and `EnginesDir` comes from
//!   `HKLM\Software\X-Matrix\Service` (administrator-only) and is re-read for
//!   every job;
//! * a job is claimed by renaming it (`*.job.json` → `*.working`), which is
//!   atomic on one volume, so two readers can never both execute it.
//!
//! Everything the service does is either pure (parsing, path building, result
//! text) or behind the loop in [`run_service`], so the pure half is unit
//! tested without installing anything.

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::engine::EngineKind;
use crate::error::{AppError, AppResult};

/// Service name as the SCM knows it (also what users type in `sc query`).
pub const SERVICE_NAME: &str = "XMatrixKernel";
/// Description shown in `services.msc`.
pub const SERVICE_DESCRIPTION: &str = "X-Matrix 的内核托管服务";

const JOB_SUFFIX: &str = ".job.json";
const CLAIMED_SUFFIX: &str = ".working";
const RESULT_SUFFIX: &str = ".result.json";
const SCAN_INTERVAL: Duration = Duration::from_millis(200);
const READY_TIMEOUT: Duration = Duration::from_secs(8);
const READY_POLL: Duration = Duration::from_millis(100);
const MAX_JOB_BYTES: u64 = 8 * 1024 * 1024;
const MAX_NONCE_LEN: usize = 64;

/// What a job asks the service to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Start,
    Stop,
    Ping,
}

/// One request from the GUI. `kind` is absent for `ping`, which only asks
/// whether the service is listening.
#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub op: Op,
    pub kind: Option<EngineKind>,
    pub ready_port: u16,
    pub nonce: String,
    pub config: Option<String>,
}

/// Wire form of a job. Kept private so the shape in the task book stays the
/// only description of the protocol.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
struct JobWire {
    op: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    kind: Option<String>,
    #[serde(default)]
    ready_port: u16,
    nonce: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    config: Option<String>,
}

/// Wire form of a successful result: the book pins `"note":null` even when
/// there is nothing to say, because the client reads the key, not its absence.
#[derive(Debug, Serialize)]
struct OkWire<'a> {
    ok: bool,
    pid: Option<u32>,
    note: Option<&'a str>,
}

/// A failed result always carries `error` and never `note`.
#[derive(Debug, Serialize)]
struct ErrorWire<'a> {
    ok: bool,
    pid: Option<u32>,
    error: &'a str,
}

/// `--service` is the only argument that turns this binary into the service.
///
/// An argument containing a path is not a request: `wants_quit` treats
/// `C:\--quit\x.exe` as a path for the same reason, and here a folder called
/// `--service` must not start a service loop.
pub fn wants_service<S: AsRef<str>>(args: impl IntoIterator<Item = S>) -> bool {
    args.into_iter().any(|arg| arg.as_ref() == "--service")
}

/// `%ProgramData%`, without relying on the GUI's per-user paths: the service
/// runs as SYSTEM and must find the same directory the installer prepared.
pub fn program_data() -> PathBuf {
    match std::env::var_os("ProgramData") {
        Some(value) => PathBuf::from(value),
        // A local fallback keeps the pure tests runnable and keeps a broken
        // environment from silently pointing at the SYSTEM profile.
        None => PathBuf::from(r"C:\ProgramData"),
    }
}

pub fn service_dir() -> PathBuf {
    program_data().join("X-Matrix")
}

pub fn jobs_dir() -> PathBuf {
    service_dir().join("jobs")
}

pub fn engine_config_dir() -> PathBuf {
    service_dir().join("service")
}

/// `service\running.json`: the service owns this directory, users may only
/// read it. It is what a restart reads to find kernels it left behind.
pub fn running_file() -> PathBuf {
    engine_config_dir().join("running.json")
}

/// A nonce is used as a file name, so it is validated rather than trusted:
/// letters and digits only, non-empty, bounded.
pub fn clean_nonce(raw: &str) -> Option<String> {
    if raw.is_empty() || raw.len() > MAX_NONCE_LEN {
        return None;
    }
    if !raw.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some(raw.to_string())
}

/// The three names one nonce may take in the job directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobPaths {
    pub job: PathBuf,
    pub working: PathBuf,
    pub result: PathBuf,
}

pub fn job_file(jobs: &Path, nonce: &str) -> PathBuf {
    jobs.join(format!("{nonce}{JOB_SUFFIX}"))
}

pub fn result_file(jobs: &Path, nonce: &str) -> PathBuf {
    jobs.join(format!("{nonce}{RESULT_SUFFIX}"))
}

fn claimed_file(jobs: &Path, nonce: &str) -> PathBuf {
    jobs.join(format!("{nonce}{CLAIMED_SUFFIX}"))
}

/// Paths for one request, with the nonce validated first: neither side may let
/// a crafted nonce (`../x`) name a file outside `jobs`.
pub fn job_paths(jobs: &Path, nonce: &str) -> AppResult<JobPaths> {
    let clean = clean_nonce(nonce)
        .ok_or_else(|| AppError::InvalidInput(format!("作业的 nonce 不合法：{nonce}")))?;
    Ok(JobPaths {
        job: job_file(jobs, &clean),
        working: claimed_file(jobs, &clean),
        result: result_file(jobs, &clean),
    })
}

/// The kernel a job may run. The engines directory is the *only* input, so a
/// client cannot point the service at an arbitrary executable; the file name
/// is whitelisted too, so an unknown `kind` string can never name another file.
pub fn engine_binary(engines_dir: &Path, name: &str) -> AppResult<PathBuf> {
    if name != "xray.exe" && name != "sing-box.exe" {
        return Err(AppError::InvalidInput(format!(
            "服务侧不认识这个内核：{name}"
        )));
    }
    Ok(engines_dir.join(name))
}

pub fn parse_job(text: &str) -> Result<Job, String> {
    let wire: JobWire = serde_json::from_str(text).map_err(|e| format!("作业不是合法 JSON：{e}"))?;
    let op = match wire.op.as_str() {
        "start" => Op::Start,
        "stop" => Op::Stop,
        "ping" => Op::Ping,
        other => return Err(format!("不认识的作业类型：{other}")),
    };
    let kind = match wire.kind.as_deref() {
        Some(name) => Some(EngineKind::parse(name).ok_or_else(|| format!("不认识的内核：{name}"))?),
        None => None,
    };
    if matches!(op, Op::Start | Op::Stop) && kind.is_none() {
        let what = if op == Op::Start { "启动" } else { "停止" };
        return Err(format!("{what}作业缺少 kind"));
    }
    let nonce = clean_nonce(&wire.nonce).ok_or_else(|| "作业里的 nonce 不合法".to_string())?;
    let config = match op {
        Op::Start => {
            let config = wire.config.ok_or_else(|| "启动作业缺少 config".to_string())?;
            if config.trim().is_empty() {
                return Err("启动作业的 config 是空的".to_string());
            }
            Some(config)
        }
        Op::Stop | Op::Ping => None,
    };
    if op == Op::Start && wire.ready_port == 0 {
        return Err("就绪端口 0 不像一个真实端口".to_string());
    }
    Ok(Job {
        op,
        kind,
        ready_port: wire.ready_port,
        nonce,
        config,
    })
}

pub fn serialize_ok(pid: Option<u32>, note: Option<&str>) -> String {
    serde_json::to_string(&OkWire {
        ok: true,
        pid,
        note,
    })
    .unwrap_or_else(|_| "{\"ok\":true,\"pid\":null,\"note\":\"结果序列化失败\"}".to_string())
}

pub fn serialize_error(error: &str) -> String {
    serde_json::to_string(&ErrorWire {
        ok: false,
        pid: None,
        error,
    })
    .unwrap_or_else(|_| "{\"ok\":false,\"error\":\"结果序列化失败\"}".to_string())
}

/// What the service remembers about a kernel it started, so a restart of the
/// service can clean up what the previous instance left behind. The book pins
/// the shape: `{"xray":{"pid":…,"port":…,"config":…},"sing-box":{…}}`.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct RunningState {
    #[serde(flatten)]
    pub kernels: HashMap<String, RunningKernel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunningKernel {
    pub pid: u32,
    pub port: u16,
    pub config: String,
}

pub fn parse_running(text: &str) -> RunningState {
    serde_json::from_str(text).unwrap_or_default()
}

/// Write through a temporary file and rename, so a reader never sees half a
/// document. `fsync` matters here more than usual: this file is what a restart
/// uses to find orphan kernels.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut file = fs::File::create(&tmp)?;
        use std::io::Write;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path)
}

fn now_nonce() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{:x}{:x}", nanos, std::process::id())
}

/// `HKLM\Software\X-Matrix\Service` → `EnginesDir`.
#[cfg(windows)]
pub fn read_engines_dir() -> AppResult<PathBuf> {
    use crate::elevate::wide;

    let sub = wide(r"Software\X-Matrix\Service");
    let name = wide("EnginesDir");
    let mut key = std::ptr::null_mut();
    let status = unsafe {
        windows_sys::Win32::System::Registry::RegOpenKeyExW(
            windows_sys::Win32::System::Registry::HKEY_LOCAL_MACHINE,
            sub.as_ptr(),
            0,
            windows_sys::Win32::System::Registry::KEY_READ,
            &mut key,
        )
    };
    if status != 0 {
        return Err(AppError::NotFound(
            "服务没有登记内核目录（还没安装过服务）".to_string(),
        ));
    }
    let mut kind = 0u32;
    let mut bytes = 0u32;
    let probe = unsafe {
        windows_sys::Win32::System::Registry::RegQueryValueExW(
            key,
            name.as_ptr(),
            std::ptr::null(),
            &mut kind,
            std::ptr::null_mut(),
            &mut bytes,
        )
    };
    if probe != 0 || bytes < 2 {
        unsafe { windows_sys::Win32::System::Registry::RegCloseKey(key) };
        return Err(AppError::NotFound(
            "服务没有登记内核目录（还没安装过服务）".to_string(),
        ));
    }
    let mut buffer = vec![0u8; bytes as usize + 2];
    let read = unsafe {
        windows_sys::Win32::System::Registry::RegQueryValueExW(
            key,
            name.as_ptr(),
            std::ptr::null(),
            &mut kind,
            buffer.as_mut_ptr(),
            &mut bytes,
        )
    };
    unsafe { windows_sys::Win32::System::Registry::RegCloseKey(key) };
    if read != 0 {
        return Err(AppError::Internal("读不到服务的内核目录".to_string()));
    }
    let units: Vec<u16> = buffer
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take_while(|unit| *unit != 0)
        .collect();
    let path = String::from_utf16_lossy(&units);
    if path.trim().is_empty() {
        return Err(AppError::NotFound("服务登记的内核目录是空的".to_string()));
    }
    Ok(PathBuf::from(path))
}

#[cfg(not(windows))]
pub fn read_engines_dir() -> AppResult<PathBuf> {
    Err(AppError::Internal("只有 Windows 实现了内核服务".to_string()))
}

/// Write `HKLM\Software\X-Matrix\Service` → `EnginesDir`. Administrator-only by
/// construction: the service reads this value instead of trusting its clients.
#[cfg(windows)]
pub fn remember_engines_dir(engines: &Path) -> AppResult<()> {
    use crate::elevate::wide;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY_LOCAL_MACHINE, KEY_WRITE,
        REG_OPTION_NON_VOLATILE, REG_SZ,
    };

    let sub = wide(r"Software\X-Matrix\Service");
    let mut key = std::ptr::null_mut();
    let mut disposition = 0u32;
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_LOCAL_MACHINE,
            sub.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            std::ptr::null(),
            &mut key,
            &mut disposition,
        )
    };
    if status != 0 {
        return Err(AppError::Internal(format!("写注册表失败（{status}）")));
    }
    let name = wide("EnginesDir");
    let text = engines.to_string_lossy().into_owned();
    let mut value = wide(&text);
    let bytes = (value.len() * 2) as u32;
    let written = unsafe {
        RegSetValueExW(key, name.as_ptr(), 0, REG_SZ, value.as_mut_ptr() as *const u8, bytes)
    };
    unsafe { RegCloseKey(key) };
    if written != 0 {
        return Err(AppError::Internal(format!(
            "写服务的 EnginesDir 失败（{written}）"
        )));
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn remember_engines_dir(_engines: &Path) -> AppResult<()> {
    Err(AppError::Internal("只有 Windows 实现了内核服务".to_string()))
}

/// Forget the recorded engines directory once the service is gone.
#[cfg(windows)]
fn forget_engines_dir() {
    use crate::elevate::wide;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegDeleteKeyW, RegDeleteValueW, RegOpenKeyExW, HKEY_LOCAL_MACHINE, KEY_WRITE,
    };

    let sub = wide(r"Software\X-Matrix\Service");
    let mut key = std::ptr::null_mut();
    if unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, sub.as_ptr(), 0, KEY_WRITE, &mut key) } == 0 {
        let name = wide("EnginesDir");
        unsafe {
            RegDeleteValueW(key, name.as_ptr());
            RegCloseKey(key);
        }
    }
    let _ = unsafe { RegDeleteKeyW(HKEY_LOCAL_MACHINE, sub.as_ptr()) };
}

#[cfg(not(windows))]
fn forget_engines_dir() {}

/// Install (or repair) the service and start it. The command layer checks the
/// token first so the user gets a sentence instead of error 5.
///
/// `Ok(Some(warning))` means the service is installed but a non-fatal step did
/// not finish (today only the jobs-directory ACL), and the sentence belongs in
/// the state the settings page shows.
#[cfg(windows)]
pub fn install(engines: &Path) -> AppResult<Option<String>> {
    use crate::elevate::wide;
    use windows_sys::Win32::Foundation::{
        GetLastError, ERROR_SERVICE_ALREADY_RUNNING, ERROR_SERVICE_EXISTS,
    };
    use windows_sys::Win32::System::Services::{
        ChangeServiceConfig2W, ChangeServiceConfigW, CloseServiceHandle, CreateServiceW,
        OpenSCManagerW, OpenServiceW, StartServiceW, SC_MANAGER_CREATE_SERVICE, SERVICE_ALL_ACCESS,
        SERVICE_CONFIG_DESCRIPTION, SERVICE_DEMAND_START, SERVICE_DESCRIPTIONW, SERVICE_ERROR_NORMAL,
        SERVICE_NO_CHANGE, SERVICE_WIN32_OWN_PROCESS,
    };

    remember_engines_dir(engines)?;
    fs::create_dir_all(jobs_dir()).map_err(|e| AppError::Io(format!("建作业目录失败：{e}")))?;
    fs::create_dir_all(engine_config_dir())
        .map_err(|e| AppError::Io(format!("建服务配置目录失败：{e}")))?;
    let acl_warning = grant_users_write(&jobs_dir());

    let exe = std::env::current_exe()?;
    let line = format!("\"{}\" --service", exe.display());
    let name = wide(SERVICE_NAME);
    let display = wide("X-Matrix 内核服务");
    let bin = wide(&line);

    let scm =
        unsafe { OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CREATE_SERVICE) };
    if scm.is_null() {
        let code = unsafe { GetLastError() };
        return Err(AppError::Engine(format!("打不开服务管理器（错误 {code}）")));
    }
    let mut service = unsafe {
        CreateServiceW(
            scm,
            name.as_ptr(),
            display.as_ptr(),
            SERVICE_ALL_ACCESS,
            SERVICE_WIN32_OWN_PROCESS,
            SERVICE_DEMAND_START,
            SERVICE_ERROR_NORMAL,
            bin.as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if service.is_null() {
        let code = unsafe { GetLastError() };
        if code != ERROR_SERVICE_EXISTS {
            unsafe { CloseServiceHandle(scm) };
            return Err(AppError::Engine(format!("建服务失败（错误 {code}）")));
        }
        // Reinstalling is a legitimate repair: keep the service, fix the
        // binary path, then start it.
        service = unsafe { OpenServiceW(scm, name.as_ptr(), SERVICE_ALL_ACCESS) };
        if service.is_null() {
            let code = unsafe { GetLastError() };
            unsafe { CloseServiceHandle(scm) };
            return Err(AppError::Engine(format!("服务已存在但打不开（错误 {code}）")));
        }
        let changed = unsafe {
            ChangeServiceConfigW(
                service,
                SERVICE_NO_CHANGE,
                SERVICE_NO_CHANGE,
                SERVICE_NO_CHANGE,
                bin.as_ptr(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        if changed == 0 {
            let code = unsafe { GetLastError() };
            unsafe {
                CloseServiceHandle(service);
                CloseServiceHandle(scm);
            }
            return Err(AppError::Engine(format!("改服务的启动命令失败（错误 {code}）")));
        }
    }

    let mut text = wide(SERVICE_DESCRIPTION);
    let mut config = SERVICE_DESCRIPTIONW {
        lpDescription: text.as_mut_ptr(),
    };
    let _ = unsafe {
        ChangeServiceConfig2W(
            service,
            SERVICE_CONFIG_DESCRIPTION,
            &mut config as *mut _ as *mut std::ffi::c_void,
        )
    };

    let started = unsafe { StartServiceW(service, 0, std::ptr::null()) };
    let code = unsafe { GetLastError() };
    unsafe {
        CloseServiceHandle(service);
        CloseServiceHandle(scm);
    }
    if started == 0 && code != ERROR_SERVICE_ALREADY_RUNNING {
        return Err(AppError::Engine(format!(
            "服务建好了但起不来（错误 {code}）：看看事件查看器里的应用程序日志"
        )));
    }
    Ok(acl_warning)
}

#[cfg(not(windows))]
pub fn install(_engines: &Path) -> AppResult<Option<String>> {
    Err(AppError::Internal("只有 Windows 实现了内核服务".to_string()))
}

/// Stop and delete the service, then forget its engines directory.
#[cfg(windows)]
pub fn uninstall() -> AppResult<()> {
    use crate::elevate::wide;
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::System::Services::{
        CloseServiceHandle, ControlService, DeleteService, OpenSCManagerW, OpenServiceW,
        SC_MANAGER_CONNECT, SERVICE_CONTROL_STOP, SERVICE_STATUS, SERVICE_STOP,
    };

    const SERVICE_DELETE_ACCESS: u32 = 0x0001_0000;
    let scm = unsafe { OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT) };
    if scm.is_null() {
        let code = unsafe { GetLastError() };
        return Err(AppError::Engine(format!("打不开服务管理器（错误 {code}）")));
    }
    let name = wide(SERVICE_NAME);
    let service = unsafe { OpenServiceW(scm, name.as_ptr(), SERVICE_STOP | SERVICE_DELETE_ACCESS) };
    if service.is_null() {
        unsafe { CloseServiceHandle(scm) };
        // "Not installed" is the state the caller asked for.
        forget_engines_dir();
        return Ok(());
    }
    let mut status: SERVICE_STATUS = unsafe { std::mem::zeroed() };
    let _ = unsafe { ControlService(service, SERVICE_CONTROL_STOP, &mut status) };
    let deleted = unsafe { DeleteService(service) };
    let code = unsafe { GetLastError() };
    unsafe {
        CloseServiceHandle(service);
        CloseServiceHandle(scm);
    }
    if deleted == 0 {
        return Err(AppError::Engine(format!("删服务失败（错误 {code}）")));
    }
    forget_engines_dir();
    Ok(())
}

#[cfg(not(windows))]
pub fn uninstall() -> AppResult<()> {
    Err(AppError::Internal("只有 Windows 实现了内核服务".to_string()))
}

/// Let ordinary users drop job files; the service still decides what may run.
///
/// A failure here is not fatal — an administrator can still start kernels — so
/// the reason travels back to the caller instead of turning into an error. The
/// settings page shows it, which is the only place it can be fixed.
#[cfg(windows)]
fn grant_users_write(dir: &Path) -> Option<String> {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const USERS_SID: &str = "*S-1-5-32-545";
    let result = Command::new("icacls")
        .arg(dir)
        .arg("/grant")
        .arg(format!("{USERS_SID}:(OI)(CI)M"))
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match result {
        Ok(output) if output.status.success() => None,
        Ok(output) => Some(format!(
            "作业目录权限没设成（icacls 退出码 {}）：{}",
            output.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&output.stderr).trim()
        )),
        Err(error) => Some(format!("调不动 icacls：{error}")),
    }
}

#[cfg(not(windows))]
fn grant_users_write(_dir: &Path) -> Option<String> {
    None
}

/// Wait until the kernel's control port answers. Same budget as the GUI's own
/// probe: a config typo must look like a failure, not like a healthy process.
fn wait_for_port(port: u16, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    while Instant::now() < deadline {
        if std::net::TcpStream::connect_timeout(&addr, READY_POLL).is_ok() {
            return true;
        }
        std::thread::sleep(READY_POLL);
    }
    false
}

/// One kernel the service is responsible for.
struct Kernel {
    child: Child,
    port: u16,
    config: String,
}

#[derive(Default)]
struct Service {
    children: HashMap<EngineKind, Kernel>,
}

impl Service {
    fn running(&self) -> RunningState {
        RunningState {
            kernels: self
                .children
                .iter()
                .map(|(kind, kernel)| {
                    (
                        kind.as_str().to_string(),
                        RunningKernel {
                            pid: kernel.child.id(),
                            port: kernel.port,
                            config: kernel.config.clone(),
                        },
                    )
                })
                .collect(),
        }
    }

    fn persist_running(&self) {
        if let Ok(text) = serde_json::to_string_pretty(&self.running()) {
            let _ = write_atomic(&running_file(), text.as_bytes());
        }
    }

    fn start(
        &mut self,
        kind: EngineKind,
        ready_port: u16,
        config: &str,
    ) -> Result<Option<u32>, String> {
        // The book's safety rule: the engines directory is re-read for every
        // job, so clearing the registry value takes effect on the next start
        // instead of after a service restart.
        let engines_dir = read_engines_dir().map_err(|error| error.to_string())?;
        let binary = engine_binary(&engines_dir, kind.engine().binary_name())
            .map_err(|error| error.to_string())?;
        if !binary.exists() {
            return Err(format!("服务侧没有找到内核：{}", binary.display()));
        }
        // One kernel at a time, like the GUI: two kernels on one inbound port
        // is the classic way these clients wedge.
        if let Some(mut kernel) = self.children.remove(&kind) {
            let _ = kernel.child.kill();
            let _ = kernel.child.wait();
        }
        let config_path = engine_config_dir().join(format!("{}.json", kind.as_str()));
        write_atomic(&config_path, config.as_bytes()).map_err(|e| format!("写内核配置失败：{e}"))?;

        let log_path = service_dir().join(format!("{}.log", kind.as_str()));
        let log = fs::File::create(&log_path).map_err(|e| format!("建内核日志失败：{e}"))?;
        let log_err = log.try_clone().map_err(|e| format!("复制日志句柄失败：{e}"))?;

        let args = kind.engine().args(&config_path.to_string_lossy());
        let mut command = Command::new(&binary);
        command
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err))
            .env("XRAY_LOCATION_ASSET", &engines_dir);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let child = command
            .spawn()
            .map_err(|e| format!("服务启动内核失败：{e}"))?;
        let pid = child.id();

        if !wait_for_port(ready_port, READY_TIMEOUT) {
            let mut child = child;
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "内核没有在 {} 秒内打开控制端口 {}（日志：{}）",
                READY_TIMEOUT.as_secs(),
                ready_port,
                log_path.display()
            ));
        }
        self.children.insert(
            kind,
            Kernel {
                child,
                port: ready_port,
                config: config_path.display().to_string(),
            },
        );
        self.persist_running();
        Ok(Some(pid))
    }

    fn stop(&mut self, kind: EngineKind) -> Result<Option<u32>, String> {
        match self.children.remove(&kind) {
            Some(mut kernel) => {
                let pid = kernel.child.id();
                kernel
                    .child
                    .kill()
                    .map_err(|e| format!("停止内核失败：{e}"))?;
                let _ = kernel.child.wait();
                self.persist_running();
                Ok(Some(pid))
            }
            None => {
                self.persist_running();
                Ok(None)
            }
        }
    }

    /// Deliberate stop: nobody may take the kernel down behind the user's back.
    fn shutdown(&mut self) {
        for (_, mut kernel) in self.children.drain() {
            let _ = kernel.child.kill();
            let _ = kernel.child.wait();
        }
        let _ = fs::remove_file(running_file());
    }

    /// A service restart must not leave the previous instance's kernels behind:
    /// they still hold the inbound ports and the user's traffic.
    fn reap_orphans(&mut self) {
        let text = fs::read_to_string(running_file()).unwrap_or_default();
        let state = parse_running(&text);
        for (name, kernel) in state.kernels {
            if EngineKind::parse(&name).is_none() {
                continue;
            }
            if !crate::service_client::process_is_alive(kernel.pid) {
                continue;
            }
            if crate::service_client::process_belongs_to_service_kernel(kernel.pid) {
                crate::service_client::terminate(kernel.pid);
                eprintln!("x-matrix: 服务启动时清掉了上次残留的内核进程 {}", kernel.pid);
            }
        }
        let _ = fs::remove_file(running_file());
    }

    fn handle(&mut self, job: &Job) -> String {
        match job.op {
            Op::Ping => serialize_ok(None, Some("pong")),
            Op::Start => {
                let Some(kind) = job.kind else {
                    return serialize_error("启动作业缺少 kind");
                };
                let Some(config) = job.config.as_deref() else {
                    return serialize_error("启动作业缺少 config");
                };
                match self.start(kind, job.ready_port, config) {
                    Ok(pid) => serialize_ok(pid, None),
                    Err(error) => serialize_error(&error),
                }
            }
            Op::Stop => {
                let Some(kind) = job.kind else {
                    return serialize_error("停止作业缺少 kind");
                };
                match self.stop(kind) {
                    Ok(Some(pid)) => serialize_ok(Some(pid), None),
                    Ok(None) => serialize_ok(None, Some("本来就没有在跑")),
                    Err(error) => serialize_error(&error),
                }
            }
        }
    }
}

fn claim(jobs: &Path, path: &Path) -> Option<(PathBuf, String)> {
    let name = path.file_name()?.to_str()?;
    let nonce = name.strip_suffix(JOB_SUFFIX)?;
    let nonce = clean_nonce(nonce)?;
    let claimed = claimed_file(jobs, &nonce);
    fs::rename(path, &claimed).ok()?;
    Some((claimed, nonce))
}

fn read_job(path: &Path) -> Result<Job, String> {
    let metadata = fs::metadata(path).map_err(|e| format!("读作业失败：{e}"))?;
    if metadata.len() > MAX_JOB_BYTES {
        return Err(format!("作业太大了（{} 字节）", metadata.len()));
    }
    let mut text = String::new();
    fs::File::open(path)
        .map_err(|e| format!("打开作业失败：{e}"))?
        .take(MAX_JOB_BYTES)
        .read_to_string(&mut text)
        .map_err(|e| format!("读作业失败：{e}"))?;
    parse_job(&text)
}

/// One pass over the job directory. Split out so a test can drive it without a
/// service loop.
fn scan_once(service: &mut Service, jobs: &Path) {
    let Ok(entries) = fs::read_dir(jobs) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.ends_with(JOB_SUFFIX))
                .unwrap_or(false)
        })
        .collect();
    paths.sort();
    for path in paths {
        let Some((claimed, nonce)) = claim(jobs, &path) else {
            continue;
        };
        let answer = match read_job(&claimed) {
            Ok(job) => service.handle(&job),
            Err(error) => serialize_error(&error),
        };
        let _ = write_atomic(&result_file(jobs, &nonce), answer.as_bytes());
        let _ = fs::remove_file(&claimed);
    }
}

/// Set by the control handler, read between scans: a stop request must be
/// honoured without waiting for the next job to finish.
static SERVICE_STOP: AtomicBool = AtomicBool::new(false);
static SERVICE_STATUS_HANDLE: AtomicUsize = AtomicUsize::new(0);

/// The service entry point. `StartServiceCtrlDispatcherW` blocks for the life
/// of the service and calls [`service_main`] on its own thread, so this
/// function returns only once the service has stopped.
#[cfg(windows)]
pub fn run_service() -> i32 {
    use crate::elevate::wide;
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::System::Services::{StartServiceCtrlDispatcherW, SERVICE_TABLE_ENTRYW};

    /// `ERROR_FAILED_SERVICE_CONTROLLER_CONNECT`: started by hand, not by the SCM.
    const ERROR_FAILED_SERVICE_CONTROLLER_CONNECT: u32 = 1063;

    let mut name = wide(SERVICE_NAME);
    let table = [
        SERVICE_TABLE_ENTRYW {
            lpServiceName: name.as_mut_ptr(),
            lpServiceProc: Some(service_main),
        },
        SERVICE_TABLE_ENTRYW {
            lpServiceName: std::ptr::null_mut(),
            lpServiceProc: None,
        },
    ];
    let started = unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) };
    if started == 0 {
        let code = unsafe { GetLastError() };
        if code == ERROR_FAILED_SERVICE_CONTROLLER_CONNECT {
            eprintln!(
                "x-matrix: --service 只能由服务管理器启动：先装服务，再 sc start XMatrixKernel"
            );
        } else {
            eprintln!("x-matrix: 内核服务没能连上服务管理器（错误 {code}）");
        }
        return code as i32;
    }
    0
}

#[cfg(not(windows))]
pub fn run_service() -> i32 {
    eprintln!("x-matrix: 内核服务只有 Windows 实现");
    1
}

#[cfg(windows)]
fn service_status(
    state: u32,
    controls: u32,
    exit_code: u32,
    wait_hint: u32,
) -> windows_sys::Win32::System::Services::SERVICE_STATUS {
    use windows_sys::Win32::System::Services::{SERVICE_STATUS, SERVICE_WIN32_OWN_PROCESS};

    SERVICE_STATUS {
        dwServiceType: SERVICE_WIN32_OWN_PROCESS,
        dwCurrentState: state,
        dwControlsAccepted: controls,
        dwWin32ExitCode: exit_code,
        dwServiceSpecificExitCode: 0,
        dwCheckPoint: 0,
        dwWaitHint: wait_hint,
    }
}

/// Runs on the thread the dispatcher created. Reports RUNNING before touching
/// the job directory, then blocks in [`run_loop`] until a stop is requested.
#[cfg(windows)]
unsafe extern "system" fn service_main(_argc: u32, _argv: *mut *mut u16) {
    use crate::elevate::wide;
    use windows_sys::Win32::System::Services::{
        RegisterServiceCtrlHandlerW, SERVICE_ACCEPT_STOP, SERVICE_RUNNING, SERVICE_START_PENDING,
        SERVICE_STOPPED, SetServiceStatus,
    };

    let name = wide(SERVICE_NAME);
    let handle = unsafe { RegisterServiceCtrlHandlerW(name.as_ptr(), Some(service_control)) };
    if handle.is_null() {
        eprintln!("x-matrix: 服务控制句柄注册失败");
        return;
    }
    SERVICE_STATUS_HANDLE.store(handle as usize, Ordering::SeqCst);
    let mut status = service_status(SERVICE_START_PENDING, 0, 0, 3000);
    unsafe { SetServiceStatus(handle, &status) };
    status = service_status(SERVICE_RUNNING, SERVICE_ACCEPT_STOP, 0, 0);
    unsafe { SetServiceStatus(handle, &status) };

    SERVICE_STOP.store(false, Ordering::SeqCst);
    let exit_code = run_loop();
    status = service_status(SERVICE_STOPPED, 0, exit_code.max(0) as u32, 0);
    unsafe { SetServiceStatus(handle, &status) };
    SERVICE_STATUS_HANDLE.store(0, Ordering::SeqCst);
}

#[cfg(windows)]
unsafe extern "system" fn service_control(control: u32) {
    use windows_sys::Win32::System::Services::{
        SERVICE_CONTROL_STOP, SERVICE_STOP_PENDING, SetServiceStatus,
    };

    if control != SERVICE_CONTROL_STOP {
        return;
    }
    SERVICE_STOP.store(true, Ordering::SeqCst);
    let handle = SERVICE_STATUS_HANDLE.load(Ordering::SeqCst);
    if handle != 0 {
        let status = service_status(SERVICE_STOP_PENDING, 0, 0, 3000);
        unsafe { SetServiceStatus(handle as *mut std::ffi::c_void, &status) };
    }
}

/// The scan loop, split out so the dispatcher stays a thin shell and so a test
/// could drive one pass without a service install.
#[cfg(windows)]
fn run_loop() -> u32 {
    if let Err(error) =
        fs::create_dir_all(jobs_dir()).and_then(|_| fs::create_dir_all(engine_config_dir()))
    {
        eprintln!("x-matrix: 建服务目录失败：{error}");
        return 2;
    }
    let mut service = Service::default();
    service.reap_orphans();
    eprintln!(
        "x-matrix: 内核服务已启动，作业目录 {}",
        jobs_dir().display()
    );
    while !SERVICE_STOP.load(Ordering::SeqCst) {
        scan_once(&mut service, &jobs_dir());
        // Leftovers from a crashed client would otherwise accumulate forever.
        purge_stale_results(&jobs_dir());
        std::thread::sleep(SCAN_INTERVAL);
    }
    service.shutdown();
    0
}

/// Results nobody picked up within a few minutes are garbage: the client that
/// would have read them is gone. Bounded so a bug cannot fill the disk.
fn purge_stale_results(jobs: &Path) {
    const KEEP: Duration = Duration::from_secs(600);
    let Ok(entries) = fs::read_dir(jobs) else {
        return;
    };
    let now = std::time::SystemTime::now();
    for entry in entries.flatten() {
        let path = entry.path();
        let stale = entry
            .metadata()
            .ok()
            .and_then(|meta| meta.modified().ok())
            .and_then(|modified| now.duration_since(modified).ok())
            .map(|age| age > KEEP)
            .unwrap_or(false);
        if stale {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_string();
            if name.ends_with(RESULT_SUFFIX) || name.ends_with(CLAIMED_SUFFIX) {
                let _ = fs::remove_file(&path);
            }
        }
    }
}

/// Nonce for one client request: now in nanoseconds plus the pid, so two
/// requests from the same build cannot collide.
pub fn make_nonce() -> String {
    now_nonce()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start_job() -> &'static str {
        r#"{"op":"start","kind":"xray","ready_port":9090,"nonce":"ab12","config":"{\"log\":{}}"}"#
    }

    #[test]
    fn a_start_job_carries_its_config_and_port() {
        let job = parse_job(start_job()).expect("job");
        assert_eq!(job.op, Op::Start);
        assert_eq!(job.kind, Some(EngineKind::Xray));
        assert_eq!(job.ready_port, 9090);
        assert_eq!(job.nonce, "ab12");
        assert_eq!(job.config.as_deref(), Some("{\"log\":{}}"));
    }

    #[test]
    fn a_stop_job_needs_no_config() {
        let job = parse_job(r#"{"op":"stop","kind":"sing-box","nonce":"cd34"}"#).expect("job");
        assert_eq!(job.op, Op::Stop);
        assert_eq!(job.kind, Some(EngineKind::SingBox));
        assert!(job.config.is_none());
    }

    #[test]
    fn a_start_job_without_a_config_is_rejected() {
        let error = parse_job(r#"{"op":"start","kind":"xray","ready_port":9090,"nonce":"ab12"}"#)
            .expect_err("must fail");
        assert!(error.contains("缺少 config"), "{error}");
    }

    #[test]
    fn an_unknown_operation_or_kernel_is_rejected() {
        assert!(parse_job(r#"{"op":"exec","kind":"xray","nonce":"aa"}"#).is_err());
        assert!(parse_job(r#"{"op":"ping","kind":"cmd","nonce":"aa"}"#).is_err());
        assert!(parse_job(r#"{"op":"ping","kind":"xray","nonce":"a b"}"#).is_err());
        assert!(parse_job("not json").is_err());
    }

    #[test]
    fn a_start_job_with_a_silly_port_is_rejected() {
        let error = parse_job(r#"{"op":"start","kind":"xray","ready_port":0,"nonce":"aa","config":"{}"}"#)
            .expect_err("must fail");
        assert!(error.contains("端口"), "{error}");
        assert!(
            parse_job(r#"{"op":"start","kind":"xray","ready_port":9090,"nonce":"aa","config":"{}"}"#)
                .is_ok()
        );
    }

    #[test]
    fn nonces_become_safe_file_names() {
        assert_eq!(clean_nonce("ab12CD"), Some("ab12CD".to_string()));
        assert_eq!(clean_nonce(""), None);
        assert_eq!(clean_nonce("a/b"), None);
        assert_eq!(clean_nonce("../x"), None);
        assert_eq!(clean_nonce(&"a".repeat(MAX_NONCE_LEN + 1)), None);
        assert!(job_file(Path::new("jobs"), "ab12").ends_with("ab12.job.json"));
        assert!(result_file(Path::new("jobs"), "ab12").ends_with("ab12.result.json"));
    }

    #[test]
    fn the_kernel_binary_comes_from_the_engines_directory_only() {
        let dir = Path::new(r"C:\engines");
        let xray = engine_binary(dir, "xray.exe").expect("xray");
        assert_eq!(xray, dir.join("xray.exe"));
        assert!(engine_binary(dir, "sing-box.exe").is_ok());
        let error = engine_binary(dir, "cmd.exe").expect_err("must fail");
        assert!(error.to_string().contains("服务侧不认识这个内核"), "{error}");
    }

    #[test]
    fn results_say_what_happened() {
        let ok = serialize_ok(Some(77), None);
        assert!(ok.contains("\"ok\":true") && ok.contains("\"pid\":77") && !ok.contains("error"));
        let noted = serialize_ok(None, Some("本来就没有在跑"));
        assert!(noted.contains("note"));
        let bad = serialize_error("内核没起来");
        assert!(bad.contains("\"ok\":false") && bad.contains("内核没起来"));
    }

    #[test]
    fn a_running_state_survives_garbage() {
        let state = parse_running("not json");
        assert!(state.kernels.is_empty());
        let text = r#"{"xray":{"pid":9,"port":9090,"config":"C:\\x.json"}}"#;
        assert_eq!(parse_running(text).kernels["xray"].pid, 9);
    }

    #[test]
    fn only_the_service_flag_counts() {
        assert!(wants_service(["X-Matrix.exe", "--service"]));
        assert!(!wants_service(["X-Matrix.exe", "--services"]));
        assert!(!wants_service([r"C:\--service\x.exe"]));
        assert!(!wants_service(["X-Matrix.exe"]));
    }

    #[test]
    fn a_job_round_trips_through_the_wire_format() {
        let job = parse_job(start_job()).expect("job");
        let wire = JobWire {
            op: "start".to_string(),
            kind: job.kind.map(|kind| kind.as_str().to_string()),
            ready_port: job.ready_port,
            nonce: job.nonce.clone(),
            config: job.config.clone(),
        };
        let text = serde_json::to_string(&wire).unwrap();
        assert_eq!(parse_job(&text).expect("again"), job);
    }

    #[test]
    fn a_ping_job_needs_no_kernel() {
        let job = parse_job(r#"{"op":"ping","nonce":"01"}"#).expect("ping");
        assert_eq!(job.op, Op::Ping);
        assert!(job.kind.is_none());
    }

    #[test]
    fn job_paths_refuse_a_nonce_that_is_not_a_file_name() {
        let jobs = Path::new("C:/jobs");
        let paths = job_paths(jobs, "ab12").expect("paths");
        assert_eq!(paths.job, jobs.join("ab12.job.json"));
        assert_eq!(paths.working, jobs.join("ab12.working"));
        assert_eq!(paths.result, jobs.join("ab12.result.json"));
        assert!(job_paths(jobs, "../x").is_err());
        assert!(job_paths(jobs, "").is_err());
    }

    #[test]
    fn results_have_exactly_the_pinned_shape() {
        assert_eq!(
            serialize_ok(Some(1234), None),
            "{\"ok\":true,\"pid\":1234,\"note\":null}"
        );
        assert_eq!(
            serialize_ok(None, Some("本来就没有在跑")),
            "{\"ok\":true,\"pid\":null,\"note\":\"本来就没有在跑\"}"
        );
        assert_eq!(
            serialize_error("内核没有在 8 秒内打开控制端口"),
            "{\"ok\":false,\"pid\":null,\"error\":\"内核没有在 8 秒内打开控制端口\"}"
        );
    }

    #[cfg(windows)]
    #[test]
    fn the_service_status_is_an_own_process_that_accepts_stop() {
        use windows_sys::Win32::System::Services::{
            SERVICE_ACCEPT_STOP, SERVICE_RUNNING, SERVICE_WIN32_OWN_PROCESS,
        };
        let status = service_status(SERVICE_RUNNING, SERVICE_ACCEPT_STOP, 0, 0);
        assert_eq!(status.dwServiceType, SERVICE_WIN32_OWN_PROCESS);
        assert_eq!(status.dwCurrentState, SERVICE_RUNNING);
        assert_eq!(status.dwControlsAccepted, SERVICE_ACCEPT_STOP);
    }

    #[cfg(windows)]
    #[test]
    fn a_missing_engines_dir_says_so() {
        if read_engines_dir().is_ok() {
            // This machine already has the service installed: nothing to assert.
            return;
        }
        let error = read_engines_dir().expect_err("must fail");
        assert!(error.to_string().contains("服务没有登记内核目录"), "{error}");
    }

    #[test]
    fn the_service_description_is_the_pinned_sentence() {
        assert_eq!(SERVICE_DESCRIPTION, "X-Matrix 的内核托管服务");
    }

    #[test]
    fn a_running_state_round_trips_both_kernels() {
        let mut state = RunningState::default();
        state.kernels.insert(
            "xray".to_string(),
            RunningKernel {
                pid: 11,
                port: 9090,
                config: "C:/x.json".to_string(),
            },
        );
        state.kernels.insert(
            "sing-box".to_string(),
            RunningKernel {
                pid: 12,
                port: 9091,
                config: "C:/s.json".to_string(),
            },
        );
        let text = serde_json::to_string(&state).unwrap();
        let parsed = parse_running(&text);
        assert_eq!(parsed.kernels["xray"].port, 9090);
        assert_eq!(parsed.kernels["sing-box"].pid, 12);
    }
}