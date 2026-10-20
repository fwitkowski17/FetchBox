//! Plugins in the manager (SPEC 6.15).
//!
//! The files of a plugin (`<data>/plugins/<id>/`) say what it is; the database says what the user
//! decided (switched on, which manifest they approved) and keeps its settings. This module joins
//! the two into the list the interface shows, runs the plugin's lifecycle (fetch its dependencies,
//! start its program) and runs its actions.
//!
//! A plugin can never hold up downloads: nothing here holds the database or the scheduler across a
//! call into a plugin, every call has a deadline, and a plugin that fails only turns its own card
//! red.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use fetchbox_core::plugin::{
    self, derive_permissions, platform_key, ActionKind, ActionResult, ActionView, Builtin,
    DependencyProgress, DependencyView, FieldKind, FieldView, Manifest, NoticeLevel, PluginList,
    PluginSource, PluginState, PluginView, Provides, SettingValue, StagedPlugin, StagedReplaces,
    UiView,
};
use fetchbox_core::version::is_newer;
use fetchbox_core::{Error, Event, Result};
use fetchbox_http_engine::UrlPolicy;
use fetchbox_plugins::deps::Installer;
use fetchbox_plugins::expand::{self, Context, DepPaths};
use fetchbox_plugins::fetch::Fetcher;
use fetchbox_plugins::install::{self, Layout, Staged};
use fetchbox_plugins::oneshot::{Runner, Spec};
use fetchbox_plugins::proc::ChildEnv;
use fetchbox_plugins::process::{self, Process};
use fetchbox_plugins::store::Catalog;
use fetchbox_plugins::webhook;
use fetchbox_storage::plugins::{self as store, NewPlugin, StoredPlugin};
use tokio::task::AbortHandle;

use super::Manager;

/// How the plugin system is configured at start-up.
#[derive(Debug, Clone)]
pub struct PluginSetup {
    /// The data directory: plugins live in `plugins/` and write to `plugin-data/`.
    pub data_dir: std::path::PathBuf,
    pub mode: PluginsMode,
    /// The destinations plugins may fetch from (the same policy as downloads).
    pub policy: UrlPolicy,
    /// Accept `http://` for dependencies. Only for tests.
    pub allow_plain_http: bool,
    /// Another server for the GitHub releases API. Only for tests.
    pub github_api: Option<String>,
    /// Where the catalog of third-party plugins is: `None` is the project's own, `Some("")` none at
    /// all (only the plugins that ship with FetchBox are offered).
    pub index_url: Option<String>,
}

/// Whether plugins may run at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginsMode {
    On,
    /// Switched off with `--plugins off`.
    Off,
    /// The server runs without a password: a plugin runs programs, so that must not be open to
    /// whoever can reach the port.
    NoAuth,
}

impl PluginsMode {
    fn reason(self) -> Option<&'static str> {
        match self {
            PluginsMode::On => None,
            PluginsMode::Off => Some("off"),
            PluginsMode::NoAuth => Some("no_auth"),
        }
    }
}

/// How long a staged package waits for the user's decision.
const STAGED_FOR: Duration = Duration::from_secs(15 * 60);
/// How long a request to the plugin's program may take.
const INVOKE_TIMEOUT: Duration = Duration::from_secs(60);

pub(super) struct Host {
    mode: PluginsMode,
    pub(super) layout: Layout,
    pub(super) fetcher: Fetcher,
    pub(super) installer: Arc<Installer>,
    pub(super) runner: Runner,
    pub(super) catalog: Catalog,
}

/// A plugin as the manager holds it.
pub(super) struct Entry {
    /// `None` when `plugin.json` cannot be used (`broken` says why).
    pub(super) manifest: Option<Manifest>,
    broken: Option<String>,
    /// SHA-256 of the manifest on disk.
    hash: String,
    /// What the database says; `None` for a folder nobody has accepted yet.
    stored: Option<StoredPlugin>,
    pub(super) state: PluginState,
    error: Option<String>,
    status_text: Option<String>,
    progress: Option<DependencyProgress>,
    pub(super) process: Option<Arc<Process>>,
    prepare: Option<AbortHandle>,
    /// Bumped whenever the plugin is (re)prepared, so a task that was overtaken cannot touch it.
    generation: u64,
}

impl Entry {
    fn enabled(&self) -> bool {
        self.stored.as_ref().is_some_and(|s| s.enabled)
    }

    fn approved(&self) -> bool {
        self.stored
            .as_ref()
            .is_some_and(|s| !s.approved_hash.is_empty() && s.approved_hash == self.hash)
    }
}

#[derive(Default)]
pub(crate) struct Plugins {
    pub(super) host: OnceLock<Host>,
    entries: Mutex<BTreeMap<String, Entry>>,
    /// Serialises installs, removals and switching on and off.
    lifecycle: tokio::sync::Mutex<()>,
    /// Where each staged package came from (token to source and store checksum).
    staged: Mutex<HashMap<String, (PluginSource, Option<String>)>>,
    /// Downloads whose program is running (see `plugin_runner`).
    pub(super) runs: super::plugin_runner::Runs,
}

/// Which dependencies a preparation fetches.
#[derive(Clone)]
enum Fetch {
    /// The required ones that are not in place.
    Missing,
    /// Everything that is installed, and the required ones: a new version of each.
    Update,
    /// One dependency (also an optional one), installed or updated.
    One(String),
}

/// What a command needs to be filled in and run.
pub(super) struct RunEnv {
    id: String,
    plugin_dir: std::path::PathBuf,
    data_dir: std::path::PathBuf,
    tmp: std::path::PathBuf,
    settings: BTreeMap<String, SettingValue>,
    deps: BTreeMap<String, DepPaths>,
    path_dirs: Vec<std::path::PathBuf>,
}

impl RunEnv {
    pub(super) fn context(&self) -> Context<'_> {
        Context {
            plugin_dir: &self.plugin_dir,
            data_dir: &self.data_dir,
            tmp: &self.tmp,
            settings: &self.settings,
            deps: &self.deps,
            url: None,
            choice_args: &[],
            out_dir: None,
        }
    }

    pub(super) fn child_env(&self, extra: BTreeMap<String, String>) -> ChildEnv {
        ChildEnv {
            plugin_id: self.id.clone(),
            plugin_dir: self.plugin_dir.clone(),
            data_dir: self.data_dir.clone(),
            path_dirs: self.path_dirs.clone(),
            extra,
        }
    }
}

