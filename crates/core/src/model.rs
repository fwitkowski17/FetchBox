//! Domain models. Placeholders for milestone 0; fields follow SPEC 6.4.

use serde::{Deserialize, Serialize};

/// Download identifier.
pub type Id = i64;

/// Lifecycle status of a download.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
pub struct Download {
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
}

/// Global settings, applied live (SPEC 2, 6.3). Zero speed limit means unlimited.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// Connections (segments) per download.
    pub connections: u32,
    /// Max simultaneously running downloads; extra ones are `Queued`.
    pub max_concurrent: u32,
    /// Speed limit per download, bytes/s.
    pub speed_per_download: u64,
    /// Global speed limit, bytes/s.
    pub speed_global: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            connections: 4,
            max_concurrent: 3,
            speed_per_download: 0,
            speed_global: 0,
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

/// Input for `Downloader::start`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub id: Id,
    pub url: String,
    pub filename: String,
    pub connections: u32,
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
