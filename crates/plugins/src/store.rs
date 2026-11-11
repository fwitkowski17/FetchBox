//! The plugin store's remote catalog (SPEC 6.15): a JSON list of plugins that can be installed from
//! a ZIP at an address, each with the SHA-256 of that ZIP. The catalog is fetched at most as often as
//! the update check is (cached for hours, retried after a failure, never faster than every 30
//! seconds on demand), and read defensively: whatever it says is untrusted until the ZIP it names
//! matches its checksum.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use fetchbox_core::plugin::{is_valid_id, StoreDownload, StoreEntry, MAX_PLUGIN_ZIP, PLATFORMS};
use fetchbox_core::version::parse_version;
use serde_json::Value;

use crate::fetch::Fetcher;
use crate::{Error, Result};

/// The catalog format this build reads.
pub const INDEX_FORMAT: u64 = 1;
const FRESH_FOR: Duration = Duration::from_secs(6 * 3600);
const RETRY_AFTER: Duration = Duration::from_secs(15 * 60);
const MIN_GAP: Duration = Duration::from_secs(30);
const TIMEOUT: Duration = Duration::from_secs(5);
const MAX_BYTES: usize = 1024 * 1024;
const MAX_ENTRIES: usize = 500;

fn clip(s: &str, max: usize) -> String {
    s.chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .take(max)
        .collect()
}

fn address(v: Option<&str>, allow_http: bool) -> Option<String> {
    v.filter(|u| {
        let rest = u
            .strip_prefix("https://")
            .or_else(|| allow_http.then(|| u.strip_prefix("http://")).flatten());
        rest.is_some_and(|r| !r.is_empty()) && !u.contains(char::is_whitespace)
    })
    .map(str::to_owned)
}

/// Read a catalog. A damaged entry is skipped (a catalog is edited by hand), but a document that is
/// not a catalog at all is an error.
pub fn parse_index(bytes: &[u8]) -> Result<Vec<StoreEntry>> {
    parse_index_with(bytes, false)
}

/// Like [`parse_index`]; `allow_http` accepts `http://` addresses (tests only).
pub fn parse_index_with(bytes: &[u8], allow_http: bool) -> Result<Vec<StoreEntry>> {
    let doc: Value = serde_json::from_slice(bytes)
        .map_err(|_| Error::Fetch("the plugin catalog is not JSON".into()))?;
    if doc["format"].as_u64() != Some(INDEX_FORMAT) {
        return Err(Error::Fetch(
            "the plugin catalog is in a format this FetchBox does not know: update FetchBox".into(),
        ));
    }
    let list = doc["plugins"]
        .as_array()
        .ok_or_else(|| Error::Fetch("the plugin catalog has no list of plugins".into()))?;
    let mut out = Vec::new();
    for v in list.iter().take(MAX_ENTRIES) {
        let text = |k: &str| v[k].as_str();
        let (Some(id), Some(name), Some(version)) = (text("id"), text("name"), text("version"))
        else {
            continue;
        };
        if !is_valid_id(id) || name.trim().is_empty() || parse_version(version).is_none() {
            continue;
        }
        let download = match (
            address(v["download"]["url"].as_str(), allow_http),
            v["download"]["sha256"].as_str(),
        ) {
            (Some(url), Some(sha))
                if sha.len() == 64 && sha.bytes().all(|c| c.is_ascii_hexdigit()) =>
            {
                Some(StoreDownload {
                    url,
                    sha256: sha.to_ascii_lowercase(),
                    size: v["download"]["size"]
                        .as_u64()
                        .filter(|s| *s <= MAX_PLUGIN_ZIP as u64),
                })
            }
            _ => None,
        };
        // A remote entry without a usable download cannot be installed: it is not offered.
        let Some(download) = download else { continue };
        let strings = |k: &str, max: usize| -> Vec<String> {
            v[k].as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .take(max)
                        .map(|s| clip(s, 40))
                        .collect()
                })
                .unwrap_or_default()
        };
        out.push(StoreEntry {
            id: id.to_owned(),
            name: clip(name.trim(), 80),
            version: version.to_owned(),
            description: clip(text("description").unwrap_or(""), 500),
            author: clip(text("author").unwrap_or(""), 100),
            homepage: address(v["homepage"].as_str(), false),
            license: text("license").map(|l| clip(l, 40)),
            icon: text("icon").map(|i| clip(i, 40)),
            tags: strings("tags", 8),
            min_fetchbox: text("min_fetchbox")
                .filter(|m| parse_version(m).is_some())
                .map(str::to_owned),
            platforms: strings("platforms", 8)
                .into_iter()
                .filter(|p| PLATFORMS.contains(&p.as_str()))
                .collect(),
            bundled: false,
            download: Some(download),
            installed: None,
            update_available: false,
            unsupported: false,
        });
    }
    Ok(out)
}

