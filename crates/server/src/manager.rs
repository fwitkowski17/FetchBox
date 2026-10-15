//! Download manager: wires storage, scheduler and the HTTP engine, and implements the
//! API's [`Service`].
//!
//! The database is the source of truth. The engine reports progress over a channel;
//! this module persists it (once per tick, never from the download hot path) and
//! turns it into UI events.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::UNIX_EPOCH;

use async_trait::async_trait;
use dlm_api::service::Service;
use dlm_core::category::detect as detect_category;
use dlm_core::filename::sanitize;
use dlm_core::limiter::RateLimiter;
use dlm_core::scheduler::Scheduler;
use dlm_core::{
    AddRequest, Category, Conflict, ConflictKind, DiskSpace, Download, Downloader, Error, Event,
    Id, Job, OnConflict, Result, Settings, SourceKind, Status,
};
use dlm_http_engine::{candidates, HttpDownloader, UrlPolicy};
use dlm_storage::downloads::{self, NewDownload};
use dlm_storage::{segments, settings, Connection};
use tokio::sync::{broadcast, mpsc};

/// Capacity of the UI event bus; slow subscribers get a lag error and resync.
const BUS_CAPACITY: usize = 256;

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
}

impl Manager {
    /// Build the manager, recover interrupted downloads and start the event loop.
    pub async fn start(
        conn: Connection,
        dir: PathBuf,
        policy: UrlPolicy,
    ) -> anyhow::Result<Arc<Self>> {
        let current = settings::load(&conn)?;
        let limiter = RateLimiter::with_global(current.speed_global);
        let (engine, rx) = HttpDownloader::new(&dir, limiter, policy)?;
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
        });

        tokio::spawn(Arc::clone(&me).run_events(rx));

        let mut to_start = Vec::new();
        for d in queued {
            to_start.extend(me.sched().enqueue(d.id));
        }
        me.launch(to_start).await;
        Ok(me)
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

    fn with_db<T>(&self, f: impl FnOnce(&mut Connection) -> dlm_storage::Result<T>) -> Result<T> {
        let mut conn = self.db();
        Ok(f(&mut conn)?)
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
        d
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
            speed_limit: s.speed_per_download,
            etag: d.etag,
            segments: if d.downloaded > 0 { segs } else { Vec::new() },
            overwrite,
            ignore_space,
        };
        self.engine.start(&job).await?;
        Ok(true)
    }

    // ---- engine events -------------------------------------------------------------

    async fn run_events(self: Arc<Self>, mut rx: mpsc::UnboundedReceiver<dlm_http_engine::Event>) {
        while let Some(ev) = rx.recv().await {
            self.handle_engine_event(ev).await;
        }
    }

    async fn handle_engine_event(&self, ev: dlm_http_engine::Event) {
        use dlm_http_engine::Event as E;
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
                self.publish_download(id);
            }
            E::Progress {
                id,
                downloaded,
                segments: segs,
            } => {
                let pairs: Vec<(u32, u64)> = segs.iter().map(|s| (s.idx, s.downloaded)).collect();
                let r = self.with_db(|c| {
                    downloads::set_downloaded(c, id, downloaded)?;
                    segments::update_offsets(c, id, &pairs)
                });
                self.log_err("progress", id, r);
                let p = self.engine.progress(id);
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
                let r = self.with_db(|c| {
                    downloads::set_downloaded(c, id, downloaded)?;
                    segments::update_offsets(c, id, &pairs)?;
                    downloads::transition(c, id, &ACTIVE, Status::Paused, None)?;
                    Ok(())
                });
                self.log_err("paused", id, r);
                self.publish_download(id);
                let next = self.sched().release(id);
                self.launch(next).await;
            }
            E::Completed { id, path, size } => {
                if !self.is_active(id) {
                    return;
                }
                let name = path.file_name().and_then(|n| n.to_str()).map(str::to_owned);
                let r = self.with_db(|c| {
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
                let r = self.with_db(|c| {
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
        let next = self.sched().release(d.id);
        match self.engine.cancel(d.id).await {
            Ok(()) | Err(Error::NotFound(_)) => {}
            Err(e) => return Err(e),
        }
        self.remove_part(d.id, &d.subdir, &d.filename).await;
        self.with_db(|c| {
            segments::replace(c, d.id, &[])?;
            downloads::set_status(c, d.id, Status::Cancelled, None).map(|_| ())
        })?;
        self.publish_download(d.id);
        self.launch(next).await;
        Ok(())
    }
}

impl Manager {
    /// Bytes that queued, running and paused downloads still have to write on
    /// file system `dev`: the space virtually reserved for them. Finished, failed and cancelled
    /// downloads reserve nothing, and a running one's reservation shrinks as the disk fills.
    fn reserved_on(&self, dev: u64) -> Result<u64> {
        let all = self.with_db(|c| downloads::list(c))?;
        // Category subdirectories may sit on other disks; look each directory up once.
        let mut same_disk: HashMap<String, bool> = HashMap::new();
        let mut total = 0u64;
        for d in all {
            if !matches!(
                d.status,
                Status::Queued | Status::Downloading | Status::Paused
            ) {
                continue;
            }
            let d = self.view(d);
            let Some(size) = d.size else { continue };
            let here = *same_disk.entry(d.subdir.clone()).or_insert_with(|| {
                dlm_http_engine::disk::volume(&self.engine.dir_for(&d.subdir))
                    .map_or(true, |v| v.dev == dev)
            });
            if here {
                total = total.saturating_add(size.saturating_sub(d.downloaded));
            }
        }
        Ok(total)
    }

    /// Refuse (`InsufficientSpace`) when `needed` more bytes do not fit next to what the other
    /// unfinished downloads on the same disk have reserved. A disk whose space cannot be read is
    /// not checked: that must not stop downloads.
    fn ensure_room(&self, subdir: &str, needed: u64) -> Result<()> {
        let vol = match dlm_http_engine::disk::volume(&self.engine.dir_for(subdir)) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "cannot read free disk space; skipping the check");
                return Ok(());
            }
        };
        let others = self.reserved_on(vol.dev)?;
        dlm_http_engine::disk::ensure_fits(needed, others, vol.space.available)?;
        Ok(())
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

    async fn add(&self, req: AddRequest) -> Result<Download> {
        let info = self.engine.probe_url(&req.url).await.map_err(|e| match e {
            dlm_http_engine::EngineError::InvalidUrl(m) => Error::InvalidInput(m),
            other => Error::Engine(format!("cannot reach {}: {other}", req.url)),
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
        let url = req.url.trim().to_owned();

        let id = {
            let _guard = self.add_lock.lock().await;
            // Reserve room for it: tell the user now, not when the download dies halfway. Inside
            // the lock, so two adds cannot both claim the last free gigabytes.
            // `ignore_space`: the user knows better (files may be moved away before it starts).
            if let (Some(size), false) = (info.size, req.ignore_space) {
                self.ensure_room(&subdir, size)?;
            }
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
            self.with_db(|c| {
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
                    },
                )?;
                downloads::set_remote_info(c, id, info.size, info.resumable, info.etag.as_deref())?;
                Ok(id)
            })?
        };

        let start = self.sched().enqueue(id);
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
        if self.sched().is_running(id) {
            // The engine answers with a `Paused` event that persists offsets and frees the slot.
            self.engine.pause(id).await?;
        } else {
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
        self.with_db(|c| downloads::set_status(c, id, Status::Queued, None).map(|_| ()))?;
        let start = self.sched().enqueue(id);
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

    async fn remove(&self, id: Id) -> Result<()> {
        let d = self.get(id)?;
        if !d.status.is_terminal() {
            self.do_cancel(&d).await?;
        }
        self.with_db(|c| downloads::delete(c, id).map(|_| ()))?;
        self.publish(Event::Removed { id });
        Ok(())
    }

    async fn set_category(&self, id: Id, category: Category) -> Result<Download> {
        if !self.with_db(|c| downloads::set_category(c, id, category))? {
            return Err(Error::NotFound(id));
        }
        self.publish_download(id);
        self.get(id).map(|d| self.view(d))
    }

    async fn disk_space(&self) -> Result<DiskSpace> {
        let vol = dlm_http_engine::disk::volume(&self.dir).map_err(io_err)?;
        let mut space = vol.space;
        space.reserved = self.reserved_on(vol.dev)?;
        Ok(space)
    }

    async fn settings(&self) -> Result<Settings> {
        Ok(self.current_settings())
    }

    async fn update_settings(&self, new: Settings) -> Result<Settings> {
        new.validate()?;
        self.with_db(|c| settings::save(c, &new))?;
        *self.settings.lock().unwrap_or_else(|e| e.into_inner()) = new.clone();
        // Live: limits touch running downloads, the slot limit only gates new ones.
        self.engine.limiter().set_global(new.speed_global);
        self.engine.set_speed_per_download(new.speed_per_download);
        let start = self.sched().set_max_concurrent(new.max_concurrent);
        self.publish(Event::Settings {
            settings: new.clone(),
        });
        self.launch(start).await;
        Ok(new)
    }

    fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.bus.subscribe()
    }
}

/// The password hash created in the first-run setup lives next to the settings.
impl dlm_api::auth::CredentialStore for Manager {
    fn load_password_hash(&self) -> std::result::Result<Option<String>, String> {
        settings::password_hash(&self.db()).map_err(|e| e.to_string())
    }

    fn save_password_hash(&self, hash: &str) -> std::result::Result<(), String> {
        settings::set_password_hash(&self.db(), hash).map_err(|e| e.to_string())
    }
}
