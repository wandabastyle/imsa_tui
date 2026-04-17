// Shared timing domain model used by feed workers, TUI rendering, and web API serialization.

use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Series {
    #[default]
    Imsa,
    Nls,
    F1,
    Wec,
}

impl Series {
    pub const fn all() -> [Series; 4] {
        [Series::Imsa, Series::Nls, Series::F1, Series::Wec]
    }

    pub fn label(self) -> &'static str {
        match self {
            Series::Imsa => "IMSA",
            Series::Nls => "NLS",
            Series::F1 => "F1",
            Series::Wec => "WEC",
        }
    }

    pub fn as_key_prefix(self) -> &'static str {
        match self {
            Series::Imsa => "imsa",
            Series::Nls => "nls",
            Series::F1 => "f1",
            Series::Wec => "wec",
        }
    }
}

impl FromStr for Series {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "imsa" => Ok(Self::Imsa),
            "nls" => Ok(Self::Nls),
            "f1" => Ok(Self::F1),
            "wec" => Ok(Self::Wec),
            other => Err(format!("unsupported series: {other}")),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TimingHeader {
    pub session_name: String,
    pub event_name: String,
    pub track_name: String,
    pub day_time: String,
    pub flag: String,
    pub time_to_go: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

#[derive(Debug, Clone)]
pub enum TimingMessage {
    Status {
        source_id: u64,
        text: String,
    },
    Error {
        source_id: u64,
        text: String,
    },
    Snapshot {
        source_id: u64,
        header: TimingHeader,
        entries: Vec<TimingEntry>,
    },
}
