//! BitTorrent models (SPEC 6.13). Torrents are a separate list from downloads: they have their own
//! statuses (a swarm is not a server), their own table and their own API.

use serde::{Deserialize, Serialize};

use crate::model::{Hold, Id};

/// What a torrent is doing now, in the words a BitTorrent client uses.
///
/// The engine only knows "initialising", "live", "paused" and "error"; the rest is read from the
/// numbers (peers, progress) by [`TorrentStatus::derive`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum TorrentStatus {
    /// A magnet link: the description of the torrent (its files) is being fetched from the swarm.
    Metadata,
    /// Files already on disk are being checked against the torrent.
    Checking,
    /// Looking for peers: none is connected yet and the torrent is not complete.
    Connecting,
    /// Receiving data (and sending to others meanwhile).
    Downloading,
    /// Complete, and sending to other peers.
    Seeding,
    /// Stopped by the user (or at start-up the way it was left). Progress is kept.
    Paused,
    /// Complete and stopped: nothing more to do.
    Finished,
    /// The engine could not go on (`Torrent::error` says why).
    Error,
}

impl TorrentStatus {
    /// Stable lowercase name, as stored in SQLite and sent over the API.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Metadata => "metadata",
            Self::Checking => "checking",
            Self::Connecting => "connecting",
            Self::Downloading => "downloading",
            Self::Seeding => "seeding",
            Self::Paused => "paused",
            Self::Finished => "finished",
            Self::Error => "error",
        }
    }

    /// Statuses in which the torrent holds connections and uses the network.
    pub fn is_active(self) -> bool {
        matches!(
            self,
            Self::Metadata | Self::Checking | Self::Connecting | Self::Downloading | Self::Seeding
        )
    }

    /// The status shown for a torrent the engine reports as running (`live`).
    ///
    /// `complete`: every wanted byte is on disk. `peers`: connections that are up now.
    pub fn derive_live(complete: bool, peers: u32) -> Self {
        match (complete, peers) {
            (true, _) => Self::Seeding,
            (false, 0) => Self::Connecting,
            (false, _) => Self::Downloading,
        }
    }

    /// The status shown for a torrent the user stopped: `finished` when there is nothing left to
    /// download, `paused` otherwise.
    pub fn derive_stopped(complete: bool) -> Self {
        if complete {
            Self::Finished
        } else {
            Self::Paused
        }
    }
}

impl std::str::FromStr for TorrentStatus {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, ()> {
        Ok(match s {
            "metadata" => Self::Metadata,
            "checking" => Self::Checking,
            "connecting" => Self::Connecting,
            "downloading" => Self::Downloading,
            "seeding" => Self::Seeding,
            "paused" => Self::Paused,
            "finished" => Self::Finished,
            "error" => Self::Error,
            _ => return Err(()),
        })
    }
}

/// One torrent as the UI shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Torrent {
    #[cfg_attr(feature = "openapi", schema(value_type = i64))]
    pub id: Id,
    /// The identity of the torrent: 40 hexadecimal characters (SHA-1 of its `info` dictionary).
    pub info_hash: String,
    /// The name inside the torrent; the name from the magnet link (or the hash) until the
    /// description has arrived.
    pub name: String,
    pub status: TorrentStatus,
    /// Total size of the files that are wanted; `None` until the description is known.
    pub size: Option<u64>,
    /// Bytes of those files that are on disk and checked.
    pub downloaded: u64,
    /// Bytes sent to other peers, over all runs of the program.
    pub uploaded: u64,
    /// Bytes per second from the swarm now.
    pub down_speed: u64,
    /// Bytes per second to the swarm now.
    pub up_speed: u64,
    /// Seconds left at the current speed.
    pub eta: Option<u64>,
    /// Peers we are connected to.
    pub peers_connected: u32,
    /// Peers we are trying to connect to.
    pub peers_connecting: u32,
    /// Peers heard of (from trackers, DHT, other peers), connected or not.
    pub peers_known: u32,
    /// Number of files in the torrent; `0` until the description is known.
    pub file_count: u32,
    /// The magnet link the torrent was added with; none for a `.torrent` file.
    pub magnet: Option<String>,
    /// Unix seconds.
    pub added_at: i64,
    /// Unix seconds; set when the last byte arrived.
    pub finished_at: Option<i64>,
    /// Why the torrent stopped (`status` is `error`), or a note such as a full disk.
    pub error: Option<String>,
    /// Why a torrent that should be running waits: the torrent hours are closed (SPEC 6.3.2).
    /// Computed, not stored; the status is `paused` meanwhile.
    pub hold: Option<Hold>,
    /// Unix seconds when the torrent hours open next (only with `hold`).
    pub starts_at: Option<i64>,
    /// The torrent hours do not apply to this torrent ("Start now").
    pub start_now: bool,
    /// The disk it is saved on (SPEC 6.14): a folder of the download directory, chosen when the
    /// torrent was added and kept for good. Empty = the download directory itself.
    pub disk: String,
}

