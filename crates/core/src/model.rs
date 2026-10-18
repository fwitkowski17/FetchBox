//! Domain models. Placeholders for milestone 0; fields follow SPEC 6.4.

use serde::{Deserialize, Serialize};

use crate::checksum::{ChecksumInput, ChecksumState};

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
    /// The file is complete and its checksum is being computed (SPEC 6.2.2). Does not hold a
    /// download slot. Ends as `completed` (verified) or `failed` (does not match).
    Verifying,
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
            Status::Verifying => "verifying",
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
            "verifying" => Status::Verifying,
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
    /// `http://` and `https://`.
    Http,
    /// `ftp://` and `ftps://` (FTP over TLS).
    Ftp,
    /// `sftp://` (the SSH file transfer protocol).
    Sftp,
    Magnet,
    TorrentFile,
}

impl SourceKind {
    /// Stable name, as stored in SQLite.
    pub fn as_str(self) -> &'static str {
        match self {
            SourceKind::Http => "http",
            SourceKind::Ftp => "ftp",
            SourceKind::Sftp => "sftp",
            SourceKind::Magnet => "magnet",
            SourceKind::TorrentFile => "torrent_file",
        }
    }

    /// Parse the stored name.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "http" => SourceKind::Http,
            "ftp" => SourceKind::Ftp,
            "sftp" => SourceKind::Sftp,
            "magnet" => SourceKind::Magnet,
            "torrent_file" => SourceKind::TorrentFile,
            _ => return None,
        })
    }

    /// The kind an address belongs to, judged by its scheme (`None` for a scheme FetchBox cannot
    /// download from).
    pub fn for_url(url: &str) -> Option<Self> {
        let (scheme, _) = url.trim_start().split_once(':')?;
        Some(match scheme.to_ascii_lowercase().as_str() {
            "http" | "https" => SourceKind::Http,
            "ftp" | "ftps" => SourceKind::Ftp,
            "sftp" => SourceKind::Sftp,
            _ => return None,
        })
    }
}

/// A user name and password for a source that asks for a login (SPEC 6.12). Kept on the server so
/// that a paused or restarted download can go on; the password is never sent back by the API and
/// never written to the log.
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Credentials {
    pub username: String,
    /// May be empty (a login without a password).
    #[serde(default)]
    pub password: String,
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("username", &self.username)
            .field("password", &"***")
            .finish()
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
    /// The user name the download logs in with, if it has a login. The password is kept on the
    /// server and never shown.
    #[serde(default)]
    pub username: Option<String>,
    /// Subdirectory of the download directory the file goes to; empty = the directory itself.
    #[serde(default)]
    pub subdir: String,
    /// The disk the file is saved on (SPEC 6.14): a folder of the download directory, chosen when
    /// the download was added and kept for good, so changing the default disk never moves it.
    /// Empty = the download directory itself.
    #[serde(default)]
    pub disk: String,
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
    /// Do not start before this moment (unix seconds): chosen when the download was added.
    #[serde(default)]
    pub start_at: Option<i64>,
    /// Why a queued download is not running (not persisted; set for waiting downloads).
    #[serde(default)]
    pub hold: Option<Hold>,
    /// When a waiting download is expected to start (unix seconds): its start time, or the next
    /// opening of the download hours. Not persisted.
    #[serde(default)]
    pub starts_at: Option<i64>,
    /// The checksum the finished file is verified against (chosen when the download was added),
    /// and what the verification found when it did not match.
    #[serde(default)]
    pub checksum: Option<ChecksumState>,
    /// The file was checked against `checksum` and matched.
    #[serde(default)]
    pub verified: bool,
    /// The verification did not succeed: the file does not match, or it could not be read. While
    /// the status is `failed` the user decides (download again, keep the file anyway, delete it);
    /// once they keep it the status is `completed` and this stays, to be shown as a warning.
    #[serde(default)]
    pub verify_failed: bool,
}

