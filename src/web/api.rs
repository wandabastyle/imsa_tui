// REST handlers for snapshots, preferences, and health probes.

use std::str::FromStr;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Serialize;

use crate::timing::Series;

use super::{prefs::Preferences, state::WebAppState};

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

pub async fn get_snapshot(
    State(state): State<WebAppState>,
    Path(series_raw): Path<String>,
) -> impl IntoResponse {
    let series = match Series::from_str(&series_raw) {
        Ok(series) => series,
        Err(err) => {
            return (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: err })).into_response();
        }
    };

    match state.snapshot_response_for(series) {
        Some(snapshot) => (StatusCode::OK, Json(snapshot)).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "series not found".to_string(),
            }),
        )
            .into_response(),
    }
}

pub async fn get_preferences(State(state): State<WebAppState>) -> impl IntoResponse {
    match state.current_preferences() {
        Some(preferences) => (StatusCode::OK, Json(preferences)).into_response(),
        None => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "preferences unavailable".to_string(),
            }),
        )
            .into_response(),
    }
}

pub async fn put_preferences(
    State(state): State<WebAppState>,
    Json(next): Json<Preferences>,
) -> impl IntoResponse {
    match state.update_preferences(next) {
        Ok(updated) => (StatusCode::OK, Json(updated)).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: err }),
        )
            .into_response(),
    }
}

pub async fn healthz() -> impl IntoResponse {
    StatusCode::OK
}

pub async fn readyz(State(state): State<WebAppState>) -> impl IntoResponse {
    let ready = Series::all()
        .iter()
        .copied()
        .all(|series| state.snapshot_for(series).is_some());
    if ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}
