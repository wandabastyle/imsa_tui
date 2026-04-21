// IMSA feed adapter: polls JSON/JSONP endpoints and normalizes rows into shared timing structs.

mod jsonp;
mod parser;
mod snapshot;

use std::{
    sync::mpsc::{Receiver, Sender},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    snapshot_runtime::derive_session_identifier,
    timing::{TimingEntry, TimingHeader, TimingMessage},
    timing_persist::{debounce_elapsed, log_series_debug, PersistState, SeriesDebugOutput},
};

use self::{
    parser::{
        fetch_snapshot, is_parsed_entry_transponder_placeholder,
        normalize_class_name as normalize_class_name_impl, FetchedSnapshot,
    },
    snapshot::{
        flush_dirty_snapshot_on_shutdown, imsa_snapshot_path, meaningful_snapshot_fingerprint,
        persist_snapshot, persist_snapshot_if_dirty, restore_snapshot_from_disk,
    },
};

#[cfg(test)]
use self::parser::{is_transponder_placeholder, parse_entry};

const RESULTS_URL: &str = "https://dcqsrdkhg933g.cloudfront.net/RaceResults_JSONP.json";
const RESULTS_CALLBACK: &str = "jsonpRaceResults";
const RACE_DATA_URL: &str = "https://dcqsrdkhg933g.cloudfront.net/RaceData_JSONP.json";
const RACE_DATA_CALLBACK: &str = "jsonpRaceData";
pub const POLL_INTERVAL: Duration = Duration::from_millis(5000);
const SNAPSHOT_SAVE_DEBOUNCE: Duration = Duration::from_secs(180);
const WAITING_NEXT_SESSION_STATUS: &str = "Waiting for next session feed";

pub type ImsaDebugOutput = SeriesDebugOutput;

#[derive(Debug, Clone)]
struct ImsaSnapshot {
    header: TimingHeader,
    entries: Vec<TimingEntry>,
    session_id: Option<String>,
    fingerprint: u64,
    raw_results_payload: Option<Value>,
    raw_race_data_payload: Option<Value>,
}

#[derive(Debug, Clone)]
struct ImsaRuntimeState {
    last_good_live_snapshot: Option<ImsaSnapshot>,
    persist: PersistState,
    last_session_id: Option<String>,
    last_classification: Option<PayloadClassification>,
    debug_output: ImsaDebugOutput,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ImsaSnapshotExtra {
    raw_results_payload: Option<Value>,
    raw_race_data_payload: Option<Value>,
}

#[cfg(test)]
type PersistedImsaSnapshot =
    crate::adapters::insights::snapshot::PersistedSnapshot<ImsaSnapshotExtra>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PayloadClassification {
    Placeholder,
    Real,
}

impl ImsaRuntimeState {
    fn new(debug_output: ImsaDebugOutput) -> Self {
        Self {
            last_good_live_snapshot: None,
            persist: PersistState::new(imsa_snapshot_path()),
            last_session_id: None,
            last_classification: None,
            debug_output,
        }
    }

