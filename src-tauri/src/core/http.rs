//! The server side of the tiny HTTP/1.1 channel the UI talks to.
//!
//! Counterpart of [`crate::net::http`], which is the *client* used against a
//! kernel. The rules are deliberately the same narrow set that client speaks:
//! one request line, headers, `Content-Length`, `Connection: close`, read to
//! EOF. No chunked encoding, no keep-alive, no HTTP/2 — the peer is a local
//! Avalonia window on loopback, so anything more would be unused code that can
//! only drift.
//!
//! Every response carries `Content-Type: application/json; charset=utf-8` and
//! an exact `Content-Length`: the client reads the body by byte count instead
//! of guessing where it starts or ends.

use std::io::{Read, Write};
use std::net::TcpStream;

use crate::error::{AppError, AppResult};

/// A request head longer than this is not a request we are willing to parse.
/// Loopback peers only send a handful of lines; anything bigger is a runaway
/// client or something that is not HTTP at all.
const MAX_HEAD_BYTES: usize = 16 * 1024;

/// Bodies are JSON commands (`{"kind":"xray","nodeId":"…"}`). A megabyte is
/// already generous for that, and caps what one connection can make us buffer.
const MAX_BODY_BYTES: usize = 1024 * 1024;

/// One parsed request. The path is stored without its query string; the core
/// routes are all plain paths today, and dropping the query here keeps the
/// router from having to repeat that.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    pub path: String,
    /// Header names lowercased once, because HTTP names are case-insensitive
    /// and every later lookup would otherwise need its own fold.
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Request {
    /// A request assembled in code (tests, and the client-side helpers).
    pub fn new(method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> Self {
        Self {
            method: method.to_string(),
            path: path.to_string(),
            headers: headers
                .iter()
                .map(|(name, value)| (name.to_ascii_lowercase(), value.to_string()))
                .collect(),
            body: body.to_string(),
        }
    }

    /// Case-insensitive header lookup. `None` is "the header is absent", which
    /// the router reports as a 401 rather than treating as an empty token.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(existing, _)| existing.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// Read exactly one request from `stream`.
///
/// The read loop stops when the head terminator is seen, then keeps reading
/// until the body promised by `Content-Length` has arrived: a client is allowed
/// to send the two in separate packets.
pub fn read_request(stream: &mut TcpStream) -> AppResult<Request> {
    let mut data = Vec::new();
    let mut chunk = [0u8; 4096];

    let head_end = loop {
        if let Some(index) = find_subslice(&data, b"\r\n\r\n") {
            break index;
        }
        if data.len() > MAX_HEAD_BYTES {
            return Err(AppError::InvalidInput("请求头过大".to_string()));
        }
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Err(AppError::InvalidInput(
                "连接在请求头结束前关闭".to_string(),
            ));
        }
        data.extend_from_slice(&chunk[..read]);
    };

    let head = std::str::from_utf8(&data[..head_end])
        .map_err(|_| AppError::InvalidInput("请求头不是合法的 UTF-8".to_string()))?;

    let mut lines = head.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| AppError::InvalidInput("缺少请求行".to_string()))?;
    let mut parts = request_line.split_whitespace();
    let (Some(method), Some(target), Some(version)) = (parts.next(), parts.next(), parts.next())
    else {
        return Err(AppError::InvalidInput(format!(
            "请求行不是「方法 路径 HTTP/x.y」：{request_line:?}"
        )));
    };
    if !version.starts_with("HTTP/1.") {
        return Err(AppError::InvalidInput(format!(
            "不支持的 HTTP 版本：{version}"
        )));
    }
    if !target.starts_with('/') {
        return Err(AppError::InvalidInput(format!(
            "请求路径必须以 / 开头：{target:?}"
        )));
    }
    // Own the method and path before reading the body: the body loop grows
    // `data`, and a slice borrowed from `head` would keep the whole buffer
    // immutably borrowed for the rest of the function.
    let method = method.to_ascii_uppercase();
    let path = target.split('?').next().unwrap_or(target).to_string();

    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(AppError::InvalidInput(format!(
                "请求头不是「名字: 值」：{line:?}"
            )));
        };
        let name = name.trim();
        if name.is_empty() {
            return Err(AppError::InvalidInput("请求头名字为空".to_string()));
        }
        headers.push((name.to_ascii_lowercase(), value.trim().to_string()));
    }

    let content_length = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .map(|(_, value)| {
            value.parse::<usize>().map_err(|_| {
                AppError::InvalidInput(format!("Content-Length 不是整数：{value:?}"))
            })
        })
        .transpose()?
        .unwrap_or(0);
    if content_length > MAX_BODY_BYTES {
        return Err(AppError::InvalidInput(format!(
            "请求体过大：{content_length} 字节（上限 {MAX_BODY_BYTES}）"
        )));
    }

    let body_start = head_end + 4;
    let body_end = body_start + content_length;
    while data.len() < body_end {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Err(AppError::InvalidInput(format!(
                "请求体不完整：声明 {content_length} 字节，实到 {} 字节",
                data.len().saturating_sub(body_start)
            )));
        }
        data.extend_from_slice(&chunk[..read]);
    }

    let body = String::from_utf8(data[body_start..body_end].to_vec())
        .map_err(|_| AppError::InvalidInput("请求体不是合法的 UTF-8".to_string()))?;

    Ok(Request {
        method,
        path,
        headers,
        body,
    })
}

