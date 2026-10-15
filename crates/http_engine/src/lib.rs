//! Segmented HTTP download engine with `Range`, pause and resume (SPEC 6.2).
//!
//! The engine owns no database. It reports what happened through [`Event`]s on a
//! channel; the server wiring persists them (batched, never on the hot path).

pub mod disk;
pub mod error;
pub mod policy;
pub mod probe;
pub mod segments;
pub mod worker;

#[cfg(test)]
mod testutil;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use dlm_core::filename::sanitize;
use dlm_core::limiter::{DownloadLimiter, RateLimiter};
use dlm_core::{Downloader, Handle, Id, Job, Progress, SegmentState};
use reqwest::Client;
use tokio::sync::mpsc;
use tokio::task::{JoinHandle, JoinSet};

pub use error::EngineError;
pub use policy::UrlPolicy;
pub use probe::{probe, validate_url, RemoteInfo};
use worker::SegmentSpec;

/// How often progress is reported (and offsets should be persisted).
const TICK: Duration = Duration::from_secs(1);

/// What the engine tells the outside world.
#[derive(Debug, Clone)]
pub enum Event {
    /// Remote file inspected, plan fixed. `segments` is empty for non-resumable downloads.
    Started {
        id: Id,
        size: Option<u64>,
        resumable: bool,
        etag: Option<String>,
        segments: Vec<SegmentState>,
    },
    /// Periodic offsets snapshot; persist it to be able to resume after a restart.
    Progress {
        id: Id,
        downloaded: u64,
        segments: Vec<SegmentState>,
    },
    /// Final snapshot after `pause`.
    Paused {
        id: Id,
        downloaded: u64,
        segments: Vec<SegmentState>,
    },
    /// File finished and renamed to `path`.
    Completed { id: Id, path: PathBuf, size: u64 },
    /// Download stopped with an error; a `Progress` snapshot precedes it.
    Failed { id: Id, error: String },
}

/// HTTP implementation of [`Downloader`].
#[derive(Clone)]
pub struct HttpDownloader {
    inner: Arc<Inner>,
}

struct Inner {
    client: Client,
    policy: UrlPolicy,
    dir: PathBuf,
    limiter: RateLimiter,
    /// Speed limit of every download without one of its own, bytes/s (`0` = none). Read and
    /// written under the `entries` lock so a start cannot miss a change.
    default_rate: AtomicU64,
    events: mpsc::UnboundedSender<Event>,
    entries: Mutex<HashMap<Id, Entry>>,
}

/// The rate a download runs at: its own limit, else the default (`0` = unlimited).
fn effective_rate(own: u64, default: u64) -> u64 {
    if own > 0 {
        own
    } else {
        default
    }
}

struct Entry {
    job: Job,
    task: Option<JoinHandle<()>>,
    shared: Arc<Shared>,
    limiter: DownloadLimiter,
}

/// State visible to `progress()`; filled once the probe is done.
#[derive(Default)]
struct Shared {
    remote: OnceLock<Remote>,
    speed: AtomicU64,
}

struct Remote {
    size: Option<u64>,
    resumable: bool,
    etag: Option<String>,
    segs: Vec<Seg>,
}

struct Seg {
    idx: u32,
    start: u64,
    len: Option<u64>,
    done: AtomicU64,
}

impl Remote {
    fn downloaded(&self) -> u64 {
        self.segs
            .iter()
            .map(|s| s.done.load(Ordering::Relaxed))
            .sum()
    }

    /// Resumable view of the plan; empty for plain streams.
    fn snapshot(&self) -> Vec<SegmentState> {
        if !self.resumable {
            return Vec::new();
        }
        self.segs
            .iter()
            .filter_map(|s| {
                let len = s.len?;
                Some(SegmentState {
                    idx: s.idx,
                    start: s.start,
                    end: s.start + len - 1,
                    downloaded: s.done.load(Ordering::Relaxed).min(len),
                })
            })
            .collect()
    }
}

