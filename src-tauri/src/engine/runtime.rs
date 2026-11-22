//! Kernel process lifecycle: write the config, spawn, wait until it is ready.
//!
//! Everything external is behind a trait ([`Spawner`], [`ReadinessProbe`]) so
//! the whole lifecycle is testable without a kernel binary on disk. That is not
//! testability for its own sake: during development a real kernel must never be
//! started, because it would fight with whatever proxy the machine is already
//! running.
//!
//! Ordering rules, borrowed from both reference clients:
//!
//! * config is written **before** spawn, and atomically, so a half-written file
//!   can never be read by a kernel;
//! * a kernel is only "running" once its control port answers — otherwise a
//!   typo in the config looks like a healthy process;
//! * a kernel that fails to become ready is killed immediately, never left
//!   behind.

use std::io::Write;
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use serde::Serialize;
use specta::Type;

use super::job::KillOnCloseJob;
use super::{Endpoints, EngineKind, LoadedRuleSet, RenderInput, RunOptions, StatsChannel};
use crate::domain::node::Node;
use crate::error::{AppError, AppResult};

/// How long a kernel gets to open its control port.
const READY_TIMEOUT: Duration = Duration::from_secs(8);
const READY_POLL: Duration = Duration::from_millis(100);

/// A spawned child process.
pub trait ProcessHandle: Send {
    fn pid(&self) -> u32;
    /// `Some(exit_code)` once the process has exited.
    fn try_wait(&mut self) -> AppResult<Option<i32>>;
    fn kill(&mut self) -> AppResult<()>;
}

/// Starts a kernel binary. Abstracted so tests never spawn anything.
pub trait Spawner: Send + Sync {
    fn spawn(
        &self,
        exe: &Path,
        args: &[String],
        log_path: &Path,
    ) -> AppResult<Box<dyn ProcessHandle>>;
}

/// Waits for a kernel to be ready. Abstracted for the same reason.
pub trait ReadinessProbe: Send + Sync {
    fn wait_for_port(&self, port: u16, timeout: Duration) -> AppResult<()>;
}

/// Real implementation: `std::process::Command` with no console window.
pub struct SystemSpawner;

impl Spawner for SystemSpawner {
    fn spawn(
        &self,
        exe: &Path,
        args: &[String],
        log_path: &Path,
    ) -> AppResult<Box<dyn ProcessHandle>> {
        if !exe.exists() {
            return Err(AppError::Engine(format!(
                "找不到内核可执行文件：{}",
                exe.display()
            )));
        }

        let log = std::fs::File::create(log_path)?;
        let log_err = log.try_clone()?;

        let mut command = Command::new(exe);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err));

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            /// `CREATE_NO_WINDOW` — a kernel must never flash a console window.
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let child = command
            .spawn()
            .map_err(|e| AppError::Engine(format!("启动内核失败：{e}")))?;
        Ok(Box::new(SystemProcess { child }))
    }
}

struct SystemProcess {
    child: std::process::Child,
}

impl ProcessHandle for SystemProcess {
    fn pid(&self) -> u32 {
        self.child.id()
    }

    fn try_wait(&mut self) -> AppResult<Option<i32>> {
        Ok(self
            .child
            .try_wait()?
            .map(|status| status.code().unwrap_or(-1)))
    }

    fn kill(&mut self) -> AppResult<()> {
        // Already gone is a success: stopping an exited kernel is not an error.
        match self.child.kill() {
            Ok(()) => {
                let _ = self.child.wait();
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::InvalidInput => Ok(()),
            Err(e) => Err(AppError::Io(e.to_string())),
        }
    }
}

/// Real implementation: poll a TCP connect on the control port.
pub struct TcpProbe;

impl ReadinessProbe for TcpProbe {
    fn wait_for_port(&self, port: u16, timeout: Duration) -> AppResult<()> {
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let deadline = Instant::now() + timeout;
        loop {
            if TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(AppError::Engine(format!(
                    "内核在 {} 秒内没有打开控制端口 {port}",
                    timeout.as_secs()
                )));
            }
            std::thread::sleep(READY_POLL);
        }
    }
}

