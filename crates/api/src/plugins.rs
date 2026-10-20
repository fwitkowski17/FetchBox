//! Plugin routes (SPEC 6.15): installing, switching on and off, settings and actions.
//!
//! A plugin runs programs, so every route here is behind the session like the rest of the API, the
//! bodies have an exact content type (`application/json`, `application/zip`; a form post from
//! another site cannot send either without the browser asking first), and an action is named, never
//! given: the browser says which button was pressed, the server looks up what it does.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    routing::{delete, get, patch, post, put},
    Json, Router,
};
use fetchbox_core::plugin::{
    ActionResult, PluginList, PluginView, SettingValue, StagedPlugin, StoreList, MAX_PLUGIN_ZIP,
};
use fetchbox_core::Error;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::error::{ApiError, ErrorBody};
use crate::service::Service;

type Svc = State<Arc<dyn Service>>;
type ApiResult<T> = Result<T, ApiError>;

/// The plugin routes, to be merged into the protected router.
pub(crate) fn routes() -> Router<Arc<dyn Service>> {
    Router::new()
        .route("/plugins", get(list))
        .route("/plugin-rescan", post(rescan))
        .route(
            "/plugin-staging",
            post(stage).layer(DefaultBodyLimit::max(MAX_PLUGIN_ZIP + 1024)),
        )
        .route("/plugin-staging/store", post(stage_store))
        .route("/plugin-staging/{token}", delete(cancel))
        .route("/plugin-store", get(store))
        .route("/plugin-store/refresh", post(store_refresh))
        .route("/plugin-staging/{token}/install", post(install))
        .route("/plugins/{id}", patch(set_enabled).delete(remove))
        .route("/plugins/{id}/approve", post(approve))
        .route("/plugins/{id}/settings", put(save_settings))
        .route("/plugins/{id}/actions/{action}", post(run_action))
        .route("/plugins/{id}/restart", post(restart))
        .route("/plugins/{id}/dependencies", post(dependencies))
        .route("/plugins/{id}/logs", get(logs))
}

/// The installed plugins, and whether the plugin system is on. Changes arrive as `plugin` and
/// `plugin_removed` events on `/api/events`; this is the state to start from.
#[utoipa::path(
    get,
    path = "/api/plugins",
    tag = "plugins",
    security(("session" = []), ("bearer" = [])),
    responses(
        (status = 200, body = PluginList),
        (status = 401, description = "Not logged in", body = ErrorBody),
    )
)]
pub(crate) async fn list(State(svc): Svc) -> ApiResult<Json<PluginList>> {
    Ok(Json(svc.plugins().await?))
}

/// Look at the plugins directory again: a folder somebody put there appears as a plugin that needs
/// review, a folder that is gone takes its plugin with it, a changed manifest needs approval again.
#[utoipa::path(
    post,
    path = "/api/plugin-rescan",
    tag = "plugins",
    security(("session" = []), ("bearer" = [])),
    responses(
        (status = 200, body = PluginList),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 409, description = "Plugins are switched off", body = ErrorBody),
    )
)]
pub(crate) async fn rescan(State(svc): Svc) -> ApiResult<Json<PluginList>> {
    Ok(Json(svc.plugin_rescan().await?))
}

/// Upload a plugin: the raw bytes of its ZIP file (`Content-Type: application/zip`, at most 32 MiB).
///
/// The package is unpacked and its manifest read, nothing of it runs. The answer says what the
/// plugin may do, for the user to review; `POST /plugin-staging/{token}/install` installs it and
/// `DELETE /plugin-staging/{token}` throws it away.
#[utoipa::path(
    post,
    path = "/api/plugin-staging",
    tag = "plugins",
    security(("session" = []), ("bearer" = [])),
    request_body(content = Vec<u8>, content_type = "application/zip"),
    responses(
        (status = 201, description = "Unpacked and read, waiting for the user's decision", body = StagedPlugin),
        (status = 400, description = "Not a usable plugin: not sent as application/zip, not a ZIP, an unsafe path in it, no valid plugin.json, or it cannot run here", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 409, description = "Plugins are switched off", body = ErrorBody),
        (status = 413, description = "The file is too large"),
    )
)]
pub(crate) async fn stage(
    State(svc): Svc,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<(StatusCode, Json<StagedPlugin>)> {
    let ok = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|t| {
            let t = t
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            t == "application/zip" || t == "application/x-zip-compressed"
        });
    if !ok {
        return Err(ApiError(Error::InvalidInput(
            "send the ZIP file with Content-Type: application/zip".into(),
        )));
    }
    Ok((
        StatusCode::CREATED,
        Json(svc.plugin_stage(body.to_vec()).await?),
    ))
}

