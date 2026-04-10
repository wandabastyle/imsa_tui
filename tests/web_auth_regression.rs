use axum::{
    body::{to_bytes, Body},
    http::{header, Method, Request, StatusCode},
    middleware,
    routing::{get, post},
    Router,
};
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

    let protected_api_routes = Router::new()
        .route("/api/snapshot/:series", get(api::get_snapshot))
        .route("/api/stream/:series", get(sse::stream_series))
        .route(
            "/api/preferences",
            get(api::get_preferences).put(api::put_preferences),
        )
        .layer(middleware::from_fn_with_state(
            auth_config.clone(),
            auth::require_session_middleware,
        ));

    let auth_routes = Router::new()
        .route("/auth/session", get(auth::session_status))
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .with_state(auth_config);

    protected_api_routes
        .merge(auth_routes)
        .with_state(app_state)
}

fn session_cookie_pair(set_cookie: &str) -> String {
    set_cookie.split(';').next().unwrap_or_default().to_string()
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
async fn protected_api_and_sse_require_authentication() {
    let app = test_app();

    for (method, uri) in [
        (Method::GET, "/api/preferences"),
        (Method::GET, "/api/snapshot/imsa"),
        (Method::GET, "/api/stream/imsa"),
        (Method::PUT, "/api/preferences"),
    ] {
        let body = if method == Method::PUT {
            Body::from(
                serde_json::to_vec(&serde_json::json!({
                    "favourites": [],
                    "selected_series": "imsa"
                }))
                .expect("put payload"),
            )
        } else {
            Body::empty()
        };

        let is_put_preferences = method == Method::PUT && uri == "/api/preferences";
        let mut builder = Request::builder().method(method).uri(uri);
        if is_put_preferences {
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
