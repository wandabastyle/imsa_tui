use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::mpsc::Sender,
    time::{Duration, SystemTime},
};

use directories::ProjectDirs;
use serde::{de::DeserializeOwned, Serialize};

#[derive(Debug, Clone, Default)]
pub struct PersistState {
    pub path: Option<PathBuf>,
    pub last_persisted_hash: Option<u64>,
    pub dirty_since_last_save: bool,
    pub last_save_at: Option<SystemTime>,
}

impl PersistState {
    pub fn new(path: Option<PathBuf>) -> Self {
        Self {
            path,
            last_persisted_hash: None,
            dirty_since_last_save: false,
            last_save_at: None,
        }
    }
}

pub fn data_local_snapshot_path(file_name: &str) -> Option<PathBuf> {
    let dirs = ProjectDirs::from("", "", "imsa_tui")?;
    Some(dirs.data_local_dir().join(file_name))
}

pub fn debounce_elapsed(last_save_at: Option<SystemTime>, debounce: Duration) -> bool {
    match last_save_at {
        Some(last) => last
            .elapsed()
            .map(|elapsed| elapsed >= debounce)
            .unwrap_or(true),
        None => true,
    }
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str::<T>(&text).ok()
}

pub fn write_json_pretty<T: Serialize>(path: &Path, payload: &T) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let encoded = serde_json::to_string_pretty(payload)
        .map_err(|err| io::Error::other(format!("json encode failed: {err}")))?;
    fs::write(path, encoded)
}

#[derive(Debug, Clone)]
pub enum SeriesDebugOutput {
    Silent,
    Stderr,
    Channel(Sender<String>),
}

pub fn log_series_debug(output: &SeriesDebugOutput, prefix: &str, message: impl AsRef<str>) {
    let line = format!("[{prefix}] {}", message.as_ref());
    match output {
        SeriesDebugOutput::Silent => {}
        SeriesDebugOutput::Stderr => eprintln!("{line}"),
        SeriesDebugOutput::Channel(tx) => {
            let _ = tx.send(line);
        }
    }
}
