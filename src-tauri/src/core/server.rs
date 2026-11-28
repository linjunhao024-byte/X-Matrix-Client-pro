//! The loopback listener: accept, one thread per connection, hand off to
//! [`crate::core::endpoints::dispatch`], write the response back.
//!
//! Deliberately small. The peer is the local UI (or `curl` in an acceptance
//! run), so there is no keep-alive, no TLS, no worker pool and no framing beyond
//! `Content-Length`. The listener binds `127.0.0.1` only: a core that is
//! reachable from the network would need authentication the token file cannot
//! provide, so it is not reachable.
//!
//! Lifecycle: bind → publish the endpoint file (`tmp` + rename) → serve until
//! `POST /shutdown` → wait briefly for in-flight responses → remove the
//! endpoint file → put the proxy back → stop the kernel. The order matters:
//! the file disappears before the token becomes useless, so a UI never reads a
//! token for a process that is already leaving.

use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::core::{CoreContext, endpoints, http};
use crate::error::{AppError, AppResult};

/// How long a leaving core waits for responses already in flight.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(3);

/// Read timeout for one connection: a client that connects and never sends must
/// not pin a thread for the life of the process.
const READ_TIMEOUT: Duration = Duration::from_secs(10);

/// What the UI reads to find and authenticate this core.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoreEndpoint {
    pub port: u16,
    pub token: String,
    pub pid: u32,
}

/// Bind, publish, serve, clean up. Returns when the core is asked to stop.
pub fn serve(context: &Arc<CoreContext>, port: u16, endpoint_path: &Path) -> AppResult<()> {
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|error| {
        AppError::Io(format!("无法监听 127.0.0.1:{port}：{error}"))
    })?;
    listener.set_nonblocking(true)?;
    let port = listener
        .local_addr()
        .map_err(|error| AppError::Io(format!("读取监听地址失败：{error}")))?
        .port();
    let token = new_token();
    let endpoint = CoreEndpoint {
        port,
        token: token.clone(),
        pid: std::process::id(),
    };
    write_endpoint(endpoint_path, &endpoint)?;

    let result = accept_loop(context, &listener, &token);

    // Cleanup runs on both the normal and the error path: an endpoint file for a
    // dead core is worse than no file, because the UI would keep retrying it.
    if let Err(error) = std::fs::remove_file(endpoint_path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            eprintln!(
                "x-matrix core: 无法删除 {}：{error}",
                endpoint_path.display()
            );
        }
    }
    match context.proxy.release_if_active() {
        Ok(true) => eprintln!("x-matrix core: 退出前已恢复系统代理设置"),
        Ok(false) => {}
        Err(error) => eprintln!("x-matrix core: 退出前恢复系统代理失败：{error}"),
    }
    if let Err(error) = context.runtime.stop() {
        eprintln!("x-matrix core: 退出前停止内核失败：{error}");
    }
    result
}

