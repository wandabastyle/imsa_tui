use axum::{
    extract::{Request, State},
    http::{header, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use directories::ProjectDirs;
use rand::{distributions::Alphanumeric, Rng};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Debug, Clone)]
pub struct WebAuthConfig {
    pub username: String,
    pub password: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub enum PasswordState {
    Loaded,
    GeneratedPersisted,
    GeneratedEphemeral,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredWebAuth {
    username: String,
    password: String,
}

impl WebAuthConfig {
    pub fn is_enabled(&self) -> bool {
        self.password.is_some()
    }
}

pub async fn basic_auth_middleware(
    State(config): State<WebAuthConfig>,
    request: Request,
    next: Next,
) -> Response {
    let Some(expected_password) = config.password.as_deref() else {
        return next.run(request).await;
    };

    let auth_header = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");

    let Some(encoded) = auth_header.strip_prefix("Basic ") else {
        return unauthorized_response();
    };

    let Ok(decoded) = STANDARD.decode(encoded) else {
        return unauthorized_response();
    };
    let Ok(decoded_text) = std::str::from_utf8(&decoded) else {
        return unauthorized_response();
    };

    let mut parts = decoded_text.splitn(2, ':');
    let Some(given_username) = parts.next() else {
        return unauthorized_response();
    };
    let Some(given_password) = parts.next() else {
        return unauthorized_response();
    };

    if given_username != config.username || given_password != expected_password {
        return unauthorized_response();
    }

    next.run(request).await
}

fn unauthorized_response() -> Response {
    let mut response = StatusCode::UNAUTHORIZED.into_response();
    response.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_static("Basic realm=\"imsa_tui\""),
    );
    response
}

pub fn load_or_initialize_password(username: &str, rotate: bool) -> (String, PasswordState) {
    if rotate {
        let generated = generate_password(24);
        return match save_stored_auth(username, &generated) {
            Ok(_) => (generated, PasswordState::GeneratedPersisted),
            Err(_) => (generated, PasswordState::GeneratedEphemeral),
        };
    }

    if let Some(stored) = load_stored_auth() {
        if !stored.password.trim().is_empty() {
            return (stored.password, PasswordState::Loaded);
        }
    }

    let generated = generate_password(24);
    match save_stored_auth(username, &generated) {
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

fn save_stored_auth(username: &str, password: &str) -> Result<(), String> {
    let Some(path) = stored_auth_path() else {
        return Err("unable to resolve config directory".to_string());
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create config directory failed: {e}"))?;
    }

    let payload = StoredWebAuth {
        username: username.to_string(),
        password: password.to_string(),
    };
    let encoded = toml::to_string_pretty(&payload)
        .map_err(|e| format!("encode auth config failed: {e}"))?;
    fs::write(path, encoded).map_err(|e| format!("write auth config failed: {e}"))
}