fn join_err(e: &tokio::task::JoinError) -> Error {
    Error::Internal(e.to_string())
}

fn stored_text(v: &SettingValue) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "\"\"".into())
}

/// The settings a plugin runs with: the manifest's defaults, then what the user saved.
pub(super) fn effective(
    m: &Manifest,
    stored: &BTreeMap<String, String>,
) -> BTreeMap<String, SettingValue> {
    let mut out = BTreeMap::new();
    for f in &m.settings {
        if let Some(d) = &f.default {
            out.insert(f.key.clone(), d.clone());
        }
        if let Some(v) = stored
            .get(&f.key)
            .and_then(|t| serde_json::from_str::<SettingValue>(t).ok())
        {
            out.insert(f.key.clone(), v);
        }
    }
    out
}

/// Why a plugin cannot run here, if it cannot.
fn unsupported(m: &Manifest) -> Option<String> {
    if !m.platforms.is_empty() && !m.platforms.contains(&platform_key()) {
        return Some(format!("it does not support {}", platform_key()));
    }
    if let Some(min) = &m.min_fetchbox {
        if is_newer(min, env!("CARGO_PKG_VERSION")) {
            return Some(format!("it needs FetchBox {min} or newer"));
        }
    }
    None
}

impl Manager {
    // ---- set-up ----------------------------------------------------------------------

