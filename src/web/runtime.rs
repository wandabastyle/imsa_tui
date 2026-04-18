use std::{
    env, fs,
    io::{self, ErrorKind},
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use super::{prefs, static_files::StaticSource};

#[derive(Debug, Serialize, Deserialize)]
pub struct RuntimeInfo {
    pub pid: u32,
    pub local_url: String,
    pub public_url: Option<String>,
    pub auth_file: Option<String>,
    pub log_file: Option<String>,
    pub started_unix_secs: u64,
}

pub fn static_source_label(source: StaticSource) -> &'static str {
    match source {
        StaticSource::Disk => "disk (WEB_DIST_DIR)",
        #[cfg(feature = "embed-ui")]
        StaticSource::Embedded => "embedded (build-time web/build)",
    }
}

pub fn env_flag(name: &str, default: bool) -> bool {
    match env::var(name) {
        Ok(value) => parse_boolish(&value).unwrap_or(default),
        Err(_) => default,
    }
}

pub fn parse_boolish(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "on" | "yes" => Some(true),
        "0" | "false" | "off" | "no" => Some(false),
        _ => None,
    }
}

pub fn setup_tailscale_funnel(port: u16) -> Option<String> {
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
            if let Some(url) = text
                .split_whitespace()
                .find(|token| token.starts_with("https://"))
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

pub async fn wait_for_shutdown_signal() {
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

pub fn runtime_dir() -> Option<PathBuf> {
    let dirs = ProjectDirs::from("", "", "imsa_tui")?;
    Some(dirs.data_local_dir().to_path_buf())
}

pub fn cleanup_legacy_config_artifacts() {
    let Some(dirs) = ProjectDirs::from("", "", "imsa_tui") else {
        return;
    };
    let legacy_dir = dirs.config_dir();
    for name in [
        "web_auth.toml",
        "web_server.pid",
        "web_server.info.toml",
        "web_server.log",
    ] {
        let path = legacy_dir.join(name);
        match fs::remove_file(&path) {
            Ok(_) => {}
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => eprintln!(
                "failed to remove legacy web artifact {}: {err}",
                path.display()
            ),
        }
    }
}

pub fn cleanup_stale_profile_artifacts() {
    match prefs::cleanup_stale_profiles_default() {
        Ok(removed) if removed > 0 => {
            println!("removed {removed} stale profile file(s) from data-local storage");
        }
        Ok(_) => {}
        Err(err) => {
            eprintln!("failed to clean up stale profile files: {err}");
        }
    }
}

pub fn pid_path() -> Option<PathBuf> {
    Some(runtime_dir()?.join("web_server.pid"))
}

pub fn info_path() -> Option<PathBuf> {
    Some(runtime_dir()?.join("web_server.info.toml"))
}

pub fn log_path() -> Option<PathBuf> {
    Some(runtime_dir()?.join("web_server.log"))
}

pub fn write_pid(pid: i32) -> Result<(), Box<dyn std::error::Error>> {
    let path = pid_path().ok_or("unable to resolve pid path")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, pid.to_string())?;
    Ok(())
}

pub fn read_pid() -> Result<Option<i32>, Box<dyn std::error::Error>> {
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

pub fn write_runtime_info(info: &RuntimeInfo) -> Result<(), Box<dyn std::error::Error>> {
    let path = info_path().ok_or("unable to resolve info path")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let encoded = toml::to_string_pretty(info)?;
    fs::write(path, encoded)?;
    Ok(())
}

pub fn read_runtime_info() -> Result<Option<RuntimeInfo>, Box<dyn std::error::Error>> {
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

pub fn clear_runtime_files() -> Result<(), Box<dyn std::error::Error>> {
    if let Some(path) = pid_path() {
        let _ = fs::remove_file(path);
    }
    if let Some(path) = info_path() {
        let _ = fs::remove_file(path);
    }
    Ok(())
}

pub fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn is_process_running(pid: i32) -> bool {
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
        matches!(err.raw_os_error(), Some(code) if code == libc::EPERM)
    }

    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

pub fn send_signal(pid: i32, signal: i32) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(unix)]
    {
        let rc = unsafe { libc::kill(pid, signal) };
        if rc == 0 {
            return Ok(());
        }
        Err(io::Error::last_os_error().into())
    }

    #[cfg(not(unix))]
    {
        let _ = (pid, signal);
        Err("signals are not supported on this platform".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_paths_use_data_local_dir() {
        let dirs = ProjectDirs::from("", "", "imsa_tui").expect("project dirs");
        let base = dirs.data_local_dir().to_path_buf();

        assert_eq!(runtime_dir(), Some(base.clone()));
        assert_eq!(pid_path(), Some(base.join("web_server.pid")));
        assert_eq!(info_path(), Some(base.join("web_server.info.toml")));
        assert_eq!(log_path(), Some(base.join("web_server.log")));
    }
}
