//! The operations the HTTP layer needs. Implemented by the server's download manager,
//! so `api` depends on neither the database nor the engine.

use async_trait::async_trait;
use fetchbox_core::{
    AddRequest, AddTorrentRequest, Category, Credentials, DiskSpace, Download, Event, Id,
    KnownHost, Result, Settings, Stats, StatsRange, Torrent, TorrentFile, TorrentPeers, TrustHost,
    UrlInfo, VersionInfo,
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
    async fn disk_space(&self, disk: Option<String>) -> Result<DiskSpace>;
    /// The disks new downloads can go to, with their space (SPEC 6.14).
    async fn disks(&self) -> Result<fetchbox_core::Disks>;
    /// A summary of the downloads of a period. `offset_minutes` is the caller's distance from UTC
    /// (east is positive), which decides where a day begins.
    async fn stats(&self, range: StatsRange, offset_minutes: i32) -> Result<Stats>;
    /// All torrents, newest first (SPEC 6.13).
    async fn torrents(&self) -> Result<Vec<Torrent>>;
    /// Add a torrent by magnet link. The description (name, files) arrives later from the swarm.
    async fn add_torrent_magnet(&self, req: AddTorrentRequest) -> Result<Torrent>;
    /// Add a torrent from the bytes of a `.torrent` file.
    async fn add_torrent_file(&self, bytes: Vec<u8>) -> Result<Torrent>;
    /// Stop a torrent; what it has stays on disk.
    async fn pause_torrent(&self, id: Id) -> Result<Torrent>;
    /// Start a stopped torrent again.
    async fn resume_torrent(&self, id: Id) -> Result<Torrent>;
    /// Exempt a torrent from the torrent hours (SPEC 6.3.2) until it is stopped.
    async fn start_torrent_now(&self, id: Id) -> Result<Torrent>;
    /// Forget a torrent; with `delete_files` its data goes too.
    async fn remove_torrent(&self, id: Id, delete_files: bool) -> Result<()>;
    /// The peers of a torrent with their speeds.
    async fn torrent_peers(&self, id: Id) -> Result<TorrentPeers>;
    /// The files of a torrent with their progress.
    async fn torrent_files(&self, id: Id) -> Result<Vec<TorrentFile>>;
    /// Live updates for SSE.
    fn subscribe(&self) -> broadcast::Receiver<Event>;
    /// Turns `true` when the server is shutting down: long-lived responses (the event stream) must
    /// end then, or a graceful shutdown would wait for them for ever.
    fn closing(&self) -> watch::Receiver<bool>;
}
