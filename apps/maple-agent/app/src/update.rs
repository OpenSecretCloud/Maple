//! Release check against the GitHub releases API.
//!
//! The desktop app asks once per launch whether a newer tag exists and
//! shows a banner with the release page. Nothing is downloaded or
//! installed. `MAPLE_UPDATE_REPO` points at another `owner/repo`, and
//! `MAPLE_DISABLE_UPDATE_CHECK=1` turns the check off.

use std::sync::OnceLock;

/// GitHub repository that publishes releases, as `owner/repo`.
const DEFAULT_REPO: &str = "benthecarman/maple-gpui";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInfo {
    /// Version without the `v` prefix.
    pub version: String,
    /// Release page for the user to open.
    pub url: String,
}

static AVAILABLE: OnceLock<UpdateInfo> = OnceLock::new();

/// The newer release found by [`check`], if any.
pub fn available() -> Option<&'static UpdateInfo> {
    AVAILABLE.get()
}

pub fn enabled() -> bool {
    !crate::env::env_flag("MAPLE_DISABLE_UPDATE_CHECK")
}

fn repo() -> String {
    crate::env::env_string("MAPLE_UPDATE_REPO").unwrap_or_else(|| DEFAULT_REPO.to_string())
}

/// Ask GitHub for the latest release. Runs on the backend runtime; the
/// HTTP client must be built inside a Tokio context.
pub async fn check() -> Option<UpdateInfo> {
    if !enabled() {
        return None;
    }
    let url = format!("https://api.github.com/repos/{}/releases/latest", repo());
    let client = reqwest::Client::builder()
        .user_agent(concat!("maple-gpui/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .ok()?;
    let response = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|error| log::debug!("update check failed: {error}"))
        .ok()?;
    if !response.status().is_success() {
        log::debug!("update check: {} from {url}", response.status());
        return None;
    }
    let body: serde_json::Value = response.json().await.ok()?;
    let info = newer_release(&body, env!("CARGO_PKG_VERSION"))?;
    let _ = AVAILABLE.set(info.clone());
    Some(info)
}

/// The release described by `body` when its tag is newer than `current`.
fn newer_release(body: &serde_json::Value, current: &str) -> Option<UpdateInfo> {
    if body["draft"].as_bool() == Some(true) || body["prerelease"].as_bool() == Some(true) {
        return None;
    }
    let tag = body["tag_name"].as_str()?;
    let version = tag.trim().trim_start_matches('v').to_string();
    if parse_version(&version)? <= parse_version(current)? {
        return None;
    }
    let url = body["html_url"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| format!("https://github.com/{}/releases/latest", repo()));
    Some(UpdateInfo { version, url })
}

/// Comparable form of `major.minor.patch[-pre][+build]`. A pre-release
/// sorts below its numeric triple, as in semver, so `1.2.0-beta.1` is
/// older than `1.2.0` and a beta user is told about the final release.
/// Build metadata is ignored. Pre-release identifiers are compared as
/// plain strings, which is enough for the tags this app publishes.
fn parse_version(text: &str) -> Option<Version> {
    let text = text.trim();
    let text = text.split_once('+').map_or(text, |(core, _)| core);
    let (core, pre) = match text.split_once('-') {
        Some((core, pre)) => (core, Some(pre.to_string())),
        None => (text, None),
    };
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    Some(Version {
        triple: (major, minor, patch),
        // `Release` (None) must sort after `Pre` (Some), so the suffix is
        // wrapped in a type whose ordering puts "no suffix" last.
        release: Release::from(pre),
    })
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Version {
    triple: (u64, u64, u64),
    release: Release,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Release {
    Pre(String),
    Final,
}

impl From<Option<String>> for Release {
    fn from(pre: Option<String>) -> Self {
        match pre {
            Some(pre) => Self::Pre(pre),
            None => Self::Final,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str) -> serde_json::Value {
        serde_json::json!({
            "tag_name": tag,
            "html_url": format!("https://example.test/{tag}"),
            "draft": false,
            "prerelease": false,
        })
    }

    #[test]
    fn newer_tag_is_reported() {
        let info = newer_release(&release("v1.2.3"), "1.2.0").expect("newer");
        assert_eq!(info.version, "1.2.3");
        assert_eq!(info.url, "https://example.test/v1.2.3");
    }

    #[test]
    fn same_or_older_tag_is_ignored() {
        assert!(newer_release(&release("v1.2.0"), "1.2.0").is_none());
        assert!(newer_release(&release("v1.1.9"), "1.2.0").is_none());
        assert!(newer_release(&release("v1.2.0-beta.1"), "1.2.0").is_none());
        assert!(newer_release(&release("v1.2.0-beta.1"), "1.2.0-beta.1").is_none());
    }

    #[test]
    fn final_release_is_newer_than_its_prerelease() {
        let info = newer_release(&release("1.2.0"), "1.2.0-beta.1").expect("newer");
        assert_eq!(info.version, "1.2.0");
        assert!(newer_release(&release("v1.2.0-beta.2"), "1.2.0-beta.1").is_some());
        assert!(newer_release(&release("v1.2.1-beta.1"), "1.2.0").is_some());
        assert!(newer_release(&release("v1.2.0+build.5"), "1.2.0").is_none());
    }

    #[test]
    fn drafts_and_prereleases_are_ignored() {
        let mut draft = release("v9.0.0");
        draft["draft"] = serde_json::Value::Bool(true);
        assert!(newer_release(&draft, "1.0.0").is_none());
        let mut pre = release("v9.0.0");
        pre["prerelease"] = serde_json::Value::Bool(true);
        assert!(newer_release(&pre, "1.0.0").is_none());
    }

    #[test]
    fn malformed_tags_are_ignored() {
        assert!(newer_release(&release("latest"), "1.0.0").is_none());
        assert!(newer_release(&serde_json::json!({}), "1.0.0").is_none());
    }
}