/// A JSON response. The body is stored as text because every route writes JSON,
/// and serialising twice (once here, once into bytes) would only add a step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub body: String,
}

impl Response {
    pub fn json(status: u16, value: serde_json::Value) -> Self {
        Self {
            status,
            body: value.to_string(),
        }
    }

    pub fn write_to(&self, stream: &mut TcpStream) -> std::io::Result<()> {
        let body = self.body.as_bytes();
        let head = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            self.status,
            reason(self.status),
            body.len()
        );
        stream.write_all(head.as_bytes())?;
        stream.write_all(body)?;
        stream.flush()
    }
}

/// The only status lines this channel ever emits.
pub fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        _ => "Unknown",
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, TcpStream};
    use std::thread;

    /// One request written by a real socket, read by [`read_request`].
    fn round_trip(raw: &'static [u8]) -> AppResult<Request> {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let writer = thread::spawn(move || {
            let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
            client.write_all(raw).unwrap();
        });
        let (mut server, _) = listener.accept().unwrap();
        let request = read_request(&mut server);
        writer.join().unwrap();
        request
    }

    #[test]
    fn reads_a_get_with_no_body() {
        let request =
            round_trip(b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Core-Token: abc\r\n\r\n")
                .unwrap();
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/health");
        assert_eq!(request.header("x-core-token"), Some("abc"));
        assert_eq!(request.body, "");
    }

    #[test]
    fn reads_the_body_promised_by_content_length() {
        // Sent in two writes so the reader cannot assume one packet per request.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let writer = thread::spawn(move || {
            let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
            client
                .write_all(
                    b"POST /engine/start HTTP/1.1\r\nContent-Length: 29\r\nContent-Type: application/json\r\n",
                )
                .unwrap();
            client.flush().unwrap();
            thread::sleep(std::time::Duration::from_millis(30));
            client
                .write_all(b"\r\n{\"kind\":\"xray\",\"nodeId\":\"n1\"}")
                .unwrap();
        });
        let (mut server, _) = listener.accept().unwrap();
        let request = read_request(&mut server).unwrap();
        writer.join().unwrap();
        assert_eq!(request.method, "POST");
        assert_eq!(request.body, r#"{"kind":"xray","nodeId":"n1"}"#);
    }

    #[test]
    fn header_names_are_case_insensitive() {
        let request = round_trip(
            b"GET /nodes HTTP/1.1\r\nx-CORE-token: AbC\r\nContent-Length: 0\r\n\r\n",
        )
        .unwrap();
        assert_eq!(request.header("X-Core-Token"), Some("AbC"));
        assert_eq!(request.header("x-core-token"), Some("AbC"));
    }

    #[test]
    fn the_query_string_is_not_part_of_the_path() {
        let request = round_trip(b"GET /nodes?limit=2 HTTP/1.1\r\n\r\n").unwrap();
        assert_eq!(request.path, "/nodes");
    }

    #[test]
    fn a_truncated_body_is_refused() {
        // Declares 10 bytes but closes after 4: read_to_end-style tolerance is
        // exactly what a command channel must not do.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let writer = thread::spawn(move || {
            let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
            client
                .write_all(b"POST /engine/start HTTP/1.1\r\nContent-Length: 10\r\n\r\nabcd")
                .unwrap();
        });
        let (mut server, _) = listener.accept().unwrap();
        let error = read_request(&mut server).unwrap_err();
        writer.join().unwrap();
        assert!(matches!(error, AppError::InvalidInput(ref text) if text.contains("请求体不完整")));
    }

    #[test]
    fn a_malformed_request_line_is_rejected() {
        let error = round_trip(b"NONSENSE\r\n\r\n").unwrap_err();
        assert!(matches!(error, AppError::InvalidInput(_)));
    }

    #[test]
    fn a_response_carries_its_own_length_and_closes() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            Response::json(200, serde_json::json!({"ok": true}))
                .write_to(&mut socket)
                .unwrap();
        });
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let mut raw = String::new();
        client.read_to_string(&mut raw).unwrap();
        server.join().unwrap();
        assert!(raw.starts_with("HTTP/1.1 200 OK\r\n"), "unexpected: {raw:?}");
        assert!(raw.contains("Content-Length: 11\r\n"), "unexpected: {raw:?}");
        assert!(raw.contains("Connection: close\r\n"), "unexpected: {raw:?}");
        assert!(raw.ends_with(r#"{"ok":true}"#), "unexpected: {raw:?}");
    }
}