/// Why a queued download is waiting instead of running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum Hold {
    /// The hours are closed (`Settings::schedule_enabled` for downloads,
    /// `Settings::torrent_schedule_enabled` for torrents).
    Window,
    /// It was set to begin at a later time (`Download::start_at`).
    Time,
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
    /// Ask GitHub now and then whether a newer release exists (`GET /api/version`). Nothing is
    /// sent but the request itself (address of the server, program name and version).
    #[serde(default = "default_check_updates")]
    pub check_updates: bool,
    /// Also announce pre-releases (`0.7.0-rc.1`) as updates. Off by default: a pre-release is for
    /// those who want to try what is coming and do not mind a rough edge.
    #[serde(default)]
    pub check_prereleases: bool,
    /// Download only during set hours (SPEC 6.3.2): outside them downloads wait, and running ones
    /// are put back in the queue. A single download can opt out (`start_now`).
    #[serde(default)]
    pub schedule_enabled: bool,
    /// Start of the hours, minutes after local midnight (`0` = 00:00).
    #[serde(default)]
    pub schedule_from: u16,
    /// End of the hours (exclusive), minutes after local midnight; earlier than the start means
    /// the hours run over midnight (`22:00`-`06:00`).
    #[serde(default = "default_schedule_to")]
    pub schedule_to: u16,
    /// IANA time zone the hours are in (`Europe/Warsaw`). The server's own clock is usually UTC,
    /// so the zone is stated, not guessed.
    #[serde(default = "default_schedule_timezone")]
    pub schedule_timezone: String,
    /// What running downloads do when the download hours end.
    #[serde(default)]
    pub schedule_on_end: OnHoursEnd,
    /// Speed limit of all torrents together, receiving, bytes/s (`0` = none). Separate from the
    /// limits of downloads: a swarm and a server are different things to cap.
    #[serde(default)]
    pub torrent_down_limit: u64,
    /// Speed limit of all torrents together, sending, bytes/s (`0` = none). Seeding goes on until
    /// the user stops a torrent, so this is what keeps the upload line free.
    #[serde(default)]
    pub torrent_up_limit: u64,
    /// Let torrents run only during set hours (SPEC 6.3.2). Separate from the download hours: a
    /// swarm is often wanted at other times than a download from a server. Outside the hours a
    /// torrent is stopped, receiving and sending alike, and goes on when they open.
    #[serde(default)]
    pub torrent_schedule_enabled: bool,
    /// Start of the torrent hours, minutes after local midnight.
    #[serde(default)]
    pub torrent_schedule_from: u16,
    /// End of the torrent hours (exclusive); earlier than the start means over midnight.
    #[serde(default = "default_schedule_to")]
    pub torrent_schedule_to: u16,
    /// IANA time zone the torrent hours are in.
    #[serde(default = "default_schedule_timezone")]
    pub torrent_schedule_timezone: String,
    /// The disk new downloads are saved on unless the request names another (SPEC 6.14): a folder
    /// of the download directory that is a file system of its own. Empty = the download directory
    /// itself. Existing downloads stay where they are.
    #[serde(default)]
    pub default_disk: String,
}

/// What happens to the downloads that are running when the download hours end.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum OnHoursEnd {
    /// Stop them (they keep what they have) and go on when the hours open again: nothing is
    /// downloaded outside the hours. The default, since a data package may end with the hours.
    #[default]
    Pause,
    /// Let them finish; only downloads that have not started wait for the next hours.
    Finish,
}

impl OnHoursEnd {
    /// Stable name, as stored in SQLite.
    pub fn as_str(self) -> &'static str {
        match self {
            OnHoursEnd::Pause => "pause",
            OnHoursEnd::Finish => "finish",
        }
    }
}

impl std::str::FromStr for OnHoursEnd {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, ()> {
        match s {
            "pause" => Ok(OnHoursEnd::Pause),
            "finish" => Ok(OnHoursEnd::Finish),
            _ => Err(()),
        }
    }
}

fn default_schedule_to() -> u16 {
    6 * 60
}

fn default_schedule_timezone() -> String {
    "UTC".to_owned()
}

fn default_check_updates() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            connections: 4,
            max_concurrent: 3,
            speed_per_download: 0,
            speed_global: 0,
            category_subdirs: false,
            check_updates: true,
            check_prereleases: false,
            schedule_enabled: false,
            schedule_from: 0,
            schedule_to: default_schedule_to(),
            schedule_timezone: default_schedule_timezone(),
            schedule_on_end: OnHoursEnd::Pause,
            torrent_down_limit: 0,
            torrent_up_limit: 0,
            torrent_schedule_enabled: false,
            torrent_schedule_from: 0,
            torrent_schedule_to: default_schedule_to(),
            torrent_schedule_timezone: default_schedule_timezone(),
            default_disk: crate::disks::MAIN.to_owned(),
        }
    }
}

/// Upper bound for connections per download; more gains nothing and hurts servers.
pub const MAX_CONNECTIONS: u32 = 32;

