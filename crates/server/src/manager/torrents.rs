//! BitTorrent in the manager (SPEC 6.13).
//!
//! The database says which torrents exist; the engine (`fetchbox_torrent_engine`) does the
//! transfers and says what they are doing. Once a second a tick joins the two into the list the UI
//! shows and publishes what changed. The engine is started on first use (or at start-up when torrents
//! are stored), so nobody who never adds a torrent has a port open or a DHT node running.

use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

use bytes::Bytes;
use fetchbox_core::disks::rel_dir;
use fetchbox_core::filename::sanitize;
use fetchbox_core::{
    AddTorrentRequest, Error, Event, Hold, Id, Result, Settings, Torrent, TorrentFile,
    TorrentPeers, TorrentStatus, MAX_MAGNET_LEN, MAX_TORRENT_FILE,
};
use fetchbox_storage::torrents::{self as store, NewTorrent, StoredTorrent};
use fetchbox_torrent_engine::{
    parse_magnet, parse_torrent_file, Config, RunState, Snapshot, TorrentEngine, TorrentError,
};
use tokio::task::AbortHandle;

use super::Manager;
use crate::clock;
use crate::logfmt;
use crate::update;

/// Where the engine keeps its state and which port it listens on (set once at start-up).
#[derive(Debug, Clone)]
pub struct TorrentSetup {
    /// The data directory; the engine uses `torrents/` in it.
    pub state_dir: PathBuf,
    /// TCP and uTP port for incoming peers; `0` picks one.
    pub listen_port: u16,
    /// Look for peers in the DHT. Off only in tests.
    pub dht: bool,
}

/// What the manager remembers about torrents between ticks.
#[derive(Default)]
pub(crate) struct Torrents {
    setup: OnceLock<TorrentSetup>,
    engine: Mutex<Option<Arc<TorrentEngine>>>,
    /// Why the engine could not start, while that is the case.
    start_error: Mutex<Option<String>>,
    /// Serialises starting the engine and adding torrents (the duplicate check and the insert).
    gate: tokio::sync::Mutex<()>,
    /// Magnet links whose description is still being fetched: they are not in the engine yet.
    fetching: Mutex<HashMap<Id, AbortHandle>>,
    /// Bytes sent in earlier runs plus the engine's own counter (see [`Sent`]).
    sent: Mutex<HashMap<Id, Sent>>,
    /// The list as last published, to publish only what changed.
    last: Mutex<HashMap<Id, Torrent>>,
    /// When the sent counters were last written to the database.
    persisted: Mutex<Option<Instant>>,
    /// Torrents the torrent hours keep stopped (SPEC 6.3.2): the user did not stop them, so they
    /// start again when the hours open. Not stored: the clock decides it afresh at every start.
    held: Mutex<HashSet<Id>>,
    /// Whether the torrent hours were last seen open (`None`: switched off), to log a change once.
    pub(super) hours_seen: Mutex<Option<bool>>,
}

/// The engine counts what a torrent sent since it started it, and starts again from zero when a
/// torrent is stopped and started: the total is what earlier runs sent plus the current counter.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Sent {
    base: u64,
    last: u64,
}

impl Sent {
    fn total(&mut self, engine_counter: u64) -> u64 {
        if engine_counter < self.last {
            // The counter started over: what it had counted belongs to the past now.
            self.base = self.base.saturating_add(self.last);
        }
        self.last = engine_counter;
        self.base.saturating_add(engine_counter)
    }
}

/// How often the bytes sent are written to the database (they are not worth a write per second).
const PERSIST_EVERY: Duration = Duration::from_secs(15);

/// The list entry for a stored torrent, given what the engine says about it (`None`: it is not in
/// the engine, yet or any more).
fn view(
    row: &StoredTorrent,
    snap: Option<&Snapshot>,
    fetching: bool,
    start_error: Option<&str>,
    uploaded: u64,
    held: Option<Option<i64>>,
) -> Torrent {
    let stopped = |complete: bool| TorrentStatus::derive_stopped(complete);
    let status = match snap {
        Some(s) => match s.state {
            RunState::Checking => TorrentStatus::Checking,
            RunState::Live => TorrentStatus::derive_live(s.complete, s.connected),
            // A note on a stopped torrent (a full disk) is an error, not a pause.
            RunState::Stopped if row.error.is_some() => TorrentStatus::Error,
            RunState::Stopped => stopped(s.complete),
            RunState::Error => TorrentStatus::Error,
        },
        None if row.error.is_some() || start_error.is_some() => TorrentStatus::Error,
        None if row.paused => stopped(row.finished_at.is_some()),
        None if fetching || !row.has_metainfo => TorrentStatus::Metadata,
        None => TorrentStatus::Checking,
    };
    // Stopped by the torrent hours: shown as paused, with the time the hours open.
    let status = if held.is_some() && status != TorrentStatus::Error {
        TorrentStatus::Paused
    } else {
        status
    };
    let name = snap
        .and_then(|s| s.name.clone())
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| row.name.clone());
    let size = snap
        .map(|s| s.size)
        .filter(|s| *s > 0)
        .or(row.size)
        .filter(|s| *s > 0 || row.has_metainfo);
    // Speeds only mean something while the torrent moves data that way: a complete torrent
    // receives nothing, whatever the engine's estimate says in the moment it finishes.
    let receiving = matches!(
        status,
        TorrentStatus::Downloading | TorrentStatus::Connecting
    );
    let sending = receiving || status == TorrentStatus::Seeding;
    Torrent {
        id: row.id,
        info_hash: row.info_hash.clone(),
        name,
        status,
        size,
        downloaded: snap.map_or_else(
            || {
                if row.finished_at.is_some() {
                    row.size.unwrap_or(0)
                } else {
                    0
                }
            },
            |s| s.downloaded,
        ),
        uploaded,
        down_speed: snap.filter(|_| receiving).map_or(0, |s| s.down_speed),
        up_speed: snap.filter(|_| sending).map_or(0, |s| s.up_speed),
        eta: snap.filter(|_| receiving).and_then(|s| s.eta_secs),
        peers_connected: snap.map_or(0, |s| s.connected),
        peers_connecting: snap.map_or(0, |s| s.connecting),
        peers_known: snap.map_or(0, |s| s.known),
        file_count: snap
            .map(|s| s.file_count)
            .filter(|c| *c > 0)
            .unwrap_or(row.file_count),
        magnet: row.magnet.clone(),
        added_at: row.added_at,
        finished_at: row.finished_at,
        error: snap
            .and_then(|s| s.error.clone())
            .or_else(|| row.error.clone())
            .or_else(|| start_error.map(str::to_owned)),
        hold: held.map(|_| Hold::Window),
        starts_at: held.flatten(),
        start_now: row.start_now,
        disk: row.disk.clone(),
    }
}

