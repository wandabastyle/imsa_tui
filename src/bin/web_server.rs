// Web server binary entrypoint.
// Supports foreground mode plus daemon lifecycle commands (--daemon/--status/--stop).

use std::{
    env, fs,
    io::{self, ErrorKind},
    net::SocketAddr,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    extract::Path,
    middleware,
    routing::{get, post},
    Router,
};
use directories::ProjectDirs;
use imsa_tui::web::{
    api,
    auth::{self, PasswordState, WebAuthConfig},
    bridge::start_feed_bridge,
    sse,
    state::WebAppState,
    static_files::{self, StaticConfig},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
struct ResolvedAuth {
    access_code: String,
    state: PasswordState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunMode {
    Foreground,
    DaemonParent,
    DaemonChild,
    Stop,
    Status,
}

#[derive(Debug, Serialize, Deserialize)]
struct RuntimeInfo {
    pid: u32,
    local_url: String,
    public_url: Option<String>,
    access_code: String,
    auth_file: Option<String>,
    log_file: Option<String>,
    started_unix_secs: u64,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mode = parse_mode()?;

    match mode {
        RunMode::Stop => {
            stop_daemon()?;
            return Ok(());
        }
        RunMode::Status => {
            print_status()?;
            return Ok(());
        }
        RunMode::DaemonParent => {
            start_daemon_parent()?;
            return Ok(());
        }
        RunMode::Foreground | RunMode::DaemonChild => {}
    }

    run_server(mode).await
}

async fn run_server(mode: RunMode) -> Result<(), Box<dyn std::error::Error>> {
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
        access_code: resolved_auth.access_code.clone(),
        auth_file: auth::stored_auth_path().map(|p| p.display().to_string()),
        log_file: log_path().map(|p| p.display().to_string()),
        started_unix_secs: now_unix_secs(),
    };

    print_startup_info(&runtime_info, resolved_auth.state);

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
        let _ = clear_runtime_files();
    }

    result?;
    Ok(())
}

fn parse_mode() -> Result<RunMode, Box<dyn std::error::Error>> {
    let mut mode = RunMode::Foreground;
    for arg in env::args().skip(1) {
        let next = match arg.as_str() {
            "--daemon" => RunMode::DaemonParent,
            "--run-daemon" => RunMode::DaemonChild,
            "--stop" => RunMode::Stop,
            "--status" => RunMode::Status,
            other => {
                return Err(format!("unknown argument: {other}").into());
            }
        };

        if mode != RunMode::Foreground {
            return Err("use only one mode flag at a time".into());
        }
        mode = next;
    }
    Ok(mode)
}

