//! The manifest (`plugin.json`). Unknown keys are ignored, so a newer manifest with extras still
//! reads; a manifest that needs a newer `api_version` is refused by `validate`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::model::Category;

fn yes() -> bool {
    true
}

fn thirty() -> u32 {
    30
}

/// A plugin: what it is, what it needs and what it adds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub api_version: u32,
    /// Lowercase letters, digits and `-`, starting with a letter (`is_valid_id`).
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    /// The lowest FetchBox version the plugin works with.
    #[serde(default)]
    pub min_fetchbox: Option<String>,
    /// `linux-x86_64`, `linux-aarch64`, `macos-aarch64`, `macos-x86_64`, `windows-x86_64`; empty
    /// means any.
    #[serde(default)]
    pub platforms: Vec<String>,
    #[serde(default)]
    pub permissions: Permissions,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    #[serde(default)]
    pub settings: Vec<Field>,
    #[serde(default)]
    pub resolvers: Vec<Resolver>,
    #[serde(default)]
    pub hooks: Vec<Hook>,
    /// A program that runs for as long as the plugin is enabled and talks JSON lines over stdio.
    #[serde(default)]
    pub process: Option<ProcessSpec>,
    #[serde(default)]
    pub ui: Ui,
    /// Translations of the plugin's own texts: `{"pl": {"English text": "Tekst"}}`.
    #[serde(default)]
    pub i18n: BTreeMap<String, BTreeMap<String, String>>,
}

/// What the plugin says it will reach. The rest (programs, events, ...) the host works out itself.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Permissions {
    /// Host patterns the plugin talks to (`example.com`, `*.example.com`, `*`, `{setting:key}`).
    #[serde(default)]
    pub network: Vec<String>,
    /// Parts of FetchBox's own API it uses (`downloads.read`, `downloads.write`); reserved.
    #[serde(default)]
    pub api: Vec<String>,
}

/// How a downloaded dependency is packed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArchiveKind {
    /// The file is the program itself.
    #[default]
    #[serde(rename = "raw")]
    Raw,
    /// One gzip-compressed file.
    #[serde(rename = "gz")]
    Gz,
    #[serde(rename = "zip")]
    Zip,
    #[serde(rename = "tar.gz")]
    TarGz,
    #[serde(rename = "tar.xz")]
    TarXz,
}

/// One file to download for one platform.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepSource {
    /// `https://` only.
    pub url: String,
    /// Hexadecimal SHA-256 of the downloaded file (not of what is inside it).
    pub sha256: String,
    #[serde(default)]
    pub archive: ArchiveKind,
    /// Leading path components removed from archive entries.
    #[serde(default)]
    pub strip: u32,
    /// Expected size in bytes, an upper bound for the download.
    #[serde(default)]
    pub size: Option<u64>,
}

/// A dependency that follows the releases of a GitHub repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GithubDependency {
    /// `owner/name`.
    pub repo: String,
    /// The release asset to take, per platform.
    pub assets: BTreeMap<String, String>,
    /// A release asset with the checksums (`SHA2-256SUMS`) the downloaded asset is verified against.
    #[serde(default)]
    pub sha256_asset: Option<String>,
    /// A tag; the latest release when absent.
    #[serde(default)]
    pub tag: Option<String>,
}

/// A program or file the plugin needs and FetchBox fetches for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dependency {
    pub id: String,
    #[serde(default)]
    pub label: String,
    /// The plugin works without it (some of its choices need it, see `Choice::requires`).
    #[serde(default)]
    pub optional: bool,
    /// Path of the program inside what was extracted; the dependency's id when absent.
    #[serde(default)]
    pub bin: Option<String>,
    /// Pinned form: a file per platform.
    #[serde(default)]
    pub platforms: BTreeMap<String, DepSource>,
    /// Following form: the releases of a repository.
    #[serde(default)]
    pub github: Option<GithubDependency>,
}

