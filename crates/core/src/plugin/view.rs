//! What the interface is told about plugins. These are projections of the manifest: they carry no
//! commands and no stored secrets, only what is needed to draw the cards, forms and dialogs.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::manifest::{
    Action, ActionKind, Dialog, Field, HookAction, Manifest, Placement, SettingValue, Tab,
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
    /// Hosts it says it talks to.
    pub network: Vec<String>,
    /// Parts of FetchBox's API it uses.
    pub api: Vec<String>,
    /// Programs it can run (dependencies and files of its own).
    pub programs: Vec<String>,
    /// Where its dependencies are downloaded from.
    pub download_hosts: Vec<String>,
    /// Where FetchBox sends webhooks on its behalf.
    pub webhook_hosts: Vec<String>,
    /// Events it receives.
    pub events: Vec<String>,
    /// It has a program that runs for as long as it is enabled.
    pub process: bool,
    /// It downloads files with its own program instead of FetchBox's engine.
    pub runs_downloads: bool,
}

fn program_label(command: &str) -> String {
    match command
        .strip_prefix("${dep:")
        .and_then(|r| r.strip_suffix('}'))
    {
        Some(dep) => format!("{dep} (dependency)"),
        None => command
            .strip_prefix("${plugin_dir}/")
            .unwrap_or(command)
            .to_owned(),
    }
}

fn url_label(url: &str) -> String {
    host_of(url).unwrap_or_else(|| {
        url.strip_prefix("${setting:")
            .and_then(|r| r.strip_suffix('}'))
            .map_or_else(|| url.to_owned(), |k| format!("{{setting:{k}}}"))
    })
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
        for src in d.platforms.values() {
            p.download_hosts.push(url_label(&src.url));
        }
        if let Some(g) = &d.github {
            p.download_hosts.push(format!("github.com/{}", g.repo));
        }
        p.programs.push(format!("{} (dependency)", d.id));
    }
    for r in &m.resolvers {
        if let Some(c) = &r.probe {
            p.programs.push(program_label(&c.command));
        }
        if let Some(c) = &r.run {
            p.programs.push(program_label(&c.command));
            p.runs_downloads = true;
        }
    }
    for h in &m.hooks {
        p.events.extend(h.on.iter().cloned());
        match &h.action {
            HookAction::Webhook(w) => p.webhook_hosts.push(url_label(&w.url)),
            HookAction::Run(c) => p.programs.push(program_label(&c.command)),
            HookAction::Process(_) => {}
        }
    }
    for a in &m.ui.actions {
        match a.kind {
            ActionKind::Run => {
                if let Some(c) = &a.command {
                    p.programs.push(program_label(c));
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
        p.programs.push(program_label(&proc.command));
    }
    for list in [
        &mut p.network,
        &mut p.api,
        &mut p.programs,
        &mut p.download_hosts,
        &mut p.webhook_hosts,
        &mut p.events,
    ] {
        list.sort();
        list.dedup();
    }
    p
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
    /// What it replaces: the installed version and what that one was allowed.
    pub replaces: Option<StagedReplaces>,
}

/// The installed plugin a staged one would replace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct StagedReplaces {
    pub version: String,
    pub permissions: PermissionSummary,
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
              "permissions": {"network": ["b.example", "a.example", "a.example"]},
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
    fn permissions_are_worked_out_from_the_manifest() {
        let p = derive_permissions(&manifest());
        assert_eq!(p.network, vec!["a.example", "b.example"]);
        assert_eq!(
            p.programs,
            vec![
                "bin/daemon",
                "bin/notify",
                "other (dependency)",
                "tool (dependency)"
            ]
        );
        assert_eq!(p.download_hosts, vec!["dl.example.com", "github.com/o/r"]);
        assert_eq!(p.webhook_hosts, vec!["hooks.example.org", "{setting:url}"]);
        assert_eq!(p.events, vec!["*", "download.completed", "download.failed"]);
        assert!(p.process);
        assert!(p.runs_downloads);
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
