//! WebDAV transport for the configuration backup.
//!
//! This module moves bytes and nothing else: it does not decide what a status
//! code means, because "404" reads differently on upload ("你在地址上写错了")
//! and on restore ("云端还没有备份"). The command layer owns those sentences;
//! here a non-2xx is just a number the caller has to deal with.
//!
//! Same egress rules as [`crate::net::remote`]: `http(s)` only, our own UA, an
//! optional loopback proxy — never the Clash API client in `http.rs`, which is
//! only allowed to talk to 127.0.0.1.

use std::time::Duration;

use base64::Engine as _;

use crate::error::{AppError, AppResult};
use crate::net::remote;

/// The one file a WebDAV directory is asked to hold. The name is part of the
/// feature, not a suggestion: a folder address gets this appended, and a user
/// who points at a file keeps their own name.
pub const BACKUP_FILE_NAME: &str = "x-matrix-backup.json";

/// Uploads are a few hundred KB of JSON at most; a minute is generous for a
/// slow disk-backed server and short enough that a stalled connection still
/// ends with a sentence instead of a spinner.
pub const TIMEOUT: Duration = Duration::from_secs(30);

/// Where a PUT or GET should actually go.
///
/// A trailing `/` means "this is the folder", so the backup file name is
/// appended. A URL that already names a file is left alone — including
/// `https://host` with no path at all: whether that host accepts a PUT at its
/// root is the server's business, and second-guessing it here would hide the
/// 404/409 that explains the problem.
pub fn resolve_target(url: &str) -> AppResult<String> {
    let url = url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(AppError::InvalidInput(format!(
            "WebDAV 地址必须是 http(s)：{url}"
        )));
    }
    if url.ends_with('/') {
        return Ok(format!("{url}{BACKUP_FILE_NAME}"));
    }
    Ok(url.to_string())
}

/// A Basic authorization header, or `None` when there is no identity to send.
///
/// Base64 is the standard table (`dTpw` for `u:p`), never URL-safe. An empty
/// username with a non-empty password still sends: some servers keep the
/// account in the password field, and silently dropping a secret the user just
/// typed would be worse than a rejected request that says so.
pub fn basic(user: &str, password: &str) -> Option<String> {
    if user.is_empty() && password.is_empty() {
        return None;
    }
    let raw = format!("{user}:{password}");
    Some(format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(raw.as_bytes())
    ))
}

/// One blocking client per request: uploads and restores are rare, and a pool
/// that outlives the call would hold a proxy route to a kernel that may have
/// stopped in the meantime.
fn client(timeout: Duration, proxy: Option<u16>) -> AppResult<reqwest::blocking::Client> {
    let mut builder = reqwest::blocking::Client::builder()
        .timeout(timeout)
        .user_agent(remote::USER_AGENT);
    if let Some(port) = proxy {
        // Spelled as HTTP even for https targets: our mixed inbound answers
        // CONNECT, which is how a blocking client tunnels TLS. A `socks5://`
        // spelling would make reqwest speak SOCKS directly to a port that also
        // answers HTTP, and the handshake race is real.
        let address = format!("http://127.0.0.1:{port}");
        let proxy = reqwest::Proxy::all(&address)
            .map_err(|error| AppError::InvalidInput(format!("代理端口 {port} 用不了：{error}")))?;
        builder = builder.proxy(proxy);
    }
    builder
        .build()
        .map_err(|error| AppError::Internal(format!("无法创建 HTTP 客户端：{error}")))
}

fn authorization(request: reqwest::blocking::RequestBuilder, value: Option<String>) -> reqwest::blocking::RequestBuilder {
    match value {
        Some(header) => request.header(reqwest::header::AUTHORIZATION, header),
        None => request,
    }
}

/// PUT the whole backup and report the status the server answered with.
pub fn put_bytes(
    url: &str,
    user: &str,
    password: &str,
    body: &[u8],
    timeout: Duration,
    proxy: Option<u16>,
) -> AppResult<u16> {
    let target = resolve_target(url)?;
    let client = client(timeout, proxy)?;
    let request = client
        .put(&target)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body.to_vec());
    let response = authorization(request, basic(user, password))
        .send()
        .map_err(|error| AppError::Engine(format!("上传备份失败：{}", remote::describe_error(&error))))?;
    Ok(response.status().as_u16())
}

