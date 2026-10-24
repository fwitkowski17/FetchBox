//! What the interface is told about plugins. These are projections of the manifest: they carry no
//! commands and no stored secrets, only what is needed to draw the cards, forms and dialogs.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::manifest::{
    Action, ActionKind, Dependency, Dialog, Field, HookAction, Manifest, PermissionGroup,
    Placement, SettingValue, Tab,
};
use super::text::host_of;

/// Where a plugin came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum PluginSource {
    /// Shipped inside the program.
    Bundled,
    /// Downloaded from the plugin store (a catalog entry).
    Store,
    /// A ZIP file the user chose.
    Zip,
    /// A folder placed in the plugins directory.
    Folder,
}

impl PluginSource {
    /// Stable name, as stored in SQLite.
    pub fn as_str(self) -> &'static str {
        match self {
            PluginSource::Bundled => "bundled",
            PluginSource::Store => "store",
            PluginSource::Zip => "zip",
            PluginSource::Folder => "folder",
        }
    }

    /// Parse the stored name.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "bundled" => PluginSource::Bundled,
            "store" => PluginSource::Store,
            "zip" => PluginSource::Zip,
            "folder" => PluginSource::Folder,
            _ => return None,
        })
    }
}

/// Where a plugin stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum PluginState {
    Disabled,
    /// Enabled, its dependencies are being fetched or checked.
    Preparing,
    /// Enabled and usable; has no program of its own running.
    Ready,
    /// Its long-lived program is starting.
    Starting,
    /// Its long-lived program runs.
    Running,
    /// It could not be prepared or started; `error` says why.
    Failed,
    /// The files on disk differ from what the user approved: it does not run until approved again.
    NeedsReview,
    /// The manifest does not support this platform or FetchBox version.
    Unsupported,
}

/// Importance of a message from a plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum NoticeLevel {
    Info,
    Success,
    Warning,
    Error,
}

/// What a plugin may do, worked out from its manifest and shown before it is installed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PermissionSummary {
    /// The groups it declares, in display order, each with the author's reason.
    pub groups: Vec<PermissionGroupView>,
    /// Hosts it says it talks to.
    pub network: Vec<String>,
    /// Parts of FetchBox's API it uses.
    pub api: Vec<String>,
    /// Programs of its own that it can run (its dependencies are listed apart, in `dependencies`).
    pub programs: Vec<String>,
    /// The programs it needs and FetchBox downloads for it, by id.
    pub dependencies: Vec<DependencyNeed>,
    /// Where FetchBox sends webhooks on its behalf.
    pub webhook_hosts: Vec<String>,
    /// Events it receives.
    pub events: Vec<String>,
    /// It has a program that runs for as long as it is enabled.
    pub process: bool,
    /// It downloads files with its own program instead of FetchBox's engine.
    pub runs_downloads: bool,
}

/// Where a dependency comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum DependencySource {
    /// A file per platform at an address the manifest gives.
    Pinned,
    /// The releases of a GitHub repository.
    Github,
    /// A program of FetchBox's own list, installed once for every plugin that names it.
    Shared,
}

/// A program a plugin needs, as the review shows it: not a permission, but something FetchBox
/// downloads from the internet for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DependencyNeed {
    pub id: String,
    /// The manifest's label, else the shared program's, else the id.
    pub label: String,
    /// It works without it: nothing is downloaded until the user asks for it.
    pub optional: bool,
    pub source: DependencySource,
    /// Where it is downloaded from (`github.com/owner/name`, or a host), sorted.
    pub hosts: Vec<String>,
    /// Bytes to download on this machine's platform, when the manifest or the list says (a GitHub
    /// release is not asked about while a package is only being reviewed).
    pub size: Option<u64>,
}

/// What switching a plugin on does about one dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum DependencyPlan {
    /// It is on this machine already (for a shared program, another plugin may have fetched it).
    Present,
    /// It is downloaded when the plugin is switched on.
    Download,
    /// It is optional and nobody asked for it: downloaded only if the user chooses to.
    OnRequest,
}

/// One dependency of a staged plugin and what happens to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct StagedDependency {
    pub id: String,
    pub plan: DependencyPlan,
}

