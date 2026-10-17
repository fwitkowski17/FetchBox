//! BitTorrent engine: an adapter over `librqbit` (SPEC 6.13).
//!
//! It knows nothing of the database, the HTTP API or the list the user sees: it adds, stops and
//! removes torrents and answers what the swarm is doing. The server's `TorrentManager` turns that
//! into statuses and events.

mod error;
mod parse;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bytes::Bytes;
use fetchbox_core::{
    PeerState, PeerTransport, TorrentFile, TorrentPeer, TorrentPeers, MAX_PEERS_LISTED,
};
use librqbit::api::TorrentIdOrHash;
use librqbit::dht::{DhtPersistenceConfig, Id20};
use librqbit::{
    AddTorrent, AddTorrentOptions, DhtSessionConfig, ListenerMode, ListenerOptions, ManagedTorrent,
    Session, SessionOptions, SessionPersistenceConfig, TorrentStatsState,
};

pub use error::TorrentError;
pub use parse::{parse_magnet, parse_torrent_file, Contents, Parsed};

/// How the session is set up.
#[derive(Debug, Clone)]
pub struct Config {
    /// Where torrents are saved (a multi-file torrent gets a folder of its name in it).
    pub output_dir: PathBuf,
    /// Where the engine keeps its own state: which torrents it has, which pieces are done (so a
    /// restart does not check every file again) and the DHT's routing table.
    pub state_dir: PathBuf,
    /// TCP and uTP port for incoming connections; `0` picks one.
    pub listen_port: u16,
    /// Receiving and sending limits of all torrents together, bytes/s; `0` = none.
    pub down_limit: u64,
    pub up_limit: u64,
    /// Find peers through the DHT. Off only in tests.
    pub dht: bool,
    /// Name the engine gives itself to peers, `FetchBox/0.9.0`.
    pub client: String,
}

/// What the engine says a torrent is doing; the manager turns it into a status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    /// Files on disk are being checked against the torrent.
    Checking,
    /// Running: talking to peers.
    Live,
    /// Stopped by the user.
    Stopped,
    /// Could not go on.
    Error,
}

/// A torrent's numbers at one moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub state: RunState,
    /// The torrent's name; for a magnet link the `dn` it carried until the description arrives.
    pub name: Option<String>,
    /// Size of the wanted files.
    pub size: u64,
    pub downloaded: u64,
    /// Bytes sent since the engine started (the manager adds what was sent in earlier runs).
    pub uploaded: u64,
    pub down_speed: u64,
    pub up_speed: u64,
    pub eta_secs: Option<u64>,
    pub connected: u32,
    pub connecting: u32,
    pub known: u32,
    pub file_count: u32,
    /// Every wanted byte is on disk.
    pub complete: bool,
    pub error: Option<String>,
}

/// A torrent that was just added.
#[derive(Debug, Clone)]
pub struct Added {
    pub info_hash: String,
    /// The `.torrent` file (built from the swarm's answer for a magnet link).
    pub metainfo: Bytes,
}

/// The last peer figures of a torrent, to turn byte counters into speeds.
struct Sample {
    at: Instant,
    bytes: HashMap<String, (u64, u64)>,
    rates: HashMap<String, (u64, u64)>,
}

/// How long, and how often, a resumed torrent is watched for the check described in `resume`.
const RESUME_WATCH: Duration = Duration::from_secs(30 * 60);
const RESUME_POLL: Duration = Duration::from_millis(300);

/// Two reads closer than this give the rates of the first one: a speed over a few milliseconds is
/// noise, and several browser tabs polling must not disturb each other.
const MIN_SAMPLE_GAP: Duration = Duration::from_millis(700);

pub struct TorrentEngine {
    session: Arc<Session>,
    samples: Mutex<HashMap<Id20, Sample>>,
}

fn engine_err(e: impl std::fmt::Display) -> TorrentError {
    TorrentError::Engine(format!("{e:#}"))
}

fn non_zero(limit: u64) -> Option<NonZeroU32> {
    NonZeroU32::new(u32::try_from(limit).unwrap_or(u32::MAX))
}

