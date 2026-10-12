//! HTTP API: axum routes, SSE, auth, OpenAPI.

pub mod auth;
pub mod openapi;
pub mod routes;
pub mod sse;

use axum::Router;

/// Build the `/api` router.
pub fn router() -> Router {
    Router::new().nest("/api", routes::router())
}
