//! Mapping of domain errors to HTTP responses.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use dlm_core::{Conflict, Error};
use serde::Serialize;
use utoipa::ToSchema;

/// Error body used by every non-2xx response.
#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorBody {
    pub error: String,
}

/// `409` body of `POST /downloads` when the new download collides with an existing one.
/// The UI shows `conflict` and re-sends the request with `on_conflict` set.
#[derive(Debug, Serialize, ToSchema)]
pub struct ConflictBody {
    /// Always `"conflict"`.
    pub error: String,
    pub conflict: Conflict,
}

fn err(message: impl Into<String>) -> ErrorBody {
    ErrorBody {
        error: message.into(),
    }
}

/// Wrapper so handlers can use `?` on `dlm_core::Result`.
#[derive(Debug)]
pub struct ApiError(pub Error);

impl From<Error> for ApiError {
    fn from(e: Error) -> Self {
        Self(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match &self.0 {
            Error::NotFound(_) => {
                (StatusCode::NOT_FOUND, Json(err(self.0.to_string()))).into_response()
            }
            Error::InvalidInput(m) => {
                (StatusCode::BAD_REQUEST, Json(err(m.as_str()))).into_response()
            }
            Error::InvalidState(m) => (StatusCode::CONFLICT, Json(err(m.as_str()))).into_response(),
            Error::Conflict(c) => (
                StatusCode::CONFLICT,
                Json(ConflictBody {
                    error: "conflict".into(),
                    conflict: (**c).clone(),
                }),
            )
                .into_response(),
            Error::Engine(m) => (StatusCode::BAD_GATEWAY, Json(err(m.as_str()))).into_response(),
            Error::Internal(m) => {
                // Details stay in the log, not in the response.
                tracing::error!(error = %m, "internal error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(err("internal error")),
                )
                    .into_response()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dlm_core::{Conflict, ConflictKind};

    fn status(e: Error) -> StatusCode {
        ApiError(e).into_response().status()
    }

    #[test]
    fn maps_errors_to_status_codes() {
        assert_eq!(status(Error::NotFound(1)), StatusCode::NOT_FOUND);
        assert_eq!(
            status(Error::InvalidInput("x".into())),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            status(Error::InvalidState("x".into())),
            StatusCode::CONFLICT
        );
        assert_eq!(status(Error::Engine("x".into())), StatusCode::BAD_GATEWAY);
        assert_eq!(
            status(Error::Internal("secret".into())),
            StatusCode::INTERNAL_SERVER_ERROR
        );
        let c = Conflict {
            kind: ConflictKind::FileExists,
            existing_name: "a".into(),
            existing_size: None,
            existing_modified: None,
            existing_id: None,
            suggested_name: "a (1)".into(),
        };
        assert_eq!(status(Error::Conflict(Box::new(c))), StatusCode::CONFLICT);
    }
}