/// What a settings field or a dialog field is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    Text,
    /// Like text, but never shown again once saved.
    Secret,
    Textarea,
    Number,
    Toggle,
    Select,
    /// A static text (the `help`).
    Info,
    /// Read-only, shows the result of the last action (dialogs only).
    Output,
}

/// A value of a field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(untagged)]
pub enum SettingValue {
    Bool(bool),
    Number(f64),
    Text(String),
}

/// One choice of a `select`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct FieldOption {
    pub value: String,
    pub label: String,
}

/// A field is shown only while another one has this value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ShowIf {
    pub key: String,
    pub equals: SettingValue,
}

/// A setting of the plugin, or a field of one of its dialogs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Field {
    /// Lowercase letters, digits and `_` (`is_valid_key`).
    pub key: String,
    #[serde(rename = "type")]
    pub kind: FieldKind,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub help: String,
    #[serde(default)]
    pub default: Option<SettingValue>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub placeholder: String,
    #[serde(default)]
    pub show_if: Option<ShowIf>,
    #[serde(default)]
    pub options: Vec<FieldOption>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    #[serde(default)]
    pub step: Option<f64>,
    /// Shown after a number (`MB/s`).
    #[serde(default)]
    pub unit: String,
}

/// One argument of a command: a template string, or a group that is only there when a condition
/// holds (`setting:key` has a value, `dep:id` is installed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Arg {
    Plain(String),
    When { when: String, args: Vec<String> },
}

/// A program to run, without a shell. `command` is `${dep:id}` or a path inside the plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Command {
    pub command: String,
    #[serde(default)]
    pub args: Vec<Arg>,
    /// Seconds before it is killed (at most 120 for a request, ignored by a download).
    #[serde(default = "thirty")]
    pub timeout_s: u32,
}

/// One or several JSON pointers; the first that exists wins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Pointers {
    One(String),
    Many(Vec<String>),
}

impl Pointers {
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        let (one, many): (Option<&str>, &[String]) = match self {
            Pointers::One(s) => (Some(s.as_str()), &[]),
            Pointers::Many(v) => (None, v.as_slice()),
        };
        one.into_iter().chain(many.iter().map(String::as_str))
    }
}

/// Where to find a playlist item's fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemMap {
    pub name: Pointers,
    pub url: Pointers,
    #[serde(default)]
    pub size: Option<Pointers>,
}

/// Where to find a single result's fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SingleMap {
    pub name: Pointers,
    #[serde(default)]
    pub size: Option<Pointers>,
}

/// How the JSON a probe printed becomes entries of the add dialog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResultMap {
    /// Pointer to an array: the result is a list of items (a playlist).
    #[serde(default)]
    pub items: Option<String>,
    #[serde(default)]
    pub item: Option<ItemMap>,
    #[serde(default)]
    pub single: Option<SingleMap>,
    /// The setting that holds the id of the choice used for each item of a list.
    #[serde(default)]
    pub default_choice_setting: Option<String>,
}

/// A preset of a resolver (quality, audio only, ...).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choice {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub category: Option<Category>,
    /// Extension of the file it produces, when the tool does not tell.
    #[serde(default)]
    pub ext: Option<String>,
    /// Dependencies that must be installed for this choice.
    #[serde(default)]
    pub requires: Vec<String>,
    /// Spliced in by `${choice.args}`.
    #[serde(default)]
    pub args: Vec<String>,
}

/// Handles addresses of some sites.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolver {
    pub id: String,
    /// Globs on the whole address (`https://*.example.com/*`); a `${setting:key}` element is
    /// replaced by the lines or comma-separated items of that setting.
    #[serde(rename = "match", default)]
    pub matches: Vec<String>,
    /// A command that prints JSON about the address.
    #[serde(default)]
    pub probe: Option<Command>,
    #[serde(default)]
    pub result: Option<ResultMap>,
    #[serde(default)]
    pub choices: Vec<Choice>,
    /// The download is done by this command instead of FetchBox's engine (SPEC 6.15).
    #[serde(default)]
    pub run: Option<Command>,
    /// Ask the plugin's long-lived process (`resolve`) instead of running `probe`.
    #[serde(default)]
    pub process: bool,
}

