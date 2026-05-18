use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(
    export,
    rename_all = "lowercase",
    export_to = "../../web/src/lib/generated"
)]
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../web/src/lib/generated")]
pub struct TimingClassColor {
    pub color: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../web/src/lib/generated")]
pub struct TimingHeader {
    pub session_name: String,
    #[serde(default)]
    pub session_type_raw: String,
    pub event_name: String,
    #[serde(default)]
    pub event_id: String,
    pub track_name: String,
    pub day_time: String,
    pub flag: String,
    pub time_to_go: String,
    #[serde(default)]
    pub class_colors: BTreeMap<String, TimingClassColor>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../web/src/lib/generated")]
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../web/src/lib/generated")]
pub struct SeriesSnapshot {
    pub header: TimingHeader,
    pub entries: Vec<TimingEntry>,
    #[serde(default)]
    pub notices: Vec<TimingNotice>,
    pub status: String,
    pub last_error: Option<String>,
    pub last_update_unix_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../web/src/lib/generated")]
pub struct TimingNotice {
    pub id: String,
    pub time: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../web/src/lib/generated")]
pub struct SnapshotResponse {
    pub series: Series,
    pub snapshot: SeriesSnapshot,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../web/src/lib/generated")]
pub struct NlsLivetickerEntry {
    pub day_label: String,
    pub time_text: String,
    pub message: String,
    pub id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../web/src/lib/generated")]
pub struct NlsLivetickerResponse {
    #[serde(default)]
    pub entries: Vec<NlsLivetickerEntry>,
    pub last_error: Option<String>,
    pub last_update_unix_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../web/src/lib/generated")]
pub struct Preferences {
    #[serde(default)]
    pub favourites: Vec<String>,
    #[serde(default)]
    pub selected_series: Series,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../web/src/lib/generated")]
pub struct PutDemoRequest {
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../web/src/lib/generated")]
pub struct DemoStateResponse {
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../web/src/lib/generated")]
pub struct LoginRequest {
    pub access_code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../web/src/lib/generated")]
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

#[cfg(test)]
mod tests {
    use ts_rs::TS;

    #[test]
    fn export_types() {
        // Export all TypeScript types to the configured directory
        // This test will generate the TypeScript bindings when run with:
        // cargo test -p web-shared export_types -- --nocapture

        let out_dir = std::path::PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../web/src/lib/generated"
        ));

        // Ensure the output directory exists
        std::fs::create_dir_all(&out_dir).expect("Failed to create output directory");

        // Define the order of types - dependencies first
        let types: Vec<(&str, Box<dyn Fn() -> Result<String, ts_rs::ExportError>>)> = vec![
            ("Series", Box::new(|| crate::Series::export_to_string())),
            (
                "TimingClassColor",
                Box::new(|| crate::TimingClassColor::export_to_string()),
            ),
            (
                "TimingHeader",
                Box::new(|| crate::TimingHeader::export_to_string()),
            ),
            (
                "TimingEntry",
                Box::new(|| crate::TimingEntry::export_to_string()),
            ),
            (
                "TimingNotice",
                Box::new(|| crate::TimingNotice::export_to_string()),
            ),
            (
                "SeriesSnapshot",
                Box::new(|| crate::SeriesSnapshot::export_to_string()),
            ),
            (
                "SnapshotResponse",
                Box::new(|| crate::SnapshotResponse::export_to_string()),
            ),
            (
                "NlsLivetickerEntry",
                Box::new(|| crate::NlsLivetickerEntry::export_to_string()),
            ),
            (
                "NlsLivetickerResponse",
                Box::new(|| crate::NlsLivetickerResponse::export_to_string()),
            ),
            (
                "Preferences",
                Box::new(|| crate::Preferences::export_to_string()),
            ),
            (
                "PutDemoRequest",
                Box::new(|| crate::PutDemoRequest::export_to_string()),
            ),
            (
                "DemoStateResponse",
                Box::new(|| crate::DemoStateResponse::export_to_string()),
            ),
            (
                "LoginRequest",
                Box::new(|| crate::LoginRequest::export_to_string()),
            ),
            (
                "SessionStateResponse",
                Box::new(|| crate::SessionStateResponse::export_to_string()),
            ),
        ];

        // Create a consolidated web-shared.ts file
        let mut consolidated = String::from("// This file was generated by [ts-rs](https://github.com/Aleph-Alpha/ts-rs). Do not edit this file manually.\n\n");

        for (_, get_content) in &types {
            let content = get_content().expect("Failed to generate content");
            // Remove the header comment from individual files to avoid duplication
            let content = content.replace("// This file was generated by [ts-rs](https://github.com/Aleph-Alpha/ts-rs). Do not edit this file manually.\n\n", "");
            consolidated.push_str(&content);
            consolidated.push('\n');
        }

        let consolidated_path = out_dir.join("web-shared.ts");
        std::fs::write(&consolidated_path, consolidated).expect("Failed to write web-shared.ts");
        println!("Generated consolidated: {:?}", consolidated_path);

        // List the generated files
        println!("\nChecking output directory: {:?}", out_dir);
        let entries = std::fs::read_dir(&out_dir).expect("Failed to read output directory");
        let count = entries.count();
        println!("Found {} entries in output directory", count);

        println!(
            "All TypeScript types exported successfully to: {:?}",
            out_dir
        );
    }
}
