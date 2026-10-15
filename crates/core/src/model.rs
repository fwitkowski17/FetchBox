//! Domain models. Placeholders for milestone 0; fields follow SPEC 6.4.

use serde::{Deserialize, Serialize};

/// Download identifier.
pub type Id = i64;

/// Lifecycle status of a download.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Queued,
    Downloading,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

impl Status {
    /// Stable lowercase name, as stored in SQLite.
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Queued => "queued",
            Status::Downloading => "downloading",
            Status::Paused => "paused",
            Status::Completed => "completed",
            Status::Failed => "failed",
            Status::Cancelled => "cancelled",
        }
    }

    /// Parse the stored name.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "queued" => Status::Queued,
            "downloading" => Status::Downloading,
            "paused" => Status::Paused,
            "completed" => Status::Completed,
            "failed" => Status::Failed,
            "cancelled" => Status::Cancelled,
            _ => return None,
        })
    }

    /// No further transitions happen without user action.
    pub fn is_terminal(self) -> bool {
        matches!(self, Status::Completed | Status::Failed | Status::Cancelled)
    }
}

/// File category, auto-detected from extension and changeable by the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Photo,
    Music,
    Video,
    Program,
    Archive,
    Other,
}

impl Category {
    /// Stable lowercase name, as stored in SQLite.
    pub fn as_str(self) -> &'static str {
        match self {
            Category::Photo => "photo",
            Category::Music => "music",
            Category::Video => "video",
            Category::Program => "program",
            Category::Archive => "archive",
            Category::Other => "other",
        }
    }

    /// Parse the stored name.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "photo" => Category::Photo,
            "music" => Category::Music,
            "video" => Category::Video,
            "program" => Category::Program,
            "archive" => Category::Archive,
            "other" => Category::Other,
            _ => return None,
        })
    }
}

/// What kind of source a download comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Http,
    Magnet,
    TorrentFile,
}

impl SourceKind {
    /// Stable name, as stored in SQLite.
    pub fn as_str(self) -> &'static str {
        match self {
            SourceKind::Http => "http",
            SourceKind::Magnet => "magnet",
            SourceKind::TorrentFile => "torrent_file",
        }
    }

    /// Parse the stored name.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "http" => SourceKind::Http,
            "magnet" => SourceKind::Magnet,
            "torrent_file" => SourceKind::TorrentFile,
            _ => return None,
        })
    }
}

/// A download as stored and shown to the user.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Download {
    #[cfg_attr(feature = "openapi", schema(value_type = i64))]
    pub id: Id,
    pub url: String,
    pub filename: String,
    pub category: Category,
    pub status: Status,
    /// Total size in bytes, if known.
    pub size: Option<u64>,
    pub downloaded: u64,
    pub source_kind: SourceKind,
    pub resumable: bool,
    pub etag: Option<String>,
    pub error: Option<String>,
    /// Subdirectory of the download directory the file goes to; empty = the directory itself.
    #[serde(default)]
    pub subdir: String,
    /// Current speed in bytes/s. Not persisted: filled in for running downloads.
    #[serde(default)]
    pub speed: u64,
    /// Held back because the file does not fit on the disk: it was added on purpose but not
    /// started (status `paused`). Free some space and resume it.
    #[serde(default)]
    pub waiting_space: bool,
    /// Hidden from the main list (only finished, failed or cancelled downloads can be archived).
    /// The file, if any, stays where it is.
    #[serde(default)]
    pub archived: bool,
    /// Speed limit of this download alone, bytes/s. Set, it replaces the default per-download
    /// limit of the settings (so it may be higher or lower); `0` means "use the default". The
    /// total limit of all downloads (`Settings::speed_global`) caps the sum either way.
    #[serde(default)]
    pub speed_limit: u64,
}

/// Free space on the file system that holds the download directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DiskSpace {
    /// Bytes this program can use on the file system: what is used plus what is available
    /// (blocks reserved for root are not counted, like `df`'s `Use%`).
    pub total: u64,
    /// Bytes available for new files.
    pub available: u64,
    /// Bytes reserved by running downloads: what they still have to write on this file system
    /// (size known). Queued, paused and held downloads reserve nothing. New downloads must fit into
    /// `available - reserved`; when `reserved` exceeds `available` (something else filled the disk)
    /// the queue will not fit.
    pub reserved: u64,
}

