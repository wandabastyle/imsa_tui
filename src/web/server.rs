use std::{env, net::SocketAddr, path::PathBuf};

use axum::{
    extract::Path,
    middleware,
    routing::{get, post},
    Router,
};

use super::{
    api,
    auth::{self, PasswordState, WebAuthConfig},
    bridge::start_feed_bridge,
    daemon::RunMode,
    runtime::{
        cleanup_legacy_config_artifacts, cleanup_stale_profile_artifacts, env_flag, log_path,
        now_unix_secs, parse_boolish, setup_tailscale_funnel, static_source_label,
        wait_for_shutdown_signal, write_pid, write_runtime_info, RuntimeInfo,
    },
    sse,
    state::WebAppState,
    static_files::{self, StaticConfig, StaticSource},
};

#[derive(Debug, Clone)]
struct ResolvedAuth {
    access_code_hash: String,
    one_time_access_code: Option<String>,
    state: PasswordState,
}

#[derive(Debug, Clone, Copy)]
struct AuthRuntimeOptions {
    cookie_secure: bool,
}

pub async fn run(mode: RunMode) -> Result<(), Box<dyn std::error::Error>> {
    let bind_addr = env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0".to_string());
    let bind_port = env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(8080);
    let static_root = env::var("WEB_DIST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("web/build"));
    cleanup_legacy_config_artifacts();
    cleanup_stale_profile_artifacts();
    let resolved_auth = resolve_auth();
    let auth_options = resolve_auth_options();

    let app_state = WebAppState::with_profile_cookie_secure(auth_options.cookie_secure);
    let feed_controller = start_feed_bridge(app_state.clone());
    app_state.set_feed_controller(feed_controller.clone());

    let static_config = resolve_static_config(static_root);

    let auth_config = WebAuthConfig::new(
        resolved_auth.access_code_hash.clone(),
        auth_options.cookie_secure,
    );

    let protected_api_routes = Router::new()
        .route("/api/snapshot/{series}", get(api::get_snapshot))
        .route("/api/stream/{series}", get(sse::stream_series))
        .route(
            "/api/preferences",
            get(api::get_preferences).put(api::put_preferences),
        )
        .route(
            "/api/demo",
            get(api::get_demo_state).put(api::put_demo_state),
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
        .with_state(auth_config.clone());

    let app_routes = Router::new()
        .route(
            "/",
            get({
                let static_config = static_config.clone();
                move || {
                    let static_config = static_config.clone();
                    async move { static_files::index(static_config).await }
                }
            }),
        )
        .route(
            "/*path",
            get({
                let static_config = static_config.clone();
                move |Path(path): Path<String>| {
                    let static_config = static_config.clone();
                    async move { static_files::asset_or_index(static_config, &path).await }
                }
            }),
        );

    let public_routes = Router::new()
        .route("/healthz", get(api::healthz))
        .route("/readyz", get(api::readyz));

    let app = public_routes
        .merge(protected_api_routes)
        .merge(auth_routes)
        .merge(app_routes)
        .with_state(app_state);

    let addr = format!("{bind_addr}:{bind_port}").parse::<SocketAddr>()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let local_url = format!("http://{}", listener.local_addr()?);

    let public_url = if env_flag("WEBUI_AUTO_FUNNEL", true) {
        setup_tailscale_funnel(bind_port)
    } else {
        None
    };

    let runtime_info = RuntimeInfo {
        pid: std::process::id(),
        local_url: local_url.clone(),
        public_url,
        auth_file: auth::stored_auth_path().map(|p| p.display().to_string()),
        log_file: log_path().map(|p| p.display().to_string()),
        started_unix_secs: now_unix_secs(),
    };

    print_startup_info(
        &runtime_info,
        resolved_auth.one_time_access_code.as_deref(),
        resolved_auth.state,
        auth_options,
        static_config.source,
    );

    if mode == RunMode::DaemonChild {
        write_runtime_info(&runtime_info)?;
        write_pid(runtime_info.pid as i32)?;
    }

    let shutdown = async move {
        wait_for_shutdown_signal().await;
        feed_controller.stop_all();
    };

    let result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await;

    if mode == RunMode::DaemonChild {
        let _ = super::runtime::clear_runtime_files();
    }

    result?;
    Ok(())
}

fn resolve_auth() -> ResolvedAuth {
    let rotate = env_flag("WEBUI_ROTATE_PASSWORD", false);
    let resolved = auth::load_or_initialize_password(rotate);
    ResolvedAuth {
        access_code_hash: resolved.access_code_hash,
        one_time_access_code: resolved.one_time_access_code,
        state: resolved.state,
    }
}

fn resolve_static_config(root_dir: PathBuf) -> StaticConfig {
    let prefer_embedded = env_flag("WEBUI_EMBED_UI", true);

    #[cfg(not(feature = "embed-ui"))]
    {
        if prefer_embedded {
            eprintln!(
                "WEBUI_EMBED_UI=1 requested, but binary was built without the embed-ui feature; using disk assets."
            );
        }
    }

    StaticConfig::new(root_dir, prefer_embedded)
}

fn resolve_auth_options() -> AuthRuntimeOptions {
    let cookie_secure = match env::var("WEBUI_COOKIE_SECURE") {
        Ok(value) => parse_boolish(&value).unwrap_or(false),
        Err(_) => env_flag("WEBUI_AUTO_FUNNEL", true),
    };

    AuthRuntimeOptions { cookie_secure }
}

fn print_startup_info(
    info: &RuntimeInfo,
    one_time_access_code: Option<&str>,
    state: PasswordState,
    options: AuthRuntimeOptions,
    static_source: StaticSource,
) {
    println!("web server listening on {}", info.local_url);
    match state {
        PasswordState::Loaded => println!("web auth enabled (loaded saved access code)."),
        PasswordState::GeneratedPersisted => {
            println!("web auth enabled (generated and saved new access code).")
        }
        PasswordState::GeneratedEphemeral => {
            println!("web auth enabled (generated access code but could not save).")
        }
    }
    if let Some(access_code) = one_time_access_code {
        println!("shared access code: {access_code}");
    }
    println!("session cookie secure: {}", options.cookie_secure);
    println!("web assets source: {}", static_source_label(static_source));
    if let Some(path) = info.auth_file.as_ref() {
        println!("web auth file: {path}");
    }
    if let Some(url) = info.public_url.as_ref() {
        println!("public web UI: {url}");
    }
}