    /// Switch the plugin system on (or record why it is off) and load the installed plugins. The
    /// enabled ones are prepared in the background: the server does not wait for any dependency.
    pub async fn enable_plugins(self: &Arc<Self>, setup: PluginSetup) {
        let mode = setup.mode;
        if mode == PluginsMode::NoAuth {
            tracing::warn!(
                "plugins are off: the server runs without authentication, and a plugin runs programs"
            );
        }
        let layout = Layout::new(&setup.data_dir);
        let fetcher = match Fetcher::new(setup.policy) {
            Ok(f) => {
                let f = if setup.allow_plain_http {
                    f.allow_plain_http()
                } else {
                    f
                };
                match &setup.github_api {
                    Some(api) => f.with_github_api(api),
                    None => f,
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "plugins are off: cannot build an HTTP client");
                return;
            }
        };
        let installer = Arc::new(Installer::new(fetcher.clone(), layout.clone()));
        let index = match setup.index_url {
            Some(url) if url.trim().is_empty() => None,
            Some(url) => Some(url),
            None => Some(super::plugin_store::DEFAULT_INDEX_URL.to_owned()),
        };
        let host = Host {
            mode,
            layout,
            catalog: Catalog::new(fetcher.clone(), index),
            fetcher,
            installer,
            runner: Runner::default(),
        };
        if self.plugins.host.set(host).is_err() {
            return;
        }
        if mode != PluginsMode::On {
            self.fail_plugin_downloads("plugins are switched off on this server");
            return;
        }
        match self.load_plugins().await {
            Ok(count) => {
                let enabled = self
                    .plugin_entries()
                    .values()
                    .filter(|e| e.enabled())
                    .count();
                tracing::info!(plugins = count, enabled, "plugins loaded");
            }
            Err(e) => tracing::warn!(error = %e, "cannot load the plugins"),
        }
        let to_prepare: Vec<String> = self
            .plugin_entries()
            .iter()
            .filter(|(_, e)| {
                e.manifest.is_some()
                    && e.enabled()
                    && e.approved()
                    && e.state == PluginState::Preparing
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in to_prepare {
            self.spawn_prepare(&id, &Fetch::Missing);
        }
    }

    /// Stop every plugin's program (the server is going down).
    pub async fn stop_plugins(&self) {
        self.stop_runners().await;
        let procs: Vec<Arc<Process>> = self
            .plugin_entries()
            .values_mut()
            .filter_map(|e| {
                if let Some(h) = e.prepare.take() {
                    h.abort();
                }
                e.process.take()
            })
            .collect();
        let tasks: Vec<_> = procs
            .into_iter()
            .map(|p| tokio::spawn(async move { p.stop().await }))
            .collect();
        for t in tasks {
            let _ = t.await;
        }
    }

    // ---- small helpers ---------------------------------------------------------------

    pub(super) fn plugin_entries(&self) -> MutexGuard<'_, BTreeMap<String, Entry>> {
        self.plugins
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    fn staged_meta(&self) -> MutexGuard<'_, HashMap<String, (PluginSource, Option<String>)>> {
        self.plugins
            .staged
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// The host, if plugins may be used.
    pub(super) fn plugin_host(&self) -> Result<&Host> {
        let Some(host) = self.plugins.host.get() else {
            return Err(Error::InvalidState("plugins are not switched on".into()));
        };
        match host.mode {
            PluginsMode::On => Ok(host),
            PluginsMode::Off => Err(Error::InvalidState(
                "plugins are switched off on this server (--plugins off)".into(),
            )),
            PluginsMode::NoAuth => Err(Error::InvalidState(
                "plugins are not available while the server runs without a password".into(),
            )),
        }
    }

    pub(super) fn plugin_stored_settings(&self, id: &str) -> BTreeMap<String, String> {
        self.with_db(|c| store::settings(c, id)).unwrap_or_default()
    }

    /// The paths and values a command of this plugin is run with.
    pub(super) fn run_env(&self, id: &str) -> Result<RunEnv> {
        let host = self.plugin_host()?;
        let manifest = self
            .plugin_entries()
            .get(id)
            .and_then(|e| e.manifest.clone())
            .ok_or_else(|| Error::Missing(format!("no plugin \"{id}\"")))?;
        let settings = effective(&manifest, &self.plugin_stored_settings(id));
        let mut deps = BTreeMap::new();
        let mut path_dirs = vec![host.layout.plugin_dir(id).join("bin")];
        for d in &manifest.dependencies {
            if host.installer.state(id, d).is_some() {
                let dir = host.installer.dep_dir(id, &d.id);
                path_dirs.push(dir.clone());
                deps.insert(
                    d.id.clone(),
                    DepPaths {
                        bin: host.installer.bin_path(id, d),
                        dir,
                    },
                );
            }
        }
        let data_dir = host.layout.data_dir_of(id);
        Ok(RunEnv {
            id: id.to_owned(),
            plugin_dir: host.layout.plugin_dir(id),
            tmp: data_dir.join("tmp"),
            data_dir,
            settings,
            deps,
            path_dirs,
        })
    }

    // ---- loading ---------------------------------------------------------------------

    /// What state a plugin that is not being prepared or run is in.
    fn resting_state(e: &Entry) -> (PluginState, Option<String>) {
        if let Some(why) = &e.broken {
            return (PluginState::Failed, Some(why.clone()));
        }
        let Some(m) = &e.manifest else {
            return (PluginState::Failed, Some("plugin.json is missing".into()));
        };
        if let Some(why) = unsupported(m) {
            return (PluginState::Unsupported, Some(why));
        }
        if !e.approved() {
            return (
                PluginState::NeedsReview,
                Some(
                    "the files differ from what was approved, or have not been approved yet".into(),
                ),
            );
        }
        if !e.enabled() {
            return (PluginState::Disabled, None);
        }
        (PluginState::Preparing, None)
    }

    fn new_entry(
        found: &install::Discovered,
        stored: Option<StoredPlugin>,
        generation: u64,
    ) -> Entry {
        let mut e = match &found.result {
            Ok((m, hash)) => Entry {
                manifest: Some(m.clone()),
                broken: None,
                hash: hash.clone(),
                stored,
                state: PluginState::Disabled,
                error: None,
                status_text: None,
                progress: None,
                process: None,
                prepare: None,
                generation,
            },
            Err(why) => Entry {
                manifest: None,
                broken: Some(why.clone()),
                hash: String::new(),
                stored,
                state: PluginState::Failed,
                error: None,
                status_text: None,
                progress: None,
                process: None,
                prepare: None,
                generation,
            },
        };
        let (state, error) = Self::resting_state(&e);
        e.state = state;
        e.error = error;
        e
    }

    /// Read the plugins directory and the database and make the list agree with them. A plugin whose
    /// files did not change is left alone (and keeps running); a new folder becomes a plugin that
    /// needs review; a plugin whose files are gone is forgotten. Returns how many plugins there are.
    async fn load_plugins(&self) -> Result<usize> {
        let host = self.plugin_host()?;
        let layout = host.layout.clone();
        let found = tokio::task::spawn_blocking(move || {
            install::sweep_staging(&layout, STAGED_FOR);
            install::discover(&layout)
        })
        .await
        .map_err(|e| join_err(&e))?;
        let rows: HashMap<String, StoredPlugin> = self
            .with_db(|c| store::list(c))?
            .into_iter()
            .map(|r| (r.id.clone(), r))
            .collect();

        let mut gone: Vec<Arc<Process>> = Vec::new();
        let mut changed: Vec<String> = Vec::new();
        {
            let mut entries = self.plugin_entries();
            let on_disk: std::collections::HashSet<&str> =
                found.iter().map(|d| d.folder.as_str()).collect();
            // Plugins whose files have disappeared.
            let missing: Vec<String> = entries
                .keys()
                .filter(|id| !on_disk.contains(id.as_str()))
                .cloned()
                .collect();
            for id in missing {
                if let Some(mut e) = entries.remove(&id) {
                    if let Some(h) = e.prepare.take() {
                        h.abort();
                    }
                    gone.extend(e.process.take());
                    changed.push(id.clone());
                    tracing::info!(target: "activity", plugin = %id, "plugin files are gone, forgotten");
                    let _ = self.with_db(|c| store::delete(c, &id));
                }
            }
            for d in &found {
                let row = rows.get(&d.folder).cloned();
                let same = entries.get(&d.folder).is_some_and(|e| match &d.result {
                    Ok((_, hash)) => e.manifest.is_some() && e.hash == *hash,
                    Err(why) => e.broken.as_deref() == Some(why.as_str()),
                });
                if same {
                    continue;
                }
                let generation = entries.get(&d.folder).map_or(0, |e| e.generation + 1);
                let mut row = row;
                if row.is_none() {
                    if let Ok((m, _)) = &d.result {
                        // A folder somebody put there: remembered, not approved, not enabled.
                        row = self
                            .with_db(|c| {
                                store::upsert(
                                    c,
                                    &NewPlugin {
                                        id: &d.folder,
                                        version: &m.version,
                                        source: PluginSource::Folder,
                                        approved_hash: "",
                                        store_sha256: None,
                                    },
                                )
                            })
                            .ok();
                    }
                }
                if let Some(mut old) = entries.remove(&d.folder) {
                    if let Some(h) = old.prepare.take() {
                        h.abort();
                    }
                    gone.extend(old.process.take());
                }
                entries.insert(d.folder.clone(), Self::new_entry(d, row, generation));
                changed.push(d.folder.clone());
            }
        }
        for p in gone {
            p.stop().await;
        }
        for id in &changed {
            if self.plugin_entries().contains_key(id) {
                self.publish_plugin(id);
            } else {
                self.publish(Event::PluginRemoved { id: id.clone() });
            }
        }
        Ok(self.plugin_entries().len())
    }

    // ---- views -----------------------------------------------------------------------

    /// The plugin as the interface shows it.
    fn plugin_view(&self, id: &str) -> Option<PluginView> {
        let host = self.plugins.host.get()?;
        let (manifest, state, error, status_text, progress, hash, stored) = {
            let entries = self.plugin_entries();
            let e = entries.get(id)?;
            (
                e.manifest.clone(),
                e.state,
                e.error.clone(),
                e.status_text.clone(),
                e.progress.clone(),
                e.hash.clone(),
                e.stored.clone(),
            )
        };
        let values = self.plugin_stored_settings(id);
        let Some(m) = manifest else {
            return Some(PluginView {
                id: id.to_owned(),
                name: id.to_owned(),
                version: stored
                    .as_ref()
                    .map(|s| s.version.clone())
                    .unwrap_or_default(),
                manifest_hash: String::new(),
                description: String::new(),
                author: String::new(),
                homepage: None,
                license: None,
                source: stored.as_ref().map_or(PluginSource::Folder, |s| s.source),
                enabled: false,
                state,
                error,
                status_text: None,
                provides: Provides::default(),
                permissions: plugin::PermissionSummary::default(),
                settings: Vec::new(),
                ui: UiView::default(),
                dependencies: Vec::new(),
                progress: None,
                update_available: None,
                i18n: BTreeMap::new(),
            });
        };
        let settings = m
            .settings
            .iter()
            .filter(|f| f.kind != FieldKind::Info)
            .map(|f| {
                let stored_value = values
                    .get(&f.key)
                    .and_then(|t| serde_json::from_str::<SettingValue>(t).ok());
                let has_value = stored_value.is_some();
                let value = if f.kind == FieldKind::Secret {
                    None
                } else {
                    stored_value.or_else(|| f.default.clone())
                };
                FieldView {
                    field: f.clone(),
                    value,
                    has_value,
                }
            })
            .collect();
        let dependencies = m
            .dependencies
            .iter()
            .map(|d| {
                let info = host.installer.state(id, d);
                DependencyView {
                    id: d.id.clone(),
                    label: if d.label.is_empty() {
                        d.id.clone()
                    } else {
                        d.label.clone()
                    },
                    optional: d.optional,
                    installed: info.is_some(),
                    version: info.and_then(|i| i.version),
                }
            })
            .collect();
        let ui = UiView {
            icon: m.ui.icon.clone(),
            tab: m.ui.tab.clone(),
            actions: m.ui.actions.iter().map(ActionView::of).collect(),
            dialogs: m.ui.dialogs.clone(),
        };
        Some(PluginView {
            id: id.to_owned(),
            name: m.name.clone(),
            version: m.version.clone(),
            manifest_hash: hash,
            description: m.description.clone(),
            author: m.author.clone(),
            homepage: m.homepage.clone(),
            license: m.license.clone(),
            source: stored.as_ref().map_or(PluginSource::Folder, |s| s.source),
            enabled: stored.as_ref().is_some_and(|s| s.enabled),
            state,
            error,
            status_text,
            provides: Provides {
                resolvers: m.resolvers.iter().map(|r| r.id.clone()).collect(),
                hooks: m.hooks.len(),
                process: m.process.is_some(),
            },
            permissions: derive_permissions(&m),
            settings,
            ui,
            dependencies,
            progress,
            update_available: self.store_update_for(id, &m.version),
            i18n: m.i18n.clone(),
        })
    }

    fn publish_plugin(&self, id: &str) {
        if let Some(v) = self.plugin_view(id) {
            self.publish(Event::Plugin {
                plugin: Box::new(v),
            });
        }
    }

    // ---- preparing and running -------------------------------------------------------

    /// Fetch what the plugin needs and start its program, in the background.
    fn spawn_prepare(&self, id: &str, which: &Fetch) {
        let Some(me) = self.this.get().and_then(std::sync::Weak::upgrade) else {
            return;
        };
        let generation = {
            let mut entries = self.plugin_entries();
            let Some(e) = entries.get_mut(id) else { return };
            if let Some(h) = e.prepare.take() {
                h.abort();
            }
            e.generation += 1;
            e.state = PluginState::Preparing;
            e.error = None;
            e.progress = None;
            e.generation
        };
        self.publish_plugin(id);
        let (id2, which) = (id.to_owned(), which.clone());
        let handle =
            tokio::spawn(async move { me.prepare(&id2, generation, &which).await }).abort_handle();
        if let Some(e) = self.plugin_entries().get_mut(id) {
            if e.generation == generation {
                e.prepare = Some(handle);
            }
        }
    }

    /// Is this still the preparation the plugin is waiting for?
    fn current(&self, id: &str, generation: u64) -> bool {
        self.plugin_entries()
            .get(id)
            .is_some_and(|e| e.generation == generation)
    }

    fn fail_plugin(&self, id: &str, generation: u64, why: String) {
        {
            let mut entries = self.plugin_entries();
            let Some(e) = entries.get_mut(id) else { return };
            if e.generation != generation {
                return;
            }
            e.state = PluginState::Failed;
            e.error = Some(why.clone());
            e.progress = None;
        }
        tracing::warn!(target: "activity", plugin = %id, reason = %why, "plugin failed to start");
        self.publish_plugin(id);
    }

    async fn prepare(self: &Arc<Self>, id: &str, generation: u64, which: &Fetch) {
        let Ok(host) = self.plugin_host() else { return };
        let Some(manifest) = self
            .plugin_entries()
            .get(id)
            .and_then(|e| e.manifest.clone())
        else {
            return;
        };
        for dep in &manifest.dependencies {
            let installed = host.installer.state(id, dep).is_some();
            let wanted = match which {
                Fetch::Missing => !dep.optional && !installed,
                Fetch::Update => !dep.optional || installed,
                Fetch::One(only) => *only == dep.id,
            };
            if !wanted {
                continue;
            }
            let weak = Arc::downgrade(self);
            let id_for_progress = id.to_owned();
            let report = move |p: fetchbox_plugins::deps::Progress| {
                let Some(me) = weak.upgrade() else { return };
                {
                    let mut entries = me.plugin_entries();
                    let Some(e) = entries.get_mut(&id_for_progress) else {
                        return;
                    };
                    if e.generation != generation {
                        return;
                    }
                    e.progress = Some(DependencyProgress {
                        dependency: p.dependency,
                        downloaded: p.downloaded,
                        total: p.total,
                    });
                }
                me.publish_plugin(&id_for_progress);
            };
            if let Err(e) = host.installer.install(id, dep, &report).await {
                let label = if dep.label.is_empty() {
                    &dep.id
                } else {
                    &dep.label
                };
                self.fail_plugin(id, generation, format!("{label}: {e}"));
                return;
            }
        }
        if !self.current(id, generation) {
            return;
        }
        if let Err(e) = self.start_runtime(id, generation, &manifest) {
            self.fail_plugin(id, generation, e.to_string());
        }
    }

    /// Everything is in place: start the plugin's program, or mark it ready.
    fn start_runtime(self: &Arc<Self>, id: &str, generation: u64, m: &Manifest) -> Result<()> {
        let Some(spec) = &m.process else {
            if let Some(e) = self.plugin_entries().get_mut(id) {
                if e.generation == generation {
                    e.state = PluginState::Ready;
                    e.progress = None;
                }
            }
            self.publish_plugin(id);
            tracing::info!(target: "activity", plugin = %id, "plugin ready");
            self.wake_downloads_of(id);
            return Ok(());
        };
        let env = self.run_env(id)?;
        let ctx = env.context();
        let program = expand::resolve_program(&spec.command, &ctx).map_err(Error::from)?;
        if !program.is_file() {
            return Err(Error::InvalidState(format!(
                "its program \"{}\" is not there",
                program
                    .strip_prefix(&env.plugin_dir)
                    .unwrap_or(&program)
                    .display()
            )));
        }
        let args = expand::expand_args(&spec.args, &ctx).map_err(Error::from)?;
        let mut extra = BTreeMap::new();
        for (k, v) in &spec.env {
            extra.insert(
                k.clone(),
                expand::expand_string(v, &ctx).map_err(Error::from)?,
            );
        }
        let settings: serde_json::Map<String, serde_json::Value> = env
            .settings
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::to_value(v).unwrap_or_default()))
            .collect();
        let cfg = process::Config {
            plugin_id: id.to_owned(),
            program,
            args,
            env: env.child_env(extra),
            restart: spec.restart,
            initialize: serde_json::json!({
                "api_version": plugin::API_VERSION,
                "plugin_id": id,
                "fetchbox_version": env!("CARGO_PKG_VERSION"),
                "data_dir": env.data_dir,
                "settings": settings,
            }),
            timing: process::Timing::default(),
        };
        let weak = Arc::downgrade(self);
        let id2 = id.to_owned();
        let sink = Arc::new(move |ev: process::Event| {
            if let Some(me) = weak.upgrade() {
                me.on_process_event(&id2, generation, ev);
            }
        });
        if !spec.autostart {
            if let Some(e) = self.plugin_entries().get_mut(id) {
                if e.generation == generation {
                    e.state = PluginState::Ready;
                }
            }
            self.publish_plugin(id);
            return Ok(());
        }
        let proc = Arc::new(Process::spawn(cfg, sink));
        let mut entries = self.plugin_entries();
        let Some(e) = entries.get_mut(id) else {
            return Ok(());
        };
        if e.generation != generation {
            drop(entries);
            let p = proc.clone();
            tokio::spawn(async move { p.stop().await });
            return Ok(());
        }
        e.process = Some(proc);
        e.state = PluginState::Starting;
        e.progress = None;
        drop(entries);
        self.publish_plugin(id);
        Ok(())
    }