/// A declared group and why the plugin wants it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PermissionGroupView {
    pub id: PermissionGroup,
    /// The author's reason (English, as written in the manifest).
    pub reason: Option<String>,
    /// The reason in the languages the manifest translates it to (`locale` -> text).
    pub reason_i18n: BTreeMap<String, String>,
}

/// How a command is listed among the programs it can run. A dependency (`${dep:id}`) is not: it is
/// listed in `dependencies`.
fn program_label(command: &str) -> Option<String> {
    if command.starts_with("${dep:") && command.ends_with('}') {
        return None;
    }
    Some(
        command
            .strip_prefix("${plugin_dir}/")
            .unwrap_or(command)
            .to_owned(),
    )
}

fn url_label(url: &str) -> String {
    host_of(url).unwrap_or_else(|| {
        url.strip_prefix("${setting:")
            .and_then(|r| r.strip_suffix('}'))
            .map_or_else(|| url.to_owned(), |k| format!("{{setting:{k}}}"))
    })
}

/// A dependency as the review shows it (see [`DependencyNeed`]).
fn dependency_need(d: &Dependency) -> DependencyNeed {
    let shared = d
        .shared
        .as_deref()
        .and_then(super::programs::shared_program);
    let (source, files) = match (shared, &d.github) {
        (Some(program), _) => (DependencySource::Shared, Some(&program.platforms)),
        (None, Some(_)) => (DependencySource::Github, None),
        (None, None) => (DependencySource::Pinned, Some(&d.platforms)),
    };
    let mut hosts: Vec<String> = files
        .into_iter()
        .flat_map(|files| files.values().map(|src| url_label(&src.url)))
        .collect();
    if let Some(g) = &d.github {
        hosts.push(format!("github.com/{}", g.repo));
    }
    hosts.sort();
    hosts.dedup();
    let label = if !d.label.is_empty() {
        d.label.clone()
    } else {
        shared.map_or_else(|| d.id.clone(), |program| program.label.clone())
    };
    DependencyNeed {
        id: d.id.clone(),
        label,
        optional: d.optional,
        source,
        hosts,
        size: files
            .and_then(|files| files.get(&super::text::platform_key()))
            .and_then(|src| src.size),
    }
}

/// Work out what the plugin may do. Pure and stable (sorted, no duplicates), so two versions can
/// be compared line by line.
pub fn derive_permissions(m: &Manifest) -> PermissionSummary {
    let mut p = PermissionSummary {
        network: m.permissions.network.clone(),
        api: m.permissions.api.clone(),
        process: m.process.is_some(),
        ..PermissionSummary::default()
    };
    for d in &m.dependencies {
        p.dependencies.push(dependency_need(d));
    }
    for r in &m.resolvers {
        if let Some(c) = &r.probe {
            p.programs.extend(program_label(&c.command));
        }
        if let Some(c) = &r.run {
            p.programs.extend(program_label(&c.command));
            p.runs_downloads = true;
        }
    }
    for h in &m.hooks {
        p.events.extend(h.on.iter().cloned());
        match &h.action {
            HookAction::Webhook(w) => p.webhook_hosts.push(url_label(&w.url)),
            HookAction::Run(c) => p.programs.extend(program_label(&c.command)),
            HookAction::Process(_) => {}
        }
    }
    for a in &m.ui.actions {
        match a.kind {
            ActionKind::Run => {
                if let Some(c) = &a.command {
                    p.programs.extend(program_label(c));
                }
            }
            ActionKind::Webhook => {
                if let Some(w) = &a.webhook {
                    p.webhook_hosts.push(url_label(&w.url));
                }
            }
            _ => {}
        }
    }
    if let Some(proc) = &m.process {
        p.programs.extend(program_label(&proc.command));
    }
    for list in [
        &mut p.network,
        &mut p.api,
        &mut p.programs,
        &mut p.webhook_hosts,
        &mut p.events,
    ] {
        list.sort();
        list.dedup();
    }
    p.dependencies.sort_by(|a, b| a.id.cmp(&b.id));
    p.groups = PermissionGroup::ALL
        .into_iter()
        .filter(|g| m.permissions.uses.contains(g))
        .map(|g| {
            let reason = m.permissions.reasons.get(g.as_str()).cloned();
            let reason_i18n = reason
                .as_deref()
                .map(|r| {
                    m.i18n
                        .iter()
                        .filter_map(|(locale, texts)| {
                            texts.get(r).map(|t| (locale.clone(), t.clone()))
                        })
                        .collect()
                })
                .unwrap_or_default();
            PermissionGroupView {
                id: g,
                reason,
                reason_i18n,
            }
        })
        .collect();
    p
}

