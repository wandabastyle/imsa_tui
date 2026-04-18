use std::{
    collections::{HashMap, HashSet},
    fs,
    path::PathBuf,
};

use directories::ProjectDirs;
use serde::{Deserialize, Deserializer, Serialize};

use crate::{favourites, timing::Series};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct AppConfig {
    pub(crate) favourites: HashSet<String>,
    #[serde(default)]
    pub(crate) selected_series: Series,
    #[serde(default, deserialize_with = "deserialize_dismissed_notice_keys")]
    pub(crate) dismissed_notice_keys: HashMap<String, u64>,
}

fn deserialize_dismissed_notice_keys<'de, D>(
    deserializer: D,
) -> Result<HashMap<String, u64>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum DismissedNoticeKeys {
        Keys(Vec<String>),
        KeyTimes(HashMap<String, u64>),
    }

    let parsed = Option::<DismissedNoticeKeys>::deserialize(deserializer)?;
    Ok(match parsed {
        Some(DismissedNoticeKeys::Keys(keys)) => keys.into_iter().map(|key| (key, 0)).collect(),
        Some(DismissedNoticeKeys::KeyTimes(key_times)) => key_times,
        None => HashMap::new(),
    })
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