impl Settings {
    /// The download hours as a window (whether or not they are switched on).
    pub fn schedule_window(&self) -> crate::schedule::Window {
        crate::schedule::Window {
            from: self.schedule_from,
            to: self.schedule_to,
        }
    }

    /// The torrent hours as a window (whether or not they are switched on).
    pub fn torrent_schedule_window(&self) -> crate::schedule::Window {
        crate::schedule::Window {
            from: self.torrent_schedule_from,
            to: self.torrent_schedule_to,
        }
    }

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
        if self.schedule_enabled && !self.schedule_window().is_valid() {
            return Err(crate::Error::InvalidInput(
                "the download hours need a start and an end that differ (minutes 0..1440)".into(),
            ));
        }
        if !crate::disks::is_valid_name(&self.default_disk) {
            return Err(crate::Error::InvalidInput(
                "default_disk must be the name of a folder in the download directory".into(),
            ));
        }
        if self.torrent_schedule_enabled && !self.torrent_schedule_window().is_valid() {
            return Err(crate::Error::InvalidInput(
                "the torrent hours need a start and an end that differ (minutes 0..1440)".into(),
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
    /// Begin at this moment (unix seconds, at most 31 days ahead) instead of at once. Not together
    /// with `start_now`; it makes no sense with download hours either (they decide the start).
    #[serde(default)]
    pub start_at: Option<i64>,
    /// "Download now": do not wait for the download hours, which apply to this download otherwise.
    #[serde(default)]
    pub start_now: bool,
    /// Verify the finished file against this checksum (SPEC 6.2.2).
    #[serde(default)]
    pub checksum: Option<ChecksumInput>,
    /// Save to this disk (SPEC 6.14) instead of the default one (`Settings::default_disk`). The
    /// empty name is the download directory itself; leave the field out to take the default.
    #[serde(default)]
    pub disk: Option<String>,
    /// Log in to the source with this user name and password (SPEC 6.12). They win over a login
    /// written into the address (`ftp://user:password@host/file`), which is moved here anyway.
    #[serde(default)]
    pub credentials: Option<Credentials>,
}

/// What a release changed, as its author wrote it on GitHub (Markdown).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ReleaseNote {
    /// Version without a leading `v`.
    pub version: String,
    /// Page of the release.
    pub url: String,
    /// The description of the release (Markdown, shortened when very long). Show it as text.
    pub body: String,
    /// A pre-release (`0.7.0-rc.1`); only listed when `Settings::check_prereleases` is on.
    pub prerelease: bool,
}

/// How FetchBox was installed on this machine. The program cannot find that out by itself, so the
/// installer states it (`FETCHBOX_INSTALL_METHOD`), and the update dialog shows the steps that
/// belong to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum InstallMethod {
    /// The container image (Docker Compose).
    Docker,
    /// A DEB or RPM package (`apt`, `dnf`).
    Package,
    /// The Proxmox container made by `deploy/proxmox`.
    Lxc,
    /// `deploy/install.sh` on a Linux host.
    Script,
    /// Built from the source code (`scripts/build.sh`).
    Source,
}

impl InstallMethod {
    /// The spelling used in `FETCHBOX_INSTALL_METHOD` and in the API.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Docker => "docker",
            Self::Package => "package",
            Self::Lxc => "lxc",
            Self::Script => "script",
            Self::Source => "source",
        }
    }
}

impl std::str::FromStr for InstallMethod {
    type Err = String;

    /// Case-insensitive; surrounding space is ignored.
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "docker" => Ok(Self::Docker),
            "package" => Ok(Self::Package),
            "lxc" => Ok(Self::Lxc),
            "script" => Ok(Self::Script),
            "source" => Ok(Self::Source),
            other => Err(format!(
                "unknown install method '{other}' (expected docker, package, lxc, script or source)"
            )),
        }
    }
}

/// The version of the running program and what is known about newer ones.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct VersionInfo {
    /// Version of this program (`0.9.0`).
    pub current: String,
    /// Update checks are switched on (`Settings::check_updates`). When off, nothing else is set.
    pub enabled: bool,
    /// Version of the newest published release, without a leading `v`.
    pub latest: Option<String>,
    /// `latest` is newer than `current`.
    pub update_available: bool,
    /// `latest` is a pre-release (only possible when `Settings::check_prereleases` is on).
    pub latest_prerelease: bool,
    /// Page of the newest release.
    pub release_url: Option<String>,
    /// When the answer was obtained (unix seconds); `None` if no check has succeeded yet.
    pub checked_at: Option<i64>,
    /// The last attempt did not get an answer (offline, rate limited, ...).
    pub check_failed: bool,
    /// What changed in every release newer than `current`, newest first (at most ten).
    pub notes: Vec<ReleaseNote>,
    /// How this copy was installed (`FETCHBOX_INSTALL_METHOD`); `None` when nobody said. Decides
    /// which update steps the UI shows.
    pub install_method: Option<InstallMethod>,
}

