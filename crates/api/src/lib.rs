//! HTTP API: axum routes, SSE, auth, OpenAPI.

pub mod auth;
pub mod error;
pub mod openapi;
pub mod routes;
pub mod service;
pub mod sse;
mod torrents;

use std::sync::Arc;

use axum::Router;

use service::Service;

pub use auth::Auth;

/// Build the `/api` router on top of a download service.
pub fn router(service: Arc<dyn Service>, auth: Arc<Auth>) -> Router {
    Router::new().nest("/api", routes::router(service, auth))
}