impl HttpDownloader {
    /// Create a downloader writing into `dir`. Events arrive on the returned receiver.
    pub fn new(
        dir: impl Into<PathBuf>,
        limiter: RateLimiter,
        policy: UrlPolicy,
    ) -> Result<(Self, mpsc::UnboundedReceiver<Event>), EngineError> {
        let mut builder = Client::builder()
            .user_agent(concat!("WebDownloadManager/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(30))
            .redirect(policy.redirect_policy());
        if let Some(resolver) = policy.resolver() {
            builder = builder.dns_resolver(resolver);
        }
        let client = builder.build()?;
        let (tx, rx) = mpsc::unbounded_channel();
        let inner = Inner {
            client,
            policy,
            dir: dir.into(),
            limiter,
            default_rate: AtomicU64::new(0),
            events: tx,
            entries: Mutex::new(HashMap::new()),
        };
        Ok((
            Self {
                inner: Arc::new(inner),
            },
            rx,
        ))
    }

    /// Shared handle to the global limiter (e.g. to apply settings live).
    pub fn limiter(&self) -> &RateLimiter {
        &self.inner.limiter
    }

    /// Validate `raw` and inspect the remote file (size, ranges, name) without downloading.
    pub async fn probe_url(&self, raw: &str) -> Result<RemoteInfo, EngineError> {
        let url = validate_url(raw)?;
        self.inner.policy.check(&url)?;
        probe(&self.inner.client, &url).await
    }

    /// Change the limit of one download live. `own` replaces the default speed (`0` = use the
    /// default). A download that is not running has nothing to change: its stored limit is
    /// passed in the job when it starts.
    pub fn set_speed_limit(&self, id: Id, own: u64) {
        let mut entries = self.inner.entries();
        if let Some(e) = entries.get_mut(&id) {
            e.job.speed_limit = own;
            e.limiter.set_rate(effective_rate(
                own,
                self.inner.default_rate.load(Ordering::Relaxed),
            ));
        }
    }

    /// Set the default speed of the downloads that have no limit of their own (`0` = none) and
    /// apply it to the running ones.
    pub fn set_default_speed(&self, rate: u64) {
        let entries = self.inner.entries();
        self.inner.default_rate.store(rate, Ordering::Relaxed);
        for e in entries.values().filter(|e| e.job.speed_limit == 0) {
            e.limiter.set_rate(rate);
        }
    }

    /// The directory a download with this `subdir` is written to (it may not exist yet).
    pub fn dir_for(&self, subdir: &str) -> PathBuf {
        target_dir(&self.inner.dir, subdir)
    }

    /// The `.part` file of a download: `<subdir>/<name>.<id>.part` (ids keep same-name
    /// downloads apart). It lives next to the final file so the finishing move stays on
    /// one filesystem.
    pub fn part_path(&self, id: Id, subdir: &str, filename: &str) -> PathBuf {
        part_path(&target_dir(&self.inner.dir, subdir), id, filename)
    }

    fn spawn(
        inner: &Arc<Inner>,
        job: Job,
        shared: Arc<Shared>,
        limiter: DownloadLimiter,
    ) -> JoinHandle<()> {
        let inner = Arc::clone(inner);
        tokio::spawn(async move {
            let id = job.id;
            let outcome = download(&inner, &job, &shared, &limiter).await;
            let event = match outcome {
                Ok((path, size)) => Event::Completed { id, path, size },
                Err(e) => {
                    tracing::warn!(id, error = %e, "download failed");
                    inner.emit_progress(id, &shared);
                    Event::Failed {
                        id,
                        error: e.to_string(),
                    }
                }
            };
            // Finished either way: forget it so `start` can be called again.
            inner.entries().remove(&id);
            inner.send(event);
        })
    }
}

impl Inner {
    /// Bytes the running downloads on file system `dev` (all but `except`) still have to write;
    /// unknown sizes count 0.
    fn running_remaining(&self, except: Option<Id>, dev: u64) -> u64 {
        let entries = self.entries();
        entries
            .iter()
            .filter(|(id, _)| Some(**id) != except)
            .filter(|(_, e)| {
                // A directory whose device cannot be read is assumed to be on the same disk.
                disk::volume(&target_dir(&self.dir, &e.job.subdir)).map_or(true, |v| v.dev == dev)
            })
            .filter_map(|(_, e)| e.shared.remote.get())
            .map(|r| r.size.map_or(0, |size| size.saturating_sub(r.downloaded())))
            .sum()
    }

    fn entries(&self) -> MutexGuard<'_, HashMap<Id, Entry>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn send(&self, event: Event) {
        // The receiver living shorter than the engine only happens during shutdown.
        let _ = self.events.send(event);
    }

    fn emit_progress(&self, id: Id, shared: &Shared) {
        if let Some(r) = shared.remote.get() {
            self.send(Event::Progress {
                id,
                downloaded: r.downloaded(),
                segments: r.snapshot(),
            });
        }
    }
}

/// Download directory plus the (sanitized) subdirectory, if any.
fn target_dir(base: &Path, subdir: &str) -> PathBuf {
    if subdir.is_empty() {
        base.to_owned()
    } else {
        base.join(sanitize(subdir))
    }
}

fn part_path(dir: &Path, id: Id, filename: &str) -> PathBuf {
    dir.join(format!("{}.{id}.part", sanitize(filename)))
}

/// `name`, then `stem (1).ext`, `stem (2).ext`, ... (extension kept).
pub fn candidates(dir: &Path, filename: &str) -> impl Iterator<Item = PathBuf> {
    let name = sanitize(filename);
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_owned(), format!(".{e}")),
        _ => (name.clone(), String::new()),
    };
    let dir = dir.to_owned();
    std::iter::once(dir.join(&name))
        .chain((1u32..).map(move |n| dir.join(format!("{stem} ({n}){ext}"))))
}