/// What the UI shows about the runtime.
#[derive(Debug, Clone, Serialize, Type)]
pub struct EngineStatus {
    pub running: bool,
    pub kind: Option<EngineKind>,
    pub node_name: String,
    pub mixed_port: u16,
    pub api_port: u16,
    pub pid: u32,
    pub config_path: String,
    // Specta refuses u64; f64 keeps millisecond timestamps exact.
    #[specta(type = f64)]
    pub started_at_ms: u64,
    /// Set when a kernel exited on its own, so the UI can explain the stop
    /// instead of silently flipping to "not running".
    pub last_exit_code: Option<i32>,
}

impl Default for EngineStatus {
    fn default() -> Self {
        Self {
            running: false,
            kind: None,
            node_name: String::new(),
            mixed_port: 0,
            api_port: 0,
            pid: 0,
            config_path: String::new(),
            started_at_ms: 0,
            last_exit_code: None,
        }
    }
}

/// One page of a kernel log: the bytes after the caller's cursor.
#[derive(Debug, Clone, Serialize, Type)]
pub struct LogDelta {
    pub text: String,
    /// Total size in bytes — the cursor for the next call. A float because
    /// TypeScript has no u64 and specta refuses to invent one; byte counts
    /// stay exact well past any log a kernel produces.
    pub size: f64,
    /// The file is shorter than the cursor, so the view must start over.
    pub reset: bool,
}

/// How much of a log one page may carry.
const MAX_LOG_PAGE: u64 = 256 * 1024;

/// One kernel binary this build can drive, and whether it is installed.
///
/// The UI needs this to explain *why* a start button will not work, instead of
/// letting the user click and receive a file-not-found error.
#[derive(Debug, Clone, Serialize, Type)]
pub struct EngineBinary {
    pub kind: EngineKind,
    pub name: String,
    pub path: String,
    pub present: bool,
}

struct Running {
    kind: EngineKind,
    node_name: String,
    handle: Box<dyn ProcessHandle>,
    endpoints: Endpoints,
    config_path: PathBuf,
    started_at_ms: u64,
}

/// Owns at most one running kernel.
pub struct EngineRuntime {
    spawner: Box<dyn Spawner>,
    probe: Box<dyn ReadinessProbe>,
    engines_dir: PathBuf,
    config_dir: PathBuf,
    log_dir: PathBuf,
    running: Mutex<Option<Running>>,
    last_exit_code: Mutex<Option<i32>>,
    /// Created on the first start; see [`crate::engine::job`] for why the kernel
    /// is adopted into it.
    job: OnceLock<Option<KillOnCloseJob>>,
}

impl EngineRuntime {
    pub fn new(engines_dir: PathBuf, config_dir: PathBuf, log_dir: PathBuf) -> Self {
        Self::with_dependencies(
            Box::new(SystemSpawner),
            Box::new(TcpProbe),
            engines_dir,
            config_dir,
            log_dir,
        )
    }

    pub fn with_dependencies(
        spawner: Box<dyn Spawner>,
        probe: Box<dyn ReadinessProbe>,
        engines_dir: PathBuf,
        config_dir: PathBuf,
        log_dir: PathBuf,
    ) -> Self {
        Self {
            spawner,
            probe,
            engines_dir,
            config_dir,
            log_dir,
            running: Mutex::new(None),
            last_exit_code: Mutex::new(None),
            job: OnceLock::new(),
        }
    }

    /// Put a freshly spawned kernel under the kill-on-close job, so it cannot
    /// outlive this process even if we are force-killed.
    fn adopt_into_job(&self, pid: u32) {
        let job = self.job.get_or_init(|| match KillOnCloseJob::new() {
            Ok(job) => Some(job),
            Err(error) => {
                eprintln!("x-matrix: 内核不受作业对象保护（强制退出时可能残留）：{error}");
                None
            }
        });
        if let Some(job) = job {
            if let Err(error) = job.adopt(pid) {
                eprintln!("x-matrix: 内核进程 {pid} 加入作业对象失败，强制退出时它可能残留：{error}");
            }
        }
    }

