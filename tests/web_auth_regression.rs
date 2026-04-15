use axum::{
    body::{to_bytes, Body},
    http::{header, Method, Request, StatusCode},
    middleware,
    routing::{get, post},
    Router,
};
use directories::ProjectDirs;
use imsa_tui::web::{
    api,
    auth::{self, WebAuthConfig},
    sse,
    state::WebAppState,
};
use tower::ServiceExt;

fn test_app() -> Router {
    let app_state = WebAppState::new();
    let auth_config = WebAuthConfig::new("secret-code".to_string(), false);

    let public_routes = Router::new()
        .route("/healthz", get(api::healthz))
        .route("/readyz", get(api::readyz));

    let protected_api_routes = Router::new()
        .route("/api/snapshot/:series", get(api::get_snapshot))
        .route("/api/stream/:series", get(sse::stream_series))
        .route(
            "/api/demo",
            get(api::get_demo_state).put(api::put_demo_state),
        )
        .route(
            "/api/preferences",
            get(api::get_preferences).put(api::put_preferences),
        )
        .route("/api/preferences/reset", post(api::reset_preferences))
        .layer(middleware::from_fn_with_state(
            auth_config.clone(),
            auth::require_session_middleware,
        ));

    let auth_routes = Router::new()
        .route("/auth/session", get(auth::session_status))
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .with_state(auth_config);

    public_routes
        .merge(protected_api_routes)
        .merge(auth_routes)
        .with_state(app_state)
}

fn session_cookie_pair(set_cookie: &str) -> String {
    set_cookie.split(';').next().unwrap_or_default().to_string()
}

fn profile_cookie_pair(set_cookie: &str) -> Option<String> {
    let pair = set_cookie.split(';').next()?.to_string();
    if pair.starts_with("imsa_profile=") {
        Some(pair)
    } else {
        None
    }
}

fn cookie_header(session_cookie: &str, profile_cookie: &str) -> String {
    format!("{session_cookie}; {profile_cookie}")
}

fn profile_path(profile_cookie: &str) -> Option<std::path::PathBuf> {
    let (_, profile_id) = profile_cookie.split_once('=')?;
    let dirs = ProjectDirs::from("", "", "imsa_tui")?;
    Some(
        dirs.data_local_dir()
            .join("profiles")
            .join(format!("{profile_id}.toml")),
    )
}

async fn response_body_text(response: axum::response::Response) -> String {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body bytes");
    String::from_utf8(bytes.to_vec()).expect("utf8 response body")
}

#[tokio::test]
async fn auth_login_session_logout_and_protected_routes() {
    let app = test_app();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/auth/session")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("session request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_body_text(response).await;
    assert!(body.contains("\"authenticated\":false"));

    let bad_login = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({ "access_code": "wrong" }))
                        .expect("bad login payload"),
                ))
                .expect("request"),
        )
        .await
        .expect("bad login request");
    assert_eq!(bad_login.status(), StatusCode::UNAUTHORIZED);

    let login = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({ "access_code": "secret-code" }))
                        .expect("login payload"),
                ))
                .expect("request"),
        )
        .await
        .expect("login request");
    assert_eq!(login.status(), StatusCode::OK);
    let set_cookie = login
        .headers()
        .get(header::SET_COOKIE)
        .expect("set-cookie present")
        .to_str()
        .expect("set-cookie utf8")
        .to_string();
    assert!(set_cookie.contains("imsa_session="));
    assert!(set_cookie.contains("HttpOnly"));
    assert!(set_cookie.contains("SameSite=Lax"));
    assert!(!set_cookie.contains("Max-Age="));

    let session_cookie = session_cookie_pair(&set_cookie);

    let authenticated_session = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/auth/session")
                .header(header::COOKIE, &session_cookie)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("session request");
    assert_eq!(authenticated_session.status(), StatusCode::OK);
    let body = response_body_text(authenticated_session).await;
    assert!(body.contains("\"authenticated\":true"));

    let protected_ok = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/preferences")
                .header(header::COOKIE, &session_cookie)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("protected request");
    assert_eq!(protected_ok.status(), StatusCode::OK);

    let sse_ok = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/stream/imsa")
                .header(header::COOKIE, &session_cookie)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("sse request");
    assert_eq!(sse_ok.status(), StatusCode::OK);

    let logout = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/auth/logout")
                .header(header::COOKIE, &session_cookie)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("logout request");
    assert_eq!(logout.status(), StatusCode::OK);
    let clear_cookie = logout
        .headers()
        .get(header::SET_COOKIE)
        .expect("clear set-cookie present")
        .to_str()
        .expect("clear set-cookie utf8")
        .to_string();
    assert!(clear_cookie.contains("Max-Age=0"));

    let session_after_logout = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/auth/session")
                .header(header::COOKIE, &session_cookie)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("session request");
    assert_eq!(session_after_logout.status(), StatusCode::OK);
    let body = response_body_text(session_after_logout).await;
    assert!(body.contains("\"authenticated\":false"));
}

