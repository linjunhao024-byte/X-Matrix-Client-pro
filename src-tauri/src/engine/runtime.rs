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

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use specta::Type;

use super::job::KillOnCloseJob;
use super::{groupwatch, Endpoints, EngineKind, LoadedRuleSet, RenderInput, RunOptions, StatsChannel};
use crate::domain::group::GroupKind;
use crate::domain::node::Node;
use crate::domain::routing::RoutingRule;
use crate::domain::settings::ProxyMode;
use crate::domain::settings::LogLevel;
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
pub struct SystemSpawner {
    /// Where the geo data files live. A kernel the user pointed at may sit in a
    /// directory of its own, but the data files stay with the engines; Xray
    /// only looks in one place, so prefer the kernel's directory when it has
    /// the files and fall back to this one otherwise.
    asset_dir: PathBuf,
}

/// The directory to hand to Xray as `XRAY_LOCATION_ASSET`: the kernel's own
/// folder when it carries the data files, otherwise the engines directory.
pub(crate) fn asset_dir_for(exe: &Path, engines_dir: &Path) -> PathBuf {
    match exe.parent() {
        Some(dir) if dir.join("geosite.dat").exists() || dir.join("geoip.dat").exists() => {
            dir.to_path_buf()
        }
        _ => engines_dir.to_path_buf(),
    }
}

impl SystemSpawner {
    fn new(asset_dir: PathBuf) -> Self {
        Self { asset_dir }
    }
}

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

        // Xray looks for geoip.dat / geosite.dat next to itself only when it is
        // told to; without this it searches the directory the GUI happened to
        // be started from, which is not a place a user can reason about. A
        // user-chosen kernel usually sits alone in its own folder while the
        // data files stay in the engines directory — pointing Xray at the exe
        // there made every `geosite:` / `geoip:` rule fail to load, so the
        // kernel's own folder only wins when it actually carries the files.
        command.env("XRAY_LOCATION_ASSET", asset_dir_for(exe, &self.asset_dir));

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
    /// The node this kernel was started with. A name is not enough: two nodes
    /// may share one.
    pub node_id: Option<String>,
    pub mixed_port: u16,
    /// The HTTP inbound the running kernel actually listens on.
    ///
    /// Equal to [`EngineStatus::mixed_port`] unless the settings split the two
    /// protocols (`0` there means "follow `mixed_port`"). This is the port the
    /// system proxy and every probe point at, so the UI shows it instead of
    /// making the reader redo the fallback.
    pub http_port: u16,
    /// The SOCKS inbound the running kernel actually listens on; resolved the
    /// same way as [`EngineStatus::http_port`].
    pub socks_port: u16,
    pub api_port: u16,
    pub pid: u32,
    pub config_path: String,
    // Specta refuses u64; f64 keeps millisecond timestamps exact.
    #[specta(type = f64)]
    pub started_at_ms: u64,
    /// Set when a kernel exited on its own, so the UI can explain the stop
    /// instead of silently flipping to "not running".
    pub last_exit_code: Option<i32>,
    /// The mode the running kernel was started with (`rule` / `global` /
    /// `direct`). The routing table is rendered from it at start, so the
    /// settings page needs this to tell a saved mode from the one in force.
    pub mode: ProxyMode,
    /// The level the running kernel was started with. The log level is baked
    /// into the rendered config, so the UI needs this to tell a saved-but-not-
    /// applied change from an applied one.
    pub log_level: LogLevel,
    /// Whether the running kernel listens on all interfaces. Baked into the
    /// rendered config, so the settings page needs it for the same reason as
    /// [`EngineStatus::log_level`].
    pub allow_lan: bool,
    /// The resolver list the running kernel was started with, verbatim from the
    /// settings; empty means the kernel uses the system resolver.
    pub dns_servers: String,
    /// Whether the running kernel multiplexes connections. Baked into the
    /// rendered config, so the settings page needs it to tell a saved change
    /// from one the kernel actually honours, same as
    /// [`EngineStatus::log_level`].
    pub mux_enabled: bool,
    /// The stream cap the running kernel was started with. Only meaningful
    /// while [`EngineStatus::mux_enabled`]; a stopped kernel reports the
    /// default instead of the last used value.
    pub mux_concurrency: u16,
    /// Whether the running kernel was started with TLS fragmentation. Xray
    /// bakes it into `tlsSettings`, so the settings page needs it for the same
    /// reason as [`EngineStatus::mux_enabled`]: to tell a saved change from an
    /// applied one.
    pub fragment_enabled: bool,
    /// The piece-size range the running kernel was started with, verbatim from
    /// the settings; only meaningful while [`EngineStatus::fragment_enabled`].
    pub fragment_length: String,
    /// The delay range the running kernel was started with, verbatim from the
    /// settings; only meaningful while [`EngineStatus::fragment_enabled`].
    pub fragment_interval: String,
    /// The probe address the running kernel was started with. sing-box bakes it
    /// into every `urltest` group, so a saved change to 测试地址 has to be
    /// reported the same way as the other rendered settings.
    pub test_url: String,
}

