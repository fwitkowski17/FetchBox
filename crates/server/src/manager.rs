//! Download manager: wires storage, scheduler and the HTTP engine, and implements the
//! API's [`Service`].
//!
//! The database is the source of truth. The engine reports progress over a channel;
//! this module persists it (once per tick, never from the download hot path) and
//! turns it into UI events.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};
use std::time::{Duration, Instant, UNIX_EPOCH};

mod torrents;

pub use torrents::TorrentSetup;

use crate::clock;
use crate::hostkeys::HostKeyCache;
use crate::logfmt;
use crate::update::{self, UpdateChecker};
use async_trait::async_trait;
use fetchbox_api::service::Service;
use fetchbox_core::category::detect as detect_category;
use fetchbox_core::checksum::normalize as normalize_checksum;
use fetchbox_core::filename::sanitize;
use fetchbox_core::limiter::RateLimiter;
use fetchbox_core::schedule::Gate;
use fetchbox_core::scheduler::Scheduler;
use fetchbox_core::{
    AddRequest, AddTorrentRequest, Category, Conflict, ConflictKind, Credentials, DiskSpace,
    Download, Downloader, Error, Event, Hold, Id, InstallMethod, Job, KnownHost, OnConflict,
    OnHoursEnd, Result, Settings, SourceKind, Stats, StatsRange, Status, Torrent, TorrentFile,
    TorrentPeers, TrustHost, UrlInfo, UrlKind, VersionInfo,
};
use fetchbox_http_engine::{
    candidates, source, verify::hash_file, EngineError, HttpDownloader, UrlPolicy,
};
use fetchbox_storage::downloads::{self, NewDownload};
use fetchbox_storage::{known_hosts, segments, settings, stats, Connection};
use tokio::sync::{broadcast, mpsc, watch};

/// Capacity of the UI event bus; slow subscribers get a lag error and resync.
const BUS_CAPACITY: usize = 256;

/// How far ahead a download may be set to begin: 31 days.
const MAX_START_AHEAD_SECS: i64 = 31 * 24 * 3600;

/// Outcome of the collision check for a new download.
#[derive(Default)]
struct Detected {
    conflict: Option<Conflict>,
    /// The file name is used on disk or by an unfinished download.
    name_taken: bool,
    /// An unfinished download will produce this name (it cannot be overwritten).
    name_reserved: bool,
}

/// A checksum verification that is running (SPEC 6.2.2).
struct VerifyJob {
    /// Bytes of the file read so far.
    hashed: Arc<AtomicU64>,
    /// Set to stop it: the user keeps the file or removes the download.
    cancel: Arc<AtomicBool>,
}

pub struct Manager {
    db: Mutex<Connection>,
    engine: HttpDownloader,
    sched: Mutex<Scheduler>,
    settings: Mutex<Settings>,
    dir: PathBuf,
    /// Serializes "check for conflicts, then insert" so two adds cannot claim one name.
    add_lock: tokio::sync::Mutex<()>,
    bus: broadcast::Sender<Event>,
    /// `true` once the server is shutting down (see `Service::closing`).
    closing: watch::Sender<bool>,
    /// Since when each running download has been transferring, as of its last progress tick. The
    /// time between ticks is added to the download's `active_ms`, which is what an average speed
    /// is measured against (queued and paused time is not).
    active: Mutex<HashMap<Id, Instant>>,
    /// The highest speed seen so far (bytes/s), also kept in the database.
    peak_speed: AtomicU64,
    /// Looks for a newer release (see `update`).
    updates: Arc<UpdateChecker>,
    /// Queued downloads that are not in the scheduler because they must wait (the download hours
    /// are closed, or a start time has not come): see `admit` and `reevaluate`.
    held: Mutex<HashSet<Id>>,
    /// Running downloads that the download hours asked to stop: when the engine reports them
    /// paused they go back to the queue (held) instead of to `paused`.
    suspended: Mutex<HashSet<Id>>,
    /// Whether the download hours were open when `reevaluate` last looked (`None`: they are off or
    /// have not been looked at since the settings changed). Only for the log: a change is news.
    hours_seen: Mutex<Option<bool>>,
    /// Itself, so that `&self` methods can start tasks that need an `Arc` (set once at start-up).
    this: OnceLock<Weak<Manager>>,
    /// Verifications that are running: they hold no download slot.
    verifying: Mutex<HashMap<Id, Arc<VerifyJob>>>,
    /// The SSH servers whose host key the user accepted (the engine asks this at every connection).
    host_keys: Arc<HostKeyCache>,
    /// How this copy was installed, as the installer stated it; picks the update steps the UI shows.
    install_method: Option<InstallMethod>,
    /// Which destinations downloads (and the trackers of torrents) may use.
    url_policy: UrlPolicy,
    /// BitTorrent: the engine and what is kept about it between ticks (see `torrents`).
    torrents: torrents::Torrents,
}

impl Manager {
    /// Build the manager, recover interrupted downloads and start the event loop.
    pub async fn start(
        conn: Connection,
        dir: PathBuf,
        policy: UrlPolicy,
    ) -> anyhow::Result<Arc<Self>> {
        // `FETCHBOX_UPDATE_URL` points the update check somewhere else (a mirror, a fork).
        let source = std::env::var("FETCHBOX_UPDATE_URL")
            .ok()
            .filter(|u| !u.is_empty());
        Self::start_with(conn, dir, policy, source, update::install_method_from_env()).await
    }

    /// Like `start`, with the address of the releases API given (`None`: GitHub).
    pub async fn start_with(
        conn: Connection,
        dir: PathBuf,
        policy: UrlPolicy,
        update_source: Option<String>,
        install_method: Option<InstallMethod>,
    ) -> anyhow::Result<Arc<Self>> {
        let current = settings::load(&conn)?;
        let peak_speed = stats::peak_speed(&conn)?;
        let limiter = RateLimiter::with_global(current.speed_global);
        let host_keys = Arc::new(HostKeyCache::load(&known_hosts::list(&conn)?));
        let (engine, rx) =
            HttpDownloader::with_host_keys(&dir, limiter, policy, Arc::clone(&host_keys) as _)?;
        engine.set_default_speed(current.speed_per_download);
        let requeued = downloads::requeue_interrupted(&conn)?;
        if requeued > 0 {
            tracing::info!(count = requeued, "re-queued interrupted downloads");
        }
        let queued = downloads::list_by_status(&conn, Status::Queued)?;
        let (bus, _) = broadcast::channel(BUS_CAPACITY);
        if let Some(method) = install_method {
            tracing::info!(install_method = method.as_str(), "installation");
        }
        tracing::info!("settings: {}", logfmt::settings_summary(&current));
        let all = downloads::list(&conn)?;
        let count = |s: Status| all.iter().filter(|d| d.status == s).count();
        tracing::info!(
            total = all.len(),
            queued = count(Status::Queued),
            paused = count(Status::Paused),
            completed = count(Status::Completed),
            failed = count(Status::Failed),
            "downloads in the list"
        );

        let me = Arc::new(Self {
            db: Mutex::new(conn),
            engine,
            sched: Mutex::new(Scheduler::new(current.max_concurrent)),
            settings: Mutex::new(current),
            dir,
            add_lock: tokio::sync::Mutex::new(()),
            bus,
            closing: watch::channel(false).0,
            active: Mutex::new(HashMap::new()),
            peak_speed: AtomicU64::new(peak_speed),
            updates: UpdateChecker::new(update_source)?,
            install_method,
            url_policy: policy,
            torrents: torrents::Torrents::default(),
            held: Mutex::new(HashSet::new()),
            suspended: Mutex::new(HashSet::new()),
            hours_seen: Mutex::new(None),
            this: OnceLock::new(),
            verifying: Mutex::new(HashMap::new()),
            host_keys,
        });
        let _ = me.this.set(Arc::downgrade(&me));

        tokio::spawn(Arc::clone(&me).run_events(rx));

        let mut to_start = Vec::new();
        for d in queued {
            to_start.extend(me.admit(d.id));
        }
        me.launch(to_start).await;
        // A verification that a restart interrupted begins again: the file is complete.
        for d in me.with_db(|c| downloads::list_verifying(c))? {
            me.start_verification(d.id);
        }
        tokio::spawn(Arc::clone(&me).run_schedule());
        Ok(me)
    }

