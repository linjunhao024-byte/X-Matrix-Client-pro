//! One-node throwaway kernel: measure a node when no kernel is running.
//!
//! [`node_delay`](crate::commands::engine::node_delay) measures through the
//! kernel the user is running. Before a kernel exists there is still a question
//! worth answering — "how fast is this node?" — and the honest way to answer it
//! is to drive a kernel. This module starts a private one on two free ports,
//! sends exactly one request through its mixed inbound and kills it again.
//!
//! Deliberately not part of [`EngineRuntime`](super::EngineRuntime): the
//! throwaway kernel has its own config, its own ports and its own stderr file.
//! A failure here therefore cannot leave the app believing a kernel is running,
//! cannot overwrite the running kernel's config, and never appears in the log
//! view as a kernel the user did not start.

use std::fs::{self, File};
use std::io::Read;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::runtime::asset_dir_for;
use super::{Endpoints, EngineKind, LogLevel, RenderInput};
use crate::domain::node::Node;
use crate::domain::settings::ProxyMode;
use crate::error::{AppError, AppResult};
use crate::net::latency;

/// How long one readiness poll waits before trying the port again.
const POLL: Duration = Duration::from_millis(50);

/// How much of the kernel's stderr the error message may quote.
const STDERR_QUOTE: usize = 200;

/// One throwaway run, split by what the caller must record afterwards.
///
/// The three cases are genuinely different facts: a measured number is worth
/// remembering, a request that failed is worth remembering as "测不到",
/// and a kernel that never came up is a refusal that must not overwrite a
/// perfectly good older measurement.
pub(crate) enum Attempt {
    Measured(u32),
    /// The throwaway kernel never got far enough to answer a request.
    NotStarted(AppError),
    /// The kernel ran, the request failed — the node itself is the problem.
    Failed(AppError),
}

/// Measure `node` through a private kernel and return the milliseconds.
///
/// `timeout` bounds both halves: how long the kernel may take to listen, and
/// how long the probe request may take. The documented entry point; the command
/// layer uses [`attempt`] because it has to tell a refusal from a failure.
#[allow(dead_code)]
pub fn measure(
    node: &Node,
    kind: EngineKind,
    engines_dir: &Path,
    config_dir: &Path,
    url: &str,
    timeout: Duration,
) -> AppResult<u32> {
    match attempt(node, kind, engines_dir, config_dir, url, timeout) {
        Attempt::Measured(ms) => Ok(ms),
        Attempt::NotStarted(error) | Attempt::Failed(error) => Err(error),
    }
}

/// The same run, classified for the probe record.
pub(crate) fn attempt(
    node: &Node,
    kind: EngineKind,
    engines_dir: &Path,
    config_dir: &Path,
    url: &str,
    timeout: Duration,
) -> Attempt {
    let endpoints = match free_endpoints() {
        Ok(endpoints) => endpoints,
        Err(error) => return Attempt::NotStarted(error),
    };
    let config = match probe_config(kind, node, endpoints, url) {
        Ok(config) => config,
        Err(error) => return Attempt::NotStarted(error),
    };

    if let Err(error) = fs::create_dir_all(config_dir) {
        return Attempt::NotStarted(AppError::Internal(format!(
            "创建临时配置目录失败：{error}"
        )));
    }
    // One stamp per run names both files, so a leftover from a killed
    // measurement can never be mistaken for this run's output.
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    let config_path = config_dir.join(format!("probe-{stamp}.json"));
    let stderr_path = config_dir.join(format!("probe-{stamp}.err"));

    let body = match serde_json::to_vec_pretty(&config) {
        Ok(body) => body,
        Err(error) => {
            return Attempt::NotStarted(AppError::Internal(format!(
                "临时配置序列化失败：{error}"
            )))
        }
    };
    if let Err(error) = fs::write(&config_path, &body) {
        let _ = fs::remove_file(&config_path);
        return Attempt::NotStarted(AppError::Internal(format!(
            "写临时配置失败：{error}"
        )));
    }

    let kernel = kind.engine();
    let binary = engines_dir.join(kernel.binary_name());
    if !binary.is_file() {
        let _ = fs::remove_file(&config_path);
        return Attempt::NotStarted(AppError::Engine(format!(
            "找不到内核可执行文件：{}",
            binary.display()
        )));
    }
    let stderr_file = match File::create(&stderr_path) {
        Ok(file) => file,
        Err(error) => {
            let _ = fs::remove_file(&config_path);
            return Attempt::NotStarted(AppError::Internal(format!(
                "创建临时错误日志失败：{error}"
            )));
        }
    };

    let mut command = Command::new(&binary);
    command.args(kernel.args(&config_path.to_string_lossy()));
    command.stdin(Stdio::null());
    command.stdout(Stdio::null());
    // The kernel's own words: a config the kernel rejects says why on stderr,
    // and that is far more useful than "it did not start".
    command.stderr(Stdio::from(stderr_file));
    // Same as the real runtime: Xray loads geoip/geosite from beside its
    // executable, and without this it dies before reading a single route.
    command.env("XRAY_LOCATION_ASSET", asset_dir_for(&binary, engines_dir));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // This runs while the user clicks a row; a console window flashing on
        // screen would be the only thing they noticed.
        command.creation_flags(0x0800_0000);
    }

    let child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = fs::remove_file(&config_path);
            let _ = fs::remove_file(&stderr_path);
            return Attempt::NotStarted(AppError::Engine(format!(
                "临时内核启动失败：{error}"
            )));
        }
    };
    let mut cleanup = Cleanup {
        child: Some(child),
        files: vec![config_path, stderr_path.clone()],
    };

    let address = SocketAddr::from(([127, 0, 0, 1], endpoints.mixed_port));
    let deadline = Instant::now() + timeout;
    let mut ready = false;
    let mut exit_status = None;
    loop {
        // Connect rather than poll the listener: the port accepting is exactly
        // what the probe request below needs, and nothing else is evidence
        // that the kernel is actually serving.
        if TcpStream::connect_timeout(&address, POLL * 4).is_ok() {
            ready = true;
            break;
        }
        if let Some(status) = cleanup.exit_code() {
            exit_status = Some(status);
            break;
        }
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(POLL);
    }
    if !ready {
        return Attempt::NotStarted(not_ready_error(
            read_stderr(&stderr_path),
            exit_status,
            endpoints.mixed_port,
        ));
    }

    match latency::http_latency_via_proxy_with_timeout(endpoints.mixed_port, url, timeout) {
        Ok(ms) => Attempt::Measured(ms),
        Err(error) => Attempt::Failed(error),
    }
}