/// The groups the manifest really needs, whatever it declares: what `validate` holds `uses` to.
pub fn used_groups(m: &Manifest) -> BTreeSet<PermissionGroup> {
    let p = derive_permissions(m);
    let mut used = BTreeSet::new();
    let mut add = |on: bool, g: PermissionGroup| {
        if on {
            used.insert(g);
        }
    };
    add(
        !p.network.is_empty() || !p.dependencies.is_empty() || !p.webhook_hosts.is_empty(),
        PermissionGroup::Network,
    );
    add(
        !p.programs.is_empty() || !p.dependencies.is_empty(),
        PermissionGroup::Programs,
    );
    add(!p.events.is_empty(), PermissionGroup::Events);
    add(p.process, PermissionGroup::Background);
    add(p.runs_downloads, PermissionGroup::Downloads);
    add(!p.api.is_empty(), PermissionGroup::Api);
    used
}

/// A setting as the interface needs it: the field, and whether it has a value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct FieldView {
    pub field: Field,
    /// The stored value; never set for a `secret`.
    pub value: Option<SettingValue>,
    /// Something is stored (also tells a secret was saved).
    pub has_value: bool,
}

/// An action without its command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ActionView {
    pub id: String,
    pub label: String,
    pub icon: Option<String>,
    pub placement: Placement,
    pub confirm: Option<String>,
    pub kind: ActionKind,
    /// The dialog an `open_dialog` action opens.
    pub dialog: Option<String>,
    /// The address an `open_url` action opens.
    pub url: Option<String>,
}

impl ActionView {
    pub fn of(a: &Action) -> Self {
        ActionView {
            id: a.id.clone(),
            label: a.label.clone(),
            icon: a.icon.clone(),
            placement: a.placement,
            confirm: a.confirm.clone(),
            kind: a.kind,
            dialog: a.dialog.clone(),
            url: a.url.clone(),
        }
    }
}

/// The plugin's interface, as drawn.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct UiView {
    pub icon: Option<String>,
    pub tab: Option<Tab>,
    pub actions: Vec<ActionView>,
    pub dialogs: Vec<Dialog>,
}

/// What a plugin adds.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Provides {
    pub resolvers: Vec<String>,
    pub hooks: usize,
    pub process: bool,
}

/// How far a dependency is on its way.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DependencyProgress {
    pub dependency: String,
    pub downloaded: u64,
    pub total: Option<u64>,
}

/// A dependency and whether it is in place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DependencyView {
    pub id: String,
    pub label: String,
    pub optional: bool,
    pub installed: bool,
    /// The version it was fetched at (a release tag), when it has one.
    pub version: Option<String>,
}

/// The plugins, and whether the plugin system is on at all.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PluginList {
    /// `false` when plugins are switched off (`--plugins off`) or the server runs without
    /// authentication (a plugin runs programs; that must not be open to anyone).
    pub available: bool,
    /// Why they are not available: `off` or `no_auth`.
    pub reason: Option<String>,
    pub plugins: Vec<PluginView>,
}

/// An installed plugin.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PluginView {
    pub id: String,
    pub name: String,
    pub version: String,
    /// SHA-256 of the `plugin.json` on disk. A plugin in `needs_review` is approved by sending this
    /// back, which says the user looked at exactly this manifest.
    pub manifest_hash: String,
    pub description: String,
    pub author: String,
    pub homepage: Option<String>,
    pub license: Option<String>,
    pub source: PluginSource,
    pub enabled: bool,
    pub state: PluginState,
    /// Why it failed, or what needs a review.
    pub error: Option<String>,
    /// A line the plugin's program reported about itself.
    pub status_text: Option<String>,
    pub provides: Provides,
    pub permissions: PermissionSummary,
    pub settings: Vec<FieldView>,
    pub ui: UiView,
    pub dependencies: Vec<DependencyView>,
    pub progress: Option<DependencyProgress>,
    /// A newer version in the store.
    pub update_available: Option<String>,
    pub i18n: BTreeMap<String, BTreeMap<String, String>>,
}