    fn on_process_event(&self, id: &str, generation: u64, ev: process::Event) {
        match ev {
            process::Event::State(s) => {
                let mut woke = false;
                {
                    let mut entries = self.plugin_entries();
                    let Some(e) = entries.get_mut(id) else { return };
                    if e.generation != generation {
                        return;
                    }
                    match s {
                        process::State::Starting | process::State::Backoff { .. } => {
                            e.state = PluginState::Starting;
                        }
                        process::State::Running => {
                            e.state = PluginState::Running;
                            e.error = None;
                            woke = true;
                        }
                        process::State::Failed(why) => {
                            e.state = PluginState::Failed;
                            e.error = Some(why);
                        }
                        process::State::Stopped => {
                            e.state = PluginState::Failed;
                            e.error = Some("its program has stopped".into());
                        }
                    }
                }
                self.publish_plugin(id);
                if woke {
                    self.wake_downloads_of(id);
                }
            }
            process::Event::Status(text) => {
                if let Some(e) = self.plugin_entries().get_mut(id) {
                    if e.generation == generation {
                        e.status_text = text;
                    }
                }
                self.publish_plugin(id);
            }
            process::Event::Notify { level, message } => {
                self.publish(Event::PluginNotice {
                    plugin_id: id.to_owned(),
                    level,
                    message,
                });
            }
        }
    }

