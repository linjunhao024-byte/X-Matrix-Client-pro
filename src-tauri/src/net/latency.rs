//! TCP handshake latency.
//!
//! What "测速" means in a proxy client: how long it takes to open a TCP
//! connection to the node. It is not a speed test and it does not touch the
//! kernel — it measures the path the user is about to depend on.
//!
//! A handshake is the honest metric here: it needs no credentials, works
//! before anything is started, and a node that cannot complete a handshake will
//! not carry traffic either.
//!
//! The second metric is the one V2RayN calls "real delay": an HTTP request sent
//! through the *running* engine's mixed port. It costs a request, but it is the
//! only number that includes the tunnel, the node's own overhead and its DNS.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use crate::error::{AppError, AppResult};

/// Anything slower than this is reported as a timeout rather than a number.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(3);

/// Milliseconds needed to open a TCP connection to `host:port`.
pub fn tcp_latency(host: &str, port: u16) -> AppResult<u32> {
    tcp_latency_with_timeout(host, port, DEFAULT_TIMEOUT)
}

pub fn tcp_latency_with_timeout(host: &str, port: u16, timeout: Duration) -> AppResult<u32> {
    if host.trim().is_empty() {
        return Err(AppError::InvalidInput("节点服务器地址为空".into()));
    }

    let mut addresses = (host, port)
        .to_socket_addrs()
        .map_err(|error| AppError::Engine(format!("无法解析 {host}：{error}")))?;
    let address = addresses
        .next()
        .ok_or_else(|| AppError::Engine(format!("{host} 没有可用的地址")))?;

    let started = Instant::now();
    std::net::TcpStream::connect_timeout(&address, timeout)
        .map_err(|error| AppError::Engine(format!("连接 {host}:{port} 失败：{error}")))?;
    Ok(started.elapsed().as_millis() as u32)
}

/// Probe URL for the real-delay measurement: it answers 204 with no body, so
/// the number is handshake + tunnel + one round trip, nothing else.
pub const PROBE_URL: &str = crate::domain::settings::DEFAULT_TEST_URL;

/// Budget for a real-delay probe. Generous on purpose: a slow node should read
/// as a big number, not as "broken".
pub const PROXY_TIMEOUT: Duration = Duration::from_secs(5);

/// Milliseconds for one HTTP request through `127.0.0.1:proxy_port`.
///
/// The request is sent in absolute-URI form, which is what an HTTP proxy
/// expects, so the kernel resolves the host and dials the node for us — that is
/// exactly the path we want to time.
pub fn http_latency_via_proxy(proxy_port: u16, url: &str) -> AppResult<u32> {
    http_latency_via_proxy_with_timeout(proxy_port, url, PROXY_TIMEOUT)
}

pub fn http_latency_via_proxy_with_timeout(
    proxy_port: u16,
    url: &str,
    timeout: Duration,
) -> AppResult<u32> {
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| AppError::InvalidInput(format!("探测地址缺少协议：{url}")))?;
    if !scheme.eq_ignore_ascii_case("http") {
        return Err(AppError::InvalidInput(
            "真实延迟只支持 http:// 探测地址：客户端里不该再带一套 TLS".into(),
        ));
    }
    let host = rest.split('/').next().unwrap_or(rest);
    if host.is_empty() {
        return Err(AppError::InvalidInput(format!("探测地址没有主机：{url}")));
    }

    let address = SocketAddr::from(([127, 0, 0, 1], proxy_port));
    let started = Instant::now();
    let mut stream = TcpStream::connect_timeout(&address, timeout).map_err(|error| {
        AppError::Engine(format!("连接本机混合端口 {proxy_port} 失败：{error}"))
    })?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;

    let request = format!(
        "GET {url} HTTP/1.1\r\nHost: {host}\r\nAccept: */*\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes())?;

    // Read only until the status line and headers are in hand: the body of a
    // 204 is empty anyway, and a node that streams forever must not hang us.
    let mut head = Vec::new();
    let mut buffer = [0u8; 512];
    loop {
        let read = stream.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        head.extend_from_slice(&buffer[..read]);
        if head.windows(4).any(|window| window == b"\r\n\r\n") || head.len() >= 8192 {
            break;
        }
    }

    let text = String::from_utf8_lossy(&head);
    let status = text
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| {
            AppError::Engine("代理没有返回可解析的 HTTP 状态行（隧道可能没通）".to_string())
        })?;
    if !(200..400).contains(&status) {
        return Err(AppError::Engine(format!(
            "经代理请求返回 HTTP {status}"
        )));
    }

    Ok(started.elapsed().as_millis() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn a_listening_port_reports_a_number() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let _ = listener.accept();
        });

        let latency = tcp_latency("127.0.0.1", port).unwrap();
        assert!(latency < 3000, "loopback handshake should be fast");
    }

    #[test]
    fn a_closed_port_is_an_error_not_a_zero() {
        // Bind then drop: the port is (almost certainly) free again.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        assert!(tcp_latency_with_timeout("127.0.0.1", port, Duration::from_millis(500)).is_err());
    }

    #[test]
    fn an_empty_host_is_rejected_before_any_dns_lookup() {
        assert!(tcp_latency("", 443).is_err());
    }

    /// A stand-in for the kernel's mixed port: accept one request, answer it.
    fn fake_proxy(status: &'static str) -> (u16, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut buffer = [0u8; 1024];
            let read = socket.read(&mut buffer).unwrap_or(0);
            let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
            let reply = format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\n\r\n");
            socket.write_all(reply.as_bytes()).ok();
            request
        });
        (port, handle)
    }

    #[test]
    fn a_request_through_the_proxy_is_timed_and_uses_absolute_uri() {
        let (port, handle) = fake_proxy("204 No Content");
        let latency =
            http_latency_via_proxy_with_timeout(port, PROBE_URL, Duration::from_secs(2)).unwrap();
        let request = handle.join().unwrap();

        // Absolute-URI form is what makes the kernel dial on our behalf.
        assert!(request.starts_with("GET http://www.gstatic.com/generate_204 HTTP/1.1"));
        assert!(request.contains("Host: www.gstatic.com"));
        assert!(latency < 2000);
    }

    #[test]
    fn a_non_2xx_from_the_proxy_is_an_error() {
        let (port, handle) = fake_proxy("502 Bad Gateway");
        let result = http_latency_via_proxy_with_timeout(port, PROBE_URL, Duration::from_secs(2));
        handle.join().ok();
        assert!(result.is_err(), "502 must not be reported as a latency");
    }

    #[test]
    fn https_probe_urls_are_refused_before_connecting() {
        let error = http_latency_via_proxy(1, "https://example.com/").unwrap_err();
        assert!(matches!(error, crate::error::AppError::InvalidInput(_)));
    }
}