    /// Render the config, write it, spawn, wait for readiness.
    ///
    /// Any previously running kernel is stopped first: two kernels on the same
    /// inbound port is the single most common way these clients wedge.
    pub fn start(
        &self,
        kind: EngineKind,
        node: &Node,
        endpoints: Endpoints,
        options: RunOptions<'_>,
    ) -> AppResult<EngineStatus> {
        self.stop()?;

        let engine = kind.engine();
        debug_assert_eq!(engine.kind(), kind, "engine factory returned a mismatched kind");
        engine.validate_node(node)?;

        // sing-box reads rule sets from files it is pointed at; Xray inlines
        // them into the routing table. So the files only exist for sing-box,
        // and only when the user actually has sets.
        let rule_set_dir = self.rule_set_dir();
        if kind == EngineKind::SingBox {
            write_rule_set_files(&rule_set_dir, options.rule_sets)?;
        }

        let mut input = RenderInput::new(node, endpoints).with_options(options);
        if kind == EngineKind::SingBox {
            input = input.with_rule_set_dir(&rule_set_dir);
        }
        let config = engine.render_config(&input)?;
        let config_path = self.config_dir.join(format!("{}.json", kind.as_str()));
        write_atomic(
            &config_path,
            &serde_json::to_string_pretty(&config)
                .map_err(|e| AppError::Internal(e.to_string()))?,
        )?;

        let exe = self.engines_dir.join(engine.binary_name());
        let args = engine.args(&config_path.to_string_lossy());
        let log_path = self.log_dir.join(format!("{}.log", kind.as_str()));

        let mut handle = self.spawner.spawn(&exe, &args, &log_path)?;
        self.adopt_into_job(handle.pid());

        if let Err(ready_err) = self.probe.wait_for_port(endpoints.api_port, READY_TIMEOUT) {
            // A kernel that never became ready is dead weight: kill it and
            // surface the real reason.
            let _ = handle.kill();
            return Err(AppError::Engine(format!(
                "{} 启动后未就绪（{}）：{ready_err}。日志：{}",
                kind.as_str(),
                node.endpoint(),
                log_path.display()
            )));
        }

        let pid = handle.pid();
        let started_at_ms = now_ms();

        *self.running.lock() = Some(Running {
            kind,
            node_name: node.name.clone(),
            handle,
            endpoints,
            config_path: config_path.clone(),
            started_at_ms,
        });
        *self.last_exit_code.lock() = None;

        Ok(EngineStatus {
            running: true,
            kind: Some(kind),
            node_name: node.name.clone(),
            mixed_port: endpoints.mixed_port,
            api_port: endpoints.api_port,
            pid,
            config_path: config_path.to_string_lossy().into_owned(),
            started_at_ms,
            last_exit_code: None,
        })
    }

    pub fn stop(&self) -> AppResult<()> {
        let Some(mut running) = self.running.lock().take() else {
            return Ok(());
        };
        running.handle.kill()?;
        Ok(())
    }

    pub fn status(&self) -> EngineStatus {
        let mut guard = self.running.lock();

        // Detect a kernel that died on its own and clear it out, so the UI
        // never shows a stale "running" badge.
        if let Some(running) = guard.as_mut() {
            match running.handle.try_wait() {
                Ok(Some(code)) => {
                    *self.last_exit_code.lock() = Some(code);
                    let status = EngineStatus {
                        last_exit_code: Some(code),
                        ..Default::default()
                    };
                    *guard = None;
                    return status;
                }
                Ok(None) => {}
                Err(_) => {}
            }
        }

        match guard.as_ref() {
            None => EngineStatus {
                last_exit_code: *self.last_exit_code.lock(),
                ..Default::default()
            },
            Some(running) => EngineStatus {
                running: true,
                kind: Some(running.kind),
                node_name: running.node_name.clone(),
                mixed_port: running.endpoints.mixed_port,
                api_port: running.endpoints.api_port,
                pid: running.handle.pid(),
                config_path: running.config_path.to_string_lossy().into_owned(),
                started_at_ms: running.started_at_ms,
                last_exit_code: None,
            },
        }
    }

