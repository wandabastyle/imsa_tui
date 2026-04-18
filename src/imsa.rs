// IMSA feed adapter: polls JSON/JSONP endpoints and normalizes rows into shared timing structs.

use std::{
    hash::{Hash, Hasher},
    path::PathBuf,
    sync::mpsc::{Receiver, Sender},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    snapshot_runtime::{base_snapshot_fingerprint, derive_session_identifier},
    timing::{TimingEntry, TimingHeader, TimingMessage},
    timing_persist::{
        data_local_snapshot_path, debounce_elapsed, log_series_debug, read_json, write_json_pretty,
        PersistState, SeriesDebugOutput,
    },
};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedImsaSnapshot {
    saved_unix_ms: u64,
    session_id: Option<String>,
    meaningful_fingerprint: u64,
    header: TimingHeader,
    entries: Vec<TimingEntry>,
    raw_results_payload: Option<Value>,
    raw_race_data_payload: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PayloadClassification {
    Placeholder,
    Real,
}

#[derive(Debug)]
struct FetchedSnapshot {
    header: TimingHeader,
    entries: Vec<TimingEntry>,
    results_root: Value,
    race_data_root: Value,
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

    loop {
        if stop_rx.try_recv().is_ok() {
            flush_dirty_snapshot_on_shutdown(&mut runtime);
            break;
        }

        let _ = tx.send(TimingMessage::Status {
            source_id,
            text: "Fetching IMSA live timing...".to_string(),
        });

        match fetch_snapshot(&client) {
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

fn get_str<'a>(obj: &'a Value, key: &str) -> Option<&'a str> {
    obj.get(key).and_then(|x| x.as_str())
}

fn get_u64(obj: &Value, key: &str) -> Option<u64> {
    obj.get(key).and_then(|x| x.as_u64())
}

fn looks_like_mojibake(s: &str) -> bool {
    s.contains("Ã") || s.contains("Â") || s.contains("â€") || s.contains("â€“") || s.contains("â€”")
}

fn fix_mojibake(s: &str) -> String {
    if !looks_like_mojibake(s) {
        return s.to_string();
    }

    let bytes: Option<Vec<u8>> = s.chars().map(|c| u8::try_from(c as u32).ok()).collect();
    let Some(bytes) = bytes else {
        return s.to_string();
    };

    match String::from_utf8(bytes) {
        Ok(decoded) => decoded,
        Err(_) => s.to_string(),
    }
}

fn clean_string(s: &str) -> String {
    fix_mojibake(s.trim())
}

fn as_string(obj: &Value, key: &str) -> String {
    if let Some(s) = get_str(obj, key) {
        let cleaned = clean_string(s);
        if !cleaned.is_empty() {
            return cleaned;
        }
    }
    if let Some(n) = get_u64(obj, key) {
        return n.to_string();
    }
    "-".to_string()
}

fn parse_position(obj: &Value) -> Option<u32> {
    if let Some(n) = obj.get("A").and_then(|v| v.as_u64()) {
        return u32::try_from(n).ok();
    }
    if let Some(s) = get_str(obj, "A") {
        return s.trim().parse::<u32>().ok();
    }
    None
}

fn parse_pit(obj: &Value) -> String {
    match obj.get("P") {
        Some(Value::Bool(true)) => "Yes".to_string(),
        Some(Value::Bool(false)) => "No".to_string(),
        Some(Value::Number(n)) if n.as_i64() == Some(1) => "Yes".to_string(),
        Some(Value::Number(n)) if n.as_i64() == Some(0) => "No".to_string(),
        Some(Value::String(s)) if s == "1" => "Yes".to_string(),
        Some(Value::String(s)) if s == "0" => "No".to_string(),
        Some(v) => {
            let s = v.to_string();
            if s == "\"\"" {
                "-".to_string()
            } else {
                s.trim_matches('"').to_string()
            }
        }
        None => "-".to_string(),
    }
}

fn is_transponder_placeholder(obj: &Value) -> bool {
    let Some(n) = get_str(obj, "N") else {
        return false;
    };

    if !n.starts_with("Tx") {
        return false;
    }

    let class_empty = get_str(obj, "C")
        .map(|s| s.trim().is_empty())
        .unwrap_or(true);
    let driver_empty = get_str(obj, "F")
        .map(|s| s.trim().is_empty())
        .unwrap_or(true);
    let vehicle_empty = get_str(obj, "V")
        .map(|s| s.trim().is_empty())
        .unwrap_or(true);

    class_empty && driver_empty && vehicle_empty
}

fn is_parsed_entry_transponder_placeholder(entry: &TimingEntry) -> bool {
    let cn = entry.car_number.trim();
    if !cn.starts_with("Tx") {
        return false;
    }
    let class_empty = entry.class_name.trim().is_empty() || entry.class_name == "-";
    let driver_empty = entry.driver.trim().is_empty() || entry.driver == "-";
    let vehicle_empty = entry.vehicle.trim().is_empty() || entry.vehicle == "-";
    class_empty && driver_empty && vehicle_empty
}

fn parse_entry(obj: &Value) -> Option<TimingEntry> {
    if is_transponder_placeholder(obj) {
        return None;
    }

    let position = parse_position(obj)?;
    let car_number = as_string(obj, "N");
    let class_name = as_string(obj, "C");
    let stable_id = parse_stable_car_id(obj, &car_number);

    Some(TimingEntry {
        position,
        car_number,
        class_name,
        class_rank: as_string(obj, "PIC"),
        driver: as_string(obj, "F"),
        vehicle: as_string(obj, "V"),
        laps: as_string(obj, "L"),
        gap_overall: as_string(obj, "D"),
        gap_class: as_string(obj, "DIC"),
        gap_next_in_class: as_string(obj, "GIC"),
        last_lap: as_string(obj, "LL"),
        best_lap: as_string(obj, "BL"),
        sector_1: "-".to_string(),
        sector_2: "-".to_string(),
        sector_3: "-".to_string(),
        sector_4: "-".to_string(),
        sector_5: "-".to_string(),
        best_lap_no: as_string(obj, "IN"),
        pit: parse_pit(obj),
        pit_stops: as_string(obj, "PS"),
        fastest_driver: as_string(obj, "FD"),
        team: "-".to_string(),
        stable_id,
    })
}

fn parse_stable_car_id(obj: &Value, car_number: &str) -> String {
    let unique_id_keys = ["ID", "Id", "id", "CID", "CarID", "EntryID", "UID"];
    for key in unique_id_keys {
        let v = as_string(obj, key);
        if v != "-" && !v.trim().is_empty() {
            return format!("feed:{v}");
        }
    }

    format!("fallback:{}", car_number.trim())
}

fn parse_jsonp_body(text: &str, callback: &str) -> Result<Value, String> {
    let trimmed = text.trim();

    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return serde_json::from_str(trimmed).map_err(|e| format!("json parse failed: {e}"));
    }

    let prefix = format!("{callback}(");
    if !trimmed.starts_with(&prefix) {
        return Err(format!(
            "response is neither raw JSON nor expected JSONP callback {callback}"
        ));
    }

    let start = prefix.len();
    let end = trimmed
        .rfind(')')
        .ok_or_else(|| "jsonp closing ')' not found".to_string())?;

    let inner = trimmed[start..end].trim();
    serde_json::from_str(inner).map_err(|e| format!("jsonp inner json parse failed: {e}"))
}

fn first_present_string(root: &Value, keys: &[&str]) -> String {
    for key in keys {
        let v = as_string(root, key);
        if v != "-" {
            return v;
        }
    }
    "-".to_string()
}

fn parse_flag_code(code: &str) -> String {
    match code.trim() {
        "0" | "1" | "" => "Green".to_string(),
        "2" => "Yellow".to_string(),
        "3" => "Red".to_string(),
        "4" => "Checkered".to_string(),
        other if !other.is_empty() => other.to_string(),
        _ => "Green".to_string(),
    }
}

fn build_results_header(root: &Value) -> TimingHeader {
    TimingHeader {
        session_name: first_present_string(root, &["S", "Session", "session", "sessionName"]),
        event_name: first_present_string(root, &["E", "Event", "event", "eventName"]),
        track_name: first_present_string(root, &["T", "Track", "track", "trackName"]),
        day_time: first_present_string(root, &["DT", "Day", "day", "dayTime", "timestamp"]),
        flag: "-".to_string(),
        time_to_go: "-".to_string(),
        class_colors: Default::default(),
    }
}

fn merge_race_data_into_header(header: &mut TimingHeader, race_data: &Value) {
    let day_time = first_present_string(race_data, &["A"]);
    if day_time != "-" {
        header.day_time = day_time;
    }

    let raw_time_to_go = first_present_string(race_data, &["T", "B"]);
    let time_to_go = clean_time_to_go(&raw_time_to_go);
    if time_to_go != "-" {
        header.time_to_go = time_to_go;
    }

    let raw_flag = first_present_string(race_data, &["C"]);
    let parsed_flag = parse_flag_code(&raw_flag);
    if parsed_flag != "-" {
        header.flag = parsed_flag;
    }

    let maybe_session = first_present_string(race_data, &["Session", "S"]);
    if maybe_session != "-" {
        header.session_name = maybe_session;
    }
}

fn parse_results_snapshot(root: &Value) -> Result<(TimingHeader, Vec<TimingEntry>), String> {
    if let Some(cars) = root.get("B").and_then(|v| v.as_array()) {
        let mut entries: Vec<TimingEntry> = cars.iter().filter_map(parse_entry).collect();
        entries.sort_by_key(|e| e.position);
        return Ok((build_results_header(root), entries));
    }

    if let Some(cars) = root.get("RaceResults").and_then(|v| v.as_array()) {
        let mut entries: Vec<TimingEntry> = cars.iter().filter_map(parse_entry).collect();
        entries.sort_by_key(|e| e.position);
        return Ok((build_results_header(root), entries));
    }

    if let Some(cars) = root.as_array() {
        let mut entries: Vec<TimingEntry> = cars.iter().filter_map(parse_entry).collect();
        entries.sort_by_key(|e| e.position);
        return Ok((build_results_header(root), entries));
    }

    if let Some(obj) = root.as_object() {
        let mut keys: Vec<String> = obj.keys().cloned().collect();
        keys.sort();
        return Err(format!(
            "unexpected JSON shape; top-level keys: {}",
            keys.join(", ")
        ));
    }

    Err("unexpected JSON shape; top-level value is not object/array".to_string())
}

fn fetch_url_text(client: &Client, url: &str) -> Result<String, String> {
    let response = client
        .get(url)
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/123 Safari/537.36",
        )
        .header("Accept", "application/javascript, application/json, text/plain, */*")
        .header("Accept-Language", "en-US,en;q=0.9")
        .header("Referer", "https://www.imsa.com/scoring/")
        .header("Origin", "https://www.imsa.com")
        .header("Cache-Control", "no-cache")
        .header("Pragma", "no-cache")
        .send()
        .map_err(|e| format!("request failed: {e}"))?;

    let status = response.status();
    if !status.is_success() {
        return Err(format!("http {status}"));
    }

    response
        .text()
        .map_err(|e| format!("body read failed: {e}"))
}