/// Where the engine saves a torrent on `disk`, relative to the download directory; empty for the
/// download directory itself, where the engine's own folder (`torrents/`) is used as before.
///
/// On a disk it is `<disk>/torrents`, and the files go straight into the folder the engine is
/// given, so `folder` says whether the torrent gets one of its own: the engine does that by itself
/// for a torrent of several files (a folder named after it), not for one file. A `.torrent` file
/// tells which it is. A magnet link does not until its description has arrived, so it always gets a
/// folder, named after the link (or its hash): the layout cannot change once the files are there.
fn torrent_output_rel(disk: &str, folder: Option<&str>) -> String {
    if disk.is_empty() {
        return String::new();
    }
    let base = rel_dir(disk, "torrents");
    match folder {
        Some(name) => format!("{base}/{}", sanitize(name)),
        None => base,
    }
}

impl Manager {
    /// Switch BitTorrent on: the once-a-second tick starts, and, when torrents are stored, the engine
    /// comes up in the background to take them back. Called once, at start-up.
    pub async fn enable_torrents(self: &Arc<Self>, setup: TorrentSetup) {
        if self.torrents.setup.set(setup).is_err() {
            return;
        }
        let weak: Weak<Manager> = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(1));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tick.tick().await;
                let Some(me) = weak.upgrade() else { break };
                if *me.closing.borrow() {
                    break;
                }
                me.torrent_tick().await;
            }
        });
        let stored = self
            .with_db(|c| store::list(c))
            .map(|l| l.len())
            .unwrap_or(0);
        if stored > 0 {
            tracing::info!(torrents = stored, "restoring torrents");
            let me = Arc::clone(self);
            tokio::spawn(async move {
                if let Err(e) = me.torrent_engine().await {
                    tracing::warn!(error = %e, "the BitTorrent engine did not start");
                }
            });
        }
    }

    fn engine_now(&self) -> Option<Arc<TorrentEngine>> {
        self.torrents
            .engine
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn fetching(&self) -> std::sync::MutexGuard<'_, HashMap<Id, AbortHandle>> {
        self.torrents
            .fetching
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    fn torrent_held(&self) -> std::sync::MutexGuard<'_, HashSet<Id>> {
        self.torrents.held.lock().unwrap_or_else(|e| e.into_inner())
    }

    // ---- torrent hours (SPEC 6.3.2) -------------------------------------------------------------

    /// Is the local time inside the torrent hours? Without hours (or with a zone nobody can read,
    /// which `update_settings` refuses) it is always "open".
    fn torrent_window_open(&self, s: &Settings, now: i64) -> bool {
        if !s.torrent_schedule_enabled {
            return true;
        }
        match clock::zone(&s.torrent_schedule_timezone)
            .and_then(|tz| clock::minute_of_day(now, &tz))
        {
            Some(minute) => s.torrent_schedule_window().contains(minute),
            None => {
                tracing::warn!(zone = %s.torrent_schedule_timezone, "unknown time zone for the torrent hours");
                true
            }
        }
    }

    /// The next moment the torrent hours open, as unix seconds.
    fn torrent_next_opening(&self, s: &Settings, now: i64) -> Option<i64> {
        if !s.torrent_schedule_enabled {
            return None;
        }
        let tz = clock::zone(&s.torrent_schedule_timezone)?;
        clock::next_at(now, &tz, s.torrent_schedule_from)
    }

    /// Do the torrent hours keep this torrent stopped right now? If so it is marked as held. A
    /// torrent the user stopped is not held (their stop is the stronger one), nor one that was
    /// told to start now.
    fn torrent_hold(&self, row: &StoredTorrent) -> bool {
        if row.paused || row.start_now {
            return false;
        }
        let closed = !self.torrent_window_open(&self.current_settings(), clock::now());
        if closed {
            self.torrent_held().insert(row.id);
        }
        closed
    }

    /// Say when the torrent hours open or close (once per change, not every check).
    fn log_torrent_hours(&self, s: &Settings, now: i64) {
        let seen = s
            .torrent_schedule_enabled
            .then(|| self.torrent_window_open(s, now));
        let mut last = self
            .torrents
            .hours_seen
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if *last == seen {
            return;
        }
        *last = seen;
        match seen {
            Some(true) => tracing::info!(target: "activity",
                hours = %format!("{}-{} {}", logfmt::clock(s.torrent_schedule_from), logfmt::clock(s.torrent_schedule_to), s.torrent_schedule_timezone),
                "torrent hours are open: torrents may run"
            ),
            Some(false) => tracing::info!(target: "activity",
                opens = %self.torrent_next_opening(s, now).map_or_else(String::new, |at| clock::describe(at, &s.torrent_schedule_timezone)),
                "torrent hours are closed: torrents are stopped"
            ),
            None => {}
        }
    }

    /// Once a second: stop the torrents the hours no longer allow and start the ones they let go
    /// again. What the user stopped stays stopped.
    async fn torrent_hours_pass(&self, rows: &[StoredTorrent]) {
        let s = self.current_settings();
        let now = clock::now();
        self.log_torrent_hours(&s, now);
        let open = self.torrent_window_open(&s, now);
        let (mut stop, mut release) = (Vec::new(), Vec::new());
        {
            let mut held = self.torrent_held();
            held.retain(|id| rows.iter().any(|r| r.id == *id && !r.paused));
            for row in rows.iter().filter(|r| !r.paused) {
                let should_hold = !open && !row.start_now;
                match (should_hold, held.contains(&row.id)) {
                    (true, false) => {
                        held.insert(row.id);
                        stop.push(row);
                    }
                    (false, true) => {
                        held.remove(&row.id);
                        release.push(row);
                    }
                    _ => {}
                }
            }
        }
        for row in stop {
            self.torrent_stop_engine(row).await;
            tracing::info!(target: "activity", id = row.id, name = %row.name, "torrent stopped by the torrent hours");
        }
        for row in release.into_iter().filter(|r| r.error.is_none()) {
            self.torrent_start_engine(row).await;
            tracing::info!(target: "activity", id = row.id, name = %row.name, "torrent started: the torrent hours are open");
        }
    }

    /// Stop a torrent in the engine without touching what the user chose (`paused`).
    async fn torrent_stop_engine(&self, row: &StoredTorrent) {
        if let Some(task) = self.fetching().remove(&row.id) {
            task.abort(); // a magnet link still waiting for its description
        }
        if let Some(engine) = self.engine_now() {
            if engine.contains(&row.info_hash) {
                if let Err(e) = engine.pause(&row.info_hash).await {
                    tracing::warn!(id = row.id, error = %e, "cannot stop a torrent");
                }
            }
        }
        self.persist_sent(true);
    }

    /// Start a stored torrent in the engine again (it is already there, or has to be added).
    async fn torrent_start_engine(&self, row: &StoredTorrent) {
        let engine = match self.torrent_engine().await {
            Ok(engine) => engine,
            Err(e) => {
                tracing::warn!(id = row.id, error = %e, "cannot start a torrent");
                return;
            }
        };
        if engine.contains(&row.info_hash) {
            if let Err(e) = engine.resume(&row.info_hash).await {
                tracing::warn!(id = row.id, error = %e, "cannot start a torrent");
            }
        } else {
            self.torrent_readd(&engine, row).await;
        }
    }

    /// "Start now": the torrent hours do not apply to this torrent, until the user stops it.
    pub(crate) async fn torrent_start_now(&self, id: Id) -> Result<Torrent> {
        let row = self.torrent_row(id)?;
        if row.paused {
            return Err(Error::InvalidState(
                "the torrent is stopped: start it first".into(),
            ));
        }
        self.with_db(|c| store::set_start_now(c, id, true))?;
        if self.torrent_held().remove(&id) {
            let fresh = self.torrent_row(id)?;
            self.torrent_start_engine(&fresh).await;
        }
        tracing::info!(target: "activity", id, name = %row.name, "torrent: start now, the torrent hours do not apply");
        self.publish_torrent(id);
        self.torrent_now(id)
    }

    /// Where the torrents of a disk are saved (the engine's own folder for the download directory).
    fn torrent_dir_of(&self, disk: &str) -> PathBuf {
        self.dir.join(rel_dir(disk, "torrents"))
    }

    /// The folder the engine is told to save a stored torrent in; none for its own default.
    fn torrent_output(&self, row: &StoredTorrent) -> Option<PathBuf> {
        (!row.output.is_empty()).then(|| self.dir.join(&row.output))
    }

    fn torrent_dir(&self) -> PathBuf {
        self.dir.join("torrents")
    }

    /// The engine, started now if it is not running. A failed start is remembered (the list shows
    /// it) and tried again the next time.
    async fn torrent_engine(&self) -> Result<Arc<TorrentEngine>> {
        if let Some(engine) = self.engine_now() {
            return Ok(engine);
        }
        let Some(setup) = self.torrents.setup.get() else {
            return Err(Error::Engine("BitTorrent is not switched on".into()));
        };
        let _gate = self.torrents.gate.lock().await;
        self.torrent_engine_locked(setup).await
    }

    /// Like [`Self::torrent_engine`] for a caller that already holds the gate.
    async fn torrent_engine_locked(&self, setup: &TorrentSetup) -> Result<Arc<TorrentEngine>> {
        if let Some(engine) = self.engine_now() {
            return Ok(engine);
        }
        let s = self.current_settings();
        let cfg = Config {
            output_dir: self.torrent_dir(),
            state_dir: setup.state_dir.join("torrents"),
            listen_port: setup.listen_port,
            down_limit: s.torrent_down_limit,
            up_limit: s.torrent_up_limit,
            dht: setup.dht,
            client: format!("FetchBox/{}", update::CURRENT),
        };
        let engine = match TorrentEngine::start(cfg).await {
            Ok(engine) => engine,
            Err(e) => {
                *self
                    .torrents
                    .start_error
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) = Some(e.to_string());
                return Err(e.into());
            }
        };
        *self
            .torrents
            .start_error
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
        tracing::info!(port = ?engine.listen_port(), dht = setup.dht, "BitTorrent engine started");
        *self
            .torrents
            .engine
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(engine.clone());
        self.reconcile_torrents(&engine).await;
        Ok(engine)
    }

    /// Make the engine's torrents the stored ones: what is stored and missing is added again, what
    /// the engine has without a record is forgotten (its files stay).
    async fn reconcile_torrents(&self, engine: &Arc<TorrentEngine>) {
        let rows = match self.with_db(|c| store::list(c)) {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!(error = %e, "cannot list torrents");
                return;
            }
        };
        let known: HashSet<&str> = rows.iter().map(|r| r.info_hash.as_str()).collect();
        for hash in engine.hashes() {
            if !known.contains(hash.as_str()) {
                tracing::warn!(%hash, "the engine has a torrent that is not in the list: forgetting it");
                let _ = engine.remove(&hash, false).await;
            }
        }
        for row in &rows {
            // A torrent on a disk that is not mounted is stopped with a note instead of being
            // written to the folder that stands in its place on the system disk. (The engine may
            // have restored it already; it only checks the files, so little can have happened.)
            if let Err(e) = self.disk_dir(&row.disk) {
                if engine.contains(&row.info_hash) {
                    let _ = engine.pause(&row.info_hash).await;
                }
                self.torrent_failed(row.id, e.to_string());
                continue;
            }
            if engine.contains(&row.info_hash) {
                // The engine restored it; what the user last chose decides if it runs.
                let _ = if row.paused || self.torrent_hold(row) {
                    engine.pause(&row.info_hash).await
                } else {
                    engine.resume(&row.info_hash).await
                };
            } else {
                self.torrent_readd(engine, row).await;
            }
        }
    }

    /// Give a stored torrent to the engine again (after a restart that lost it, or a resume).
    async fn torrent_readd(&self, engine: &Arc<TorrentEngine>, row: &StoredTorrent) {
        if let Err(e) = self.disk_dir(&row.disk) {
            self.torrent_failed(row.id, e.to_string());
            return;
        }
        if row.has_metainfo {
            let bytes = match self.with_db(|c| store::metainfo(c, row.id)) {
                Ok(Some(b)) => Bytes::from(b),
                _ => {
                    self.torrent_failed(row.id, "the stored .torrent file is missing".into());
                    return;
                }
            };
            let stopped = row.paused || self.torrent_hold(row);
            if let Err(e) = engine
                .add_file_in(bytes, stopped, vec![], self.torrent_output(row))
                .await
            {
                if !matches!(e, TorrentError::Duplicate) {
                    self.torrent_failed(row.id, e.to_string());
                }
            }
        } else if let Some(magnet) = &row.magnet {
            // Its description has to come from the swarm again; a stopped one waits for "resume",
            // one held by the torrent hours for the hours.
            if !row.paused && !self.torrent_hold(row) {
                self.fetch_magnet(row.id, magnet.clone(), self.torrent_output(row));
            }
        } else {
            self.torrent_failed(row.id, "nothing to restore it from".into());
        }
    }

    /// Fetch the description of a magnet link in the background and, once it is there, finish the
    /// record. The task is kept so that pausing or removing the torrent can cancel it.
    fn fetch_magnet(&self, id: Id, magnet: String, output: Option<PathBuf>) {
        let Some(me) = self.this.get().and_then(Weak::upgrade) else {
            return;
        };
        let mut fetching = self.fetching();
        if fetching.contains_key(&id) {
            return;
        }
        let task = tokio::spawn({
            let me = Arc::clone(&me);
            async move {
                let outcome = match me.engine_now() {
                    Some(engine) => engine
                        .add_magnet_in(&magnet, false, vec![], output)
                        .await
                        .map(|added| (engine, added.info_hash)),
                    None => Err(TorrentError::Engine(
                        "the BitTorrent engine is not running".into(),
                    )),
                };
                me.fetching().remove(&id);
                match outcome {
                    Ok((engine, hash)) => me.torrent_described(id, &engine, &hash).await,
                    Err(e) => me.torrent_failed(id, e.to_string()),
                }
            }
        });
        fetching.insert(id, task.abort_handle());
    }

    /// The description of a torrent is known: store it, and stop the torrent when it cannot fit.
    async fn torrent_described(&self, id: Id, engine: &Arc<TorrentEngine>, hash: &str) {
        let Some(snap) = engine.snapshot(hash) else {
            return;
        };
        if snap.file_count == 0 {
            return;
        }
        let name = snap.name.clone().unwrap_or_else(|| hash.to_owned());
        let metainfo = engine.metainfo(hash);
        let stored = self.with_db(|c| {
            store::set_description(
                c,
                id,
                &name,
                snap.size,
                snap.file_count,
                metainfo.as_deref(),
            )
        });
        if let Err(e) = stored {
            tracing::warn!(id, error = %e, "cannot store the description of a torrent");
        }
        let disk = self.torrent_row(id).map(|r| r.disk).unwrap_or_default();
        if let Some(missing) =
            self.torrent_space_problem(&disk, snap.size.saturating_sub(snap.downloaded))
        {
            tracing::warn!(target: "activity", id, name = %name, "{missing}");
            let _ = engine.pause(hash).await;
            let _ = self.with_db(|c| {
                store::set_paused(c, id, true)?;
                store::set_error(c, id, Some(&missing))
            });
        }
        tracing::info!(target: "activity", id, name = %name, size = snap.size, files = snap.file_count, "torrent description received");
        self.publish_torrent(id);
    }

    /// Why `needed` more bytes do not fit where torrents are saved, or `None` when they do (or the
    /// disk cannot be asked).
    fn torrent_space_problem(&self, disk: &str, needed: u64) -> Option<String> {
        let vol = fetchbox_http_engine::disk::volume(&self.torrent_dir_of(disk)).ok()?;
        let reserved = self.reserved_on(vol.dev).ok()?;
        let free = vol.space.available.saturating_sub(reserved);
        (needed > free)
            .then(|| fetchbox_core::error::no_space_message(needed, vol.space.available, reserved))
    }

    fn torrent_failed(&self, id: Id, message: String) {
        tracing::warn!(target: "activity", id, error = %message, "torrent failed");
        let _ = self.with_db(|c| store::set_error(c, id, Some(&message)));
        self.publish_torrent(id);
    }

    /// Refuse trackers at private addresses while `--block-private-urls` is on: a tracker is a URL
    /// the server would fetch for whoever adds the torrent. Names are resolved here (a name that
    /// changes its answer later is not caught: peers cannot be filtered either).
    async fn check_trackers(&self, trackers: &[String]) -> Result<()> {
        let policy = self.url_policy;
        if !policy.block_private {
            return Ok(());
        }
        for tracker in trackers {
            let Ok(url) = reqwest::Url::parse(tracker) else {
                continue;
            };
            let host = url.host_str().unwrap_or("?").to_owned();
            let blocked = Error::InvalidInput(format!(
                "the torrent names a tracker at a private address, which is blocked: {host}"
            ));
            if policy.check(&url).is_err() {
                return Err(blocked);
            }
            if host.trim_matches(['[', ']']).parse::<IpAddr>().is_ok() || host == "?" {
                continue;
            }
            let port = url.port_or_known_default().unwrap_or(80);
            let resolved = tokio::net::lookup_host((host.as_str(), port)).await;
            if let Ok(addrs) = resolved {
                let addrs: Vec<_> = addrs.collect();
                if addrs.iter().any(|a| !policy.allows(a.ip())) {
                    return Err(blocked);
                }
            }
        }
        Ok(())
    }

    // ---- what the API asks for ------------------------------------------------------------------

    pub(crate) async fn torrents_list(&self) -> Result<Vec<Torrent>> {
        let rows = self.with_db(|c| store::list(c))?;
        let engine = self.engine_now();
        let snaps: HashMap<String, Snapshot> = engine
            .map(|e| e.snapshots().into_iter().collect())
            .unwrap_or_default();
        Ok(rows
            .iter()
            .map(|r| self.torrent_view_of(r, snaps.get(&r.info_hash)))
            .collect())
    }

    fn torrent_view_of(&self, row: &StoredTorrent, snap: Option<&Snapshot>) -> Torrent {
        let fetching = self.fetching().contains_key(&row.id);
        let start_error = self
            .torrents
            .start_error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let uploaded = {
            let mut sent = self.torrents.sent.lock().unwrap_or_else(|e| e.into_inner());
            let entry = sent.entry(row.id).or_insert(Sent {
                base: row.uploaded,
                last: 0,
            });
            entry.total(snap.map_or(0, |s| s.uploaded))
        };
        let held = (!row.paused && self.torrent_held().contains(&row.id))
            .then(|| self.torrent_next_opening(&self.current_settings(), clock::now()));
        view(row, snap, fetching, start_error.as_deref(), uploaded, held)
    }

    fn publish_torrent(&self, id: Id) {
        let Ok(Some(row)) = self.with_db(|c| store::get(c, id)) else {
            return;
        };
        let snap = self.engine_now().and_then(|e| e.snapshot(&row.info_hash));
        let torrent = self.torrent_view_of(&row, snap.as_ref());
        self.torrents
            .last
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, torrent.clone());
        self.publish(Event::Torrent {
            torrent: Box::new(torrent),
        });
    }

    pub(crate) async fn torrent_add_magnet(&self, req: AddTorrentRequest) -> Result<Torrent> {
        let uri = req.magnet.trim().to_owned();
        if !fetchbox_core::torrent::is_magnet(&uri) {
            return Err(Error::InvalidInput("this is not a magnet link".into()));
        }
        if uri.len() > MAX_MAGNET_LEN {
            return Err(Error::InvalidInput("the magnet link is too long".into()));
        }
        let parsed = parse_magnet(&uri)?;
        self.check_trackers(&parsed.trackers).await?;
        let disk = req
            .disk
            .clone()
            .unwrap_or_else(|| self.current_settings().torrent_disk().to_owned());
        self.disk_dir(&disk)?;
        let output = torrent_output_rel(
            &disk,
            Some(parsed.name.as_deref().unwrap_or(&parsed.info_hash)),
        );
        let Some(setup) = self.torrents.setup.get() else {
            return Err(Error::Engine("BitTorrent is not switched on".into()));
        };
        let _gate = self.torrents.gate.lock().await;
        if self
            .with_db(|c| store::find_by_hash(c, &parsed.info_hash))?
            .is_some()
        {
            return Err(TorrentError::Duplicate.into());
        }
        // The engine first: a start that fails leaves nothing behind in the list.
        self.torrent_engine_locked(setup).await?;
        let name = parsed
            .name
            .clone()
            .unwrap_or_else(|| parsed.info_hash.clone());
        let row = self.with_db(|c| {
            store::insert(
                c,
                &NewTorrent {
                    info_hash: &parsed.info_hash,
                    name: &name,
                    magnet: Some(&uri),
                    metainfo: None,
                    size: None,
                    file_count: 0,
                    disk: &disk,
                    output: &output,
                },
            )
        })?;
        tracing::info!(target: "activity", id = row.id, name = %name, disk = %logfmt::disk_label(&disk), "torrent added (magnet link)");
        if !self.torrent_hold(&row) {
            self.fetch_magnet(row.id, uri, self.torrent_output(&row));
        }
        self.publish_torrent(row.id);
        self.torrent_now(row.id)
    }

    pub(crate) async fn torrent_add_file(
        &self,
        bytes: Vec<u8>,
        disk: Option<String>,
    ) -> Result<Torrent> {
        if bytes.is_empty() {
            return Err(Error::InvalidInput("the .torrent file is empty".into()));
        }
        if bytes.len() > MAX_TORRENT_FILE {
            return Err(Error::InvalidInput("the .torrent file is too large".into()));
        }
        let parsed = parse_torrent_file(&bytes)?;
        self.check_trackers(&parsed.trackers).await?;
        let contents = parsed.contents;
        let disk = disk.unwrap_or_else(|| self.current_settings().torrent_disk().to_owned());
        self.disk_dir(&disk)?;
        let name_for_folder = parsed
            .name
            .clone()
            .unwrap_or_else(|| parsed.info_hash.clone());
        // One file goes straight into the folder; several get a folder named after the torrent.
        let own_folder = contents.is_none_or(|c| c.file_count > 1);
        let output = torrent_output_rel(&disk, own_folder.then_some(name_for_folder.as_str()));
        let Some(setup) = self.torrents.setup.get() else {
            return Err(Error::Engine("BitTorrent is not switched on".into()));
        };
        let _gate = self.torrents.gate.lock().await;
        if self
            .with_db(|c| store::find_by_hash(c, &parsed.info_hash))?
            .is_some()
        {
            return Err(TorrentError::Duplicate.into());
        }
        if let Some(c) = contents {
            if let Ok(vol) = fetchbox_http_engine::disk::volume(&self.torrent_dir_of(&disk)) {
                let reserved = self.reserved_on(vol.dev)?;
                if c.size > vol.space.available.saturating_sub(reserved) {
                    return Err(Error::InsufficientSpace {
                        needed: c.size,
                        free: vol.space.available,
                        reserved,
                    });
                }
            }
        }
        let engine = self.torrent_engine_locked(setup).await?;
        let name = parsed
            .name
            .clone()
            .unwrap_or_else(|| parsed.info_hash.clone());
        let row = self.with_db(|c| {
            store::insert(
                c,
                &NewTorrent {
                    info_hash: &parsed.info_hash,
                    name: &name,
                    magnet: None,
                    metainfo: Some(&bytes),
                    size: contents.map(|c| c.size),
                    file_count: contents.map_or(0, |c| c.file_count),
                    disk: &disk,
                    output: &output,
                },
            )
        })?;
        let held = self.torrent_hold(&row);
        if let Err(e) = engine
            .add_file_in(Bytes::from(bytes), held, vec![], self.torrent_output(&row))
            .await
        {
            let _ = self.with_db(|c| store::remove(c, row.id));
            return Err(e.into());
        }
        tracing::info!(target: "activity", id = row.id, name = %name, size = contents.map_or(0, |c| c.size), disk = %logfmt::disk_label(&disk), "torrent added (.torrent file)");
        self.publish_torrent(row.id);
        self.torrent_now(row.id)
    }

    fn torrent_row(&self, id: Id) -> Result<StoredTorrent> {
        self.with_db(|c| store::get(c, id))?
            .ok_or_else(|| TorrentError::NotFound.into())
    }

    /// The list entry of one torrent as it is right now.
    fn torrent_now(&self, id: Id) -> Result<Torrent> {
        let row = self.torrent_row(id)?;
        let snap = self.engine_now().and_then(|e| e.snapshot(&row.info_hash));
        Ok(self.torrent_view_of(&row, snap.as_ref()))
    }

    pub(crate) async fn torrent_pause(&self, id: Id) -> Result<Torrent> {
        let row = self.torrent_row(id)?;
        if let Some(task) = self.fetching().remove(&id) {
            task.abort(); // a magnet link still waiting for its description
        }
        if let Some(engine) = self.engine_now() {
            if engine.contains(&row.info_hash) {
                engine.pause(&row.info_hash).await?;
            }
        }
        // The user's stop ends a "start now" too: the hours decide again when they start it.
        self.with_db(|c| {
            store::set_paused(c, id, true)?;
            store::set_start_now(c, id, false)
        })?;
        self.torrent_held().remove(&id);
        self.persist_sent(true);
        tracing::info!(target: "activity", id, name = %row.name, "torrent stopped");
        self.publish_torrent(id);
        self.torrent_now(id)
    }

    pub(crate) async fn torrent_resume(&self, id: Id) -> Result<Torrent> {
        let row = self.torrent_row(id)?;
        // A disk that is not mounted would be filled in its place: it has to be there to start.
        self.disk_dir(&row.disk)?;
        self.with_db(|c| {
            store::set_paused(c, id, false)?;
            store::set_error(c, id, None)
        })?;
        // Not enough room is checked again now: the user may have made some.
        if let Some(size) = row.size {
            let left = size.saturating_sub(self.torrent_now(id)?.downloaded);
            if let Some(problem) = self.torrent_space_problem(&row.disk, left) {
                self.with_db(|c| {
                    store::set_paused(c, id, true)?;
                    store::set_error(c, id, Some(&problem))
                })?;
                self.publish_torrent(id);
                return Err(Error::InsufficientSpace {
                    needed: left,
                    free: fetchbox_http_engine::disk::volume(&self.torrent_dir_of(&row.disk))
                        .map(|v| v.space.available)
                        .unwrap_or(0),
                    reserved: 0,
                });
            }
        }
        let engine = self.torrent_engine().await?;
        // Outside the torrent hours it waits for them instead of starting.
        let fresh = self.torrent_row(id)?;
        if self.torrent_hold(&fresh) {
            tracing::info!(target: "activity", id, name = %row.name, "torrent waits for the torrent hours");
            self.publish_torrent(id);
            return self.torrent_now(id);
        }
        if engine.contains(&row.info_hash) {
            engine.resume(&row.info_hash).await?;
        } else {
            let fresh = self.torrent_row(id)?;
            self.torrent_readd(&engine, &fresh).await;
        }
        tracing::info!(target: "activity", id, name = %row.name, "torrent started");
        self.publish_torrent(id);
        self.torrent_now(id)
    }

    pub(crate) async fn torrent_remove(&self, id: Id, delete_files: bool) -> Result<()> {
        let row = self.torrent_row(id)?;
        if let Some(task) = self.fetching().remove(&id) {
            task.abort();
        }
        if let Some(engine) = self.engine_now() {
            if engine.contains(&row.info_hash) {
                engine.remove(&row.info_hash, delete_files).await?;
            }
        }
        self.with_db(|c| store::remove(c, id))?;
        self.torrent_held().remove(&id);
        self.torrents
            .sent
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
        self.torrents
            .last
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
        tracing::info!(target: "activity", id, name = %row.name, files_deleted = delete_files, "torrent removed");
        self.publish(Event::TorrentRemoved { id });
        Ok(())
    }

    pub(crate) async fn torrent_peers_of(&self, id: Id) -> Result<TorrentPeers> {
        let row = self.torrent_row(id)?;
        match self.engine_now() {
            Some(engine) if engine.contains(&row.info_hash) => Ok(engine.peers(&row.info_hash)?),
            _ => Ok(TorrentPeers::default()),
        }
    }

    pub(crate) async fn torrent_files_of(&self, id: Id) -> Result<Vec<TorrentFile>> {
        let row = self.torrent_row(id)?;
        Ok(self
            .engine_now()
            .and_then(|e| e.files(&row.info_hash))
            .unwrap_or_default())
    }

    /// New speed limits for all torrents (called when the settings are saved).
    pub(crate) fn apply_torrent_limits(&self, s: &Settings) {
        if let Some(engine) = self.engine_now() {
            engine.set_limits(s.torrent_down_limit, s.torrent_up_limit);
        }
    }

    /// Write the bytes sent to the database (`force`: now, otherwise at most every `PERSIST_EVERY`).
    fn persist_sent(&self, force: bool) {
        {
            let mut at = self
                .torrents
                .persisted
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if !force && at.is_some_and(|t| t.elapsed() < PERSIST_EVERY) {
                return;
            }
            *at = Some(Instant::now());
        }
        let totals: Vec<(Id, u64)> = self
            .torrents
            .sent
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|(id, s)| (*id, s.base.saturating_add(s.last)))
            .collect();
        let _ = self.with_db(|c| {
            let tx = c.transaction()?;
            for (id, total) in &totals {
                store::set_progress(&tx, *id, *total, None)?;
            }
            tx.commit()?;
            Ok(())
        });
    }

    /// Once a second: join the stored torrents with what the engine says and publish what changed.
    async fn torrent_tick(&self) {
        let Ok(rows) = self.with_db(|c| store::list(c)) else {
            return;
        };
        if rows.is_empty() {
            return;
        }
        self.torrent_hours_pass(&rows).await;
        let engine = self.engine_now();
        let snaps: HashMap<String, Snapshot> = engine
            .as_ref()
            .map(|e| e.snapshots().into_iter().collect())
            .unwrap_or_default();
        let now = clock::now();
        let mut described = Vec::new();
        for row in &rows {
            let snap = snaps.get(&row.info_hash);
            if let Some(s) = snap {
                if s.complete && row.finished_at.is_none() {
                    let _ = self.with_db(|c| store::set_progress(c, row.id, 0, Some(now)));
                    tracing::info!(target: "activity", id = row.id, name = %row.name, size = s.size, "torrent finished downloading");
                }
                if row.size.is_none() && s.file_count > 0 {
                    described.push(row.info_hash.clone());
                }
            }
            let torrent = self.torrent_view_of(row, snap);
            let mut last = self.torrents.last.lock().unwrap_or_else(|e| e.into_inner());
            if last.get(&row.id) != Some(&torrent) {
                last.insert(row.id, torrent.clone());
                drop(last);
                self.publish(Event::Torrent {
                    torrent: Box::new(torrent),
                });
            }
        }
        // A torrent that came back with the session (its description was not stored before).
        if let Some(engine) = &engine {
            for hash in described {
                if let Some(row) = rows.iter().find(|r| r.info_hash == hash) {
                    self.torrent_described(row.id, engine, &hash).await;
                }
            }
        }
        self.persist_sent(false);
    }

    /// Before the program ends: write what is not written yet and let the engine stop its torrents.
    pub async fn stop_torrents(&self) {
        self.persist_sent(true);
        if let Some(engine) = self.engine_now() {
            engine.shutdown().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: &str = "0123456789abcdef0123456789abcdef01234567";

    fn row() -> StoredTorrent {
        StoredTorrent {
            id: 1,
            info_hash: HASH.into(),
            name: "From the link".into(),
            magnet: Some("magnet:?xt=urn:btih:x".into()),
            has_metainfo: false,
            size: None,
            file_count: 0,
            paused: false,
            uploaded: 0,
            added_at: 100,
            finished_at: None,
            error: None,
            start_now: false,
            disk: String::new(),
            output: String::new(),
        }
    }

    fn snap(state: RunState) -> Snapshot {
        Snapshot {
            state,
            name: Some("Real name".into()),
            size: 1000,
            downloaded: 250,
            uploaded: 40,
            down_speed: 100,
            up_speed: 10,
            eta_secs: Some(7),
            connected: 3,
            connecting: 1,
            known: 20,
            file_count: 2,
            complete: false,
            error: None,
        }
    }

    #[test]
    fn a_magnet_link_without_a_description_is_fetching_it() {
        let t = view(&row(), None, true, None, 0, None);
        assert_eq!(t.status, TorrentStatus::Metadata);
        assert_eq!(
            (t.name.as_str(), t.size, t.file_count),
            ("From the link", None, 0)
        );
        // Not in the engine and not being fetched (just stored, restart in progress): still waiting.
        assert_eq!(
            view(&row(), None, false, None, 0, None).status,
            TorrentStatus::Metadata
        );
    }

    #[test]
    fn a_running_torrent_shows_the_engines_numbers_and_the_status_follows_the_peers() {
        let t = view(&row(), Some(&snap(RunState::Live)), false, None, 55, None);
        assert_eq!(t.status, TorrentStatus::Downloading);
        assert_eq!(
            (
                t.name.as_str(),
                t.size,
                t.downloaded,
                t.uploaded,
                t.down_speed,
                t.up_speed,
                t.eta
            ),
            ("Real name", Some(1000), 250, 55, 100, 10, Some(7))
        );
        assert_eq!(
            (
                t.peers_connected,
                t.peers_connecting,
                t.peers_known,
                t.file_count
            ),
            (3, 1, 20, 2)
        );

        let lonely = Snapshot {
            connected: 0,
            ..snap(RunState::Live)
        };
        assert_eq!(
            view(&row(), Some(&lonely), false, None, 0, None).status,
            TorrentStatus::Connecting
        );

        let done = Snapshot {
            complete: true,
            downloaded: 1000,
            ..snap(RunState::Live)
        };
        assert_eq!(
            view(&row(), Some(&done), false, None, 0, None).status,
            TorrentStatus::Seeding
        );
    }

    #[test]
    fn a_stopped_torrent_has_no_speed_and_is_paused_or_finished() {
        let t = view(&row(), Some(&snap(RunState::Stopped)), false, None, 0, None);
        assert_eq!(
            (t.status, t.down_speed, t.up_speed, t.eta),
            (TorrentStatus::Paused, 0, 0, None)
        );
        let done = Snapshot {
            complete: true,
            ..snap(RunState::Stopped)
        };
        assert_eq!(
            view(&row(), Some(&done), false, None, 0, None).status,
            TorrentStatus::Finished
        );
    }

    #[test]
    fn checking_and_errors() {
        assert_eq!(
            view(
                &row(),
                Some(&snap(RunState::Checking)),
                false,
                None,
                0,
                None
            )
            .status,
            TorrentStatus::Checking
        );
        let failed = Snapshot {
            error: Some("disk gone".into()),
            ..snap(RunState::Error)
        };
        let t = view(&row(), Some(&failed), false, None, 0, None);
        assert_eq!(
            (t.status, t.error.as_deref()),
            (TorrentStatus::Error, Some("disk gone"))
        );
        // A note on a stopped torrent (not enough room) is an error, not a pause.
        let noted = StoredTorrent {
            error: Some("not enough disk space".into()),
            paused: true,
            ..row()
        };
        let t = view(&noted, Some(&snap(RunState::Stopped)), false, None, 0, None);
        assert_eq!(t.status, TorrentStatus::Error);
        assert_eq!(t.error.as_deref(), Some("not enough disk space"));
        // The engine did not start: every torrent says why.
        let t = view(&row(), None, false, Some("port in use"), 0, None);
        assert_eq!(
            (t.status, t.error.as_deref()),
            (TorrentStatus::Error, Some("port in use"))
        );
    }

    #[test]
    fn a_stored_torrent_the_engine_does_not_have_yet_is_shown_from_the_record() {
        let finished = StoredTorrent {
            paused: true,
            finished_at: Some(5),
            size: Some(900),
            has_metainfo: true,
            ..row()
        };
        let t = view(&finished, None, false, None, 7, None);
        assert_eq!(
            (t.status, t.downloaded, t.size, t.uploaded),
            (TorrentStatus::Finished, 900, Some(900), 7)
        );
        let stopped = StoredTorrent {
            paused: true,
            has_metainfo: true,
            ..row()
        };
        assert_eq!(
            view(&stopped, None, false, None, 0, None).status,
            TorrentStatus::Paused
        );
        let starting = StoredTorrent {
            has_metainfo: true,
            ..row()
        };
        assert_eq!(
            view(&starting, None, false, None, 0, None).status,
            TorrentStatus::Checking
        );
    }

    #[test]
    fn what_was_sent_survives_a_counter_that_starts_over() {
        let mut s = Sent {
            base: 1000,
            last: 0,
        };
        assert_eq!(s.total(0), 1000);
        assert_eq!(s.total(300), 1300);
        assert_eq!(s.total(500), 1500);
        // Stopped and started: the engine counts from zero again.
        assert_eq!(s.total(0), 1500);
        assert_eq!(s.total(200), 1700);
        assert_eq!(s.total(200), 1700, "no change, no growth");
    }

    #[test]
    fn a_torrent_held_by_the_hours_shows_as_paused_with_the_opening_time() {
        let live = snap(RunState::Live);
        let t = view(
            &row(),
            Some(&live),
            false,
            None,
            0,
            Some(Some(1_700_000_000)),
        );
        assert_eq!(t.status, TorrentStatus::Paused);
        assert_eq!(t.hold, Some(Hold::Window));
        assert_eq!(t.starts_at, Some(1_700_000_000));
        assert_eq!(
            (t.down_speed, t.up_speed),
            (0, 0),
            "nothing moves while held"
        );
        // Without a zone to compute from, it is still held, just without a time.
        let t = view(&row(), Some(&live), false, None, 0, Some(None));
        assert_eq!((t.hold, t.starts_at), (Some(Hold::Window), None));
        // Not held: no hold, and the flag of "start now" is passed on.
        let free = StoredTorrent {
            start_now: true,
            ..row()
        };
        let t = view(&free, Some(&live), false, None, 0, None);
        assert_eq!((t.hold, t.starts_at, t.start_now), (None, None, true));
    }

    #[test]
    fn an_error_is_not_hidden_by_the_hours() {
        let noted = StoredTorrent {
            error: Some("disk full".into()),
            ..row()
        };
        let t = view(&noted, None, false, None, 0, Some(None));
        assert_eq!(t.status, TorrentStatus::Error);
    }

    #[test]
    fn a_torrent_on_a_disk_is_saved_under_that_disk() {
        // The download directory keeps the engine's own folder, as before disks.
        assert_eq!(torrent_output_rel("", None), "");
        assert_eq!(torrent_output_rel("", Some("Name")), "");
        // On a disk: straight into <disk>/torrents for one file, a folder of its own otherwise.
        assert_eq!(torrent_output_rel("ssd1", None), "ssd1/torrents");
        assert_eq!(
            torrent_output_rel("ssd1", Some("Ubuntu 24")),
            "ssd1/torrents/Ubuntu 24"
        );
        // The name comes from the swarm or a link: it cannot climb out.
        assert_eq!(
            torrent_output_rel("ssd1", Some("../../etc")),
            "ssd1/torrents/etc"
        );
        assert_eq!(torrent_output_rel("ssd1", Some("a/b")), "ssd1/torrents/b");
        assert_eq!(
            torrent_output_rel("ssd1", Some("..")),
            "ssd1/torrents/download"
        );
    }
}
