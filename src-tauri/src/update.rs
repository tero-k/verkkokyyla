//! Opt-in "new version available" notifier.
//!
//! Queries the GitHub Releases API for the latest stable release and reports
//! it to the frontend when it is newer than the running build. Nothing is
//! downloaded or installed; the frontend only renders a link to the release
//! page. Every failure mode resolves to "no update" so a flaky network can
//! never surface an error UI for a background convenience check.

use std::time::Duration;

use serde::{Deserialize, Serialize};

const LATEST_RELEASE_API_URL: &str =
    "https://api.github.com/repos/tero-k/verkkokyyla/releases/latest";
const RELEASES_PAGE_URL: &str = "https://github.com/tero-k/verkkokyyla/releases/latest";

/// What the frontend needs to render the update notice.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfoDto {
    pub version: String,
    pub url: String,
    pub current: String,
}

/// The two fields the app reads from the GitHub release payload.
#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
}

fn build_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(5))
        // The GitHub API rejects requests without a User-Agent.
        .user_agent(concat!("verkkokyyla/", env!("CARGO_PKG_VERSION")))
        .build()
}

/// Parse a release tag like `v0.1.4` into a semver version. Prerelease
/// versions are rejected outright: even if the API ever hands us one, users
/// must never be nudged onto a beta.
fn parse_release_tag(tag: &str) -> Option<semver::Version> {
    let version = semver::Version::parse(tag.strip_prefix('v').unwrap_or(tag)).ok()?;
    if !version.pre.is_empty() {
        return None;
    }
    Some(version)
}

/// Only trust release URLs that point back to GitHub over HTTPS; anything
/// unexpected falls back to the hardcoded releases page.
fn release_page_url(html_url: &str) -> String {
    if html_url.starts_with("https://github.com/") {
        html_url.to_string()
    } else {
        RELEASES_PAGE_URL.to_string()
    }
}

/// Decide whether a release is worth telling the user about.
fn evaluate_release(
    tag_name: &str,
    html_url: &str,
    current: &semver::Version,
) -> Option<UpdateInfoDto> {
    let latest = parse_release_tag(tag_name)?;
    if latest <= *current {
        return None;
    }
    Some(UpdateInfoDto {
        version: latest.to_string(),
        url: release_page_url(html_url),
        current: current.to_string(),
    })
}

/// Fetch the latest stable release and compare it with the running build.
/// `api_url` is a parameter so tests can point the check at a mock server.
async fn fetch_latest_release(client: &reqwest::Client, api_url: &str) -> Option<UpdateInfoDto> {
    let body = client
        .get(api_url)
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .text()
        .await
        .ok()?;
    let release: GitHubRelease = serde_json::from_str(&body).ok()?;
    let current = semver::Version::parse(env!("CARGO_PKG_VERSION")).ok()?;
    evaluate_release(&release.tag_name, &release.html_url, &current)
}

/// Check GitHub Releases for a version newer than this build. Returns `None`
/// when the app is up to date or when anything about the check fails —
/// offline, timeout, outage, or a payload that does not parse.
#[tauri::command]
pub async fn check_for_update() -> Option<UpdateInfoDto> {
    let client = build_client().ok()?;
    fetch_latest_release(&client, LATEST_RELEASE_API_URL).await
}

/// The running application version, for display in Help.
#[tauri::command]
pub fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use semver::Version;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    fn current() -> Version {
        // Matches env!("CARGO_PKG_VERSION") for this crate.
        Version::parse(env!("CARGO_PKG_VERSION")).unwrap()
    }

    #[test]
    fn evaluate_release_reports_newer_version() {
        let info = evaluate_release(
            "v0.2.0",
            "https://github.com/tero-k/verkkokyyla/releases/tag/v0.2.0",
            &current(),
        )
        .expect("newer release should be reported");
        assert_eq!(info.version, "0.2.0");
        assert_eq!(info.current, env!("CARGO_PKG_VERSION"));
        assert_eq!(
            info.url,
            "https://github.com/tero-k/verkkokyyla/releases/tag/v0.2.0"
        );
    }

    #[test]
    fn evaluate_release_accepts_tag_without_v_prefix() {
        let info = evaluate_release(
            "0.2.0",
            "https://github.com/tero-k/verkkokyyla/releases",
            &current(),
        );
        assert_eq!(info.expect("newer release").version, "0.2.0");
    }

    #[test]
    fn evaluate_release_ignores_equal_version() {
        let tag = format!("v{}", env!("CARGO_PKG_VERSION"));
        assert_eq!(
            evaluate_release(
                &tag,
                "https://github.com/tero-k/verkkokyyla/releases",
                &current()
            ),
            None
        );
    }

    #[test]
    fn evaluate_release_ignores_older_version() {
        assert_eq!(
            evaluate_release(
                "v0.0.1",
                "https://github.com/tero-k/verkkokyyla/releases",
                &current()
            ),
            None
        );
    }

    #[test]
    fn evaluate_release_never_reports_prereleases() {
        // Newer than current, but a beta: users must stay on stable.
        assert_eq!(
            evaluate_release(
                "v999.0.0-beta.1",
                "https://github.com/tero-k/verkkokyyla/releases",
                &current()
            ),
            None
        );
    }

    #[test]
    fn evaluate_release_rejects_unparseable_tags() {
        assert_eq!(
            evaluate_release(
                "latest",
                "https://github.com/tero-k/verkkokyyla/releases",
                &current()
            ),
            None
        );
        assert_eq!(
            evaluate_release(
                "v1.2",
                "https://github.com/tero-k/verkkokyyla/releases",
                &current()
            ),
            None
        );
    }

    #[test]
    fn evaluate_release_falls_back_to_releases_page_for_foreign_urls() {
        let info = evaluate_release("v0.2.0", "https://evil.example/steal", &current())
            .expect("newer release should be reported");
        assert_eq!(info.url, RELEASES_PAGE_URL);
    }

    #[tokio::test]
    async fn fetch_reports_newer_release() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/releases/latest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "tag_name": "v99.0.0",
                "html_url": "https://github.com/tero-k/verkkokyyla/releases/tag/v99.0.0"
            })))
            .mount(&server)
            .await;
        let client = build_client().unwrap();
        let info = fetch_latest_release(&client, &format!("{}/releases/latest", server.uri()))
            .await
            .expect("newer release should be reported");
        assert_eq!(info.version, "99.0.0");
    }

    #[tokio::test]
    async fn fetch_returns_none_for_older_release() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/releases/latest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "tag_name": "v0.0.1",
                "html_url": "https://github.com/tero-k/verkkokyyla/releases/tag/v0.0.1"
            })))
            .mount(&server)
            .await;
        let client = build_client().unwrap();
        assert_eq!(
            fetch_latest_release(&client, &format!("{}/releases/latest", server.uri())).await,
            None
        );
    }

    #[tokio::test]
    async fn fetch_returns_none_for_malformed_payload() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/releases/latest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "unexpected": "shape"
            })))
            .mount(&server)
            .await;
        let client = build_client().unwrap();
        assert_eq!(
            fetch_latest_release(&client, &format!("{}/releases/latest", server.uri())).await,
            None
        );
    }

    #[tokio::test]
    async fn fetch_returns_none_for_server_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/releases/latest"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        let client = build_client().unwrap();
        assert_eq!(
            fetch_latest_release(&client, &format!("{}/releases/latest", server.uri())).await,
            None
        );
    }
}
