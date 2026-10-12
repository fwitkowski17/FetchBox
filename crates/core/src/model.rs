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

/// What kind of source a download comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Http,
    Magnet,
    TorrentFile,
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
