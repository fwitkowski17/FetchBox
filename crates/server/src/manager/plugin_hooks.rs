//! Event hooks of plugins (SPEC 6.15): what happens to downloads and torrents is passed on to the
//! plugins that asked for it, as a webhook FetchBox sends, a command it runs, or a message to the
//! plugin's program.
//!
//! One task reads the same event bus the browser's stream reads. It turns the changes into named
//! events (`download.completed`, `torrent.added`, ...), finds the hooks that listen, and puts the
//! work on a short queue per plugin, so a plugin that answers slowly (a webhook that hangs) only
//! delays its own deliveries. Delivery is at most once: a full queue drops, a failure is not
//! retried, and a failure shows on the plugin's card, never anywhere else.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fetchbox_core::plugin::{event_matches, Hook, HookAction, PluginState};
use fetchbox_core::{Download, Event, Id, Status, Torrent, TorrentStatus};
use fetchbox_plugins::oneshot::Spec;
use fetchbox_plugins::{expand, webhook};
use serde_json::{json, Value};
use tokio::sync::{broadcast, mpsc};

use super::Manager;

/// Deliveries that may wait for one plugin.
const QUEUE: usize = 256;
/// How often a hook that listens to progress gets it, when it does not say.
const DEFAULT_PROGRESS_EVERY: Duration = Duration::from_secs(5);
/// What the card says after a failed delivery (and what is cleared again by the next success).
const FAILED_PREFIX: &str = "The last event could not be delivered: ";

/// One delivery.
pub(super) struct Job {
    name: &'static str,
    action: HookAction,
    doc: Value,
}

/// The queues, by plugin.
pub(super) type Workers = Mutex<HashMap<String, mpsc::Sender<Job>>>;

/// What the hub remembers to tell what changed.
#[derive(Default)]
struct Seen {
    downloads: HashMap<Id, Download>,
    torrents: HashMap<Id, Torrent>,
    /// When a hook last got a progress event for a download or torrent.
    sampled: HashMap<(String, usize, String), Instant>,
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// The events a download's change of state means.
pub(super) fn download_events(prev: Option<&Download>, now: &Download) -> Vec<&'static str> {
    let Some(prev) = prev else {
        return vec!["download.added"];
    };
    if prev.status == now.status {
        return Vec::new();
    }
    match now.status {
        Status::Downloading => vec!["download.started"],
        Status::Paused => vec!["download.paused"],
        Status::Queued if matches!(prev.status, Status::Paused | Status::Failed) => {
            vec!["download.resumed"]
        }
        Status::Completed => vec!["download.completed"],
        Status::Failed => vec!["download.failed"],
        Status::Cancelled => vec!["download.cancelled"],
        Status::Queued | Status::Verifying => Vec::new(),
    }
}

/// The events a torrent's change of state means.
pub(super) fn torrent_events(prev: Option<&Torrent>, now: &Torrent) -> Vec<&'static str> {
    use TorrentStatus as S;
    let Some(prev) = prev else {
        return vec!["torrent.added"];
    };
    if prev.status == now.status {
        return Vec::new();
    }
    let working = |s: S| {
        matches!(
            s,
            S::Metadata | S::Checking | S::Connecting | S::Downloading
        )
    };
    match now.status {
        S::Seeding | S::Finished if working(prev.status) => vec!["torrent.completed"],
        S::Paused => vec!["torrent.paused"],
        S::Downloading | S::Connecting if matches!(prev.status, S::Paused | S::Error) => {
            vec!["torrent.started"]
        }
        _ => Vec::new(),
    }
}

impl Manager {
    /// Start the hub (once, when plugins are on).
    pub(super) fn start_hooks(self: &Arc<Self>) {
        let me = Arc::clone(self);
        tokio::spawn(async move { me.run_hooks().await });
    }

    async fn seed_seen(&self) -> Seen {
        let mut seen = Seen::default();
        if let Ok(all) = self.with_db(|c| fetchbox_storage::downloads::list(c)) {
            seen.downloads = all.into_iter().map(|d| (d.id, d)).collect();
        }
        if let Ok(all) = self.torrents_list().await {
            seen.torrents = all.into_iter().map(|t| (t.id, t)).collect();
        }
        seen
    }

    async fn run_hooks(self: Arc<Self>) {
        let mut rx: broadcast::Receiver<Event> = self.bus.subscribe();
        let mut closing = self.closing.subscribe();
        let mut seen = self.seed_seen().await;
        loop {
            tokio::select! {
                ev = rx.recv() => match ev {
                    Ok(ev) => self.hook_event(&mut seen, ev),
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        // Some events were missed: start from how things are now, so the next change
                        // is told apart correctly. The missed ones are not delivered.
                        tracing::warn!(missed = n, "plugin hooks fell behind and skipped events");
                        seen = self.seed_seen().await;
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                },
                _ = closing.changed() => break,
            }
        }
    }