    /// The server is going down: end the long-lived responses so the HTTP server can finish.
    /// Running downloads need nothing: progress is persisted every tick and what was running
    /// is re-queued at the next start.
    pub fn shutdown(&self) {
        self.closing.send_replace(true);
    }

    /// For the shutdown deadline in `main`.
    pub fn closing_receiver(&self) -> watch::Receiver<bool> {
        self.closing.subscribe()
    }

    // ---- time spent transferring ----------------------------------------------------

    fn elapsed_ms(since: Instant) -> u64 {
        u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// The download starts transferring now.
    fn active_start(&self, id: Id) {
        self.active
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, Instant::now());
    }

    /// Milliseconds since the last tick of a running download (`0` for one that is not running),
    /// and the clock goes on from now.
    fn active_lap(&self, id: Id) -> u64 {
        let mut active = self.active.lock().unwrap_or_else(|e| e.into_inner());
        active.get_mut(&id).map_or(0, |since| {
            let ms = Self::elapsed_ms(*since);
            *since = Instant::now();
            ms
        })
    }

    /// Like `active_lap`, but the download stops transferring (paused, finished, failed, cancelled).
    fn active_stop(&self, id: Id) -> u64 {
        self.active
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id)
            .map_or(0, Self::elapsed_ms)
    }

    /// Remember a new record speed.
    fn note_speed(&self, speed: u64) {
        if speed > self.peak_speed.fetch_max(speed, Ordering::Relaxed) {
            let r = self.with_db(|c| stats::set_peak_speed(c, speed));
            if let Err(e) = r {
                tracing::warn!(error = %e, "cannot store the peak speed");
            }
        }
    }

    // ---- small helpers -------------------------------------------------------------

    fn db(&self) -> MutexGuard<'_, Connection> {
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn sched(&self) -> MutexGuard<'_, Scheduler> {
        self.sched.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn current_settings(&self) -> Settings {
        self.settings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn with_db<T>(
        &self,
        f: impl FnOnce(&mut Connection) -> fetchbox_storage::Result<T>,
    ) -> Result<T> {
        let mut conn = self.db();
        Ok(f(&mut conn)?)
    }

    fn verifying(&self) -> MutexGuard<'_, HashMap<Id, Arc<VerifyJob>>> {
        self.verifying.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn held(&self) -> MutexGuard<'_, HashSet<Id>> {
        self.held.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn suspended(&self) -> MutexGuard<'_, HashSet<Id>> {
        self.suspended.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The file name of a download, for the log (empty when it is gone).
    fn name_of(&self, id: Id) -> String {
        self.with_db(|c| downloads::get(c, id))
            .ok()
            .flatten()
            .map(|d| d.filename)
            .unwrap_or_default()
    }

    /// What a download has averaged: `2.0 MiB/s over 3m 20s`, or why there is no figure.
    fn average_of(&self, id: Id, bytes: u64) -> String {
        let ms = self.with_db(|c| stats::active_ms(c, id)).unwrap_or(0);
        match logfmt::average_speed(bytes, ms) {
            Some(speed) => format!(
                "{} over {}",
                logfmt::rate(speed),
                logfmt::duration(ms / 1000)
            ),
            None => "no time measured".to_owned(),
        }
    }

    fn get(&self, id: Id) -> Result<Download> {
        self.with_db(|c| downloads::get(c, id))?
            .ok_or(Error::NotFound(id))
    }

    /// Fresh numbers from the engine for running downloads.
    fn view(&self, mut d: Download) -> Download {
        if d.status == Status::Downloading {
            let p = self.engine.progress(d.id);
            if p.downloaded > 0 {
                d.downloaded = p.downloaded;
            }
            if d.size.is_none() {
                d.size = p.total;
            }
            d.speed = p.speed;
        }
        if d.status == Status::Verifying {
            // The bar of a download that is being verified shows how much of the file was read.
            d.downloaded = self
                .verifying()
                .get(&d.id)
                .map_or(0, |job| job.hashed.load(Ordering::Relaxed));
        }
        if d.status == Status::Queued && self.held().contains(&d.id) {
            let now = clock::now();
            match d.start_at {
                Some(at) if at > now => {
                    d.hold = Some(Hold::Time);
                    d.starts_at = Some(at);
                }
                _ => {
                    d.hold = Some(Hold::Window);
                    d.starts_at = self.next_opening(now);
                }
            }
        }
        d
    }

    // ---- download hours --------------------------------------------------------------

    /// Is the local time inside the download hours? Without hours (or with a zone nobody can read,
    /// which `update_settings` refuses) it is always "open".
    fn window_open(&self, s: &Settings, now: i64) -> bool {
        if !s.schedule_enabled {
            return true;
        }
        match clock::zone(&s.schedule_timezone).and_then(|tz| clock::minute_of_day(now, &tz)) {
            Some(minute) => s.schedule_window().contains(minute),
            None => {
                tracing::warn!(zone = %s.schedule_timezone, "unknown time zone for the download hours");
                true
            }
        }
    }

    /// The next moment the download hours open, as unix seconds.
    fn next_opening(&self, now: i64) -> Option<i64> {
        let s = self.current_settings();
        if !s.schedule_enabled {
            return None;
        }
        let tz = clock::zone(&s.schedule_timezone)?;
        clock::next_at(now, &tz, s.schedule_from)
    }

    /// What decides whether download `id` may start or go on, right now.
    fn gate_for(&self, id: Id) -> Result<Gate> {
        let s = self.current_settings();
        let (start_at, start_now) = self.with_db(|c| {
            let start_at = downloads::get(c, id)?.and_then(|d| d.start_at);
            Ok((start_at, downloads::start_now_flag(c, id)?))
        })?;
        Ok(Gate {
            window_enabled: s.schedule_enabled,
            window_open: self.window_open(&s, clock::now()),
            start_now,
            start_at,
            finish_running: s.schedule_on_end == OnHoursEnd::Finish,
        })
    }

    /// A download is ready to run: put it in the scheduler, or, when the download hours or its
    /// start time say wait, keep it on the side (`held`) until `reevaluate` lets it go. Returns
    /// the downloads that should be started now.
    fn admit(&self, id: Id) -> Vec<Id> {
        let wait = match self.gate_for(id) {
            Ok(gate) => !gate.may_start(clock::now()),
            Err(e) => {
                tracing::warn!(id, error = %e, "cannot read the schedule of a download; starting it");
                false
            }
        };
        if wait {
            self.held().insert(id);
            self.log_waiting(id);
            Vec::new()
        } else {
            self.sched().enqueue(id)
        }
    }

    /// Say why a download waits and until when.
    fn log_waiting(&self, id: Id) {
        let name = self.name_of(id);
        let now = clock::now();
        let start_at = self
            .with_db(|c| downloads::get(c, id))
            .ok()
            .flatten()
            .and_then(|d| d.start_at);
        match start_at {
            Some(at) if at > now => {
                let when = jiff::Timestamp::from_second(at)
                    .map(|t| t.to_string())
                    .unwrap_or_default();
                tracing::info!(target: "activity", id, %name, start_at = %when, "download waits for its start time");
            }
            _ => {
                let s = self.current_settings();
                let opens = self
                    .next_opening(now)
                    .map_or_else(String::new, |at| clock::describe(at, &s.schedule_timezone));
                tracing::info!(target: "activity", id, %name, opens = %opens, "download waits for the download hours");
            }
        }
    }

    /// Say when the download hours open or close (once per change, not every check).
    fn log_hours(&self, s: &Settings, now: i64) {
        let seen = if s.schedule_enabled {
            Some(self.window_open(s, now))
        } else {
            None
        };
        let mut last = self.hours_seen.lock().unwrap_or_else(|e| e.into_inner());
        if *last == seen {
            return;
        }
        *last = seen;
        match seen {
            Some(true) => tracing::info!(target: "activity",
                hours = %format!("{}-{} {}", logfmt::clock(s.schedule_from), logfmt::clock(s.schedule_to), s.schedule_timezone),
                "download hours are open: waiting downloads may start"
            ),
            Some(false) => tracing::info!(target: "activity",
                opens = %self.next_opening(now).map_or_else(String::new, |at| clock::describe(at, &s.schedule_timezone)),
                on_end = s.schedule_on_end.as_str(),
                "download hours are closed: nothing new starts"
            ),
            None => {}
        }
    }

    /// Look at the clock again: let the waiting downloads whose time has come go, and stop the
    /// running ones the download hours no longer allow (they return to the queue, keeping what
    /// they have). Called every few seconds, and at once when the settings change.
    pub(crate) async fn reevaluate(&self) {
        let now = clock::now();
        self.log_hours(&self.current_settings(), now);
        let waiting: Vec<Id> = self.held().iter().copied().collect();
        let mut start = Vec::new();
        for id in waiting {
            let queued = matches!(self.get(id), Ok(d) if d.status == Status::Queued);
            if !queued {
                self.held().remove(&id);
                continue;
            }
            let may = self.gate_for(id).map_or(true, |g| g.may_start(now));
            if may {
                self.held().remove(&id);
                tracing::info!(target: "activity", id, name = %self.name_of(id), "no longer waiting: it may start");
                start.extend(self.sched().enqueue(id));
                self.publish_download(id);
            }
        }
        let running = self.sched().running_ids();
        for id in running {
            let stop = self.gate_for(id).is_ok_and(|g| !g.may_run());
            let downloading = matches!(self.get(id), Ok(d) if d.status == Status::Downloading);
            if stop && downloading {
                self.suspended().insert(id);
                if self.engine.pause(id).await.is_err() {
                    self.suspended().remove(&id);
                }
            }
        }
        self.launch(start).await;
    }

    /// Every few seconds: see `reevaluate`. Ends when the server shuts down.
    async fn run_schedule(self: Arc<Self>) {
        let mut closing = self.closing.subscribe();
        let mut tick = tokio::time::interval(Duration::from_secs(15));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = tick.tick() => self.reevaluate().await,
                _ = closing.changed() => break,
            }
        }
    }

    fn publish(&self, event: Event) {
        // No subscribers is the normal case when no browser is open.
        let _ = self.bus.send(event);
    }

    fn publish_download(&self, id: Id) {
        match self.with_db(|c| downloads::get(c, id)) {
            Ok(Some(d)) => self.publish(Event::Download {
                download: Box::new(self.view(d)),
            }),
            Ok(None) => {}
            Err(e) => tracing::warn!(id, error = %e, "cannot load download for event"),
        }
    }

    fn is_active(&self, id: Id) -> bool {
        matches!(
            self.with_db(|c| downloads::get(c, id)),
            Ok(Some(d)) if matches!(d.status, Status::Queued | Status::Downloading)
        )
    }

    // ---- starting downloads --------------------------------------------------------

    /// Start the given scheduled downloads; failures free their slot for the next in line.
    async fn launch(&self, ids: Vec<Id>) {
        let mut queue: VecDeque<Id> = ids.into();
        while let Some(id) = queue.pop_front() {
            match self.try_start(id).await {
                Ok(true) => {}
                Ok(false) => queue.extend(self.sched().release(id)),
                Err(e) => {
                    tracing::warn!(id, error = %e, "failed to start download");
                    let msg = e.to_string();
                    let _ = self.with_db(|c| {
                        downloads::set_status(c, id, Status::Failed, Some(&msg)).map(|_| ())
                    });
                    self.publish_download(id);
                    queue.extend(self.sched().release(id));
                }
            }
        }
    }

    /// Build the job from persisted state and hand it to the engine.
    /// `Ok(false)` means the download is no longer waiting (its slot must be released).
    async fn try_start(&self, id: Id) -> Result<bool> {
        let (d, segs, overwrite, ignore_space, credentials) = self.with_db(|c| {
            let d = downloads::get(c, id)?;
            let segs = segments::list(c, id)?;
            let overwrite = downloads::overwrite_flag(c, id)?;
            let ignore_space = downloads::ignore_space_flag(c, id)?;
            let credentials = downloads::credentials(c, id)?;
            Ok((d, segs, overwrite, ignore_space, credentials))
        })?;
        let Some(d) = d else { return Ok(false) };
        if d.status != Status::Queued {
            return Ok(false);
        }
        let s = self.current_settings();
        let job = Job {
            id,
            url: d.url,
            filename: d.filename,
            subdir: d.subdir,
            connections: s.connections,
            speed_limit: d.speed_limit,
            etag: d.etag,
            segments: if d.downloaded > 0 { segs } else { Vec::new() },
            overwrite,
            ignore_space,
            credentials,
        };
        self.engine.start(&job).await?;
        Ok(true)
    }

    // ---- engine events -------------------------------------------------------------

    async fn run_events(
        self: Arc<Self>,
        mut rx: mpsc::UnboundedReceiver<fetchbox_http_engine::Event>,
    ) {
        while let Some(ev) = rx.recv().await {
            self.handle_engine_event(ev).await;
        }
    }

    async fn handle_engine_event(&self, ev: fetchbox_http_engine::Event) {
        use fetchbox_http_engine::Event as E;
        const ACTIVE: [Status; 2] = [Status::Queued, Status::Downloading];
        match ev {
            E::Started {
                id,
                size,
                resumable,
                etag,
                segments: segs,
            } => {
                if !self.is_active(id) {
                    return;
                }
                let r = self.with_db(|c| {
                    downloads::set_remote_info(c, id, size, resumable, etag.as_deref())?;
                    segments::replace(c, id, &segs)?;
                    downloads::transition(c, id, &ACTIVE, Status::Downloading, None)?;
                    Ok(())
                });
                self.log_err("started", id, r);
                tracing::info!(target: "activity",
                    id,
                    name = %self.name_of(id),
                    size = %size.map_or_else(|| "unknown".to_owned(), logfmt::bytes),
                    resumable,
                    segments = segs.len(),
                    "download started"
                );
                self.active_start(id);
                self.publish_download(id);
            }
            E::Progress {
                id,
                downloaded,
                segments: segs,
            } => {
                let pairs: Vec<(u32, u64)> = segs.iter().map(|s| (s.idx, s.downloaded)).collect();
                // What is stored is what is safe on the disk (the segments); the screen gets what
                // has been received so far. A plain stream has no segments and is stored as is.
                let stored = if segs.is_empty() {
                    downloaded
                } else {
                    segs.iter().map(|s| s.downloaded).sum()
                };
                let ms = self.active_lap(id);
                let r = self.with_db(|c| {
                    downloads::set_downloaded(c, id, stored)?;
                    stats::add_active_ms(c, id, ms)?;
                    segments::update_offsets(c, id, &pairs)
                });
                self.log_err("progress", id, r);
                let p = self.engine.progress(id);
                self.note_speed(p.speed);
                self.publish(Event::Progress {
                    id,
                    downloaded,
                    total: p.total,
                    speed: p.speed,
                });
            }
            E::Paused {
                id,
                downloaded,
                segments: segs,
            } => {
                if !self.is_active(id) {
                    return;
                }
                let pairs: Vec<(u32, u64)> = segs.iter().map(|s| (s.idx, s.downloaded)).collect();
                let ms = self.active_stop(id);
                // Stopped by the download hours: back to the queue, waiting for them to open.
                let by_schedule = self.suspended().remove(&id);
                let to = if by_schedule {
                    Status::Queued
                } else {
                    Status::Paused
                };
                let r = self.with_db(|c| {
                    downloads::set_downloaded(c, id, downloaded)?;
                    stats::add_active_ms(c, id, ms)?;
                    segments::update_offsets(c, id, &pairs)?;
                    downloads::transition(c, id, &ACTIVE, to, None)?;
                    Ok(())
                });
                self.log_err("paused", id, r);
                if by_schedule {
                    self.held().insert(id);
                    tracing::info!(target: "activity",
                        id,
                        name = %self.name_of(id),
                        downloaded = %logfmt::bytes(downloaded),
                        "stopped by the download hours; it goes on when they open"
                    );
                } else {
                    tracing::info!(target: "activity", id, name = %self.name_of(id), downloaded = %logfmt::bytes(downloaded), "download paused");
                }
                self.publish_download(id);
                let next = self.sched().release(id);
                self.launch(next).await;
            }
            E::Completed { id, path, size } => {
                if !self.is_active(id) {
                    return;
                }
                let name = path.file_name().and_then(|n| n.to_str()).map(str::to_owned);
                let ms = self.active_stop(id);
                // A download with a checksum is not done yet: the file is verified first.
                let verify = matches!(self.get(id), Ok(d) if d.checksum.is_some());
                let end = if verify {
                    Status::Verifying
                } else {
                    Status::Completed
                };
                let r = self.with_db(|c| {
                    stats::add_active_ms(c, id, ms)?;
                    downloads::set_downloaded(c, id, size)?;
                    downloads::set_size(c, id, size)?;
                    if let Some(name) = &name {
                        // The engine may have picked a different free name.
                        downloads::set_filename(c, id, name)?;
                    }
                    segments::replace(c, id, &[])?;
                    downloads::transition(c, id, &ACTIVE, end, None)?;
                    Ok(())
                });
                self.log_err("completed", id, r);
                tracing::info!(target: "activity",
                    id,
                    name = %self.name_of(id),
                    size = %logfmt::bytes(size),
                    average = %self.average_of(id, size),
                    "{}",
                    if verify { "download received, verifying the checksum" } else { "download completed" }
                );
                if verify {
                    // Reading the file back holds no download slot: the next download may start.
                    self.start_verification(id);
                }
                self.publish_download(id);
                let next = self.sched().release(id);
                self.launch(next).await;
            }
            E::Failed { id, error } => {
                if !self.is_active(id) {
                    return;
                }
                let ms = self.active_stop(id);
                let r = self.with_db(|c| {
                    stats::add_active_ms(c, id, ms)?;
                    downloads::transition(c, id, &ACTIVE, Status::Failed, Some(&error))
                });
                self.log_err("failed", id, r);
                tracing::warn!(id, name = %self.name_of(id), %error, "download failed");
                self.publish_download(id);
                let next = self.sched().release(id);
                self.launch(next).await;
            }
        }
    }

    fn log_err<T>(&self, what: &str, id: Id, r: Result<T>) {
        if let Err(e) = r {
            tracing::error!(id, error = %e, "cannot persist engine event ({what})");
        }
    }

    // ---- conflicts -----------------------------------------------------------------

    /// First file name that is free on disk and not claimed by an unfinished download.
    async fn free_name(&self, subdir: &str, name: &str) -> Result<String> {
        for path in candidates(&self.dir.join(subdir), name).take(10_000) {
            let cand = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(name)
                .to_owned();
            if tokio::fs::try_exists(&path).await.map_err(io_err)? {
                continue;
            }
            if self
                .with_db(|c| downloads::find_unfinished_by_filename(c, subdir, &cand))?
                .is_some()
            {
                continue;
            }
            return Ok(cand);
        }
        Err(Error::Internal("no free file name".into()))
    }

    /// The most relevant collision for `url`/`name`, if any.
    async fn detect_conflict(&self, url: &str, subdir: &str, name: &str) -> Result<Detected> {
        let dup = self
            .with_db(|c| downloads::find_by_url(c, url))?
            .into_iter()
            .next();
        let reserved = self.with_db(|c| downloads::find_unfinished_by_filename(c, subdir, name))?;
        let on_disk = tokio::fs::metadata(self.dir.join(subdir).join(name))
            .await
            .ok();
        let name_reserved = reserved.is_some();
        let name_taken = name_reserved || on_disk.is_some();
        if dup.is_none() && !name_taken {
            return Ok(Detected::default());
        }
        let suggested_name = self.free_name(subdir, name).await?;

        let conflict = if let Some(d) = dup {
            Conflict {
                kind: ConflictKind::DuplicateUrl,
                existing_name: d.filename,
                existing_size: d.size,
                existing_modified: None,
                existing_id: Some(d.id),
                suggested_name,
            }
        } else if let Some(d) = reserved {
            Conflict {
                kind: ConflictKind::NameReserved,
                existing_name: d.filename,
                existing_size: d.size,
                existing_modified: None,
                existing_id: Some(d.id),
                suggested_name,
            }
        } else {
            let meta = on_disk.as_ref();
            Conflict {
                kind: ConflictKind::FileExists,
                existing_name: name.to_owned(),
                existing_size: meta.map(|m| m.len()),
                existing_modified: meta
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .and_then(|d| i64::try_from(d.as_secs()).ok()),
                existing_id: None,
                suggested_name,
            }
        };
        Ok(Detected {
            conflict: Some(conflict),
            name_taken,
            name_reserved,
        })
    }

    /// Delete the finished file of a completed download. Only a plain file name directly inside
    /// the download's own directory is ever touched; a file that is already gone is fine.
    async fn delete_finished_file(&self, d: &Download) -> Result<()> {
        // Names are sanitized when added and again by the engine; this is the last line of defence
        // against a path that points anywhere else.
        if d.filename.is_empty() || sanitize(&d.filename) != d.filename {
            return Err(Error::InvalidState(format!(
                "refusing to delete a file with an unsafe name: {:?}",
                d.filename
            )));
        }
        let path = self.engine.dir_for(&d.subdir).join(&d.filename);
        match tokio::fs::remove_file(&path).await {
            Ok(()) => {
                tracing::info!(id = d.id, path = %path.display(), "deleted the downloaded file");
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => {
                tracing::warn!(id = d.id, path = %path.display(), error = %e, "cannot delete the file");
                Err(io_err(e))
            }
        }
    }

    /// Remove the `.part` file of a download, ignoring "not there".
    async fn remove_part(&self, id: Id, subdir: &str, filename: &str) {
        let part = self.engine.part_path(id, subdir, filename);
        if let Err(e) = tokio::fs::remove_file(&part).await {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(path = %part.display(), error = %e, "cannot remove part file");
            }
        }
    }

    /// Stop a download that is queued, running or paused and mark it cancelled.
    async fn do_cancel(&self, d: &Download) -> Result<()> {
        // Release first: afterwards no new engine start can pick this id up.
        self.held().remove(&d.id);
        self.suspended().remove(&d.id);
        let next = self.sched().release(d.id);
        match self.engine.cancel(d.id).await {
            Ok(()) | Err(Error::NotFound(_)) => {}
            Err(e) => return Err(e),
        }
        self.remove_part(d.id, &d.subdir, &d.filename).await;
        let ms = self.active_stop(d.id);
        self.with_db(|c| {
            segments::replace(c, d.id, &[])?;
            stats::add_active_ms(c, d.id, ms)?;
            downloads::set_status(c, d.id, Status::Cancelled, None).map(|_| ())
        })?;
        tracing::info!(target: "activity", id = d.id, name = %d.filename, "download cancelled");
        self.publish_download(d.id);
        self.launch(next).await;
        Ok(())
    }
}

