//! File name sanitization: no path traversal, no control or reserved characters.

const MAX_LEN: usize = 255;
const FALLBACK: &str = "download";

/// Turn an untrusted name (URL segment, `Content-Disposition`) into a safe single
/// path component. Never returns an empty string, `.`, `..` or anything with a separator.
pub fn sanitize(name: &str) -> String {
    // Keep only the last component, whichever separator style was used.
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = base
        .chars()
        .map(|c| match c {
            c if c.is_control() => '_',
            '<' | '>' | ':' | '"' | '|' | '?' | '*' | '\0' => '_',
            c => c,
        })
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').trim_end_matches('.');
    let cleaned = truncate(cleaned, MAX_LEN);
    if cleaned.is_empty() {
        FALLBACK.to_string()
    } else {
        cleaned
    }
}

/// Cut to `max` bytes on a char boundary, keeping the extension when possible.
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let ext = s
        .rsplit_once('.')
        .map(|(_, e)| e)
        .filter(|e| e.len() <= 16)
        .map(|e| format!(".{e}"))
        .unwrap_or_default();
    let mut end = max - ext.len();
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{ext}", &s[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_name_is_unchanged() {
        assert_eq!(sanitize("film.mkv"), "film.mkv");
    }

    #[test]
    fn strips_path_traversal() {
        assert_eq!(sanitize("../../etc/passwd"), "passwd");
        assert_eq!(sanitize("..\\..\\windows\\evil.exe"), "evil.exe");
        assert_eq!(sanitize("/abs/path/file.txt"), "file.txt");
    }

    #[test]
    fn dots_and_empty_fall_back() {
        assert_eq!(sanitize(""), "download");
        assert_eq!(sanitize(".."), "download");
        assert_eq!(sanitize("."), "download");
        assert_eq!(sanitize("a/"), "download");
    }

    #[test]
    fn replaces_reserved_and_control_chars() {
        assert_eq!(sanitize("a:b*c?.txt"), "a_b_c_.txt");
        assert_eq!(sanitize("a\nb\0c.txt"), "a_b_c.txt");
    }

    #[test]
    fn hidden_file_prefix_is_removed() {
        assert_eq!(sanitize(".bashrc"), "bashrc");
    }

    #[test]
    fn long_names_are_truncated_keeping_extension() {
        let name = format!("{}.mp4", "a".repeat(400));
        let out = sanitize(&name);
        assert!(out.len() <= 255);
        assert!(out.ends_with(".mp4"));
    }

    #[test]
    fn truncation_respects_char_boundaries() {
        let name = format!("{}.zip", "ł".repeat(300));
        let out = sanitize(&name);
        assert!(out.len() <= 255);
        assert!(out.ends_with(".zip"));
    }
}