/// Move the finished `.part` to its final name without ever clobbering a file the
/// user did not agree to replace. Returns the final path.
///
/// A hard link fails atomically with `AlreadyExists`, which closes the window between
/// "is the name free?" and "take it" that a plain `rename` (silently overwriting on
/// Unix) would leave open for two downloads finishing together.
async fn finalize(
    dir: &Path,
    filename: &str,
    part: &Path,
    overwrite: bool,
) -> std::io::Result<PathBuf> {
    if overwrite {
        let target = sanitize(filename);
        let target = dir.join(target);
        tokio::fs::rename(part, &target).await?;
        return Ok(target);
    }
    for target in candidates(dir, filename) {
        match tokio::fs::hard_link(part, &target).await {
            Ok(()) => {
                tokio::fs::remove_file(part).await?;
                return Ok(target);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                // Filesystems without hard links (FAT, some network mounts): fall back to
                // check-then-rename, which is racy but still never replaces a seen file.
                tracing::debug!(error = %e, "hard link unavailable, using rename");
                return rename_if_free(dir, filename, part).await;
            }
        }
    }
    Err(std::io::Error::other("no free file name"))
}

async fn rename_if_free(dir: &Path, filename: &str, part: &Path) -> std::io::Result<PathBuf> {
    for target in candidates(dir, filename) {
        if tokio::fs::try_exists(&target).await? {
            continue;
        }
        tokio::fs::rename(part, &target).await?;
        return Ok(target);
    }
    Err(std::io::Error::other("no free file name"))
}

/// Probe, plan, fetch all segments, rename. Returns the final path and size.
async fn download(
    inner: &Arc<Inner>,
    job: &Job,
    shared: &Arc<Shared>,
    limiter: &DownloadLimiter,
) -> Result<(PathBuf, u64), EngineError> {
    let url = validate_url(&job.url)?;
    inner.policy.check(&url)?;
    let info = probe(&inner.client, &url).await?;
    let dir = target_dir(&inner.dir, &job.subdir);
    if !job.subdir.is_empty() {
        tokio::fs::create_dir_all(&dir).await?;
    }
    let part = part_path(&dir, job.id, &job.filename);

    // Resume only if the file is demonstrably the same and the .part is still there.
    let plan_end = job.segments.last().map(|s| s.end + 1);
    let reuse = info.resumable
        && !job.segments.is_empty()
        && job.etag == info.etag
        && plan_end == info.size
        && tokio::fs::metadata(&part).await.is_ok();

    // Refuse early when the rest of the file, plus what the other running downloads on the same
    // disk still have to write, cannot fit: otherwise the disk would fill up halfway and the
    // download would die with a bare write error. (The server reserves room for queued ones when
    // they are added.) Nothing can be checked without a known size, and a download the user added
    // anyway is left alone: they know better, and if the disk really fills up the write fails.
    if let (Some(size), false) = (info.size, job.ignore_space) {
        let have: u64 = if reuse {
            job.segments.iter().map(|s| s.downloaded).sum()
        } else {
            0
        };
        match disk::volume(&dir) {
            Ok(v) => disk::ensure_fits(
                size.saturating_sub(have),
                inner.running_remaining(Some(job.id), v.dev),
                v.space.available,
            )?,
            // Free space unknown (exotic file system): do not block downloads over it.
            Err(e) => tracing::warn!(error = %e, "cannot read free disk space; skipping the check"),
        }
    }

    let remote = if reuse {
        build_remote(&info, &job.segments)
    } else {
        let fresh = match (info.resumable, info.size) {
            (true, Some(size)) => segments::plan(size, job.connections),
            _ => Vec::new(),
        };
        let file = tokio::fs::File::create(&part).await?;
        if let Some(size) = info.size {
            file.set_len(size).await?;
        }
        drop(file);
        build_remote(&info, &fresh)
    };
    let _ = shared.remote.set(remote);
    let Some(remote) = shared.remote.get() else {
        return Err(EngineError::Protocol("remote state not set".into()));
    };

    inner.send(Event::Started {
        id: job.id,
        size: remote.size,
        resumable: remote.resumable,
        etag: remote.etag.clone(),
        segments: remote.snapshot(),
    });

    let mut set = JoinSet::new();
    for (i, seg) in remote.segs.iter().enumerate() {
        let spec = SegmentSpec {
            start: seg.start,
            len: seg.len,
            ranged: remote.resumable,
        };
        let (client, url, part, limiter) = (
            inner.client.clone(),
            url.clone(),
            part.clone(),
            limiter.clone(),
        );
        let shared = Arc::clone(shared);
        set.spawn(async move {
            let Some(remote) = shared.remote.get() else {
                return Err(EngineError::Protocol("remote state not set".into()));
            };
            worker::run(&client, &url, &part, spec, &remote.segs[i].done, &limiter).await
        });
    }

    let mut ticker = tokio::time::interval(TICK);
    ticker.tick().await; // the first tick fires immediately
    let mut last = (Instant::now(), remote.downloaded());
    while !set.is_empty() {
        tokio::select! {
            Some(joined) = set.join_next() => {
                joined.map_err(|e| EngineError::Protocol(format!("worker panicked: {e}")))??;
            }
            _ = ticker.tick() => {
                let (now, total) = (Instant::now(), remote.downloaded());
                let secs = now.duration_since(last.0).as_secs_f64().max(0.001);
                let speed = (total.saturating_sub(last.1) as f64 / secs) as u64;
                shared.speed.store(speed, Ordering::Relaxed);
                last = (now, total);
                inner.emit_progress(job.id, shared);
            }
        }
    }

    let total = remote.downloaded();
    if let Some(size) = remote.size {
        if total != size {
            return Err(EngineError::Protocol(format!(
                "size mismatch: got {total} of {size} bytes"
            )));
        }
    }
    let final_path = finalize(&dir, &job.filename, &part, job.overwrite).await?;
    shared.speed.store(0, Ordering::Relaxed);
    Ok((final_path, total))
}