/// Request to look at a URL before adding it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ProbeRequest {
    pub url: String,
    /// The login to look at the address with (see `AddRequest::credentials`).
    #[serde(default)]
    pub credentials: Option<Credentials>,
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
    /// What the file name says it is (`Other` for a directory).
    pub category: Category,
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

/// An SSH server the user has accepted the host key of (SPEC 6.12). SFTP has no certificate
/// authorities: the key a server presents is only believed when it is the one that was accepted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct KnownHost {
    #[cfg_attr(feature = "openapi", schema(value_type = i64))]
    pub id: Id,
    /// Host name or address, lowercase, as written in the download address.
    pub host: String,
    pub port: u16,
    /// Kind of the key (`ssh-ed25519`, `ssh-rsa`, ...).
    pub key_type: String,
    /// `SHA256:` and the unpadded base64 of the key's hash, as `ssh-keygen -l` prints it.
    pub fingerprint: String,
    /// When it was accepted (unix seconds).
    pub added_at: i64,
}

/// Request to accept the host key of an SSH server: send back what a refused add showed.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TrustHost {
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub key_type: String,
    pub fingerprint: String,
}

/// What is wrong with the host key of an SSH server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum HostKeyProblemKind {
    /// The server has not been seen before: the user decides whether to trust the key.
    Untrusted,
    /// The server presents another key than the accepted one: a reinstalled server, or someone
    /// in between. Nothing is downloaded until the user has removed the old key.
    Changed,
}

/// A refused connection to an SSH server because of its host key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct HostKeyProblem {
    pub kind: HostKeyProblemKind,
    pub host: String,
    pub port: u16,
    pub key_type: String,
    /// The fingerprint the server just presented.
    pub fingerprint: String,
    /// The accepted fingerprint, for `changed`.
    pub trusted_fingerprint: Option<String>,
}

impl std::fmt::Display for HostKeyProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.kind {
            HostKeyProblemKind::Untrusted => write!(
                f,
                "the SSH server {}:{} is not trusted yet (its {} key is {})",
                self.host, self.port, self.key_type, self.fingerprint
            ),
            HostKeyProblemKind::Changed => write!(
                f,
                "the host key of {}:{} has changed (now {}, accepted {}); \
                 remove the old key in Settings if the server was reinstalled",
                self.host,
                self.port,
                self.fingerprint,
                self.trusted_fingerprint.as_deref().unwrap_or("?")
            ),
        }
    }
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
    Download { download: Box<Download> },
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
    /// A torrent was added, or its status or numbers changed (about once a second while it moves).
    Torrent {
        torrent: Box<crate::torrent::Torrent>,
    },
    /// A torrent was removed from the list.
    TorrentRemoved {
        #[cfg_attr(feature = "openapi", schema(value_type = i64))]
        id: Id,
    },
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
    /// The login of the source, if it needs one.
    pub credentials: Option<Credentials>,
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
            SourceKind::Ftp,
            SourceKind::Sftp,
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

    #[test]
    fn the_scheme_decides_the_kind_of_source() {
        for (url, kind) in [
            ("http://h/a", Some(SourceKind::Http)),
            ("HTTPS://h/a", Some(SourceKind::Http)),
            ("  ftp://h/a", Some(SourceKind::Ftp)),
            ("ftps://h/a", Some(SourceKind::Ftp)),
            ("sftp://h/a", Some(SourceKind::Sftp)),
            ("file:///etc/passwd", None),
            ("javascript:alert(1)", None),
            ("no scheme", None),
        ] {
            assert_eq!(SourceKind::for_url(url), kind, "{url}");
        }
    }

    #[test]
    fn a_password_never_shows_in_debug_output() {
        let c = Credentials {
            username: "filip".into(),
            password: "hunter2".into(),
        };
        let shown = format!("{c:?}");
        assert!(
            shown.contains("filip") && !shown.contains("hunter2"),
            "{shown}"
        );
        let req: AddRequest = serde_json::from_str(
            r#"{"url":"ftp://h/a","credentials":{"username":"u","password":"hunter2"}}"#,
        )
        .expect("request");
        assert!(!format!("{req:?}").contains("hunter2"));
    }
}