/// The config a throwaway kernel runs: this node, one mixed inbound, and a
/// route that cannot send the probe anywhere but through that node.
///
/// `Global` mode is what makes this honest: the probe must go out over the
/// node being measured, never over `direct`.
fn probe_config(
    kind: EngineKind,
    node: &Node,
    endpoints: Endpoints,
    url: &str,
) -> AppResult<serde_json::Value> {
    let nodes = std::slice::from_ref(node);
    let mut input = RenderInput::new(node, endpoints);
    input.log_level = LogLevel::Warn;
    input.rules = &[];
    input.mode = ProxyMode::Global;
    input.allow_lan = false;
    input.nodes = nodes;
    input.groups = &[];
    input.rule_sets = &[];
    input.rule_set_dir = None;
    input.probe_url = url.to_string();
    input.dns_servers = Vec::new();
    input.mux_enabled = false;
    input.mux_concurrency = 8;
    kind.engine().render_config(&input)
}

/// Two ports nobody else is using, for one kernel's inbound and control API.
fn free_endpoints() -> AppResult<Endpoints> {
    let mixed_port = free_port()?;
    let mut api_port = free_port()?;
    // The OS may hand the same number back twice now that the first listener
    // is closed, and a kernel cannot bind both on one port.
    for _ in 0..8 {
        if api_port != mixed_port {
            return Endpoints::new(mixed_port, api_port);
        }
        api_port = free_port()?;
    }
    Err(AppError::Internal(
        "找不到两个不同的空闲端口".to_string(),
    ))
}

/// One free port: bind to port 0, read what the OS chose, release it again.
fn free_port() -> AppResult<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|error| AppError::Internal(format!("找不到空闲端口：{error}")))?;
    let port = listener
        .local_addr()
        .map_err(|error| AppError::Internal(format!("读取空闲端口失败：{error}")))?
        .port();
    drop(listener);
    Ok(port)
}

/// Not ready: quote the kernel's own words, or say what was missing.
fn not_ready_error(detail: String, exit_status: Option<String>, port: u16) -> AppError {
    let reason = if !detail.is_empty() {
        detail
    } else if let Some(status) = exit_status {
        format!("内核已退出（{status}），没有任何输出")
    } else {
        format!("127.0.0.1:{port} 在限定时间内没有监听")
    };
    AppError::Engine(format!("临时内核没能就绪：{reason}"))
}

/// The kernel's stderr, flattened to one line and cut to a quotable length.
fn read_stderr(path: &Path) -> String {
    let mut text = String::new();
    if let Ok(mut file) = File::open(path) {
        // Only the head: a kernel that fails on a config can print megabytes,
        // and the first lines are the ones that name the mistake.
        let _ = (&mut file).take(4096).read_to_string(&mut text);
    }
    let flattened = text.split_whitespace().collect::<Vec<_>>().join(" ");
    flattened.chars().take(STDERR_QUOTE).collect()
}

