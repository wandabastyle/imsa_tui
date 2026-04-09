use std::{env, net::SocketAddr, path::PathBuf, process::Command};

use axum::{
    extract::Path,
    middleware,
    routing::get,
    Router,
};
use imsa_tui::web::{
    api,
    auth::{self, PasswordState, WebAuthConfig},
    bridge::start_feed_bridge,
    sse,
    state::WebAppState,
    static_files::{self, StaticConfig},
};

#[derive(Debug, Clone)]
struct ResolvedAuth {
    access_code: String,
    state: PasswordState,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bind_addr = env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0".to_string());
    let bind_port = env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(8080);
    let static_root = env::var("WEB_DIST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("web/build"));
    let resolved_auth = resolve_auth();

    let app_state = WebAppState::new();
    let feed_controller = start_feed_bridge(app_state.clone());

    let static_config = StaticConfig {
        root_dir: static_root,
    };

    let auth_config = WebAuthConfig::new(resolved_auth.access_code.clone());

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
        .route("/auth/login", axum::routing::post(auth::login))
        .route("/auth/logout", axum::routing::post(auth::logout))
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
    println!("web server listening on http://{}", listener.local_addr()?);
    match resolved_auth.state {
        PasswordState::Loaded => println!(
            "web auth enabled (loaded saved access code)."
        ),
        PasswordState::GeneratedPersisted => {
            println!("web auth enabled (generated and saved new access code).")
        }
        PasswordState::GeneratedEphemeral => {
            println!("web auth enabled (generated access code but could not save).")
        }
    }
    println!("shared access code: {}", resolved_auth.access_code);
    if let Some(path) = auth::stored_auth_path() {
        println!("web auth file: {}", path.display());
    }

    if env_flag("WEBUI_AUTO_FUNNEL", true) {
        // This keeps sharing friction low: `cargo run --bin web_server` can
        // immediately expose the authenticated web UI for friends.
        setup_tailscale_funnel(bind_port);
    }

    let shutdown = async move {
        let _ = tokio::signal::ctrl_c().await;
        feed_controller.stop_all();
    };

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await?;

    Ok(())
}

fn resolve_auth() -> ResolvedAuth {
    let rotate = env_flag("WEBUI_ROTATE_PASSWORD", false);
    let (access_code, state) = auth::load_or_initialize_password(rotate);
    ResolvedAuth {
        access_code,
        state,
    }
}

fn env_flag(name: &str, default: bool) -> bool {
    match env::var(name) {
        Ok(value) => {
            let v = value.trim().to_ascii_lowercase();
            !(v == "0" || v == "false" || v == "off" || v == "no")
        }
        Err(_) => default,
    }
}

fn setup_tailscale_funnel(port: u16) {
    let target = format!("http://127.0.0.1:{port}");
    let start = Command::new("tailscale")
        .args(["funnel", "--bg", &target])
        .output();

    match start {
        Ok(output) if output.status.success() => {
            println!("tailscale funnel enabled for {target}");
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!("tailscale funnel setup failed: {}", stderr.trim());
            return;
        }
        Err(err) => {
            eprintln!("tailscale command unavailable: {err}");
            return;
        }
    }

    let status = Command::new("tailscale")
        .args(["funnel", "status"])
        .output();

    match status {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout);
            if let Some(url) = text.split_whitespace().find(|token| token.starts_with("https://"))
            {
                println!("public web UI: {url}");
            } else {
                println!("tailscale funnel status:\n{}", text.trim());
            }
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!("tailscale funnel status failed: {}", stderr.trim());
        }
        Err(err) => {
            eprintln!("tailscale command unavailable: {err}");
        }
    }
}
