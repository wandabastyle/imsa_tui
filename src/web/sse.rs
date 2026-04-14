// SSE endpoint for live snapshot streaming per series.

use std::{convert::Infallible, str::FromStr};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    http::{header, HeaderMap},
    response::{sse::Event, IntoResponse, Sse},
};
use tokio_stream::{wrappers::BroadcastStream, StreamExt};

use crate::timing::Series;

use super::state::WebAppState;

pub async fn stream_series(
    State(state): State<WebAppState>,
    headers: HeaderMap,
    Path(series_raw): Path<String>,
) -> impl IntoResponse {
    let series = match Series::from_str(&series_raw) {
        Ok(series) => series,
        Err(err) => return (StatusCode::BAD_REQUEST, err).into_response(),
    };

    if let Some(session_token) = session_token_from_headers(&headers) {
        if state.demo_state_for_session(&session_token).enabled {
            return stream_demo_series(state, series, session_token).into_response();
        }
    }

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

fn stream_demo_series(
    state: WebAppState,
    series: Series,
    session_token: String,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    use std::time::Duration;
    use tokio_stream::wrappers::IntervalStream;

    let initial_event = state
        .demo_snapshot_response_for(series, &session_token)
        .and_then(|snapshot| serde_json::to_string(&snapshot).ok())
        .map(|json| Ok(Event::default().event("snapshot").data(json)));

    let interval = tokio::time::interval(Duration::from_secs(1));
    let update_stream = IntervalStream::new(interval).filter_map({
        let state = state.clone();
        let session_token = session_token.clone();
        move |_| {
            let json = state
                .demo_snapshot_response_for(series, &session_token)
                .and_then(|snapshot| serde_json::to_string(&snapshot).ok())?;
            Some(Ok(Event::default().event("snapshot").data(json)))
        }
    });

    let stream = if let Some(initial) = initial_event {
        tokio_stream::iter(vec![initial]).chain(update_stream)
    } else {
        tokio_stream::iter(Vec::<Result<Event, Infallible>>::new()).chain(update_stream)
    };

    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
}

fn session_token_from_headers(headers: &HeaderMap) -> Option<String> {
    let raw_cookie = headers.get(header::COOKIE)?.to_str().ok()?;
    raw_cookie.split(';').find_map(|part| {
        let (name, value) = part.trim().split_once('=')?;
        if name == "imsa_session" {
            Some(value.to_string())
        } else {
            None
        }
    })
}