    /// Turn one bus event into named events and deliver them.
    fn hook_event(&self, seen: &mut Seen, ev: Event) {
        match ev {
            Event::Download { download } => {
                let prev = seen.downloads.insert(download.id, (*download).clone());
                for name in download_events(prev.as_ref(), &download) {
                    self.dispatch(seen, name, json!({"download": download}), None);
                }
            }
            Event::Removed { id } => {
                let last = seen.downloads.remove(&id);
                self.dispatch(
                    seen,
                    "download.removed",
                    json!({"id": id, "download": last}),
                    None,
                );
            }
            Event::Progress {
                id,
                downloaded,
                total,
                speed,
            } => {
                let doc = json!({
                    "id": id,
                    "progress": {"downloaded": downloaded, "total": total, "speed": speed},
                    "download": seen.downloads.get(&id),
                });
                self.dispatch(seen, "download.progress", doc, Some(format!("d{id}")));
            }
            Event::Torrent { torrent } => {
                let prev = seen.torrents.insert(torrent.id, (*torrent).clone());
                for name in torrent_events(prev.as_ref(), &torrent) {
                    self.dispatch(seen, name, json!({"torrent": torrent}), None);
                }
                // The torrent's numbers come as events about once a second.
                self.dispatch(
                    seen,
                    "torrent.progress",
                    json!({"torrent": torrent}),
                    Some(format!("t{}", torrent.id)),
                );
            }
            Event::TorrentRemoved { id } => {
                let last = seen.torrents.remove(&id);
                self.dispatch(
                    seen,
                    "torrent.removed",
                    json!({"id": id, "torrent": last}),
                    None,
                );
            }
            Event::Settings { settings } => {
                self.dispatch(
                    seen,
                    "settings.changed",
                    json!({"settings": settings}),
                    None,
                );
            }
            Event::Plugin { .. } | Event::PluginRemoved { .. } | Event::PluginNotice { .. } => {}
        }
    }

    /// The hooks (of plugins that are usable) that listen to `name`.
    fn listeners(
        &self,
        name: &str,
    ) -> Vec<(
        String,
        usize,
        Hook,
        Option<Arc<fetchbox_plugins::process::Process>>,
    )> {
        let entries = self.plugin_entries();
        let mut out = Vec::new();
        for (id, e) in entries.iter() {
            if !matches!(e.state, PluginState::Ready | PluginState::Running) {
                continue;
            }
            let Some(m) = &e.manifest else { continue };
            for (i, h) in m.hooks.iter().enumerate() {
                if event_matches(&h.on, name) {
                    out.push((id.clone(), i, h.clone(), e.process.clone()));
                }
            }
        }
        out
    }

    fn dispatch(
        &self,
        seen: &mut Seen,
        name: &'static str,
        mut doc: Value,
        sample: Option<String>,
    ) {
        let listeners = self.listeners(name);
        if listeners.is_empty() {
            return;
        }
        if let Some(o) = doc.as_object_mut() {
            o.insert("event".into(), name.into());
            o.insert("at".into(), now_unix().into());
        }
        for (plugin, idx, hook, process) in listeners {
            if let Some(key) = &sample {
                let every = hook.every_s.map_or(DEFAULT_PROGRESS_EVERY, |s| {
                    Duration::from_secs(u64::from(s))
                });
                let k = (plugin.clone(), idx, key.clone());
                if seen.sampled.get(&k).is_some_and(|t| t.elapsed() < every) {
                    continue;
                }
                seen.sampled.insert(k, Instant::now());
            }
            match hook.action {
                HookAction::Process(_) => {
                    if let Some(p) = process {
                        p.notify("event", doc.clone());
                    }
                }
                action => self.enqueue(
                    &plugin,
                    Job {
                        name,
                        action,
                        doc: doc.clone(),
                    },
                ),
            }
        }
        // Finished downloads and torrents leave nothing behind in the sampling table.
        if seen.sampled.len() > 4096 {
            seen.sampled
                .retain(|_, t| t.elapsed() < Duration::from_secs(600));
        }
    }

    fn enqueue(&self, plugin: &str, job: Job) {
        let tx = {
            let mut workers = self
                .plugins
                .hook_workers
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            match workers.get(plugin).filter(|t| !t.is_closed()) {
                Some(t) => t.clone(),
                None => {
                    let Some(me) = self.this.get().and_then(std::sync::Weak::upgrade) else {
                        return;
                    };
                    let (tx, rx) = mpsc::channel(QUEUE);
                    workers.insert(plugin.to_owned(), tx.clone());
                    tokio::spawn(me.hook_worker(plugin.to_owned(), rx));
                    tx
                }
            }
        };
        if tx.try_send(job).is_err() {
            tracing::warn!(
                plugin,
                "a plugin is too slow to take its events: one was dropped"
            );
        }
    }

