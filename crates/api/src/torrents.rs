//! BitTorrent routes (SPEC 6.13): a list of their own, apart from downloads.

use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use fetchbox_core::{AddTorrentRequest, Id, Torrent, TorrentFile, TorrentPeers, MAX_TORRENT_FILE};
use serde::Deserialize;
use std::sync::Arc;
use utoipa::IntoParams;

use crate::error::{ApiError, ErrorBody, SpaceBody};
use crate::service::Service;

type Svc = State<Arc<dyn Service>>;
type ApiResult<T> = Result<T, ApiError>;

/// The torrent routes, to be merged into the protected router.
pub(crate) fn routes() -> Router<Arc<dyn Service>> {
    Router::new()
        .route("/torrents", get(list).post(add_magnet))
        .route(
            "/torrents/file",
            // A real `.torrent` file is a few tens of kilobytes; the default 2 MiB body limit
            // would refuse a big one, and no limit would accept anything.
            post(add_file).layer(DefaultBodyLimit::max(MAX_TORRENT_FILE + 1024)),
        )
        .route("/torrents/{id}", axum::routing::delete(remove))
        .route("/torrents/{id}/pause", post(pause))
        .route("/torrents/{id}/resume", post(resume))
        .route("/torrents/{id}/peers", get(peers))
        .route("/torrents/{id}/files", get(files))
}

/// All torrents, newest first. Live numbers (speed, peers, progress) come as `torrent` events on
/// `/api/events`; this is the state to start from.
#[utoipa::path(
    get,
    path = "/api/torrents",
    tag = "torrents",
    security(("session" = []), ("bearer" = [])),
    responses(
        (status = 200, body = Vec<Torrent>),
        (status = 401, description = "Not logged in", body = ErrorBody),
    )
)]
pub(crate) async fn list(State(svc): Svc) -> ApiResult<Json<Vec<Torrent>>> {
    Ok(Json(svc.torrents().await?))
}

/// Add a torrent by magnet link.
///
/// The torrent shows up at once with the status `metadata`; its name and files arrive from the
/// swarm, which can take a while (or never happen when nobody has it).
#[utoipa::path(
    post,
    path = "/api/torrents",
    tag = "torrents",
    security(("session" = []), ("bearer" = [])),
    request_body = AddTorrentRequest,
    responses(
        (status = 201, description = "Added", body = Torrent),
        (status = 400, description = "Not a magnet link, or a tracker at a private address while those are blocked", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 409, description = "The torrent has been added already", body = ErrorBody),
        (status = 502, description = "The BitTorrent engine could not start (the port is in use, ...)", body = ErrorBody),
    )
)]
pub(crate) async fn add_magnet(
    State(svc): Svc,
    Json(req): Json<AddTorrentRequest>,
) -> ApiResult<(StatusCode, Json<Torrent>)> {
    Ok((
        StatusCode::CREATED,
        Json(svc.add_torrent_magnet(req).await?),
    ))
}

/// Add a torrent from a `.torrent` file: the raw bytes of the file as the request body
/// (`Content-Type: application/x-bittorrent`), at most 8 MiB.
#[utoipa::path(
    post,
    path = "/api/torrents/file",
    tag = "torrents",
    security(("session" = []), ("bearer" = [])),
    request_body(content = Vec<u8>, content_type = "application/x-bittorrent"),
    responses(
        (status = 201, description = "Added", body = Torrent),
        (status = 400, description = "Not a usable .torrent file, or a tracker at a private address while those are blocked", body = ErrorBody),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 409, description = "The torrent has been added already", body = ErrorBody),
        (status = 413, description = "The file is too large"),
        (status = 502, description = "The BitTorrent engine could not start", body = ErrorBody),
        (status = 507, description = "The files of the torrent do not fit into the free disk space", body = SpaceBody),
    )
)]
pub(crate) async fn add_file(
    State(svc): Svc,
    body: Bytes,
) -> ApiResult<(StatusCode, Json<Torrent>)> {
    Ok((
        StatusCode::CREATED,
        Json(svc.add_torrent_file(body.to_vec()).await?),
    ))
}

/// Stop a torrent. What it has stays on disk; it stops sending to others too.
#[utoipa::path(
    post,
    path = "/api/torrents/{id}/pause",
    tag = "torrents",
    security(("session" = []), ("bearer" = [])),
    params(("id" = i64, Path, description = "Torrent id")),
    responses(
        (status = 200, body = Torrent),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "Unknown id", body = ErrorBody),
    )
)]
pub(crate) async fn pause(State(svc): Svc, Path(id): Path<Id>) -> ApiResult<Json<Torrent>> {
    Ok(Json(svc.pause_torrent(id).await?))
}

/// Start a stopped torrent (or try again after an error).
#[utoipa::path(
    post,
    path = "/api/torrents/{id}/resume",
    tag = "torrents",
    security(("session" = []), ("bearer" = [])),
    params(("id" = i64, Path, description = "Torrent id")),
    responses(
        (status = 200, body = Torrent),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "Unknown id", body = ErrorBody),
        (status = 507, description = "What is left does not fit into the free disk space", body = SpaceBody),
    )
)]
pub(crate) async fn resume(State(svc): Svc, Path(id): Path<Id>) -> ApiResult<Json<Torrent>> {
    Ok(Json(svc.resume_torrent(id).await?))
}

/// Query of `DELETE /torrents/{id}`.
#[derive(Debug, Deserialize, IntoParams)]
pub struct RemoveTorrentQuery {
    /// Also delete the downloaded files from the disk.
    #[serde(default)]
    pub delete_files: bool,
}

/// Remove a torrent from the list; with `delete_files=true` its data goes too.
#[utoipa::path(
    delete,
    path = "/api/torrents/{id}",
    tag = "torrents",
    security(("session" = []), ("bearer" = [])),
    params(("id" = i64, Path, description = "Torrent id"), RemoveTorrentQuery),
    responses(
        (status = 204, description = "Removed"),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "Unknown id", body = ErrorBody),
    )
)]
pub(crate) async fn remove(
    State(svc): Svc,
    Path(id): Path<Id>,
    Query(q): Query<RemoveTorrentQuery>,
) -> ApiResult<StatusCode> {
    svc.remove_torrent(id, q.delete_files).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The peers of a torrent, with the speed of each. Poll it while a panel is open; it is not part
/// of the event stream. The list is empty while the torrent is stopped.
#[utoipa::path(
    get,
    path = "/api/torrents/{id}/peers",
    tag = "torrents",
    security(("session" = []), ("bearer" = [])),
    params(("id" = i64, Path, description = "Torrent id")),
    responses(
        (status = 200, body = TorrentPeers),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "Unknown id", body = ErrorBody),
    )
)]
pub(crate) async fn peers(State(svc): Svc, Path(id): Path<Id>) -> ApiResult<Json<TorrentPeers>> {
    Ok(Json(svc.torrent_peers(id).await?))
}

/// The files of a torrent and how much of each is on disk. Empty until the description of the
/// torrent has arrived.
#[utoipa::path(
    get,
    path = "/api/torrents/{id}/files",
    tag = "torrents",
    security(("session" = []), ("bearer" = [])),
    params(("id" = i64, Path, description = "Torrent id")),
    responses(
        (status = 200, body = Vec<TorrentFile>),
        (status = 401, description = "Not logged in", body = ErrorBody),
        (status = 404, description = "Unknown id", body = ErrorBody),
    )
)]
pub(crate) async fn files(
    State(svc): Svc,
    Path(id): Path<Id>,
) -> ApiResult<Json<Vec<TorrentFile>>> {
    Ok(Json(svc.torrent_files(id).await?))
}