    #[cfg(test)]
    fn from_parts(persist_path: Option<std::path::PathBuf>) -> Self {
        Self {
            persist: PersistState::new(persist_path),
            ..Self::new(ImsaDebugOutput::Silent)
        }
    }
}

pub fn polling_worker(tx: Sender<TimingMessage>, source_id: u64, stop_rx: Receiver<()>) {
    polling_worker_with_debug(tx, source_id, stop_rx, ImsaDebugOutput::Silent)
}

pub fn polling_worker_with_debug(
    tx: Sender<TimingMessage>,
    source_id: u64,
    stop_rx: Receiver<()>,
    debug_output: ImsaDebugOutput,
) {
    let client = match Client::builder()
        .timeout(Duration::from_secs(12))
        .brotli(true)
        .gzip(true)
        .deflate(true)
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            let _ = tx.send(TimingMessage::Error {
                source_id,
                text: format!("client init failed: {e}"),
            });
            return;
        }
    };

    let mut runtime = ImsaRuntimeState::new(debug_output);
    restore_snapshot_from_disk(&mut runtime, &tx, source_id);
    let _ = tx.send(TimingMessage::Status {
        source_id,
        text: "[SNAPSHOT] Restored from saved data".to_string(),
    });

    loop {
        if stop_rx.try_recv().is_ok() {
            flush_dirty_snapshot_on_shutdown(&mut runtime);
            break;
        }

        let _ = tx.send(TimingMessage::Status {
            source_id,
            text: "Fetching IMSA live timing...".to_string(),
        });

        match fetch_snapshot(
            &client,
            RESULTS_URL,
            RESULTS_CALLBACK,
            RACE_DATA_URL,
            RACE_DATA_CALLBACK,
            now_millis(),
        ) {
            Ok(fetched) => {
                handle_fetched_snapshot(&tx, source_id, &mut runtime, fetched);
            }
            Err(err) => {
                let _ = tx.send(TimingMessage::Error {
                    source_id,
                    text: err,
                });
            }
        }

        if stop_rx.recv_timeout(POLL_INTERVAL).is_ok() {
            flush_dirty_snapshot_on_shutdown(&mut runtime);
            break;
        }
    }
}

fn log_debug(output: &ImsaDebugOutput, message: String) {
    log_series_debug(output, "IMSA", message);
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_millis()
}

fn handle_fetched_snapshot(
    tx: &Sender<TimingMessage>,
    source_id: u64,
    runtime: &mut ImsaRuntimeState,
    fetched: FetchedSnapshot,
) {
    let classification = classify_payload(
        &fetched.results_root,
        &fetched.race_data_root,
        &fetched.entries,
    );
    if runtime.last_classification != Some(classification) {
        let message = match runtime.last_classification {
            Some(previous) => format!(
                "IMSA payload classification changed: {} -> {}.",
                payload_classification_label(previous),
                payload_classification_label(classification)
            ),
            None => format!(
                "IMSA payload classified as {}.",
                payload_classification_label(classification)
            ),
        };
        log_debug(&runtime.debug_output, message);
    }
    runtime.last_classification = Some(classification);

    let session_complete = is_session_complete(&fetched.race_data_root, &fetched.header);

    if classification == PayloadClassification::Placeholder {
        if runtime.last_good_live_snapshot.is_some() {
            let _ = tx.send(TimingMessage::Status {
                source_id,
                text: WAITING_NEXT_SESSION_STATUS.to_string(),
            });
        }

        if session_complete {
            persist_snapshot_if_dirty(runtime, "session complete");
        }
        return;
    }

    let session_id = derive_session_identifier(&fetched.header);
    let fingerprint = meaningful_snapshot_fingerprint(&fetched.header, &fetched.entries);
    let previous_snapshot = runtime.last_good_live_snapshot.as_ref();
    let dirty_reason =
        dirty_transition_reason(previous_snapshot, &fetched.header, &fetched.entries);
    let snapshot = ImsaSnapshot {
        header: fetched.header,
        entries: fetched.entries,
        session_id: session_id.clone(),
        fingerprint,
        raw_results_payload: Some(fetched.results_root),
        raw_race_data_payload: Some(fetched.race_data_root),
    };

    let first_real_of_session = session_id.is_some() && session_id != runtime.last_session_id;
    let materially_changed = previous_snapshot
        .map(|prev| prev.fingerprint != snapshot.fingerprint)
        .unwrap_or(true);

    runtime.last_good_live_snapshot = Some(snapshot.clone());
    runtime.last_session_id = session_id;

    let _ = tx.send(TimingMessage::Snapshot {
        source_id,
        header: snapshot.header.clone(),
        entries: snapshot.entries.clone(),
    });

    if materially_changed {
        let was_dirty = runtime.persist.dirty_since_last_save;
        runtime.persist.dirty_since_last_save = true;
        if !was_dirty {
            log_debug(
                &runtime.debug_output,
                format!("IMSA snapshot marked dirty ({dirty_reason})."),
            );
        }
    }

    let never_persisted = runtime.persist.last_persisted_hash.is_none();
    let save_now = never_persisted
        || first_real_of_session
        || session_complete
        || (runtime.persist.dirty_since_last_save
            && debounce_elapsed(runtime.persist.last_save_at, SNAPSHOT_SAVE_DEBOUNCE));

    if save_now {
        let reason = if never_persisted {
            "first real payload"
        } else if first_real_of_session {
            "first real payload of session"
        } else if session_complete {
            "session complete"
        } else {
            "debounced dirty snapshot"
        };
        persist_snapshot(runtime, &snapshot, reason);
    }
}