impl Default for EngineStatus {
    fn default() -> Self {
        Self {
            running: false,
            kind: None,
            node_name: String::new(),
            node_id: None,
            mixed_port: 0,
            http_port: 0,
            socks_port: 0,
            api_port: 0,
            pid: 0,
            config_path: String::new(),
            started_at_ms: 0,
            last_exit_code: None,
            mode: ProxyMode::Rule,
            log_level: LogLevel::Warn,
            allow_lan: false,
            dns_servers: String::new(),
            mux_enabled: false,
            mux_concurrency: 8,
            fragment_enabled: false,
            fragment_length: "100-200".to_string(),
            fragment_interval: "10-20".to_string(),
            test_url: String::new(),
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
    /// The node this kernel was started with; see [`EngineStatus::node_id`].
    node_id: String,
    handle: Box<dyn ProcessHandle>,
    endpoints: Endpoints,
    config_path: PathBuf,
    started_at_ms: u64,
    /// The routing mode this kernel was started with; see
    /// [`EngineStatus::mode`].
    mode: ProxyMode,
    /// The level this kernel was started with; see [`EngineStatus::log_level`].
    log_level: LogLevel,
    /// The LAN opt-in this kernel was started with; see
    /// [`EngineStatus::allow_lan`].
    allow_lan: bool,
    /// The resolver list this kernel was started with; see
    /// [`EngineStatus::dns_servers`].
    dns_servers: String,
    /// The multiplexing opt-in this kernel was started with; see
    /// [`EngineStatus::mux_enabled`].
    mux_enabled: bool,
    /// The stream cap this kernel was started with; see
    /// [`EngineStatus::mux_concurrency`].
    mux_concurrency: u16,
    /// The TLS fragmentation opt-in this kernel was started with; see
    /// [`EngineStatus::fragment_enabled`].
    fragment_enabled: bool,
    /// The piece-size range this kernel was started with; see
    /// [`EngineStatus::fragment_length`].
    fragment_length: String,
    /// The delay range this kernel was started with; see
    /// [`EngineStatus::fragment_interval`].
    fragment_interval: String,
    /// The probe address this kernel was started with; see
    /// [`EngineStatus::test_url`].
    test_url: String,
    /// True once [`EngineRuntime::stop`] asked this kernel to go away. Set
    /// before the kill because a kill that reports an error can still have
    /// taken the process down: either way the exit was the user's decision, not
    /// a crash worth a notification.
    expected: bool,
    /// True when the kernel was started by the installed service: this process
    /// has no child handle to kill, so stopping goes through a job file.
    via_service: bool,
}

/// Owns at most one running kernel.
pub struct EngineRuntime {
    spawner: Box<dyn Spawner>,
    probe: Box<dyn ReadinessProbe>,
    engines_dir: PathBuf,
    config_dir: PathBuf,
    log_dir: PathBuf,
    /// Per-kernel executables the user pointed us at (see
    /// [`EngineRuntime::binary_path`]). Empty for a kernel means "the engines
    /// directory", which keeps the default path out of a map that exists for
    /// the exceptions.
    overrides: RwLock<HashMap<EngineKind, PathBuf>>,
    running: Mutex<Option<Running>>,
    last_exit_code: Mutex<Option<i32>>,
    /// Exit code of a kernel that died on its own, waiting to be announced.
    /// [`EngineRuntime::take_unexpected_exit`] drains it, so one crash is one
    /// notification no matter how often the status is polled.
    unexpected_exit: Mutex<Option<i32>>,
    /// Created on the first start; see [`crate::engine::job`] for why the kernel
    /// is adopted into it.
    job: OnceLock<Option<KillOnCloseJob>>,
    /// When a kernel service is installed and running, kernels are started by
    /// it (as SYSTEM) instead of by this process. `None` means "spawn locally",
    /// which is also what an ordinary user without the service gets.
    service: RwLock<Option<Arc<Mutex<crate::service_client::ServiceClient>>>>,
    /// The stop flag of the group watcher for the current kernel, when one is
    /// running. Setting it is the only way that thread is told to go away, so
    /// [`EngineRuntime::stop`] takes it (leaving `None`) before killing the
    /// kernel it was probing.
    watcher: Mutex<Option<Arc<AtomicBool>>>,
}

impl EngineRuntime {
    pub fn new(engines_dir: PathBuf, config_dir: PathBuf, log_dir: PathBuf) -> Self {
        Self::with_dependencies(
            Box::new(SystemSpawner::new(engines_dir.clone())),
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
            overrides: RwLock::new(HashMap::new()),
            running: Mutex::new(None),
            last_exit_code: Mutex::new(None),
            unexpected_exit: Mutex::new(None),
            service: RwLock::new(None),
            job: OnceLock::new(),
            watcher: Mutex::new(None),
        }
    }

    /// Hand kernels to the installed service (see [`crate::service`]) instead
    /// of spawning them here. The service decides which executables may run, so
    /// this is also how a TUN inbound gets an administrator token without the
    /// GUI holding one.
    pub fn set_service(&self, client: Option<Arc<Mutex<crate::service_client::ServiceClient>>>) {
        *self.service.write() = client;
    }

    /// Re-probe the service after an install or uninstall, so the next start
    /// uses it (or stops using it) without restarting the app.
    pub fn refresh_service(&self) {
        self.set_service(
            crate::service_client::probe().map(|client| Arc::new(Mutex::new(client))),
        );
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
            // A geo matcher is a rule-set too, and the file has to exist before
            // the kernel boots — see `prepare_geo_rule_sets`.
            prepare_geo_rule_sets(&rule_set_dir, options.rules)?;
        }

        // The log level is rendered into the config, so the options are the
        // only place it exists before the config is written — read it before
        // `with_options` consumes them. Same for the DNS/LAN/mux/fragment
        // settings: the UI compares them with the saved ones, and the rendered
        // config cannot give the original text back.
        let mode = options.mode;
        let log_level = options.log_level;
        let allow_lan = options.allow_lan;
        let dns_servers = options.dns_servers_raw.clone();
        let mux_enabled = options.mux_enabled;
        let mux_concurrency = options.mux_concurrency;
        let fragment_enabled = options.fragment_enabled;
        let fragment_length = options.fragment_length.clone();
        let fragment_interval = options.fragment_interval.clone();
        let test_url = options.probe_url.clone();
        // Neither kernel has a fallback or load-balance outbound: those two
        // kinds are rendered as a plain selector and moved from here, through
        // the control API. Collect what the watcher needs before `with_options`
        // consumes the groups. Xray gets nothing on purpose — it has no
        // selector to rewrite, so a watcher there would only log.
        let watch_groups: Vec<groupwatch::GroupSpec> = if kind == EngineKind::SingBox {
            options
                .groups
                .iter()
                .filter(|group| {
                    group.enabled
                        && matches!(group.kind, GroupKind::Fallback | GroupKind::LoadBalance)
                })
                .map(|group| groupwatch::GroupSpec {
                    id: group.id.clone(),
                    kind: group.kind,
                    members: group.members.clone(),
                })
                .collect()
        } else {
            Vec::new()
        };
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

        let exe = self.binary_path(kind);
        let args = engine.args(&config_path.to_string_lossy());
        let log_path = self.log_dir.join(format!("{}.log", kind.as_str()));

        let service = self.service.read().clone();
        let via_service = service.is_some();
        let mut handle: Box<dyn ProcessHandle> = match service {
            Some(client) => {
                let config = fs::read_to_string(&config_path)
                    .map_err(|e| AppError::Io(format!("读不到刚写好的配置：{e}")))?;
                let pid = client.lock().start(kind, &config, endpoints.api_port)?;
                Box::new(crate::service_client::ServiceProcess { pid, kind, client })
            }
            None => {
                let handle = self.spawner.spawn(&exe, &args, &log_path)?;
                self.adopt_into_job(handle.pid());
                handle
            }
        };

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
            node_id: node.id.clone(),
            handle,
            endpoints,
            config_path: config_path.clone(),
            started_at_ms,
            mode,
            log_level,
            allow_lan,
            dns_servers: dns_servers.clone(),
            mux_enabled,
            mux_concurrency,
            fragment_enabled,
            fragment_length: fragment_length.clone(),
            fragment_interval: fragment_interval.clone(),
            test_url: test_url.clone(),
            expected: false,
            via_service,
        });
        *self.last_exit_code.lock() = None;
        // A report left over from the previous kernel must not be announced
        // now: the user already restarted, and a notification for a dead
        // process would contradict the "running" badge.
        *self.unexpected_exit.lock() = None;