impl Manager {
    // ---- checksum verification (SPEC 6.2.2) ------------------------------------------

    /// Begin verifying the file of a download that is `verifying`.
    fn start_verification(&self, id: Id) {
        let Some(me) = self.this.get().and_then(Weak::upgrade) else {
            return;
        };
        let job = Arc::new(VerifyJob {
            hashed: Arc::default(),
            cancel: Arc::default(),
        });
        {
            let mut running = self.verifying();
            if running.contains_key(&id) {
                return;
            }
            running.insert(id, Arc::clone(&job));
        }
        tokio::spawn(async move { me.verify(id, job).await });
    }

    /// Stop the verification of a download (the user keeps the file or removes the download).
    fn stop_verification(&self, id: Id) {
        if let Some(job) = self.verifying().remove(&id) {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Does the file of this download exist as a finished file (so that it can be deleted)?
    fn has_finished_file(d: &Download) -> bool {
        matches!(d.status, Status::Completed | Status::Verifying)
            || (d.status == Status::Failed && d.verify_failed)
    }

    async fn verify(self: Arc<Self>, id: Id, job: Arc<VerifyJob>) {
        let Ok(d) = self.get(id) else { return };
        let (Status::Verifying, Some(sum)) = (d.status, d.checksum.clone()) else {
            self.forget_verification(id, &job);
            return;
        };
        let path = self.engine.dir_for(&d.subdir).join(&d.filename);
        // The progress bar: how much of the file has been read.
        let ticker = {
            let (me, job, total) = (Arc::clone(&self), Arc::clone(&job), d.size);
            tokio::spawn(async move {
                let mut tick = tokio::time::interval(Duration::from_secs(1));
                tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                let mut last = (Instant::now(), 0u64);
                tick.tick().await;
                loop {
                    tick.tick().await;
                    let (now, hashed) = (Instant::now(), job.hashed.load(Ordering::Relaxed));
                    let secs = now.duration_since(last.0).as_secs_f64().max(0.001);
                    let speed = (hashed.saturating_sub(last.1) as f64 / secs) as u64;
                    last = (now, hashed);
                    me.publish(Event::Progress {
                        id,
                        downloaded: hashed,
                        total,
                        speed,
                    });
                }
            })
        };
        let result = hash_file(
            &path,
            sum.algo,
            Arc::clone(&job.hashed),
            Arc::clone(&job.cancel),
        )
        .await;
        ticker.abort();
        self.forget_verification(id, &job);
        if job.cancel.load(Ordering::Relaxed) {
            return; // the user decided; whoever did it has updated the download
        }
        let name = d.filename.clone();
        let verdict = match result {
            Ok(Some(actual)) if actual == sum.expected => Ok(()),
            Ok(Some(actual)) => Err((
                Some(actual.clone()),
                format!(
                    "checksum mismatch: expected {} {}, the file has {}",
                    sum.algo.as_str().to_ascii_uppercase(),
                    sum.expected,
                    actual
                ),
            )),
            Ok(None) => return,
            Err(e) => Err((None, format!("cannot verify the file: {e}"))),
        };
        let r = self.with_db(|c| match &verdict {
            Ok(()) => {
                if downloads::transition(c, id, &[Status::Verifying], Status::Completed, None)? {
                    downloads::set_verification(c, id, true, None)?;
                }
                Ok(())
            }
            Err((actual, message)) => {
                if downloads::transition(
                    c,
                    id,
                    &[Status::Verifying],
                    Status::Failed,
                    Some(message),
                )? {
                    downloads::set_verification(c, id, false, actual.as_deref())?;
                }
                Ok(())
            }
        });
        self.log_err("verification", id, r);
        match &verdict {
            Ok(()) => {
                tracing::info!(target: "activity", id, %name, algo = sum.algo.as_str(), "checksum verified")
            }
            Err((_, message)) => tracing::warn!(id, %name, error = %message, "verification failed"),
        }
        self.publish_download(id);
    }

    /// The verification is over; forget it unless a newer one has taken its place.
    fn forget_verification(&self, id: Id, job: &Arc<VerifyJob>) {
        let mut running = self.verifying();
        if running.get(&id).is_some_and(|j| Arc::ptr_eq(j, job)) {
            running.remove(&id);
        }
    }

    /// Bytes that the running downloads still have to write on file system `dev`: the space
    /// reserved for them. "Running" means holding a download slot, whether or not the engine has
    /// reported its start yet, so two downloads added in quick succession see each other.
    /// Queued downloads waiting for a slot, paused and held ones, and finished, failed and
    /// cancelled ones reserve nothing. A running download's reservation shrinks as the disk fills.
    fn reserved_on(&self, dev: u64) -> Result<u64> {
        let all = self.with_db(|c| downloads::list(c))?;
        // Category subdirectories may sit on other disks; look each directory up once.
        let mut same_disk: HashMap<String, bool> = HashMap::new();
        let mut total = 0u64;
        for d in all {
            let running = d.status == Status::Downloading
                || (d.status == Status::Queued && self.sched().is_running(d.id));
            if !running {
                continue;
            }
            let d = self.view(d);
            let Some(size) = d.size else { continue };
            let here = *same_disk.entry(d.subdir.clone()).or_insert_with(|| {
                fetchbox_http_engine::disk::volume(&self.engine.dir_for(&d.subdir))
                    .map_or(true, |v| v.dev == dev)
            });
            if here {
                total = total.saturating_add(size.saturating_sub(d.downloaded));
            }
        }
        Ok(total)
    }

    /// Where does a new download of `needed` bytes stand on its disk?
    /// - `Ok(false)`: it fits, next to what the running downloads still have to write.
    /// - `Ok(true)`: it does not fit even into the space that is really free; it should be added
    ///   but held back, not started.
    /// - `Err(InsufficientSpace)`: it fits into the free space but not next to what the running
    ///   downloads still have to write; the user decides (`ignore_space`).
    ///
    /// A disk whose space cannot be read is not checked: that must not stop downloads.
    fn check_room(&self, subdir: &str, needed: u64) -> Result<bool> {
        let vol = match fetchbox_http_engine::disk::volume(&self.engine.dir_for(subdir)) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "cannot read free disk space; skipping the check");
                return Ok(false);
            }
        };
        if needed > vol.space.available {
            return Ok(true);
        }
        let others = self.reserved_on(vol.dev)?;
        fetchbox_http_engine::disk::ensure_fits(needed, others, vol.space.available)?;
        Ok(false)
    }
}

