//! The only place that talks to the internet on the user's behalf.
//!
//! Subscriptions are plain HTTP GETs. `latency.rs` measures nodes; this fetches
//! documents. Both are kept apart so the loopback-only rule of `http.rs` stays
//! visible: that module must never be pointed at a remote host.

use std::time::Duration;

use crate::error::{AppError, AppResult};

/// Generous: airport panels are often slow and sometimes gzip a lot of text.
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(20);

/// A UA that says who we are, so a provider can rate-limit us fairly.
pub const USER_AGENT: &str = concat!("X-Matrix/", env!("CARGO_PKG_VERSION"));

/// What a provider sent back.
///
/// `subscription_userinfo` is the quota header airports put on subscription
/// responses (`upload=…; download=…; total=…; expire=…`). Both V2RayN and Clash
/// show it as 已用 / 总量 / 到期, so we keep the raw header and let the domain
/// layer decide what it means.
pub struct Fetched {
    pub body: String,
    pub subscription_userinfo: Option<String>,
}

pub fn fetch_text(url: &str) -> AppResult<String> {
    fetch_text_with_timeout(url, FETCH_TIMEOUT)
}

pub fn fetch_text_with_timeout(url: &str, timeout: Duration) -> AppResult<String> {
    Ok(fetch_with_timeout(url, timeout)?.body)
}

pub fn fetch_with_timeout(url: &str, timeout: Duration) -> AppResult<Fetched> {
    fetch_as(url, timeout, None)
}

/// Same fetch, but with a caller-chosen `User-Agent`.
///
/// Some providers only answer a Clash UA and return an empty body (or a 403) to
/// anything else, so the settings carry an override. An empty override keeps our
/// own UA: sending an empty header is worse than not sending one at all.
pub fn fetch_as(url: &str, timeout: Duration, user_agent: Option<&str>) -> AppResult<Fetched> {
    fetch_through(url, timeout, user_agent, None)
}

/// Same fetch, but routed through the local mixed inbound.
///
/// `proxy` is the loopback port of the running kernel. `None` means "go direct".
/// The proxy is spelled as an HTTP proxy for both schemes: our own mixed
/// inbound speaks HTTP CONNECT, and handing reqwest a `socks5://` URL would make
/// it try SOCKS5 directly against a port that also speaks HTTP.
pub fn fetch_through(
    url: &str,
    timeout: Duration,
    user_agent: Option<&str>,
    proxy: Option<u16>,
) -> AppResult<Fetched> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(AppError::InvalidInput(format!("订阅地址必须是 http(s)：{url}")));
    }
    let agent = match user_agent.map(str::trim) {
        Some(custom) if !custom.is_empty() => custom,
        _ => USER_AGENT,
    };
    let mut builder = reqwest::blocking::Client::builder()
        .timeout(timeout)
        .user_agent(agent);
    if let Some(port) = proxy {
        let address = format!("http://127.0.0.1:{port}");
        builder = builder.proxy(
            reqwest::Proxy::all(&address)
                .map_err(|error| AppError::Internal(format!("代理地址不可用：{error}")))?,
        );
    }
    let client = builder
        .build()
        .map_err(|error| AppError::Internal(format!("无法创建 HTTP 客户端：{error}")))?;

    let response = client
        .get(url)
        .send()
        .map_err(|error| AppError::Engine(format!("拉取订阅失败：{error}")))?;

    let status = response.status();
    if !status.is_success() {
        return Err(AppError::Engine(format!("订阅返回 HTTP {}", status.as_u16())));
    }

    // Read the header before consuming the body: `text()` takes the response.
    let subscription_userinfo = response
        .headers()
        .get("subscription-userinfo")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);

    let body = response
        .text()
        .map_err(|error| AppError::Engine(format!("订阅内容读取失败：{error}")))?;

    Ok(Fetched {
        body,
        subscription_userinfo,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_non_http_url_is_rejected_before_any_socket_opens() {
        let error = fetch_text_with_timeout("file:///etc/passwd", Duration::from_millis(50))
            .expect_err("file:// must not be fetched");
        assert!(matches!(error, AppError::InvalidInput(_)), "got {error:?}");
    }

    #[test]
    fn a_connection_refused_port_is_an_error_not_a_panic() {
        // Port 1 on loopback: nothing listens there, and the failure is ours to
        // report, not to crash on.
        let error = fetch_text_with_timeout("http://127.0.0.1:1/sub", Duration::from_millis(300))
            .expect_err("nothing listens on port 1");
        assert!(matches!(error, AppError::Engine(_)), "got {error:?}");
    }
}