/// A peer we know about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TorrentPeer {
    /// `ip:port`.
    pub address: String,
    pub state: PeerState,
    /// How the connection is made (only for a connected peer).
    pub transport: Option<PeerTransport>,
    /// The program the peer runs, as it names itself (`qBittorrent 5.0`).
    pub client: Option<String>,
    /// Bytes received from this peer since it was first connected.
    pub downloaded: u64,
    /// Bytes sent to this peer.
    pub uploaded: u64,
    /// Bytes per second from this peer now.
    pub down_speed: u64,
    /// Bytes per second to this peer now.
    pub up_speed: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum PeerState {
    /// In the queue to be tried.
    Queued,
    /// A connection is being made.
    Connecting,
    /// Connected: data can flow.
    Connected,
    /// Nothing to exchange with it (for instance both sides are complete): the connection is closed
    /// and is not tried again while that holds.
    Idle,
    /// Failed or refused; may be tried again later.
    Dead,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum PeerTransport {
    Tcp,
    /// uTP: BitTorrent's transport over UDP.
    Utp,
    Socks,
}

/// The peers of a torrent, with the counts of every state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TorrentPeers {
    /// Connected peers first (fastest first), then those being connected, then the rest; at most
    /// [`MAX_PEERS_LISTED`].
    pub peers: Vec<TorrentPeer>,
    pub connected: u32,
    pub connecting: u32,
    pub queued: u32,
    pub idle: u32,
    pub dead: u32,
    /// Peers heard of in total.
    pub known: u32,
    /// The list was cut at the limit.
    pub truncated: bool,
}

/// How many peers one answer lists. A busy swarm knows thousands; the screen needs the active ones.
pub const MAX_PEERS_LISTED: usize = 500;

/// A file inside a torrent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TorrentFile {
    /// Path inside the torrent, `/` separated.
    pub path: String,
    pub size: u64,
    /// Bytes of this file that are on disk and checked.
    pub downloaded: u64,
}

/// Add a torrent by magnet link. A `.torrent` file is sent as the raw body of
/// `POST /api/torrents/file` instead.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AddTorrentRequest {
    /// A `magnet:?xt=urn:btih:...` link.
    pub magnet: String,
    /// Save to this disk (SPEC 6.14) instead of the default one for torrents
    /// (`Settings::torrent_default_disk`, else `default_disk`). Empty = the download directory.
    #[serde(default)]
    pub disk: Option<String>,
}

/// Longest magnet link that is accepted.
pub const MAX_MAGNET_LEN: usize = 8 * 1024;
/// Largest `.torrent` file that is accepted (real ones are well below 1 MiB).
pub const MAX_TORRENT_FILE: usize = 8 * 1024 * 1024;

/// Is this text a magnet link (case-insensitive scheme, surrounding space ignored)?
pub fn is_magnet(text: &str) -> bool {
    let t = text.trim_start();
    // Bytes, not `t[..8]`: that would panic inside a multi-byte character.
    t.as_bytes()
        .get(..8)
        .is_some_and(|head| head.eq_ignore_ascii_case(b"magnet:?"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_status_follows_the_peers_and_the_progress() {
        assert_eq!(
            TorrentStatus::derive_live(false, 0),
            TorrentStatus::Connecting
        );
        assert_eq!(
            TorrentStatus::derive_live(false, 3),
            TorrentStatus::Downloading
        );
        assert_eq!(TorrentStatus::derive_live(true, 0), TorrentStatus::Seeding);
        assert_eq!(TorrentStatus::derive_live(true, 9), TorrentStatus::Seeding);
        assert_eq!(TorrentStatus::derive_stopped(false), TorrentStatus::Paused);
        assert_eq!(TorrentStatus::derive_stopped(true), TorrentStatus::Finished);
    }

    #[test]
    fn statuses_round_trip_through_their_stored_names() {
        for s in [
            TorrentStatus::Metadata,
            TorrentStatus::Checking,
            TorrentStatus::Connecting,
            TorrentStatus::Downloading,
            TorrentStatus::Seeding,
            TorrentStatus::Paused,
            TorrentStatus::Finished,
            TorrentStatus::Error,
        ] {
            assert_eq!(s.as_str().parse(), Ok(s));
        }
        assert_eq!("nonsense".parse::<TorrentStatus>(), Err(()));
    }

    #[test]
    fn only_a_running_torrent_is_active() {
        assert!(TorrentStatus::Seeding.is_active());
        assert!(TorrentStatus::Metadata.is_active());
        assert!(!TorrentStatus::Paused.is_active());
        assert!(!TorrentStatus::Finished.is_active());
        assert!(!TorrentStatus::Error.is_active());
    }

    #[test]
    fn magnet_links_are_recognised_by_their_scheme() {
        assert!(is_magnet("magnet:?xt=urn:btih:abc"));
        assert!(is_magnet("  MAGNET:?xt=urn:btih:abc"));
        assert!(!is_magnet("https://example.com/a.torrent"));
        assert!(!is_magnet("magnet:"));
        assert!(!is_magnet(""));
        assert!(!is_magnet("magnet:ąę"), "not a boundary: must not panic");
    }
}
