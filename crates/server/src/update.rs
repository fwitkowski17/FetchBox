//! Is there a newer release? Asks the GitHub releases API now and then, keeps the answer for a
//! while, and says nothing else about the machine: the request carries the program name and
//! version in its `User-Agent`, like any HTTP client. Switched off by `Settings::check_updates`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use fetchbox_core::VersionInfo;

/// The newest published (not draft, not pre-release) release of the project.
pub const RELEASES_API: &str =
    "https://api.github.com/repos/fwitkowski17/WebDownloadManager/releases/latest";

/// Version of this program.
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// A successful answer is good for this long.
const FRESH_FOR: Duration = Duration::from_secs(6 * 60 * 60);
/// After a failure the next look waits this long, so an offline server does not retry on every page view.
const RETRY_AFTER: Duration = Duration::from_secs(15 * 60);
/// "Check now" cannot ask more often than this (GitHub allows 60 anonymous requests an hour).
const MIN_GAP: Duration = Duration::from_secs(30);
/// Do not wait for GitHub longer than this.
const TIMEOUT: Duration = Duration::from_secs(5);

/// What GitHub said about the latest release.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Release {
    version: String,
    url: String,
}

#[derive(Debug, Clone)]
enum Outcome {
    Found(Release),
    /// GitHub answered, but nothing is published yet (`404`).
    NoRelease,
    Failed,
}

struct Cache {
    outcome: Outcome,
    at: Instant,
    /// Unix seconds of the last answer that was not a failure.
    answered_at: Option<i64>,
}

pub struct UpdateChecker {
    client: reqwest::Client,
    url: String,
    /// Held while asking, so two page views share one request.
    cache: tokio::sync::Mutex<Option<Cache>>,
}

impl UpdateChecker {
    /// `url` is the releases API endpoint; `FETCHBOX_UPDATE_URL` replaces the default (a mirror,
    /// a fork, a test server).
    pub fn new(url: Option<String>) -> anyhow::Result<Arc<Self>> {
        let client = reqwest::Client::builder()
            .user_agent(format!("FetchBox/{CURRENT}"))
            .timeout(TIMEOUT)
            .build()?;
        Ok(Arc::new(Self {
            client,
            url: url.unwrap_or_else(|| RELEASES_API.to_owned()),
            cache: tokio::sync::Mutex::new(None),
        }))
    }

    /// The answer for the UI. Asks GitHub when there is no usable answer yet, or when `force` is
    /// set (but not more often than `MIN_GAP`).
    pub async fn info(&self, force: bool) -> VersionInfo {
        let mut cache = self.cache.lock().await;
        let usable = cache.as_ref().is_some_and(|c| {
            let age = c.at.elapsed();
            let limit = match (&c.outcome, force) {
                (_, true) => MIN_GAP,
                (Outcome::Failed, false) => RETRY_AFTER,
                (_, false) => FRESH_FOR,
            };
            age < limit
        });
        if !usable {
            let outcome = self.fetch().await;
            let answered_at = match outcome {
                Outcome::Failed => cache.as_ref().and_then(|c| c.answered_at),
                _ => Some(now_unix()),
            };
            *cache = Some(Cache {
                outcome,
                at: Instant::now(),
                answered_at,
            });
        }
        let c = cache.as_ref().expect("filled above");
        describe(&c.outcome, c.answered_at)
    }

    async fn fetch(&self) -> Outcome {
        let resp = match self
            .client
            .get(&self.url)
            .header("accept", "application/vnd.github+json")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                tracing::debug!(error = %e, "update check failed");
                return Outcome::Failed;
            }
        };
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Outcome::NoRelease;
        }
        if !resp.status().is_success() {
            tracing::debug!(status = %resp.status(), "update check refused");
            return Outcome::Failed;
        }
        match resp.text().await.ok().and_then(|t| parse_release(&t)) {
            Some(r) => Outcome::Found(r),
            None => Outcome::Failed,
        }
    }
}

/// Info for the UI when update checks are switched off.
pub fn disabled() -> VersionInfo {
    VersionInfo {
        current: CURRENT.to_owned(),
        enabled: false,
        latest: None,
        update_available: false,
        release_url: None,
        checked_at: None,
        check_failed: false,
    }
}

fn describe(outcome: &Outcome, answered_at: Option<i64>) -> VersionInfo {
    let mut info = VersionInfo {
        enabled: true,
        checked_at: answered_at,
        ..disabled()
    };
    match outcome {
        Outcome::Found(r) => {
            info.update_available = is_newer(&r.version, CURRENT);
            info.latest = Some(r.version.clone());
            info.release_url = Some(r.url.clone());
        }
        Outcome::NoRelease => {}
        Outcome::Failed => info.check_failed = true,
    }
    info
}

fn parse_release(body: &str) -> Option<Release> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let tag = v.get("tag_name")?.as_str()?;
    let version = tag.trim().trim_start_matches(['v', 'V']).to_owned();
    parse_version(&version)?;
    let url = v.get("html_url")?.as_str()?.to_owned();
    // The link is shown to the user and opened by them: only a web address will do.
    url.starts_with("https://")
        .then_some(Release { version, url })
}

/// `1.2.3` or `1.2.3-rc.1`: the numbers, and whether it is a pre-release.
fn parse_version(v: &str) -> Option<(Vec<u64>, bool)> {
    let (numbers, pre) = match v.split_once('-') {
        Some((n, _)) => (n, true),
        None => (v, false),
    };
    let numbers: Option<Vec<u64>> = numbers.split('.').map(|p| p.parse().ok()).collect();
    let numbers = numbers?;
    (!numbers.is_empty()).then_some((numbers, pre))
}

