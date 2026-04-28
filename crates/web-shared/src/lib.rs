use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Series {
    #[default]
    Imsa,
    Nls,
    F1,
    Wec,
    Dhlm,
}

impl Series {
    pub const fn all() -> [Series; 5] {
        [
            Series::Dhlm,
            Series::F1,
            Series::Imsa,
            Series::Nls,
            Series::Wec,
        ]
    }

    pub fn as_key_prefix(self) -> &'static str {
        match self {
            Series::Dhlm => "dhlm",
            Series::Imsa => "imsa",
            Series::Nls => "nls",
            Series::F1 => "f1",
            Series::Wec => "wec",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimingClassColor {
    pub color: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimingHeader {
    pub session_name: String,
    #[serde(default)]
    pub session_type_raw: String,
    pub event_name: String,
    pub track_name: String,
    pub day_time: String,
    pub flag: String,
    pub time_to_go: String,
    #[serde(default)]
    pub class_colors: BTreeMap<String, TimingClassColor>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimingEntry {
    pub position: u32,
    pub car_number: String,
    pub class_name: String,
    pub class_rank: String,
    pub driver: String,
    pub vehicle: String,
    pub team: String,
    pub laps: String,
    pub gap_overall: String,
    pub gap_class: String,
    pub gap_next_in_class: String,
    pub last_lap: String,
    pub best_lap: String,
    pub sector_1: String,
    pub sector_2: String,
    pub sector_3: String,
    pub sector_4: String,
    pub sector_5: String,
    pub best_lap_no: String,
    pub pit: String,
    pub pit_stops: String,
    pub fastest_driver: String,
    pub stable_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeriesSnapshot {
    pub header: TimingHeader,
    pub entries: Vec<TimingEntry>,
    #[serde(default)]
    pub notices: Vec<TimingNotice>,
    pub status: String,
    pub last_error: Option<String>,
    pub last_update_unix_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimingNotice {
    pub id: String,
    pub time: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotResponse {
    pub series: Series,
    pub snapshot: SeriesSnapshot,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NlsLivetickerEntry {
    pub day_label: String,
    pub time_text: String,
    pub message: String,
    pub id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NlsLivetickerResponse {
    #[serde(default)]
    pub entries: Vec<NlsLivetickerEntry>,
    pub last_error: Option<String>,
    pub last_update_unix_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preferences {
    #[serde(default)]
    pub favourites: Vec<String>,
    #[serde(default)]
    pub selected_series: Series,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PutDemoRequest {
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DemoStateResponse {
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginRequest {
    pub access_code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionStateResponse {
    pub authenticated: bool,
}

pub fn canonicalize_class_name(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed == "-" {
        return "-".to_string();
    }

    let mut normalized = String::with_capacity(trimmed.len());
    let mut pending_separator = false;
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_separator && !normalized.is_empty() {
                normalized.push('-');
            }
            normalized.push(ch.to_ascii_uppercase());
            pending_separator = false;
        } else if ch.is_whitespace() || ch == '_' || ch == '-' {
            pending_separator = !normalized.is_empty();
        }
    }

    let canonical = match normalized.as_str() {
        "GTDPRO" => "GTD-PRO".to_string(),
        "PROAM" => "PRO-AM".to_string(),
        "HYPERCAR" => "HYPER".to_string(),
        _ => normalized,
    };

    if canonical.is_empty() {
        "-".to_string()
    } else {
        canonical
    }
}

pub fn class_display_name(name: &str) -> String {
    canonicalize_class_name(name)
}