/// What can be installed from the store: the plugins that ship with FetchBox (they work offline) and
/// the remote catalog, one entry per plugin, marked with what is installed and what has an update.
/// The catalog is fetched when it is stale; if that fails the answer still lists the bundled plugins,
/// with the reason in `error`.
#[utoipa::path(
    get,
    path = "/api/plugin-store",
    tag = "plugins",
    security(("session" = []), ("bearer" = [])),
    responses(
        (status = 200, body = StoreList),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 409, description = "Plugins are switched off", body = ErrorBody),
    )
)]
pub(crate) async fn store(State(svc): Svc) -> ApiResult<Json<StoreList>> {
    Ok(Json(svc.plugin_store(false).await?))
}

/// Fetch the remote catalog again now (not more often than every 30 seconds).
#[utoipa::path(
    post,
    path = "/api/plugin-store/refresh",
    tag = "plugins",
    security(("session" = []), ("bearer" = [])),
    responses(
        (status = 200, body = StoreList),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 409, description = "Plugins are switched off", body = ErrorBody),
    )
)]
pub(crate) async fn store_refresh(State(svc): Svc) -> ApiResult<Json<StoreList>> {
    Ok(Json(svc.plugin_store(true).await?))
}

/// Body of `POST /plugin-staging/store`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct StageFromStore {
    /// The plugin's id, from `GET /plugin-store`.
    pub id: String,
}

/// Unpack a plugin of the store, like an upload: the answer says what it may do, for the user to
/// review; then install it with `POST /plugin-staging/{token}/install`. A plugin from the remote
/// catalog is downloaded and checked against its checksum first.
#[utoipa::path(
    post,
    path = "/api/plugin-staging/store",
    tag = "plugins",
    security(("session" = []), ("bearer" = [])),
    request_body = StageFromStore,
    responses(
        (status = 201, description = "Unpacked and read, waiting for the user's decision", body = StagedPlugin),
        (status = 400, description = "It cannot run here, or the package is not the plugin the catalog named", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "The store has no such plugin", body = ErrorBody),
        (status = 409, description = "Plugins are switched off", body = ErrorBody),
        (status = 502, description = "The download failed or does not match its checksum", body = ErrorBody),
    )
)]
pub(crate) async fn stage_store(
    State(svc): Svc,
    Json(req): Json<StageFromStore>,
) -> ApiResult<(StatusCode, Json<StagedPlugin>)> {
    Ok((
        StatusCode::CREATED,
        Json(svc.plugin_stage_store(req.id).await?),
    ))
}

/// Install a plugin that was uploaded. A new plugin starts switched off; an update of an installed
/// plugin keeps its settings and whether it is on.
#[utoipa::path(
    post,
    path = "/api/plugin-staging/{token}/install",
    tag = "plugins",
    security(("session" = []), ("bearer" = [])),
    params(("token" = String, Path, description = "From the upload")),
    responses(
        (status = 201, description = "Installed", body = PluginView),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "No such package (it is kept for 15 minutes)", body = ErrorBody),
        (status = 409, description = "Plugins are switched off", body = ErrorBody),
    )
)]
pub(crate) async fn install(
    State(svc): Svc,
    Path(token): Path<String>,
) -> ApiResult<(StatusCode, Json<PluginView>)> {
    Ok((StatusCode::CREATED, Json(svc.plugin_install(token).await?)))
}

