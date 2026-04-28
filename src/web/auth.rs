// Shared-access-code authentication:
// - persists one access code
// - issues in-memory cookie sessions
// - guards protected API routes

use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use directories::ProjectDirs;
use rand::{
    distr::{Alphanumeric, SampleString},
    RngExt,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::hash_map::DefaultHasher,
    collections::HashMap,
    fs,
    hash::{Hash, Hasher},
    path::PathBuf,
    sync::{Arc, RwLock},
    time::{SystemTime, UNIX_EPOCH},
};
use web_shared::{LoginRequest, SessionStateResponse};

#[derive(Debug, Clone)]
pub struct WebAuthConfig {
    pub access_code_hash: String,
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

#[derive(Debug, Clone)]
pub struct ResolvedAccessCode {
    pub access_code_hash: String,
    pub one_time_access_code: Option<String>,
    pub state: PasswordState,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredWebAuth {
    access_code_hash: String,
}

#[derive(Debug, Clone, Copy, Default)]
struct LoginAttemptState {
    window_start: u64,
    attempts: u32,
    blocked_until: u64,
}

impl WebAuthConfig {
    pub fn new(access_code_hash: String, cookie_secure: bool) -> Self {
        Self {
            access_code_hash,
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

    fn revoke_from_headers(&self, headers: &HeaderMap) -> bool {
        let Some(token) = cookie_value(headers, &self.cookie_name) else {
            return false;
        };
        if let Ok(mut guard) = self.sessions.write() {
            return guard.remove(token).is_some();
        }
        false
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
    let client = anonymized_client_key(&login_key);
    if let Err(retry_after_secs) = config.check_login_allowed(&login_key) {
        log_auth_event(
            "auth_login",
            "blocked",
            Some(retry_after_secs),
            Some(client.as_str()),
        );
        return error_response(
            StatusCode::TOO_MANY_REQUESTS,
            "too many login attempts, try again later",
            Some(retry_after_secs),
        );
    }

    let valid = verify_access_code(&payload.access_code, &config.access_code_hash).unwrap_or(false);
    if !valid {
        config.record_login_failure(&login_key);
        log_auth_event("auth_login", "failure", None, Some(client.as_str()));
        return error_response(StatusCode::UNAUTHORIZED, "invalid access code", None);
    }

    config.record_login_success(&login_key);

    let Some(token) = config.create_session() else {
        log_auth_event(
            "auth_login",
            "session_create_error",
            None,
            Some(client.as_str()),
        );
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to create session",
            None,
        );
    };

    log_auth_event("auth_login", "success", None, Some(client.as_str()));

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
    let had_session = config.revoke_from_headers(request.headers());
    let client = anonymized_client_key(&login_attempt_key(request.headers()));
    log_auth_event(
        "auth_logout",
        if had_session { "success" } else { "no_session" },
        None,
        Some(client.as_str()),
    );

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
        let client = anonymized_client_key(&login_attempt_key(request.headers()));
        log_auth_event("auth_guard", "denied", None, Some(client.as_str()));
        return unauthorized_response();
    }

    next.run(request).await
}

fn unauthorized_response() -> Response {
    error_response(StatusCode::UNAUTHORIZED, "authentication required", None)
}

fn log_auth_event(event: &str, outcome: &str, retry_after_secs: Option<u64>, client: Option<&str>) {
    eprintln!(
        "{}",
        json!({
            "event": event,
            "outcome": outcome,
            "client": client,
            "retry_after_secs": retry_after_secs,
            "ts_unix": now_unix_secs(),
        })
    );
}

fn anonymized_client_key(raw: &str) -> String {
    if raw == "unknown-client" {
        return raw.to_string();
    }

    let mut hasher = DefaultHasher::new();
    raw.hash(&mut hasher);
    format!("client-{hash:016x}", hash = hasher.finish())
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

pub fn load_or_initialize_password(rotate: bool) -> ResolvedAccessCode {
    if rotate {
        let generated = generate_password(24);
        let hash = match hash_access_code(&generated) {
            Ok(value) => value,
            Err(_) => {
                return ResolvedAccessCode {
                    access_code_hash: String::new(),
                    one_time_access_code: Some(generated),
                    state: PasswordState::GeneratedEphemeral,
                }
            }
        };

        return match save_stored_auth(&hash) {
            Ok(_) => ResolvedAccessCode {
                access_code_hash: hash,
                one_time_access_code: Some(generated),
                state: PasswordState::GeneratedPersisted,
            },
            Err(_) => ResolvedAccessCode {
                access_code_hash: hash,
                one_time_access_code: Some(generated),
                state: PasswordState::GeneratedEphemeral,
            },
        };
    }

    if let Some(stored) = load_stored_auth() {
        if !stored.access_code_hash.trim().is_empty()
            && PasswordHash::new(stored.access_code_hash.trim()).is_ok()
        {
            return ResolvedAccessCode {
                access_code_hash: stored.access_code_hash,
                one_time_access_code: None,
                state: PasswordState::Loaded,
            };
        }
    }

    let generated = generate_password(24);
    let hash = match hash_access_code(&generated) {
        Ok(value) => value,
        Err(_) => {
            return ResolvedAccessCode {
                access_code_hash: String::new(),
                one_time_access_code: Some(generated),
                state: PasswordState::GeneratedEphemeral,
            }
        }
    };

    match save_stored_auth(&hash) {
        Ok(_) => ResolvedAccessCode {
            access_code_hash: hash,
            one_time_access_code: Some(generated),
            state: PasswordState::GeneratedPersisted,
        },
        Err(_) => ResolvedAccessCode {
            access_code_hash: hash,
            one_time_access_code: Some(generated),
            state: PasswordState::GeneratedEphemeral,
        },
    }
}

pub fn hash_access_code(access_code: &str) -> Result<String, String> {
    let mut rng = rand::rng();
    let salt_bytes: [u8; 16] = rng.random();
    let salt = SaltString::encode_b64(&salt_bytes)
        .map_err(|err| format!("access code salt failed: {err}"))?;
    Argon2::default()
        .hash_password(access_code.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|err| format!("access code hash failed: {err}"))
}

fn verify_access_code(access_code: &str, access_code_hash: &str) -> Result<bool, String> {
    let parsed = PasswordHash::new(access_code_hash)
        .map_err(|err| format!("access code hash parse failed: {err}"))?;
    Ok(Argon2::default()
        .verify_password(access_code.as_bytes(), &parsed)
        .is_ok())
}

pub fn stored_auth_path() -> Option<PathBuf> {
    let dirs = ProjectDirs::from("", "", "imsa_tui")?;
    Some(dirs.data_local_dir().join("web_auth.toml"))
}

fn generate_password(length: usize) -> String {
    let mut rng = rand::rng();
    Alphanumeric.sample_string(&mut rng, length)
}

fn load_stored_auth() -> Option<StoredWebAuth> {
    let path = stored_auth_path()?;
    let text = fs::read_to_string(path).ok()?;
    toml::from_str::<StoredWebAuth>(&text).ok()
}

fn save_stored_auth(access_code_hash: &str) -> Result<(), String> {
    let Some(path) = stored_auth_path() else {
        return Err("unable to resolve config directory".to_string());
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create config directory failed: {e}"))?;
    }

    let payload = StoredWebAuth {
        access_code_hash: access_code_hash.to_string(),
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
    use std::{
        path::PathBuf,
        sync::{Mutex, OnceLock},
    };

    fn auth_file_test_lock() -> &'static Mutex<()> {
        static AUTH_FILE_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        AUTH_FILE_TEST_LOCK.get_or_init(|| Mutex::new(()))
    }

    struct AuthFileRestore {
        path: PathBuf,
        previous: Option<Vec<u8>>,
    }

    impl AuthFileRestore {
        fn capture() -> Self {
            let path = stored_auth_path().expect("stored auth path");
            let previous = fs::read(&path).ok();
            Self { path, previous }
        }

        fn overwrite(&self, content: &str) {
            if let Some(parent) = self.path.parent() {
                fs::create_dir_all(parent).expect("create auth dir");
            }
            fs::write(&self.path, content).expect("write auth fixture");
        }

        fn read_current(&self) -> String {
            fs::read_to_string(&self.path).expect("read current auth file")
        }
    }

    impl Drop for AuthFileRestore {
        fn drop(&mut self) {
            if let Some(previous) = self.previous.as_ref() {
                if let Some(parent) = self.path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let _ = fs::write(&self.path, previous);
            } else {
                let _ = fs::remove_file(&self.path);
            }
        }
    }

    #[test]
    fn stored_auth_path_uses_data_local_dir() {
        let path = stored_auth_path().expect("stored auth path");
        let dirs = ProjectDirs::from("", "", "imsa_tui").expect("project dirs");
        assert_eq!(path, dirs.data_local_dir().join("web_auth.toml"));
    }

    #[test]
    fn access_code_hash_roundtrip_verifies_plaintext() {
        let hash = hash_access_code("secret-code").expect("hash access code");
        assert!(verify_access_code("secret-code", &hash).expect("verify access code"));
        assert!(!verify_access_code("wrong", &hash).expect("verify wrong code"));
    }

    #[test]
    fn bootstrap_replaces_non_hash_auth_file_with_hash_only_format() {
        let _guard = auth_file_test_lock().lock().expect("lock auth file test");
        let restore = AuthFileRestore::capture();
        restore.overwrite("access_code = \"legacy-plaintext\"\n");

        let resolved = load_or_initialize_password(false);
        let access_code = resolved.one_time_access_code.expect("one-time access code");
        assert!(
            verify_access_code(&access_code, &resolved.access_code_hash).expect("verify generated")
        );

        if matches!(resolved.state, PasswordState::GeneratedPersisted) {
            let saved = restore.read_current();
            assert!(saved.contains("access_code_hash"));
            assert!(!saved.contains("access_code ="));
            assert!(!saved.contains("password ="));
        }
    }

    #[test]
    fn rotate_generates_new_secret_and_hash() {
        let _guard = auth_file_test_lock().lock().expect("lock auth file test");
        let _restore = AuthFileRestore::capture();

        let first = load_or_initialize_password(false);
        let rotated = load_or_initialize_password(true);

        let rotated_secret = rotated
            .one_time_access_code
            .as_deref()
            .expect("rotated access code");
        assert!(
            verify_access_code(rotated_secret, &rotated.access_code_hash).expect("verify rotated")
        );
        assert_ne!(first.access_code_hash, rotated.access_code_hash);
    }
}
