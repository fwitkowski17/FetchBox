//! Server-sent events: progress and status changes (SPEC 6.6).
//!
//! Event names: `download`, `progress`, `removed`, `settings`, plus `resync` when this
//! client fell behind and must reload the list. Data is the JSON of [`dlm_core::Event`].

use std::convert::Infallible;
use std::sync::Arc;

use axum::{
    extract::State,
    response::sse::{Event as Sse_, KeepAlive, Sse},
};
use dlm_core::Event;
use futures_util::stream::{self, Stream};
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

/// `GET /api/events`.
pub async fn events(
    State(svc): State<Arc<dyn Service>>,
) -> Sse<impl Stream<Item = Result<Sse_, Infallible>>> {
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
    Sse::new(stream).keep_alive(KeepAlive::default())
}
