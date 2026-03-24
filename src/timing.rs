use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Series {
    Imsa,
    Nls,
    F1,
}

impl Default for Series {
    fn default() -> Self {
        Self::Imsa
    }
}

impl Series {
    pub fn label(self) -> &'static str {
        match self {
            Series::Imsa => "IMSA",
            Series::Nls => "NLS",
            Series::F1 => "F1",
        }
    }

    pub fn toggle(self) -> Self {
        match self {
            Series::Imsa => Series::Nls,
            Series::Nls => Series::F1,
            Series::F1 => Series::Imsa,
        }
    }

    pub fn as_key_prefix(self) -> &'static str {
        match self {
            Series::Imsa => "imsa",
            Series::Nls => "nls",
            Series::F1 => "f1",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TimingHeader {
    pub session_name: String,
    pub event_name: String,
    pub track_name: String,
    pub day_time: String,
    pub flag: String,
    pub time_to_go: String,
}

#[derive(Debug, Clone, Default)]
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
