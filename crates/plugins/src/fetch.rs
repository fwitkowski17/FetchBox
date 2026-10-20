//! Fetching things for plugins: dependency binaries, release metadata, the store catalog.
//!
//! Every address goes through the same URL policy as the downloads (`--block-private-urls`, SPEC 6.7)
//! and must be `https://`; hashes are computed while the bytes arrive, and nothing is trusted until
//! it matches.

use std::path::Path;
use std::time::{Duration, Instant};

use fetchbox_http_engine::UrlPolicy;
use futures_util::StreamExt;
use reqwest::{Client, Url};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::{Error, Result};

/// How long a whole download of a dependency may take.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// Progress is reported at most this often.
const PROGRESS_EVERY: Duration = Duration::from_millis(250);

/// An HTTP client for fetching on behalf of plugins.
#[derive(Debug, Clone)]
pub struct Fetcher {
    client: Client,
    policy: UrlPolicy,
    allow_http: bool,
    github_api: String,
}

impl Fetcher {
    pub fn new(policy: UrlPolicy) -> Result<Self> {
        let client = policy
            .apply(
                Client::builder()
                    .user_agent(concat!("FetchBox/", env!("CARGO_PKG_VERSION")))
                    .connect_timeout(Duration::from_secs(15))
                    .read_timeout(Duration::from_secs(30)),
            )
            .build()
            .map_err(|e| Error::Fetch(format!("cannot build an HTTP client: {e}")))?;
        Ok(Fetcher {
            client,
            policy,
            allow_http: false,
            github_api: "https://api.github.com".into(),
        })
    }

    /// Accept `http://` addresses. Only for tests, which serve from the loopback; a manifest is
    /// never allowed to name one (`fetchbox_core::plugin::validate`).
    #[must_use]
    pub fn allow_plain_http(mut self) -> Self {
        self.allow_http = true;
        self
    }

    /// Use another server for the GitHub releases API (tests, a mirror).
    #[must_use]
    pub fn with_github_api(mut self, base: impl Into<String>) -> Self {
        self.github_api = base.into().trim_end_matches('/').to_owned();
        self
    }

    pub(crate) fn github_api(&self) -> &str {
        &self.github_api
    }

    /// Parse an address and check that it may be fetched.
    pub fn check(&self, url: &str) -> Result<Url> {
        self.check_scheme(url, self.allow_http)
    }

    fn check_scheme(&self, url: &str, allow_http: bool) -> Result<Url> {
        let parsed = Url::parse(url)
            .map_err(|e| Error::Fetch(format!("\"{url}\" is not an address: {e}")))?;
        let scheme_ok = parsed.scheme() == "https" || (allow_http && parsed.scheme() == "http");
        if !scheme_ok {
            return Err(Error::Fetch(format!(
                "\"{url}\" is not an https:// address"
            )));
        }
        if !parsed.username().is_empty() || parsed.password().is_some() {
            return Err(Error::Fetch(
                "an address with a login in it is not accepted".into(),
            ));
        }
        self.policy
            .check(&parsed)
            .map_err(|e| Error::Fetch(format!("{}: {e}", host_for_message(&parsed))))?;
        Ok(parsed)
    }