fn fetch_snapshot(client: &Client) -> Result<FetchedSnapshot, String> {
    let results_url = format!(
        "{RESULTS_URL}?callback={RESULTS_CALLBACK}&_={}",
        now_millis()
    );
    let results_text = fetch_url_text(client, &results_url)?;
    let results_root = parse_jsonp_body(&results_text, RESULTS_CALLBACK)?;
    let (mut header, entries) = parse_results_snapshot(&results_root)?;

    let race_data_url = format!(
        "{RACE_DATA_URL}?callback={RACE_DATA_CALLBACK}&_={}",
        now_millis()
    );
    let race_data_text = fetch_url_text(client, &race_data_url)?;
    let race_data_root = parse_jsonp_body(&race_data_text, RACE_DATA_CALLBACK)?;
    merge_race_data_into_header(&mut header, &race_data_root);

    Ok(FetchedSnapshot {
        header,
        entries,
        results_root,
        race_data_root,
    })
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

fn persist_snapshot_if_dirty(runtime: &mut ImsaRuntimeState, reason: &str) {
    if !runtime.persist.dirty_since_last_save {
        return;
    }
    if let Some(snapshot) = runtime.last_good_live_snapshot.clone() {
        persist_snapshot(runtime, &snapshot, reason);
    }
}

fn flush_dirty_snapshot_on_shutdown(runtime: &mut ImsaRuntimeState) {
    persist_snapshot_if_dirty(runtime, "shutdown flush");
}

fn persist_snapshot(runtime: &mut ImsaRuntimeState, snapshot: &ImsaSnapshot, reason: &str) {
    let Some(path) = runtime.persist.path.as_ref() else {
        log_debug(
            &runtime.debug_output,
            "IMSA snapshot persist skipped: config path unavailable.".to_string(),
        );
        return;
    };

    let payload = PersistedImsaSnapshot {
        saved_unix_ms: now_unix_ms(),
        session_id: snapshot.session_id.clone(),
        meaningful_fingerprint: snapshot.fingerprint,
        header: snapshot.header.clone(),
        entries: snapshot.entries.clone(),
        raw_results_payload: snapshot.raw_results_payload.clone(),
        raw_race_data_payload: snapshot.raw_race_data_payload.clone(),
    };

    if let Err(err) = write_json_pretty(path, &payload) {
        log_debug(
            &runtime.debug_output,
            format!("IMSA snapshot persist failed writing file: {err}"),
        );
        return;
    }

    runtime.persist.last_persisted_hash = Some(snapshot.fingerprint);
    runtime.persist.last_save_at = Some(SystemTime::now());
    runtime.persist.dirty_since_last_save = false;
    log_debug(
        &runtime.debug_output,
        format!("IMSA snapshot persisted ({reason}) to {}.", path.display()),
    );
}

fn restore_snapshot_from_disk(
    runtime: &mut ImsaRuntimeState,
    tx: &Sender<TimingMessage>,
    source_id: u64,
) {
    let Some(path) = runtime.persist.path.as_ref() else {
        return;
    };

    let Some(saved) = read_json::<PersistedImsaSnapshot>(path) else {
        return;
    };

    let entries: Vec<TimingEntry> = saved
        .entries
        .into_iter()
        .filter(|e| !is_parsed_entry_transponder_placeholder(e))
        .collect();

    let restored = ImsaSnapshot {
        header: saved.header,
        entries,
        session_id: saved.session_id,
        fingerprint: saved.meaningful_fingerprint,
        raw_results_payload: saved.raw_results_payload,
        raw_race_data_payload: saved.raw_race_data_payload,
    };

    runtime.last_session_id = restored.session_id.clone();
    runtime.persist.last_persisted_hash = Some(restored.fingerprint);
    runtime.persist.last_save_at = Some(SystemTime::now());
    runtime.last_good_live_snapshot = Some(restored.clone());

    let _ = tx.send(TimingMessage::Snapshot {
        source_id,
        header: restored.header,
        entries: restored.entries,
    });

    log_debug(
        &runtime.debug_output,
        format!("IMSA snapshot restored from {}.", path.display()),
    );
}

fn imsa_snapshot_path() -> Option<PathBuf> {
    data_local_snapshot_path("imsa_snapshot.json")
}

fn meaningful_snapshot_fingerprint(header: &TimingHeader, entries: &[TimingEntry]) -> u64 {
    let mut hasher = base_snapshot_fingerprint(header);

    for entry in entries {
        entry.position.hash(&mut hasher);
        entry.car_number.trim().hash(&mut hasher);
        entry
            .class_name
            .trim()
            .to_ascii_lowercase()
            .hash(&mut hasher);
        entry.class_rank.trim().hash(&mut hasher);
        entry.driver.trim().to_ascii_lowercase().hash(&mut hasher);
        entry.vehicle.trim().to_ascii_lowercase().hash(&mut hasher);
        entry.laps.trim().hash(&mut hasher);
        entry.gap_overall.trim().hash(&mut hasher);
        entry.gap_class.trim().hash(&mut hasher);
        entry.gap_next_in_class.trim().hash(&mut hasher);
        entry.last_lap.trim().hash(&mut hasher);
        entry.best_lap.trim().hash(&mut hasher);
        entry.best_lap_no.trim().hash(&mut hasher);
        entry.pit.trim().to_ascii_lowercase().hash(&mut hasher);
        entry.pit_stops.trim().hash(&mut hasher);
        entry
            .fastest_driver
            .trim()
            .to_ascii_lowercase()
            .hash(&mut hasher);
        entry.stable_id.trim().hash(&mut hasher);
    }

    hasher.finish()
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

    if let Some(session) = get_str(race_data_root, "Session") {
        if session.to_ascii_lowercase().contains("complete") {
            return true;
        }
    }

    if let Some(text) = get_str(race_data_root, "B") {
        if text.to_ascii_lowercase().contains("session complete") {
            return true;
        }
    }

    false
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
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
    name.chars()
        .filter(|c| !c.is_whitespace() && *c != '_')
        .collect::<String>()
        .to_uppercase()
}

fn clean_time_to_go(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "-" {
        return "-".to_string();
    }

    trimmed
        .strip_prefix("Time to go:")
        .unwrap_or(trimmed)
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{env, sync::mpsc, time::SystemTime};

    fn test_header() -> TimingHeader {
        TimingHeader {
            session_name: "Race".to_string(),
            event_name: "12h Sebring".to_string(),
            track_name: "Sebring".to_string(),
            day_time: "2026-01-01T12:00:00Z".to_string(),
            flag: "Green".to_string(),
            time_to_go: "00:45:00".to_string(),
            class_colors: Default::default(),
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

        let classification = classify_payload(&results, &race_data, &[]);
        assert_eq!(classification, PayloadClassification::Placeholder);
    }

    #[test]
    fn classifies_results_rows_as_real() {
        let results = json!({"B": [{"A": 1, "N": "31", "F": "Driver A", "L": "100"}]});
        let race_data = json!({"B": "Session Complete"});

        let classification = classify_payload(&results, &race_data, &[]);
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
}
