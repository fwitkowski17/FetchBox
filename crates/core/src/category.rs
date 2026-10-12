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
}
