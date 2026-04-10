// Shared-access-code authentication:
// - persists one access code
// - issues in-memory cookie sessions
// - guards protected API routes

use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use directories::ProjectDirs;
use rand::{distributions::Alphanumeric, Rng};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::{Arc, RwLock},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone)]
pub struct WebAuthConfig {
    pub access_code: String,
    pub cookie_name: String,
    pub cookie_secure: bool,
    pub session_ttl_secs: u64,
    pub login_window_secs: u64,
    pub max_login_attempts: u32,
    pub login_block_secs: u64,
    sessions: Arc<RwLock<HashMap<String, u64>>>,
    login_attempts: Arc<RwLock<HashMap<String, LoginAttemptState>>>,
}

#[derive(Debug, Clone, Copy)]
pub enum PasswordState {
    Loaded,
    GeneratedPersisted,
    GeneratedEphemeral,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredWebAuth {
    #[serde(alias = "password")]
    access_code: String,
}

#[derive(Debug, Clone, Copy, Default)]
struct LoginAttemptState {
    window_start: u64,
    attempts: u32,
    blocked_until: u64,
}

impl WebAuthConfig {
    pub fn new(access_code: String, cookie_secure: bool) -> Self {
        Self {
            access_code,
            cookie_name: "imsa_session".to_string(),
            cookie_secure,
            session_ttl_secs: 60 * 60 * 24 * 30,
            login_window_secs: 60,
            max_login_attempts: 6,
            login_block_secs: 5 * 60,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            login_attempts: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn create_session(&self) -> Option<String> {
        let expires_at = now_unix_secs().saturating_add(self.session_ttl_secs);
        let token = generate_password(48);
        let mut guard = self.sessions.write().ok()?;
        guard.insert(token.clone(), expires_at);
        Some(token)
    }

    fn validate_headers(&self, headers: &HeaderMap) -> bool {
        let Some(token) = cookie_value(headers, &self.cookie_name) else {
            return false;
        };
        let now = now_unix_secs();
        let mut guard = match self.sessions.write() {
            Ok(g) => g,
            Err(_) => return false,
        };

        guard.retain(|_, expires| *expires > now);

        matches!(guard.get(token), Some(expires) if *expires > now)
    }

    fn revoke_from_headers(&self, headers: &HeaderMap) {
        let Some(token) = cookie_value(headers, &self.cookie_name) else {
            return;
        };
        if let Ok(mut guard) = self.sessions.write() {
            guard.remove(token);
        }
    }

    fn check_login_allowed(&self, key: &str) -> Result<(), u64> {
        let now = now_unix_secs();
        let mut guard = match self.login_attempts.write() {
            Ok(g) => g,
            Err(_) => return Err(self.login_block_secs),
        };

        guard.retain(|_, state| {
            state.blocked_until > now
                || now.saturating_sub(state.window_start) <= self.login_window_secs
        });

        let state = guard.entry(key.to_string()).or_default();
        if state.blocked_until > now {
            return Err(state.blocked_until.saturating_sub(now));
        }

        if now.saturating_sub(state.window_start) > self.login_window_secs {
            state.window_start = now;
            state.attempts = 0;
            state.blocked_until = 0;
        }

        if state.attempts >= self.max_login_attempts {
            state.blocked_until = now.saturating_add(self.login_block_secs);
            return Err(self.login_block_secs);
        }

        Ok(())
    }

    fn record_login_failure(&self, key: &str) {
        let now = now_unix_secs();
        if let Ok(mut guard) = self.login_attempts.write() {
            let state = guard.entry(key.to_string()).or_default();
            if state.window_start == 0
                || now.saturating_sub(state.window_start) > self.login_window_secs
            {
                state.window_start = now;
                state.attempts = 0;
                state.blocked_until = 0;
            }
            state.attempts = state.attempts.saturating_add(1);
            if state.attempts >= self.max_login_attempts {
                state.blocked_until = now.saturating_add(self.login_block_secs);
            }
        }
    }

    fn record_login_success(&self, key: &str) {
        if let Ok(mut guard) = self.login_attempts.write() {
            guard.remove(key);
        }
    }

    fn build_cookie(&self, name: &str, value: &str, max_age: Option<u64>) -> String {
        let mut cookie = format!("{name}={value}; Path=/; HttpOnly; SameSite=Lax");
        if let Some(max_age) = max_age {
            cookie.push_str(&format!("; Max-Age={max_age}"));
        }
        if self.cookie_secure {
            cookie.push_str("; Secure");
        }
        cookie
    }
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub access_code: String,
}

#[derive(Debug, Serialize)]
pub struct SessionStateResponse {
    pub authenticated: bool,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    retry_after_secs: Option<u64>,
}

pub async fn login(
    State(config): State<WebAuthConfig>,
    headers: HeaderMap,
    Json(payload): Json<LoginRequest>,
) -> Response {
    let login_key = login_attempt_key(&headers);
    if let Err(retry_after_secs) = config.check_login_allowed(&login_key) {
        return error_response(
            StatusCode::TOO_MANY_REQUESTS,
            "too many login attempts, try again later",
            Some(retry_after_secs),
        );
    }

    if payload.access_code != config.access_code {
        config.record_login_failure(&login_key);
        return error_response(StatusCode::UNAUTHORIZED, "invalid access code", None);
    }

    config.record_login_success(&login_key);

    let Some(token) = config.create_session() else {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to create session",
            None,
        );
    };

    let cookie = config.build_cookie(&config.cookie_name, &token, None);

    let mut response = (
        StatusCode::OK,
        Json(SessionStateResponse {
            authenticated: true,
        }),
    )
        .into_response();
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    response
}

pub async fn logout(State(config): State<WebAuthConfig>, request: Request) -> Response {
    config.revoke_from_headers(request.headers());

    let clear_cookie = config.build_cookie(&config.cookie_name, "", Some(0));
    let mut response = (
        StatusCode::OK,
        Json(SessionStateResponse {
            authenticated: false,
        }),
    )
        .into_response();
    if let Ok(value) = HeaderValue::from_str(&clear_cookie) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    response
}

pub async fn session_status(State(config): State<WebAuthConfig>, request: Request) -> Response {
    let authenticated = config.validate_headers(request.headers());
    (StatusCode::OK, Json(SessionStateResponse { authenticated })).into_response()
}

pub async fn require_session_middleware(
    State(config): State<WebAuthConfig>,
    request: Request,
    next: Next,
) -> Response {
    if !config.validate_headers(request.headers()) {
        return unauthorized_response();
    }

    next.run(request).await
}

fn unauthorized_response() -> Response {
    error_response(StatusCode::UNAUTHORIZED, "authentication required", None)
}

fn error_response(status: StatusCode, message: &str, retry_after_secs: Option<u64>) -> Response {
    let mut response = (
        status,
        Json(ErrorResponse {
            error: message.to_string(),
            retry_after_secs,
        }),
    )
        .into_response();

    if let Some(seconds) = retry_after_secs {
        if let Ok(value) = HeaderValue::from_str(&seconds.to_string()) {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
    }

    response
}

pub fn load_or_initialize_password(rotate: bool) -> (String, PasswordState) {
    if rotate {
        let generated = generate_password(24);
        return match save_stored_auth(&generated) {
            Ok(_) => (generated, PasswordState::GeneratedPersisted),
            Err(_) => (generated, PasswordState::GeneratedEphemeral),
        };
    }

    if let Some(stored) = load_stored_auth() {
        if !stored.access_code.trim().is_empty() {
            return (stored.access_code, PasswordState::Loaded);
        }
    }

    let generated = generate_password(24);
    match save_stored_auth(&generated) {
        Ok(_) => (generated, PasswordState::GeneratedPersisted),
        Err(_) => (generated, PasswordState::GeneratedEphemeral),
    }
}

pub fn stored_auth_path() -> Option<PathBuf> {
    let dirs = ProjectDirs::from("", "", "imsa_tui")?;
    Some(dirs.data_local_dir().join("web_auth.toml"))
}

fn generate_password(length: usize) -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(length)
        .map(char::from)
        .collect()
}

fn load_stored_auth() -> Option<StoredWebAuth> {
    let path = stored_auth_path()?;
    let text = fs::read_to_string(path).ok()?;
    toml::from_str::<StoredWebAuth>(&text).ok()
}

fn save_stored_auth(access_code: &str) -> Result<(), String> {
    let Some(path) = stored_auth_path() else {
        return Err("unable to resolve config directory".to_string());
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create config directory failed: {e}"))?;
    }

    let payload = StoredWebAuth {
        access_code: access_code.to_string(),
    };
    let encoded =
        toml::to_string_pretty(&payload).map_err(|e| format!("encode auth config failed: {e}"))?;
    fs::write(path, encoded).map_err(|e| format!("write auth config failed: {e}"))
}

fn cookie_value<'a>(headers: &'a HeaderMap, cookie_name: &str) -> Option<&'a str> {
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

fn login_attempt_key(headers: &HeaderMap) -> String {
    if let Some(forwarded) = headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|raw| raw.split(',').next())
    {
        let key = forwarded.trim();
        if !key.is_empty() {
            return key.to_string();
        }
    }

    if let Some(real_ip) = headers
        .get("x-real-ip")
        .and_then(|value| value.to_str().ok())
    {
        let key = real_ip.trim();
        if !key.is_empty() {
            return key.to_string();
        }
    }

    "unknown-client".to_string()
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_auth_path_uses_data_local_dir() {
        let path = stored_auth_path().expect("stored auth path");
        let dirs = ProjectDirs::from("", "", "imsa_tui").expect("project dirs");
        assert_eq!(path, dirs.data_local_dir().join("web_auth.toml"));
    }
}