    /// Downloads that waited for this plugin may start now.
    fn wake_downloads_of(&self, id: &str) {
        if let Some(me) = self.this.get().and_then(std::sync::Weak::upgrade) {
            tokio::spawn(me.wake_plugin_downloads(id.to_owned()));
        }
    }

    /// End a plugin's preparation and program (it is being switched off, replaced or removed).
    async fn stop_runtime(&self, id: &str) {
        // What the plugin is downloading stops with it; those downloads wait, paused.
        self.pause_runs_of(id);
        let proc = {
            let mut entries = self.plugin_entries();
            let Some(e) = entries.get_mut(id) else { return };
            if let Some(h) = e.prepare.take() {
                h.abort();
            }
            e.generation += 1;
            e.progress = None;
            e.status_text = None;
            e.process.take()
        };
        if let Some(p) = proc {
            p.stop().await;
        }
    }

    // ---- the operations of the Service ----------------------------------------------

    pub(super) async fn plugins_list(&self) -> Result<PluginList> {
        let (available, reason) = match self.plugins.host.get() {
            None => (false, Some("off".to_owned())),
            Some(h) => (
                h.mode == PluginsMode::On,
                h.mode.reason().map(str::to_owned),
            ),
        };
        let ids: Vec<String> = self.plugin_entries().keys().cloned().collect();
        let plugins = if available {
            ids.iter().filter_map(|id| self.plugin_view(id)).collect()
        } else {
            Vec::new()
        };
        Ok(PluginList {
            available,
            reason,
            plugins,
        })
    }

    pub(super) async fn plugins_rescan(&self) -> Result<PluginList> {
        let _turn = self.plugins.lifecycle.lock().await;
        self.plugin_host()?;
        self.load_plugins().await?;
        drop(_turn);
        self.plugins_list().await
    }

    fn check_runnable(m: &Manifest) -> Result<()> {
        match unsupported(m) {
            Some(why) => Err(Error::InvalidInput(format!(
                "{} cannot be used here: {why}",
                m.name
            ))),
            None => Ok(()),
        }
    }

    pub(super) async fn plugins_stage(&self, zip: Vec<u8>) -> Result<StagedPlugin> {
        let host = self.plugin_host()?;
        if zip.len() > plugin::MAX_PLUGIN_ZIP {
            return Err(Error::InvalidInput(format!(
                "the ZIP is larger than {} MiB",
                plugin::MAX_PLUGIN_ZIP / (1024 * 1024)
            )));
        }
        let layout = host.layout.clone();
        let staged = tokio::task::spawn_blocking(move || -> fetchbox_plugins::Result<Staged> {
            let path = layout.upload_path()?;
            std::fs::write(&path, &zip)?;
            let staged = install::stage_zip(&layout, &path);
            let _ = std::fs::remove_file(&path);
            staged
        })
        .await
        .map_err(|e| join_err(&e))?
        .map_err(Error::from)?;
        self.finish_stage(staged, PluginSource::Zip, None)
    }

