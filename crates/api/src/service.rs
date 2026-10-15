//! The operations the HTTP layer needs. Implemented by the server's download manager,
//! so `api` depends on neither the database nor the engine.

use async_trait::async_trait;
use fetchbox_core::{
    AddRequest, Category, DiskSpace, Download, Event, Id, Result, Settings, Stats, StatsRange,
    UrlInfo,
};
use tokio::sync::{broadcast, watch};

/// Download manager as seen by the API.
#[async_trait]
pub trait Service: Send + Sync + 'static {
    /// All downloads, newest first.
    async fn list(&self) -> Result<Vec<Download>>;
    /// Add a download. A collision returns `Error::Conflict` unless the request says how to resolve it.
    async fn add(&self, req: AddRequest) -> Result<Download>;
    /// Ask the remote server about a URL (name, size, range support) without adding anything.
    async fn probe(&self, url: &str) -> Result<UrlInfo>;
    async fn pause(&self, id: Id) -> Result<Download>;
    async fn resume(&self, id: Id) -> Result<Download>;
    async fn cancel(&self, id: Id) -> Result<Download>;
    /// Remove from the list (cancels first when still active). A finished file stays on disk
    /// unless `delete_file` is set.
    async fn remove(&self, id: Id, delete_file: bool) -> Result<()>;
    async fn set_category(&self, id: Id, category: Category) -> Result<Download>;
    /// Limit the speed of one download (bytes/s, `0` = none); applied live when it is running.
    async fn set_speed_limit(&self, id: Id, rate: u64) -> Result<Download>;
    /// Hide a finished download from the main list, or bring it back.
    async fn set_archived(&self, id: Id, archived: bool) -> Result<Download>;
    async fn settings(&self) -> Result<Settings>;
    async fn update_settings(&self, settings: Settings) -> Result<Settings>;
    /// Free space where downloads are written.
    async fn disk_space(&self) -> Result<DiskSpace>;
    /// A summary of the downloads of a period. `offset_minutes` is the caller's distance from UTC
    /// (east is positive), which decides where a day begins.
    async fn stats(&self, range: StatsRange, offset_minutes: i32) -> Result<Stats>;
    /// Live updates for SSE.
    fn subscribe(&self) -> broadcast::Receiver<Event>;
    /// Turns `true` when the server is shutting down: long-lived responses (the event stream) must
    /// end then, or a graceful shutdown would wait for them for ever.
    fn closing(&self) -> watch::Receiver<bool>;
}