/// Is `latest` a higher version than `current`? A pre-release ranks below its release
/// (`0.7.0-rc.1` < `0.7.0`); anything that is not a version is never "newer".
fn is_newer(latest: &str, current: &str) -> bool {
    let (Some((a, a_pre)), Some((b, b_pre))) = (parse_version(latest), parse_version(current))
    else {
        return false;
    };
    let len = a.len().max(b.len());
    for i in 0..len {
        let (x, y) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return x > y;
        }
    }
    // Same numbers: only a release is newer than a pre-release.
    b_pre && !a_pre
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use axum::routing::get;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn versions_are_compared_by_their_numbers() {
        assert!(is_newer("0.7.0", "0.6.1"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(!is_newer("0.6.1", "0.6.1"));
        assert!(!is_newer("0.6.0", "0.6.1"));
        assert!(!is_newer("1.0.0", "1"));
    }

    #[test]
    fn a_pre_release_ranks_below_its_release() {
        assert!(is_newer("0.7.0", "0.7.0-rc.1"));
        assert!(!is_newer("0.7.0-rc.1", "0.7.0"));
        assert!(is_newer("0.7.1-rc.1", "0.7.0"));
    }

    #[test]
    fn something_that_is_not_a_version_is_never_newer() {
        assert!(!is_newer("nightly", "0.6.1"));
        assert!(!is_newer("0.7.0", "dev"));
        assert!(!is_newer("", "0.6.1"));
    }

    #[test]
    fn a_release_is_read_from_the_github_answer() {
        let body = r#"{"tag_name":"v0.7.0","html_url":"https://github.com/o/r/releases/tag/v0.7.0","name":"x"}"#;
        assert_eq!(
            parse_release(body),
            Some(Release {
                version: "0.7.0".into(),
                url: "https://github.com/o/r/releases/tag/v0.7.0".into()
            })
        );
        // Not a version, not a web link, not JSON.
        assert_eq!(
            parse_release(r#"{"tag_name":"nightly","html_url":"https://x"}"#),
            None
        );
        assert_eq!(
            parse_release(r#"{"tag_name":"v1.0.0","html_url":"javascript:alert(1)"}"#),
            None
        );
        assert_eq!(parse_release("<html>"), None);
    }

    async fn serve(status: StatusCode, body: &'static str, hits: Arc<AtomicUsize>) -> String {
        let app = axum::Router::new().route(
            "/latest",
            get(move || {
                hits.fetch_add(1, Ordering::SeqCst);
                async move { (status, body) }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        format!("http://{addr}/latest")
    }

    const NEWER: &str =
        r#"{"tag_name":"v99.0.0","html_url":"https://github.com/o/r/releases/tag/v99.0.0"}"#;

    #[tokio::test]
    async fn a_newer_release_is_reported_and_the_answer_is_reused() {
        let hits = Arc::new(AtomicUsize::new(0));
        let url = serve(StatusCode::OK, NEWER, hits.clone()).await;
        let checker = UpdateChecker::new(Some(url)).expect("checker");

        let a = checker.info(false).await;
        assert!(a.enabled && a.update_available && !a.check_failed);
        assert_eq!(a.latest.as_deref(), Some("99.0.0"));
        assert_eq!(a.current, CURRENT);
        assert!(a.checked_at.is_some());
        let b = checker.info(false).await;
        assert_eq!(a, b);
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "the second look is served from the cache"
        );
        // "Check now" inside the minimum gap does not ask again either.
        checker.info(true).await;
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn the_current_version_is_up_to_date() {
        let body: &'static str = Box::leak(
            format!(r#"{{"tag_name":"v{CURRENT}","html_url":"https://github.com/o/r/x"}}"#)
                .into_boxed_str(),
        );
        let url = serve(StatusCode::OK, body, Arc::default()).await;
        let info = UpdateChecker::new(Some(url))
            .expect("checker")
            .info(false)
            .await;
        assert!(!info.update_available);
        assert_eq!(info.latest.as_deref(), Some(CURRENT));
    }

    #[tokio::test]
    async fn no_release_yet_is_not_an_error() {
        let url = serve(StatusCode::NOT_FOUND, "{}", Arc::default()).await;
        let info = UpdateChecker::new(Some(url))
            .expect("checker")
            .info(false)
            .await;
        assert!(info.enabled && !info.check_failed && !info.update_available);
        assert_eq!(info.latest, None);
        assert!(info.checked_at.is_some());
    }

    #[tokio::test]
    async fn an_unreachable_server_is_reported_as_a_failed_check() {
        // Nothing listens on the discard port.
        let checker =
            UpdateChecker::new(Some("http://127.0.0.1:9/latest".into())).expect("checker");
        let info = checker.info(false).await;
        assert!(info.enabled && info.check_failed && info.latest.is_none());
        assert_eq!(info.checked_at, None);
    }

    #[tokio::test]
    async fn rate_limiting_and_garbage_count_as_failures() {
        let url = serve(StatusCode::FORBIDDEN, "{}", Arc::default()).await;
        assert!(
            UpdateChecker::new(Some(url))
                .expect("c")
                .info(false)
                .await
                .check_failed
        );
        let url = serve(StatusCode::OK, "not json", Arc::default()).await;
        assert!(
            UpdateChecker::new(Some(url))
                .expect("c")
                .info(false)
                .await
                .check_failed
        );
    }
}