        // Only after readiness: before this line there is no control API to
        // ask, and a probing thread would spend its budget on refused
        // connections. The thread is detached on purpose — `stop` trips the
        // flag and the kernel outlives the last probe by well under a second.
        if !watch_groups.is_empty() {
            let stop = Arc::new(AtomicBool::new(false));
            *self.watcher.lock() = Some(Arc::clone(&stop));
            let runner = groupwatch::Runner::new(endpoints.api_port, test_url.clone(), watch_groups);
            std::thread::spawn(move || groupwatch::run(runner, stop));
        }

        Ok(EngineStatus {
            running: true,
            mode,
            kind: Some(kind),
            node_name: node.name.clone(),
            node_id: Some(node.id.clone()),
            mixed_port: endpoints.mixed_port,
            http_port: endpoints.http_port,
            socks_port: endpoints.socks_port,
            api_port: endpoints.api_port,
            pid,
            config_path: config_path.to_string_lossy().into_owned(),
            started_at_ms,
            last_exit_code: None,
            log_level,
            allow_lan,
            dns_servers,
            mux_enabled,
            mux_concurrency,
            fragment_enabled,
            fragment_length,
            fragment_interval,
            test_url,
        })
    }

    /// Signal the group watcher (if any) to exit, and forget its flag so a
    /// second call cannot re-signal a thread that is already gone.
    ///
    /// Always called while `running` is held, which is the single lock order
    /// this type uses (`running` then `watcher`): taking them the other way
    /// round in one method would let `status` and `stop` deadlock.
    fn stop_watcher(&self) {
        if let Some(stop) = self.watcher.lock().take() {
            stop.store(true, Ordering::SeqCst);
        }
    }

    pub fn stop(&self) -> AppResult<()> {
        let mut guard = self.running.lock();
        // Wake the group watcher before killing the process it probes: it is
        // about to lose its control port, and a probe against a closed one only
        // produces noise.
        self.stop_watcher();
        let Some(running) = guard.as_mut() else {
            return Ok(());
        };
        // Mark first, kill second: the user asked for this stop, so whatever
        // the process does on the way out is expected and must never be
        // announced as a crash — not even if `kill` returns an error after
        // having taken the process down.
        running.expected = true;
        if running.via_service {
            // The kernel runs as SYSTEM under the service. This token may not
            // even be allowed to signal it, and a half-performed kill would be
            // a lie: let the service stop its own child, then forget the state.
            if let Some(client) = self.service.read().clone() {
                client.lock().stop(running.kind)?;
            } else {
                // The service vanished between the start and this stop; the
                // handle still holds its own client, so it can say what is left.
                running.handle.kill()?;
            }
            *guard = None;
            return Ok(());
        }
        running.handle.kill()?;
        *guard = None;
        Ok(())
    }

    /// Re-point a running kernel at the id its node has now.
    ///
    /// A subscription refresh mints a fresh id for every row it imports, so the
    /// same server comes back under a new id and the id the kernel was started
    /// with names nothing. Adopting the new id keeps "which node is this kernel
    /// running" true, so a later delete of that node still finds it — and the
    /// kernel is not restarted just because a provider renamed a server.
    ///
    /// A no-op when nothing is running, or when the running kernel is on some
    /// other node: moving an id that belongs to a different kernel would be a
    /// lie about what is being proxied.
    pub fn adopt_node_id(&self, from: &str, to: &str) {
        let mut guard = self.running.lock();
        if let Some(running) = guard.as_mut() {
            if running.node_id == from {
                running.node_id = to.to_string();
            }
        }
    }

    /// The exit code of a kernel that died on its own, drained on read.
    ///
    /// `status()` is what notices the death; this is how the rest of the app
    /// hears about it without polling the badge. Taking the code is
    /// deliberately destructive: one exit is reported exactly once, so a
    /// watcher that ticks every second does not repeat itself.
    ///
    /// A kernel stopped through [`Self::stop`] never lands here, and neither
    /// does one that was killed because it never became ready — those exits
    /// are ours, not the kernel's.
    pub fn take_unexpected_exit(&self) -> Option<i32> {
        self.unexpected_exit.lock().take()
    }

    pub fn status(&self) -> EngineStatus {
        let mut guard = self.running.lock();

        // Detect a kernel that died on its own and clear it out, so the UI
        // never shows a stale "running" badge.
        if let Some(running) = guard.as_mut() {
            match running.handle.try_wait() {
                Ok(Some(code)) => {
                    let unexpected = !running.expected;
                    *guard = None;
                    // The kernel is gone; there is no control port left for
                    // the watcher to probe.
                    self.stop_watcher();
                    *self.last_exit_code.lock() = Some(code);
                    if unexpected {
                        *self.unexpected_exit.lock() = Some(code);
                    }
                    let status = EngineStatus {
                        last_exit_code: Some(code),
                        ..Default::default()
                    };
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
                node_id: Some(running.node_id.clone()),
                mixed_port: running.endpoints.mixed_port,
                http_port: running.endpoints.http_port,
                socks_port: running.endpoints.socks_port,
                api_port: running.endpoints.api_port,
                pid: running.handle.pid(),
                config_path: running.config_path.to_string_lossy().into_owned(),
                started_at_ms: running.started_at_ms,
                last_exit_code: None,
                mode: running.mode,
                log_level: running.log_level,
                allow_lan: running.allow_lan,
                dns_servers: running.dns_servers.clone(),
                mux_enabled: running.mux_enabled,
                mux_concurrency: running.mux_concurrency,
                fragment_enabled: running.fragment_enabled,
                fragment_length: running.fragment_length.clone(),
                fragment_interval: running.fragment_interval.clone(),
                test_url: running.test_url.clone(),
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

    /// The executable this runtime would start for `kind`.
    ///
    /// One resolver for spawn, [`Self::version`] and [`Self::binaries`]: if the
    /// three disagreed, the settings page could report a version from one file
    /// while the start button spawned another — the classic "the version says
    /// 26.x but it behaves like 24.x" bug. An explicit override wins; an empty
    /// one falls back to the engines directory.
    pub fn binary_path(&self, kind: EngineKind) -> PathBuf {
        if let Some(path) = self.overrides.read().get(&kind) {
            return path.clone();
        }
        self.engines_dir.join(kind.engine().binary_name())
    }

    /// Point one kernel at an explicit executable, or clear the override.
    ///
    /// `None` means "back to the engines directory", which is what an empty
    /// setting maps to. Only the named kernel changes: a user can run the
    /// upstream Xray and a self-built sing-box at the same time.
    pub fn set_binary(&self, kind: EngineKind, path: Option<PathBuf>) {
        let mut overrides = self.overrides.write();
        match path {
            Some(path) => {
                overrides.insert(kind, path);
            }
            None => {
                overrides.remove(&kind);
            }
        }
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
    ///
    /// `path` is the *effective* file — an override is reported as it is, so the
    /// settings page shows the same path the next start will use. `name` stays
    /// the engine's own binary name: it says which kernel this row is about,
    /// not what the user renamed the file to.
    pub fn binaries(&self) -> Vec<EngineBinary> {
        EngineKind::all()
            .iter()
            .map(|kind| {
                let name = kind.engine().binary_name().to_string();
                let path = self.binary_path(*kind);
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
        let path = self.binary_path(kind);
        if !path.is_file() {
            // Name the path, not just the folder: with an override the user is
            // looking somewhere else entirely, so "内核目录里没有 xray.exe" would
            // send them to the wrong place.
            return Err(AppError::NotFound(format!(
                "找不到 {} 内核文件：{}",
                kind.as_str(),
                path.display()
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
                path.display(),
                output.status.code().unwrap_or(-1)
            )));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        first_line(&stdout).ok_or_else(|| {
            AppError::Engine(format!("{} version 没有任何输出", path.display()))
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
            self.stop_watcher();
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
/// Make sure every `geosite:`/`geoip:` matcher a rule uses has its `.srs` file.
///
/// Files live in `<config dir>/rule-sets/geo/` and survive restarts, so the CDN
/// is asked once per name. A failure is reported before the kernel is spawned:
/// "下载失败：…" is a sentence a user can act on, whereas letting sing-box try
/// it produces `内核在 8 秒内没有打开控制端口 9090`.
fn prepare_geo_rule_sets(dir: &Path, rules: &[RoutingRule]) -> AppResult<()> {
    use crate::domain::routing::RuleMatcher;
    use crate::engine::geo;

    let mut wanted: Vec<(&str, &str)> = Vec::new();
    for rule in rules.iter().filter(|rule| rule.enabled) {
        let pair = match &rule.matcher {
            RuleMatcher::GeoSite { value } => ("geosite", value.as_str()),
            RuleMatcher::GeoIp { value } => ("geoip", value.as_str()),
            _ => continue,
        };
        if !wanted.contains(&pair) {
            wanted.push(pair);
        }
    }
    if wanted.is_empty() {
        return Ok(());
    }

    let geo_dir = dir.join(geo::RULE_SET_SUBDIR);
    std::fs::create_dir_all(&geo_dir)?;
    for (kind, name) in wanted {
        let path = geo_dir.join(format!("{kind}-{name}.srs"));
        // A `.srs` file starts with the magic "SRS" plus a format byte. Anything
        // else on disk is a leftover from a failed write — older builds stored
        // the body as text, and the kernel answered `flate: corrupt input`.
        if geo::is_rule_set_file(&path) {
            continue;
        }
        let noun = format!("{kind}:{name} 规则集");
        let mut attempts: Vec<String> = Vec::new();
        let mut saved = false;
        for url in geo::rule_set_candidates(kind, name) {
            match crate::net::remote::fetch_bytes(&url, std::time::Duration::from_secs(30), &noun) {
                Ok((200..=299, body)) if body.len() > 64 => {
                    // Bytes, not text: a `.srs` file is a binary document, and
                    // writing it through `String::from_utf8_lossy` replaced
                    // every non-UTF-8 byte with U+FFFD — the kernel then said
                    // `flate: corrupt input before offset 10`.
                    let temp = path.with_extension("srs.tmp");
                    std::fs::write(&temp, &body)?;
                    std::fs::rename(&temp, &path)?;
                    saved = true;
                    break;
                }
                Ok((status, _)) => attempts.push(format!("返回 HTTP {status}")),
                Err(error) => attempts.push(format!("{error}")),
            }
        }
        if !saved {
            return Err(AppError::Engine(format!(
                "{noun} 下载失败：{}",
                attempts.join("；")
            )));
        }
    }
    Ok(())
}

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
pub(crate) mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::domain::node::{Protocol, Security, Transport};

    /// A kernel the user picked elsewhere carries its own data files, so they
    /// win; a bare binary in a folder of its own must still borrow the engines
    /// directory, or every `geosite:` rule fails to load.
    #[test]
    fn the_kernel_folder_wins_only_when_it_has_the_data_files() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("asset-dir-tests");
        let _ = std::fs::remove_dir_all(&root);
        let engines = root.join("engines");
        let portable = root.join("portable");
        let bare = root.join("bare");
        std::fs::create_dir_all(&engines).unwrap();
        std::fs::create_dir_all(&portable).unwrap();
        std::fs::create_dir_all(&bare).unwrap();
        std::fs::write(portable.join("geosite.dat"), b"x").unwrap();

        assert_eq!(
            asset_dir_for(&portable.join("xray.exe"), &engines),
            portable
        );
        assert_eq!(asset_dir_for(&bare.join("xray.exe"), &engines), engines);
        assert_eq!(asset_dir_for(Path::new("xray.exe"), &engines), engines);

        let _ = std::fs::remove_dir_all(&root);
    }

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

    pub(crate) struct Harness {
        /// Behind an `Arc` because `start_engine` takes the runtime the same way
        /// the app state holds it; tests then call it exactly like production.
        pub(crate) runtime: Arc<EngineRuntime>,
        spawned: Arc<Mutex<Vec<(PathBuf, Vec<String>)>>>,
        killed: Arc<Mutex<Vec<u32>>>,
        exits: Arc<Mutex<Vec<Arc<Mutex<Option<i32>>>>>>,
        pub(crate) dir: PathBuf,
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

    pub(crate) fn harness(ready: bool, fail: bool) -> Harness {
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
            runtime: Arc::new(runtime),
            spawned,
            killed,
            exits,
            dir,
        }
    }

    pub(crate) fn node() -> Node {
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
        let endpoints = Endpoints::new(7890, 0, 0, 9090).unwrap();
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
    fn the_default_binary_lives_in_the_engines_directory() {
        let h = harness(true, false);
        assert_eq!(
            h.runtime.binary_path(EngineKind::Xray),
            h.dir.join("engines").join("xray.exe")
        );
        assert_eq!(
            h.runtime.binary_path(EngineKind::SingBox),
            h.dir.join("engines").join("sing-box.exe")
        );

        // No override, no binary on disk: the report says so instead of
        // pointing at a file that would fail at spawn time.
        let xray = h
            .runtime
            .binaries()
            .into_iter()
            .find(|binary| binary.kind == EngineKind::Xray)
            .expect("xray is always reported");
        assert_eq!(xray.path, h.dir.join("engines").join("xray.exe").display().to_string());
        assert!(!xray.present);
        assert_eq!(xray.name, "xray.exe");

        std::fs::remove_dir_all(&h.dir).ok();
    }

    #[test]
    fn an_explicit_binary_wins_and_is_what_gets_spawned() {
        let h = harness(true, false);
        let custom = h.dir.join("portable").join("Xray-26.11.exe");
        std::fs::create_dir_all(custom.parent().unwrap()).unwrap();
        std::fs::write(&custom, b"").unwrap();

        h.runtime.set_binary(EngineKind::Xray, Some(custom.clone()));

        assert_eq!(h.runtime.binary_path(EngineKind::Xray), custom);
        // Overrides are per kernel: sing-box still resolves through engines/.
        assert_eq!(
            h.runtime.binary_path(EngineKind::SingBox),
            h.dir.join("engines").join("sing-box.exe")
        );

        // The same resolution feeds the spawn ...
        h.runtime
            .start(
                EngineKind::Xray,
                &node(),
                Endpoints::new(7890, 0, 0, 9090).unwrap(),
                RunOptions::new(),
            )
            .unwrap();
        assert_eq!(h.spawned.lock()[0].0, custom);

        // ... and the row the settings page displays.
        let xray = h
            .runtime
            .binaries()
            .into_iter()
            .find(|binary| binary.kind == EngineKind::Xray)
            .expect("xray is always reported");
        assert_eq!(xray.path, custom.display().to_string());
        assert!(xray.present);
        assert_eq!(xray.name, "xray.exe");

        std::fs::remove_dir_all(&h.dir).ok();
    }

    #[test]
    fn clearing_the_override_restores_the_default() {
        let h = harness(true, false);
        let custom = h.dir.join("portable").join("xray.exe");
        h.runtime.set_binary(EngineKind::Xray, Some(custom.clone()));
        assert_eq!(h.runtime.binary_path(EngineKind::Xray), custom);

        h.runtime.set_binary(EngineKind::Xray, None);
        assert_eq!(
            h.runtime.binary_path(EngineKind::Xray),
            h.dir.join("engines").join("xray.exe")
        );
        assert!(h
            .runtime
            .binaries()
            .iter()
            .all(|binary| !binary.path.contains("portable")));

        std::fs::remove_dir_all(&h.dir).ok();
    }

    #[test]
    fn a_missing_override_names_the_path_it_looked_at() {
        let h = harness(false, false);
        let missing = h.dir.join("nowhere").join("custom-xray.exe");
        h.runtime.set_binary(EngineKind::Xray, Some(missing.clone()));

        let err = h.runtime.version(EngineKind::Xray).unwrap_err();
        let AppError::NotFound(message) = err else {
            panic!("a missing binary must be NotFound, got a different error");
        };
        // The user chose this file, so the message has to point at it — not at
        // the engines folder they are no longer using.
        assert!(message.contains(&missing.display().to_string()), "message was: {message}");

        std::fs::remove_dir_all(&h.dir).ok();
    }

    #[test]
    fn a_kernel_that_never_becomes_ready_is_killed() {
        let h = harness(false, false);
        let endpoints = Endpoints::new(7890, 0, 0, 9090).unwrap();
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
        let endpoints = Endpoints::new(7890, 0, 0, 9090).unwrap();
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
    fn a_running_kernel_adopts_the_new_id_of_its_own_node() {
        let h = harness(true, false);
        let endpoints = Endpoints::new(7890, 0, 0, 9090).unwrap();
        h.runtime
            .start(EngineKind::Xray, &node(), endpoints, RunOptions::new())
            .unwrap();

        // A refresh re-imports the same server under a fresh id: the kernel
        // follows that id so a later delete of the node still finds it.
        h.runtime.adopt_node_id("n1", "n9");
        let adopted = h.runtime.status();
        assert_eq!(adopted.node_id.as_deref(), Some("n9"));
        assert!(adopted.running, "adoption is a rename, not a restart");
        assert_eq!(adopted.pid, 4242);
        assert!(h.killed.lock().is_empty(), "the kernel was not touched");

        // An id that belongs to another node is not ours to move.
        h.runtime.adopt_node_id("n2", "n3");
        assert_eq!(h.runtime.status().node_id.as_deref(), Some("n9"));

        std::fs::remove_dir_all(&h.dir).ok();
    }

    #[test]
    fn adopting_an_id_with_nothing_running_is_a_no_op() {
        let h = harness(true, false);
        h.runtime.adopt_node_id("n1", "n2");
        let status = h.runtime.status();
        assert!(!status.running);
        assert_eq!(status.node_id, None);

        std::fs::remove_dir_all(&h.dir).ok();
    }

    #[test]
    fn stop_kills_the_running_kernel() {
        let h = harness(true, false);
        let endpoints = Endpoints::new(7890, 0, 0, 9090).unwrap();
        h.runtime
            .start(EngineKind::Xray, &node(), endpoints, RunOptions::new())
            .unwrap();
        h.runtime.stop().unwrap();
        assert_eq!(*h.killed.lock(), vec![4242]);
        assert!(h.runtime.stats_channel().is_none());
        std::fs::remove_dir_all(&h.dir).ok();
    }

    #[test]
    fn status_reports_the_level_the_kernel_was_started_with() {
        let h = harness(true, false);
        let endpoints = Endpoints::new(7890, 0, 0, 9090).unwrap();
        let options = RunOptions {
            log_level: LogLevel::Debug,
            ..RunOptions::new()
        };
        let started = h
            .runtime
            .start(EngineKind::Xray, &node(), endpoints, options)
            .unwrap();

        // Both the start result and the polled status carry the level the
        // config was rendered with, so the settings page can compare it with
        // the saved value.
        assert_eq!(started.log_level, LogLevel::Debug);
        assert_eq!(h.runtime.status().log_level, LogLevel::Debug);

        std::fs::remove_dir_all(&h.dir).ok();
    }

    #[test]
    fn status_reports_the_mode_the_kernel_was_started_with() {
        let h = harness(true, false);
        let endpoints = Endpoints::new(7890, 0, 0, 9090).unwrap();
        let options = RunOptions {
            mode: ProxyMode::Global,
            ..RunOptions::new()
        };
        let started = h
            .runtime
            .start(EngineKind::Xray, &node(), endpoints, options)
            .unwrap();

        // The routing table is rendered from the mode, so the settings page
        // compares this against the saved value instead of guessing.
        assert_eq!(started.mode, ProxyMode::Global);
        assert_eq!(h.runtime.status().mode, ProxyMode::Global);

        std::fs::remove_dir_all(&h.dir).ok();
    }

    #[test]
    fn a_stopped_kernel_reports_the_default_level() {
        let h = harness(true, false);
        let endpoints = Endpoints::new(7890, 0, 0, 9090).unwrap();
        let options = RunOptions {
            log_level: LogLevel::Debug,
            ..RunOptions::new()
        };
        h.runtime
            .start(EngineKind::Xray, &node(), endpoints, options)
            .unwrap();

        h.runtime.stop().unwrap();
        // Nothing runs, so no level is applied: the UI must fall back to the
        // domain default rather than claim the debug kernel is still alive.
        let status = h.runtime.status();
        assert!(!status.running);
        assert_eq!(status.log_level, LogLevel::Warn);

        std::fs::remove_dir_all(&h.dir).ok();
    }

    #[test]
    fn status_reports_the_lan_and_dns_the_kernel_was_started_with() {
        let h = harness(true, false);
        let endpoints = Endpoints::new(7890, 0, 0, 9090).unwrap();
        let options = RunOptions {
            allow_lan: true,
            dns_servers_raw: "1.1.1.1, 8.8.8.8".into(),
            ..RunOptions::new()
        };
        let started = h
            .runtime
            .start(EngineKind::Xray, &node(), endpoints, options)
            .unwrap();

        // Both are baked into the rendered config, so the settings page can
        // only learn them here: the raw text stays as the user wrote it, not
        // as the parsed resolver list.
        assert!(started.allow_lan);
        assert_eq!(started.dns_servers, "1.1.1.1, 8.8.8.8");
        let status = h.runtime.status();
        assert!(status.allow_lan);
        assert_eq!(status.dns_servers, "1.1.1.1, 8.8.8.8");

        std::fs::remove_dir_all(&h.dir).ok();
    }

    #[test]
    fn a_stopped_kernel_reports_no_lan_and_no_dns() {
        let h = harness(true, false);
        let endpoints = Endpoints::new(7890, 0, 0, 9090).unwrap();
        let options = RunOptions {
            allow_lan: true,
            dns_servers_raw: "1.1.1.1".into(),
            ..RunOptions::new()
        };
        h.runtime
            .start(EngineKind::Xray, &node(), endpoints, options)
            .unwrap();

        h.runtime.stop().unwrap();
        // Nothing runs, so nothing listens on 0.0.0.0 or uses those resolvers:
        // a leftover value would make the UI claim an unapplied change is live.
        let status = h.runtime.status();
        assert!(!status.running);
        assert!(!status.allow_lan);
        assert_eq!(status.dns_servers, "");

        std::fs::remove_dir_all(&h.dir).ok();
    }

    #[test]
    fn status_reports_the_mux_the_kernel_was_started_with() {
        let h = harness(true, false);
        let endpoints = Endpoints::new(7890, 0, 0, 9090).unwrap();
        let options = RunOptions {
            mux_enabled: true,
            mux_concurrency: 16,
            ..RunOptions::new()
        };
        let started = h
            .runtime
            .start(EngineKind::Xray, &node(), endpoints, options)
            .unwrap();

        // Multiplexing is baked into the rendered config as well, so the
        // settings page can tell an applied change from a saved one only if
        // the runtime remembers what it started with.
        assert!(started.mux_enabled);
        assert_eq!(started.mux_concurrency, 16);
        let status = h.runtime.status();
        assert!(status.mux_enabled);
        assert_eq!(status.mux_concurrency, 16);

        std::fs::remove_dir_all(&h.dir).ok();
    }

    #[test]
    fn a_stopped_kernel_reports_no_mux() {
        let h = harness(true, false);
        let endpoints = Endpoints::new(7890, 0, 0, 9090).unwrap();
        let options = RunOptions {
            mux_enabled: true,
            mux_concurrency: 32,
            ..RunOptions::new()
        };
        h.runtime
            .start(EngineKind::Xray, &node(), endpoints, options)
            .unwrap();

        h.runtime.stop().unwrap();
        // Nothing runs, so no link is being multiplexed and no stream cap is
        // in force: both fall back to the default rather than the last used
        // value, which would read as a live setting the kernel never got.
        let status = h.runtime.status();
        assert!(!status.running);
        assert!(!status.mux_enabled);
        assert_eq!(status.mux_concurrency, 8);
        assert!(!EngineStatus::default().mux_enabled);
        assert_eq!(EngineStatus::default().mux_concurrency, 8);

        std::fs::remove_dir_all(&h.dir).ok();
    }

    #[test]
    fn status_reports_the_fragment_the_kernel_was_started_with() {
        let h = harness(true, false);
        let endpoints = Endpoints::new(7890, 0, 0, 9090).unwrap();
        let options = RunOptions {
            fragment_enabled: true,
            fragment_length: "120-240".into(),
            fragment_interval: "15-30".into(),
            ..RunOptions::new()
        };
        let started = h
            .runtime
            .start(EngineKind::Xray, &node(), endpoints, options)
            .unwrap();

        // The fragment text is baked into the rendered config, so only the
        // runtime's own copy can tell the page what the kernel actually got.
        assert!(started.fragment_enabled);
        assert_eq!(started.fragment_length, "120-240");
        assert_eq!(started.fragment_interval, "15-30");
        let status = h.runtime.status();
        assert!(status.fragment_enabled);
        assert_eq!(status.fragment_length, "120-240");
        assert_eq!(status.fragment_interval, "15-30");

        std::fs::remove_dir_all(&h.dir).ok();
    }

    #[test]
    fn status_reports_the_probe_address_the_kernel_was_started_with() {
        let h = harness(true, false);
        let endpoints = Endpoints::new(7890, 0, 0, 9090).unwrap();
        let options = RunOptions {
            probe_url: "http://probe.example.com/generate_204".into(),
            ..RunOptions::new()
        };
        let started = h
            .runtime
            .start(EngineKind::SingBox, &node(), endpoints, options)
            .unwrap();

        // sing-box writes this into every `urltest` group, so the settings page
        // needs the runtime's copy to tell a saved change from an applied one.
        assert_eq!(started.test_url, "http://probe.example.com/generate_204");
        assert_eq!(
            h.runtime.status().test_url,
            "http://probe.example.com/generate_204"
        );

        std::fs::remove_dir_all(&h.dir).ok();
    }

    #[test]
    fn a_stopped_kernel_reports_no_fragment() {
        let h = harness(true, false);
        let endpoints = Endpoints::new(7890, 0, 0, 9090).unwrap();
        let options = RunOptions {
            fragment_enabled: true,
            fragment_length: "80-160".into(),
            fragment_interval: "5-10".into(),
            ..RunOptions::new()
        };
        h.runtime
            .start(EngineKind::Xray, &node(), endpoints, options)
            .unwrap();

        h.runtime.stop().unwrap();
        // Nothing runs, so nothing is being fragmented: every field falls back
        // to the default rather than the last used value, which the UI would
        // otherwise read as a live setting the kernel never received.
        let status = h.runtime.status();
        assert!(!status.running);
        assert!(!status.fragment_enabled);
        assert_eq!(status.fragment_length, "100-200");
        assert_eq!(status.fragment_interval, "10-20");
        let defaults = EngineStatus::default();
        assert!(!defaults.fragment_enabled);
        assert_eq!(defaults.fragment_length, "100-200");
        assert_eq!(defaults.fragment_interval, "10-20");

        std::fs::remove_dir_all(&h.dir).ok();
    }

    #[test]
    fn status_reports_the_node_the_kernel_was_started_with() {
        let h = harness(true, false);
        let endpoints = Endpoints::new(7890, 0, 0, 9090).unwrap();
        let started = h
            .runtime
            .start(EngineKind::Xray, &node(), endpoints, RunOptions::new())
            .unwrap();

        // Both the start result and the polled status name the node id, so the
        // UI can ask "is this the node the kernel runs?" without comparing
        // names, which two nodes may share.
        assert_eq!(started.node_id.as_deref(), Some("n1"));
        assert_eq!(h.runtime.status().node_id.as_deref(), Some("n1"));

        std::fs::remove_dir_all(&h.dir).ok();
    }

    #[test]
    fn a_stopped_kernel_reports_no_node_id() {
        let h = harness(true, false);
        let endpoints = Endpoints::new(7890, 0, 0, 9090).unwrap();
        h.runtime
            .start(EngineKind::Xray, &node(), endpoints, RunOptions::new())
            .unwrap();

        h.runtime.stop().unwrap();
        // Nothing runs, so nothing is running a node: a leftover id would let
        // the UI believe a deleted node still had a kernel on it.
        let status = h.runtime.status();
        assert!(!status.running);
        assert_eq!(status.node_id, None);
        assert_eq!(EngineStatus::default().node_id, None);

        std::fs::remove_dir_all(&h.dir).ok();
    }

    #[test]
    fn a_kernel_that_exits_on_its_own_is_reported_as_stopped() {
        let h = harness(true, false);
        let endpoints = Endpoints::new(7890, 0, 0, 9090).unwrap();
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
    fn stop_on_purpose_is_never_an_unexpected_exit() {
        let h = harness(true, false);
        let endpoints = Endpoints::new(7890, 0, 0, 9090).unwrap();
        h.runtime
            .start(EngineKind::Xray, &node(), endpoints, RunOptions::new())
            .unwrap();

        h.runtime.stop().unwrap();
        // The kill marks the fake handle as exited, so this status sees the
        // same thing it would see on a real machine: a process that is gone.
        assert!(!h.runtime.status().running);
        assert_eq!(h.runtime.take_unexpected_exit(), None);

        std::fs::remove_dir_all(&h.dir).ok();
    }

    #[test]
    fn a_kernel_that_dies_on_its_own_is_announced_exactly_once() {
        let h = harness(true, false);
        let endpoints = Endpoints::new(7890, 0, 0, 9090).unwrap();
        h.runtime
            .start(EngineKind::Xray, &node(), endpoints, RunOptions::new())
            .unwrap();

        h.crash(7);
        assert_eq!(h.runtime.status().last_exit_code, Some(7));
        // First read hands the code over, second read has nothing left: the
        // watcher and the UI can both poll, only one gets the news.
        assert_eq!(h.runtime.take_unexpected_exit(), Some(7));
        assert_eq!(h.runtime.take_unexpected_exit(), None);

        std::fs::remove_dir_all(&h.dir).ok();
    }

    #[test]
    fn spawn_failure_surfaces_as_an_engine_error() {
        let h = harness(true, true);
        let endpoints = Endpoints::new(7890, 0, 0, 9090).unwrap();
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
        let endpoints = Endpoints::new(7890, 0, 0, 9090).unwrap();
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
