//! Downloads that a plugin's program makes (SPEC 6.15).
//!
//! Some sources cannot be fetched with a plain request: a video that is split into a video and an
//! audio stream, a page that needs a tool to find its file. Then the resolver has a `run` command,
//! and the download is that command running in a folder of its own beside the final place
//! (`.fetchbox-job-<id>`). To the rest of the program it is a download like any other: it takes a
//! slot of the scheduler, shows progress, can be paused, resumed (the command is run again; the
//! tools resume their own partial files), cancelled and retried, and it counts in the statistics.
//!
//! The command talks to FetchBox on its standard output, one line at a time:
//! `@fetchbox progress <downloaded> <total> <speed>` (numbers, `NA` for unknown) and
//! `@fetchbox file <path>` (the finished file). Everything else it prints is ignored; the last line
//! of its standard error is the reason when it fails.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fetchbox_core::disks::rel_dir;
use fetchbox_core::filename::sanitize;
use fetchbox_core::plugin::PluginState;
use fetchbox_core::{Download, Downloader, Error, Id, Progress, Result, Status};
use fetchbox_http_engine::{candidates, Event as E};
use fetchbox_plugins::expand;
use fetchbox_plugins::oneshot::last_line;
use fetchbox_plugins::proc::{self, ChildEnv};
use fetchbox_storage::downloads;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Child;
use tokio::sync::{mpsc, watch};

use super::Manager;

/// How a running download is asked to stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stop {
    Run,
    /// The user (or the hours) paused it: keep what is there.
    Pause,
    /// Cancelled: the folder goes too (the manager removes it).
    Cancel,
    /// The server is going down: end it quietly; it is re-queued at the next start.
    Shutdown,
}

/// A download whose program is running.
pub(super) struct Run {
    plugin: String,
    stop: watch::Sender<Stop>,
    finished: watch::Sender<bool>,
    progress: Mutex<Progress>,
}

/// The runs, by download id.
pub(super) type Runs = Mutex<HashMap<Id, Arc<Run>>>;

/// How often progress is reported.
const TICK: Duration = Duration::from_secs(1);
/// Time a stopped program gets to end before it is killed.
const GRACE: Duration = Duration::from_secs(2);
/// Standard error is kept up to this much, the end of it.
const STDERR_KEEP: usize = 16 * 1024;

/// A line the program wrote that means something to FetchBox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Msg {
    Progress {
        downloaded: u64,
        total: Option<u64>,
        speed: u64,
    },
    File(String),
}

fn number(token: Option<&str>) -> Option<u64> {
    let v: f64 = token?.parse().ok()?;
    (v.is_finite() && v >= 0.0).then_some(v as u64)
}

/// Read one line of the program's output. Anything that is not an `@fetchbox` line is `None`.
pub(super) fn parse_line(line: &str) -> Option<Msg> {
    let rest = line.trim().strip_prefix("@fetchbox ")?;
    if let Some(path) = rest.strip_prefix("file ") {
        let path = path.trim();
        return (!path.is_empty()).then(|| Msg::File(path.to_owned()));
    }
    let mut t = rest.strip_prefix("progress")?.split_whitespace();
    let downloaded = number(t.next()).unwrap_or(0);
    let total = number(t.next()).filter(|n| *n > 0);
    let speed = number(t.next()).unwrap_or(0);
    Some(Msg::Progress {
        downloaded,
        total,
        speed,
    })
}

/// The name the finished file gets: the planned name with the extension the program really made.
/// A planned extension that came from the preset is replaced by the real one.
pub(super) fn final_name(
    planned: &str,
    planned_ext: Option<&str>,
    actual_ext: Option<&str>,
) -> String {
    let lower = planned.to_ascii_lowercase();
    let stem = match planned_ext {
        Some(e) if lower.ends_with(&format!(".{}", e.to_ascii_lowercase())) => {
            &planned[..planned.len() - e.len() - 1]
        }
        _ => planned,
    };
    let name = match actual_ext.filter(|e| !e.is_empty()) {
        Some(a)
            if !stem
                .to_ascii_lowercase()
                .ends_with(&format!(".{}", a.to_ascii_lowercase())) =>
        {
            format!("{stem}.{a}")
        }
        _ => stem.to_owned(),
    };
    sanitize(&name)
}