fn hash_of(info_hash: &str) -> Result<Id20, TorrentError> {
    Id20::from_str(info_hash).map_err(|_| TorrentError::Invalid("not an info hash".into()))
}

impl TorrentEngine {
    /// Start the session: restores the torrents it had, opens the listening port and joins the DHT.
    pub async fn start(cfg: Config) -> Result<Arc<Self>, TorrentError> {
        std::fs::create_dir_all(&cfg.output_dir).map_err(engine_err)?;
        std::fs::create_dir_all(&cfg.state_dir).map_err(engine_err)?;
        let mut opts = SessionOptions {
            fastresume: true,
            persistence: Some(SessionPersistenceConfig::Json {
                folder: Some(cfg.state_dir.join("session")),
            }),
            dht: cfg.dht.then(|| DhtSessionConfig {
                persistence: Some(DhtPersistenceConfig {
                    config_filename: Some(cfg.state_dir.join("dht.json")),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            listen: Some(ListenerOptions {
                mode: ListenerMode::TcpAndUtp,
                listen_addr: SocketAddr::from(([0, 0, 0, 0], cfg.listen_port)),
                ipv4_only: true,
                ..Default::default()
            }),
            client_name_and_version: Some(cfg.client),
            ipv4_only: true,
            ..Default::default()
        };
        opts.ratelimits.download_bps = non_zero(cfg.down_limit);
        opts.ratelimits.upload_bps = non_zero(cfg.up_limit);
        let session = Session::new_with_opts(cfg.output_dir, opts)
            .await
            .map_err(engine_err)?;
        Ok(Arc::new(Self {
            session,
            samples: Mutex::new(HashMap::new()),
        }))
    }

    /// The port peers connect to, once the session listens.
    pub fn listen_port(&self) -> Option<u16> {
        self.session.listen_addr().map(|a| a.port())
    }

    /// Change the limits of all torrents together (bytes/s, `0` = none); applies at once.
    pub fn set_limits(&self, down: u64, up: u64) {
        self.session.ratelimits.set_download_bps(non_zero(down));
        self.session.ratelimits.set_upload_bps(non_zero(up));
    }

    fn handle(&self, info_hash: &str) -> Result<Arc<ManagedTorrent>, TorrentError> {
        self.session
            .get(TorrentIdOrHash::Hash(hash_of(info_hash)?))
            .ok_or(TorrentError::NotFound)
    }

    /// Info hashes of every torrent the session has.
    pub fn hashes(&self) -> Vec<String> {
        self.session
            .with_torrents(|it| it.map(|(_, t)| t.info_hash().as_string()).collect())
    }

    pub fn contains(&self, info_hash: &str) -> bool {
        self.handle(info_hash).is_ok()
    }

    /// Add a torrent from a magnet link. Resolves once the swarm has sent the description of the
    /// torrent, which can take long (or never happen): run it as a task and drop it to give up.
    pub async fn add_magnet(
        &self,
        uri: &str,
        paused: bool,
        initial_peers: Vec<SocketAddr>,
    ) -> Result<Added, TorrentError> {
        self.add(
            AddTorrent::from_url(uri.trim().to_owned()),
            paused,
            initial_peers,
        )
        .await
    }

    /// Add a torrent from the bytes of a `.torrent` file.
    pub async fn add_file(
        &self,
        bytes: Bytes,
        paused: bool,
        initial_peers: Vec<SocketAddr>,
    ) -> Result<Added, TorrentError> {
        self.add(AddTorrent::from_bytes(bytes), paused, initial_peers)
            .await
    }

    async fn add(
        &self,
        what: AddTorrent<'_>,
        paused: bool,
        initial_peers: Vec<SocketAddr>,
    ) -> Result<Added, TorrentError> {
        let opts = AddTorrentOptions {
            paused,
            // Files already in the torrents folder are checked and kept (a torrent added again after
            // it was removed without its files), not refused.
            overwrite: true,
            initial_peers: (!initial_peers.is_empty()).then_some(initial_peers),
            ..Default::default()
        };
        match self
            .session
            .add_torrent(what, Some(opts))
            .await
            .map_err(engine_err)?
        {
            librqbit::AddTorrentResponse::Added(_, handle) => Ok(added(&handle)),
            librqbit::AddTorrentResponse::AlreadyManaged(..) => Err(TorrentError::Duplicate),
            librqbit::AddTorrentResponse::ListOnly(_) => {
                Err(TorrentError::Engine("unexpected list-only answer".into()))
            }
        }
    }

    /// The `.torrent` file of a torrent whose description has arrived.
    pub fn metainfo(&self, info_hash: &str) -> Option<Bytes> {
        let handle = self.handle(info_hash).ok()?;
        let md = handle.metadata.load_full()?;
        Some(md.torrent_bytes.clone())
    }

    pub async fn pause(&self, info_hash: &str) -> Result<(), TorrentError> {
        let handle = self.handle(info_hash)?;
        if handle.is_paused() {
            return Ok(());
        }
        self.session.pause(&handle).await.map_err(engine_err)
    }

    pub async fn resume(&self, info_hash: &str) -> Result<(), TorrentError> {
        let handle = self.handle(info_hash)?;
        if !handle.is_paused() {
            return Ok(());
        }
        self.session.unpause(&handle).await.map_err(engine_err)?;
        // A torrent that came back stopped from the last run is still checking its files when it is
        // started: the engine finishes that check with the start it was given then ("stay paused"),
        // and the request just made is lost. Once the check is over, start it again.
        let session = self.session.clone();
        tokio::spawn(async move {
            for _ in 0..(RESUME_WATCH.as_millis() / RESUME_POLL.as_millis()) {
                tokio::time::sleep(RESUME_POLL).await;
                match handle.stats().state {
                    TorrentStatsState::Initializing { .. } => {}
                    TorrentStatsState::Paused if !handle.is_paused() => {
                        let _ = session.unpause(&handle).await;
                        return;
                    }
                    _ => return,
                }
            }
        });
        Ok(())
    }

    /// Forget a torrent; with `delete_files` its data goes too.
    pub async fn remove(&self, info_hash: &str, delete_files: bool) -> Result<(), TorrentError> {
        let id = hash_of(info_hash)?;
        if self.session.get(TorrentIdOrHash::Hash(id)).is_none() {
            return Err(TorrentError::NotFound);
        }
        self.session
            .delete(TorrentIdOrHash::Hash(id), delete_files)
            .await
            .map_err(engine_err)?;
        self.samples
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
        Ok(())
    }

    /// The numbers of one torrent.
    pub fn snapshot(&self, info_hash: &str) -> Option<Snapshot> {
        let handle = self.handle(info_hash).ok()?;
        Some(snapshot_of(&handle))
    }

    /// The numbers of every torrent, by info hash.
    pub fn snapshots(&self) -> Vec<(String, Snapshot)> {
        let handles: Vec<_> = self
            .session
            .with_torrents(|it| it.map(|(_, t)| t.clone()).collect());
        handles
            .iter()
            .map(|h| (h.info_hash().as_string(), snapshot_of(h)))
            .collect()
    }

    /// The files of a torrent with how much of each is on disk; `None` while the description is
    /// still being fetched.
    pub fn files(&self, info_hash: &str) -> Option<Vec<TorrentFile>> {
        let handle = self.handle(info_hash).ok()?;
        let md = handle.metadata.load_full()?;
        let progress = handle.stats().file_progress;
        Some(
            md.file_infos
                .iter()
                .enumerate()
                .map(|(i, f)| TorrentFile {
                    path: f
                        .relative_filename
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy())
                        .collect::<Vec<_>>()
                        .join("/"),
                    size: f.len,
                    downloaded: progress.get(i).copied().unwrap_or(0).min(f.len),
                })
                .collect(),
        )
    }

    /// The peers of a torrent, the active ones first, each with its speed (the bytes moved since the
    /// previous call, over the time between them).
    pub fn peers(&self, info_hash: &str) -> Result<TorrentPeers, TorrentError> {
        let id = hash_of(info_hash)?;
        let handle = self.handle(info_hash)?;
        let Some(live) = handle.live() else {
            return Ok(TorrentPeers::default());
        };
        // The filter's type is not public; it is the JSON of the engine's own HTTP API.
        let filter =
            serde_json::from_value(serde_json::json!({ "state": "all" })).map_err(engine_err)?;
        let mut read = read_peers(&live.per_peer_stats_snapshot(filter));
        // Our own port, found on the loopback address by local discovery, is not a peer.
        if let Some(port) = self.listen_port() {
            let me = [format!("127.0.0.1:{port}"), format!("[::1]:{port}")];
            read.retain(|p| !me.contains(&p.address));
        }

        let now = Instant::now();
        let mut samples = self.samples.lock().unwrap_or_else(|e| e.into_inner());
        let sample = samples.entry(id).or_insert_with(|| Sample {
            at: now - MIN_SAMPLE_GAP * 2,
            bytes: HashMap::new(),
            rates: HashMap::new(),
        });
        let elapsed = now.duration_since(sample.at);
        if elapsed >= MIN_SAMPLE_GAP {
            let secs = elapsed.as_secs_f64();
            // The first read has nothing to compare with: the speeds start at zero.
            let first = sample.bytes.is_empty();
            let mut rates = HashMap::with_capacity(read.len());
            let mut bytes = HashMap::with_capacity(read.len());
            for p in &read {
                let (prev_down, prev_up) = sample
                    .bytes
                    .get(&p.address)
                    .copied()
                    .unwrap_or((p.downloaded, p.uploaded));
                let rate = |now: u64, before: u64| {
                    if first {
                        0
                    } else {
                        (now.saturating_sub(before) as f64 / secs) as u64
                    }
                };
                rates.insert(
                    p.address.clone(),
                    (rate(p.downloaded, prev_down), rate(p.uploaded, prev_up)),
                );
                bytes.insert(p.address.clone(), (p.downloaded, p.uploaded));
            }
            sample.at = now;
            sample.bytes = bytes;
            sample.rates = rates;
        }

        let mut out = TorrentPeers::default();
        let mut peers = read;
        for p in &mut peers {
            let (down, up) = sample.rates.get(&p.address).copied().unwrap_or((0, 0));
            p.down_speed = down;
            p.up_speed = up;
            match p.state {
                PeerState::Connected => out.connected += 1,
                PeerState::Connecting => out.connecting += 1,
                PeerState::Queued => out.queued += 1,
                PeerState::Idle => out.idle += 1,
                PeerState::Dead => out.dead += 1,
            }
        }
        out.known = u32::try_from(peers.len()).unwrap_or(u32::MAX);
        // Connected first, the busiest of them on top; then those being connected; then the rest.
        peers.sort_by(|a, b| {
            let rank = |p: &TorrentPeer| match p.state {
                PeerState::Connected => 0,
                PeerState::Connecting => 1,
                PeerState::Queued => 2,
                PeerState::Idle => 3,
                PeerState::Dead => 4,
            };
            rank(a)
                .cmp(&rank(b))
                .then((b.down_speed + b.up_speed).cmp(&(a.down_speed + a.up_speed)))
                .then(b.downloaded.cmp(&a.downloaded))
                .then(a.address.cmp(&b.address))
        });
        out.truncated = peers.len() > MAX_PEERS_LISTED;
        peers.truncate(MAX_PEERS_LISTED);
        out.peers = peers;
        Ok(out)
    }

    /// Stop everything. Torrents are paused for the session to write its state; they are not
    /// removed, and start again with the program.
    pub async fn shutdown(&self) {
        self.session.stop().await;
    }
}

fn added(handle: &ManagedTorrent) -> Added {
    Added {
        info_hash: handle.info_hash().as_string(),
        metainfo: handle
            .metadata
            .load_full()
            .map(|m| m.torrent_bytes.clone())
            .unwrap_or_default(),
    }
}

fn snapshot_of(handle: &ManagedTorrent) -> Snapshot {
    let stats = handle.stats();
    let state = match &stats.state {
        TorrentStatsState::Initializing { paused: true } | TorrentStatsState::Paused => {
            RunState::Stopped
        }
        TorrentStatsState::Initializing { paused: false } => RunState::Checking,
        TorrentStatsState::Live => RunState::Live,
        TorrentStatsState::Error => RunState::Error,
    };
    let (mut down_speed, mut up_speed, mut eta_secs): (u64, u64, Option<u64>) = (0, 0, None);
    let (mut connected, mut connecting, mut known) = (0, 0, 0);
    if let Some(live) = &stats.live {
        down_speed = live.download_speed.as_bytes();
        up_speed = live.upload_speed.as_bytes();
        let peers = &live.snapshot.peer_stats;
        connected = peers.live;
        connecting = peers.connecting;
        known = peers.seen;
    }
    // Own arithmetic: the engine's estimate is only available as text.
    if !stats.finished && down_speed > 0 {
        eta_secs = Some(stats.total_bytes.saturating_sub(stats.progress_bytes) / down_speed);
    }
    let file_count = handle
        .metadata
        .load()
        .as_ref()
        .map_or(0, |m| u32::try_from(m.file_infos.len()).unwrap_or(u32::MAX));
    Snapshot {
        state,
        name: handle.name(),
        size: stats.total_bytes,
        downloaded: stats.progress_bytes,
        uploaded: stats.uploaded_bytes,
        down_speed,
        up_speed,
        eta_secs,
        connected,
        connecting,
        known,
        file_count,
        complete: stats.finished,
        error: stats.error,
    }
}

/// The per-peer figures of the engine, through its JSON form: the types behind them are not
/// public, the shape is what the engine's own HTTP API sends.
fn read_peers(snapshot: &impl serde::Serialize) -> Vec<TorrentPeer> {
    let Ok(value) = serde_json::to_value(snapshot) else {
        return Vec::new();
    };
    let Some(peers) = value.get("peers").and_then(|p| p.as_object()) else {
        return Vec::new();
    };
    peers
        .iter()
        .map(|(address, p)| {
            let counters = p.get("counters");
            let num = |k: &str| {
                counters
                    .and_then(|c| c.get(k))
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0)
            };
            let state = match p.get("state").and_then(|s| s.as_str()).unwrap_or("") {
                "live" => PeerState::Connected,
                "connecting" => PeerState::Connecting,
                "queued" => PeerState::Queued,
                "not needed" => PeerState::Idle,
                _ => PeerState::Dead,
            };
            let transport = match p
                .get("conn_kind")
                .and_then(|k| k.as_str())
                .map(str::to_ascii_lowercase)
                .as_deref()
            {
                Some("tcp") => Some(PeerTransport::Tcp),
                Some("utp") => Some(PeerTransport::Utp),
                Some("socks") => Some(PeerTransport::Socks),
                _ => None,
            };
            TorrentPeer {
                address: address.clone(),
                state,
                transport,
                client: p
                    .get("client_name")
                    .and_then(|c| c.as_str())
                    .map(str::to_owned),
                downloaded: num("fetched_bytes"),
                uploaded: num("uploaded_bytes"),
                down_speed: 0,
                up_speed: 0,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn config(dir: &Path, name: &str) -> Config {
        Config {
            output_dir: dir.join(name).join("files"),
            state_dir: dir.join(name).join("state"),
            listen_port: 0,
            down_limit: 0,
            up_limit: 0,
            dht: false,
            client: "FetchBox/test".into(),
        }
    }

    /// A file and the `.torrent` made from it, in the seeder's folder.
    async fn make_torrent(seed: &TorrentEngine, dir: &Path, len: usize) -> (Vec<u8>, Bytes) {
        let content: Vec<u8> = (0..len).map(|i| (i * 31 % 251) as u8).collect();
        let folder = dir.join("seed").join("files");
        std::fs::create_dir_all(&folder).expect("folder");
        let path = folder.join("data.bin");
        std::fs::write(&path, &content).expect("write");
        let made = librqbit::create_torrent(
            &path,
            librqbit::CreateTorrentOptions {
                piece_length: Some(256 * 1024),
                ..Default::default()
            },
            &librqbit::spawn_utils::BlockingSpawner::new(2),
        )
        .await
        .expect("torrent");
        let bytes = made.as_bytes().expect("bytes");
        let _ = seed;
        (content, bytes)
    }

    async fn wait_for<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
        for _ in 0..200 {
            if let Some(v) = f() {
                return v;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("timed out waiting for: {what}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_torrent_goes_from_one_session_to_another() {
        let dir = tempfile::tempdir().expect("tmp");
        let seeder = TorrentEngine::start(config(dir.path(), "seed"))
            .await
            .expect("seeder");
        let (content, torrent) = make_torrent(&seeder, dir.path(), 3 * 1024 * 1024 + 123).await;

        // The seeder finds the file on disk, checks it and is complete at once.
        let added = seeder
            .add_file(torrent.clone(), false, vec![])
            .await
            .expect("seed add");
        let hash = added.info_hash.clone();
        let snap = wait_for("the seeder to be complete", || {
            seeder
                .snapshot(&hash)
                .filter(|s| s.complete && s.state == RunState::Live)
        })
        .await;
        assert_eq!(snap.size, content.len() as u64);
        assert_eq!(snap.file_count, 1);
        let seed_addr = SocketAddr::from(([127, 0, 0, 1], seeder.listen_port().expect("port")));
        // Slow enough that the transfer can be watched.
        seeder.set_limits(0, 400 * 1024);

        // The leecher starts from nothing and is pointed at the seeder.
        let leech = TorrentEngine::start(config(dir.path(), "leech"))
            .await
            .expect("leecher");
        let parsed = parse_torrent_file(&torrent).expect("parse");
        assert_eq!(parsed.info_hash, hash);
        assert_eq!(
            parsed.contents,
            Some(Contents {
                size: content.len() as u64,
                file_count: 1
            })
        );
        leech
            .add_file(torrent.clone(), false, vec![seed_addr])
            .await
            .expect("leech add");
        assert!(leech.contains(&hash));
        assert!(matches!(
            leech.add_file(torrent.clone(), false, vec![]).await,
            Err(TorrentError::Duplicate)
        ));

        // While it runs: the seeder is a connected peer that moves bytes at a measurable speed.
        let busy = wait_for("a peer that moves bytes", || {
            let p = leech.peers(&hash).ok()?;
            p.peers.iter().any(|x| x.down_speed > 0).then_some(p)
        })
        .await;
        let first = &busy.peers[0];
        assert_eq!(first.state, PeerState::Connected);
        assert!(
            first.address.ends_with(&format!(":{}", seed_addr.port())),
            "{}",
            first.address
        );
        assert!(
            first.downloaded > 0 && first.uploaded == 0,
            "bytes came from the seeder only"
        );
        assert!(
            first.down_speed < 2 * 1024 * 1024,
            "the limit holds: {}",
            first.down_speed
        );
        let mid = leech.snapshot(&hash).expect("there");
        assert!(
            !mid.complete && mid.connected >= 1 && mid.down_speed > 0,
            "{mid:?}"
        );
        assert!(mid.eta_secs.is_some());
        seeder.set_limits(0, 0);

        let done = wait_for("the download to finish", || {
            leech.snapshot(&hash).filter(|s| s.complete)
        })
        .await;
        assert_eq!(done.downloaded, content.len() as u64);
        assert_eq!(done.state, RunState::Live);
        let got = std::fs::read(dir.path().join("leech/files/data.bin")).expect("file");
        assert!(got == content, "the downloaded file differs");

        let files = leech.files(&hash).expect("files");
        assert_eq!(files.len(), 1);
        assert_eq!(
            (files[0].path.as_str(), files[0].size),
            ("data.bin", content.len() as u64)
        );
        assert_eq!(files[0].downloaded, files[0].size);

        // Stop, start, remove.
        leech.pause(&hash).await.expect("pause");
        let s = wait_for("the pause", || {
            leech
                .snapshot(&hash)
                .filter(|s| s.state == RunState::Stopped)
        })
        .await;
        assert!(s.complete, "stopping keeps what is done");
        leech.resume(&hash).await.expect("resume");
        wait_for("the resume", || {
            leech.snapshot(&hash).filter(|s| s.state == RunState::Live)
        })
        .await;
        leech.remove(&hash, true).await.expect("remove");
        assert!(!leech.contains(&hash));
        assert!(
            !dir.path().join("leech/files/data.bin").exists(),
            "its files went with it"
        );
        assert!(matches!(
            leech.remove(&hash, false).await,
            Err(TorrentError::NotFound)
        ));

        seeder.shutdown().await;
        leech.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_magnet_link_gets_its_description_from_a_peer() {
        let dir = tempfile::tempdir().expect("tmp");
        let seeder = TorrentEngine::start(config(dir.path(), "seed"))
            .await
            .expect("seeder");
        let (content, torrent) = make_torrent(&seeder, dir.path(), 700 * 1024).await;
        let hash = seeder
            .add_file(torrent, false, vec![])
            .await
            .expect("seed")
            .info_hash;
        wait_for("the seeder", || {
            seeder.snapshot(&hash).filter(|s| s.complete)
        })
        .await;
        let seed_addr = SocketAddr::from(([127, 0, 0, 1], seeder.listen_port().expect("port")));

        // All the leecher has is the link: the name, the size and the file come from the peer.
        let leech = TorrentEngine::start(config(dir.path(), "leech"))
            .await
            .expect("leecher");
        let uri = format!("magnet:?xt=urn:btih:{hash}&dn=from-the-link");
        assert_eq!(parse_magnet(&uri).expect("parse").info_hash, hash);
        let added = leech
            .add_magnet(&uri, false, vec![seed_addr])
            .await
            .expect("magnet");
        assert_eq!(added.info_hash, hash);
        assert!(
            !added.metainfo.is_empty(),
            "the description that arrived is handed back"
        );
        assert_eq!(
            parse_torrent_file(&added.metainfo)
                .expect("a real .torrent")
                .info_hash,
            hash
        );

        wait_for("the download", || {
            leech.snapshot(&hash).filter(|s| s.complete)
        })
        .await;
        let got = std::fs::read(dir.path().join("leech/files/data.bin")).expect("file");
        assert!(got == content);
        seeder.shutdown().await;
        leech.shutdown().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_session_comes_back_with_its_torrents() {
        let dir = tempfile::tempdir().expect("tmp");
        let first = TorrentEngine::start(config(dir.path(), "seed"))
            .await
            .expect("start");
        let (_, torrent) = make_torrent(&first, dir.path(), 600 * 1024).await;
        let hash = first
            .add_file(torrent, true, vec![])
            .await
            .expect("add")
            .info_hash;
        assert_eq!(
            first.snapshot(&hash).expect("there").state,
            RunState::Stopped
        );
        first.shutdown().await;
        drop(first);

        let again = TorrentEngine::start(config(dir.path(), "seed"))
            .await
            .expect("restart");
        wait_for("the torrent to be restored", || {
            again.contains(&hash).then_some(())
        })
        .await;
        assert_eq!(again.hashes(), vec![hash.clone()]);
        assert_eq!(
            again.snapshot(&hash).expect("there").state,
            RunState::Stopped,
            "paused stays paused"
        );
        assert!(again.metainfo(&hash).is_some());
        // A torrent that came back stopped can be started like any other.
        again.resume(&hash).await.expect("resume");
        wait_for("the restored torrent to run", || {
            again.snapshot(&hash).filter(|s| s.state == RunState::Live)
        })
        .await;
        again.shutdown().await;
    }

    #[tokio::test]
    async fn the_limits_are_clamped_to_what_the_engine_takes() {
        assert_eq!(non_zero(0), None);
        assert_eq!(non_zero(1_000_000).map(NonZeroU32::get), Some(1_000_000));
        assert_eq!(non_zero(u64::MAX).map(NonZeroU32::get), Some(u32::MAX));
    }
}
