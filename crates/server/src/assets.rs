//! The embedded frontend: caching and compression of the static files.
//!
//! Hashed build output (`assets/*`) never changes under its name, so browsers may keep it for good
//! and a repeat visit loads nothing but `index.html` (revalidated by `ETag`, a `304` with no body).
//! Text files are served precompressed: `npm run build` writes `<file>.br` and `<file>.gz` next
//! to each of them (see `web/vite.config.ts`), so there is no compression work at request time on
//! small hardware. Without those files (a plain dev build) everything is simply served as is.

use axum::{
    http::{header, HeaderMap, HeaderValue, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

/// Frontend build output, embedded at compile time.
#[derive(RustEmbed)]
#[folder = "../../web/dist"]
struct Assets;

/// Content encodings the build precompresses to, best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Encoding {
    Brotli,
    Gzip,
}

impl Encoding {
    fn token(self) -> &'static str {
        match self {
            Encoding::Brotli => "br",
            Encoding::Gzip => "gzip",
        }
    }

    /// Lower is better (smaller files).
    fn rank(self) -> u8 {
        match self {
            Encoding::Brotli => 0,
            Encoding::Gzip => 1,
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Encoding::Brotli => "br",
            Encoding::Gzip => "gz",
        }
    }
}

/// Serve embedded files; unknown non-API paths fall back to `index.html` (SPA routes).
pub async fn handler(uri: Uri, headers: HeaderMap) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path == "api" || path.starts_with("api/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    let path = if path.is_empty() { "index.html" } else { path };
    // The precompressed siblings are an implementation detail, not files to ask for.
    if path.ends_with(".br") || path.ends_with(".gz") {
        return StatusCode::NOT_FOUND.into_response();
    }

    if let Some(resp) = serve(path, &headers) {
        return resp;
    }
    // Missing file with an extension is a real 404; extensionless paths are SPA routes.
    let last = path.rsplit('/').next().unwrap_or(path);
    if !last.contains('.') {
        if let Some(resp) = serve("index.html", &headers) {
            return resp;
        }
    }
    StatusCode::NOT_FOUND.into_response()
}

fn serve(path: &str, request: &HeaderMap) -> Option<Response> {
    let plain = Assets::get(path)?;
    let mime = mime_guess::from_path(path).first_or_octet_stream();

    let accept = request
        .get(header::ACCEPT_ENCODING)
        .and_then(|v| v.to_str().ok());
    // The best precompressed variant the client takes and the build produced.
    let variant = if compressible(path) {
        preferred_encodings(accept).into_iter().find_map(|enc| {
            let file = Assets::get(&format!("{path}.{}", enc.extension()))?;
            Some((enc, file))
        })
    } else {
        None
    };
    let (encoding, file) = match variant {
        Some((enc, file)) => (Some(enc), file),
        None => (None, plain),
    };

    let etag = etag(&file.metadata.sha256_hash(), encoding);
    let mut resp = if if_none_match_hits(
        request
            .get(header::IF_NONE_MATCH)
            .and_then(|v| v.to_str().ok()),
        &etag,
    ) {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        (
            [(header::CONTENT_TYPE, mime.to_string())],
            file.data.into_owned(),
        )
            .into_response()
    };

    let h = resp.headers_mut();
    if let Ok(v) = HeaderValue::from_str(&etag) {
        h.insert(header::ETAG, v);
    }
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(cache_control(path)),
    );
    if compressible(path) {
        h.insert(header::VARY, HeaderValue::from_static("accept-encoding"));
    }
    if let Some(enc) = encoding {
        h.insert(
            header::CONTENT_ENCODING,
            HeaderValue::from_static(enc.token()),
        );
    }
    Some(resp)
}

/// Build output under `assets/` carries a content hash in its name: safe to keep forever. Everything
/// else (the page itself, the manifest, icons) is revalidated on every visit, which costs one
/// request answered with `304` when nothing changed.
fn cache_control(path: &str) -> &'static str {
    if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    }
}

