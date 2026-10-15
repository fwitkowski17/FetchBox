//! REST routes (SPEC 6.5), mounted under `/api`.

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    middleware,
    routing::{get, patch, post},
    Json, Router,
};
use fetchbox_core::{
    AddRequest, Category, DiskSpace, Download, Error, Id, ProbeRequest, Settings, Stats,
    StatsRange, UrlInfo,
};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::auth::{self, Auth};
use crate::error::{ApiError, ConflictBody, ErrorBody, SpaceBody};
use crate::openapi;
use crate::service::Service;
use crate::sse;

type Svc = State<Arc<dyn Service>>;
type ApiResult<T> = Result<T, ApiError>;

/// Routes mounted under `/api`. Only `/health`, `/openapi.json` and `/auth/*` are public.
pub fn router(service: Arc<dyn Service>, auth: Arc<Auth>) -> Router {
    let protected = Router::new()
        .route("/downloads", get(list).post(add))
        .route("/downloads/{id}", patch(update).delete(remove))
        .route("/downloads/{id}/pause", post(pause))
        .route("/downloads/{id}/resume", post(resume))
        .route("/downloads/{id}/cancel", post(cancel))
        .route("/settings", get(get_settings).put(put_settings))
        .route("/probe", post(probe))
        .route("/disk", get(disk))
        .route("/stats", get(stats))
        .route("/events", get(sse::events))
        .with_state(service)
        .route_layer(middleware::from_fn_with_state(
            Arc::clone(&auth),
            auth::require_auth,
        ));

    Router::new()
        .route("/health", get(health))
        .route("/openapi.json", get(openapi::serve))
        .merge(auth::router(auth))
        .merge(protected)
}

/// Liveness probe.
#[derive(Debug, Serialize, ToSchema)]
pub struct Health {
    /// Always `"ok"`.
    pub status: String,
}

/// Body of `PATCH /downloads/{id}`.
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateDownload {
    /// New category (manual override of the detected one).
    pub category: Option<Category>,
    /// Hide the download from the main list (`true`) or bring it back (`false`). Only finished,
    /// failed or cancelled downloads can be archived (`409` otherwise).
    pub archived: Option<bool>,
    /// Speed of this download alone, bytes per second. It replaces the default speed of each
    /// download (it may be faster or slower); `0` removes it, so the default applies again. The
    /// total limit of all downloads still caps the sum. Only for queued, running, paused and
    /// failed downloads (`409` otherwise). Applied live to a running download.
    pub speed_limit: Option<u64>,
}

/// Liveness probe (public).
#[utoipa::path(
    get,
    path = "/api/health",
    tag = "system",
    responses((status = 200, body = Health))
)]
pub(crate) async fn health() -> Json<Health> {
    Json(Health {
        status: "ok".into(),
    })
}

/// All downloads, newest first. Running downloads carry live `downloaded` and `speed`.
#[utoipa::path(
    get,
    path = "/api/downloads",
    tag = "downloads",
    security(("session" = []), ("bearer" = [])),
    responses(
        (status = 200, body = Vec<Download>),
        (status = 401, description = "Not logged in", body = ErrorBody),
    )
)]
pub(crate) async fn list(State(svc): Svc) -> ApiResult<Json<Vec<Download>>> {
    Ok(Json(svc.list().await?))
}

/// Add a download by URL.
///
/// If the URL, the file name or a reserved name collides with something, the server answers
/// `409` with a `conflict` description and changes nothing. Re-send the request with
/// `on_conflict` set to `rename` (uses `conflict.suggested_name`) or `overwrite` (replaces the
/// existing file once the download finishes) to proceed. Not re-sending cancels the add.
#[utoipa::path(
    post,
    path = "/api/downloads",
    tag = "downloads",
    security(("session" = []), ("bearer" = [])),
    request_body = AddRequest,
    responses(
        (status = 201, description = "Download created (queued or started)", body = Download),
        (status = 400, description = "Invalid URL, or overwrite of a name another download is still producing", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 409, description = "Collision; the user must choose", body = ConflictBody),
        (status = 502, description = "The remote server could not be reached", body = ErrorBody),
        (status = 507, description = "The file does not fit into the free disk space that the running downloads have not claimed. Send the request again with `ignore_space` to add it anyway.", body = SpaceBody),
    )
)]
pub(crate) async fn add(
    State(svc): Svc,
    Json(req): Json<AddRequest>,
) -> ApiResult<(StatusCode, Json<Download>)> {
    Ok((StatusCode::CREATED, Json(svc.add(req).await?)))
}