impl Manager {
    /// Probe a URL through the engine (which applies the private-address policy) and name what
    /// went wrong the way the API reports it.
    async fn probe_remote(
        &self,
        url: &str,
        login: Option<&Credentials>,
    ) -> Result<fetchbox_http_engine::RemoteInfo> {
        self.engine
            .probe_url(url, login)
            .await
            .map_err(|e| engine_error(url, e))
    }
}

/// An engine error as the API reports it.
fn engine_error(url: &str, e: EngineError) -> Error {
    match e {
        EngineError::InvalidUrl(m) => Error::InvalidInput(m),
        EngineError::HostKey(problem) => Error::HostKey(problem),
        // These say what is wrong by themselves.
        e @ (EngineError::Auth(_) | EngineError::NotFound(_)) => Error::Engine(e.to_string()),
        other => Error::Engine(format!("cannot reach {url}: {other}")),
    }
}

/// Take a login out of an address: the address to keep, and the login to keep apart from it.
fn split_login(url: &str, explicit: Option<&Credentials>) -> Result<(String, Option<Credentials>)> {
    source::split_login(url, explicit).map_err(|e| match e {
        EngineError::InvalidUrl(m) => Error::InvalidInput(m),
        other => Error::Engine(other.to_string()),
    })
}

fn io_err(e: std::io::Error) -> Error {
    Error::Internal(e.to_string())
}