/// GET the backup and hand back its status and bytes.
///
/// A failed status comes back with an empty body: the command layer only reads
/// the bytes on success, and draining an error page would turn a clear HTTP
/// sentence into a "not UTF-8" parse error.
pub fn get_bytes(
    url: &str,
    user: &str,
    password: &str,
    timeout: Duration,
    proxy: Option<u16>,
) -> AppResult<(u16, Vec<u8>)> {
    let target = resolve_target(url)?;
    let client = client(timeout, proxy)?;
    let request = client.get(&target);
    let response = authorization(request, basic(user, password))
        .send()
        .map_err(|error| AppError::Engine(format!("读取云端备份失败：{}", remote::describe_error(&error))))?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Ok((status, Vec::new()));
    }
    let body = response
        .bytes()
        .map_err(|error| AppError::Engine(format!("云端备份读取失败：{error}")))?;
    Ok((status, body.to_vec()))
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    use super::*;

    #[test]
    fn a_folder_address_gets_the_backup_file_name() {
        assert_eq!(
            resolve_target("https://h/dav/").unwrap(),
            "https://h/dav/x-matrix-backup.json"
        );
    }

    #[test]
    fn a_file_address_is_left_alone() {
        assert_eq!(
            resolve_target("https://h/dav/x.json").unwrap(),
            "https://h/dav/x.json"
        );
    }

    #[test]
    fn a_bare_host_is_not_second_guessed() {
        // No trailing slash, so no file name: the server gets to answer 404 or
        // 409 instead of us inventing a path it may not use.
        assert_eq!(resolve_target("https://h").unwrap(), "https://h");
    }

    #[test]
    fn only_http_addresses_are_accepted() {
        assert!(resolve_target("ftp://h/x").is_err());
        assert!(resolve_target("h/x").is_err());
        assert!(resolve_target("  ").is_err());
    }

    #[test]
    fn basic_uses_the_standard_alphabet() {
        assert_eq!(basic("u", "p"), Some("Basic dTpw".to_string()));
    }

    #[test]
    fn no_credentials_means_no_header() {
        assert_eq!(basic("", ""), None);
    }

    #[test]
    fn a_password_without_a_username_is_still_sent() {
        // Some DAV servers keep the account name in the password field; the
        // header is the only way the user's secret ever reaches them.
        assert_eq!(basic("", "p"), Some("Basic OnA=".to_string()));
    }

    /// Read one whole request (headers plus the body promised by
    /// `Content-Length`), answer it, and hand the raw request back.
    fn serve_capture(response: &'static str) -> (u16, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            let mut data = Vec::new();
            if let Ok((mut socket, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                loop {
                    let read = socket.read(&mut buf).unwrap_or(0);
                    if read == 0 {
                        break;
                    }
                    data.extend_from_slice(&buf[..read]);
                    let text = String::from_utf8_lossy(&data);
                    if let Some((head, body)) = text.split_once("\r\n\r\n") {
                        let length = head
                            .lines()
                            .find_map(|line| {
                                let lower = line.to_ascii_lowercase();
                                lower
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                            })
                            .unwrap_or(0);
                        if body.len() >= length {
                            break;
                        }
                    }
                }
                let _ = socket.write_all(response.as_bytes());
            }
            String::from_utf8_lossy(&data).into_owned()
        });
        (port, handle)
    }

    #[test]
    fn an_upload_is_a_put_with_json_auth_and_the_exact_body() {
        let (port, handle) = serve_capture("HTTP/1.1 201 Created\r\nContent-Length: 0\r\n\r\n");
        let body = br#"{"kind": "x-matrix-backup"}"#;
        let status = put_bytes(
            &format!("http://127.0.0.1:{port}/dav/"),
            "u",
            "p",
            body,
            Duration::from_secs(2),
            None,
        )
        .unwrap();

        assert_eq!(status, 201);
        let request = handle.join().unwrap();
        assert!(
            request.starts_with("PUT /dav/x-matrix-backup.json HTTP/1.1\r\n"),
            "unexpected request: {request:?}"
        );
        // Hyper writes header names in lower case, so match it there rather
        // than pretending the wire spelling is capitalised.
        let lower = request.to_lowercase();
        assert!(lower.contains(&format!("content-length: {}\r\n", body.len())));
        assert!(lower.contains("content-type: application/json"));
        assert!(lower.contains("authorization: basic dtpw"));
        assert!(
            request.ends_with(&format!("\r\n\r\n{}", String::from_utf8_lossy(body))),
            "the body must be the last thing on the wire: {request:?}"
        );
    }

    #[test]
    fn a_download_returns_the_bytes_verbatim() {
        let (port, handle) = serve_capture(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\n\r\n{\"kind\":1}\n",
        );
        let (status, body) = get_bytes(
            &format!("http://127.0.0.1:{port}/dav/x-matrix-backup.json"),
            "u",
            "p",
            Duration::from_secs(2),
            None,
        )
        .unwrap();

        assert_eq!(status, 200);
        assert_eq!(body, b"{\"kind\":1}\n");
        let request = handle.join().unwrap();
        assert!(request.starts_with("GET /dav/x-matrix-backup.json HTTP/1.1\r\n"));
        assert!(request.to_lowercase().contains("authorization: basic dtpw"));
    }

    #[test]
    fn a_failed_status_is_reported_without_an_error() {
        // The status is data here; the command layer decides whether 401 is a
        // refusal to retry or a message about the password.
        let (port, _handle) = serve_capture("HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n");
        let (status, body) = get_bytes(
            &format!("http://127.0.0.1:{port}/dav/"),
            "u",
            "wrong",
            Duration::from_secs(2),
            None,
        )
        .unwrap();
        assert_eq!(status, 401);
        assert!(body.is_empty());
    }

    #[test]
    fn no_credentials_means_no_authorization_header_on_the_wire() {
        let (port, handle) = serve_capture("HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n");
        put_bytes(
            &format!("http://127.0.0.1:{port}/dav/x.json"),
            "",
            "",
            b"{}",
            Duration::from_secs(2),
            None,
        )
        .unwrap();
        let request = handle.join().unwrap();
        assert!(!request.to_ascii_lowercase().contains("authorization:"));
    }

    #[test]
    fn a_refused_connection_is_an_engine_error() {
        let port = {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        let error = get_bytes(
            &format!("http://127.0.0.1:{port}/dav/"),
            "",
            "",
            Duration::from_millis(200),
            None,
        )
        .unwrap_err();
        assert!(matches!(error, AppError::Engine(_)));
    }
}