fn build_remote(info: &RemoteInfo, planned: &[SegmentState]) -> Remote {
    let segs = if info.resumable {
        planned
            .iter()
            .map(|s| Seg {
                idx: s.idx,
                start: s.start,
                len: Some(s.len()),
                done: AtomicU64::new(s.downloaded),
            })
            .collect()
    } else {
        // Single plain stream, from the start.
        vec![Seg {
            idx: 0,
            start: 0,
            len: info.size,
            done: AtomicU64::new(0),
        }]
    };
    Remote {
        size: info.size,
        resumable: info.resumable,
        etag: info.etag.clone(),
        segs,
    }
}

#[async_trait]
impl Downloader for HttpDownloader {
    async fn start(&self, job: &Job) -> dlm_core::Result<Handle> {
        let url = validate_url(&job.url)?;
        self.inner.policy.check(&url)?;
        let shared = Arc::new(Shared::default());
        // Hold the lock across spawn so the task cannot finish before the entry exists.
        let mut entries = self.inner.entries();
        // A paused entry is replaced: the caller's job (built from persisted offsets) wins,
        // which makes resume-after-pause and resume-after-restart the same code path.
        if entries.get(&job.id).is_some_and(|e| e.task.is_some()) {
            return Err(dlm_core::Error::InvalidState(format!(
                "download {} is already active",
                job.id
            )));
        }
        let default_rate = self.inner.default_rate.load(Ordering::Relaxed);
        let limiter = self
            .inner
            .limiter
            .for_download(effective_rate(job.speed_limit, default_rate));
        let task = Self::spawn(
            &self.inner,
            job.clone(),
            Arc::clone(&shared),
            limiter.clone(),
        );
        entries.insert(
            job.id,
            Entry {
                job: job.clone(),
                task: Some(task),
                shared,
                limiter,
            },
        );
        Ok(Handle { id: job.id })
    }

    async fn pause(&self, id: Id) -> dlm_core::Result<()> {
        let mut entries = self.inner.entries();
        let entry = entries.get_mut(&id).ok_or(dlm_core::Error::NotFound(id))?;
        let task = entry
            .task
            .take()
            .ok_or_else(|| dlm_core::Error::InvalidState("download is already paused".into()))?;
        // Offsets only advance after a write completes, so aborting mid-write is safe:
        // resume rewrites at most the bytes of one chunk.
        task.abort();
        if let Some(r) = entry.shared.remote.get() {
            entry.job.segments = r.snapshot();
            entry.job.etag = r.etag.clone();
            self.inner.send(Event::Paused {
                id,
                downloaded: r.downloaded(),
                segments: entry.job.segments.clone(),
            });
        } else {
            self.inner.send(Event::Paused {
                id,
                downloaded: 0,
                segments: entry.job.segments.clone(),
            });
        }
        entry.shared.speed.store(0, Ordering::Relaxed);
        Ok(())
    }

    async fn resume(&self, id: Id) -> dlm_core::Result<()> {
        let mut entries = self.inner.entries();
        let entry = entries.get_mut(&id).ok_or(dlm_core::Error::NotFound(id))?;
        if entry.task.is_some() {
            return Err(dlm_core::Error::InvalidState(
                "download is not paused".into(),
            ));
        }
        let shared = Arc::new(Shared::default());
        entry.shared = Arc::clone(&shared);
        entry.task = Some(Self::spawn(
            &self.inner,
            entry.job.clone(),
            shared,
            entry.limiter.clone(),
        ));
        Ok(())
    }