#[async_trait]
impl Service for Manager {
    async fn list(&self) -> Result<Vec<Download>> {
        let all = self.with_db(|c| downloads::list(c))?;
        Ok(all.into_iter().map(|d| self.view(d)).collect())
    }

    async fn version(&self, refresh: bool) -> Result<VersionInfo> {
        let settings = self.current_settings();
        let mut info = if settings.check_updates {
            self.updates.info(refresh, settings.check_prereleases).await
        } else {
            update::disabled()
        };
        info.install_method = self.install_method;
        Ok(info)
    }

    async fn probe(&self, url: &str, credentials: Option<Credentials>) -> Result<UrlInfo> {
        let (url, login) = split_login(url, credentials.as_ref())?;
        let found = self
            .engine
            .inspect_url(&url, login.as_ref())
            .await
            .map_err(|e| engine_error(&url, e))?;
        let info = found.info;
        // A directory index (or an FTP or SFTP folder) is offered as a list to choose from.
        let (kind, category, entries, truncated) = match found.listing {
            Some(l) => (UrlKind::Listing, Category::Other, l.entries, l.truncated),
            None => (
                UrlKind::File,
                detect_category(&info.filename, info.content_type.as_deref()),
                Vec::new(),
                false,
            ),
        };
        Ok(UrlInfo {
            kind,
            category,
            filename: info.filename,
            size: info.size,
            resumable: info.resumable,
            entries,
            truncated,
        })
    }

