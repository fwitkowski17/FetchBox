//! The plugin system's vocabulary (SPEC 6.15): the manifest a plugin ships (`plugin.json`), what the
//! interface is shown of an installed plugin, and the pure rules around them (validation, argument
//! templates, URL globs, translations). Nothing here starts a process or touches the disk; the
//! `plugins` crate and the server do that.

mod manifest;
mod programs;
mod text;
mod validate;
mod view;

pub mod template;

pub use manifest::{
    Action, ActionKind, ArchiveKind, Arg, Builtin, Button, ButtonStyle, Choice, Command, DepSource,
    Dependency, Dialog, Field, FieldKind, FieldOption, GithubDependency, Hook, HookAction, ItemMap,
    Manifest, PermissionGroup, Permissions, Placement, Pointers, ProcessSpec, Resolver,
    RestartPolicy, ResultMap, Section, SettingValue, ShowIf, SingleMap, Tab, Ui, UiKind, Webhook,
    WebhookBody,
};
pub use programs::{is_valid_version, shared_program, shared_programs, SharedProgram};
pub use text::{
    glob_match, host_of, is_safe_relative, is_valid_id, is_valid_key, platform_key, tr, PLATFORMS,
};
pub use validate::{check_setting_value, event_matches, validate, EVENT_NAMES, MAX_TEXT_SETTING};
pub use view::{
    derive_permissions, used_groups, ActionResult, ActionView, DependencyNeed, DependencyPlan,
    DependencyProgress, DependencySource, DependencyView, FieldView, NoticeLevel,
    PermissionGroupView, PermissionSummary, PluginList, PluginSource, PluginState, PluginView,
    Provides, StagedDependency, StagedPlugin, StagedReplaces, StoreDownload, StoreEntry, StoreList,
    UiView,
};

/// The manifest format this build understands (`Manifest::api_version`).
pub const API_VERSION: u32 = 1;
/// Largest `plugin.json` that is read.
pub const MAX_MANIFEST_BYTES: usize = 256 * 1024;
/// Largest plugin ZIP that is accepted (the request body limit of the stage route).
pub const MAX_PLUGIN_ZIP: usize = 32 * 1024 * 1024;
