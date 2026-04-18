use std::{collections::HashSet, fs, path::PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use crate::{favourites, timing::Series};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct AppConfig {
    pub(crate) favourites: HashSet<String>,
    #[serde(default)]
    pub(crate) selected_series: Series,
}

fn config_path() -> Option<PathBuf> {
    let dirs = ProjectDirs::from("", "", "imsa_tui")?;
    Some(dirs.config_dir().join("config.toml"))
}

pub(crate) fn load_config() -> AppConfig {
    let Some(path) = config_path() else {
        return AppConfig::default();
    };

    let Ok(text) = fs::read_to_string(path) else {
        return AppConfig::default();
    };

    toml::from_str::<AppConfig>(&text).unwrap_or_default()
}

pub(crate) fn save_config(config: &AppConfig) -> Result<(), String> {
    let Some(path) = config_path() else {
        return Err("unable to resolve platform config directory".to_string());
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create config directory failed: {e}"))?;
    }

    let mut filtered = config.clone();
    filtered.favourites = favourites::normalize_favourites(filtered.favourites);
    let encoded =
        toml::to_string_pretty(&filtered).map_err(|e| format!("encode config failed: {e}"))?;
    fs::write(path, encoded).map_err(|e| format!("write config failed: {e}"))
}