    /// Where the running kernel publishes statistics, if anything is running.
    ///
    /// The *kernel* decides the channel, so the stats reader never has to
    /// repeat a `match kind { … }` that could drift from the renderer.
    /// The tail of a kernel's log file, newest last.
    ///
    /// Only the last 64 KiB of the file is read: a kernel left running for days
    /// can produce megabytes, and the interesting part is always the end.
    pub fn log_tail(&self, kind: EngineKind, lines: usize) -> AppResult<String> {
        let path = self.log_dir.join(format!("{}.log", kind.as_str()));
        if !path.exists() {
            return Ok(String::new());
        }
        use std::io::{Read, Seek, SeekFrom};

        let mut file = std::fs::File::open(&path)?;
        let length = file.metadata()?.len();
        let window: u64 = 64 * 1024;
        if length > window {
            file.seek(SeekFrom::Start(length - window))?;
        }
        let mut buffer = String::new();
        file.read_to_string(&mut buffer)?;

        let wanted = lines.clamp(1, 2000);
        let collected: Vec<&str> = buffer.lines().rev().take(wanted).collect();
        let text: Vec<&str> = collected.into_iter().rev().collect();
        Ok(text.join("
"))
    }

    /// The log file a kernel writes to, whether or not it exists yet.
    pub fn log_path(&self, kind: EngineKind) -> PathBuf {
        self.log_dir.join(format!("{}.log", kind.as_str()))
    }

    /// How many bytes a kernel's log holds; 0 when it has never run.
    pub fn log_size(&self, kind: EngineKind) -> u64 {
        std::fs::metadata(self.log_path(kind))
            .map(|meta| meta.len())
            .unwrap_or(0)
    }

    /// Everything appended to a kernel's log since `since`, newest last.
    ///
    /// The log page polls this instead of re-reading a growing file: `size`
    /// doubles as the cursor for the next call. When the file is shorter than
    /// `since` it was rotated or cleared, and `reset` tells the view to drop
    /// what it has — otherwise the page would show yesterday's run forever.
    pub fn log_delta(&self, kind: EngineKind, since: u64) -> AppResult<LogDelta> {
        let path = self.log_path(kind);
        if !path.exists() {
            return Ok(LogDelta {
                text: String::new(),
                size: 0.0,
                reset: since != 0,
            });
        }

        let size = self.log_size(kind);
        let reset = size < since;
        let start = if reset { size.saturating_sub(MAX_LOG_PAGE) } else { since };
        // Never read more than one page at a time: a kernel that spewed during
        // a sleep/resume must not hand the webview a 40 MB string.
        let start = start.max(size.saturating_sub(MAX_LOG_PAGE));
        let mut text = String::new();
        if size > start {
            use std::io::{Read, Seek, SeekFrom};
            let mut file = std::fs::File::open(&path)?;
            file.seek(SeekFrom::Start(start))?;
            file.take(size - start).read_to_string(&mut text)?;
        }
        Ok(LogDelta {
            text,
            size: size as f64,
            reset,
        })
    }

    /// Drop a kernel's log. Used by the log page's "clear" button; a running
    /// kernel keeps the handle and starts writing at offset 0 again, which the
    /// next [`Self::log_delta`] reports as a reset.
    pub fn log_clear(&self, kind: EngineKind) -> AppResult<()> {
        let path = self.log_path(kind);
        if path.exists() {
            std::fs::File::create(&path)?;
        }
        Ok(())
    }

    /// Where sing-box rule-set files are written for a run.
    pub fn rule_set_dir(&self) -> PathBuf {
        self.config_dir.join("rule-sets")
    }

    /// Where kernels are expected to live (used to open the folder for the user).
    pub fn engines_dir(&self) -> &std::path::Path {
        &self.engines_dir
    }

    /// Where rendered kernel configs are written.
    pub fn config_dir(&self) -> &std::path::Path {
        &self.config_dir
    }

    /// Where kernel stdout/stderr goes.
    pub fn log_dir(&self) -> &std::path::Path {
        &self.log_dir
    }

    /// Every kernel binary we can drive, and whether it is on disk.
    pub fn binaries(&self) -> Vec<EngineBinary> {
        EngineKind::all()
            .iter()
            .map(|kind| {
                let name = kind.engine().binary_name().to_string();
                let path = self.engines_dir.join(&name);
                EngineBinary {
                    kind: *kind,
                    present: path.is_file(),
                    name,
                    path: path.display().to_string(),
                }
            })
            .collect()
    }

    /// Ask an installed kernel what version it is.
    ///
    /// `version` is a self-report: no config, no network, no port. Both kernels
    /// print their build line first, so the first line is the answer and the
    /// rest (a go runtime banner, a list of tags) is noise for this UI.
    pub fn version(&self, kind: EngineKind) -> AppResult<String> {
        let path = self.engines_dir.join(kind.engine().binary_name());
        if !path.is_file() {
            return Err(AppError::NotFound(format!(
                "内核目录里没有 {}",
                kind.engine().binary_name()
            )));
        }
        let output = Command::new(&path)
            .arg("version")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|error| AppError::Io(format!("{} 跑不起来：{error}", path.display())))?;
        if !output.status.success() {
            return Err(AppError::Engine(format!(
                "{} version 退出码 {}",
                kind.engine().binary_name(),
                output.status.code().unwrap_or(-1)
            )));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        first_line(&stdout).ok_or_else(|| {
            AppError::Engine(format!("{} version 没有任何输出", kind.engine().binary_name()))
        })
    }

    pub fn stats_channel(&self) -> Option<StatsChannel> {
        let guard = self.running.lock();
        let running = guard.as_ref()?;
        Some(running.kind.engine().stats_channel(running.endpoints))
    }
}

/// The first non-blank line of a kernel's `version` output.
///
/// Kept separate so the parsing is testable without a binary: some builds lead
/// with a blank line, and Windows line endings would otherwise leave a stray
/// carriage return in a label the user reads.
fn first_line(output: &str) -> Option<String> {
    output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

impl Drop for EngineRuntime {
    fn drop(&mut self) {
        // Never leave an orphaned kernel behind when the app closes.
        if let Some(mut running) = self.running.lock().take() {
            let _ = running.handle.kill();
        }
    }
}

/// Write one file per enabled rule set, in the format sing-box expects for
/// `"format": "source"`.
///
/// Rewritten on every start rather than on every set edit: the file is a
/// rendering detail of *this run*, and a set that is deleted while nothing is
/// running should not leave a stale file that a later config could point at.
fn write_rule_set_files(dir: &Path, sets: &[LoadedRuleSet]) -> AppResult<()> {
    let active: Vec<&LoadedRuleSet> = sets.iter().filter(|loaded| loaded.set.enabled).collect();
    if active.is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(dir)?;
    for loaded in &active {
        // sing-box's source format is JSON (see `singbox_rule_set_source`), so
        // the file is a `.json` document, not a line list.
        if loaded.entries.is_empty() {
            continue;
        }
        let path = dir.join(format!("{}.json", loaded.set.id));
        let text = super::render::singbox_rule_set_source(&loaded.entries);
        write_atomic(&path, &text)?;
    }

    // Drop files left behind by deleted sets (and the `.txt` spelling older
    // builds used): the directory belongs to this app, and a stale file is a
    // pattern list nobody can see in the UI.
    let keep: Vec<&str> = active.iter().map(|loaded| loaded.set.id.as_str()).collect();
    if let Ok(files) = std::fs::read_dir(dir) {
        for file in files.flatten() {
            let path = file.path();
            let stem = path.file_stem().and_then(|name| name.to_str()).unwrap_or_default();
            if !keep.contains(&stem) {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    Ok(())
}

/// Write via a temporary file plus rename, so a reader never sees a partial
/// document.
fn write_atomic(path: &Path, contents: &str) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::domain::node::{Protocol, Security, Transport};

    struct FakeHandle {
        pid: u32,
        exited: Arc<Mutex<Option<i32>>>,
        killed: Arc<Mutex<Vec<u32>>>,
    }

    impl ProcessHandle for FakeHandle {
        fn pid(&self) -> u32 {
            self.pid
        }

        fn try_wait(&mut self) -> AppResult<Option<i32>> {
            Ok(*self.exited.lock())
        }

        fn kill(&mut self) -> AppResult<()> {
            self.killed.lock().push(self.pid);
            *self.exited.lock() = Some(0);
            Ok(())
        }
    }

    struct FakeSpawner {
        spawned: Arc<Mutex<Vec<(PathBuf, Vec<String>)>>>,
        killed: Arc<Mutex<Vec<u32>>>,
        /// One exit cell per spawned process, in spawn order.
        exits: Arc<Mutex<Vec<Arc<Mutex<Option<i32>>>>>>,
        fail: bool,
    }

    impl Spawner for FakeSpawner {
        fn spawn(
            &self,
            exe: &Path,
            args: &[String],
            _log_path: &Path,
        ) -> AppResult<Box<dyn ProcessHandle>> {
            if self.fail {
                return Err(AppError::Engine("fake spawn failure".into()));
            }
            self.spawned
                .lock()
                .push((exe.to_path_buf(), args.to_vec()));
            let exited = Arc::new(Mutex::new(None));
            self.exits.lock().push(exited.clone());
            Ok(Box::new(FakeHandle {
                pid: 4242,
                exited,
                killed: self.killed.clone(),
            }))
        }
    }

    struct FakeProbe {
        ready: bool,
    }

    impl ReadinessProbe for FakeProbe {
        fn wait_for_port(&self, _port: u16, _timeout: Duration) -> AppResult<()> {
            if self.ready {
                Ok(())
            } else {
                Err(AppError::Engine("never became ready".into()))
            }
        }
    }

    struct Harness {
        runtime: EngineRuntime,
        spawned: Arc<Mutex<Vec<(PathBuf, Vec<String>)>>>,
        killed: Arc<Mutex<Vec<u32>>>,
        exits: Arc<Mutex<Vec<Arc<Mutex<Option<i32>>>>>>,
        dir: PathBuf,
    }

    impl Harness {
        /// Simulate the kernel crashing by itself.
        fn crash(&self, code: i32) {
            if let Some(cell) = self.exits.lock().last() {
                *cell.lock() = Some(code);
            }
        }
    }

    #[test]
    fn log_tail_reads_the_end_of_the_file() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("log-tail-{}", std::process::id()));
        let logs = dir.join("logs");
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::write(logs.join("xray.log"), "one
two
three
four
").unwrap();

        let runtime = EngineRuntime::new(dir.join("engines"), dir.join("configs"), logs);

        assert_eq!(
            runtime.log_tail(EngineKind::Xray, 2).unwrap(),
            "three
four"
        );
        // A kernel that never ran has no log: empty, not an error.
        assert_eq!(runtime.log_tail(EngineKind::SingBox, 10).unwrap(), "");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn log_delta_hands_back_only_what_was_appended() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("log-delta-{}", std::process::id()));
        let logs = dir.join("logs");
        std::fs::create_dir_all(&logs).unwrap();
        let path = logs.join("xray.log");
        std::fs::write(&path, "first
").unwrap();

        let runtime = EngineRuntime::new(dir.join("engines"), dir.join("configs"), logs);

        let first = runtime.log_delta(EngineKind::Xray, 0).unwrap();
        assert_eq!(first.text, "first
");
        assert_eq!(first.size, 6.0);
        assert!(!first.reset);

        // Nothing new: same cursor, no text, no reset.
        let idle = runtime.log_delta(EngineKind::Xray, first.size as u64).unwrap();
        assert_eq!(idle.text, "");
        assert_eq!(idle.size, 6.0);
        assert!(!idle.reset);

        // Appended lines arrive alone, so the view can just push them.
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(b"second
").unwrap();
        let more = runtime.log_delta(EngineKind::Xray, first.size as u64).unwrap();
        assert_eq!(more.text, "second
");
        assert_eq!(more.size, 13.0);

        // Cleared while the view holds a cursor: it must be told to start over.
        runtime.log_clear(EngineKind::Xray).unwrap();
        let after_clear = runtime.log_delta(EngineKind::Xray, more.size as u64).unwrap();
        assert!(after_clear.reset);
        assert_eq!(after_clear.text, "");
        assert_eq!(after_clear.size, 0.0);

        std::fs::remove_dir_all(&dir).ok();
    }

    fn harness(ready: bool, fail: bool) -> Harness {
        // A per-harness counter, not a timestamp: tests run in parallel and a
        // shared directory makes one test delete another's files. The path is
        // anchored to the crate, never to `%TEMP%`, because the temp directory
        // of a POSIX-ish shell maps to something unwritable on Windows.
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("runtime-{}-{}", std::process::id(), seq));
        let spawned = Arc::new(Mutex::new(Vec::new()));
        let killed = Arc::new(Mutex::new(Vec::new()));
        let exits = Arc::new(Mutex::new(Vec::new()));

        let runtime = EngineRuntime::with_dependencies(
            Box::new(FakeSpawner {
                spawned: spawned.clone(),
                killed: killed.clone(),
                exits: exits.clone(),
                fail,
            }),
            Box::new(FakeProbe { ready }),
            dir.join("engines"),
            dir.join("configs"),
            dir.join("logs"),
        );

        Harness {
            runtime,
            spawned,
            killed,
            exits,
            dir,
        }
    }

    fn node() -> Node {
        Node {
            id: "n1".into(),
            name: "测试节点".into(),
            server: "example.com".into(),
            port: 443,
            protocol: Protocol::Trojan {
                password: "pw".into(),
            },
            transport: Transport::Tcp,
            security: Security::None,
            tags: vec![],
        }
    }

    #[test]
    fn start_writes_the_config_before_spawning() {
        let h = harness(true, false);
        let endpoints = Endpoints::new(7890, 9090).unwrap();
        let status = h
            .runtime
            .start(EngineKind::Xray, &node(), endpoints, RunOptions::new())
            .unwrap();

        assert!(status.running);
        assert_eq!(status.pid, 4242);
        assert_eq!(status.mixed_port, 7890);

        let config_path = h.dir.join("configs").join("xray.json");
        let written: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
        assert_eq!(written["metrics"]["listen"], serde_json::json!("127.0.0.1:9090"));

        // The spawn used the rendered config path, not some default.
        let spawned = h.spawned.lock();
        assert_eq!(spawned.len(), 1);
        assert_eq!(spawned[0].0, h.dir.join("engines").join("xray.exe"));
        assert!(spawned[0].1.contains(&config_path.to_string_lossy().into_owned()));

        std::fs::remove_dir_all(&h.dir).ok();
    }

    #[test]
    fn a_kernel_that_never_becomes_ready_is_killed() {
        let h = harness(false, false);
        let endpoints = Endpoints::new(7890, 9090).unwrap();
        let err = h
            .runtime
            .start(EngineKind::SingBox, &node(), endpoints, RunOptions::new())
            .unwrap_err();

        let AppError::Engine(msg) = err else {
            panic!("expected an engine error");
        };
        assert!(msg.contains("未就绪"));
        // It was killed, and nothing is left marked as running.
        assert_eq!(*h.killed.lock(), vec![4242]);
        assert!(!h.runtime.status().running);

        std::fs::remove_dir_all(&h.dir).ok();
    }

    #[test]
    fn starting_again_stops_the_previous_kernel() {
        let h = harness(true, false);
        let endpoints = Endpoints::new(7890, 9090).unwrap();
        h.runtime
            .start(EngineKind::Xray, &node(), endpoints, RunOptions::new())
            .unwrap();
        h.runtime
            .start(EngineKind::SingBox, &node(), endpoints, RunOptions::new())
            .unwrap();

        assert_eq!(*h.killed.lock(), vec![4242]);
        assert_eq!(h.runtime.status().kind, Some(EngineKind::SingBox));

        std::fs::remove_dir_all(&h.dir).ok();
    }

    #[test]
    fn stop_is_idempotent() {
        let h = harness(true, false);
        h.runtime.stop().unwrap();
        assert!(!h.runtime.status().running);
        std::fs::remove_dir_all(&h.dir).ok();
    }

    #[test]
    fn stop_kills_the_running_kernel() {
        let h = harness(true, false);
        let endpoints = Endpoints::new(7890, 9090).unwrap();
        h.runtime
            .start(EngineKind::Xray, &node(), endpoints, RunOptions::new())
            .unwrap();
        h.runtime.stop().unwrap();
        assert_eq!(*h.killed.lock(), vec![4242]);
        assert!(h.runtime.stats_channel().is_none());
        std::fs::remove_dir_all(&h.dir).ok();
    }

    #[test]
    fn a_kernel_that_exits_on_its_own_is_reported_as_stopped() {
        let h = harness(true, false);
        let endpoints = Endpoints::new(7890, 9090).unwrap();
        h.runtime
            .start(EngineKind::Xray, &node(), endpoints, RunOptions::new())
            .unwrap();

        // Simulate the kernel crashing.
        h.crash(1);
        let status = h.runtime.status();
        assert!(!status.running);
        assert_eq!(status.last_exit_code, Some(1));

        std::fs::remove_dir_all(&h.dir).ok();
    }

    #[test]
    fn spawn_failure_surfaces_as_an_engine_error() {
        let h = harness(true, true);
        let endpoints = Endpoints::new(7890, 9090).unwrap();
        let err = h
            .runtime
            .start(EngineKind::Xray, &node(), endpoints, RunOptions::new())
            .unwrap_err();
        assert!(matches!(err, AppError::Engine(_)));
        assert!(!h.runtime.status().running);
        std::fs::remove_dir_all(&h.dir).ok();
    }

    #[test]
    fn an_invalid_node_is_rejected_before_any_file_is_written() {
        let h = harness(true, false);
        let mut bad = node();
        bad.server = String::new();
        let endpoints = Endpoints::new(7890, 9090).unwrap();
        assert!(h
            .runtime
            .start(EngineKind::Xray, &bad, endpoints, RunOptions::new())
            .is_err());
        assert!(!h.dir.join("configs").join("xray.json").exists());
        assert!(h.spawned.lock().is_empty());
        std::fs::remove_dir_all(&h.dir).ok();
    }

    #[test]
    fn the_version_line_is_the_first_non_blank_one() {
        // Real output starts with a banner line and then a go runtime line;
        // some builds put a blank line first, which must not become the label.
        assert_eq!(
            super::first_line("
Xray 26.11.9 (Xray, Penetrates Everything.)
A unified platform"),
            Some("Xray 26.11.9 (Xray, Penetrates Everything.)".to_string())
        );
        assert_eq!(super::first_line("   

"), None);
    }

    #[test]
    fn a_missing_binary_is_reported_as_not_found() {
        let h = harness(false, false);
        let err = h.runtime.version(EngineKind::Xray).unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
        std::fs::remove_dir_all(&h.dir).ok();
    }
}