    /// Record a staged package and describe it for the user's review.
    pub(super) fn finish_stage(
        &self,
        staged: Staged,
        source: PluginSource,
        store_sha256: Option<String>,
    ) -> Result<StagedPlugin> {
        let host = self.plugin_host()?;
        if let Err(e) = Self::check_runnable(&staged.manifest) {
            install::cancel(&host.layout, &staged.token);
            return Err(e);
        }
        let m = &staged.manifest;
        let replaces = self.plugin_entries().get(&m.id).and_then(|e| {
            e.manifest.as_ref().map(|old| StagedReplaces {
                version: old.version.clone(),
                permissions: derive_permissions(old),
            })
        });
        self.staged_meta()
            .insert(staged.token.clone(), (source, store_sha256));
        Ok(StagedPlugin {
            token: staged.token.clone(),
            id: m.id.clone(),
            name: m.name.clone(),
            version: m.version.clone(),
            description: m.description.clone(),
            author: m.author.clone(),
            homepage: m.homepage.clone(),
            license: m.license.clone(),
            icon: m.ui.icon.clone(),
            source,
            permissions: derive_permissions(m),
            replaces,
        })
    }

    pub(super) async fn plugins_install(&self, token: String) -> Result<PluginView> {
        let host = self.plugin_host()?;
        let _turn = self.plugins.lifecycle.lock().await;
        let layout = host.layout.clone();
        let t = token.clone();
        let staged = tokio::task::spawn_blocking(move || install::load_staged(&layout, &t))
            .await
            .map_err(|e| join_err(&e))?
            .map_err(Error::from)?;
        let id = staged.manifest.id.clone();
        let (source, sha) = self
            .staged_meta()
            .remove(&token)
            .unwrap_or((PluginSource::Zip, None));
        // An older version stops before its files are replaced.
        self.stop_runtime(&id).await;
        let layout = host.layout.clone();
        let t = token.clone();
        let done = tokio::task::spawn_blocking(move || install::commit(&layout, &t))
            .await
            .map_err(|e| join_err(&e))?
            .map_err(Error::from)?;
        let row = self.with_db(|c| {
            store::upsert(
                c,
                &NewPlugin {
                    id: &id,
                    version: &done.manifest.version,
                    source,
                    approved_hash: &done.manifest_hash,
                    store_sha256: sha.as_deref(),
                },
            )
        })?;
        let found = install::Discovered {
            folder: id.clone(),
            result: Ok((done.manifest.clone(), done.manifest_hash.clone())),
        };
        let generation = self
            .plugin_entries()
            .get(&id)
            .map_or(0, |e| e.generation + 1);
        let entry = Self::new_entry(&found, Some(row), generation);
        let run = entry.state == PluginState::Preparing;
        self.plugin_entries().insert(id.clone(), entry);
        tracing::info!(
            target: "activity", plugin = %id, version = %done.manifest.version,
            source = source.as_str(), replaced = done.replaced.as_deref().unwrap_or("-"),
            "plugin installed"
        );
        if run {
            self.spawn_prepare(&id, &Fetch::Missing);
        } else {
            self.publish_plugin(&id);
        }
        self.plugin_view(&id)
            .ok_or_else(|| Error::Internal("plugin vanished after install".into()))
    }

    pub(super) async fn plugins_cancel(&self, token: String) -> Result<()> {
        let host = self.plugin_host()?;
        self.staged_meta().remove(&token);
        install::cancel(&host.layout, &token);
        Ok(())
    }

    pub(super) async fn plugins_enable(&self, id: String, enabled: bool) -> Result<PluginView> {
        self.plugin_host()?;
        let _turn = self.plugins.lifecycle.lock().await;
        let (manifest, state, was_enabled) = {
            let entries = self.plugin_entries();
            let e = entries
                .get(&id)
                .ok_or_else(|| Error::Missing(format!("no plugin \"{id}\"")))?;
            (e.manifest.clone(), e.state, e.enabled())
        };
        if enabled {
            let m = manifest
                .ok_or_else(|| Error::InvalidState("its plugin.json cannot be used".into()))?;
            match state {
                PluginState::NeedsReview => {
                    return Err(Error::InvalidState(
                        "review what it may do and approve it first".into(),
                    ));
                }
                PluginState::Unsupported => {
                    return Err(Error::InvalidState(format!(
                        "{} cannot be used here: {}",
                        m.name,
                        unsupported(&m).unwrap_or_default()
                    )));
                }
                _ => {}
            }
            let values = effective(&m, &self.plugin_stored_settings(&id));
            for f in m.settings.iter().filter(|f| f.required) {
                if !expand::is_set(values.get(&f.key)) {
                    let name = if f.label.is_empty() { &f.key } else { &f.label };
                    return Err(Error::InvalidInput(format!("set \"{name}\" first")));
                }
            }
        }
        let found = self.with_db(|c| store::set_enabled(c, &id, enabled))?;
        if !found {
            return Err(Error::Missing(format!("no plugin \"{id}\"")));
        }
        if let Some(e) = self.plugin_entries().get_mut(&id) {
            if let Some(s) = &mut e.stored {
                s.enabled = enabled;
            }
        }
        if enabled {
            tracing::info!(target: "activity", plugin = %id, "plugin switched on");
            self.spawn_prepare(&id, &Fetch::Missing);
        } else {
            self.stop_runtime(&id).await;
            if let Some(e) = self.plugin_entries().get_mut(&id) {
                let (state, error) = Self::resting_state(e);
                e.state = state;
                e.error = error;
            }
            if was_enabled {
                tracing::info!(target: "activity", plugin = %id, "plugin switched off");
            }
            self.publish_plugin(&id);
        }
        self.plugin_view(&id)
            .ok_or_else(|| Error::Missing(format!("no plugin \"{id}\"")))
    }