fn classify_payload(
    results_root: &Value,
    race_data_root: &Value,
    parsed_entries: &[TimingEntry],
) -> PayloadClassification {
    if !parsed_entries.is_empty() || has_meaningful_results_rows(results_root) {
        return PayloadClassification::Real;
    }

    if race_data_looks_shell_only(race_data_root) {
        return PayloadClassification::Placeholder;
    }

    PayloadClassification::Placeholder
}

fn has_meaningful_results_rows(results_root: &Value) -> bool {
    extract_results_rows(results_root)
        .map(|rows| rows.iter().any(row_looks_meaningful))
        .unwrap_or(false)
}

fn extract_results_rows(results_root: &Value) -> Option<&[Value]> {
    if let Some(rows) = results_root.get("B").and_then(|v| v.as_array()) {
        return Some(rows.as_slice());
    }
    if let Some(rows) = results_root.get("RaceResults").and_then(|v| v.as_array()) {
        return Some(rows.as_slice());
    }
    results_root.as_array().map(Vec::as_slice)
}

fn row_looks_meaningful(row: &Value) -> bool {
    let Some(obj) = row.as_object() else {
        return false;
    };

    ["A", "N", "F", "L", "BL", "PIC"]
        .iter()
        .filter_map(|key| obj.get(*key))
        .any(value_looks_real)
}

fn race_data_looks_shell_only(race_data_root: &Value) -> bool {
    if let Some(obj) = race_data_root.as_object() {
        if obj.is_empty() {
            return true;
        }
        let meaningful = obj.values().filter(|value| value_looks_real(value)).count();
        return meaningful == 0;
    }
    true
}

fn value_looks_real(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(_) => false,
        Value::Number(number) => number
            .as_i64()
            .map(|v| v != 0)
            .or_else(|| number.as_u64().map(|v| v != 0))
            .unwrap_or(false),
        Value::String(text) => {
            let trimmed = text.trim();
            !trimmed.is_empty()
                && trimmed != "-"
                && trimmed != "---"
                && trimmed != "0"
                && !trimmed.eq_ignore_ascii_case("session complete")
                && !trimmed.eq_ignore_ascii_case("not available")
                && !trimmed.eq_ignore_ascii_case("n/a")
        }
        Value::Array(values) => !values.is_empty() && values.iter().any(value_looks_real),
        Value::Object(map) => !map.is_empty() && map.values().any(value_looks_real),
    }
}

fn is_session_complete(race_data_root: &Value, header: &TimingHeader) -> bool {
    if header.flag.trim().eq_ignore_ascii_case("checkered") {
        return true;
    }

    if let Some(session) = race_data_root
        .get("Session")
        .and_then(|value| value.as_str())
    {
        if session.to_ascii_lowercase().contains("complete") {
            return true;
        }
    }

    if let Some(text) = race_data_root.get("B").and_then(|value| value.as_str()) {
        if text.to_ascii_lowercase().contains("session complete") {
            return true;
        }
    }

    false
}

fn payload_classification_label(classification: PayloadClassification) -> &'static str {
    match classification {
        PayloadClassification::Placeholder => "placeholder",
        PayloadClassification::Real => "real",
    }
}

