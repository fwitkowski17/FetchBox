//! Learn what the server offers before downloading: size, range support, `ETag`, name.

use fetchbox_core::filename::sanitize;
use reqwest::header::{HeaderMap, CONTENT_DISPOSITION, CONTENT_RANGE, CONTENT_TYPE, ETAG, RANGE};
use reqwest::{Client, StatusCode, Url};

use crate::error::EngineError;

/// What a probe learned about the remote file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteInfo {
    /// Total size in bytes, if the server told us.
    pub size: Option<u64>,
    /// Server honors `Range` (needs a known size for segmenting).
    pub resumable: bool,
    pub etag: Option<String>,
    /// Sanitized suggestion from `Content-Disposition` or the URL path.
    pub filename: String,
    pub content_type: Option<String>,
}

/// Accept only `http(s)` URLs with a host.
pub fn validate_url(raw: &str) -> Result<Url, EngineError> {
    let url = Url::parse(raw.trim()).map_err(|e| EngineError::InvalidUrl(e.to_string()))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(EngineError::InvalidUrl(format!(
            "unsupported scheme {:?}",
            url.scheme()
        )));
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(EngineError::InvalidUrl("missing host".into()));
    }
    Ok(url)
}

/// Probe with a one-byte ranged GET. `206` means ranges work; `200` means they do not.
/// (Some servers reject `HEAD`, a tiny ranged GET works everywhere.)
pub async fn probe(client: &Client, url: &Url) -> Result<RemoteInfo, EngineError> {
    let resp = client
        .get(url.clone())
        .header(RANGE, "bytes=0-0")
        .send()
        .await?;
    let status = resp.status();
    let headers = resp.headers();

    let (size, resumable) = match status {
        StatusCode::PARTIAL_CONTENT => {
            let total = headers
                .get(CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(parse_content_range_total);
            (total, total.is_some())
        }
        // Range not satisfiable: an empty file (`bytes */0`).
        StatusCode::RANGE_NOT_SATISFIABLE => {
            let total = headers
                .get(CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(parse_content_range_total);
            (total, false)
        }
        s if s.is_success() => (resp.content_length(), false),
        s => return Err(EngineError::Status(s.as_u16())),
    };

    Ok(RemoteInfo {
        size,
        resumable,
        etag: header_str(headers, ETAG.as_str()),
        filename: suggested_filename(headers, url),
        content_type: header_str(headers, CONTENT_TYPE.as_str()),
    })
}

fn header_str(headers: &HeaderMap, name: &str) -> Option<String> {
    headers.get(name)?.to_str().ok().map(str::to_owned)
}

/// `bytes 0-0/12345` or `bytes */12345` -> `12345`; `bytes 0-0/*` -> `None`.
fn parse_content_range_total(v: &str) -> Option<u64> {
    v.rsplit_once('/')?.1.trim().parse().ok()
}

fn suggested_filename(headers: &HeaderMap, url: &Url) -> String {
    let from_header = headers
        .get(CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
        .and_then(content_disposition_filename);
    let name = from_header.unwrap_or_else(|| {
        url.path_segments()
            .and_then(|mut s| s.next_back())
            .map(percent_decode)
            .unwrap_or_default()
    });
    sanitize(&name)
}

/// Prefer RFC 5987 `filename*=UTF-8''...`, fall back to plain `filename=`.
fn content_disposition_filename(v: &str) -> Option<String> {
    let mut plain = None;
    let mut extended = None;
    for part in v.split(';').skip(1) {
        let Some((key, val)) = part.trim().split_once('=') else {
            continue;
        };
        let val = val.trim().trim_matches('"');
        match key.trim().to_ascii_lowercase().as_str() {
            "filename*" => extended = val.splitn(3, '\'').nth(2).map(percent_decode),
            "filename" => plain = Some(val.to_owned()),
            _ => {}
        }
    }
    extended.or(plain)
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(h << 4 | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: u8) -> Option<u8> {
    (b as char).to_digit(16).map(|d| d as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_validation() {
        assert!(validate_url("https://example.com/a.zip").is_ok());
        assert!(validate_url("  http://example.com/a  ").is_ok());
        assert!(validate_url("ftp://example.com/a").is_err());
        assert!(validate_url("file:///etc/passwd").is_err());
        assert!(validate_url("javascript:alert(1)").is_err());
        assert!(validate_url("not a url").is_err());
    }

    #[test]
    fn content_range_total() {
        assert_eq!(parse_content_range_total("bytes 0-0/12345"), Some(12345));
        assert_eq!(parse_content_range_total("bytes */0"), Some(0));
        assert_eq!(parse_content_range_total("bytes 0-0/*"), None);
        assert_eq!(parse_content_range_total("garbage"), None);
    }

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("a%20b.txt"), "a b.txt");
        assert_eq!(percent_decode("%C5%82%C3%B3d%C5%BA.txt"), "łódź.txt");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }

    #[test]
    fn disposition_parsing() {
        assert_eq!(
            content_disposition_filename("attachment; filename=\"a b.zip\""),
            Some("a b.zip".into())
        );
        assert_eq!(
            content_disposition_filename(
                "attachment; filename=\"x.zip\"; filename*=UTF-8''%C5%82.zip"
            ),
            Some("ł.zip".into())
        );
        assert_eq!(content_disposition_filename("inline"), None);
    }

    #[test]
    fn filename_falls_back_to_url_and_is_sanitized() {
        let url = Url::parse("http://h/dir/My%20File.mkv?x=1").expect("url");
        assert_eq!(suggested_filename(&HeaderMap::new(), &url), "My File.mkv");
        let url = Url::parse("http://h/").expect("url");
        assert_eq!(suggested_filename(&HeaderMap::new(), &url), "download");
        let mut h = HeaderMap::new();
        h.insert(
            CONTENT_DISPOSITION,
            "attachment; filename=\"../../evil.sh\""
                .parse()
                .expect("hv"),
        );
        let url = Url::parse("http://h/x").expect("url");
        assert_eq!(suggested_filename(&h, &url), "evil.sh");
    }
}
