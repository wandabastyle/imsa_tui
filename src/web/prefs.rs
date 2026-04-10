// Persistence for shared web preferences (global favourites + default series).

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

pub fn load_preferences() -> Preferences {
    let Some(path) = preferences_path() else {
        return Preferences::default();
    };
    let Ok(text) = fs::read_to_string(path) else {
        return Preferences::default();
    };

    toml::from_str::<Preferences>(&text).unwrap_or_default()
}

pub fn save_preferences(preferences: &Preferences) -> Result<(), String> {
    let Some(path) = preferences_path() else {
        return Err("unable to resolve config directory".to_string());
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create config directory failed: {e}"))?;
    }

    let encoded = toml::to_string_pretty(preferences)
        .map_err(|e| format!("encode preferences failed: {e}"))?;
    fs::write(path, encoded).map_err(|e| format!("write preferences failed: {e}"))
}

fn preferences_path() -> Option<PathBuf> {
    let dirs = ProjectDirs::from("", "", "imsa_tui")?;
    Some(dirs.config_dir().join("config.toml"))
}
