//! A minimal blocking HTTP/1.1 client.
//!
//! The app only ever talks to a kernel on `127.0.0.1`, so a real HTTP stack
//! (TLS, redirects, connection pooling, HTTP/2) would be dead weight. This
//! covers exactly what Xray's expvar endpoint and the Clash API need:
//! `GET <path>`, `Connection: close`, read to EOF.
//!
//! Everything is blocking and short-lived — it runs on the Tauri command
//! thread or a small polling task, never on the UI thread.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use crate::error::{AppError, AppResult};

/// Timeout used for kernel endpoints: they are local, so anything slower than
/// this means the kernel is wedged rather than merely busy.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(1500);

/// `GET http://127.0.0.1:{port}{path}` and parse the body as JSON.
pub fn get_json(port: u16, path: &str) -> AppResult<serde_json::Value> {
    get_json_with_timeout(port, path, DEFAULT_TIMEOUT)
}

pub fn get_json_with_timeout(
    port: u16,
    path: &str,
    timeout: Duration,
) -> AppResult<serde_json::Value> {
    let body = get_with_timeout(port, path, timeout)?;
    serde_json::from_str(&body)
        .map_err(|e| AppError::Engine(format!("内核 {path} 返回的不是合法 JSON：{e}")))
}

/// `GET` returning the decoded body. Handles `Transfer-Encoding: chunked`
/// because Go's `net/http` (which both kernels use) switches to chunked
/// encoding for streamed responses.
pub fn get_with_timeout(port: u16, path: &str, timeout: Duration) -> AppResult<String> {
    request_with_timeout("GET", port, path, timeout)
}

/// `DELETE`, used by the Clash API to drop one connection.
pub fn delete_with_timeout(port: u16, path: &str, timeout: Duration) -> AppResult<String> {
    request_with_timeout("DELETE", port, path, timeout)
}

/// One request/response round trip against a kernel on loopback.
pub fn request_with_timeout(
    method: &str,
    port: u16,
    path: &str,
    timeout: Duration,
) -> AppResult<String> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&addr, timeout).map_err(|e| {
        AppError::Engine(format!("连接内核 127.0.0.1:{port} 失败：{e}"))
    })?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;

    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes())?;

    let mut raw = Vec::new();
    stream.read_to_end(&mut raw)?;
    parse_response(&raw)
}

fn parse_response(raw: &[u8]) -> AppResult<String> {
    let text = String::from_utf8_lossy(raw);
    let (head, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| AppError::Engine("内核响应缺少头部结束标记".to_string()))?;

    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| AppError::Engine("无法解析 HTTP 状态行".to_string()))?;

    if !(200..300).contains(&status) {
        return Err(AppError::Engine(format!(
            "内核返回 HTTP {status}：{}",
            body.chars().take(200).collect::<String>()
        )));
    }

    let chunked = head.lines().any(|line| {
        let lower = line.to_ascii_lowercase();
        lower.starts_with("transfer-encoding:") && lower.contains("chunked")
    });

    if chunked {
        decode_chunked(body)
    } else {
        Ok(body.to_string())
    }
}

/// Decode a chunked body. Tolerates a truncated tail: a half-read stream is
/// still more useful than an error when the kernel is shutting down.
fn decode_chunked(body: &str) -> AppResult<String> {
    let mut out = String::new();
    let mut rest = body;

    loop {
        let Some((size_line, after)) = rest.split_once("\r\n") else {
            break;
        };
        let size_field = size_line.split(';').next().unwrap_or("").trim();
        let Ok(size) = usize::from_str_radix(size_field, 16) else {
            return Err(AppError::Engine(format!(
                "分块长度无法解析：{size_field:?}"
            )));
        };
        if size == 0 {
            break;
        }
        if after.len() < size {
            out.push_str(after);
            break;
        }
        out.push_str(&after[..size]);
        rest = after[size..].strip_prefix("\r\n").unwrap_or(&after[size..]);
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    /// Serve exactly one request with a canned response, then close.
    fn serve_once(response: &'static str) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        thread::spawn(move || {
            if let Ok((mut socket, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = socket.read(&mut buf);
                let _ = socket.write_all(response.as_bytes());
            }
        });
        port
    }

    #[test]
    fn reads_a_plain_json_body() {
        let port = serve_once(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\n\r\n{\"ok\":true}",
        );
        let value = get_json(port, "/debug/vars").unwrap();
        assert_eq!(value["ok"], serde_json::json!(true));
    }

    #[test]
    fn decodes_a_chunked_body() {
        // Two chunks spelling `{"a":1,"b":2}`: `{"a":1,` then `"b":2}`.
        let port = serve_once(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n7\r\n{\"a\":1,\r\n6\r\n\"b\":2}\r\n0\r\n\r\n",
        );
        let value = get_json(port, "/connections").unwrap();
        assert_eq!(value["a"], serde_json::json!(1));
        assert_eq!(value["b"], serde_json::json!(2));
    }

    #[test]
    fn non_2xx_becomes_an_engine_error() {
        let port = serve_once("HTTP/1.1 503 Service Unavailable\r\n\r\nnope");
        let err = get_json(port, "/connections").unwrap_err();
        assert!(matches!(err, AppError::Engine(_)));
    }

    #[test]
    fn invalid_json_is_reported_as_such() {
        let port = serve_once("HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello");
        let err = get_json(port, "/debug/vars").unwrap_err();
        let AppError::Engine(msg) = err else {
            panic!("expected an engine error");
        };
        assert!(msg.contains("不是合法 JSON"));
    }

    #[test]
    fn refused_connection_is_an_engine_error() {
        // Bind then drop, so the port is almost certainly closed.
        let port = {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        let err = get_with_timeout(port, "/x", Duration::from_millis(200)).unwrap_err();
        assert!(matches!(err, AppError::Engine(_)));
    }
}
