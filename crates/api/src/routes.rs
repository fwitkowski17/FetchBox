//! REST routes. Milestone 0: health only.

use axum::{routing::get, Json, Router};
use serde_json::{json, Value};

/// Routes mounted under `/api`.
pub fn router() -> Router {
    Router::new().route("/health", get(health))
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
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