/// Throw an uploaded plugin away.
#[utoipa::path(
    delete,
    path = "/api/plugin-staging/{token}",
    tag = "plugins",
    security(("session" = []), ("bearer" = [])),
    params(("token" = String, Path, description = "From the upload")),
    responses(
        (status = 204, description = "Gone"),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 409, description = "Plugins are switched off", body = ErrorBody),
    )
)]
pub(crate) async fn cancel(State(svc): Svc, Path(token): Path<String>) -> ApiResult<StatusCode> {
    svc.plugin_cancel(token).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Body of `PATCH /plugins/{id}`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdatePlugin {
    /// Switch the plugin on (its dependencies are fetched, its program started) or off.
    pub enabled: bool,
}

/// Switch a plugin on or off. Switching on needs the required settings to be set and the plugin
/// not to be waiting for a review.
#[utoipa::path(
    patch,
    path = "/api/plugins/{id}",
    tag = "plugins",
    security(("session" = []), ("bearer" = [])),
    params(("id" = String, Path, description = "Plugin id")),
    request_body = UpdatePlugin,
    responses(
        (status = 200, body = PluginView),
        (status = 400, description = "A required setting has no value", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "Unknown plugin", body = ErrorBody),
        (status = 409, description = "It needs a review first, cannot run here, or plugins are switched off", body = ErrorBody),
    )
)]
pub(crate) async fn set_enabled(
    State(svc): Svc,
    Path(id): Path<String>,
    Json(req): Json<UpdatePlugin>,
) -> ApiResult<Json<PluginView>> {
    Ok(Json(svc.plugin_set_enabled(id, req.enabled).await?))
}

/// Query of `DELETE /plugins/{id}`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct RemoveQuery {
    /// Keep what the plugin wrote (its downloaded dependencies and state). Default: delete it.
    #[serde(default)]
    pub keep_data: bool,
}

/// Uninstall a plugin: its program stops, its files and settings go.
#[utoipa::path(
    delete,
    path = "/api/plugins/{id}",
    tag = "plugins",
    security(("session" = []), ("bearer" = [])),
    params(("id" = String, Path, description = "Plugin id"), RemoveQuery),
    responses(
        (status = 204, description = "Removed"),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "Unknown plugin", body = ErrorBody),
        (status = 409, description = "Plugins are switched off", body = ErrorBody),
    )
)]
pub(crate) async fn remove(
    State(svc): State<Arc<dyn Service>>,
    Path(id): Path<String>,
    Query(q): Query<RemoveQuery>,
) -> ApiResult<StatusCode> {
    svc.plugin_remove(id, q.keep_data).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Body of `POST /plugins/{id}/approve`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ApprovePlugin {
    /// The `manifest_hash` the user was shown: the approval is for exactly those files.
    pub hash: String,
}

/// Accept the files a plugin has on disk now, for a plugin that is waiting for a review (a folder
/// somebody put in the plugins directory, or files that changed).
#[utoipa::path(
    post,
    path = "/api/plugins/{id}/approve",
    tag = "plugins",
    security(("session" = []), ("bearer" = [])),
    params(("id" = String, Path, description = "Plugin id")),
    request_body = ApprovePlugin,
    responses(
        (status = 200, body = PluginView),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "Unknown plugin", body = ErrorBody),
        (status = 409, description = "The files changed since the user looked, or plugins are switched off", body = ErrorBody),
    )
)]
pub(crate) async fn approve(
    State(svc): Svc,
    Path(id): Path<String>,
    Json(req): Json<ApprovePlugin>,
) -> ApiResult<Json<PluginView>> {
    Ok(Json(svc.plugin_approve(id, req.hash).await?))
}

/// Body of `PUT /plugins/{id}/settings`.
#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct SavePluginSettings {
    /// Settings to store, by key. A key that is left out stays as it is.
    #[serde(default)]
    pub values: BTreeMap<String, SettingValue>,
    /// Settings to clear (back to the default, or no value for a secret).
    #[serde(default)]
    pub clear: Vec<String>,
}

/// Change settings of a plugin. Every value is checked against its field; nothing is stored if one
/// is refused. A running program is told (`settings_changed`).
#[utoipa::path(
    put,
    path = "/api/plugins/{id}/settings",
    tag = "plugins",
    security(("session" = []), ("bearer" = [])),
    params(("id" = String, Path, description = "Plugin id")),
    request_body = SavePluginSettings,
    responses(
        (status = 200, body = PluginView),
        (status = 400, description = "Not a setting of this plugin, or a value its field does not accept", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "Unknown plugin", body = ErrorBody),
        (status = 409, description = "Plugins are switched off", body = ErrorBody),
    )
)]
pub(crate) async fn save_settings(
    State(svc): Svc,
    Path(id): Path<String>,
    Json(req): Json<SavePluginSettings>,
) -> ApiResult<Json<PluginView>> {
    let mut changes: BTreeMap<String, Option<SettingValue>> =
        req.clear.into_iter().map(|k| (k, None)).collect();
    changes.extend(req.values.into_iter().map(|(k, v)| (k, Some(v))));
    Ok(Json(svc.plugin_save_settings(id, changes).await?))
}