/// Text formats worth compressing; images and fonts (`woff2`, `png`) are compressed already.
fn compressible(path: &str) -> bool {
    matches!(
        path.rsplit('.').next(),
        Some("html" | "js" | "css" | "svg" | "json" | "webmanifest" | "txt")
    )
}

/// A strong validator from the content hash; the encodings are different bytes, so they differ.
fn etag(hash: &[u8; 32], encoding: Option<Encoding>) -> String {
    let hex: String = hash[..12].iter().map(|b| format!("{b:02x}")).collect();
    match encoding {
        None => format!("\"{hex}\""),
        Some(enc) => format!("\"{hex}-{}\"", enc.extension()),
    }
}

/// `If-None-Match`: a list of validators (weak ones compare equal here) or `*`.
fn if_none_match_hits(header: Option<&str>, etag: &str) -> bool {
    let Some(header) = header else { return false };
    header.split(',').map(str::trim).any(|candidate| {
        candidate == "*" || candidate.strip_prefix("W/").unwrap_or(candidate) == etag
    })
}

/// The encodings from `Accept-Encoding` that the build can serve, best first. An encoding with
/// `q=0` is refused; the order follows the quality values, ties go to Brotli.
fn preferred_encodings(accept: Option<&str>) -> Vec<Encoding> {
    let Some(accept) = accept else {
        return Vec::new();
    };
    let mut found: Vec<(f32, Encoding)> = Vec::new();
    let mut star: Option<f32> = None;
    for part in accept.split(',') {
        let mut pieces = part.split(';');
        let name = pieces.next().unwrap_or("").trim().to_ascii_lowercase();
        let q = pieces
            .find_map(|p| p.trim().strip_prefix("q="))
            .and_then(|q| q.trim().parse::<f32>().ok())
            .unwrap_or(1.0);
        match name.as_str() {
            "br" => found.push((q, Encoding::Brotli)),
            "gzip" | "x-gzip" => found.push((q, Encoding::Gzip)),
            "*" => star = Some(q),
            _ => {}
        }
    }
    if let Some(q) = star {
        for enc in [Encoding::Brotli, Encoding::Gzip] {
            if !found.iter().any(|(_, e)| *e == enc) {
                found.push((q, enc));
            }
        }
    }
    found.retain(|(q, _)| *q > 0.0);
    // Quality first; on a tie the smaller one, whatever order the client listed them in.
    found.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.rank().cmp(&b.1.rank())));
    found.into_iter().map(|(_, e)| e).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashed_build_output_is_cached_for_good_the_rest_is_revalidated() {
        assert_eq!(
            cache_control("assets/index-CINTf201.js"),
            "public, max-age=31536000, immutable"
        );
        assert_eq!(cache_control("index.html"), "no-cache");
        assert_eq!(cache_control("manifest.webmanifest"), "no-cache");
        assert_eq!(cache_control("icon.svg"), "no-cache");
    }

    #[test]
    fn only_text_is_compressed() {
        for p in [
            "index.html",
            "assets/a.js",
            "assets/a.css",
            "icon.svg",
            "manifest.webmanifest",
        ] {
            assert!(compressible(p), "{p}");
        }
        for p in ["assets/inter.woff2", "icon-192.png", "noextension"] {
            assert!(!compressible(p), "{p}");
        }
    }

    #[test]
    fn the_client_picks_the_encoding() {
        use Encoding::{Brotli, Gzip};
        assert_eq!(preferred_encodings(None), vec![]);
        assert_eq!(preferred_encodings(Some("")), vec![]);
        assert_eq!(
            preferred_encodings(Some("gzip, deflate, br, zstd")),
            vec![Brotli, Gzip]
        );
        assert_eq!(preferred_encodings(Some("gzip")), vec![Gzip]);
        assert_eq!(
            preferred_encodings(Some("br;q=0.5, gzip;q=0.9")),
            vec![Gzip, Brotli]
        );
        assert_eq!(preferred_encodings(Some("br;q=0, gzip")), vec![Gzip]);
        assert_eq!(preferred_encodings(Some("*")), vec![Brotli, Gzip]);
        assert_eq!(preferred_encodings(Some("identity")), vec![]);
        assert_eq!(preferred_encodings(Some("GZIP")), vec![Gzip]);
    }

    #[test]
    fn validators_match_like_http_says() {
        let tag = "\"abc\"";
        assert!(!if_none_match_hits(None, tag));
        assert!(if_none_match_hits(Some("\"abc\""), tag));
        assert!(if_none_match_hits(Some("W/\"abc\""), tag));
        assert!(if_none_match_hits(Some("\"x\", \"abc\""), tag));
        assert!(if_none_match_hits(Some("*"), tag));
        assert!(!if_none_match_hits(Some("\"abd\""), tag));
    }

    #[test]
    fn each_encoding_has_its_own_validator() {
        let hash = [7u8; 32];
        let plain = etag(&hash, None);
        assert_ne!(plain, etag(&hash, Some(Encoding::Brotli)));
        assert_ne!(
            etag(&hash, Some(Encoding::Gzip)),
            etag(&hash, Some(Encoding::Brotli))
        );
        assert!(plain.starts_with('"') && plain.ends_with('"'));
    }

    fn get(path: &str, headers: &[(&str, &str)]) -> axum::http::Response<axum::body::Body> {
        let mut map = HeaderMap::new();
        for (k, v) in headers {
            map.insert(
                axum::http::HeaderName::from_bytes(k.as_bytes()).expect("name"),
                HeaderValue::from_str(v).expect("value"),
            );
        }
        let uri: Uri = path.parse().expect("uri");
        futures_util::FutureExt::now_or_never(handler(uri, map)).expect("no await inside")
    }

    #[test]
    fn the_page_is_served_revalidated_and_a_matching_validator_gets_304() {
        if Assets::get("index.html").is_none() {
            return; // frontend not built: nothing to serve
        }
        let first = get("/", &[]);
        assert_eq!(first.status(), 200);
        assert_eq!(first.headers()[header::CACHE_CONTROL], "no-cache");
        let tag = first.headers()[header::ETAG]
            .to_str()
            .expect("etag")
            .to_owned();

        let again = get("/", &[("if-none-match", &tag)]);
        assert_eq!(again.status(), 304);
        assert_eq!(again.headers()[header::ETAG].to_str().expect("etag"), tag);

        // SPA routes fall back to the same page; API paths and the precompressed files are not served.
        assert_eq!(get("/settings", &[]).status(), 200);
        assert_eq!(get("/api/nothing", &[]).status(), 404);
        assert_eq!(get("/index.html.br", &[]).status(), 404);
        assert_eq!(get("/missing.js", &[]).status(), 404);
    }

    #[test]
    fn built_assets_are_immutable_and_precompressed_when_the_build_made_them() {
        let Some(js) = Assets::iter().find(|f| f.starts_with("assets/") && f.ends_with(".js"))
        else {
            return;
        };
        let url = format!("/{js}");
        let plain = get(&url, &[]);
        assert_eq!(plain.status(), 200);
        assert_eq!(
            plain.headers()[header::CACHE_CONTROL],
            "public, max-age=31536000, immutable"
        );
        assert!(plain.headers().get(header::CONTENT_ENCODING).is_none());

        if Assets::get(&format!("{js}.br")).is_some() {
            let br = get(&url, &[("accept-encoding", "gzip, br")]);
            assert_eq!(br.headers()[header::CONTENT_ENCODING], "br");
            assert_eq!(br.headers()[header::VARY], "accept-encoding");
            assert_eq!(br.headers()[header::CONTENT_TYPE], "text/javascript");
            assert_ne!(br.headers()[header::ETAG], plain.headers()[header::ETAG]);
        }
    }
}