impl DiskSpace {
    /// Bytes in use.
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.available)
    }
}

/// Global settings, applied live (SPEC 2, 6.3). Zero speed limit means unlimited.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Settings {
    /// Connections (segments) per download.
    pub connections: u32,
    /// Max simultaneously running downloads; extra ones are `Queued`.
    pub max_concurrent: u32,
    /// Default speed limit of every download, bytes/s. A download with a limit of its own
    /// (`Download::speed_limit`) ignores it.
    pub speed_per_download: u64,
    /// Total speed limit of all downloads together, bytes/s.
    pub speed_global: u64,
    /// Put new downloads into a subdirectory named after their category (`video/`, `music/`, ...).
    /// Existing downloads keep the directory they were created with.
    #[serde(default)]
    pub category_subdirs: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            connections: 4,
            max_concurrent: 3,
            speed_per_download: 0,
            speed_global: 0,
            category_subdirs: false,
        }
    }
}

/// Upper bound for connections per download; more gains nothing and hurts servers.
pub const MAX_CONNECTIONS: u32 = 32;

impl Settings {
    /// Reject values that would break the scheduler or the engine.
    pub fn validate(&self) -> crate::Result<()> {
        if !(1..=MAX_CONNECTIONS).contains(&self.connections) {
            return Err(crate::Error::InvalidInput(format!(
                "connections must be 1..={MAX_CONNECTIONS}"
            )));
        }
        if self.max_concurrent == 0 {
            return Err(crate::Error::InvalidInput(
                "max_concurrent must be at least 1".into(),
            ));
        }
        Ok(())
    }
}

/// What to do when a new download collides with an existing one (SPEC: name conflicts).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum OnConflict {
    /// Refuse with a conflict description (default; nothing happens behind the user's back).
    #[default]
    Reject,
    /// Use the first free name.
    Rename,
    /// Replace the existing file once the download finishes.
    Overwrite,
}

/// Request to add a download by URL.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AddRequest {
    pub url: String,
    /// Override the file name suggested by the server.
    #[serde(default)]
    pub filename: Option<String>,
    /// Override the category detected from the file name.
    #[serde(default)]
    pub category: Option<Category>,
    #[serde(default)]
    pub on_conflict: OnConflict,
    /// Add the download even though it does not fit into the free space left after the space
    /// reserved by running downloads (`507` otherwise). The user's call: files may be moved away
    /// or deleted before it starts. If it still does not fit when it starts, it fails then.
    #[serde(default)]
    pub ignore_space: bool,
}

/// Request to look at a URL before adding it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ProbeRequest {
    pub url: String,
}

/// What a URL turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum UrlKind {
    /// A file (or anything that is not recognised as a listing): add it as it is.
    File,
    /// A directory index page; `entries` lists what is in it, to choose from.
    Listing,
}

/// One item of a directory index page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ListingEntry {
    /// Name as shown on the page (decoded), without a trailing slash.
    pub name: String,
    /// Absolute address; a file is added with it, a directory is opened with it.
    pub url: String,
    /// Size in bytes when the page shows one.
    pub size: Option<u64>,
    pub is_dir: bool,
}

/// What the remote server says about a URL: what an add request would work with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct UrlInfo {
    pub kind: UrlKind,
    /// The (sanitized) file name the download would get.
    pub filename: String,
    /// Size in bytes; `None` when the server does not tell.
    pub size: Option<u64>,
    /// The server honours `Range`, so the download can be paused and resumed.
    pub resumable: bool,
    /// Category the download would get when none is chosen.
    pub category: Category,
    /// For a listing: the directories first, then the files, each group in page order. Empty for a file.
    pub entries: Vec<ListingEntry>,
    /// The page had more items than are returned.
    pub truncated: bool,
}