    pub(super) async fn plugins_approve(&self, id: String, hash: String) -> Result<PluginView> {
        self.plugin_host()?;
        let _turn = self.plugins.lifecycle.lock().await;
        {
            let entries = self.plugin_entries();
            let e = entries
                .get(&id)
                .ok_or_else(|| Error::Missing(format!("no plugin \"{id}\"")))?;
            if e.manifest.is_none() || e.hash != hash {
                return Err(Error::InvalidState(
                    "its files changed since you looked: reload and review it again".into(),
                ));
            }
        }
        self.with_db(|c| store::set_approved_hash(c, &id, &hash))?;
        let run = {
            let mut entries = self.plugin_entries();
            let Some(e) = entries.get_mut(&id) else {
                return Err(Error::Missing(format!("no plugin \"{id}\"")));
            };
            if let Some(s) = &mut e.stored {
                s.approved_hash = hash;
            }
            let (state, error) = Self::resting_state(e);
            e.state = state;
            e.error = error;
            state == PluginState::Preparing
        };
        tracing::info!(target: "activity", plugin = %id, "plugin approved");
        if run {
            self.spawn_prepare(&id, &Fetch::Missing);
        } else {
            self.publish_plugin(&id);
        }
        self.plugin_view(&id)
            .ok_or_else(|| Error::Missing(format!("no plugin \"{id}\"")))
    }

    pub(super) async fn plugins_remove(&self, id: String, keep_data: bool) -> Result<()> {
        let host = self.plugin_host()?;
        let _turn = self.plugins.lifecycle.lock().await;
        if !self.plugin_entries().contains_key(&id) {
            return Err(Error::Missing(format!("no plugin \"{id}\"")));
        }
        self.stop_runtime(&id).await;
        let layout = host.layout.clone();
        let id2 = id.clone();
        let removed =
            tokio::task::spawn_blocking(move || install::remove(&layout, &id2, keep_data))
                .await
                .map_err(|e| join_err(&e))?;
        match removed {
            Ok(()) | Err(fetchbox_plugins::Error::NotFound(_)) => {}
            Err(e) => return Err(e.into()),
        }
        self.with_db(|c| store::delete(c, &id))?;
        self.plugin_entries().remove(&id);
        tracing::info!(target: "activity", plugin = %id, kept_data = keep_data, "plugin removed");
        self.publish(Event::PluginRemoved { id });
        Ok(())
    }

