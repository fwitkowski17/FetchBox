//! Extension-based category detection.

use crate::model::Category;

const PHOTO: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "bmp", "tif", "tiff", "heic", "heif", "svg", "raw", "cr2",
    "nef", "arw", "dng",
];
const MUSIC: &[&str] = &[
    "mp3", "flac", "wav", "aac", "ogg", "oga", "opus", "m4a", "wma", "aiff", "alac",
];
const VIDEO: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "mpg", "mpeg", "ts", "3gp",
];
const PROGRAM: &[&str] = &[
    "exe", "msi", "dmg", "pkg", "deb", "rpm", "apk", "appimage", "iso", "bat", "sh",
];
const ARCHIVE: &[&str] = &[
    "zip", "rar", "7z", "tar", "gz", "tgz", "bz2", "xz", "zst", "lz", "cab",
];

/// Detect the category from a bare extension (no dot, case-insensitive).
pub fn from_extension(ext: &str) -> Category {
    let ext = ext.to_ascii_lowercase();
    let ext = ext.as_str();
    if PHOTO.contains(&ext) {
        Category::Photo
    } else if MUSIC.contains(&ext) {
        Category::Music
    } else if VIDEO.contains(&ext) {
        Category::Video
    } else if PROGRAM.contains(&ext) {
        Category::Program
    } else if ARCHIVE.contains(&ext) {
        Category::Archive
    } else {
        Category::Other
    }
}

/// Detect the category from a `Content-Type` value (parameters like `; charset=` are ignored).
/// Generic types such as `application/octet-stream` and `text/*` say nothing: `Other`.
pub fn from_content_type(content_type: &str) -> Category {
    let mime = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let Some((kind, subtype)) = mime.split_once('/') else {
        return Category::Other;
    };
    match kind {
        "image" => Category::Photo,
        "audio" => Category::Music,
        "video" => Category::Video,
        _ => match subtype {
            "zip" | "x-zip-compressed" | "gzip" | "x-gzip" | "x-tar" | "x-bzip2" | "x-xz"
            | "x-7z-compressed" | "vnd.rar" | "x-rar-compressed" | "zstd" | "x-zstd" => {
                Category::Archive
            }
            "x-msdownload"
            | "vnd.microsoft.portable-executable"
            | "x-msi"
            | "x-apple-diskimage"
            | "vnd.android.package-archive"
            | "vnd.debian.binary-package"
            | "x-debian-package"
            | "x-rpm"
            | "x-iso9660-image"
            | "x-sh"
            | "x-executable"
            | "x-appimage" => Category::Program,
            _ => Category::Other,
        },
    }
}

/// Best guess for a download: the extension wins (servers often send
/// `application/octet-stream`), the `Content-Type` is the fallback.
pub fn detect(filename: &str, content_type: Option<&str>) -> Category {
    match from_filename(filename) {
        Category::Other => content_type.map_or(Category::Other, from_content_type),
        by_name => by_name,
    }
}

/// Detect the category from a file name. Names without extension are `Other`.
pub fn from_filename(name: &str) -> Category {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => from_extension(ext),
        _ => Category::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_each_category() {
        assert_eq!(from_filename("a.jpg"), Category::Photo);
        assert_eq!(from_filename("a.flac"), Category::Music);
        assert_eq!(from_filename("a.mkv"), Category::Video);
        assert_eq!(from_filename("a.exe"), Category::Program);
        assert_eq!(from_filename("a.zip"), Category::Archive);
        assert_eq!(from_filename("a.xyz"), Category::Other);
    }

    #[test]
    fn extension_is_case_insensitive() {
        assert_eq!(from_filename("PHOTO.JPG"), Category::Photo);
        assert_eq!(from_extension("Mp4"), Category::Video);
    }

    #[test]
    fn uses_last_extension() {
        assert_eq!(from_filename("backup.tar.gz"), Category::Archive);
        assert_eq!(from_filename("movie.zip.mp4"), Category::Video);
    }

    #[test]
    fn no_extension_is_other() {
        assert_eq!(from_filename("README"), Category::Other);
        assert_eq!(from_filename(".bashrc"), Category::Other);
        assert_eq!(from_filename("trailing."), Category::Other);
        assert_eq!(from_filename(""), Category::Other);
    }

    #[test]
    fn content_types() {
        assert_eq!(from_content_type("image/png"), Category::Photo);
        assert_eq!(from_content_type("image/svg+xml"), Category::Photo);
        assert_eq!(from_content_type("audio/mpeg"), Category::Music);
        assert_eq!(from_content_type("video/mp4; codecs=avc1"), Category::Video);
        assert_eq!(from_content_type("Application/ZIP"), Category::Archive);
        assert_eq!(
            from_content_type("application/x-7z-compressed"),
            Category::Archive
        );
        assert_eq!(
            from_content_type("application/vnd.android.package-archive"),
            Category::Program
        );
        assert_eq!(
            from_content_type("application/octet-stream"),
            Category::Other
        );
        assert_eq!(
            from_content_type("text/html; charset=utf-8"),
            Category::Other
        );
        assert_eq!(from_content_type("garbage"), Category::Other);
        assert_eq!(from_content_type(""), Category::Other);
    }

    #[test]
    fn extension_beats_content_type_but_content_type_fills_gaps() {
        assert_eq!(
            detect("a.mkv", Some("application/octet-stream")),
            Category::Video
        );
        assert_eq!(detect("a.mkv", Some("audio/mpeg")), Category::Video);
        assert_eq!(detect("download", Some("image/jpeg")), Category::Photo);
        assert_eq!(
            detect("download", Some("application/octet-stream")),
            Category::Other
        );
        assert_eq!(detect("download", None), Category::Other);
    }
}
