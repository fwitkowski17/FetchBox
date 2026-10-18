//! Disks (SPEC 6.14): the file systems new downloads can be saved on.
//!
//! A user who has two disks mounts them inside the download directory
//! (`/downloads/hdd10tb`, `/downloads/ssd1`). Each such folder is a *disk*; its name is how the
//! settings, the add request and the stored downloads refer to it. The empty name is the download
//! directory itself. Nothing here touches the file system: finding the mount points is the
//! engine's job (`fetchbox_http_engine::disk`).

use serde::{Deserialize, Serialize};

use crate::model::Category;

/// The download directory itself.
pub const MAIN: &str = "";

/// Longest disk name accepted (a single file name component).
const MAX_NAME: usize = 255;

/// Folder names inside the download directory that mean something else, so they are never offered
/// as disks even when they are mount points: the category folders (6.3.1; one of them may sit on
/// another disk of its own) and the folder of the torrents (6.13).
pub fn is_reserved(name: &str) -> bool {
    name == "torrents"
        || [
            Category::Photo,
            Category::Music,
            Category::Video,
            Category::Program,
            Category::Archive,
            Category::Other,
        ]
        .iter()
        .any(|c| c.as_str() == name)
}

/// Is this usable as the name of a disk? The empty name (the download directory) is. Otherwise a
/// single, plain folder name: no separators, no `.` or `..`, nothing hidden, no control characters.
pub fn is_valid_name(name: &str) -> bool {
    if name.is_empty() {
        return true;
    }
    name.len() <= MAX_NAME
        && !name.starts_with('.')
        && !name.contains(['/', '\\'])
        && !name.chars().any(char::is_control)
        && name.trim() == name
}

/// `disk` and `subdir` as one path relative to the download directory (`ssd1/video`). Either may
/// be empty. This is what the download engine takes as its sub-directory.
pub fn rel_dir(disk: &str, subdir: &str) -> String {
    match (disk.is_empty(), subdir.is_empty()) {
        (true, _) => subdir.to_owned(),
        (false, true) => disk.to_owned(),
        (false, false) => format!("{disk}/{subdir}"),
    }
}

/// One place downloads can be saved to, with its space.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DiskInfo {
    /// Folder name inside the download directory; empty for the download directory itself.
    pub name: String,
    /// Where it is on the machine running FetchBox (shown, never taken from a client).
    pub path: String,
    /// Bytes this program can use on the file system (used plus available).
    pub total: u64,
    /// Bytes available for new files.
    pub available: u64,
    /// What the running downloads on this file system still have to write.
    pub reserved: u64,
}

/// The disks FetchBox found and which one new downloads go to by default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Disks {
    /// The download directory first, then the mounted disks by name.
    pub disks: Vec<DiskInfo>,
    /// The disk new downloads go to unless the request names another (`Settings::default_disk`).
    pub default: String,
    /// The default is the download directory or a disk that is mounted now.
    pub default_available: bool,
    /// More than the download directory itself can be chosen: the UI offers a choice only then.
    pub choosable: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_plain_folder_names() {
        assert!(is_valid_name(""));
        assert!(is_valid_name("hdd10tb"));
        assert!(is_valid_name("My Disk 2"));
        assert!(!is_valid_name("."));
        assert!(!is_valid_name(".."));
        assert!(!is_valid_name(".hidden"));
        assert!(!is_valid_name("a/b"));
        assert!(!is_valid_name("a\\b"));
        assert!(!is_valid_name("/etc"));
        assert!(!is_valid_name(" padded "));
        assert!(!is_valid_name("tab\tname"));
        assert!(!is_valid_name(&"x".repeat(256)));
    }

    #[test]
    fn category_and_torrent_folders_are_not_disks() {
        for name in [
            "torrents", "video", "photo", "music", "program", "archive", "other",
        ] {
            assert!(is_reserved(name), "{name}");
        }
        assert!(!is_reserved("hdd10tb"));
        assert!(!is_reserved("Video"), "names are compared as they are");
    }

    #[test]
    fn a_disk_and_a_category_folder_make_one_relative_path() {
        assert_eq!(rel_dir("", ""), "");
        assert_eq!(rel_dir("", "video"), "video");
        assert_eq!(rel_dir("ssd1", ""), "ssd1");
        assert_eq!(rel_dir("ssd1", "video"), "ssd1/video");
    }
}