/// Is this file one the program is still working on?
fn is_work_file(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.starts_with('.')
        || [
            ".part",
            ".ytdl",
            ".tmp",
            ".temp",
            ".crdownload",
            ".download",
        ]
        .iter()
        .any(|e| n.ends_with(e))
}

/// Find the file the program made: the one it announced, if that is inside the folder, else the
/// largest finished file in it. `Err` says what is wrong, for the download's error.
pub(super) fn find_made_file(
    staging: &Path,
    announced: Option<&str>,
) -> std::result::Result<PathBuf, String> {
    if let Some(a) = announced {
        let given = Path::new(a);
        let path = if given.is_absolute() {
            given.to_path_buf()
        } else {
            staging.join(given)
        };
        let (real, root) = (std::fs::canonicalize(&path), std::fs::canonicalize(staging));
        return match (real, root) {
            (Ok(real), Ok(root)) if real.starts_with(&root) && real.is_file() => Ok(real),
            (Ok(real), Ok(root)) if !real.starts_with(&root) => {
                Err("the program named a file outside its folder".into())
            }
            _ => Err("the program named a file that is not there".into()),
        };
    }
    let mut best: Option<(u64, PathBuf)> = None;
    for entry in std::fs::read_dir(staging)
        .map_err(|e| format!("cannot read its folder: {e}"))?
        .flatten()
    {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(meta) = entry.metadata() else { continue };
        // A link could lead anywhere; only real files count.
        let real_file = entry.file_type().is_ok_and(|t| t.is_file());
        if real_file
            && !is_work_file(&name)
            && best.as_ref().is_none_or(|(len, _)| meta.len() > *len)
        {
            best = Some((meta.len(), entry.path()));
        }
    }
    best.map(|(_, p)| p)
        .ok_or_else(|| "the program finished but left no file".to_owned())
}

/// Put the finished file into the download's folder under `name` (the next free name unless the
/// user chose to overwrite), and remove the working folder. Returns the final path and its size.
fn move_into_place(
    file: &Path,
    staging: &Path,
    target: &Path,
    name: &str,
    overwrite: bool,
) -> std::io::Result<(PathBuf, u64)> {
    let size = std::fs::metadata(file)?.len();
    // What the program wrote is on disk before it is called finished.
    if let Ok(f) = std::fs::File::open(file) {
        let _ = f.sync_all();
    }
    std::fs::create_dir_all(target)?;
    let placed = if overwrite {
        let to = target.join(name);
        std::fs::rename(file, &to)?;
        to
    } else {
        let mut placed = None;
        for cand in candidates(target, name).take(10_000) {
            // A hard link fails atomically when the name is taken, so two downloads that finish
            // together cannot take the same one.
            match std::fs::hard_link(file, &cand) {
                Ok(()) => {
                    let _ = std::fs::remove_file(file);
                    placed = Some(cand);
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(_) if !cand.exists() => {
                    // No hard links here (some file systems): move it instead.
                    std::fs::rename(file, &cand)?;
                    placed = Some(cand);
                    break;
                }
                Err(_) => {}
            }
        }
        placed.ok_or_else(|| std::io::Error::other("no free file name"))?
    };
    let _ = std::fs::remove_dir_all(staging);
    Ok((placed, size))
}

enum Line {
    Msg(String),
    Eof,
}

/// Read lines of at most 64 KiB; a longer one is cut (the program is not trusted to be polite).
async fn read_lines(out: tokio::process::ChildStdout, tx: mpsc::Sender<Line>) {
    const MAX: u64 = 64 * 1024;
    let mut reader = BufReader::new(out);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match (&mut reader).take(MAX).read_until(b'\n', &mut buf).await {
            Ok(0) | Err(_) => {
                let _ = tx.send(Line::Eof).await;
                return;
            }
            Ok(_) => {
                let text = String::from_utf8_lossy(&buf).trim().to_owned();
                if !text.is_empty() && tx.send(Line::Msg(text)).await.is_err() {
                    return;
                }
            }
        }
    }
}

