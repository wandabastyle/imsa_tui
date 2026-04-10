// Persistence for per-profile web preferences.

use std::{collections::HashSet, fs, path::PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::timing::Series;

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

    toml::from_str::<Preferences>(&text).unwrap_or_default()
}

pub fn save_preferences(profile_id: &str, preferences: &Preferences) -> Result<(), String> {
    let Some(path) = preferences_path(profile_id) else {
        return Err("unable to resolve config directory".to_string());
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create config directory failed: {e}"))?;
    }

    let encoded = toml::to_string_pretty(preferences)
        .map_err(|e| format!("encode preferences failed: {e}"))?;
    fs::write(path, encoded).map_err(|e| format!("write preferences failed: {e}"))
}

fn preferences_path(profile_id: &str) -> Option<PathBuf> {
    if !valid_profile_id(profile_id) {
        return None;
    }
    let dirs = ProjectDirs::from("", "", "imsa_tui")?;
    Some(
        dirs.data_local_dir()
            .join("profiles")
            .join(format!("{profile_id}.toml")),
    )
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