fn dirty_transition_reason(
    previous_snapshot: Option<&ImsaSnapshot>,
    next_header: &TimingHeader,
    next_entries: &[TimingEntry],
) -> String {
    let Some(previous) = previous_snapshot else {
        return "initial real snapshot".to_string();
    };

    if previous.session_id != derive_session_identifier(next_header) {
        return "session changed".to_string();
    }

    if previous.entries.len() != next_entries.len() {
        return format!(
            "entry count {} -> {}",
            previous.entries.len(),
            next_entries.len()
        );
    }

    let previous_leader = previous
        .entries
        .first()
        .map(|entry| entry.stable_id.as_str());
    let next_leader = next_entries.first().map(|entry| entry.stable_id.as_str());
    if previous_leader != next_leader {
        let previous_text = previous_leader.unwrap_or("-");
        let next_text = next_leader.unwrap_or("-");
        return format!("leader changed {previous_text} -> {next_text}");
    }

    if previous.header.flag != next_header.flag {
        return format!(
            "flag changed {} -> {}",
            previous.header.flag, next_header.flag
        );
    }

    if previous.header.time_to_go != next_header.time_to_go {
        return format!(
            "time-to-go changed {} -> {}",
            previous.header.time_to_go, next_header.time_to_go
        );
    }

    "classification/timing fields updated".to_string()
}

