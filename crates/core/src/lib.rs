//! Core domain of FetchBox: models, categories, the `Downloader` trait,
//! scheduler and rate limiter. No HTTP, no SQL, no axum.

pub mod category;
pub mod checksum;
pub mod error;
pub mod filename;
pub mod limiter;
pub mod model;
pub mod schedule;
pub mod scheduler;
pub mod stats;
pub mod torrent;

use async_trait::async_trait;

pub use checksum::{ChecksumAlgo, ChecksumInput, ChecksumState};
pub use error::{Error, Result};
pub use model::{
    AddRequest, Category, Conflict, ConflictKind, Credentials, DiskSpace, Download, Event, Handle,
    Hold, HostKeyProblem, HostKeyProblemKind, Id, InstallMethod, Job, KnownHost, ListingEntry,
    OnConflict, OnHoursEnd, ProbeRequest, Progress, ReleaseNote, SegmentState, Settings,
    SourceKind, Status, TrustHost, UrlInfo, UrlKind, VersionInfo,
};
pub use stats::{CategoryStat, DayStat, LargestFile, Stats, StatsRange};
pub use torrent::{
    AddTorrentRequest, PeerState, PeerTransport, Torrent, TorrentFile, TorrentPeer, TorrentPeers,
    TorrentStatus, MAX_MAGNET_LEN, MAX_PEERS_LISTED, MAX_TORRENT_FILE,
};

/// Backend that actually moves bytes (HTTP now, BitTorrent later).
#[async_trait]
pub trait Downloader: Send + Sync {
    /// Start a job and return a handle to it.
    async fn start(&self, job: &Job) -> Result<Handle>;
    /// Pause a running download, flushing its offsets.
    async fn pause(&self, id: Id) -> Result<()>;
    /// Resume a paused download from its saved offsets.
    async fn resume(&self, id: Id) -> Result<()>;
    /// Cancel a download and stop all of its tasks.
    async fn cancel(&self, id: Id) -> Result<()>;
    /// Current progress snapshot. Must be cheap and non-blocking.
    fn progress(&self, id: Id) -> Progress;
}