/// Body of `POST /plugins/{id}/actions/{action}`.
#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct RunPluginAction {
    /// The values of the fields of the dialog the button is in, by key.
    #[serde(default)]
    pub input: BTreeMap<String, SettingValue>,
}

/// Press a button of a plugin: the action is looked up in its manifest and done.
#[utoipa::path(
    post,
    path = "/api/plugins/{id}/actions/{action}",
    tag = "plugins",
    security(("session" = []), ("bearer" = [])),
    params(
        ("id" = String, Path, description = "Plugin id"),
        ("action" = String, Path, description = "Action id from the plugin's manifest"),
    ),
    request_body = RunPluginAction,
    responses(
        (status = 200, body = ActionResult),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "Unknown plugin or action", body = ErrorBody),
        (status = 409, description = "The plugin is not running, or plugins are switched off", body = ErrorBody),
        (status = 502, description = "The command or the request failed; the message says how", body = ErrorBody),
    )
)]
pub(crate) async fn run_action(
    State(svc): Svc,
    Path((id, action)): Path<(String, String)>,
    Json(req): Json<RunPluginAction>,
) -> ApiResult<Json<ActionResult>> {
    Ok(Json(svc.plugin_action(id, action, req.input).await?))
}

/// Start the plugin's program again, also after it gave up.
#[utoipa::path(
    post,
    path = "/api/plugins/{id}/restart",
    tag = "plugins",
    security(("session" = []), ("bearer" = [])),
    params(("id" = String, Path, description = "Plugin id")),
    responses(
        (status = 200, body = PluginView),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "Unknown plugin", body = ErrorBody),
        (status = 409, description = "The plugin is not switched on, or plugins are switched off", body = ErrorBody),
    )
)]
pub(crate) async fn restart(
    State(svc): Svc,
    Path(id): Path<String>,
) -> ApiResult<Json<PluginView>> {
    Ok(Json(svc.plugin_restart(id).await?))
}

/// Body of `POST /plugins/{id}/dependencies`.
#[derive(Debug, Default, Deserialize, ToSchema)]
pub struct FetchDependencies {
    /// One dependency (an optional one is installed this way); without it every dependency that is
    /// installed or required is fetched again, for its newest version.
    #[serde(default)]
    pub dependency: Option<String>,
}

/// Fetch a plugin's dependencies (again). The plugin's program restarts on the new files.
#[utoipa::path(
    post,
    path = "/api/plugins/{id}/dependencies",
    tag = "plugins",
    security(("session" = []), ("bearer" = [])),
    params(("id" = String, Path, description = "Plugin id")),
    request_body = FetchDependencies,
    responses(
        (status = 200, description = "Started; progress arrives as plugin events", body = PluginView),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "Unknown plugin or dependency", body = ErrorBody),
        (status = 409, description = "The plugin is not switched on, or plugins are switched off", body = ErrorBody),
    )
)]
pub(crate) async fn dependencies(
    State(svc): Svc,
    Path(id): Path<String>,
    Json(req): Json<FetchDependencies>,
) -> ApiResult<Json<PluginView>> {
    Ok(Json(svc.plugin_dependencies(id, req.dependency).await?))
}

/// What a plugin's program has logged.
#[derive(Debug, Serialize, ToSchema)]
pub struct PluginLogs {
    /// The last lines, oldest first.
    pub lines: Vec<String>,
}

/// The last lines the plugin's program wrote to its log.
#[utoipa::path(
    get,
    path = "/api/plugins/{id}/logs",
    tag = "plugins",
    security(("session" = []), ("bearer" = [])),
    params(("id" = String, Path, description = "Plugin id")),
    responses(
        (status = 200, body = PluginLogs),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "Unknown plugin", body = ErrorBody),
        (status = 409, description = "Plugins are switched off", body = ErrorBody),
    )
)]
pub(crate) async fn logs(State(svc): Svc, Path(id): Path<String>) -> ApiResult<Json<PluginLogs>> {
    Ok(Json(PluginLogs {
        lines: svc.plugin_logs(id).await?,
    }))
}