/// Where the body of a webhook comes from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebhookBody {
    /// The event as JSON.
    #[default]
    Event,
    /// `Webhook::template` with the fields of the event substituted.
    Template,
}

/// A request FetchBox sends itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Webhook {
    /// `https://` (or `http://`) address, or a `${setting:key}`.
    pub url: String,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub body: WebhookBody,
    #[serde(default)]
    pub template: Option<String>,
}

/// What a hook does with an event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookAction {
    Webhook(Webhook),
    /// Run a command; the event is on its standard input.
    Run(Command),
    /// Send the event to the plugin's long-lived process.
    Process(bool),
}

/// An event handler.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hook {
    /// Event names (`download.completed`, `download.*`, `*`), see `EVENT_NAMES`.
    pub on: Vec<String>,
    pub action: HookAction,
    /// Progress events are passed on at most this often.
    #[serde(default)]
    pub every_s: Option<u32>,
}

/// What happens when the process dies.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestartPolicy {
    #[default]
    OnFailure,
    Always,
    Never,
}

/// The long-lived process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessSpec {
    pub command: String,
    #[serde(default)]
    pub args: Vec<Arg>,
    /// Extra environment; values are templates.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default = "yes")]
    pub autostart: bool,
    #[serde(default)]
    pub restart: RestartPolicy,
}

/// How the plugin's interface is made. Only `declarative` exists; `iframe` is reserved.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiKind {
    #[default]
    Declarative,
    Iframe,
}

/// Where an action is offered.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    /// On the plugin's card in Extensions.
    #[default]
    Card,
    /// In the plugin's settings.
    Settings,
}

/// What an action does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    /// Run `command` once; its output is the result.
    Run,
    Webhook,
    /// Ask the long-lived process (`invoke`).
    Process,
    /// Something the host does itself (`builtin`).
    Builtin,
    /// Open the dialog `dialog`.
    OpenDialog,
    /// Open `url` in a new tab (http and https only).
    OpenUrl,
}

/// Actions the host performs itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Builtin {
    UpdateDependencies,
    Restart,
}

/// A button of the plugin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Action {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub placement: Placement,
    /// Ask this before doing it.
    #[serde(default)]
    pub confirm: Option<String>,
    pub kind: ActionKind,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<Arg>,
    #[serde(default = "thirty")]
    pub timeout_s: u32,
    #[serde(default)]
    pub webhook: Option<Webhook>,
    #[serde(default)]
    pub builtin: Option<Builtin>,
    #[serde(default)]
    pub dialog: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

/// How a dialog button looks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum ButtonStyle {
    #[default]
    Default,
    Primary,
    Danger,
}

/// A button of a dialog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Button {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub style: ButtonStyle,
    /// The action it runs; none just closes the dialog.
    #[serde(default)]
    pub action: Option<String>,
    /// Close the dialog after the action (default: yes for a button without an action).
    #[serde(default)]
    pub closes: Option<bool>,
}

/// A modal the plugin shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Dialog {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub fields: Vec<Field>,
    #[serde(default)]
    pub buttons: Vec<Button>,
}

/// A group of settings and actions in a plugin's own tab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Section {
    #[serde(default)]
    pub title: String,
    /// Keys of settings.
    #[serde(default)]
    pub fields: Vec<String>,
    /// Ids of actions.
    #[serde(default)]
    pub actions: Vec<String>,
}

/// A tab of its own in Settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Tab {
    pub title: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub sections: Vec<Section>,
}

/// The plugin's interface.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Ui {
    #[serde(default)]
    pub kind: UiKind,
    /// A name from the icons the app has; anything else shows the default one.
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub tab: Option<Tab>,
    #[serde(default)]
    pub actions: Vec<Action>,
    #[serde(default)]
    pub dialogs: Vec<Dialog>,
}
