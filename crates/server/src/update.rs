//! Is there a newer release? Asks the GitHub releases API now and then, keeps the answer for a
//! while, and says nothing else about the machine: the request carries the program name and
//! version in its `User-Agent`, like any HTTP client. Switched off by `Settings::check_updates`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use fetchbox_core::{ReleaseNote, VersionInfo};

/// The latest published releases of the project (drafts are not listed for anonymous requests;
/// pre-releases are filtered out below).
pub const RELEASES_API: &str =
    "https://api.github.com/repos/fwitkowski17/WebDownloadManager/releases?per_page=20";

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

/// How many releases newer than this one are described to the user.
const MAX_NOTES: usize = 10;
/// A longer release description is cut here (characters).
const MAX_NOTE_CHARS: usize = 8000;

/// A published release.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Release {
    version: String,
    url: String,
    body: String,
}

#[derive(Debug, Clone)]
enum Outcome {
    /// The published releases, newest first (never empty).
    Found(Vec<Release>),
    /// GitHub answered, but nothing is published yet.
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
        match resp.text().await.ok().and_then(|t| parse_releases(&t)) {
            Some(r) if r.is_empty() => Outcome::NoRelease,
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
        notes: Vec::new(),
    }
}

fn describe(outcome: &Outcome, answered_at: Option<i64>) -> VersionInfo {
    let mut info = VersionInfo {
        enabled: true,
        checked_at: answered_at,
        ..disabled()
    };
    match outcome {
        Outcome::Found(releases) => {
            let latest = &releases[0];
            info.update_available = is_newer(&latest.version, CURRENT);
            info.latest = Some(latest.version.clone());
            info.release_url = Some(latest.url.clone());
            info.notes = releases
                .iter()
                .filter(|r| is_newer(&r.version, CURRENT))
                .take(MAX_NOTES)
                .map(|r| ReleaseNote {
                    version: r.version.clone(),
                    url: r.url.clone(),
                    body: r.body.chars().take(MAX_NOTE_CHARS).collect(),
                })
                .collect();
        }
        Outcome::NoRelease => {}
        Outcome::Failed => info.check_failed = true,
    }
    info
}

/// The published releases of a GitHub answer (a JSON list), newest version first. Drafts,
/// pre-releases and entries whose tag is not a version or whose link is not a web address are
/// left out. `None` when the answer is not a list at all.
fn parse_releases(body: &str) -> Option<Vec<Release>> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let mut out: Vec<Release> = v
        .as_array()?
        .iter()
        .filter_map(|item| {
            let flag = |k: &str| item.get(k).and_then(|f| f.as_bool()).unwrap_or(false);
            if flag("draft") || flag("prerelease") {
                return None;
            }
            let tag = item.get("tag_name")?.as_str()?;
            let version = tag.trim().trim_start_matches(['v', 'V']).to_owned();
            parse_version(&version)?;
            let url = item.get("html_url")?.as_str()?.to_owned();
            // The link is shown to the user and opened by them: only a web address will do.
            if !url.starts_with("https://") {
                return None;
            }
            let body = item
                .get("body")
                .and_then(|b| b.as_str())
                .unwrap_or_default()
                .to_owned();
            Some(Release { version, url, body })
        })
        .collect();
    out.sort_by(|a, b| {
        if is_newer(&a.version, &b.version) {
            std::cmp::Ordering::Less
        } else if is_newer(&b.version, &a.version) {
            std::cmp::Ordering::Greater
        } else {
            std::cmp::Ordering::Equal
        }
    });
    Some(out)
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
    fn releases_are_read_from_the_github_list_newest_first() {
        let body = r###"[
            {"tag_name":"v0.7.0","html_url":"https://github.com/o/r/releases/tag/v0.7.0","body":"## Changes\n- a"},
            {"tag_name":"v0.9.0-rc.1","html_url":"https://github.com/o/r/x","prerelease":true},
            {"tag_name":"v0.8.0","html_url":"https://github.com/o/r/releases/tag/v0.8.0","body":null},
            {"tag_name":"v0.10.0","html_url":"https://github.com/o/r/releases/tag/v0.10.0","draft":true},
            {"tag_name":"nightly","html_url":"https://github.com/o/r/y"},
            {"tag_name":"v1.0.0","html_url":"javascript:alert(1)"}
        ]"###;
        let list = parse_releases(body).expect("a list");
        let versions: Vec<_> = list.iter().map(|r| r.version.as_str()).collect();
        assert_eq!(versions, vec!["0.8.0", "0.7.0"]);
        assert_eq!(list[1].body, "## Changes\n- a");
        assert_eq!(list[0].body, "");
        assert_eq!(parse_releases("[]"), Some(Vec::new()));
        // Not a list, not JSON.
        assert_eq!(parse_releases(r#"{"message":"Not Found"}"#), None);
        assert_eq!(parse_releases("<html>"), None);
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

    const NEWER: &str = r###"[
        {"tag_name":"v100.0.0-rc.1","html_url":"https://github.com/o/r/releases/tag/v100.0.0-rc.1","prerelease":true,"body":"candidate"},
        {"tag_name":"v99.0.0","html_url":"https://github.com/o/r/releases/tag/v99.0.0","body":"## What is new\n- Faster"},
        {"tag_name":"v98.0.0","html_url":"https://github.com/o/r/releases/tag/v98.0.0","body":"- Earlier"},
        {"tag_name":"v0.0.1","html_url":"https://github.com/o/r/releases/tag/v0.0.1","body":"- ancient"}
    ]"###;

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
        // Only what is newer than this version is described, newest first; no pre-release.
        let notes: Vec<_> = a
            .notes
            .iter()
            .map(|n| (n.version.as_str(), n.body.as_str()))
            .collect();
        assert_eq!(
            notes,
            vec![
                ("99.0.0", "## What is new\n- Faster"),
                ("98.0.0", "- Earlier")
            ]
        );
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
            format!(
                r#"[{{"tag_name":"v{CURRENT}","html_url":"https://github.com/o/r/x","body":"now"}}]"#
            )
                .into_boxed_str(),
        );
        let url = serve(StatusCode::OK, body, Arc::default()).await;
        let info = UpdateChecker::new(Some(url))
            .expect("checker")
            .info(false)
            .await;
        assert!(!info.update_available);
        assert_eq!(info.latest.as_deref(), Some(CURRENT));
        assert!(info.notes.is_empty(), "nothing is newer than this version");
    }

    #[tokio::test]
    async fn no_release_yet_is_not_an_error() {
        // A repository without releases answers with an empty list; a missing one with a 404.
        for (status, body) in [(StatusCode::OK, "[]"), (StatusCode::NOT_FOUND, "{}")] {
            let url = serve(status, body, Arc::default()).await;
            let info = UpdateChecker::new(Some(url))
                .expect("checker")
                .info(false)
                .await;
            assert!(info.enabled && !info.check_failed && !info.update_available);
            assert_eq!(info.latest, None);
            assert!(info.checked_at.is_some());
        }
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