/// Keep the end of standard error.
async fn read_tail(err: tokio::process::ChildStderr) -> String {
    let mut reader = BufReader::new(err);
    let mut tail: Vec<u8> = Vec::new();
    let mut chunk = vec![0u8; 8 * 1024];
    while let Ok(n) = reader.read(&mut chunk).await {
        if n == 0 {
            break;
        }
        tail.extend_from_slice(&chunk[..n]);
        if tail.len() > STDERR_KEEP {
            tail.drain(..tail.len() - STDERR_KEEP);
        }
    }
    String::from_utf8_lossy(&tail).into_owned()
}

/// What a running download needs from the manager once it has been prepared.
struct Plan {
    id: Id,
    staging: PathBuf,
    target: PathBuf,
    planned: String,
    planned_ext: Option<String>,
    overwrite: bool,
}

impl Manager {
    /// The progress of a download whose program is running, for the list and the progress events.
    pub(super) fn runner_progress(&self, id: Id) -> Option<Progress> {
        let run = self
            .plugins
            .runs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .cloned()?;
        let p = *run.progress.lock().unwrap_or_else(|e| e.into_inner());
        Some(p)
    }

    /// The working folder of a download made by a plugin.
    fn staging_of(&self, d: &Download) -> PathBuf {
        self.engine
            .dir_for(&rel_dir(&d.disk, &d.subdir))
            .join(format!(".fetchbox-job-{}", d.id))
    }

    /// Stop a running download (or tell the engine to).
    pub(super) async fn pause_any(&self, id: Id) -> Result<()> {
        let run = self
            .plugins
            .runs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .cloned();
        match run {
            Some(run) => {
                run.stop.send_replace(Stop::Pause);
                Ok(())
            }
            None => self.engine.pause(id).await,
        }
    }

