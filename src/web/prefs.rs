// Persistence for per-profile web preferences.

use std::{
    collections::HashSet,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::favourites;
use crate::timing::Series;

const PROFILE_RETENTION_DAYS_DEFAULT: u64 = 180;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Preferences {
    #[serde(default)]
    pub favourites: HashSet<String>,
    #[serde(default)]
    pub selected_series: Series,
}

pub fn load_preferences(profile_id: &str) -> Preferences {
    let Some(path) = preferences_path(profile_id) else {
        return Preferences::default();
    };
    let Ok(text) = fs::read_to_string(path) else {
        return Preferences::default();
    };

    normalize_preferences(toml::from_str::<Preferences>(&text).unwrap_or_default())
}

pub fn save_preferences(profile_id: &str, preferences: &Preferences) -> Result<(), String> {
    let Some(path) = preferences_path(profile_id) else {
        return Err("unable to resolve config directory".to_string());
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create config directory failed: {e}"))?;
    }

    let normalized = normalize_preferences(preferences.clone());
    let encoded = toml::to_string_pretty(&normalized)
        .map_err(|e| format!("encode preferences failed: {e}"))?;
    fs::write(path, encoded).map_err(|e| format!("write preferences failed: {e}"))
}

pub fn reset_preferences(profile_id: &str) -> Result<Preferences, String> {
    let Some(path) = preferences_path(profile_id) else {
        return Err("unable to resolve config directory".to_string());
    };

    match fs::remove_file(path) {
        Ok(_) => {}
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => return Err(format!("reset preferences failed: {err}")),
    }

    Ok(Preferences::default())
}

pub fn cleanup_stale_profiles_default() -> Result<usize, String> {
    cleanup_stale_profiles(PROFILE_RETENTION_DAYS_DEFAULT)
}

fn cleanup_stale_profiles(retention_days: u64) -> Result<usize, String> {
    let Some(dir) = profiles_dir() else {
        return Err("unable to resolve profile directory".to_string());
    };

    let retention = Duration::from_secs(retention_days.saturating_mul(24 * 60 * 60));
    let now = SystemTime::now();

    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(0),
        Err(err) => return Err(format!("read profile directory failed: {err}")),
    };

    let mut removed = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        if !is_profile_file(&path) {
            continue;
        }

        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        let Ok(age) = now.duration_since(modified) else {
            continue;
        };
        if age <= retention {
            continue;
        }

        if fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }

    Ok(removed)
}

fn preferences_path(profile_id: &str) -> Option<PathBuf> {
    if !valid_profile_id(profile_id) {
        return None;
    }
    let profiles_dir = profiles_dir()?;
    Some(profiles_dir.join(format!("{profile_id}.toml")))
}

fn valid_profile_id(value: &str) -> bool {
    let len = value.len();
    if !(8..=128).contains(&len) {
        return false;
    }

    value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

fn profiles_dir() -> Option<PathBuf> {
    let dirs = ProjectDirs::from("", "", "imsa_tui")?;
    Some(dirs.data_local_dir().join("profiles"))
}

fn is_profile_file(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("toml")
}

fn normalize_preferences(mut preferences: Preferences) -> Preferences {
    preferences.favourites = favourites::normalize_favourites(preferences.favourites.into_iter());
    preferences
}