    async fn hook_worker(self: Arc<Self>, plugin: String, mut rx: mpsc::Receiver<Job>) {
        while let Some(job) = rx.recv().await {
            let usable = self
                .plugin_entries()
                .get(&plugin)
                .is_some_and(|e| matches!(e.state, PluginState::Ready | PluginState::Running));
            if !usable {
                continue;
            }
            let name = job.name;
            match self.deliver(&plugin, job).await {
                Ok(()) => self.hook_result(&plugin, None),
                Err(e) => {
                    tracing::warn!(target: "activity", plugin = %plugin, event = name, error = %e, "plugin hook failed");
                    self.hook_result(&plugin, Some(e.to_string()));
                }
            }
        }
    }

    async fn deliver(&self, plugin: &str, job: Job) -> fetchbox_core::Result<()> {
        let host = self.plugin_host()?;
        let env = self.run_env(plugin)?;
        match job.action {
            HookAction::Webhook(hook) => {
                webhook::send(&host.fetcher, &hook, &env.context(), &job.doc)
                    .await
                    .map_err(fetchbox_core::Error::from)?;
            }
            HookAction::Run(cmd) => {
                let ctx = env.context();
                let program = expand::resolve_program(&cmd.command, &ctx)
                    .map_err(fetchbox_core::Error::from)?;
                let args =
                    expand::expand_args(&cmd.args, &ctx).map_err(fetchbox_core::Error::from)?;
                let mut spec = Spec::new(
                    program,
                    args,
                    env.child_env(Default::default()),
                    Duration::from_secs(u64::from(cmd.timeout_s.clamp(1, 120))),
                );
                spec.stdin = Some(job.doc.to_string().into_bytes());
                host.runner
                    .run(plugin, spec)
                    .await
                    .map_err(fetchbox_core::Error::from)?;
            }
            HookAction::Process(_) => {}
        }
        Ok(())
    }

    /// Show the outcome of a delivery on the plugin's card: a failure as a line of status, cleared
    /// by the next success.
    fn hook_result(&self, plugin: &str, failure: Option<String>) {
        let changed = {
            let mut entries = self.plugin_entries();
            let Some(e) = entries.get_mut(plugin) else {
                return;
            };
            match failure {
                Some(why) => {
                    let line = format!("{FAILED_PREFIX}{why}");
                    let line: String = line.chars().take(300).collect();
                    let changed = e.status_text.as_deref() != Some(line.as_str());
                    e.status_text = Some(line);
                    changed
                }
                None if e
                    .status_text
                    .as_deref()
                    .is_some_and(|t| t.starts_with(FAILED_PREFIX)) =>
                {
                    e.status_text = None;
                    true
                }
                None => false,
            }
        };
        if changed {
            self.publish_plugin(plugin);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fetchbox_core::{Category, SourceKind};

    fn download(status: Status) -> Download {
        Download {
            id: 1,
            url: "https://x/f".into(),
            filename: "f".into(),
            category: Category::Other,
            status,
            size: None,
            downloaded: 0,
            source_kind: SourceKind::Http,
            resumable: true,
            etag: None,
            error: None,
            username: None,
            subdir: String::new(),
            disk: String::new(),
            speed: 0,
            waiting_space: false,
            archived: false,
            speed_limit: 0,
            start_at: None,
            hold: None,
            starts_at: None,
            checksum: None,
            verified: false,
            verify_failed: false,
            plugin: None,
        }
    }

    #[test]
    fn a_download_tells_what_happened_to_it() {
        use Status::*;
        let ev = |from: Option<Status>, to: Status| {
            download_events(from.map(download).as_ref(), &download(to))
        };
        assert_eq!(ev(None, Queued), ["download.added"]);
        assert_eq!(
            ev(None, Paused),
            ["download.added"],
            "also one added already held back"
        );
        assert_eq!(ev(Some(Queued), Downloading), ["download.started"]);
        assert_eq!(ev(Some(Downloading), Paused), ["download.paused"]);
        assert_eq!(ev(Some(Paused), Queued), ["download.resumed"]);
        assert_eq!(ev(Some(Failed), Queued), ["download.resumed"]);
        assert_eq!(
            ev(Some(Downloading), Queued),
            Vec::<&str>::new(),
            "put back by the hours"
        );
        assert_eq!(ev(Some(Downloading), Verifying), Vec::<&str>::new());
        assert_eq!(ev(Some(Verifying), Completed), ["download.completed"]);
        assert_eq!(ev(Some(Downloading), Completed), ["download.completed"]);
        assert_eq!(ev(Some(Downloading), Failed), ["download.failed"]);
        assert_eq!(ev(Some(Queued), Cancelled), ["download.cancelled"]);
        assert_eq!(
            ev(Some(Downloading), Downloading),
            Vec::<&str>::new(),
            "a change of numbers is not news"
        );
    }
}