fn accept_loop(
    context: &Arc<CoreContext>,
    listener: &TcpListener,
    token: &str,
) -> AppResult<()> {
    let active = Arc::new(AtomicUsize::new(0));
    let shutting_down = Arc::new(AtomicBool::new(false));

    loop {
        if context.is_shutting_down() {
            shutting_down.store(true, Ordering::SeqCst);
            break;
        }
        match listener.accept() {
            Ok((stream, _)) => {
                active.fetch_add(1, Ordering::SeqCst);
                let context = Arc::clone(context);
                let token = token.to_string();
                let active = Arc::clone(&active);
                std::thread::spawn(move || {
                    handle_connection(&context, &token, stream);
                    active.fetch_sub(1, Ordering::SeqCst);
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) => {
                return Err(AppError::Io(format!("接受连接失败：{error}")));
            }
        }
    }

    // Let responses already being written finish, so a client that called
    // `/shutdown` receives its `{"ok":true}` instead of a reset connection.
    let deadline = Instant::now() + DRAIN_TIMEOUT;
    while active.load(Ordering::SeqCst) > 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = shutting_down;
    Ok(())
}

fn handle_connection(context: &CoreContext, token: &str, mut stream: TcpStream) {
    // Windows hands the accepted socket the listener's non-blocking flag, so a
    // connection coming off the non-blocking accept loop starts non-blocking
    // too. Left that way, a request whose bytes have not arrived yet fails with
    // WSAEWOULDBLOCK (10035) and the UI reads a 500 instead of its answer — the
    // failure depended on packet timing, which is why curl usually "worked".
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
    let response = match http::read_request(&mut stream) {
        Ok(request) => endpoints::dispatch(context, token, &request),
        // A request we cannot parse is still a request: answer with the shared
        // envelope instead of closing silently, so the UI can show why.
        Err(error) => endpoints::from_error(&error),
    };
    if let Err(error) = response.write_to(&mut stream) {
        eprintln!("x-matrix core: 写入响应失败：{error}");
    }
    let _ = stream.flush();
    // A plain drop can reset the connection while the peer is still reading
    // the last bytes; shutting the write side down first turns it into the
    // orderly end the client expects (measured as "你的主机中的软件中止了一个
    // 已建立的连接" in the shell before this line existed).
    let _ = stream.shutdown(std::net::Shutdown::Write);
}

/// Publish the endpoint atomically: write a sibling `*.tmp`, then rename over
/// the final name. A reader either sees the previous complete file or the new
/// complete file, never a half-written JSON document.
fn write_endpoint(path: &Path, endpoint: &CoreEndpoint) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("tmp");
    let body = serde_json::to_string(endpoint)?;
    std::fs::write(&temporary, body)
        .map_err(|error| AppError::Io(format!("无法写入 {}：{error}", temporary.display())))?;
    if let Err(error) = std::fs::rename(&temporary, path) {
        // Windows can refuse a replacing rename while another process has the
        // destination open. Removing first loses atomicity for that one run,
        // but leaving the UI a stale endpoint would be the worse failure.
        let _ = std::fs::remove_file(&temporary);
        return Err(AppError::Io(format!(
            "无法把 {} 改名到 {}：{error}",
            temporary.display(),
            path.display()
        )));
    }
    Ok(())
}

/// 32 lowercase hex characters. Seed material is the OS hash randomisation
/// (`RandomState`) plus the clock and pid; the token only has to be
/// unguessable by another local user, and the channel never leaves loopback.
pub fn new_token() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let mut token = String::with_capacity(32);
    for round in 0u64..2 {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u64(round);
        hasher.write_u128(nanos);
        hasher.write_u32(std::process::id());
        token.push_str(&format!("{:016x}", hasher.finish()));
    }
    token
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::CoreContext;
    use crate::engine::EngineRuntime;
    use crate::error::AppResult;
    use crate::state::AppState;
    use crate::store::Store;
    use crate::sysproxy::{ProxyGuard, ProxyState, SystemProxy};
    use std::io::Read;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    struct OffProxy;

    impl SystemProxy for OffProxy {
        fn read(&self) -> AppResult<ProxyState> {
            Ok(ProxyState::off())
        }

        fn write(&self, _state: &ProxyState) -> AppResult<()> {
            Err(AppError::Internal("测试不得写系统代理注册表".to_string()))
        }
    }

    fn temp_dir(tag: &str) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("xmatrix-core-tests");
        std::fs::create_dir_all(&root).unwrap();
        let dir = root.join(format!(
            "server-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn test_context(dir: &Path) -> Arc<CoreContext> {
        let state = AppState::load(Store::new(dir.join("store.json")));
        let runtime = Arc::new(EngineRuntime::new(
            dir.join("engines"),
            dir.join("configs"),
            dir.join("logs"),
        ));
        let proxy = Arc::new(ProxyGuard::new(
            Box::new(OffProxy),
            dir.join("proxy-snapshot.json"),
            dir.join("x-matrix.pac"),
        ));
        let traffic = Arc::new(crate::engine::TrafficLog::new(dir.join("traffic.jsonl")));
        Arc::new(CoreContext::new(state, runtime, proxy, traffic))
    }

    fn wait_for_endpoint(path: &Path) -> CoreEndpoint {
        for _ in 0..300 {
            if let Ok(text) = std::fs::read_to_string(path) {
                if let Ok(endpoint) = serde_json::from_str::<CoreEndpoint>(&text) {
                    return endpoint;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("服务没有在 3 秒内写出 {}", path.display());
    }

    fn raw_request(port: u16, request: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    }

    fn get_with(port: u16, path: &str, token: Option<&str>) -> String {
        let header = token
            .map(|token| format!("X-Core-Token: {token}\r\n"))
            .unwrap_or_default();
        raw_request(
            port,
            &format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{header}Connection: close\r\n\r\n"),
        )
    }

    fn post_with(port: u16, path: &str, token: &str) -> String {
        raw_request(
            port,
            &format!(
                "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Core-Token: {token}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            ),
        )
    }

    #[test]
    fn a_token_is_32_lowercase_hex_characters() {
        let token = new_token();
        assert_eq!(token.len(), 32);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit() || c.is_ascii_lowercase()));
        assert_ne!(token, new_token());
    }

    #[test]
    fn the_listener_answers_with_the_token_and_refuses_without_it() {
        let dir = temp_dir("live");
        let context = test_context(&dir);
        let endpoint_path = dir.join("core.endpoint");
        let handle = {
            let context = Arc::clone(&context);
            let path = endpoint_path.clone();
            std::thread::spawn(move || serve(&context, 0, &path))
        };

        let endpoint = wait_for_endpoint(&endpoint_path);
        assert!(endpoint.port > 0);
        assert_eq!(endpoint.pid, std::process::id());

        let healthy = get_with(endpoint.port, "/health", Some(&endpoint.token));
        assert!(healthy.starts_with("HTTP/1.1 200 OK"), "{healthy}");
        assert!(healthy.contains("\"ok\":true"), "{healthy}");

        let refused = get_with(endpoint.port, "/health", None);
        assert!(refused.starts_with("HTTP/1.1 401"), "{refused}");
        assert!(refused.contains("\"code\":\"unauthorized\""), "{refused}");

        let shutdown = post_with(endpoint.port, "/shutdown", &endpoint.token);
        assert!(shutdown.starts_with("HTTP/1.1 200 OK"), "{shutdown}");

        let result = handle.join().unwrap();
        assert!(result.is_ok(), "{result:?}");
        assert!(
            !endpoint_path.exists(),
            "退出后必须删掉 {}",
            endpoint_path.display()
        );
    }

    #[test]
    fn a_malformed_request_still_gets_the_error_envelope() {
        let dir = temp_dir("malformed");
        let context = test_context(&dir);
        let endpoint_path = dir.join("core.endpoint");
        let handle = {
            let context = Arc::clone(&context);
            let path = endpoint_path.clone();
            std::thread::spawn(move || serve(&context, 0, &path))
        };

        let endpoint = wait_for_endpoint(&endpoint_path);
        let response = raw_request(endpoint.port, "NOT-HTTP\r\n\r\n");
        assert!(response.starts_with("HTTP/1.1 400 Bad Request"), "{response}");
        assert!(response.contains("\"error\":{"), "{response}");

        let _ = post_with(endpoint.port, "/shutdown", &endpoint.token);
        assert!(handle.join().unwrap().is_ok());
    }

    #[test]
    fn a_busy_port_is_a_clean_error_not_a_panic() {
        let first = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = first.local_addr().unwrap().port();
        let dir = temp_dir("busy");
        let context = test_context(&dir);
        let error = serve(&context, port, &dir.join("core.endpoint")).unwrap_err();
        assert!(matches!(error, AppError::Io(_)), "{error:?}");
        drop(first);
    }
}