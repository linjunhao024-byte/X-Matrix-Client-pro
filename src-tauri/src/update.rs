//! Asking GitHub whether a newer X-Matrix exists.
//!
//! Nothing here downloads or installs anything: the answer is "there is a
//! newer tag, here is the page", and opening that page stays the user's own
//! click. An app that silently replaces its own binary is exactly the thing
//! people are right to be suspicious of.
//!
//! The whole module is a pure function of (JSON text, current version) plus one
//! call to `net::remote`. That is deliberate: the release feed is unreachable
//! from CI, so the parsing and the version comparison have to be testable
//! without a socket.

use serde::Deserialize;

use crate::error::{AppError, AppResult};

/// Where releases are announced for this app.
///
/// GitHub's `latest` endpoint (rather than the list) because "latest" is the
/// question we are asking, and it keeps the response small. A private or
/// self-hosted feed can be swapped in later through settings; until then a
/// constant keeps the surface honest.
pub const FEED_URL: &str =
    "https://api.github.com/repos/linjunhao024-byte/X-Matrix-Client-pro/releases/latest";

/// Same budget as a subscription fetch: this is a favour, not a page load.
pub const FEED_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// What the About page shows after a check.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct UpdateStatus {
    pub current: String,
    pub latest: String,
    pub has_update: bool,
    /// The release page, opened by the user. Empty when the feed omitted it.
    pub url: String,
    /// The release notes, trimmed by the caller for display.
    pub notes: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    body: Option<String>,
}

/// `v1.2.3` / `1.2.3` / `1.2` → `(1, 2, 3)`. Anything else is `None`.
///
/// We do not pull in a semver crate for this: the only comparison we ever make
/// is "is the feed ahead of us", and a wrong answer here costs a user one
/// confusing sentence, not a broken install.
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let trimmed = text.trim();
    let trimmed = trimmed.strip_prefix(['v', 'V']).unwrap_or(trimmed);
    let mut parts = trimmed.split('.');
    let major = parts.next()?.parse::<u64>().ok()?;
    let minor = parts.next().and_then(|part| part.parse::<u64>().ok()).unwrap_or(0);
    let patch = parts
        .next()
        .and_then(|part| {
            // `1.2.3-beta.1` still ships as 1.2.3: a pre-release tag is not what
            // a stable user should be told to upgrade to.
            let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
            digits.parse::<u64>().ok()
        })
        .unwrap_or(0);
    Some((major, minor, patch))
}

/// True when `latest` is strictly ahead of `current`.
///
/// An unparsable version on either side answers `false`: "we could not tell"
/// must never turn into a nag.
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(latest), Some(current)) => latest > current,
        _ => false,
    }
}

/// Turn a release payload into the three facts the UI needs.
pub fn parse_release(json: &str, current: &str) -> AppResult<UpdateStatus> {
    let release: Release = serde_json::from_str(json)
        .map_err(|error| AppError::Engine(format!("更新信息不是合法的 JSON：{error}")))?;
    let latest = release.tag_name.trim().to_string();
    if latest.is_empty() {
        return Err(AppError::Engine("更新信息里没有版本号".into()));
    }
    Ok(UpdateStatus {
        current: current.to_string(),
        has_update: is_newer(&latest, current),
        latest,
        url: release.html_url.unwrap_or_default().trim().to_string(),
        notes: release.body.unwrap_or_default().trim().to_string(),
    })
}

/// What a non-2xx answer means when the question was a release feed.
///
/// 404 is the ordinary state of a project that has not published anything yet,
/// and it is not an error worth alarming anyone about — it just needs a
/// sentence that says so instead of a status code borrowed from the
/// subscription screen.
fn explain_status(status: u16) -> AppError {
    match status {
        404 => AppError::Engine("发布页还没有发布记录（HTTP 404）".into()),
        other => AppError::Engine(format!("发布页返回 HTTP {other}")),
    }
}

/// Ask the feed. Separate from `parse_release` so tests never open a socket.
pub fn check(current: &str) -> AppResult<UpdateStatus> {
    let answer = crate::net::remote::fetch_answer(FEED_URL, FEED_TIMEOUT, None, None, "发布页")?;
    if !(200..300).contains(&answer.status) {
        return Err(explain_status(answer.status));
    }
    parse_release(&answer.body, current)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"{
        "tag_name": "v0.2.0",
        "html_url": "https://example.invalid/releases/v0.2.0",
        "body": "  faster start\n"
    }"#;

    #[test]
    fn a_newer_tag_is_reported_with_its_page_and_notes() {
        let status = parse_release(FIXTURE, "0.1.0").expect("fixture parses");
        assert!(status.has_update);
        assert_eq!(status.latest, "v0.2.0");
        assert_eq!(status.current, "0.1.0");
        assert_eq!(status.url, "https://example.invalid/releases/v0.2.0");
        assert_eq!(status.notes, "faster start");
    }

    #[test]
    fn the_same_version_is_not_an_update() {
        let status = parse_release(FIXTURE, "0.2.0").expect("fixture parses");
        assert!(!status.has_update, "0.2.0 must not be told it has an update");
    }

    #[test]
    fn a_shorter_feed_version_still_compares() {
        // 0.2 vs 0.1.9: the missing patch reads as 0, so 0.1.9 is ahead.
        assert!(is_newer("0.2", "0.1.9"));
        assert!(!is_newer("0.1.9", "0.2"));
    }

    #[test]
    fn prerelease_suffixes_do_not_become_upgrades() {
        // 1.3.0-beta.1 parses as 1.3.0, so a user on 1.3.0 is left alone.
        assert_eq!(parse_version("1.3.0-beta.1"), Some((1, 3, 0)));
        assert!(!is_newer("1.3.0-beta.1", "1.3.0"));
        assert!(is_newer("1.3.1", "1.3.0-beta.1"));
    }

    #[test]
    fn nonsense_versions_never_nag() {
        assert_eq!(parse_version("nightly"), None);
        assert!(!is_newer("nightly", "0.1.0"));
        assert!(!is_newer("0.2.0", "not a version"));
    }

    #[test]
    fn a_missing_tag_is_an_error_not_an_empty_string() {
        let error = parse_release("{\"tag_name\":\"\"}", "0.1.0").expect_err("empty tag");
        assert!(matches!(error, AppError::Engine(_)), "got {error:?}");
    }

    #[test]
    fn a_payload_that_is_not_a_release_is_reported_as_such() {
        let error = parse_release("<html>403</html>", "0.1.0").expect_err("not json");
        assert!(
            error.to_string().contains("JSON"),
            "the message should say what went wrong: {error}"
        );
    }

    #[test]
    fn an_unpublished_repository_says_so_instead_of_quoting_a_status_code() {
        // GitHub answers 404 for /releases/latest until the first release
        // exists. "订阅返回 HTTP 404" was what the About page used to show,
        // because the fetch helper only knew how to talk about subscriptions.
        let message = explain_status(404).to_string();
        assert!(message.contains("还没有发布记录"), "got {message}");
        assert!(!message.contains("订阅"), "got {message}");
    }

    #[test]
    fn other_statuses_still_name_the_feed_and_the_code() {
        let message = explain_status(500).to_string();
        assert!(message.contains("发布页"), "got {message}");
        assert!(message.contains("500"), "got {message}");
    }
}