fn start_daemon_parent() -> Result<(), Box<dyn std::error::Error>> {
    if let Some(pid) = read_pid()? {
        if is_process_running(pid) {
            println!("web_server already running (pid {pid}).");
            print_status()?;
            return Ok(());
        }
        let _ = clear_runtime_files();
    }

    let log_path = log_path().ok_or("unable to resolve log path")?;
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let log_file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let log_file_err = log_file.try_clone()?;

    let exe = env::current_exe()?;
    let mut cmd = Command::new(exe);
    cmd.arg("--run-daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_file_err));

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }

    let child = cmd.spawn()?;
    println!("web_server daemon starting (pid {}).", child.id());

    for _ in 0..80 {
        if let Some(info) = read_runtime_info()? {
            println!("local web UI: {}", info.local_url);
            if let Some(url) = info.public_url.as_ref() {
                println!("public web UI: {url}");
            }
            println!("shared access code: {}", info.access_code);
            if let Some(path) = info.auth_file.as_ref() {
                println!("web auth file: {path}");
            }
            println!("log file: {}", log_path.display());
            println!("stop with: web_server --stop");
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    println!(
        "daemon started but startup info was not written yet. check logs: {}",
        log_path.display()
    );
    Ok(())
}

fn stop_daemon() -> Result<(), Box<dyn std::error::Error>> {
    let Some(pid) = read_pid()? else {
        println!("web_server is not running (no pid file).");
        return Ok(());
    };

    if !is_process_running(pid) {
        println!("stale pid file found; cleaning up.");
        clear_runtime_files()?;
        return Ok(());
    }

    send_signal(pid, libc::SIGTERM)?;
    for _ in 0..50 {
        if !is_process_running(pid) {
            clear_runtime_files()?;
            println!("web_server stopped.");
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    println!("graceful stop timed out; sending SIGKILL.");
    let _ = send_signal(pid, libc::SIGKILL);
    clear_runtime_files()?;
    println!("web_server stopped.");
    Ok(())
}

fn print_status() -> Result<(), Box<dyn std::error::Error>> {
    let pid = read_pid()?;
    let info = read_runtime_info()?;

    match pid {
        Some(pid) if is_process_running(pid) => {
            println!("web_server status: running (pid {pid})");
            if let Some(info) = info {
                println!("local web UI: {}", info.local_url);
                if let Some(url) = info.public_url.as_ref() {
                    println!("public web UI: {url}");
                }
                println!("shared access code: {}", info.access_code);
                if let Some(path) = info.auth_file.as_ref() {
                    println!("web auth file: {path}");
                }
                if let Some(path) = info.log_file.as_ref() {
                    println!("log file: {path}");
                }
            }
        }
        Some(_) => {
            println!("web_server status: not running (stale pid file)");
        }
        None => {
            println!("web_server status: not running");
        }
    }

    Ok(())
}

fn resolve_auth() -> ResolvedAuth {
    let rotate = env_flag("WEBUI_ROTATE_PASSWORD", false);
    let (access_code, state) = auth::load_or_initialize_password(rotate);
    ResolvedAuth { access_code, state }
}

fn print_startup_info(info: &RuntimeInfo, state: PasswordState) {
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
    println!("shared access code: {}", info.access_code);
    if let Some(path) = info.auth_file.as_ref() {
        println!("web auth file: {path}");
    }
    if let Some(url) = info.public_url.as_ref() {
        println!("public web UI: {url}");
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

fn setup_tailscale_funnel(port: u16) -> Option<String> {
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
            return None;
        }
        Err(err) => {
            eprintln!("tailscale command unavailable: {err}");
            return None;
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
                Some(url.to_string())
            } else {
                println!("tailscale funnel status:\n{}", text.trim());
                None
            }
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            eprintln!("tailscale funnel status failed: {}", stderr.trim());
            None
        }
        Err(err) => {
            eprintln!("tailscale command unavailable: {err}");
            None
        }
    }
}

async fn wait_for_shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        match signal(SignalKind::terminate()) {
            Ok(mut sigterm) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = sigterm.recv() => {}
                }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }

    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

fn runtime_dir() -> Option<PathBuf> {
    let dirs = ProjectDirs::from("com", "imsa", "imsa_tui")?;
    Some(dirs.config_dir().to_path_buf())
}

fn pid_path() -> Option<PathBuf> {
    Some(runtime_dir()?.join("web_server.pid"))
}

fn info_path() -> Option<PathBuf> {
    Some(runtime_dir()?.join("web_server.info.toml"))
}

fn log_path() -> Option<PathBuf> {
    Some(runtime_dir()?.join("web_server.log"))
}

fn write_pid(pid: i32) -> Result<(), Box<dyn std::error::Error>> {
    let path = pid_path().ok_or("unable to resolve pid path")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, pid.to_string())?;
    Ok(())
}

fn read_pid() -> Result<Option<i32>, Box<dyn std::error::Error>> {
    let Some(path) = pid_path() else {
        return Ok(None);
    };
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    let pid = text.trim().parse::<i32>().ok();
    Ok(pid)
}

fn write_runtime_info(info: &RuntimeInfo) -> Result<(), Box<dyn std::error::Error>> {
    let path = info_path().ok_or("unable to resolve info path")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let encoded = toml::to_string_pretty(info)?;
    fs::write(path, encoded)?;
    Ok(())
}

fn read_runtime_info() -> Result<Option<RuntimeInfo>, Box<dyn std::error::Error>> {
    let Some(path) = info_path() else {
        return Ok(None);
    };
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    match toml::from_str::<RuntimeInfo>(&text) {
        Ok(info) => Ok(Some(info)),
        Err(_) => Ok(None),
    }
}

fn clear_runtime_files() -> Result<(), Box<dyn std::error::Error>> {
    if let Some(path) = pid_path() {
        let _ = fs::remove_file(path);
    }
    if let Some(path) = info_path() {
        let _ = fs::remove_file(path);
    }
    Ok(())
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn is_process_running(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }

    #[cfg(unix)]
    {
        let rc = unsafe { libc::kill(pid, 0) };
        if rc == 0 {
            return true;
        }
        let err = io::Error::last_os_error();
        return matches!(err.raw_os_error(), Some(code) if code == libc::EPERM);
    }

    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

fn send_signal(pid: i32, signal: i32) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(unix)]
    {
        let rc = unsafe { libc::kill(pid, signal) };
        if rc == 0 {
            return Ok(());
        }
        return Err(io::Error::last_os_error().into());
    }

    #[cfg(not(unix))]
    {
        let _ = (pid, signal);
        Err("signals are not supported on this platform".into())
    }
}