#[tokio::test]
async fn healthz_is_public_and_returns_ok_body() {
    let app = test_app();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/healthz")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("healthz request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_body_text(response).await;
    assert_eq!(body, "ok\n");
}

#[tokio::test]
async fn protected_api_and_sse_require_authentication() {
    let app = test_app();

    for (method, uri) in [
        (Method::GET, "/api/preferences"),
        (Method::GET, "/api/demo"),
        (Method::GET, "/api/snapshot/imsa"),
        (Method::GET, "/api/stream/imsa"),
        (Method::PUT, "/api/demo"),
        (Method::PUT, "/api/preferences"),
        (Method::POST, "/api/preferences/reset"),
    ] {
        let body = if method == Method::PUT && uri == "/api/preferences" {
            Body::from(
                serde_json::to_vec(&serde_json::json!({
                    "favourites": [],
                    "selected_series": "imsa"
                }))
                .expect("put payload"),
            )
        } else if method == Method::PUT && uri == "/api/demo" {
            Body::from(
                serde_json::to_vec(&serde_json::json!({
                    "enabled": true
                }))
                .expect("demo payload"),
            )
        } else {
            Body::empty()
        };

        let is_json_put = method == Method::PUT;
        let mut builder = Request::builder().method(method).uri(uri);
        if is_json_put {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
        }

        let response = app
            .clone()
            .oneshot(builder.body(body).expect("request"))
            .await
            .expect("protected request");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "uri={uri}");
        let body = response_body_text(response).await;
        assert!(body.contains("authentication required"), "uri={uri}");
    }
}

#[tokio::test]
async fn preferences_are_isolated_per_profile_cookie() {
    let app = test_app();

    let login = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({ "access_code": "secret-code" }))
                        .expect("login payload"),
                ))
                .expect("request"),
        )
        .await
        .expect("login request");
    assert_eq!(login.status(), StatusCode::OK);
    let session_cookie = session_cookie_pair(
        login
            .headers()
            .get(header::SET_COOKIE)
            .expect("session set-cookie present")
            .to_str()
            .expect("set-cookie utf8"),
    );

    let first_get = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/preferences")
                .header(header::COOKIE, &session_cookie)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("get preferences request");
    assert_eq!(first_get.status(), StatusCode::OK);
    let first_profile_cookie = profile_cookie_pair(
        first_get
            .headers()
            .get(header::SET_COOKIE)
            .expect("profile set-cookie present")
            .to_str()
            .expect("set-cookie utf8"),
    )
    .expect("profile cookie pair");

    let update = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/preferences")
                .header(
                    header::COOKIE,
                    cookie_header(&session_cookie, &first_profile_cookie),
                )
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "favourites": ["imsa|fallback:7"],
                        "selected_series": "nls"
                    }))
                    .expect("put payload"),
                ))
                .expect("request"),
        )
        .await
        .expect("put preferences request");
    assert_eq!(update.status(), StatusCode::OK);

    let first_profile_read = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/preferences")
                .header(
                    header::COOKIE,
                    cookie_header(&session_cookie, &first_profile_cookie),
                )
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("get profile 1 preferences");
    assert_eq!(first_profile_read.status(), StatusCode::OK);
    let first_body = response_body_text(first_profile_read).await;
    assert!(first_body.contains("\"selected_series\":\"nls\""));
    assert!(first_body.contains("imsa|fallback:7"));

    let second_get = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/preferences")
                .header(header::COOKIE, &session_cookie)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("get preferences request");
    assert_eq!(second_get.status(), StatusCode::OK);
    let second_profile_cookie = profile_cookie_pair(
        second_get
            .headers()
            .get(header::SET_COOKIE)
            .expect("second profile set-cookie present")
            .to_str()
            .expect("set-cookie utf8"),
    )
    .expect("second profile cookie pair");
    assert_ne!(first_profile_cookie, second_profile_cookie);

    let second_profile_read = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/preferences")
                .header(
                    header::COOKIE,
                    cookie_header(&session_cookie, &second_profile_cookie),
                )
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("get profile 2 preferences");
    assert_eq!(second_profile_read.status(), StatusCode::OK);
    let second_body = response_body_text(second_profile_read).await;
    assert!(second_body.contains("\"selected_series\":\"imsa\""));
    assert!(!second_body.contains("imsa|fallback:7"));

    let reset = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/preferences/reset")
                .header(
                    header::COOKIE,
                    cookie_header(&session_cookie, &first_profile_cookie),
                )
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("reset preferences request");
    assert_eq!(reset.status(), StatusCode::OK);

    let reset_read = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/preferences")
                .header(
                    header::COOKIE,
                    cookie_header(&session_cookie, &first_profile_cookie),
                )
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("get reset profile preferences");
    assert_eq!(reset_read.status(), StatusCode::OK);
    let reset_body = response_body_text(reset_read).await;
    assert!(reset_body.contains("\"selected_series\":\"imsa\""));
    assert!(!reset_body.contains("imsa|fallback:7"));

    if let Some(path) = profile_path(&first_profile_cookie) {
        let _ = std::fs::remove_file(path);
    }
    if let Some(path) = profile_path(&second_profile_cookie) {
        let _ = std::fs::remove_file(path);
    }
}

