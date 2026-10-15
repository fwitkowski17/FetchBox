//! Directory index pages ("Index of /pub/", as made by Apache, nginx, lighttpd or Python's
//! `http.server`): find out that a URL is one and list the files and folders in it, so the user
//! can choose what to download. Only recognised index pages count; any other HTML is a file.
//!
//! No HTML parser: an index page is a list of anchors, and the scanner below needs nothing else.
//! Only the direct children of the page are listed (a link outside the page's folder, to another
//! host, a sort link or the parent folder is not one).

use fetchbox_core::category::detect as detect_category;
use fetchbox_core::{Category, ListingEntry};
use reqwest::header::CONTENT_TYPE;
use reqwest::{Client, Url};

use crate::error::EngineError;
use crate::probe::percent_decode;

/// How much of a page is read. An index of thousands of files still fits.
const MAX_PAGE_BYTES: usize = 2 * 1024 * 1024;
/// How many items are returned.
const MAX_ENTRIES: usize = 2000;

/// The items of an index page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    /// Directories first, then files, each group in page order.
    pub entries: Vec<ListingEntry>,
    /// There were more items than `MAX_ENTRIES`, or the page was cut off.
    pub truncated: bool,
}

/// Fetch `url`; `Ok(None)` when it is not an index page.
pub async fn fetch(client: &Client, url: &Url) -> Result<Option<Listing>, EngineError> {
    let mut resp = client.get(url.clone()).send().await?;
    if !resp.status().is_success() {
        return Err(EngineError::Status(resp.status().as_u16()));
    }
    let is_html = resp
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            let v = v.to_ascii_lowercase();
            v.starts_with("text/html") || v.starts_with("application/xhtml")
        });
    if !is_html {
        return Ok(None);
    }
    // Links are relative to where the page really is (a redirect from `/dir` to `/dir/`).
    let base = resp.url().clone();
    let mut body = Vec::new();
    let mut cut = false;
    while let Some(chunk) = resp.chunk().await? {
        body.extend_from_slice(&chunk);
        if body.len() >= MAX_PAGE_BYTES {
            body.truncate(MAX_PAGE_BYTES);
            cut = true;
            break;
        }
    }
    let mut listing = parse_index(&base, &String::from_utf8_lossy(&body));
    if let Some(l) = listing.as_mut() {
        l.truncated |= cut;
    }
    Ok(listing)
}

/// The items of `html`, a page fetched from `base`, if it is a directory index.
pub fn parse_index(base: &Url, html: &str) -> Option<Listing> {
    if !looks_like_index(html) {
        return None;
    }
    // An index is a folder: relative links must resolve below it even when the address lacks the
    // closing slash.
    let mut prefix = base.path().to_owned();
    if !prefix.ends_with('/') {
        prefix.push('/');
    }
    let mut base = base.clone();
    base.set_path(&prefix);
    let mut dirs: Vec<ListingEntry> = Vec::new();
    let mut files: Vec<ListingEntry> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut truncated = false;

    for anchor in anchors(html) {
        let Some(href) = anchor.href else { continue };
        let href = decode_entities(href.trim());
        if href.is_empty() || href.starts_with('#') || href.starts_with('?') {
            continue;
        }
        let Ok(mut url) = base.join(&href) else {
            continue;
        };
        url.set_fragment(None);
        if !matches!(url.scheme(), "http" | "https") || url.origin() != base.origin() {
            continue;
        }
        // Only direct children of this folder: this drops the parent folder, sort links
        // (`?C=N;O=D`, the same path) and anything nested deeper.
        let Some(rest) = url.path().strip_prefix(prefix.as_str()) else {
            continue;
        };
        let is_dir = rest.ends_with('/');
        let rest = rest.trim_end_matches('/');
        if rest.is_empty() || rest.contains('/') {
            continue;
        }
        let name = percent_decode(rest);
        if name
            .chars()
            .any(|c| c.is_control() || c == '/' || c == '\\')
        {
            continue;
        }
        if !seen.insert(url.to_string()) {
            continue;
        }
        if seen.len() > MAX_ENTRIES {
            truncated = true;
            break;
        }
        let entry = ListingEntry {
            category: if is_dir {
                Category::Other
            } else {
                detect_category(&name, None)
            },
            name,
            url: url.to_string(),
            size: if is_dir {
                None
            } else {
                size_after(anchor.after)
            },
            is_dir,
        };
        if is_dir { &mut dirs } else { &mut files }.push(entry);
    }
    dirs.extend(files);
    Some(Listing {
        entries: dirs,
        truncated,
    })
}

/// "Index of /x" or "Directory listing for /x" in the title or the first heading.
fn looks_like_index(html: &str) -> bool {
    let head: String = html
        .chars()
        .take(64 * 1024)
        .collect::<String>()
        .to_ascii_lowercase();
    ["<title>", "<h1>"].iter().any(|tag| {
        head.match_indices(tag).any(|(i, _)| {
            let text = head[i + tag.len()..].trim_start();
            text.starts_with("index of") || text.starts_with("directory listing for")
        })
    })
}

