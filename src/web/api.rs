// REST handlers for snapshots, preferences, and health probes.

use std::{
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use rand::{distributions::Alphanumeric, Rng};
use serde::Serialize;
use serde_json::json;

use crate::timing::Series;

use super::{prefs::Preferences, state::WebAppState};

const SESSION_COOKIE_NAME: &str = "imsa_session";

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct PutDemoRequest {
    pub enabled: bool,
}

pub async fn get_snapshot(
    State(state): State<WebAppState>,
    headers: HeaderMap,
    Path(series_raw): Path<String>,
) -> impl IntoResponse {
    let series = match Series::from_str(&series_raw) {
        Ok(series) => series,
        Err(err) => {
            return (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: err })).into_response();
        }
    };

    let session_token = session_token_from_headers(&headers);
    if let Some(token) = session_token.as_deref() {
        if let Some(snapshot) = state.demo_snapshot_response_for(series, token) {
            return (StatusCode::OK, Json(snapshot)).into_response();
        }
    }

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

pub async fn get_demo_state(State(state): State<WebAppState>, headers: HeaderMap) -> Response {
    let Some(session_token) = session_token_from_headers(&headers) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "authentication required".to_string(),
            }),
        )
            .into_response();
    };

    let response = state.demo_state_for_session(&session_token);
    (StatusCode::OK, Json(response)).into_response()
}

pub async fn put_demo_state(
    State(state): State<WebAppState>,
    headers: HeaderMap,
    Json(payload): Json<PutDemoRequest>,
) -> Response {
    let Some(session_token) = session_token_from_headers(&headers) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "authentication required".to_string(),
            }),
        )
            .into_response();
    };

    let response = state.set_demo_for_session(&session_token, payload.enabled);
    (StatusCode::OK, Json(response)).into_response()
}

pub async fn get_preferences(State(state): State<WebAppState>, headers: HeaderMap) -> Response {
    let (profile_id, set_cookie) = profile_context(&state, &headers);

    match state.current_preferences_for(&profile_id) {
        Ok(preferences) => with_profile_cookie(
            set_cookie,
            (StatusCode::OK, Json(preferences)).into_response(),
        ),
        Err(err) => with_profile_cookie(
            set_cookie,
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: err }),
            )
                .into_response(),
        ),
    }
}

pub async fn put_preferences(
    State(state): State<WebAppState>,
    headers: HeaderMap,
    Json(next): Json<Preferences>,
) -> Response {
    let (profile_id, set_cookie) = profile_context(&state, &headers);

    match state.update_preferences_for(&profile_id, next) {
        Ok(updated) => {
            with_profile_cookie(set_cookie, (StatusCode::OK, Json(updated)).into_response())
        }
        Err(err) => with_profile_cookie(
            set_cookie,
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: err }),
            )
                .into_response(),
        ),
    }
}

pub async fn reset_preferences(State(state): State<WebAppState>, headers: HeaderMap) -> Response {
    let (profile_id, set_cookie) = profile_context(&state, &headers);

    match state.reset_preferences_for(&profile_id) {
        Ok(defaults) => {
            with_profile_cookie(set_cookie, (StatusCode::OK, Json(defaults)).into_response())
        }
        Err(err) => with_profile_cookie(
            set_cookie,
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: err }),
            )
                .into_response(),
        ),
    }
}

pub async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, "ok\n")
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

const PROFILE_COOKIE_NAME: &str = "imsa_profile";
const PROFILE_COOKIE_MAX_AGE_SECS: u64 = 60 * 60 * 24 * 365;

fn profile_context(
    state: &WebAppState,
    headers: &axum::http::HeaderMap,
) -> (String, Option<String>) {
    let mut create_reason = "missing_cookie";

    if let Some(profile_id) = cookie_value(headers, PROFILE_COOKIE_NAME) {
        if valid_profile_id(profile_id) {
            return (profile_id.to_string(), None);
        }
        create_reason = "invalid_cookie";
    }

    let generated = generate_profile_id();
    let cookie = build_profile_cookie(state.profile_cookie_secure(), &generated);
    log_profile_event("web_profile", "created", create_reason, &generated);
    (generated, Some(cookie))
}

fn log_profile_event(event: &str, outcome: &str, reason: &str, profile_id: &str) {
    eprintln!(
        "{}",
        json!({
            "event": event,
            "outcome": outcome,
            "reason": reason,
            "profile_hint": profile_hint(profile_id),
            "ts_unix": now_unix_secs(),
        })
    );
}

fn profile_hint(profile_id: &str) -> String {
    if profile_id == "-" {
        return "-".to_string();
    }

    profile_id.chars().take(8).collect()
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn with_profile_cookie(cookie: Option<String>, mut response: Response) -> Response {
    if let Some(cookie) = cookie {
        if let Ok(value) = HeaderValue::from_str(&cookie) {
            response.headers_mut().insert(header::SET_COOKIE, value);
        }
    }
    response
}

fn build_profile_cookie(secure: bool, profile_id: &str) -> String {
    let mut cookie = format!(
        "{PROFILE_COOKIE_NAME}={profile_id}; Path=/; HttpOnly; SameSite=Lax; Max-Age={PROFILE_COOKIE_MAX_AGE_SECS}"
    );
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

fn generate_profile_id() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(48)
        .map(char::from)
        .collect()
}

fn cookie_value<'a>(headers: &'a axum::http::HeaderMap, cookie_name: &str) -> Option<&'a str> {
    let raw_cookie = headers.get(header::COOKIE)?.to_str().ok()?;
    raw_cookie.split(';').find_map(|part| {
        let (name, value) = part.trim().split_once('=')?;
        if name == cookie_name {
            Some(value)
        } else {
            None
        }
    })
}

fn valid_profile_id(value: &str) -> bool {
    let len = value.len();
    if !(8..=128).contains(&len) {
        return false;
    }

    value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

fn session_token_from_headers(headers: &axum::http::HeaderMap) -> Option<String> {
    cookie_value(headers, SESSION_COOKIE_NAME).map(ToString::to_string)
}
