//! Small pure helpers: names, platforms, address globs and translations.

use std::collections::BTreeMap;

/// The platforms a manifest may name.
pub const PLATFORMS: [&str; 5] = [
    "linux-x86_64",
    "linux-aarch64",
    "macos-aarch64",
    "macos-x86_64",
    "windows-x86_64",
];

/// The platform this build runs on, in the manifest's spelling (`linux-x86_64`).
pub fn platform_key() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

/// A plugin or dependency id: 2 to 32 characters, lowercase letters, digits and `-`, starting with a
/// letter and not ending in `-`. It becomes a folder name, so nothing else is allowed.
pub fn is_valid_id(id: &str) -> bool {
    let b = id.as_bytes();
    (2..=32).contains(&b.len())
        && b[0].is_ascii_lowercase()
        && b[b.len() - 1] != b'-'
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-')
}

/// The key of a setting, a dialog field, an action, a dialog or a choice: 1 to 32 characters,
/// lowercase letters, digits, `_` and `-`, starting with a letter.
pub fn is_valid_key(key: &str) -> bool {
    let b = key.as_bytes();
    (1..=32).contains(&b.len())
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_' || *c == b'-')
}

/// A path inside a folder: relative, no `..`, no drive letter, backslash or whitespace.
pub fn is_safe_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains(['\\', ':', '\0'])
        && path.split('/').all(|c| c != "..")
        && !path.chars().any(|c| c.is_control() || c.is_whitespace())
}

/// Does `text` match `pattern`? `*` stands for any run of characters (also `/`), `?` for one
/// character; everything else matches itself, ignoring ASCII case.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().map(|c| c.to_ascii_lowercase()).collect();
    let t: Vec<char> = text.chars().map(|c| c.to_ascii_lowercase()).collect();
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// The host part of an `http://` or `https://` address, lowercase, without user info or port.
pub fn host_of(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit('@').next()?;
    let host = if let Some(v6) = authority.strip_prefix('[') {
        v6.split(']').next()?
    } else {
        authority.split(':').next()?
    };
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// A plugin's own text in the language of the interface: `i18n[locale][text]`, then the same with
/// the bare language (`pl` for `pl-PL`), else the text itself (the English original).
pub fn tr<'a>(
    i18n: &'a BTreeMap<String, BTreeMap<String, String>>,
    locale: &str,
    text: &'a str,
) -> &'a str {
    let lang = locale.split(['-', '_']).next().unwrap_or(locale);
    [locale, lang]
        .into_iter()
        .filter_map(|l| i18n.get(l))
        .find_map(|m| m.get(text))
        .map_or(text, String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_plain_folder_names() {
        for ok in ["yt-dlp", "ab", "webhook-notify", "a1", "x-y-z"] {
            assert!(is_valid_id(ok), "{ok}");
        }
        for bad in [
            "",
            "a",
            "Yt-dlp",
            "1abc",
            "-ab",
            "ab-",
            "a b",
            "a/b",
            "a.b",
            "..",
            "a_b",
            "ąb",
            &"a".repeat(33),
        ] {
            assert!(!is_valid_id(bad), "{bad}");
        }
    }

    #[test]
    fn relative_paths_stay_inside_their_folder() {
        for ok in ["bin/tool", "tool", "a/b/c.sh", "./x"] {
            assert!(is_safe_relative(ok), "{ok}");
        }
        for bad in [
            "", "/abs", "../x", "a/../b", "a\\b", "C:x", "a b", "a\0b", "a/\n",
        ] {
            assert!(!is_safe_relative(bad), "{bad:?}");
        }
    }

    #[test]
    fn keys_allow_underscores() {
        assert!(is_valid_key("cookies_file"));
        assert!(is_valid_key("a"));
        assert!(!is_valid_key("Cookies"));
        assert!(!is_valid_key("_a"));
        assert!(!is_valid_key("a b"));
        assert!(!is_valid_key(""));
    }

    #[test]
    fn globs_match_the_whole_address() {
        assert!(glob_match(
            "https://*.youtube.com/*",
            "https://www.youtube.com/watch?v=abc"
        ));
        assert!(glob_match("https://youtu.be/*", "HTTPS://YOUTU.BE/abc"));
        assert!(glob_match("*", ""));
        assert!(glob_match("a?c", "abc"));
        assert!(!glob_match("a?c", "ac"));
        assert!(!glob_match(
            "https://*.youtube.com/*",
            "https://youtube.com/x"
        ));
        assert!(!glob_match("https://youtu.be/*", "https://youtu.be"));
        assert!(glob_match("a*b*c", "aXXbYYc"));
        assert!(!glob_match("a*b*c", "aXXbYY"));
    }

    #[test]
    fn the_host_is_read_from_the_address() {
        assert_eq!(
            host_of("https://Example.com/a?b").as_deref(),
            Some("example.com")
        );
        assert_eq!(host_of("http://u:p@host:8080/x").as_deref(), Some("host"));
        assert_eq!(host_of("https://[::1]:80/").as_deref(), Some("::1"));
        assert_eq!(host_of("ftp://host/"), None);
        assert_eq!(host_of("https:///x"), None);
    }

    #[test]
    fn texts_fall_back_from_the_locale_to_the_language_to_the_original() {
        let mut pl = BTreeMap::new();
        pl.insert("Update".to_owned(), "Zaktualizuj".to_owned());
        let mut pl_pl = BTreeMap::new();
        pl_pl.insert("Update".to_owned(), "Zaktualizuj (PL)".to_owned());
        let mut i18n = BTreeMap::new();
        i18n.insert("pl".to_owned(), pl);
        assert_eq!(tr(&i18n, "pl-PL", "Update"), "Zaktualizuj");
        assert_eq!(tr(&i18n, "pl", "Update"), "Zaktualizuj");
        assert_eq!(tr(&i18n, "pl", "Other"), "Other");
        assert_eq!(tr(&i18n, "de", "Update"), "Update");
        i18n.insert("pl-PL".to_owned(), pl_pl);
        assert_eq!(tr(&i18n, "pl-PL", "Update"), "Zaktualizuj (PL)");
    }
}
