// SSE endpoint for live snapshot streaming per series.

use std::{convert::Infallible, str::FromStr};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{sse::Event, IntoResponse, Sse},
};
use tokio_stream::{wrappers::BroadcastStream, StreamExt};

use crate::timing::Series;

use super::state::WebAppState;

pub async fn stream_series(
    State(state): State<WebAppState>,
    Path(series_raw): Path<String>,
) -> impl IntoResponse {
    let series = match Series::from_str(&series_raw) {
        Ok(series) => series,
        Err(err) => return (StatusCode::BAD_REQUEST, err).into_response(),
    };

    let Some(rx) = state.subscribe_series(series) else {
        return (StatusCode::NOT_FOUND, "unknown series").into_response();
    };

    let initial_event = state
        .snapshot_response_for(series)
        .and_then(|snapshot| serde_json::to_string(&snapshot).ok())
        .map(|json| Ok(Event::default().event("snapshot").data(json)));

    // Clients get one immediate snapshot, then one event per worker update.
    let update_stream = BroadcastStream::new(rx).filter_map({
        let state = state.clone();
        move |event| {
            if event.is_err() {
                return None;
            }

            let json = state
                .snapshot_response_for(series)
                .and_then(|snapshot| serde_json::to_string(&snapshot).ok())?;
            Some(Ok(Event::default().event("snapshot").data(json)))
        }
    });

    let stream = if let Some(initial) = initial_event {
        tokio_stream::iter(vec![initial]).chain(update_stream)
    } else {
        tokio_stream::iter(Vec::<Result<Event, Infallible>>::new()).chain(update_stream)
    };

    Sse::new(stream)
        .keep_alive(axum::response::sse::KeepAlive::default())
        .into_response()
}
