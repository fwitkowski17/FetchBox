//! The plugin system's vocabulary (SPEC 6.15): the manifest a plugin ships (`plugin.json`), what the
//! interface is shown of an installed plugin, and the pure rules around them (validation, argument
//! templates, URL globs, translations). Nothing here starts a process or touches the disk; the
//! `plugins` crate and the server do that.

mod manifest;
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
pub use text::{
    glob_match, host_of, is_safe_relative, is_valid_id, is_valid_key, platform_key, tr, PLATFORMS,
};
pub use validate::{check_setting_value, event_matches, validate, EVENT_NAMES, MAX_TEXT_SETTING};
pub use view::{
    derive_permissions, used_groups, ActionResult, ActionView, CompatWarning, DependencyNeed,
    DependencyPlan,
    DependencyProgress, DependencySource, DependencyView, FieldView, NoticeLevel,
    PermissionGroupView, PermissionSummary, PluginList, PluginSource, PluginState, PluginView,
    Provides, StagedDependency, StagedPlugin, StagedReplaces, StoreDownload, StoreEntry, StoreList,
    UiView,
};

/// The manifest format this build understands (`Manifest::api_version`). 2 removed the `shared`
/// form of dependencies.
pub const API_VERSION: u32 = 2;
/// The oldest manifest format this build still runs. Between it and [`API_VERSION`] a plugin runs
/// with a warning that it was made for an older FetchBox; below it, it is not supported.
pub const MIN_API_VERSION: u32 = 1;

/// How the plugin API a manifest was written for compares with this build's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiCompat {
    Current,
    /// Older, still run, with a warning.
    Older,
    /// Older than [`MIN_API_VERSION`]: not supported.
    TooOld,
}

/// [`ApiCompat`] of a manifest's `api_version` (newer ones are refused by `validate`).
pub fn api_compat(api_version: u32) -> ApiCompat {
    api_compat_in(api_version, MIN_API_VERSION, API_VERSION)
}

fn api_compat_in(api_version: u32, min: u32, current: u32) -> ApiCompat {
    if api_version >= current {
        ApiCompat::Current
    } else if api_version >= min {
        ApiCompat::Older
    } else {
        ApiCompat::TooOld
    }
}

/// The warning shown on a plugin made for an older, still supported plugin API.
pub fn compat_warning(api_version: u32) -> Option<CompatWarning> {
    (api_compat(api_version) == ApiCompat::Older).then_some(CompatWarning {
        api_version,
        current: API_VERSION,
    })
}
/// Largest `plugin.json` that is read.
pub const MAX_MANIFEST_BYTES: usize = 256 * 1024;
/// Largest plugin ZIP that is accepted (the request body limit of the stage route).
pub const MAX_PLUGIN_ZIP: usize = 32 * 1024 * 1024;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_compat_tells_current_older_and_too_old_apart() {
        assert_eq!(api_compat_in(4, 2, 4), ApiCompat::Current);
        assert_eq!(api_compat_in(3, 2, 4), ApiCompat::Older);
        assert_eq!(api_compat_in(2, 2, 4), ApiCompat::Older);
        assert_eq!(api_compat_in(1, 2, 4), ApiCompat::TooOld);
        assert_eq!(api_compat(API_VERSION), ApiCompat::Current);
    }

    #[test]
    fn only_an_older_api_gets_a_warning() {
        assert_eq!(compat_warning(API_VERSION), None);
        assert_eq!(
            compat_warning(1),
            Some(CompatWarning {
                api_version: 1,
                current: 2
            })
        );
    }
}