/// Look at a URL without adding it: the file name, size, range support and category the server
/// and the file name suggest. The same checks as an add apply (private addresses can be refused).
/// A `POST` so that the address, which may carry a token, stays out of access logs.
#[utoipa::path(
    post,
    path = "/api/probe",
    tag = "downloads",
    security(("session" = []), ("bearer" = [])),
    request_body = ProbeRequest,
    responses(
        (status = 200, body = UrlInfo),
        (status = 400, description = "Not a usable http(s) URL", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 502, description = "The remote server could not be reached", body = ErrorBody),
    )
)]
pub(crate) async fn probe(
    State(svc): Svc,
    Json(req): Json<ProbeRequest>,
) -> ApiResult<Json<UrlInfo>> {
    Ok(Json(svc.probe(&req.url).await?))
}

/// Free space on the disk that holds the download directory, and how much of it the running
/// downloads still have to write.
#[utoipa::path(
    get,
    path = "/api/disk",
    tag = "system",
    security(("session" = []), ("bearer" = [])),
    responses(
        (status = 200, body = DiskSpace),
        (status = 401, description = "Not logged in", body = ErrorBody),
    )
)]
pub(crate) async fn disk(State(svc): Svc) -> ApiResult<Json<DiskSpace>> {
    Ok(Json(svc.disk_space().await?))
}

/// Query of `GET /stats`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct StatsQuery {
    /// How far back to look (default: everything).
    pub range: Option<StatsRange>,
    /// The caller's distance from UTC in minutes, east positive (`-new Date().getTimezoneOffset()`
    /// in a browser). It decides where a day begins. Default 0 (UTC).
    pub offset: Option<i32>,
}

/// A summary of the downloads of a period: how many finished, failed or were cancelled, the data
/// they fetched, the average and peak speed, the largest file, the categories and the last 14 days.
/// The numbers outlive the entries of the list: a download that is removed stays in them.
#[utoipa::path(
    get,
    path = "/api/stats",
    tag = "system",
    security(("session" = []), ("bearer" = [])),
    params(StatsQuery),
    responses(
        (status = 200, body = Stats),
        (status = 401, description = "Not logged in", body = ErrorBody),
    )
)]
pub(crate) async fn stats(State(svc): Svc, Query(q): Query<StatsQuery>) -> ApiResult<Json<Stats>> {
    Ok(Json(
        svc.stats(q.range.unwrap_or_default(), q.offset.unwrap_or(0))
            .await?,
    ))
}

/// Change the category of a download, archive it (hide it from the main list) and/or set its own
/// speed limit.
#[utoipa::path(
    patch,
    path = "/api/downloads/{id}",
    tag = "downloads",
    security(("session" = []), ("bearer" = [])),
    params(("id" = i64, Path, description = "Download id")),
    request_body = UpdateDownload,
    responses(
        (status = 200, body = Download),
        (status = 400, description = "Nothing to update", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "Unknown id", body = ErrorBody),
        (status = 409, description = "Not possible in the download's current status (archiving needs a finished one, a speed limit an unfinished or failed one)", body = ErrorBody),
    )
)]
pub(crate) async fn update(
    State(svc): Svc,
    Path(id): Path<Id>,
    Json(body): Json<UpdateDownload>,
) -> ApiResult<Json<Download>> {
    if body.category.is_none() && body.archived.is_none() && body.speed_limit.is_none() {
        return Err(Error::InvalidInput("nothing to update".into()).into());
    }
    let mut updated = None;
    if let Some(rate) = body.speed_limit {
        updated = Some(svc.set_speed_limit(id, rate).await?);
    }
    if let Some(category) = body.category {
        updated = Some(svc.set_category(id, category).await?);
    }
    if let Some(archived) = body.archived {
        updated = Some(svc.set_archived(id, archived).await?);
    }
    updated.map(Json).ok_or_else(|| Error::NotFound(id).into())
}