pub fn normalize_class_name(name: &str) -> String {
    normalize_class_name_impl(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{env, sync::mpsc, time::SystemTime};

    fn test_header() -> TimingHeader {
        TimingHeader {
            session_name: "Race".to_string(),
            session_type_raw: "R".to_string(),
            event_name: "12h Sebring".to_string(),
            track_name: "Sebring".to_string(),
            day_time: "2026-01-01T12:00:00Z".to_string(),
            flag: "Green".to_string(),
            time_to_go: "00:45:00".to_string(),
            ..TimingHeader::default()
        }
    }

    fn test_entry() -> TimingEntry {
        TimingEntry {
            position: 1,
            car_number: "31".to_string(),
            class_name: "GTP".to_string(),
            class_rank: "1".to_string(),
            driver: "Driver A".to_string(),
            vehicle: "Cadillac".to_string(),
            laps: "100".to_string(),
            stable_id: "feed:31".to_string(),
            ..TimingEntry::default()
        }
    }

    #[test]
    fn classifies_empty_shell_payload_as_placeholder() {
        let results = json!({"B": [], "S": "Session Complete"});
        let race_data = json!({"A": "", "B": "Session Complete", "C": "0", "T": ""});

        let classification = classify_payload(&results, &race_data, &[][..]);
        assert_eq!(classification, PayloadClassification::Placeholder);
    }

    #[test]
    fn classifies_results_rows_as_real() {
        let results = json!({"B": [{"A": 1, "N": "31", "F": "Driver A", "L": "100"}]});
        let race_data = json!({"B": "Session Complete"});

        let classification = classify_payload(&results, &race_data, &[][..]);
        assert_eq!(classification, PayloadClassification::Real);
    }

    #[test]
    fn placeholder_payload_does_not_overwrite_last_good_snapshot() {
        let (tx, rx) = mpsc::channel::<TimingMessage>();
        let mut runtime = ImsaRuntimeState::from_parts(None);

        let real_snapshot = FetchedSnapshot {
            header: test_header(),
            entries: vec![test_entry()],
            results_root: json!({"B": [{"A": 1, "N": "31", "F": "Driver A", "L": "100"}]}),
            race_data_root: json!({"A": "12:00", "C": "1", "T": "00:45:00"}),
        };
        handle_fetched_snapshot(&tx, 1, &mut runtime, real_snapshot);

        let saved_fingerprint = runtime
            .last_good_live_snapshot
            .as_ref()
            .map(|snapshot| snapshot.fingerprint)
            .expect("last good snapshot after real payload");

        let placeholder = FetchedSnapshot {
            header: TimingHeader::default(),
            entries: Vec::new(),
            results_root: json!({"B": []}),
            race_data_root: json!({"B": "Session Complete", "T": "", "C": "0"}),
        };
        handle_fetched_snapshot(&tx, 1, &mut runtime, placeholder);

        let current_fingerprint = runtime
            .last_good_live_snapshot
            .as_ref()
            .map(|snapshot| snapshot.fingerprint)
            .expect("snapshot should remain available");
        assert_eq!(current_fingerprint, saved_fingerprint);

        let mut messages = Vec::new();
        while let Ok(message) = rx.try_recv() {
            messages.push(message);
        }

        assert!(
            messages
                .iter()
                .any(|message| matches!(message, TimingMessage::Snapshot { .. })),
            "expected real snapshot message"
        );
        assert!(
            messages.iter().any(|message| {
                matches!(message, TimingMessage::Status { text, .. } if text == WAITING_NEXT_SESSION_STATUS)
            }),
            "expected waiting status for placeholder phase"
        );
    }

    #[test]
    fn shutdown_flush_saves_dirty_snapshot() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("unix time")
            .as_nanos();
        let temp_path = env::temp_dir().join(format!("imsa_snapshot_test_{unique}.json"));
        let mut runtime = ImsaRuntimeState::from_parts(Some(temp_path.clone()));

        let header = test_header();
        let entries = vec![test_entry()];
        let fingerprint = meaningful_snapshot_fingerprint(&header, &entries);
        runtime.last_good_live_snapshot = Some(ImsaSnapshot {
            header,
            entries,
            session_id: Some("event|session|track".to_string()),
            fingerprint,
            raw_results_payload: Some(json!({"B": [{"A": 1, "N": "31"}]})),
            raw_race_data_payload: Some(json!({"C": "4", "B": "Session Complete"})),
        });
        runtime.persist.dirty_since_last_save = true;

        flush_dirty_snapshot_on_shutdown(&mut runtime);

        let text = std::fs::read_to_string(&temp_path).expect("persisted snapshot file");
        let restored: PersistedImsaSnapshot =
            serde_json::from_str(&text).expect("parse persisted snapshot");
        assert_eq!(restored.meaningful_fingerprint, fingerprint);
        assert_eq!(restored.entries.len(), 1);

        let _ = std::fs::remove_file(temp_path);
    }

    #[test]
    fn transponder_placeholder_row_is_filtered() {
        let placeholder = json!({
            "A": 30,
            "N": "Tx12345678",
            "C": "",
            "F": " ",
            "V": ""
        });
        assert!(is_transponder_placeholder(&placeholder));
        assert!(parse_entry(&placeholder).is_none());
    }

    #[test]
    fn transponder_with_data_is_kept() {
        let real_tx_row = json!({
            "A": 1,
            "N": "Tx12345678",
            "C": "GTP",
            "F": "Driver Name",
            "V": "Porsche"
        });
        assert!(!is_transponder_placeholder(&real_tx_row));
        let entry = parse_entry(&real_tx_row).expect("should parse real transponder row");
        assert_eq!(entry.car_number, "Tx12345678");
        assert_eq!(entry.class_name, "GTP");
    }

    #[test]
    fn normal_car_number_is_kept() {
        let normal = json!({
            "A": 1,
            "N": "31",
            "C": "GTP",
            "F": "Driver",
            "V": "Cadillac"
        });
        assert!(!is_transponder_placeholder(&normal));
        assert!(parse_entry(&normal).is_some());
    }

    #[test]
    fn normalize_class_name_uses_canonical_dash_keys() {
        assert_eq!(normalize_class_name("gtdpro"), "GTD-PRO");
        assert_eq!(normalize_class_name("GTD_PRO"), "GTD-PRO");
        assert_eq!(normalize_class_name("pro am"), "PRO-AM");
        assert_eq!(normalize_class_name("pro_am"), "PRO-AM");
        assert_eq!(normalize_class_name("hypercar"), "HYPER");
    }
}
