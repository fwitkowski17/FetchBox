//! REST routes (SPEC 6.5), mounted under `/api`.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    middleware,
    routing::{get, patch, post},
    Json, Router,
};
use dlm_core::{AddRequest, Category, Download, Error, Id, Settings};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::{self, Auth};
use crate::error::ApiError;
use crate::service::Service;
use crate::sse;

type Svc = State<Arc<dyn Service>>;
type ApiResult<T> = Result<T, ApiError>;

/// Routes mounted under `/api`. Only `/health` and `/auth/*` are public.
pub fn router(service: Arc<dyn Service>, auth: Arc<Auth>) -> Router {
    let protected = Router::new()
        .route("/downloads", get(list).post(add))
        .route("/downloads/{id}", patch(update).delete(remove))
        .route("/downloads/{id}/pause", post(pause))
        .route("/downloads/{id}/resume", post(resume))
        .route("/downloads/{id}/cancel", post(cancel))
        .route("/settings", get(get_settings).put(put_settings))
        .route("/events", get(sse::events))
        .with_state(service)
        .route_layer(middleware::from_fn_with_state(
            Arc::clone(&auth),
            auth::require_auth,
        ));

    Router::new()
        .route("/health", get(health))
        .merge(auth::router(auth))
        .merge(protected)
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

async fn list(State(svc): Svc) -> ApiResult<Json<Vec<Download>>> {
    Ok(Json(svc.list().await?))
}

async fn add(
    State(svc): Svc,
    Json(req): Json<AddRequest>,
) -> ApiResult<(StatusCode, Json<Download>)> {
    Ok((StatusCode::CREATED, Json(svc.add(req).await?)))
}

#[derive(Debug, Deserialize)]
struct UpdateDownload {
    category: Option<Category>,
}

async fn update(
    State(svc): Svc,
    Path(id): Path<Id>,
    Json(body): Json<UpdateDownload>,
) -> ApiResult<Json<Download>> {
    let category = body
        .category
        .ok_or_else(|| Error::InvalidInput("nothing to update".into()))?;
    Ok(Json(svc.set_category(id, category).await?))
}

async fn remove(State(svc): Svc, Path(id): Path<Id>) -> ApiResult<StatusCode> {
    svc.remove(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn pause(State(svc): Svc, Path(id): Path<Id>) -> ApiResult<Json<Download>> {
    Ok(Json(svc.pause(id).await?))
}

async fn resume(State(svc): Svc, Path(id): Path<Id>) -> ApiResult<Json<Download>> {
    Ok(Json(svc.resume(id).await?))
}

async fn cancel(State(svc): Svc, Path(id): Path<Id>) -> ApiResult<Json<Download>> {
    Ok(Json(svc.cancel(id).await?))
}

async fn get_settings(State(svc): Svc) -> ApiResult<Json<Settings>> {
    Ok(Json(svc.settings().await?))
}

async fn put_settings(State(svc): Svc, Json(s): Json<Settings>) -> ApiResult<Json<Settings>> {
    Ok(Json(svc.update_settings(s).await?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn health_is_ok() {
        let Json(v) = health().await;
        assert_eq!(v, json!({ "status": "ok" }));
    }
}