/// Query of `DELETE /downloads/{id}`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct RemoveQuery {
    /// Also delete the finished file from the disk. Ignored for downloads that have not
    /// completed (their partial file is always removed).
    #[serde(default)]
    pub delete_file: bool,
}

/// Remove a download from the list (cancels it first when still active).
/// A finished file stays on disk unless `delete_file=true`.
#[utoipa::path(
    delete,
    path = "/api/downloads/{id}",
    tag = "downloads",
    security(("session" = []), ("bearer" = [])),
    params(("id" = i64, Path, description = "Download id"), RemoveQuery),
    responses(
        (status = 204, description = "Removed"),
        (status = 500, description = "The file could not be deleted; the entry is kept so it can be retried", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "Unknown id", body = ErrorBody),
    )
)]
pub(crate) async fn remove(
    State(svc): Svc,
    Path(id): Path<Id>,
    Query(q): Query<RemoveQuery>,
) -> ApiResult<StatusCode> {
    svc.remove(id, q.delete_file).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Pause a queued or running download; offsets are saved so it can resume.
#[utoipa::path(
    post,
    path = "/api/downloads/{id}/pause",
    tag = "downloads",
    security(("session" = []), ("bearer" = [])),
    params(("id" = i64, Path, description = "Download id")),
    responses(
        (status = 200, body = Download),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "Unknown id", body = ErrorBody),
        (status = 409, description = "Not queued or downloading", body = ErrorBody),
    )
)]
pub(crate) async fn pause(State(svc): Svc, Path(id): Path<Id>) -> ApiResult<Json<Download>> {
    Ok(Json(svc.pause(id).await?))
}

/// Resume a paused (or retry a failed) download.
#[utoipa::path(
    post,
    path = "/api/downloads/{id}/resume",
    tag = "downloads",
    security(("session" = []), ("bearer" = [])),
    params(("id" = i64, Path, description = "Download id")),
    responses(
        (status = 200, body = Download),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "Unknown id", body = ErrorBody),
        (status = 409, description = "Not paused or failed", body = ErrorBody),
    )
)]
pub(crate) async fn resume(State(svc): Svc, Path(id): Path<Id>) -> ApiResult<Json<Download>> {
    Ok(Json(svc.resume(id).await?))
}

/// Cancel a download and delete its partial file.
#[utoipa::path(
    post,
    path = "/api/downloads/{id}/cancel",
    tag = "downloads",
    security(("session" = []), ("bearer" = [])),
    params(("id" = i64, Path, description = "Download id")),
    responses(
        (status = 200, body = Download),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "Unknown id", body = ErrorBody),
        (status = 409, description = "Already finished", body = ErrorBody),
    )
)]
pub(crate) async fn cancel(State(svc): Svc, Path(id): Path<Id>) -> ApiResult<Json<Download>> {
    Ok(Json(svc.cancel(id).await?))
}

/// Current settings.
#[utoipa::path(
    get,
    path = "/api/settings",
    tag = "settings",
    security(("session" = []), ("bearer" = [])),
    responses(
        (status = 200, body = Settings),
        (status = 401, description = "Not logged in", body = ErrorBody),
    )
)]
pub(crate) async fn get_settings(State(svc): Svc) -> ApiResult<Json<Settings>> {
    Ok(Json(svc.settings().await?))
}

/// Replace the settings. Speed limits and `max_concurrent` apply to running downloads
/// immediately; `connections` applies to downloads started afterwards. `0` speed = unlimited.
#[utoipa::path(
    put,
    path = "/api/settings",
    tag = "settings",
    security(("session" = []), ("bearer" = [])),
    request_body = Settings,
    responses(
        (status = 200, body = Settings),
        (status = 400, description = "Invalid values", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
    )
)]
pub(crate) async fn put_settings(
    State(svc): Svc,
    Json(s): Json<Settings>,
) -> ApiResult<Json<Settings>> {
    Ok(Json(svc.update_settings(s).await?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn health_is_ok() {
        let Json(v) = health().await;
        assert_eq!(v.status, "ok");
    }
}