struct Anchor<'a> {
    href: Option<&'a str>,
    /// The text after the anchor up to the next anchor or the end of its line / table row: where
    /// nginx and Apache print date and size.
    after: &'a str,
}

/// Every `<a ...>` of the page, in order.
fn anchors(html: &str) -> Vec<Anchor<'_>> {
    let bytes = html.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 2 < bytes.len() {
        let is_open = bytes[i] == b'<'
            && bytes[i + 1].eq_ignore_ascii_case(&b'a')
            && bytes[i + 2].is_ascii_whitespace();
        if !is_open {
            i += 1;
            continue;
        }
        let Some(tag_len) = html[i..].find('>') else {
            break;
        };
        let tag = &html[i + 2..i + tag_len];
        let after_tag = i + tag_len + 1;
        let close = find_ci(&html[after_tag..], "</a>").map_or(html.len(), |p| after_tag + p);
        let tail_start = (close + 4).min(html.len());
        let tail_end = html[tail_start..]
            .find(['\n', '\r'])
            .map(|p| tail_start + p)
            .into_iter()
            .chain(find_ci(&html[tail_start..], "<a ").map(|p| tail_start + p))
            .chain(find_ci(&html[tail_start..], "</tr>").map(|p| tail_start + p))
            .min()
            .unwrap_or(html.len());
        out.push(Anchor {
            href: attribute(tag, "href"),
            after: &html[tail_start..tail_end],
        });
        i = after_tag;
    }
    out
}

fn find_ci(haystack: &str, needle: &str) -> Option<usize> {
    haystack
        .as_bytes()
        .windows(needle.len())
        .position(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
}

/// The value of attribute `name` in the inside of a tag.
fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let mut from = 0;
    while let Some(p) = find_ci(&tag[from..], name) {
        let at = from + p;
        from = at + name.len();
        let before_ok = at == 0 || tag.as_bytes()[at - 1].is_ascii_whitespace();
        let rest = tag[from..].trim_start();
        let Some(rest) = rest.strip_prefix('=') else {
            continue;
        };
        if !before_ok {
            continue;
        }
        let rest = rest.trim_start();
        return Some(match rest.chars().next()? {
            q @ ('"' | '\'') => {
                let inner = &rest[1..];
                &inner[..inner.find(q)?]
            }
            _ => &rest[..rest.find(char::is_whitespace).unwrap_or(rest.len())],
        });
    }
    None
}

fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find('&') {
        out.push_str(&rest[..p]);
        rest = &rest[p..];
        let decoded = rest.find(';').filter(|&e| e <= 8).and_then(|e| {
            let ch = match &rest[1..e] {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                n if n.starts_with("#x") || n.starts_with("#X") => {
                    char::from_u32(u32::from_str_radix(&n[2..], 16).ok()?)?
                }
                n if n.starts_with('#') => char::from_u32(n[1..].parse().ok()?)?,
                _ => return None,
            };
            Some((ch, e + 1))
        });
        match decoded {
            Some((ch, used)) => {
                out.push(ch);
                rest = &rest[used..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The size in the text after an entry: bytes (nginx) or `1.2M` (Apache); `-` means a folder.
fn size_after(text: &str) -> Option<u64> {
    // Strip tags (`<td align="right">1.2M</td>`), keep the text between them.
    let mut plain = String::with_capacity(text.len());
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => {
                in_tag = true;
                plain.push(' ');
            }
            '>' => in_tag = false,
            c if !in_tag => plain.push(c),
            _ => {}
        }
    }
    let plain = decode_entities(&plain);
    plain.split_whitespace().last().and_then(parse_size)
}

fn parse_size(token: &str) -> Option<u64> {
    let (digits, unit) = match token.char_indices().last()? {
        (i, c) if c.is_ascii_alphabetic() => (&token[..i], Some(c.to_ascii_uppercase())),
        _ => (token, None),
    };
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    let factor: f64 = match unit {
        None => 1.0,
        Some('K') => 1024.0,
        Some('M') => 1024.0 * 1024.0,
        Some('G') => 1024.0 * 1024.0 * 1024.0,
        Some('T') => 1024.0_f64.powi(4),
        Some(_) => return None,
    };
    let n: f64 = digits.parse().ok()?;
    Some((n * factor).round() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(path: &str) -> Url {
        Url::parse(&format!("http://files.example:8080{path}")).expect("url")
    }

    fn summary(l: &Listing) -> Vec<(String, Option<u64>, bool)> {
        l.entries
            .iter()
            .map(|e| (e.name.clone(), e.size, e.is_dir))
            .collect()
    }

    const NGINX: &str = r#"<html>
<head><title>Index of /pub/</title></head>
<body>
<h1>Index of /pub/</h1><hr><pre><a href="../">../</a>
<a href="docs/">docs/</a>                                      12-Jan-2024 10:00                   -
<a href="a%20b.iso">a b.iso</a>                                 12-Jan-2024 10:01          4700372992
<a href="notes.txt">notes.txt</a>                               13-Jan-2024 08:30                 120
</pre><hr></body>
</html>"#;

    const APACHE: &str = r#"<!DOCTYPE HTML PUBLIC "-//W3C//DTD HTML 3.2 Final//EN">
<html><head><title>Index of /pub</title></head><body>
<h1>Index of /pub</h1>
<table>
<tr><th><a href="?C=N;O=D">Name</a></th><th><a href="?C=M;O=A">Last modified</a></th><th><a href="?C=S;O=A">Size</a></th></tr>
<tr><td valign="top"><a href="/">Parent Directory</a></td><td>&nbsp;</td><td align="right">  - </td></tr>
<tr><td valign="top"><a href="movie.mkv">movie.mkv</a></td><td align="right">2024-01-12 10:00  </td><td align="right">1.5G</td></tr>
<tr><td valign="top"><a href="sub/">sub/</a></td><td align="right">2024-01-12 10:00  </td><td align="right">  - </td></tr>
<tr><td valign="top"><a href="a&amp;b.zip">a&amp;b.zip</a></td><td align="right">2024-01-12 10:00  </td><td align="right"> 12K</td></tr>
</table></body></html>"#;

    const PYTHON: &str = r#"<!DOCTYPE HTML>
<html lang="en"><head><meta charset="utf-8"><title>Directory listing for /</title></head>
<body><h1>Directory listing for /</h1><hr><ul>
<li><a href="backup.zip">backup.zip</a></li>
<li><a href="holiday.mp4">holiday.mp4</a></li>
<li><a href="folder/">folder/</a></li>
</ul><hr></body></html>"#;

    #[test]
    fn nginx_index_lists_folders_first_with_exact_sizes() {
        let l = parse_index(&base("/pub/"), NGINX).expect("index");
        assert_eq!(
            summary(&l),
            vec![
                ("docs".into(), None, true),
                ("a b.iso".into(), Some(4_700_372_992), false),
                ("notes.txt".into(), Some(120), false),
            ]
        );
        assert_eq!(l.entries[1].url, "http://files.example:8080/pub/a%20b.iso");
        assert!(!l.truncated);
    }

    #[test]
    fn apache_index_skips_sort_and_parent_links_and_reads_human_sizes() {
        let l = parse_index(&base("/pub/"), APACHE).expect("index");
        assert_eq!(
            summary(&l),
            vec![
                ("sub".into(), None, true),
                ("movie.mkv".into(), Some(1_610_612_736), false),
                ("a&b.zip".into(), Some(12 * 1024), false),
            ]
        );
    }

    #[test]
    fn a_page_without_sizes_still_lists_its_files() {
        let l = parse_index(&base("/"), PYTHON).expect("index");
        assert_eq!(
            summary(&l),
            vec![
                ("folder".into(), None, true),
                ("backup.zip".into(), None, false),
                ("holiday.mp4".into(), None, false),
            ]
        );
    }

    #[test]
    fn other_pages_are_not_listings() {
        let page =
            "<html><head><title>My blog</title></head><body><a href=\"a.zip\">a</a></body></html>";
        assert_eq!(parse_index(&base("/"), page), None);
    }

    #[test]
    fn links_to_other_hosts_and_places_and_unsafe_schemes_are_ignored() {
        let page = r#"<title>Index of /pub/</title>
<a href="http://evil.example/pub/x.bin">x</a>
<a href="javascript:alert(1)">y</a>
<a href="/other/z.bin">z</a>
<a href="deeper/file.bin">w</a>
<a href="..%2Fescape">v</a>
<a href="ok.bin">ok</a>
<a href="ok.bin">ok again</a>"#;
        let l = parse_index(&base("/pub/"), page).expect("index");
        assert_eq!(summary(&l), vec![("ok.bin".into(), None, false)]);
    }

    #[test]
    fn a_page_without_trailing_slash_is_read_as_that_folder() {
        let l = parse_index(&base("/pub"), APACHE).expect("index");
        assert_eq!(l.entries.len(), 3);
    }

    #[test]
    fn sizes_are_read_from_the_text_after_the_link() {
        assert_eq!(parse_size("1.5G"), Some(1_610_612_736));
        assert_eq!(parse_size("120"), Some(120));
        assert_eq!(parse_size("-"), None);
        assert_eq!(parse_size("10:00"), None);
        assert_eq!(parse_size("12-Jan-2024"), None);
        assert_eq!(size_after("  12-Jan-2024 10:00    1234"), Some(1234));
    }

    #[test]
    fn entities_in_links_are_decoded() {
        assert_eq!(decode_entities("a&amp;b&#47;c&#x41;&nope;"), "a&b/cA&nope;");
    }

    #[test]
    fn a_huge_listing_is_cut_at_the_limit() {
        let mut page = String::from("<title>Index of /</title>\n");
        for i in 0..MAX_ENTRIES + 10 {
            page.push_str(&format!("<a href=\"f{i}.bin\">f</a>\n"));
        }
        let l = parse_index(&base("/"), &page).expect("index");
        assert_eq!(l.entries.len(), MAX_ENTRIES);
        assert!(l.truncated);
    }
}