/// What the catalog said, and when.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub entries: Vec<StoreEntry>,
    /// Unix seconds of the last successful fetch.
    pub fetched_at: Option<i64>,
    /// Why the last try failed.
    pub error: Option<String>,
}

#[derive(Default)]
struct State {
    snapshot: Snapshot,
    ok_at: Option<Instant>,
    attempt_at: Option<Instant>,
    failed: bool,
}

/// The remote catalog with its cache. `url: None` means there is no remote catalog.
pub struct Catalog {
    fetcher: Fetcher,
    url: Option<String>,
    state: Mutex<State>,
    /// One fetch at a time.
    gate: tokio::sync::Mutex<()>,
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

impl Catalog {
    pub fn new(fetcher: Fetcher, url: Option<String>) -> Self {
        Catalog {
            fetcher,
            url,
            state: Mutex::new(State::default()),
            gate: tokio::sync::Mutex::new(()),
        }
    }

    /// Is there a remote catalog at all?
    pub fn enabled(&self) -> bool {
        self.url.is_some()
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// What was fetched last, without fetching.
    pub fn cached(&self) -> Snapshot {
        self.state().snapshot.clone()
    }

    /// The catalog: from the cache while it is fresh, else fetched. `force` asks again even if it is
    /// fresh, but never within 30 seconds of the last try.
    pub async fn get(&self, force: bool) -> Snapshot {
        let Some(url) = &self.url else {
            return Snapshot::default();
        };
        let _turn = self.gate.lock().await;
        {
            let st = self.state();
            let recent = |t: Option<Instant>, d: Duration| t.is_some_and(|t| t.elapsed() < d);
            let fresh = if st.failed {
                recent(st.attempt_at, RETRY_AFTER)
            } else {
                recent(st.ok_at, FRESH_FOR)
            };
            let too_soon = recent(st.attempt_at, MIN_GAP);
            if (fresh && !force) || too_soon {
                return st.snapshot.clone();
            }
        }
        let result = async {
            let body = self
                .fetcher
                .get_bytes(url, MAX_BYTES, TIMEOUT, None)
                .await?;
            parse_index_with(&body, self.fetcher.allows_plain_http())
        }
        .await;
        let mut st = self.state();
        st.attempt_at = Some(Instant::now());
        match result {
            Ok(entries) => {
                st.snapshot = Snapshot {
                    entries,
                    fetched_at: Some(now()),
                    error: None,
                };
                st.ok_at = Some(Instant::now());
                st.failed = false;
            }
            Err(e) => {
                tracing::debug!(error = %e, "cannot fetch the plugin catalog");
                st.snapshot.error = Some(e.to_string());
                st.failed = true;
            }
        }
        st.snapshot.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use axum::http::StatusCode;
    use axum::routing::get;
    use fetchbox_http_engine::UrlPolicy;
    use serde_json::json;

    const SHA: &str = "ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12ab12";

    fn entry(id: &str) -> Value {
        json!({
            "id": id, "name": "A plugin", "version": "1.2.0", "description": "Does things",
            "author": "Someone", "homepage": "https://example.com/p", "license": "MIT",
            "tags": ["video", "audio"], "min_fetchbox": "0.9.0", "platforms": ["linux-x86_64", "amiga"],
            "download": {"url": "https://example.com/p.zip", "sha256": SHA.to_uppercase(), "size": 1234}
        })
    }

    fn index(plugins: Vec<Value>) -> Vec<u8> {
        json!({"format": 1, "plugins": plugins})
            .to_string()
            .into_bytes()
    }

    #[test]
    fn a_catalog_entry_is_read_in_full() {
        let got = parse_index(&index(vec![entry("good-one")])).expect("catalog");
        assert_eq!(got.len(), 1);
        let e = &got[0];
        assert_eq!((e.id.as_str(), e.version.as_str()), ("good-one", "1.2.0"));
        assert_eq!(e.tags, ["video", "audio"]);
        assert_eq!(
            e.platforms,
            ["linux-x86_64"],
            "an unknown platform is dropped"
        );
        assert_eq!(e.min_fetchbox.as_deref(), Some("0.9.0"));
        let d = e.download.as_ref().expect("download");
        assert_eq!(
            (d.url.as_str(), d.sha256.as_str(), d.size),
            ("https://example.com/p.zip", SHA, Some(1234))
        );
        assert!(!e.bundled && e.installed.is_none());
    }

    #[test]
    fn damaged_entries_are_skipped_not_fatal() {
        let mut no_sha = entry("no-sha");
        no_sha["download"]["sha256"] = json!("abc");
        let mut plain_http = entry("plain-http");
        plain_http["download"]["url"] = json!("http://example.com/p.zip");
        let mut no_download = entry("no-download");
        no_download
            .as_object_mut()
            .expect("object")
            .remove("download");
        let mut bad_version = entry("bad-version");
        bad_version["version"] = json!("latest");
        let mut huge = entry("huge-size");
        huge["download"]["size"] = json!(u64::MAX);
        let mut home = entry("odd-home");
        home["homepage"] = json!("javascript:alert(1)");
        let got = parse_index(&index(vec![
            no_sha,
            plain_http,
            no_download,
            bad_version,
            entry("Bad Id"),
            json!("junk"),
            json!({"id": "x"}),
            huge,
            home,
            entry("fine-one"),
        ]))
        .expect("catalog");
        let ids: Vec<_> = got.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["huge-size", "odd-home", "fine-one"]);
        assert_eq!(
            got[0].download.as_ref().expect("d").size,
            None,
            "an absurd size is not believed"
        );
        assert_eq!(got[1].homepage, None, "only https links are kept");
    }

    #[test]
    fn something_that_is_not_a_catalog_is_an_error() {
        for bad in [
            &b"<html>"[..],
            b"[]",
            br#"{"format":1}"#,
            br#"{"format":2,"plugins":[]}"#,
            b"",
        ] {
            assert!(
                parse_index(bad).is_err(),
                "{:?}",
                String::from_utf8_lossy(bad)
            );
        }
        assert!(parse_index(&index(vec![])).expect("empty").is_empty());
    }

    #[test]
    fn a_catalog_is_capped() {
        let many: Vec<_> = (0..MAX_ENTRIES + 20)
            .map(|i| entry(&format!("p-{}", "a".repeat(1 + i % 20))))
            .collect();
        assert!(parse_index(&index(many)).expect("catalog").len() <= MAX_ENTRIES);
    }

    async fn serve(hits: Arc<AtomicUsize>, status: Arc<std::sync::atomic::AtomicU16>) -> String {
        let app = axum::Router::new().route(
            "/index.json",
            get(move || {
                hits.fetch_add(1, Ordering::SeqCst);
                let code = StatusCode::from_u16(status.load(Ordering::SeqCst)).expect("status");
                async move {
                    (
                        code,
                        String::from_utf8(index(vec![entry("served")])).expect("utf8"),
                    )
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let base = format!("http://{}", listener.local_addr().expect("addr"));
        tokio::spawn(async move { axum::serve(listener, app).await });
        format!("{base}/index.json")
    }

    #[tokio::test]
    async fn the_catalog_is_cached_and_a_failure_is_remembered() {
        let hits = Arc::new(AtomicUsize::new(0));
        let status = Arc::new(std::sync::atomic::AtomicU16::new(200));
        let url = serve(hits.clone(), status.clone()).await;
        let fetcher = Fetcher::new(UrlPolicy::default())
            .expect("fetcher")
            .allow_plain_http();
        let cat = Catalog::new(fetcher, Some(url));
        assert!(cat.enabled());
        assert!(
            cat.cached().entries.is_empty(),
            "nothing is fetched before it is asked for"
        );
        assert_eq!(hits.load(Ordering::SeqCst), 0);

        let first = cat.get(false).await;
        assert_eq!(first.entries.len(), 1);
        assert!(first.fetched_at.is_some() && first.error.is_none());
        // Fresh: no second request, not even when asked to refresh within the minimum gap.
        cat.get(false).await;
        cat.get(true).await;
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert_eq!(cat.cached().entries.len(), 1);
    }

    #[tokio::test]
    async fn a_failed_fetch_keeps_the_old_list_and_says_why() {
        let hits = Arc::new(AtomicUsize::new(0));
        let status = Arc::new(std::sync::atomic::AtomicU16::new(500));
        let url = serve(hits.clone(), status.clone()).await;
        let fetcher = Fetcher::new(UrlPolicy::default())
            .expect("fetcher")
            .allow_plain_http();
        let cat = Catalog::new(fetcher, Some(url));
        let failed = cat.get(false).await;
        assert!(failed.entries.is_empty());
        assert!(failed.error.expect("error").contains("500"));
        // The failure is remembered for a while: asking again does not hammer the server.
        cat.get(false).await;
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn without_an_address_there_is_no_remote_catalog() {
        let fetcher = Fetcher::new(UrlPolicy::default()).expect("fetcher");
        let cat = Catalog::new(fetcher, None);
        assert!(!cat.enabled());
        assert_eq!(cat.get(true).await, Snapshot::default());
    }
}