    async fn cancel(&self, id: Id) -> dlm_core::Result<()> {
        let entry = self
            .inner
            .entries()
            .remove(&id)
            .ok_or(dlm_core::Error::NotFound(id))?;
        if let Some(task) = entry.task {
            task.abort();
        }
        let part = self.part_path(id, &entry.job.subdir, &entry.job.filename);
        match tokio::fs::remove_file(&part).await {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(EngineError::Io(e).into()),
        }
        Ok(())
    }

    fn progress(&self, id: Id) -> Progress {
        let entries = self.inner.entries();
        let Some(entry) = entries.get(&id) else {
            return Progress::default();
        };
        match entry.shared.remote.get() {
            Some(r) => Progress {
                downloaded: r.downloaded(),
                total: r.size,
                speed: entry.shared.speed.load(Ordering::Relaxed),
            },
            None => Progress::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TestServer;
    use std::time::Duration;

    fn job(id: Id, server: &TestServer, connections: u32) -> Job {
        Job {
            id,
            url: server.url("file.bin"),
            filename: "file.bin".into(),
            connections,
            speed_limit: 0,
            etag: None,
            segments: Vec::new(),
            overwrite: false,
            ignore_space: false,
            subdir: String::new(),
        }
    }

    async fn next_terminal(rx: &mut mpsc::UnboundedReceiver<Event>) -> Event {
        loop {
            let ev = tokio::time::timeout(Duration::from_secs(20), rx.recv())
                .await
                .expect("timed out waiting for event")
                .expect("channel closed");
            if matches!(ev, Event::Completed { .. } | Event::Failed { .. }) {
                return ev;
            }
        }
    }

    #[tokio::test]
    async fn segmented_download_assembles_correct_file() {
        let server = TestServer::spawn(3 * 1024 * 1024 + 7, true, Duration::ZERO).await;
        let dir = tempfile::tempdir().expect("tmp");
        let (dl, mut rx) =
            HttpDownloader::new(dir.path(), RateLimiter::new(), UrlPolicy::default()).expect("new");
        dl.start(&job(1, &server, 4)).await.expect("start");

        let Event::Completed { path, size, .. } = next_terminal(&mut rx).await else {
            panic!("expected completion");
        };
        assert_eq!(size, server.len() as u64);
        assert_eq!(std::fs::read(&path).expect("read"), server.body());
        assert_eq!(path.file_name().and_then(|n| n.to_str()), Some("file.bin"));
        assert!(!dir.path().join("file.bin.1.part").exists());
    }

    #[tokio::test]
    async fn server_without_ranges_falls_back_to_single_stream() {
        let server = TestServer::spawn(600_000, false, Duration::ZERO).await;
        let dir = tempfile::tempdir().expect("tmp");
        let (dl, mut rx) =
            HttpDownloader::new(dir.path(), RateLimiter::new(), UrlPolicy::default()).expect("new");
        dl.start(&job(1, &server, 4)).await.expect("start");

        let mut started = None;
        let done = loop {
            match next_terminal_or_started(&mut rx, &mut started).await {
                Some(ev) => break ev,
                None => continue,
            }
        };
        let Some(Event::Started {
            resumable,
            segments,
            ..
        }) = started
        else {
            panic!("no Started event");
        };
        assert!(!resumable);
        assert!(segments.is_empty());
        let Event::Completed { path, .. } = done else {
            panic!("expected completion");
        };
        assert_eq!(std::fs::read(path).expect("read"), server.body());
    }

    async fn next_terminal_or_started(
        rx: &mut mpsc::UnboundedReceiver<Event>,
        started: &mut Option<Event>,
    ) -> Option<Event> {
        let ev = tokio::time::timeout(Duration::from_secs(20), rx.recv())
            .await
            .expect("timed out")
            .expect("closed");
        match ev {
            Event::Started { .. } => {
                *started = Some(ev);
                None
            }
            Event::Completed { .. } | Event::Failed { .. } => Some(ev),
            _ => None,
        }
    }

    #[tokio::test]
    async fn pause_and_resume_continue_from_offsets() {
        // ~4 MiB at 32 KiB / 10 ms per connection: slow enough to pause mid-flight.
        let server = TestServer::spawn(4 * 1024 * 1024, true, Duration::from_millis(10)).await;
        let dir = tempfile::tempdir().expect("tmp");
        let (dl, mut rx) =
            HttpDownloader::new(dir.path(), RateLimiter::new(), UrlPolicy::default()).expect("new");
        dl.start(&job(7, &server, 4)).await.expect("start");

        tokio::time::sleep(Duration::from_millis(250)).await;
        dl.pause(7).await.expect("pause");
        let at_pause = loop {
            match rx.recv().await.expect("closed") {
                Event::Paused {
                    downloaded,
                    segments,
                    ..
                } => break (downloaded, segments),
                Event::Completed { .. } => panic!("finished before pause; make server slower"),
                _ => {}
            }
        };
        assert!(
            at_pause.0 > 0 && at_pause.0 < server.len() as u64,
            "{}",
            at_pause.0
        );
        assert_eq!(
            at_pause.1.iter().map(|s| s.downloaded).sum::<u64>(),
            at_pause.0
        );
        assert!(matches!(
            dl.pause(7).await,
            Err(dlm_core::Error::InvalidState(_))
        ));

        dl.resume(7).await.expect("resume");
        let Event::Completed { path, .. } = next_terminal(&mut rx).await else {
            panic!("expected completion");
        };
        assert_eq!(std::fs::read(path).expect("read"), server.body());
    }

    #[tokio::test]
    async fn start_over_paused_entry_resumes_from_given_offsets() {
        let server = TestServer::spawn(4 * 1024 * 1024, true, Duration::from_millis(10)).await;
        let dir = tempfile::tempdir().expect("tmp");
        let (dl, mut rx) =
            HttpDownloader::new(dir.path(), RateLimiter::new(), UrlPolicy::default()).expect("new");
        let j = job(5, &server, 4);
        dl.start(&j).await.expect("start");
        tokio::time::sleep(Duration::from_millis(250)).await;
        dl.pause(5).await.expect("pause");
        let segments = loop {
            if let Event::Paused { segments, .. } = rx.recv().await.expect("closed") {
                break segments;
            }
        };
        // Same call the manager makes after reading offsets back from SQLite.
        let resumed = Job {
            etag: Some("\"v1\"".into()),
            segments,
            ..j
        };
        dl.start(&resumed).await.expect("restart over paused entry");
        let Event::Completed { path, .. } = next_terminal(&mut rx).await else {
            panic!("expected completion");
        };
        assert_eq!(std::fs::read(path).expect("read"), server.body());
    }

    #[tokio::test]
    async fn cancel_removes_part_file() {
        let server = TestServer::spawn(4 * 1024 * 1024, true, Duration::from_millis(10)).await;
        let dir = tempfile::tempdir().expect("tmp");
        let (dl, mut rx) =
            HttpDownloader::new(dir.path(), RateLimiter::new(), UrlPolicy::default()).expect("new");
        dl.start(&job(3, &server, 2)).await.expect("start");
        // Wait for the plan so the .part file exists.
        loop {
            if matches!(rx.recv().await.expect("closed"), Event::Started { .. }) {
                break;
            }
        }
        assert!(dir.path().join("file.bin.3.part").exists());
        dl.cancel(3).await.expect("cancel");
        assert!(!dir.path().join("file.bin.3.part").exists());
        assert!(matches!(
            dl.cancel(3).await,
            Err(dlm_core::Error::NotFound(3))
        ));
    }

    #[tokio::test]
    async fn name_collisions_get_a_suffix() {
        let server = TestServer::spawn(10_000, true, Duration::ZERO).await;
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::write(dir.path().join("file.bin"), b"existing").expect("write");
        let (dl, mut rx) =
            HttpDownloader::new(dir.path(), RateLimiter::new(), UrlPolicy::default()).expect("new");
        dl.start(&job(1, &server, 1)).await.expect("start");
        let Event::Completed { path, .. } = next_terminal(&mut rx).await else {
            panic!("expected completion");
        };
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some("file (1).bin")
        );
        assert_eq!(
            std::fs::read(dir.path().join("file.bin")).expect("read"),
            b"existing"
        );
    }

    #[tokio::test]
    async fn file_larger_than_the_disk_fails_before_anything_is_written() {
        // Ten exabytes (10^19 bytes): more than any disk, so the test needs no way to fill one.
        let server = TestServer::claiming(10_000_000_000_000_000_000).await;
        let dir = tempfile::tempdir().expect("tmp");
        let (dl, mut rx) =
            HttpDownloader::new(dir.path(), RateLimiter::new(), UrlPolicy::default()).expect("new");
        dl.start(&job(1, &server, 4)).await.expect("start");
        let mut started = None;
        let failed = loop {
            if let Some(ev) = next_terminal_or_started(&mut rx, &mut started).await {
                break ev;
            }
        };
        let Event::Failed { error, .. } = failed else {
            panic!("expected a failure, got {failed:?}");
        };
        assert!(error.starts_with("not enough disk space: "), "{error}");
        assert!(error.contains("9094947 TB needed"), "{error}");
        assert!(started.is_none(), "must fail before the plan is announced");
        // Nothing was created, in particular no sparse .part file of that size.
        assert_eq!(std::fs::read_dir(dir.path()).expect("ls").count(), 0);
    }

    #[tokio::test]
    async fn a_download_added_anyway_skips_the_space_check() {
        let dir = tempfile::tempdir().expect("tmp");
        let free = disk::volume(dir.path()).expect("volume").space.available;
        // Twice the free space (sparse, so nothing is really taken): would be refused otherwise.
        let server = TestServer::claiming(free.saturating_mul(2)).await;
        let (dl, mut rx) =
            HttpDownloader::new(dir.path(), RateLimiter::new(), UrlPolicy::default()).expect("new");

        dl.start(&job(1, &server, 1)).await.expect("start");
        let refused = next_terminal(&mut rx).await;
        assert!(
            matches!(&refused, Event::Failed { error, .. } if error.starts_with("not enough disk space")),
            "{refused:?}"
        );

        let mut anyway = job(2, &server, 1);
        anyway.ignore_space = true;
        dl.start(&anyway).await.expect("start");
        loop {
            match rx.recv().await.expect("closed") {
                Event::Started { id: 2, .. } => break, // got past the check
                Event::Failed { id: 2, error } => {
                    assert!(!error.starts_with("not enough disk space"), "{error}");
                    break; // failed for another reason (the fake server), but not for space
                }
                _ => {}
            }
        }
    }

    #[tokio::test]
    async fn running_remaining_follows_running_downloads() {
        let server = TestServer::spawn(256 * 1024, true, Duration::from_millis(40)).await;
        let dir = tempfile::tempdir().expect("tmp");
        let (dl, mut rx) =
            HttpDownloader::new(dir.path(), RateLimiter::new(), UrlPolicy::default()).expect("new");
        let dev = disk::volume(dir.path()).expect("volume").dev;
        assert_eq!(dl.inner.running_remaining(None, dev), 0);
        dl.start(&job(1, &server, 1)).await.expect("start");
        loop {
            if matches!(rx.recv().await.expect("closed"), Event::Started { .. }) {
                break;
            }
        }
        let left = dl.inner.running_remaining(None, dev);
        assert!(left > 0 && left <= server.len() as u64, "{left}");
        assert_eq!(
            dl.inner.running_remaining(Some(1), dev),
            0,
            "a download does not count against itself"
        );
        assert_eq!(
            dl.inner.running_remaining(None, dev.wrapping_add(1)),
            0,
            "other file systems are not affected"
        );
        assert!(matches!(
            next_terminal(&mut rx).await,
            Event::Completed { .. }
        ));
        assert_eq!(
            dl.inner.running_remaining(None, dev),
            0,
            "finished downloads need nothing"
        );
    }

    #[tokio::test]
    async fn http_error_fails_the_download() {
        let server = TestServer::spawn(10, true, Duration::ZERO).await;
        let dir = tempfile::tempdir().expect("tmp");
        let (dl, mut rx) =
            HttpDownloader::new(dir.path(), RateLimiter::new(), UrlPolicy::default()).expect("new");
        let mut j = job(1, &server, 1);
        j.url = server.url("missing/404");
        dl.start(&j).await.expect("start");
        assert!(matches!(next_terminal(&mut rx).await, Event::Failed { .. }));
        // The entry is gone, so the id can be started again.
        assert!(matches!(
            dl.pause(1).await,
            Err(dlm_core::Error::NotFound(1))
        ));
    }

    #[tokio::test]
    async fn rejects_non_http_urls() {
        let dir = tempfile::tempdir().expect("tmp");
        let (dl, _rx) =
            HttpDownloader::new(dir.path(), RateLimiter::new(), UrlPolicy::default()).expect("new");
        let j = Job {
            id: 1,
            url: "file:///etc/passwd".into(),
            filename: "x".into(),
            connections: 1,
            speed_limit: 0,
            etag: None,
            segments: Vec::new(),
            overwrite: false,
            ignore_space: false,
            subdir: String::new(),
        };
        assert!(matches!(
            dl.start(&j).await,
            Err(dlm_core::Error::InvalidInput(_))
        ));
    }

    #[test]
    fn candidates_keep_extension() {
        let dir = Path::new("/d");
        let got: Vec<PathBuf> = candidates(dir, "a.tar.gz").take(3).collect();
        assert_eq!(
            got,
            vec![
                PathBuf::from("/d/a.tar.gz"),
                PathBuf::from("/d/a.tar (1).gz"),
                PathBuf::from("/d/a.tar (2).gz")
            ]
        );
        let got: Vec<PathBuf> = candidates(dir, "README").take(2).collect();
        assert_eq!(got[1], PathBuf::from("/d/README (1)"));
    }

    #[tokio::test]
    async fn finalize_skips_existing_names() {
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::write(dir.path().join("a.txt"), b"keep").expect("w");
        std::fs::write(dir.path().join("a (1).txt"), b"keep").expect("w");
        let part = dir.path().join("a.txt.9.part");
        std::fs::write(&part, b"new").expect("w");
        let out = finalize(dir.path(), "a.txt", &part, false)
            .await
            .expect("fin");
        assert_eq!(out, dir.path().join("a (2).txt"));
        assert_eq!(std::fs::read(&out).expect("r"), b"new");
        assert_eq!(std::fs::read(dir.path().join("a.txt")).expect("r"), b"keep");
        assert!(!part.exists());
    }

    #[tokio::test]
    async fn finalize_overwrites_only_when_asked() {
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::write(dir.path().join("a.txt"), b"old").expect("w");
        let part = dir.path().join("a.txt.9.part");
        std::fs::write(&part, b"new").expect("w");
        let out = finalize(dir.path(), "a.txt", &part, true)
            .await
            .expect("fin");
        assert_eq!(out, dir.path().join("a.txt"));
        assert_eq!(std::fs::read(&out).expect("r"), b"new");
    }

    #[tokio::test]
    async fn concurrent_finalize_never_loses_a_file() {
        let dir = tempfile::tempdir().expect("tmp");
        let mut tasks = Vec::new();
        for i in 0..16u8 {
            let part = dir.path().join(format!("a.txt.{i}.part"));
            std::fs::write(&part, [i]).expect("w");
            let d = dir.path().to_owned();
            tasks.push(tokio::spawn(async move {
                finalize(&d, "a.txt", &part, false).await.expect("fin")
            }));
        }
        let mut paths = Vec::new();
        for t in tasks {
            paths.push(t.await.expect("join"));
        }
        paths.sort();
        paths.dedup();
        assert_eq!(paths.len(), 16, "two downloads ended up on the same file");
        let mut contents: Vec<u8> = paths
            .iter()
            .map(|p| std::fs::read(p).expect("r")[0])
            .collect();
        contents.sort();
        assert_eq!(contents, (0..16).collect::<Vec<u8>>());
    }

    #[tokio::test]
    async fn subdirectory_holds_part_and_final_file() {
        let server = TestServer::spawn(4 * 1024 * 1024, true, Duration::from_millis(10)).await;
        let dir = tempfile::tempdir().expect("tmp");
        let (dl, mut rx) =
            HttpDownloader::new(dir.path(), RateLimiter::new(), UrlPolicy::default()).expect("new");
        let j = Job {
            subdir: "video".into(),
            ..job(4, &server, 2)
        };
        dl.start(&j).await.expect("start");
        loop {
            if matches!(rx.recv().await.expect("closed"), Event::Started { .. }) {
                break;
            }
        }
        assert!(dir.path().join("video/file.bin.4.part").exists());
        assert_eq!(
            dl.part_path(4, "video", "file.bin"),
            dir.path().join("video/file.bin.4.part")
        );
        dl.cancel(4).await.expect("cancel");
        assert!(!dir.path().join("video/file.bin.4.part").exists());

        // A finished download ends up in the subdirectory, not in the base directory.
        let done = Job { id: 5, ..j };
        dl.start(&done).await.expect("start");
        let Event::Completed { path, .. } = next_terminal(&mut rx).await else {
            panic!("expected completion");
        };
        assert_eq!(path, dir.path().join("video/file.bin"));
        assert!(!dir.path().join("file.bin").exists());
        assert_eq!(std::fs::read(path).expect("read"), server.body());
    }

    #[test]
    fn subdirectory_cannot_escape_the_download_dir() {
        let base = Path::new("/downloads");
        assert_eq!(target_dir(base, ""), PathBuf::from("/downloads"));
        assert_eq!(target_dir(base, "video"), PathBuf::from("/downloads/video"));
        assert_eq!(
            target_dir(base, "../../etc"),
            PathBuf::from("/downloads/etc")
        );
        assert_eq!(target_dir(base, "/etc"), PathBuf::from("/downloads/etc"));
    }

    #[tokio::test]
    async fn private_address_policy_is_enforced() {
        let server = TestServer::spawn(1000, true, Duration::ZERO).await;
        let dir = tempfile::tempdir().expect("tmp");
        let policy = UrlPolicy {
            block_private: true,
        };
        let (dl, _rx) = HttpDownloader::new(dir.path(), RateLimiter::new(), policy).expect("new");

        // IP literal: refused before any connection is made.
        let literal = server.url("file.bin");
        assert!(matches!(
            dl.probe_url(&literal).await,
            Err(EngineError::InvalidUrl(_))
        ));
        assert!(matches!(
            dl.start(&Job {
                url: literal,
                ..job(1, &server, 1)
            })
            .await,
            Err(dlm_core::Error::InvalidInput(_))
        ));

        // Host name that resolves to loopback: refused where it is resolved.
        let named = literal_to_localhost(&server.url("file.bin"));
        let err = dl.probe_url(&named).await.expect_err("must be blocked");
        assert!(matches!(err, EngineError::Http(_)), "{err}");
        assert!(err.to_string().contains("blocked"), "{err}");

        // Without the policy the very same server is reachable.
        let (open, _rx2) =
            HttpDownloader::new(dir.path(), RateLimiter::new(), UrlPolicy::default()).expect("new");
        assert!(open.probe_url(&server.url("file.bin")).await.is_ok());
    }

    fn literal_to_localhost(url: &str) -> String {
        url.replace("127.0.0.1", "localhost")
    }
}