    pub(super) async fn plugins_settings(
        &self,
        id: String,
        values: BTreeMap<String, Option<SettingValue>>,
    ) -> Result<PluginView> {
        self.plugin_host()?;
        let manifest = self
            .plugin_entries()
            .get(&id)
            .and_then(|e| e.manifest.clone())
            .ok_or_else(|| Error::Missing(format!("no plugin \"{id}\"")))?;
        let mut changes: Vec<(String, Option<String>)> = Vec::new();
        for (key, value) in &values {
            let field = manifest
                .settings
                .iter()
                .find(|f| f.key == *key)
                .ok_or_else(|| {
                    Error::InvalidInput(format!("\"{key}\" is not a setting of this plugin"))
                })?;
            match value {
                None => changes.push((key.clone(), None)),
                Some(v) => {
                    plugin::check_setting_value(field, v).map_err(Error::InvalidInput)?;
                    changes.push((key.clone(), Some(stored_text(v))));
                }
            }
        }
        let refs: Vec<(&str, Option<&str>)> = changes
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_deref()))
            .collect();
        self.with_db(|c| store::save_settings(c, &id, &refs))?;
        tracing::info!(
            target: "activity", plugin = %id,
            keys = %changes.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(","),
            "plugin settings changed"
        );
        // A program that is running hears about it.
        let proc = self
            .plugin_entries()
            .get(&id)
            .and_then(|e| e.process.clone());
        if let (Some(p), Ok(env)) = (proc, self.run_env(&id)) {
            let all: serde_json::Map<String, serde_json::Value> = env
                .settings
                .iter()
                .map(|(k, v)| (k.clone(), serde_json::to_value(v).unwrap_or_default()))
                .collect();
            p.notify("settings_changed", serde_json::json!({ "settings": all }));
        }
        self.publish_plugin(&id);
        self.plugin_view(&id)
            .ok_or_else(|| Error::Missing(format!("no plugin \"{id}\"")))
    }

    pub(super) async fn plugins_restart(&self, id: String) -> Result<PluginView> {
        self.plugin_host()?;
        let _turn = self.plugins.lifecycle.lock().await;
        let (enabled, proc) = {
            let entries = self.plugin_entries();
            let e = entries
                .get(&id)
                .ok_or_else(|| Error::Missing(format!("no plugin \"{id}\"")))?;
            (e.enabled() && e.approved(), e.process.clone())
        };
        if !enabled {
            return Err(Error::InvalidState("the plugin is not switched on".into()));
        }
        match proc {
            Some(p) => p.restart().await,
            None => self.spawn_prepare(&id, &Fetch::Missing),
        }
        self.plugin_view(&id)
            .ok_or_else(|| Error::Missing(format!("no plugin \"{id}\"")))
    }

    pub(super) async fn plugins_dependencies(
        &self,
        id: String,
        dependency: Option<String>,
    ) -> Result<PluginView> {
        self.plugin_host()?;
        let _turn = self.plugins.lifecycle.lock().await;
        let (manifest, enabled) = {
            let entries = self.plugin_entries();
            let e = entries
                .get(&id)
                .ok_or_else(|| Error::Missing(format!("no plugin \"{id}\"")))?;
            (e.manifest.clone(), e.enabled() && e.approved())
        };
        let m =
            manifest.ok_or_else(|| Error::InvalidState("its plugin.json cannot be used".into()))?;
        if !enabled {
            return Err(Error::InvalidState("switch the plugin on first".into()));
        }
        let which = match dependency {
            Some(d) => {
                if !m.dependencies.iter().any(|x| x.id == d) {
                    return Err(Error::Missing(format!(
                        "\"{d}\" is not a dependency of this plugin"
                    )));
                }
                Fetch::One(d)
            }
            None => Fetch::Update,
        };
        // The program restarts on the new files.
        let proc = self
            .plugin_entries()
            .get_mut(&id)
            .and_then(|e| e.process.take());
        if let Some(p) = proc {
            p.stop().await;
        }
        self.spawn_prepare(&id, &which);
        self.plugin_view(&id)
            .ok_or_else(|| Error::Missing(format!("no plugin \"{id}\"")))
    }

    pub(super) async fn plugins_logs(&self, id: String) -> Result<Vec<String>> {
        self.plugin_host()?;
        let entries = self.plugin_entries();
        let e = entries
            .get(&id)
            .ok_or_else(|| Error::Missing(format!("no plugin \"{id}\"")))?;
        Ok(e.process.as_ref().map(|p| p.logs()).unwrap_or_default())
    }

    // ---- actions ---------------------------------------------------------------------

    pub(super) async fn plugins_action(
        &self,
        id: String,
        action: String,
        input: BTreeMap<String, SettingValue>,
    ) -> Result<ActionResult> {
        let host = self.plugin_host()?;
        let (manifest, state) = {
            let entries = self.plugin_entries();
            let e = entries
                .get(&id)
                .ok_or_else(|| Error::Missing(format!("no plugin \"{id}\"")))?;
            (e.manifest.clone(), e.state)
        };
        let m =
            manifest.ok_or_else(|| Error::InvalidState("its plugin.json cannot be used".into()))?;
        let a = m
            .ui
            .actions
            .iter()
            .find(|a| a.id == action)
            .ok_or_else(|| Error::Missing(format!("\"{action}\" is not an action of this plugin")))?
            .clone();
        let usable = matches!(state, PluginState::Ready | PluginState::Running)
            || (a.kind == ActionKind::Builtin
                && matches!(state, PluginState::Failed | PluginState::Starting));
        if !usable {
            return Err(Error::InvalidState("the plugin is not running".into()));
        }
        tracing::info!(target: "activity", plugin = %id, action = %a.id, "plugin action");
        let input_json = serde_json::to_value(&input).unwrap_or_default();
        match a.kind {
            ActionKind::OpenDialog => Ok(ActionResult {
                ok: true,
                open_dialog: a.dialog.clone(),
                ..ActionResult::default()
            }),
            ActionKind::OpenUrl => Ok(ActionResult {
                ok: true,
                ..ActionResult::default()
            }),
            ActionKind::Builtin => match a.builtin {
                Some(Builtin::UpdateDependencies) => {
                    self.plugins_dependencies(id, None).await?;
                    Ok(ActionResult {
                        ok: true,
                        message: Some("Updating".into()),
                        ..ActionResult::default()
                    })
                }
                Some(Builtin::Restart) => {
                    self.plugins_restart(id).await?;
                    Ok(ActionResult {
                        ok: true,
                        ..ActionResult::default()
                    })
                }
                None => Err(Error::InvalidInput("the action has no builtin".into())),
            },
            ActionKind::Run => {
                let env = self.run_env(&id)?;
                let ctx = env.context();
                let command = a.command.as_deref().unwrap_or_default();
                let program = expand::resolve_program(command, &ctx).map_err(Error::from)?;
                let args = expand::expand_args(&a.args, &ctx).map_err(Error::from)?;
                let mut spec = Spec::new(
                    program,
                    args,
                    env.child_env(BTreeMap::new()),
                    Duration::from_secs(u64::from(a.timeout_s.clamp(1, 120))),
                );
                spec.stdin = Some(input_json.to_string().into_bytes());
                let out = host.runner.run(&id, spec).await.map_err(Error::from)?;
                let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
                Ok(ActionResult {
                    ok: true,
                    message: (!text.is_empty()).then(|| text.chars().take(4000).collect()),
                    level: Some(NoticeLevel::Success),
                    ..ActionResult::default()
                })
            }
            ActionKind::Webhook => {
                let env = self.run_env(&id)?;
                let hook = a
                    .webhook
                    .as_ref()
                    .ok_or_else(|| Error::InvalidInput("the action has no webhook".into()))?;
                let doc = serde_json::json!({"plugin": id, "action": a.id, "input": input_json});
                let status = webhook::send(&host.fetcher, hook, &env.context(), &doc)
                    .await
                    .map_err(Error::from)?;
                Ok(ActionResult {
                    ok: true,
                    message: Some(format!("Sent ({status})")),
                    level: Some(NoticeLevel::Success),
                    ..ActionResult::default()
                })
            }
            ActionKind::Process => {
                let proc = self
                    .plugin_entries()
                    .get(&id)
                    .and_then(|e| e.process.clone())
                    .ok_or_else(|| {
                        Error::InvalidState("the plugin's program is not running".into())
                    })?;
                let answer = proc
                    .request(
                        "invoke",
                        serde_json::json!({"action": a.id, "input": input_json}),
                        INVOKE_TIMEOUT,
                    )
                    .await
                    .map_err(Error::from)?;
                Ok(action_result_from(&answer))
            }
        }
    }
}

/// Read what a plugin's program answered to `invoke`: an object with the fields of
/// [`ActionResult`] (all optional), or anything else, shown as text.
fn action_result_from(answer: &serde_json::Value) -> ActionResult {
    // An object that has none of the fields of a result is just data: show it.
    const KNOWN: [&str; 6] = ["ok", "message", "level", "close", "set", "open_dialog"];
    let result_shaped = answer
        .as_object()
        .is_some_and(|o| o.keys().any(|k| KNOWN.contains(&k.as_str())));
    let Some(o) = answer.as_object().filter(|_| result_shaped) else {
        return ActionResult {
            ok: true,
            message: match answer {
                serde_json::Value::Null => None,
                serde_json::Value::String(s) => Some(s.clone()),
                other => Some(other.to_string()),
            },
            ..ActionResult::default()
        };
    };
    let text = |k: &str| {
        o.get(k)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    let level = match o.get("level").and_then(serde_json::Value::as_str) {
        Some("success") => Some(NoticeLevel::Success),
        Some("warning") => Some(NoticeLevel::Warning),
        Some("error") => Some(NoticeLevel::Error),
        Some("info") => Some(NoticeLevel::Info),
        _ => None,
    };
    let set = o
        .get("set")
        .and_then(serde_json::Value::as_object)
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| {
                    serde_json::from_value::<SettingValue>(v.clone())
                        .ok()
                        .map(|v| (k.clone(), v))
                })
                .collect()
        })
        .unwrap_or_default();
    ActionResult {
        ok: o
            .get("ok")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true),
        message: text("message"),
        level,
        close: o
            .get("close")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        set,
        open_dialog: text("open_dialog"),
    }
}
