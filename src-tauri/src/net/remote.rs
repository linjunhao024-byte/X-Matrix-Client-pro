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
    let answer = fetch_answer(url, timeout, user_agent, proxy, "订阅")?;
    if !(200..300).contains(&answer.status) {
        return Err(AppError::Engine(format!("订阅返回 HTTP {}", answer.status)));
    }
    Ok(Fetched {
        body: answer.body,
        subscription_userinfo: answer.subscription_userinfo,
    })
}

/// Flatten an error and its `source()` chain into one line.
///
/// reqwest's own `Display` is "error sending request for url (...)", which says
/// nothing about *why*: DNS failure, TLS rejection and a timeout all read the
/// same. The actual cause sits one or two `source()` steps down, so the chain
/// is joined with `：` and the sentence the user sees names the real problem.
///
/// Repeated text is dropped: some layers re-wrap their cause with the same
/// words, and "失败：x：x" reads like a bug rather than a diagnosis.
pub fn describe_error(error: &(dyn std::error::Error + 'static)) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut current: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(cause) = current {
        let text = cause.to_string();
        if !text.is_empty() && !parts.iter().any(|part| part == &text) {
            parts.push(text);
        }
        current = cause.source();
    }
    parts.join("：")
}

/// Bytes, for the two binary payloads we fetch on purpose (geo data).
///
/// `Fetched` carries a `String` because a subscription is text; a `.dat` file
/// decoded as UTF-8 would be silently mangled. Status handling stays the
/// caller's job, and `noun` keeps the failure sentences honest.
pub fn fetch_bytes(url: &str, timeout: Duration, noun: &str) -> AppResult<(u16, Vec<u8>)> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(AppError::InvalidInput(format!(
            "{noun}地址必须是 http(s)：{url}"
        )));
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(timeout)
        .user_agent(USER_AGENT)
        .build()
        .map_err(|error| AppError::Internal(format!("无法创建 HTTP 客户端：{error}")))?;
    let response = client
        .get(url)
        .send()
        // The cause chain is spelled out here on purpose: this error is the one
        // shown for a failed geo download, and "error sending request" alone
        // cannot be acted on.
        .map_err(|error| AppError::Engine(format!("拉取{noun}失败：{}", describe_error(&error))))?;
    let status = response.status().as_u16();
    let body = response
        .bytes()
        .map_err(|error| AppError::Engine(format!("{noun}内容读取失败：{error}")))?;
    Ok((status, body.to_vec()))
}

/// A response whose status the caller words itself.
///
/// The release feed answers 404 until the project has published something, and
/// "订阅返回 HTTP 404" is the wrong sentence on a screen that is not about
/// subscriptions. Everything before the status — validation, client, request —
/// stays shared with `fetch_through`, so one place still knows how to reach a
/// remote host.
pub struct Answer {
    pub status: u16,
    pub body: String,
    pub subscription_userinfo: Option<String>,
}

/// `noun` names the thing being fetched in every failure this can produce
/// ("订阅", "发布页"), so a caller never has to rewrite our error text.
pub fn fetch_answer(
    url: &str,
    timeout: Duration,
    user_agent: Option<&str>,
    proxy: Option<u16>,
    noun: &str,
) -> AppResult<Answer> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(AppError::InvalidInput(format!(
            "{noun}地址必须是 http(s)：{url}"
        )));
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
        .map_err(|error| AppError::Engine(format!("拉取{noun}失败：{error}")))?;

    let status = response.status().as_u16();

    // Read the header before consuming the body: `text()` takes the response.
    let subscription_userinfo = response
        .headers()
        .get("subscription-userinfo")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);

    let body = response
        .text()
        .map_err(|error| AppError::Engine(format!("{noun}内容读取失败：{error}")))?;

    Ok(Answer {
        status,
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

    /// A TLS failure the way reqwest wraps one: the send error on top, the
    /// real reason one `source()` step below.
    #[derive(Debug)]
    struct TlsRefused;

    impl std::fmt::Display for TlsRefused {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "invalid peer certificate: UnknownIssuer")
        }
    }

    impl std::error::Error for TlsRefused {}

    #[derive(Debug)]
    struct SendFailed;

    impl std::fmt::Display for SendFailed {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(
                f,
                "error sending request for url (https://cdn.jsdelivr.net/gh/…)"
            )
        }
    }

    impl std::error::Error for SendFailed {
        fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
            Some(&TlsRefused)
        }
    }

    #[test]
    fn a_send_failure_names_the_underlying_cause_too() {
        let text = describe_error(&SendFailed);
        assert_eq!(
            text,
            "error sending request for url (https://cdn.jsdelivr.net/gh/…)\
             ：invalid peer certificate: UnknownIssuer"
        );
        assert_eq!(text.matches('：').count(), 1, "{text}");
    }
}