    /// Cancel a download made by a plugin: end its program, wait for it, remove its folder.
    pub(super) async fn runner_cancel(&self, d: &Download) {
        let run = self
            .plugins
            .runs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&d.id)
            .cloned();
        if let Some(run) = run {
            run.stop.send_replace(Stop::Cancel);
            let mut done = run.finished.subscribe();
            let _ = tokio::time::timeout(GRACE * 3, done.wait_for(|f| *f)).await;
        }
        let staging = self.staging_of(d);
        if let Err(e) = tokio::fs::remove_dir_all(&staging).await {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(path = %staging.display(), error = %e, "cannot remove the working folder");
            }
        }
    }

    /// Pause the downloads a plugin is making (it is being switched off, replaced or removed). They
    /// stay in the list as paused and can be resumed when the plugin is back.
    pub(super) fn pause_runs_of(&self, plugin: &str) {
        let runs: Vec<Arc<Run>> = self
            .plugins
            .runs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .filter(|r| r.plugin == plugin)
            .cloned()
            .collect();
        for r in runs {
            r.stop.send_replace(Stop::Pause);
        }
    }

    /// The server is going down: end every program quietly (the downloads are re-queued at the next start).
    pub(super) async fn stop_runners(&self) {
        let runs: Vec<Arc<Run>> = self
            .plugins
            .runs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .cloned()
            .collect();
        for r in &runs {
            r.stop.send_replace(Stop::Shutdown);
        }
        for r in runs {
            let mut done = r.finished.subscribe();
            let _ = tokio::time::timeout(GRACE * 3, done.wait_for(|f| *f)).await;
        }
    }

    /// Start the program that makes a download. Everything that can be wrong is checked first, so
    /// the download fails with a clear reason; then the program runs in a task of its own.
    pub(super) async fn plugins_start_download(&self, id: Id) -> Result<bool> {
        let Some(me) = self.this.get().and_then(std::sync::Weak::upgrade) else {
            return Err(Error::Internal("the manager is shutting down".into()));
        };
        let (d, runner, overwrite) = self.with_db(|c| {
            Ok((
                downloads::get(c, id)?,
                downloads::runner(c, id)?,
                downloads::overwrite_flag(c, id)?,
            ))
        })?;
        let (Some(d), Some((runner, choice_id))) = (d, runner) else {
            return Ok(false);
        };
        if d.status != Status::Queued {
            return Ok(false);
        }
        let (plugin, resolver_id) = runner.split_once('/').unwrap_or((&runner, ""));
        // After a restart the queue is started before the plugins are loaded: the download waits
        // (it stays queued) and `wake_plugin_downloads` starts it when its plugin is ready.
        if self.plugins.host.get().is_none() {
            return Ok(false);
        }
        self.plugin_host()?;
        let (manifest, state) = {
            let entries = self.plugin_entries();
            let e = entries.get(plugin);
            (e.and_then(|e| e.manifest.clone()), e.map(|e| e.state))
        };
        if matches!(state, Some(PluginState::Preparing | PluginState::Starting)) {
            return Ok(false);
        }
        let (Some(m), Some(PluginState::Ready | PluginState::Running)) = (manifest, state) else {
            return Err(Error::InvalidState(format!(
                "the plugin \"{plugin}\" is not running: switch it on and resume the download"
            )));
        };
        let resolver = m
            .resolvers
            .iter()
            .find(|r| r.id == resolver_id)
            .ok_or_else(|| {
                Error::InvalidState(format!(
                    "{} has no resolver \"{resolver_id}\" any more",
                    m.name
                ))
            })?;
        let run = resolver
            .run
            .as_ref()
            .ok_or_else(|| Error::InvalidState(format!("{} cannot make files any more", m.name)))?;
        let choice = resolver
            .choices
            .iter()
            .find(|c| Some(c.id.as_str()) == choice_id.as_deref())
            .ok_or_else(|| {
                Error::InvalidState(format!(
                    "{} has no preset \"{}\" any more",
                    m.name,
                    choice_id.clone().unwrap_or_default()
                ))
            })?;
        let host = self.plugin_host()?;
        if let Err(missing) = self.choice_ready(host, plugin, &m, choice) {
            return Err(Error::InvalidState(format!(
                "\"{}\" needs {missing}: install it from the plugin's card, then resume",
                choice.label
            )));
        }
        // A disk that is not mounted is an empty folder on the system disk: do not fill that.
        self.disk_dir(&d.disk)?;

        let plan = Plan {
            id,
            staging: self.staging_of(&d),
            target: self.engine.dir_for(&rel_dir(&d.disk, &d.subdir)),
            planned: d.filename.clone(),
            planned_ext: choice.ext.clone(),
            overwrite,
        };
        tokio::fs::create_dir_all(&plan.staging)
            .await
            .map_err(|e| Error::Internal(e.to_string()))?;
        let env = self.run_env(plugin)?;
        let mut ctx = env.context();
        ctx.url = Some(&d.url);
        ctx.choice_args = &choice.args;
        ctx.out_dir = Some(&plan.staging);
        let program = expand::resolve_program(&run.command, &ctx).map_err(Error::from)?;
        let args = expand::expand_args(&run.args, &ctx).map_err(Error::from)?;
        let child_env: ChildEnv = env.child_env(Default::default());
        proc::prepare_dirs(&child_env.data_dir).map_err(|e| Error::Internal(e.to_string()))?;
        let name = program.file_name().map_or_else(
            || "the program".into(),
            |n| n.to_string_lossy().into_owned(),
        );
        let child = proc::command(&program, &args, &child_env)
            .spawn()
            .map_err(|e| Error::Engine(format!("cannot run {name}: {e}")))?;

        let handle = Arc::new(Run {
            plugin: plugin.to_owned(),
            stop: watch::channel(Stop::Run).0,
            finished: watch::channel(false).0,
            progress: Mutex::new(Progress {
                downloaded: d.downloaded,
                total: d.size,
                speed: 0,
            }),
        });
        self.plugins
            .runs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, Arc::clone(&handle));
        tokio::spawn(me.watch_download(plan, child, handle));
        Ok(true)
    }

    /// Watch the program of a download until it ends, and turn what it does into the events every
    /// download has.
    ///
    /// Boxed on purpose: starting a download, its events and the next start form a cycle of async
    /// calls, and naming one type in the cycle as `Send` is what lets the compiler see that the
    /// task may run on any thread.
    fn watch_download(
        self: Arc<Self>,
        plan: Plan,
        child: Child,
        run: Arc<Run>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
        Box::pin(async move { self.watch(plan, child, run).await })
    }

    async fn watch(self: Arc<Self>, plan: Plan, mut child: Child, run: Arc<Run>) {
        let id = plan.id;
        self.handle_engine_event(E::Started {
            id,
            size: None,
            resumable: true,
            etag: None,
            segments: Vec::new(),
        })
        .await;
        let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
            proc::kill(&mut child);
            self.finish_run(id, &run).await;
            self.handle_engine_event(E::Failed {
                id,
                error: "cannot talk to the program".into(),
            })
            .await;
            return;
        };
        drop(child.stdin.take());
        let (tx, mut lines) = mpsc::channel(64);
        let reader = tokio::spawn(read_lines(stdout, tx));
        let err_task = tokio::spawn(read_tail(stderr));

        let mut stop = run.stop.subscribe();
        let mut tick = tokio::time::interval(TICK);
        let mut announced: Option<String> = None;
        let mut reported = (0u64, None::<u64>);
        let mut open = true;
        let end = loop {
            tokio::select! {
                line = lines.recv(), if open => match line {
                    Some(Line::Msg(text)) => match parse_line(&text) {
                        Some(Msg::Progress { downloaded, total, speed }) => {
                            let mut p = run.progress.lock().unwrap_or_else(|e| e.into_inner());
                            // `NA` for the size keeps the last known one.
                            *p = Progress { downloaded, total: total.or(p.total), speed };
                        }
                        Some(Msg::File(path)) => announced = Some(path),
                        None => {}
                    },
                    Some(Line::Eof) | None => open = false,
                },
                _ = tick.tick() => {
                    let p = *run.progress.lock().unwrap_or_else(|e| e.into_inner());
                    if p.total != reported.1 {
                        if let Some(total) = p.total {
                            let _ = self.with_db(|c| downloads::set_size(c, id, total));
                        }
                    }
                    reported = (p.downloaded, p.total);
                    self.handle_engine_event(E::Progress { id, downloaded: p.downloaded, segments: Vec::new() }).await;
                },
                changed = stop.changed() => {
                    let wanted = *stop.borrow();
                    if changed.is_ok() && wanted != Stop::Run {
                        break End::Stopped(wanted);
                    }
                },
                status = child.wait() => break End::Exited(status),
            }
        };

        match &end {
            End::Stopped(_) => {
                proc::terminate(&mut child);
                if tokio::time::timeout(GRACE, child.wait()).await.is_err() {
                    proc::kill(&mut child);
                    let _ = tokio::time::timeout(GRACE, child.wait()).await;
                }
            }
            End::Exited(_) => proc::kill_leftovers(child.id()),
        }
        reader.abort();
        let stderr_text = tokio::time::timeout(GRACE, err_task)
            .await
            .ok()
            .and_then(std::result::Result::ok)
            .unwrap_or_default();
        let last = *run.progress.lock().unwrap_or_else(|e| e.into_inner());

        match end {
            End::Stopped(Stop::Pause) => {
                self.finish_run(id, &run).await;
                self.handle_engine_event(E::Paused {
                    id,
                    downloaded: last.downloaded,
                    segments: Vec::new(),
                })
                .await;
            }
            // Cancelled or shut down: the manager has dealt with the download itself.
            End::Stopped(_) => self.finish_run(id, &run).await,
            End::Exited(Err(e)) => {
                self.finish_run(id, &run).await;
                self.handle_engine_event(E::Failed {
                    id,
                    error: format!("cannot wait for the program: {e}"),
                })
                .await;
            }
            End::Exited(Ok(status)) if !status.success() => {
                self.finish_run(id, &run).await;
                let why = last_line(&stderr_text);
                let how = status.code().map_or_else(
                    || "was killed".to_owned(),
                    |c| format!("exited with code {c}"),
                );
                let error = if why.is_empty() {
                    format!("the program {how}")
                } else {
                    why
                };
                self.handle_engine_event(E::Failed { id, error }).await;
            }
            End::Exited(Ok(_)) => {
                let (staging, target, planned, planned_ext, overwrite) = (
                    plan.staging.clone(),
                    plan.target.clone(),
                    plan.planned.clone(),
                    plan.planned_ext.clone(),
                    plan.overwrite,
                );
                let placed = tokio::task::spawn_blocking(
                    move || -> std::result::Result<(PathBuf, u64), String> {
                        let file = find_made_file(&staging, announced.as_deref())?;
                        let actual = file.extension().and_then(|e| e.to_str()).map(str::to_owned);
                        let name = final_name(&planned, planned_ext.as_deref(), actual.as_deref());
                        move_into_place(&file, &staging, &target, &name, overwrite)
                            .map_err(|e| format!("cannot put the file in place: {e}"))
                    },
                )
                .await
                .unwrap_or_else(|e| Err(e.to_string()));
                self.finish_run(id, &run).await;
                match placed {
                    Ok((path, size)) => {
                        self.handle_engine_event(E::Completed { id, path, size })
                            .await
                    }
                    Err(error) => self.handle_engine_event(E::Failed { id, error }).await,
                }
            }
        }
    }

    /// Start the downloads that were waiting for a plugin to become ready (after a restart, or while
    /// it fetched its dependencies). Boxed for the same reason as `watch_download`.
    pub(super) fn wake_plugin_downloads(
        self: Arc<Self>,
        plugin: String,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
        Box::pin(async move {
            let waiting = self
                .with_db(|c| downloads::list_by_status(c, Status::Queued))
                .unwrap_or_default();
            let mut start = Vec::new();
            for d in waiting {
                let mine = self
                    .with_db(|c| downloads::runner(c, d.id))
                    .ok()
                    .flatten()
                    .is_some_and(|(r, _)| r.split('/').next() == Some(plugin.as_str()));
                if mine {
                    start.extend(self.admit(d.id));
                }
            }
            self.launch(start).await;
        })
    }

    /// The plugin system is off: the downloads that waited for a plugin cannot be made.
    pub(super) fn fail_plugin_downloads(&self, why: &str) {
        let waiting = self
            .with_db(|c| downloads::list_by_status(c, Status::Queued))
            .unwrap_or_default();
        for d in waiting {
            if d.plugin.is_some() {
                let _ = self.with_db(|c| {
                    downloads::set_status(c, d.id, Status::Failed, Some(why)).map(|_| ())
                });
                self.held().remove(&d.id);
                self.publish_download(d.id);
            }
        }
    }

    /// The program is gone: forget the run and tell whoever waits for it.
    async fn finish_run(&self, id: Id, run: &Run) {
        self.plugins
            .runs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
        run.finished.send_replace(true);
    }
}

