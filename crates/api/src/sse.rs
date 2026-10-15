//! Server-sent events: progress and status changes (SPEC 6.6).
//!
//! Event names: `download`, `progress`, `removed`, `settings`, plus `resync` when this
//! client fell behind and must reload the list. Data is the JSON of [`dlm_core::Event`].

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::State,
    http::{HeaderName, HeaderValue},
    response::{
        sse::{Event as Sse_, KeepAlive, Sse},
        IntoResponse,
    },
};
use dlm_core::Event;
use futures_util::stream::{self, StreamExt};
use tokio::sync::broadcast::error::RecvError;

use crate::service::Service;

fn event_name(e: &Event) -> &'static str {
    match e {
        Event::Download { .. } => "download",
        Event::Progress { .. } => "progress",
        Event::Removed { .. } => "removed",
        Event::Settings { .. } => "settings",
    }
}

/// Live updates as server-sent events.
///
/// Each SSE message has an `event:` name (`download`, `progress`, `removed`, `settings`, or
/// `resync` with empty data) and a JSON `data:` payload shaped like [`Event`]. After `resync`
/// (this client fell behind) reload the list with `GET /api/downloads`. Subscribe before
/// fetching the list so nothing in between is lost.
#[utoipa::path(
    get,
    path = "/api/events",
    tag = "events",
    security(("session" = []), ("bearer" = [])),
    responses(
        (status = 200, description = "Event stream", content_type = "text/event-stream", body = Event),
        (status = 401, description = "Not logged in", body = crate::error::ErrorBody),
    )
)]
pub async fn events(State(svc): State<Arc<dyn Service>>) -> impl IntoResponse {
    // Subscribe before returning so nothing between the client's list fetch and now is lost.
    let rx = svc.subscribe();
    let stream = stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(ev) => match Sse_::default().event(event_name(&ev)).json_data(&ev) {
                    Ok(sse) => return Some((Ok(sse), rx)),
                    Err(_) => continue,
                },
                Err(RecvError::Lagged(_)) => {
                    return Some((Ok(Sse_::default().event("resync").data("")), rx))
                }
                Err(RecvError::Closed) => return None,
            }
        }
    });
    // Sent at once, so the connection counts as open on the client even behind a proxy that holds
    // back headers until the first bytes. `retry` is how long a browser waits before it connects
    // again after the stream dropped (default: about three seconds).
    let hello = stream::once(async {
        Ok::<_, Infallible>(
            Sse_::default()
                .retry(Duration::from_millis(RECONNECT_MS))
                .comment("connected"),
        )
    });
    (
        // nginx and similar proxies buffer responses unless told not to; a buffered event stream
        // looks dead until the buffer fills.
        [(
            HeaderName::from_static("x-accel-buffering"),
            HeaderValue::from_static("no"),
        )],
        Sse::new(hello.chain(stream)).keep_alive(KeepAlive::default()),
    )
}

/// Client reconnect delay announced in the stream.
const RECONNECT_MS: u64 = 1000;
