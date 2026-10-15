//! Core domain of FetchBox: models, categories, the `Downloader` trait,
//! scheduler and rate limiter. No HTTP, no SQL, no axum.

pub mod category;
pub mod error;
pub mod filename;
pub mod limiter;
pub mod model;
pub mod scheduler;
pub mod stats;

use async_trait::async_trait;

pub use error::{Error, Result};
pub use model::{
    AddRequest, Category, Conflict, ConflictKind, DiskSpace, Download, Event, Handle, Id, Job,
    ListingEntry, OnConflict, ProbeRequest, Progress, SegmentState, Settings, SourceKind, Status,
    UrlInfo, UrlKind, VersionInfo,
};
pub use stats::{CategoryStat, DayStat, LargestFile, Stats, StatsRange};

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