#[tokio::test]
async fn demo_mode_is_isolated_per_session_cookie() {
    let app = test_app();

    let login_a = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({ "access_code": "secret-code" }))
                        .expect("login payload"),
                ))
                .expect("request"),
        )
        .await
        .expect("login request");
    assert_eq!(login_a.status(), StatusCode::OK);
    let session_a = session_cookie_pair(
        login_a
            .headers()
            .get(header::SET_COOKIE)
            .expect("set-cookie present")
            .to_str()
            .expect("set-cookie utf8"),
    );

    let login_b = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/auth/login")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({ "access_code": "secret-code" }))
                        .expect("login payload"),
                ))
                .expect("request"),
        )
        .await
        .expect("login request");
    assert_eq!(login_b.status(), StatusCode::OK);
    let session_b = session_cookie_pair(
        login_b
            .headers()
            .get(header::SET_COOKIE)
            .expect("set-cookie present")
            .to_str()
            .expect("set-cookie utf8"),
    );

    let enable_demo_a = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/demo")
                .header(header::COOKIE, &session_a)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({ "enabled": true }))
                        .expect("put payload"),
                ))
                .expect("request"),
        )
        .await
        .expect("put demo request");
    assert_eq!(enable_demo_a.status(), StatusCode::OK);

    let demo_state_a = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/demo")
                .header(header::COOKIE, &session_a)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("get demo request");
    assert_eq!(demo_state_a.status(), StatusCode::OK);
    let demo_body_a = response_body_text(demo_state_a).await;
    assert!(demo_body_a.contains("\"enabled\":true"));

    let demo_state_b = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/demo")
                .header(header::COOKIE, &session_b)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("get demo request");
    assert_eq!(demo_state_b.status(), StatusCode::OK);
    let demo_body_b = response_body_text(demo_state_b).await;
    assert!(demo_body_b.contains("\"enabled\":false"));

    let snapshot_a = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/snapshot/imsa")
                .header(header::COOKIE, &session_a)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("snapshot request");
    assert_eq!(snapshot_a.status(), StatusCode::OK);
    let snapshot_body_a = response_body_text(snapshot_a).await;
    assert!(snapshot_body_a.contains("demo data"));

    let snapshot_b = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri("/api/snapshot/imsa")
                .header(header::COOKIE, &session_b)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("snapshot request");
    assert_eq!(snapshot_b.status(), StatusCode::OK);
    let snapshot_body_b = response_body_text(snapshot_b).await;
    assert!(!snapshot_body_b.contains("demo data"));
}