/// How the program's run ended.
enum End {
    Stopped(Stop),
    Exited(std::io::Result<std::process::ExitStatus>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_and_file_lines_are_understood() {
        assert_eq!(
            parse_line("@fetchbox progress 1500 4000 1000.5"),
            Some(Msg::Progress {
                downloaded: 1500,
                total: Some(4000),
                speed: 1000
            })
        );
        assert_eq!(
            parse_line("  @fetchbox progress 12.9 NA None \n"),
            Some(Msg::Progress {
                downloaded: 12,
                total: None,
                speed: 0
            })
        );
        assert_eq!(
            parse_line("@fetchbox progress NA NA NA"),
            Some(Msg::Progress {
                downloaded: 0,
                total: None,
                speed: 0
            })
        );
        assert_eq!(
            parse_line("@fetchbox progress 5 0 0"),
            Some(Msg::Progress {
                downloaded: 5,
                total: None,
                speed: 0
            }),
            "a total of zero is no total"
        );
        assert_eq!(
            parse_line("@fetchbox file /data/x/My Video.mp4"),
            Some(Msg::File("/data/x/My Video.mp4".into())),
            "a path may have spaces"
        );
        for nothing in [
            "",
            "[download] 10%",
            "@fetchbox",
            "@fetchbox file ",
            "@fetchbox nonsense 1",
            "fetchbox progress 1 2 3",
        ] {
            assert_eq!(parse_line(nothing), None, "{nothing:?}");
        }
        assert_eq!(
            parse_line("@fetchbox progress -5 1 1"),
            Some(Msg::Progress {
                downloaded: 0,
                total: Some(1),
                speed: 1
            }),
            "a negative number is unknown"
        );
    }

    #[test]
    fn the_real_extension_replaces_the_one_the_preset_guessed() {
        assert_eq!(
            final_name("Title.m4a", Some("m4a"), Some("webm")),
            "Title.webm"
        );
        assert_eq!(
            final_name("Title.M4A", Some("m4a"), Some("m4a")),
            "Title.m4a"
        );
        assert_eq!(final_name("Title", None, Some("mp4")), "Title.mp4");
        assert_eq!(
            final_name("Title v1.2", None, Some("mp4")),
            "Title v1.2.mp4",
            "a dot in a title is not an extension"
        );
        assert_eq!(final_name("Title.mp4", None, Some("mp4")), "Title.mp4");
        assert_eq!(final_name("Title", None, None), "Title");
        assert_eq!(
            final_name("a/b:c", None, Some("mkv")),
            "b_c.mkv",
            "the name stays a plain file name"
        );
    }

    #[test]
    fn the_finished_file_is_the_announced_one_or_the_biggest_that_is_done() {
        let tmp = tempfile::tempdir().expect("tmp");
        let dir = tmp.path();
        assert!(find_made_file(dir, None).unwrap_err().contains("no file"));
        std::fs::write(dir.join("a.mp4.part"), "partial").expect("w");
        std::fs::write(dir.join(".hidden"), "x").expect("w");
        std::fs::write(dir.join("x.ytdl"), "x").expect("w");
        assert!(
            find_made_file(dir, None).unwrap_err().contains("no file"),
            "work files do not count"
        );
        std::fs::write(dir.join("small.txt"), "1").expect("w");
        std::fs::write(dir.join("big.mp4"), "12345678").expect("w");
        assert_eq!(
            find_made_file(dir, None)
                .expect("file")
                .file_name()
                .expect("n"),
            "big.mp4"
        );
        assert_eq!(
            find_made_file(dir, Some("small.txt"))
                .expect("file")
                .file_name()
                .expect("n"),
            "small.txt"
        );
        assert_eq!(
            find_made_file(dir, Some(dir.join("small.txt").to_str().expect("utf8")))
                .expect("file")
                .file_name()
                .expect("n"),
            "small.txt"
        );
        assert!(find_made_file(dir, Some("missing.mp4"))
            .unwrap_err()
            .contains("not there"));
        assert!(find_made_file(dir, Some("/etc/hostname"))
            .unwrap_err()
            .contains("outside"));
        assert!(find_made_file(dir, Some("../../etc/hostname")).is_err());
        #[cfg(unix)]
        {
            // A link to somewhere else is not a file the program made.
            std::fs::remove_file(dir.join("big.mp4")).expect("rm");
            std::os::unix::fs::symlink("/etc/hostname", dir.join("link.mp4")).expect("link");
            assert_eq!(
                find_made_file(dir, None)
                    .expect("file")
                    .file_name()
                    .expect("n"),
                "small.txt"
            );
            assert!(find_made_file(dir, Some("link.mp4"))
                .unwrap_err()
                .contains("outside"));
        }
    }

    #[test]
    fn the_file_is_put_in_place_without_taking_a_name_that_is_used() {
        let tmp = tempfile::tempdir().expect("tmp");
        let target = tmp.path().join("downloads");
        std::fs::create_dir_all(&target).expect("dir");
        let make = |n: &str, body: &str| {
            let staging = tmp.path().join(format!("job-{n}"));
            std::fs::create_dir_all(&staging).expect("dir");
            let f = staging.join("made.bin");
            std::fs::write(&f, body).expect("w");
            (f, staging)
        };
        let (f1, s1) = make("1", "one");
        let (p1, size) = move_into_place(&f1, &s1, &target, "Clip.mp4", false).expect("place");
        assert_eq!((p1.file_name().expect("n"), size), ("Clip.mp4".as_ref(), 3));
        assert!(!s1.exists(), "the working folder goes");
        let (f2, s2) = make("2", "two!");
        let (p2, _) = move_into_place(&f2, &s2, &target, "Clip.mp4", false).expect("place");
        assert_eq!(
            p2.file_name().expect("n"),
            "Clip (1).mp4",
            "an existing file is never replaced"
        );
        assert_eq!(std::fs::read_to_string(&p1).expect("r"), "one");
        let (f3, s3) = make("3", "three");
        let (p3, _) = move_into_place(&f3, &s3, &target, "Clip.mp4", true).expect("place");
        assert_eq!(p3.file_name().expect("n"), "Clip.mp4");
        assert_eq!(
            std::fs::read_to_string(&p3).expect("r"),
            "three",
            "overwrite is the user's choice"
        );
    }
}
