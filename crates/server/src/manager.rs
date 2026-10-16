//! Download manager: wires storage, scheduler and the HTTP engine, and implements the
//! API's [`Service`].
//!
//! The database is the source of truth. The engine reports progress over a channel;
//! this module persists it (once per tick, never from the download hot path) and
//! turns it into UI events.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, UNIX_EPOCH};

use crate::clock;
use crate::update::{self, UpdateChecker};
use async_trait::async_trait;
use fetchbox_api::service::Service;
use fetchbox_core::category::detect as detect_category;
use fetchbox_core::filename::sanitize;
use fetchbox_core::limiter::RateLimiter;
use fetchbox_core::schedule::Gate;
use fetchbox_core::scheduler::Scheduler;
use fetchbox_core::{
    AddRequest, Category, Conflict, ConflictKind, DiskSpace, Download, Downloader, Error, Event,
    Hold, Id, Job, OnConflict, OnHoursEnd, Result, Settings, SourceKind, Stats, StatsRange, Status,
    UrlInfo, UrlKind, VersionInfo,
};
use fetchbox_http_engine::{candidates, HttpDownloader, UrlPolicy};
use fetchbox_storage::downloads::{self, NewDownload};
use fetchbox_storage::{segments, settings, stats, Connection};
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
        Self::start_with(conn, dir, policy, source).await
    }

    /// Like `start`, with the address of the releases API given (`None`: GitHub).
    pub async fn start_with(
        conn: Connection,
        dir: PathBuf,
        policy: UrlPolicy,
        update_source: Option<String>,
    ) -> anyhow::Result<Arc<Self>> {
        let current = settings::load(&conn)?;
        let peak_speed = stats::peak_speed(&conn)?;
        let limiter = RateLimiter::with_global(current.speed_global);
        let (engine, rx) = HttpDownloader::new(&dir, limiter, policy)?;
        engine.set_default_speed(current.speed_per_download);
        let requeued = downloads::requeue_interrupted(&conn)?;
        if requeued > 0 {
            tracing::info!(count = requeued, "re-queued interrupted downloads");
        }
        let queued = downloads::list_by_status(&conn, Status::Queued)?;
        let (bus, _) = broadcast::channel(BUS_CAPACITY);

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
            held: Mutex::new(HashSet::new()),
            suspended: Mutex::new(HashSet::new()),
        });

        tokio::spawn(Arc::clone(&me).run_events(rx));

        let mut to_start = Vec::new();
        for d in queued {
            to_start.extend(me.admit(d.id));
        }
        me.launch(to_start).await;
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

    fn held(&self) -> MutexGuard<'_, HashSet<Id>> {
        self.held.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn suspended(&self) -> MutexGuard<'_, HashSet<Id>> {
        self.suspended.lock().unwrap_or_else(|e| e.into_inner())
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
            Vec::new()
        } else {
            self.sched().enqueue(id)
        }
    }

    /// Look at the clock again: let the waiting downloads whose time has come go, and stop the
    /// running ones the download hours no longer allow (they return to the queue, keeping what
    /// they have). Called every few seconds, and at once when the settings change.
    pub(crate) async fn reevaluate(&self) {
        let now = clock::now();
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
                download: self.view(d),
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
        let (d, segs, overwrite, ignore_space) = self.with_db(|c| {
            let d = downloads::get(c, id)?;
            let segs = segments::list(c, id)?;
            let overwrite = downloads::overwrite_flag(c, id)?;
            let ignore_space = downloads::ignore_space_flag(c, id)?;
            Ok((d, segs, overwrite, ignore_space))
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
                self.active_start(id);
                self.publish_download(id);
            }
            E::Progress {
                id,
                downloaded,
                segments: segs,
            } => {
                let pairs: Vec<(u32, u64)> = segs.iter().map(|s| (s.idx, s.downloaded)).collect();
                let ms = self.active_lap(id);
                let r = self.with_db(|c| {
                    downloads::set_downloaded(c, id, downloaded)?;
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
                let r = self.with_db(|c| {
                    stats::add_active_ms(c, id, ms)?;
                    downloads::set_downloaded(c, id, size)?;
                    downloads::set_size(c, id, size)?;
                    if let Some(name) = &name {
                        // The engine may have picked a different free name.
                        downloads::set_filename(c, id, name)?;
                    }
                    segments::replace(c, id, &[])?;
                    downloads::transition(c, id, &ACTIVE, Status::Completed, None)?;
                    Ok(())
                });
                self.log_err("completed", id, r);
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
        self.publish_download(d.id);
        self.launch(next).await;
        Ok(())
    }
}

impl Manager {
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
    async fn probe_remote(&self, url: &str) -> Result<fetchbox_http_engine::RemoteInfo> {
        self.engine.probe_url(url).await.map_err(|e| match e {
            fetchbox_http_engine::EngineError::InvalidUrl(m) => Error::InvalidInput(m),
            other => Error::Engine(format!("cannot reach {url}: {other}")),
        })
    }
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
        if !self.current_settings().check_updates {
            return Ok(update::disabled());
        }
        Ok(self.updates.info(refresh).await)
    }

    async fn probe(&self, url: &str) -> Result<UrlInfo> {
        let info = self.probe_remote(url).await?;
        let is_html = info.content_type.as_deref().is_some_and(|c| {
            let c = c.to_ascii_lowercase();
            c.starts_with("text/html") || c.starts_with("application/xhtml")
        });
        // A directory index is offered as a list to choose from. Reading it is an extra request, and
        // a failure of that one only means "treat it as a file": the add reports real problems.
        let listing = if is_html {
            self.engine.listing_url(url).await.ok().flatten()
        } else {
            None
        };
        let (kind, entries, truncated) = match listing {
            Some(l) => (UrlKind::Listing, l.entries, l.truncated),
            None => (UrlKind::File, Vec::new(), false),
        };
        Ok(UrlInfo {
            kind,
            category: detect_category(&info.filename, info.content_type.as_deref()),
            filename: info.filename,
            size: info.size,
            resumable: info.resumable,
            entries,
            truncated,
        })
    }

    async fn add(&self, req: AddRequest) -> Result<Download> {
        let info = self.probe_remote(&req.url).await?;
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
        let url = req.url.trim().to_owned();
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
                (Some(size), false) => self.check_room(&subdir, size)?,
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
            let id = self.with_db(|c| {
                let id = downloads::insert(
                    c,
                    &NewDownload {
                        url,
                        filename: filename.clone(),
                        category,
                        source_kind: SourceKind::Http,
                        overwrite,
                        ignore_space: req.ignore_space,
                        subdir,
                        start_at: req.start_at,
                        start_now: req.start_now,
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
        let d = self.get(id)?;
        if !matches!(d.status, Status::Paused | Status::Failed) {
            return Err(Error::InvalidState(format!(
                "cannot resume a {} download",
                d.status.as_str()
            )));
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
        let start = self.admit(id);
        self.publish_download(id);
        self.launch(start).await;
        self.get(id).map(|d| self.view(d))
    }

    async fn cancel(&self, id: Id) -> Result<Download> {
        let d = self.get(id)?;
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
        if !d.status.is_terminal() {
            self.do_cancel(&d).await?;
        }
        // The file first: if it cannot be deleted the entry stays, so the user can try again.
        if delete_file && d.status == Status::Completed {
            self.delete_finished_file(&d).await?;
        }
        self.with_db(|c| downloads::delete(c, id).map(|_| ()))?;
        self.publish(Event::Removed { id });
        Ok(())
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
        self.publish_download(id);
        self.get(id).map(|d| self.view(d))
    }

    async fn set_category(&self, id: Id, category: Category) -> Result<Download> {
        if !self.with_db(|c| downloads::set_category(c, id, category))? {
            return Err(Error::NotFound(id));
        }
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
        self.with_db(|c| settings::save(c, &new))?;
        *self.settings.lock().unwrap_or_else(|e| e.into_inner()) = new.clone();
        // Live: limits touch running downloads, the slot limit only gates new ones.
        self.engine.limiter().set_global(new.speed_global);
        self.engine.set_default_speed(new.speed_per_download);
        let start = self.sched().set_max_concurrent(new.max_concurrent);
        self.publish(Event::Settings {
            settings: new.clone(),
        });
        self.launch(start).await;
        // New download hours (or none) may let waiting downloads go, or stop running ones.
        self.reevaluate().await;
        Ok(new)
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