/// Why an add request collided with something.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum ConflictKind {
    /// The same URL is already queued, running, paused or completed.
    DuplicateUrl,
    /// A file with this name is already in the download directory.
    FileExists,
    /// Another unfinished download will produce a file with this name.
    NameReserved,
}

/// Description of a collision, shown to the user so they can choose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Conflict {
    pub kind: ConflictKind,
    pub existing_name: String,
    pub existing_size: Option<u64>,
    /// Unix seconds, for files already on disk.
    pub existing_modified: Option<i64>,
    /// The existing download, when the collision is with one.
    #[cfg_attr(feature = "openapi", schema(value_type = Option<i64>))]
    pub existing_id: Option<Id>,
    /// First free name, used by `OnConflict::Rename`.
    pub suggested_name: String,
}

/// Live updates pushed to the UI (SSE).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// A download was created or its status/metadata changed.
    Download { download: Download },
    /// Periodic progress of a running download.
    Progress {
        #[cfg_attr(feature = "openapi", schema(value_type = i64))]
        id: Id,
        downloaded: u64,
        total: Option<u64>,
        speed: u64,
    },
    /// A download was removed from the list.
    Removed {
        #[cfg_attr(feature = "openapi", schema(value_type = i64))]
        id: Id,
    },
    /// Settings changed.
    Settings { settings: Settings },
}

/// One byte range `[start, end]` (inclusive) of a file and how much of it is on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentState {
    pub idx: u32,
    pub start: u64,
    pub end: u64,
    pub downloaded: u64,
}

impl SegmentState {
    /// Length of the range in bytes.
    pub fn len(&self) -> u64 {
        self.end - self.start + 1
    }

    /// Ranges are never empty (`end >= start`).
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Whole range is on disk.
    pub fn is_done(&self) -> bool {
        self.downloaded >= self.len()
    }
}

/// Input for `Downloader::start`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub id: Id,
    pub url: String,
    pub filename: String,
    /// Subdirectory of the download directory (empty = none). Must be a single safe name.
    pub subdir: String,
    pub connections: u32,
    /// Per-download speed limit, bytes/s (`0` = unlimited).
    pub speed_limit: u64,
    /// `ETag` seen when the stored offsets were written; used to validate a resume.
    pub etag: Option<String>,
    /// Stored segment offsets to resume from; empty means a fresh start.
    pub segments: Vec<SegmentState>,
    /// The user explicitly chose to replace an existing file with the same name.
    /// When `false` the engine never overwrites: it picks a free name instead.
    pub overwrite: bool,
    /// The user added it although it did not fit: skip the free-space check at start. If the disk
    /// really fills up, the download fails with the write error.
    pub ignore_space: bool,
}

/// Opaque reference to a started job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Handle {
    pub id: Id,
}

/// Progress snapshot of a running download.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Progress {
    pub downloaded: u64,
    pub total: Option<u64>,
    /// Current speed, bytes/s.
    pub speed: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enum_names_round_trip() {
        for s in [
            Status::Queued,
            Status::Downloading,
            Status::Paused,
            Status::Completed,
            Status::Failed,
            Status::Cancelled,
        ] {
            assert_eq!(Status::parse(s.as_str()), Some(s));
        }
        for c in [
            Category::Photo,
            Category::Music,
            Category::Video,
            Category::Program,
            Category::Archive,
            Category::Other,
        ] {
            assert_eq!(Category::parse(c.as_str()), Some(c));
        }
        for k in [
            SourceKind::Http,
            SourceKind::Magnet,
            SourceKind::TorrentFile,
        ] {
            assert_eq!(SourceKind::parse(k.as_str()), Some(k));
        }
        assert_eq!(Status::parse("nope"), None);
    }

    #[test]
    fn settings_validation() {
        assert!(Settings::default().validate().is_ok());
        let bad = Settings {
            connections: 0,
            ..Settings::default()
        };
        assert!(bad.validate().is_err());
        let bad = Settings {
            connections: MAX_CONNECTIONS + 1,
            ..Settings::default()
        };
        assert!(bad.validate().is_err());
        let bad = Settings {
            max_concurrent: 0,
            ..Settings::default()
        };
        assert!(bad.validate().is_err());
    }
}