/// Owns the throwaway kernel and its two temp files.
///
/// `Drop` is the whole point: every early return above must both kill the
/// process and delete the files, and a leaked kernel would keep a port bound
/// and show up in the user's task manager as a proxy they never started.
struct Cleanup {
    child: Option<Child>,
    files: Vec<PathBuf>,
}

impl Cleanup {
    /// Has the kernel exited by itself? Polling this is what turns a crash into
    /// a fast, quotable error instead of a full timeout.
    fn exit_code(&mut self) -> Option<String> {
        let child = self.child.as_mut()?;
        match child.try_wait() {
            Ok(Some(status)) => Some(status.to_string()),
            _ => None,
        }
    }
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        for path in &self.files {
            let _ = fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::domain::node::{Protocol, Security, Transport};

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

    /// A directory of its own per test: tests run in parallel and a shared path
    /// lets one delete another's files.
    fn scratch(name: &str) -> PathBuf {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp")
            .join(format!("temp-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_probe_config_has_one_node_outbound_and_no_user_rules() {
        for kind in EngineKind::all() {
            let config = probe_config(
                *kind,
                &node(),
                Endpoints::new(7890, 9090).unwrap(),
                "http://example.com/probe",
            )
            .unwrap();

            // Exactly one outbound for this node. The baseline `direct`
            // (and, for Xray, `block`) are not nodes and are allowed; a group
            // or a second node would mean the probe could bypass what is being
            // measured.
            let outbounds = config["outbounds"].as_array().unwrap();
            let proxy: Vec<&serde_json::Value> = outbounds
                .iter()
                .filter(|outbound| outbound["tag"] == "proxy")
                .collect();
            assert_eq!(proxy.len(), 1, "{kind:?} must emit one proxy outbound");
            // The node's own address, wherever this kernel spells it: Xray
            // nests it under `settings`, sing-box keeps it flat.
            assert!(
                proxy[0].to_string().contains("example.com"),
                "{kind:?} must carry the node server: {}",
                proxy[0]
            );
            let baseline: BTreeSet<&str> = ["direct", "block"].into_iter().collect();
            for outbound in outbounds {
                let tag = outbound["tag"].as_str().unwrap_or_default();
                assert!(
                    tag == "proxy" || baseline.contains(tag),
                    "{kind:?} emitted an unexpected outbound {tag}"
                );
            }

            // Global mode legitimately emits one catch-all pointing at `proxy`;
            // what must not appear is a user rule (a domain, an ip, a rule set)
            // that could route the probe around the node.
            let rules = if *kind == EngineKind::Xray {
                config["routing"]["rules"].as_array().unwrap().clone()
            } else {
                config["route"]["rules"].as_array().unwrap().clone()
            };
            assert_eq!(rules.len(), 1, "{kind:?}: only the global catch-all");
            // Same rule, two spellings: Xray names the target `outboundTag`,
            // sing-box just `outbound`.
            let target = if *kind == EngineKind::Xray {
                "outboundTag"
            } else {
                "outbound"
            };
            assert_eq!(rules[0][target], "proxy");
            // Nothing to declare: no rule sets are handed in, so the config
            // must not reference one either.
            assert!(config["route"]["rule_set"].is_null());

            // The mixed inbound is on the port the probe will talk to.
            let port = if *kind == EngineKind::Xray {
                config["inbounds"][0]["port"].clone()
            } else {
                config["inbounds"][0]["listen_port"].clone()
            };
            assert_eq!(port, serde_json::json!(7890));
            assert_eq!(config["inbounds"][0]["listen"], "127.0.0.1");
        }
    }

    #[test]
    fn a_missing_kernel_is_an_error_that_leaves_no_temp_files() {
        let dir = scratch("missing-kernel");
        let engines_dir = dir.join("engines");
        let config_dir = dir.join("configs");

        for kind in EngineKind::all() {
            let error = measure(
                &node(),
                *kind,
                &engines_dir,
                &config_dir,
                "http://example.com",
                Duration::from_millis(500),
            )
            .unwrap_err();
            assert!(
                error.to_string().contains("找不到内核可执行文件"),
                "{kind:?} must report the missing binary, got {error}"
            );
        }

        // No partial config and no stderr file survive the failed run.
        let leftovers: Vec<String> = std::fs::read_dir(&config_dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(leftovers.is_empty(), "left behind: {leftovers:?}");

        std::fs::remove_dir_all(&dir).ok();
    }
}