    async fn known_hosts(&self) -> Result<Vec<KnownHost>> {
        self.with_db(|c| known_hosts::list(c))
    }

    async fn trust_host(&self, req: TrustHost) -> Result<KnownHost> {
        let host = req
            .host
            .trim()
            .trim_start_matches('[')
            .trim_end_matches(']');
        let fingerprint = req.fingerprint.trim();
        let valid_fingerprint = fingerprint.strip_prefix("SHA256:").is_some_and(|rest| {
            !rest.is_empty()
                && rest
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '='))
        });
        if host.is_empty() || host.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return Err(Error::InvalidInput(
                "the host is not a host name or address".into(),
            ));
        }
        if req.port == 0 {
            return Err(Error::InvalidInput("the port must be 1..=65535".into()));
        }
        if !valid_fingerprint {
            return Err(Error::InvalidInput(
                "the fingerprint must look like SHA256:... (as ssh-keygen -l prints it)".into(),
            ));
        }
        let key_type: String = req
            .key_type
            .chars()
            .filter(|c| !c.is_control())
            .take(64)
            .collect();
        let known =
            self.with_db(|c| known_hosts::trust(c, host, req.port, &key_type, fingerprint))?;
        self.host_keys
            .set(&known.host, known.port, &known.fingerprint);
        tracing::info!(target: "activity",
            host = %known.host,
            port = known.port,
            fingerprint = %known.fingerprint,
            "SSH host key accepted"
        );
        Ok(known)
    }

    async fn forget_host(&self, id: Id) -> Result<()> {
        let removed = self.with_db(|c| {
            let host = known_hosts::list(c)?.into_iter().find(|h| h.id == id);
            if host.is_some() {
                known_hosts::forget(c, id)?;
            }
            Ok(host)
        })?;
        let Some(host) = removed else {
            return Err(Error::Missing(format!("no accepted host key with id {id}")));
        };
        self.host_keys.forget(&host.host, host.port);
        tracing::info!(target: "activity", host = %host.host, port = host.port, "SSH host key forgotten");
        Ok(())
    }

    async fn add(&self, req: AddRequest) -> Result<Download> {
        // Said before anything is looked up: a checksum that cannot be right is the user's to fix.
        let checksum = req
            .checksum
            .as_ref()
            .map(|c| normalize_checksum(c.algo, &c.value).map(|v| (c.algo, v)))
            .transpose()
            .map_err(Error::InvalidInput)?;
        // A login written into the address is kept apart from it: the address is stored and shown,
        // the password is neither.
        let (url, login) = split_login(&req.url, req.credentials.as_ref())?;
        let info = self
            .probe_remote(&url, login.as_ref())
            .await
            .inspect_err(|e| {
                tracing::warn!(from = %logfmt::host(&url), error = %e, "cannot add the download");
            })?;
        let name = sanitize(req.filename.as_deref().unwrap_or(&info.filename));
        let category = req
            .category
            .unwrap_or_else(|| detect_category(&name, info.content_type.as_deref()));
        // Decided once, now: later setting changes or category edits never move a download.
        let subdir = if self.current_settings().category_subdirs {
            category.as_str().to_owned()
        } else {
            String::new()
        };
        let from = logfmt::host(&url);
        if let Some(at) = req.start_at {
            let now = clock::now();
            if req.start_now || at > now + MAX_START_AHEAD_SECS {
                return Err(Error::InvalidInput(
                    "start_at must be at most 31 days ahead and not together with start_now".into(),
                ));
            }
            if at < now - 60 {
                return Err(Error::InvalidInput("start_at is in the past".into()));
            }
        }

        let (id, hold) = {
            let _guard = self.add_lock.lock().await;
            // Reserve room for it: tell the user now, not when the download dies halfway. Inside
            // the lock, so two adds cannot both claim the last free gigabytes.
            // `ignore_space`: the user knows better (files may be moved away before it starts).
            let hold = match (info.size, req.ignore_space) {
                (Some(size), false) => self.check_room(&subdir, size).inspect_err(|e| {
                    tracing::info!(target: "activity", %name, error = %e, "download refused: it does not fit on the disk");
                })?,
                _ => false,
            };
            let found = self.detect_conflict(&url, &subdir, &name).await?;
            let (filename, overwrite) = match (found.conflict, req.on_conflict) {
                (None, _) => (name, false),
                (Some(c), OnConflict::Reject) => return Err(Error::Conflict(Box::new(c))),
                (Some(c), OnConflict::Rename) => (c.suggested_name, false),
                (Some(_), OnConflict::Overwrite) => {
                    if found.name_reserved {
                        return Err(Error::InvalidInput(
                            "cannot overwrite a file that another download is still producing"
                                .into(),
                        ));
                    }
                    (name, found.name_taken)
                }
            };
            let source_kind = SourceKind::for_url(&url).unwrap_or(SourceKind::Http);
            let id = self.with_db(|c| {
                let id = downloads::insert(
                    c,
                    &NewDownload {
                        url,
                        filename: filename.clone(),
                        category,
                        source_kind,
                        overwrite,
                        ignore_space: req.ignore_space,
                        subdir,
                        start_at: req.start_at,
                        start_now: req.start_now,
                        checksum,
                        credentials: login,
                    },
                )?;
                downloads::set_remote_info(c, id, info.size, info.resumable, info.etag.as_deref())?;
                if hold {
                    // Really no room: keep the entry, but do not start it.
                    downloads::set_status(c, id, Status::Paused, None)?;
                    downloads::set_waiting_space(c, id, true)?;
                }
                Ok(id)
            })?;
            (id, hold)
        };

        if hold {
            tracing::info!(id, "not enough free disk space: added but held back");
            self.publish_download(id);
            return self.get(id).map(|d| self.view(d));
        }
        tracing::info!(target: "activity",
            id,
            name = %self.name_of(id),
            category = category.as_str(),
            size = %info.size.map_or_else(|| "unknown".to_owned(), logfmt::bytes),
            from = %from,
            "download added"
        );
        let start = self.admit(id);
        self.publish_download(id);
        self.launch(start).await;
        self.get(id).map(|d| self.view(d))
    }

    async fn start_now(&self, id: Id) -> Result<Download> {
        let d = self.get(id)?;
        if !matches!(d.status, Status::Queued | Status::Downloading) {
            return Err(Error::InvalidState(format!(
                "a {} download is not waiting to start",
                d.status.as_str()
            )));
        }
        self.with_db(|c| downloads::set_start_now(c, id).map(|_| ()))?;
        tracing::info!(target: "activity", id, name = %d.filename, "download now: the download hours no longer apply to it");
        let start = if self.held().remove(&id) {
            self.sched().enqueue(id)
        } else {
            Vec::new()
        };
        self.publish_download(id);
        self.launch(start).await;
        self.get(id).map(|d| self.view(d))
    }

    async fn pause(&self, id: Id) -> Result<Download> {
        let d = self.get(id)?;
        if !matches!(d.status, Status::Queued | Status::Downloading) {
            return Err(Error::InvalidState(format!(
                "cannot pause a {} download",
                d.status.as_str()
            )));
        }
        // The user's pause is a pause, even when the download hours were about to stop it.
        self.suspended().remove(&id);
        if self.sched().is_running(id) {
            // The engine answers with a `Paused` event that persists offsets and frees the slot.
            self.engine.pause(id).await?;
        } else {
            self.held().remove(&id);
            let next = self.sched().release(id);
            self.with_db(|c| downloads::set_status(c, id, Status::Paused, None).map(|_| ()))?;
            self.publish_download(id);
            self.launch(next).await;
        }
        self.get(id).map(|d| self.view(d))
    }

    async fn resume(&self, id: Id) -> Result<Download> {
        let mut d = self.get(id)?;
        if !matches!(d.status, Status::Paused | Status::Failed) {
            return Err(Error::InvalidState(format!(
                "cannot resume a {} download",
                d.status.as_str()
            )));
        }
        // The file did not pass its verification: there is nothing to continue from. The bad file
        // goes and the download starts again from the beginning.
        if d.verify_failed {
            self.delete_finished_file(&d).await?;
            self.with_db(|c| {
                segments::replace(c, id, &[])?;
                downloads::reset_for_redownload(c, id).map(|_| ())
            })?;
            d = self.get(id)?;
        }
        // Resuming into a disk that is still really full would only fail again (or fill it): say so
        // and leave the download as it is. The user's own `ignore_space` choice is respected.
        if let Some(size) = d.size {
            let ignore = self.with_db(|c| downloads::ignore_space_flag(c, id))?;
            let needed = size.saturating_sub(d.downloaded);
            if !ignore {
                if let Ok(v) = fetchbox_http_engine::disk::volume(&self.engine.dir_for(&d.subdir)) {
                    if needed > v.space.available {
                        return Err(Error::InsufficientSpace {
                            needed,
                            free: v.space.available,
                            reserved: 0,
                        });
                    }
                }
            }
        }
        self.with_db(|c| downloads::set_status(c, id, Status::Queued, None).map(|_| ()))?;
        tracing::info!(target: "activity",
            id,
            name = %d.filename,
            downloaded = %logfmt::bytes(d.downloaded),
            "{}",
            if d.status == Status::Failed { "download retried" } else { "download resumed" }
        );
        let start = self.admit(id);
        self.publish_download(id);
        self.launch(start).await;
        self.get(id).map(|d| self.view(d))
    }

    async fn cancel(&self, id: Id) -> Result<Download> {
        let d = self.get(id)?;
        if d.status == Status::Verifying {
            return Err(Error::InvalidState(
                "the file is complete and being verified: skip the verification, or remove the download"
                    .into(),
            ));
        }
        if d.status.is_terminal() {
            return Err(Error::InvalidState(format!(
                "cannot cancel a {} download",
                d.status.as_str()
            )));
        }
        self.do_cancel(&d).await?;
        self.get(id)
    }

    async fn remove(&self, id: Id, delete_file: bool) -> Result<()> {
        let d = self.get(id)?;
        if d.status == Status::Verifying {
            // The file is complete; only the reading of it is stopped.
            self.stop_verification(id);
        } else if !d.status.is_terminal() {
            self.do_cancel(&d).await?;
        }
        // The file first: if it cannot be deleted the entry stays, so the user can try again.
        let has_file = Self::has_finished_file(&d);
        if delete_file && has_file {
            self.delete_finished_file(&d).await?;
        }
        // What the download came to, now that it is over (a cancelled one has stopped counting).
        let fetched = self.get(id).map_or(d.downloaded, |now| now.downloaded);
        tracing::info!(target: "activity",
            id,
            name = %d.filename,
            was = d.status.as_str(),
            size = %d.size.map_or_else(|| "unknown".to_owned(), logfmt::bytes),
            fetched = %logfmt::bytes(fetched),
            average = %self.average_of(id, fetched),
            file_deleted = delete_file && has_file,
            "download removed from the list"
        );
        self.with_db(|c| downloads::delete(c, id).map(|_| ()))?;
        self.publish(Event::Removed { id });
        Ok(())
    }

    async fn keep(&self, id: Id) -> Result<Download> {
        let d = self.get(id)?;
        let failed_check = d.status == Status::Failed && d.verify_failed;
        if d.status != Status::Verifying && !failed_check {
            return Err(Error::InvalidState(format!(
                "a {} download has no verification to skip",
                d.status.as_str()
            )));
        }
        self.stop_verification(id);
        self.with_db(|c| {
            // Skipping a verification leaves no verdict. Keeping a file that did not match leaves
            // it: the download is completed, but remembers (and shows) that its checksum differed.
            if !failed_check {
                downloads::clear_verification(c, id)?;
            }
            downloads::transition(
                c,
                id,
                &[Status::Verifying, Status::Failed],
                Status::Completed,
                None,
            )
            .map(|_| ())
        })?;
        tracing::info!(target: "activity",
            id,
            name = %d.filename,
            "{}",
            if failed_check { "file kept although its checksum did not match" } else { "verification skipped, the file is kept" }
        );
        self.publish_download(id);
        self.get(id).map(|d| self.view(d))
    }

    async fn set_speed_limit(&self, id: Id, rate: u64) -> Result<Download> {
        let d = self.get(id)?;
        if !matches!(
            d.status,
            Status::Queued | Status::Downloading | Status::Paused | Status::Failed
        ) {
            return Err(Error::InvalidState(format!(
                "a {} download has no speed to limit",
                d.status.as_str()
            )));
        }
        self.with_db(|c| downloads::set_speed_limit(c, id, rate))?;
        self.engine.set_speed_limit(id, rate); // live, if it is running
        tracing::info!(target: "activity", id, name = %d.filename, limit = %logfmt::limit(rate), "speed of the download changed");
        self.publish_download(id);
        self.get(id).map(|d| self.view(d))
    }

    async fn set_archived(&self, id: Id, archived: bool) -> Result<Download> {
        let d = self.get(id)?;
        if archived && !d.status.is_terminal() {
            return Err(Error::InvalidState(format!(
                "cannot archive a {} download",
                d.status.as_str()
            )));
        }
        self.with_db(|c| downloads::set_archived(c, id, archived))?;
        tracing::info!(target: "activity", id, name = %d.filename, "{}", if archived { "download archived" } else { "download restored from the archive" });
        self.publish_download(id);
        self.get(id).map(|d| self.view(d))
    }

    async fn set_category(&self, id: Id, category: Category) -> Result<Download> {
        if !self.with_db(|c| downloads::set_category(c, id, category))? {
            return Err(Error::NotFound(id));
        }
        tracing::info!(target: "activity", id, name = %self.name_of(id), category = category.as_str(), "category of the download changed");
        self.publish_download(id);
        self.get(id).map(|d| self.view(d))
    }

    async fn disk_space(&self) -> Result<DiskSpace> {
        let vol = fetchbox_http_engine::disk::volume(&self.dir).map_err(io_err)?;
        let mut space = vol.space;
        space.reserved = self.reserved_on(vol.dev)?;
        Ok(space)
    }

    async fn stats(&self, range: StatsRange, offset_minutes: i32) -> Result<Stats> {
        let now = std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
        // Real time zones run from UTC-12 to UTC+14.
        let offset = i64::from(offset_minutes.clamp(-14 * 60, 14 * 60)) * 60;
        Ok(self.with_db(|c| stats::summary(c, now, range, offset))?)
    }

    async fn settings(&self) -> Result<Settings> {
        Ok(self.current_settings())
    }

    async fn update_settings(&self, new: Settings) -> Result<Settings> {
        new.validate()?;
        if clock::zone(&new.schedule_timezone).is_none() {
            return Err(Error::InvalidInput(format!(
                "unknown time zone {:?} (use a name like Europe/Warsaw)",
                new.schedule_timezone
            )));
        }
        if clock::zone(&new.torrent_schedule_timezone).is_none() {
            return Err(Error::InvalidInput(format!(
                "unknown time zone {:?} (use a name like Europe/Warsaw)",
                new.torrent_schedule_timezone
            )));
        }
        let old = self.current_settings();
        self.with_db(|c| settings::save(c, &new))?;
        *self.settings.lock().unwrap_or_else(|e| e.into_inner()) = new.clone();
        let changes = logfmt::settings_changes(&old, &new);
        if changes.is_empty() {
            tracing::debug!("settings saved, nothing changed");
        } else {
            tracing::info!(target: "activity", changes = %changes.join("; "), "settings changed");
            // The hours may have moved or been switched: say how they stand once more.
            *self.hours_seen.lock().unwrap_or_else(|e| e.into_inner()) = None;
            *self
                .torrents
                .hours_seen
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = None;
        }
        // Live: limits touch running downloads, the slot limit only gates new ones.
        self.engine.limiter().set_global(new.speed_global);
        self.engine.set_default_speed(new.speed_per_download);
        self.apply_torrent_limits(&new);
        let start = self.sched().set_max_concurrent(new.max_concurrent);
        self.publish(Event::Settings {
            settings: new.clone(),
        });
        self.launch(start).await;
        // New download hours (or none) may let waiting downloads go, or stop running ones.
        self.reevaluate().await;
        Ok(new)
    }

    async fn torrents(&self) -> Result<Vec<Torrent>> {
        self.torrents_list().await
    }

    async fn add_torrent_magnet(&self, req: AddTorrentRequest) -> Result<Torrent> {
        self.torrent_add_magnet(req).await
    }

    async fn add_torrent_file(&self, bytes: Vec<u8>) -> Result<Torrent> {
        self.torrent_add_file(bytes).await
    }

    async fn pause_torrent(&self, id: Id) -> Result<Torrent> {
        self.torrent_pause(id).await
    }

    async fn resume_torrent(&self, id: Id) -> Result<Torrent> {
        self.torrent_resume(id).await
    }

    async fn start_torrent_now(&self, id: Id) -> Result<Torrent> {
        self.torrent_start_now(id).await
    }

    async fn remove_torrent(&self, id: Id, delete_files: bool) -> Result<()> {
        self.torrent_remove(id, delete_files).await
    }

    async fn torrent_peers(&self, id: Id) -> Result<TorrentPeers> {
        self.torrent_peers_of(id).await
    }

    async fn torrent_files(&self, id: Id) -> Result<Vec<TorrentFile>> {
        self.torrent_files_of(id).await
    }

    fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.bus.subscribe()
    }

    fn closing(&self) -> watch::Receiver<bool> {
        self.closing.subscribe()
    }
}

/// The password hash created in the first-run setup lives next to the settings.
impl fetchbox_api::auth::CredentialStore for Manager {
    fn load_password_hash(&self) -> std::result::Result<Option<String>, String> {
        settings::password_hash(&self.db()).map_err(|e| e.to_string())
    }

    fn save_password_hash(&self, hash: &str) -> std::result::Result<(), String> {
        settings::set_password_hash(&self.db(), hash).map_err(|e| e.to_string())
    }
}
