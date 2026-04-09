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
    pub session_ttl_secs: u64,
    sessions: Arc<RwLock<HashMap<String, u64>>>,
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

impl WebAuthConfig {
    pub fn new(access_code: String) -> Self {
        Self {
            access_code,
            cookie_name: "imsa_session".to_string(),
            session_ttl_secs: 60 * 60 * 24 * 30,
            sessions: Arc::new(RwLock::new(HashMap::new())),
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
}

pub async fn login(State(config): State<WebAuthConfig>, Json(payload): Json<LoginRequest>) -> Response {
    if payload.access_code != config.access_code {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "invalid access code".to_string(),
            }),
        )
            .into_response();
    }

    let Some(token) = config.create_session() else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "failed to create session".to_string(),
            }),
        )
            .into_response();
    };

    let cookie = format!(
        "{}={}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}",
        config.cookie_name, token, config.session_ttl_secs
    );

    let mut response = (StatusCode::OK, Json(SessionStateResponse { authenticated: true })).into_response();
    if let Ok(value) = HeaderValue::from_str(&cookie) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    response
}

pub async fn logout(State(config): State<WebAuthConfig>, request: Request) -> Response {
    config.revoke_from_headers(request.headers());

    let clear_cookie = format!(
        "{}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0",
        config.cookie_name
    );
    let mut response = (StatusCode::OK, Json(SessionStateResponse { authenticated: false })).into_response();
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
    (
        StatusCode::UNAUTHORIZED,
        Json(ErrorResponse {
            error: "authentication required".to_string(),
        }),
    )
        .into_response()
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
    let dirs = ProjectDirs::from("com", "imsa", "imsa_tui")?;
    Some(dirs.config_dir().join("web_auth.toml"))
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
    let encoded = toml::to_string_pretty(&payload)
        .map_err(|e| format!("encode auth config failed: {e}"))?;
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

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