    /// Send a request on behalf of a plugin (a webhook). Unlike a download, it may go to `http://`:
    /// the services a homelab notifies are often on the LAN without TLS. The URL policy still
    /// applies. Returns the status when it is a success.
    pub async fn send(
        &self,
        method: &str,
        url: &str,
        headers: &[(String, String)],
        body: Vec<u8>,
        content_type: &str,
        timeout: Duration,
    ) -> Result<u16> {
        let url = self.check_scheme(url, true)?;
        let host = host_for_message(&url);
        let method = reqwest::Method::from_bytes(method.as_bytes())
            .map_err(|_| Error::Fetch(format!("\"{method}\" is not an HTTP method")))?;
        let mut req = self
            .client
            .request(method, url)
            .header(reqwest::header::CONTENT_TYPE, content_type)
            .body(body)
            .timeout(timeout);
        for (k, v) in headers {
            req = req.header(k.as_str(), v.as_str());
        }
        let resp = req
            .send()
            .await
            .map_err(|e| Error::Fetch(format!("{host}: {}", error_text(&e))))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(Error::Fetch(format!(
                "{host}: the server answered {status}"
            )));
        }
        Ok(status.as_u16())
    }

    async fn open(&self, url: &str, accept: Option<&str>) -> Result<reqwest::Response> {
        let url = self.check(url)?;
        let host = host_for_message(&url);
        let mut req = self.client.get(url);
        if let Some(a) = accept {
            req = req.header(reqwest::header::ACCEPT, a);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| Error::Fetch(format!("{host}: {}", error_text(&e))))?;
        if !resp.status().is_success() {
            return Err(Error::Fetch(format!(
                "{host}: the server answered {}",
                resp.status()
            )));
        }
        Ok(resp)
    }

    /// Fetch a small document (release metadata, a checksum list, the store catalog).
    pub async fn get_bytes(
        &self,
        url: &str,
        max: usize,
        timeout: Duration,
        accept: Option<&str>,
    ) -> Result<Vec<u8>> {
        let work = async {
            let resp = self.open(url, accept).await?;
            if resp.content_length().is_some_and(|n| n > max as u64) {
                return Err(Error::Fetch(format!(
                    "the answer is larger than {max} bytes"
                )));
            }
            let mut body = Vec::new();
            let mut stream = resp.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|e| Error::Fetch(error_text(&e)))?;
                if body.len() + chunk.len() > max {
                    return Err(Error::Fetch(format!(
                        "the answer is larger than {max} bytes"
                    )));
                }
                body.extend_from_slice(&chunk);
            }
            Ok(body)
        };
        tokio::time::timeout(timeout, work)
            .await
            .map_err(|_| Error::Fetch("the server took too long".into()))?
    }

    /// Download a file to `dest`, never more than `max` bytes, and return its SHA-256 (lowercase
    /// hexadecimal) computed on the way. `progress(downloaded, total)` is called now and then.
    pub async fn download(
        &self,
        url: &str,
        dest: &Path,
        max: u64,
        progress: &(dyn Fn(u64, Option<u64>) + Send + Sync),
    ) -> Result<String> {
        let work = async {
            let resp = self.open(url, None).await?;
            let total = resp.content_length();
            if total.is_some_and(|n| n > max) {
                return Err(Error::Fetch(format!(
                    "the file is larger than expected ({} MiB at most)",
                    max / (1024 * 1024)
                )));
            }
            let mut file = tokio::fs::File::create(dest).await?;
            let mut hash = Sha256::new();
            let mut got = 0u64;
            let mut last = Instant::now();
            progress(0, total);
            let mut stream = resp.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|e| Error::Fetch(error_text(&e)))?;
                got += chunk.len() as u64;
                if got > max {
                    return Err(Error::Fetch(format!(
                        "the file is larger than expected ({} MiB at most)",
                        max / (1024 * 1024)
                    )));
                }
                hash.update(&chunk);
                file.write_all(&chunk).await?;
                if last.elapsed() >= PROGRESS_EVERY {
                    last = Instant::now();
                    progress(got, total);
                }
            }
            file.flush().await?;
            if total.is_some_and(|n| n != got) {
                return Err(Error::Fetch("the download ended early".into()));
            }
            progress(got, total.or(Some(got)));
            Ok(crate::install::hex(&hash.finalize()))
        };
        tokio::time::timeout(DOWNLOAD_TIMEOUT, work)
            .await
            .map_err(|_| Error::Fetch("the download took too long".into()))?
    }
}

fn host_for_message(url: &Url) -> String {
    url.host_str().unwrap_or("the server").to_owned()
}

fn error_text(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "the server took too long".into()
    } else if e.is_connect() {
        "cannot connect".into()
    } else if e.is_redirect() {
        "refused a redirect (too many, or to a blocked address)".into()
    } else {
        // reqwest's own text can carry the address; the source chain is the useful part.
        std::error::Error::source(e)
            .map_or_else(|| "the transfer failed".into(), ToString::to_string)
    }
}
