//! The operations the HTTP layer needs. Implemented by the server's download manager,
//! so `api` depends on neither the database nor the engine.

use async_trait::async_trait;
use fetchbox_core::{
    AddRequest, Category, Credentials, DiskSpace, Download, Event, Id, KnownHost, Result, Settings,
    Stats, StatsRange, TrustHost, UrlInfo, VersionInfo,
};
use tokio::sync::{broadcast, watch};

/// Download manager as seen by the API.
#[async_trait]
pub trait Service: Send + Sync + 'static {
    /// All downloads, newest first.
    async fn list(&self) -> Result<Vec<Download>>;
    /// Add a download. A collision returns `Error::Conflict` unless the request says how to resolve it.
    async fn add(&self, req: AddRequest) -> Result<Download>;
    /// The running version and whether a newer release is known. `refresh` asks again now instead
    /// of using what was found a while ago (still not more often than every half minute).
    async fn version(&self, refresh: bool) -> Result<VersionInfo>;
    /// Ask the remote server about a URL (name, size, range support) without adding anything.
    /// `credentials` is the login to look at it with (a login written into the address is used too).
    async fn probe(&self, url: &str, credentials: Option<Credentials>) -> Result<UrlInfo>;
    /// The SSH servers whose host key was accepted.
    async fn known_hosts(&self) -> Result<Vec<KnownHost>>;
    /// Accept the host key of an SSH server (replacing the one accepted before).
    async fn trust_host(&self, req: TrustHost) -> Result<KnownHost>;
    /// Forget an accepted host key: the server is unknown again.
    async fn forget_host(&self, id: Id) -> Result<()>;
    /// "Download now": the download hours (and a start time) no longer hold this download back.
    async fn start_now(&self, id: Id) -> Result<Download>;
    async fn pause(&self, id: Id) -> Result<Download>;
    async fn resume(&self, id: Id) -> Result<Download>;
    async fn cancel(&self, id: Id) -> Result<Download>;
    /// Keep the file of a download that is being verified, or that failed its verification,
    /// whatever the checksum says: it becomes `completed`, not verified.
    async fn keep(&self, id: Id) -> Result<Download>;
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