/// A plugin that was uploaded and read but is not installed yet: the user reviews it first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct StagedPlugin {
    /// Identifies the staged files for `install` and `cancel`.
    pub token: String,
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub homepage: Option<String>,
    pub license: Option<String>,
    pub icon: Option<String>,
    pub source: PluginSource,
    pub permissions: PermissionSummary,
    /// What switching it on does about each of its dependencies on this machine.
    pub dependency_plan: Vec<StagedDependency>,
    /// What it replaces: the installed version and what that one was allowed.
    pub replaces: Option<StagedReplaces>,
}

/// The installed plugin a staged one would replace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct StagedReplaces {
    pub version: String,
    pub permissions: PermissionSummary,
    /// It is switched on now, so its dependencies are downloaded as soon as the update is installed.
    pub enabled: bool,
}

/// Where a store entry is fetched from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct StoreDownload {
    pub url: String,
    pub sha256: String,
    pub size: Option<u64>,
}

/// A plugin offered in the store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct StoreEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub homepage: Option<String>,
    pub license: Option<String>,
    pub icon: Option<String>,
    pub tags: Vec<String>,
    pub min_fetchbox: Option<String>,
    pub platforms: Vec<String>,
    /// Shipped inside the program (works offline).
    pub bundled: bool,
    /// Where a third-party entry is fetched from; none for a bundled one.
    pub download: Option<StoreDownload>,
    /// The version that is installed, if any.
    pub installed: Option<String>,
    /// A newer version than the installed one exists.
    pub update_available: bool,
    /// This platform or FetchBox version cannot run it.
    pub unsupported: bool,
}

/// The plugin store: what can be installed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct StoreList {
    pub entries: Vec<StoreEntry>,
    /// A remote catalog is configured (the entries that ship with FetchBox are always there).
    pub remote: bool,
    /// When the remote catalog was last fetched (unix seconds).
    pub fetched_at: Option<i64>,
    /// Why the remote catalog could not be fetched, if the last try failed.
    pub error: Option<String>,
}

