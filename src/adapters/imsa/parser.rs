use reqwest::blocking::Client;
use serde_json::Value;

use crate::timing::{canonicalize_class_name, TimingEntry, TimingHeader};

use super::jsonp::{fetch_url_text, parse_jsonp_body};

#[derive(Debug)]
pub(super) struct FetchedSnapshot {
    pub(super) header: TimingHeader,
    pub(super) entries: Vec<TimingEntry>,
    pub(super) results_root: Value,
    pub(super) race_data_root: Value,
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

pub(super) fn is_transponder_placeholder(obj: &Value) -> bool {
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

pub(super) fn is_parsed_entry_transponder_placeholder(entry: &TimingEntry) -> bool {
    let cn = entry.car_number.trim();
    if !cn.starts_with("Tx") {
        return false;
    }
    let class_empty = entry.class_name.trim().is_empty() || entry.class_name == "-";
    let driver_empty = entry.driver.trim().is_empty() || entry.driver == "-";
    let vehicle_empty = entry.vehicle.trim().is_empty() || entry.vehicle == "-";
    class_empty && driver_empty && vehicle_empty
}

pub(super) fn parse_entry(obj: &Value) -> Option<TimingEntry> {
    if is_transponder_placeholder(obj) {
        return None;
    }

    let position = parse_position(obj)?;
    let car_number = as_string(obj, "N");
    let class_name = normalize_class_name(&as_string(obj, "C"));
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
        ..TimingHeader::default()
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

pub(super) fn fetch_snapshot(
    client: &Client,
    results_url_base: &str,
    results_callback: &str,
    race_data_url_base: &str,
    race_data_callback: &str,
    now_ms: u128,
) -> Result<FetchedSnapshot, String> {
    let results_url = format!("{results_url_base}?callback={results_callback}&_={now_ms}");
    let results_text = fetch_url_text(client, &results_url)?;
    let results_root = parse_jsonp_body(&results_text, results_callback)?;
    let (mut header, entries) = parse_results_snapshot(&results_root)?;

    let race_data_url = format!("{race_data_url_base}?callback={race_data_callback}&_={now_ms}");
    let race_data_text = fetch_url_text(client, &race_data_url)?;
    let race_data_root = parse_jsonp_body(&race_data_text, race_data_callback)?;
    merge_race_data_into_header(&mut header, &race_data_root);

    Ok(FetchedSnapshot {
        header,
        entries,
        results_root,
        race_data_root,
    })
}

pub(super) fn normalize_class_name(name: &str) -> String {
    canonicalize_class_name(name)
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
