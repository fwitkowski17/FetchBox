//! HTTP API: axum routes, SSE, auth, OpenAPI.

pub mod auth;
pub mod error;
pub mod openapi;
pub mod routes;
pub mod service;
pub mod sse;

use std::sync::Arc;

use axum::Router;

use service::Service;

/// Build the `/api` router on top of a download service.
pub fn router(service: Arc<dyn Service>) -> Router {
    Router::new().nest("/api", routes::router(service))
}