/// The answer to an action.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ActionResult {
    pub ok: bool,
    pub message: Option<String>,
    pub level: Option<NoticeLevel>,
    /// Close the dialog the action came from.
    #[serde(default)]
    pub close: bool,
    /// Values for fields of the open dialog (an `output` field shows its text).
    #[serde(default)]
    pub set: BTreeMap<String, SettingValue>,
    /// Open this dialog next.
    pub open_dialog: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> Manifest {
        serde_json::from_str(
            r#"{
              "api_version": 1, "id": "demo", "name": "Demo", "version": "1.0.0",
              "permissions": {
                "uses": ["events", "network", "programs", "downloads", "background"],
                "reasons": {"network": "Talks to servers."},
                "network": ["b.example", "a.example", "a.example"]
              },
              "i18n": {"pl": {"Talks to servers.": "Łączy się z serwerami."}},
              "dependencies": [
                {"id": "tool", "platforms": {"linux-x86_64": {"url": "https://dl.example.com/t.tar.gz", "sha256": "00"}}},
                {"id": "other", "github": {"repo": "o/r", "assets": {"linux-x86_64": "x"}}}
              ],
              "resolvers": [{"id": "r", "match": ["https://x/*"],
                "probe": {"command": "${dep:tool}"},
                "run": {"command": "${dep:tool}"}}],
              "hooks": [
                {"on": ["download.completed"], "action": {"webhook": {"url": "https://hooks.example.org/x"}}},
                {"on": ["download.failed"], "action": {"webhook": {"url": "${setting:url}"}}},
                {"on": ["*"], "action": {"run": {"command": "${plugin_dir}/bin/notify"}}}
              ],
              "process": {"command": "${plugin_dir}/bin/daemon"}
            }"#,
        )
        .expect("a manifest")
    }

    #[test]
    fn dependencies_are_listed_apart_from_the_programs_it_runs() {
        let p = derive_permissions(&manifest());
        assert_eq!(p.network, vec!["a.example", "b.example"]);
        // The programs of its own; a dependency is not a permission, it is listed in `dependencies`.
        assert_eq!(p.programs, vec!["bin/daemon", "bin/notify"]);
        let deps: Vec<_> = p
            .dependencies
            .iter()
            .map(|d| (d.id.as_str(), d.source, d.hosts.clone(), d.optional))
            .collect();
        assert_eq!(
            deps,
            vec![
                (
                    "other",
                    DependencySource::Github,
                    vec!["github.com/o/r".to_owned()],
                    false
                ),
                (
                    "tool",
                    DependencySource::Pinned,
                    vec!["dl.example.com".to_owned()],
                    false
                ),
            ]
        );
        assert_eq!(p.webhook_hosts, vec!["hooks.example.org", "{setting:url}"]);
        assert_eq!(p.events, vec!["*", "download.completed", "download.failed"]);
        assert!(p.process);
        assert!(p.runs_downloads);
        // The permission model is the same as before: the groups the manifest needs.
        assert_eq!(used_groups(&manifest()).len(), 5);
    }

    #[test]
    fn a_shared_program_shows_where_fetchbox_gets_it_from_and_how_big_it_is() {
        let mut m = manifest();
        m.dependencies.push(super::super::Dependency {
            id: "ffmpeg".into(),
            label: String::new(),
            optional: true,
            bin: None,
            platforms: BTreeMap::new(),
            github: None,
            shared: Some("ffmpeg".into()),
        });
        let p = derive_permissions(&m);
        let ffmpeg = p
            .dependencies
            .iter()
            .find(|d| d.id == "ffmpeg")
            .expect("ffmpeg");
        assert_eq!(ffmpeg.source, DependencySource::Shared);
        assert!(ffmpeg.optional);
        assert_eq!(ffmpeg.hosts, vec!["files.pythonhosted.org"]);
        // The label falls back to the entry's, and the size is that of this machine's platform.
        let entry = super::super::shared_program("ffmpeg").expect("entry");
        assert_eq!(ffmpeg.label, entry.label);
        let expected = entry
            .platforms
            .get(&super::super::platform_key())
            .and_then(|s| s.size);
        assert_eq!(ffmpeg.size, expected);
        assert!(!p.programs.iter().any(|x| x.contains("ffmpeg")));

        // A plugin whose only need is a shared program still needs `network` and `programs`.
        let only: Manifest = serde_json::from_value(serde_json::json!({
            "api_version": 1, "id": "demo", "name": "Demo", "version": "1.0.0",
            "dependencies": [{"id": "ffmpeg", "shared": "ffmpeg"}]
        }))
        .expect("manifest");
        let used: Vec<_> = used_groups(&only).into_iter().map(|g| g.as_str()).collect();
        assert_eq!(used, ["network", "programs"]);
    }

    #[test]
    fn declared_groups_come_in_display_order_with_their_reasons() {
        let m = manifest();
        let p = derive_permissions(&m);
        let ids: Vec<_> = p.groups.iter().map(|g| g.id).collect();
        assert_eq!(
            ids,
            vec![
                PermissionGroup::Network,
                PermissionGroup::Programs,
                PermissionGroup::Downloads,
                PermissionGroup::Background,
                PermissionGroup::Events,
            ]
        );
        assert_eq!(p.groups[0].reason.as_deref(), Some("Talks to servers."));
        assert_eq!(
            p.groups[0].reason_i18n.get("pl").map(String::as_str),
            Some("Łączy się z serwerami.")
        );
        assert_eq!(p.groups[1].reason, None);
        assert_eq!(used_groups(&m).len(), 5);
    }

    #[test]
    fn a_plugin_that_only_declares_a_name_may_do_nothing() {
        let m: Manifest = serde_json::from_str(
            r#"{"api_version":1,"id":"quiet","name":"Quiet","version":"0.1.0"}"#,
        )
        .expect("a manifest");
        assert_eq!(derive_permissions(&m), PermissionSummary::default());
    }
}
