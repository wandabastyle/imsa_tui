use std::collections::{BTreeMap, HashMap};

use serde_json::Value;

use crate::timing::{TimingClassColor, TimingEntry, TimingHeader};

use super::{model::parse_standings_data, store::CollectionStore};

pub fn snapshot_from_store(store: &CollectionStore) -> Option<(TimingHeader, Vec<TimingEntry>)> {
    let header = build_header(store);
    let entries = build_entries(store);

    if entries.is_empty() && header.event_name == "-" && header.session_name == "-" {
        return None;
    }

    Some((header, entries))
}

fn build_header(store: &CollectionStore) -> TimingHeader {
    let event_doc = first_doc(store, "events");
    let session_info_doc = first_doc(store, "session_info");
    let session_status_doc = first_doc(store, "session_status");
    let session_results_doc = first_doc(store, "session_results");
    let track_info_doc = first_doc(store, "track_info");
    let race_control_doc = first_doc(store, "race_control");

    TimingHeader {
        session_name: pick_text(
            session_info_doc,
            &[
                "info.name",
                "sessionName",
                "name",
                "sessionType.name",
                "session",
            ],
        )
        .unwrap_or_else(|| "-".to_string()),
        event_name: pick_text(
            session_info_doc,
            &["info.eventName", "info.eventShortName", "info.champName"],
        )
        .or_else(|| {
            pick_text(
                session_results_doc,
                &[
                    "classification.session.event_name",
                    "classification.session.championship_name",
                ],
            )
        })
        .or_else(|| {
            pick_text(
                event_doc,
                &["info.champName", "info.champShortName", "name", "champName"],
            )
        })
        .unwrap_or_else(|| "-".to_string()),
        track_name: pick_text(
            track_info_doc,
            &["track.name", "track.shortName", "name", "trackName"],
        )
        .unwrap_or_else(|| "-".to_string()),
        day_time: pick_text(session_info_doc, &["info.lastSessionTime", "info.date"])
            .or_else(|| {
                pick_text(
                    session_status_doc,
                    &["time", "clock", "now", "updatedAt", "timestamp"],
                )
            })
            .or_else(|| pick_text(event_doc, &["date", "updatedAt"]))
            .unwrap_or_else(|| "-".to_string()),
        flag: pick_text(
            session_status_doc,
            &["status.currentFlag", "flag", "status", "sessionFlag"],
        )
        .map(|raw| map_flag(&raw))
        .or_else(|| infer_flag_from_race_control(race_control_doc))
        .unwrap_or_else(|| "-".to_string()),
        time_to_go: build_time_to_go(session_status_doc),
        class_colors: extract_class_colors(store),
        ..TimingHeader::default()
    }
}

fn build_entries(store: &CollectionStore) -> Vec<TimingEntry> {
    let session_entries = entry_lookup(store);
    let results_rows = results_lookup(store);
    let standings_docs = store.collection("standings");
    let mut entries = Vec::new();

    let Some(standings_docs) = standings_docs else {
        return entries;
    };

    for standing_doc in standings_docs.values() {
        let Some(rows) = standing_rows(standing_doc) else {
            continue;
        };

        for (row_key, row) in &rows {
            let compact = lookup_path(row, "data")
                .and_then(read_text)
                .unwrap_or_default();
            let parsed = parse_standings_data(&compact);

            let car_number = parsed
                .car_number
                .clone()
                .or_else(|| pick_text(Some(row), &["number", "car", "entry", "bib"]))
                .unwrap_or_else(|| "-".to_string());

            let result_row = result_row_for_car(&results_rows, &car_number);
            let entry_doc = entry_doc_for_car(&session_entries, &car_number);

            let class_name = pick_text(result_row, &["class"])
                .or_else(|| pick_text(Some(row), &["class", "className"]))
                .or_else(|| pick_text(entry_doc, &["class", "category"]))
                .unwrap_or_else(|| "-".to_string());

            let position = pick_text(result_row, &["position"])
                .and_then(|value| value.parse::<u32>().ok())
                .or_else(|| {
                    parsed
                        .overall_position
                        .as_deref()
                        .and_then(|value| value.parse::<u32>().ok())
                })
                .or_else(|| row_key.parse::<u32>().ok())
                .unwrap_or(u32::MAX);

            let class_rank = parsed
                .class_position
                .clone()
                .or_else(|| pick_text(Some(row), &["classPosition", "positionClass"]))
                .unwrap_or_else(|| "-".to_string());

            let pit_marker = parsed.pit_marker.clone().unwrap_or_else(|| "-".to_string());

            let stable_id = if car_number.trim().is_empty() || car_number == "-" {
                format!("wec:row:{row_key}")
            } else {
                format!("wec:{car_number}")
            };

            let driver = display_driver(entry_doc, result_row);
            let sectors = sector_times_from_row(row);

            let last_lap = pick_text(result_row, &["time"])
                .or_else(|| {
                    lookup_path(row, "lastLapTime")
                        .and_then(read_u64)
                        .map(format_millis)
                })
                .unwrap_or_else(|| "-".to_string());

            let best_lap = lookup_path(row, "bestLapTime")
                .and_then(read_u64)
                .map(format_millis)
                .or_else(|| pick_text(result_row, &["time"]))
                .unwrap_or_else(|| "-".to_string());

            let in_pit = pit_marker.eq_ignore_ascii_case("BOX")
                || pit_marker.eq_ignore_ascii_case("PIT")
                || pit_marker.eq_ignore_ascii_case("IN");
            let pit = if in_pit { "Yes" } else { "No" }.to_string();

            entries.push(TimingEntry {
                position,
                car_number,
                class_name,
                class_rank,
                driver,
                vehicle: pick_text(result_row, &["vehicle"])
                    .or_else(|| pick_text(entry_doc, &["vehicle", "carModel"]))
                    .unwrap_or_else(|| "-".to_string()),
                team: pick_text(result_row, &["team"])
                    .or_else(|| pick_text(entry_doc, &["team", "entrant"]))
                    .unwrap_or_else(|| "-".to_string()),
                laps: pick_text(result_row, &["laps", "lap"])
                    .or_else(|| pick_text(Some(row), &["laps", "lap", "completedLaps"]))
                    .unwrap_or_else(|| "-".to_string()),
                gap_overall: pick_text(result_row, &["gap_first", "gap", "gapFirst"])
                    .or_else(|| pick_text(Some(row), &["elapsedTime", "gap", "delta"]))
                    .unwrap_or_else(|| "-".to_string()),
                gap_class: pick_text(result_row, &["gap_previous", "gapPrevious"])
                    .or(parsed.status.clone())
                    .unwrap_or_else(|| "-".to_string()),
                gap_next_in_class: pit_marker.clone(),
                last_lap,
                best_lap,
                sector_1: sectors.get("1").cloned().unwrap_or_else(|| "-".to_string()),
                sector_2: sectors.get("2").cloned().unwrap_or_else(|| "-".to_string()),
                sector_3: sectors
                    .get("3")
                    .cloned()
                    .map(|value| if in_pit { "PIT".to_string() } else { value })
                    .unwrap_or_else(|| {
                        if in_pit {
                            "PIT".to_string()
                        } else {
                            "-".to_string()
                        }
                    }),
                sector_4: "-".to_string(),
                sector_5: "-".to_string(),
                best_lap_no: pick_text(Some(row), &["bestLapNumber", "bestLapNo"])
                    .unwrap_or_else(|| "-".to_string()),
                pit,
                pit_stops: pick_text(result_row, &["pit_stops", "pitStops"])
                    .or(parsed.pit_stops.clone())
                    .unwrap_or_else(|| "-".to_string()),
                fastest_driver: "-".to_string(),
                stable_id,
            });
        }
    }

    entries.sort_by_key(|entry| entry.position);
    entries
}

fn standing_rows(standings_doc: &Value) -> Option<serde_json::Map<String, Value>> {
    let standings_root = lookup_path(standings_doc, "standings")?;
    let rows = lookup_path(standings_root, "standings")?;
    rows.as_object().cloned()
}

fn entry_lookup(store: &CollectionStore) -> HashMap<String, Value> {
    let mut entries = HashMap::new();
    let Some(session_entry_docs) = store.collection("session_entry") else {
        return entries;
    };

    for session_entry_doc in session_entry_docs.values() {
        let Some(entry_map) = lookup_path(session_entry_doc, "entry").and_then(Value::as_object)
        else {
            continue;
        };

        for (car, payload) in entry_map {
            entries.insert(car.clone(), payload.clone());
        }
    }

    entries
}

fn results_lookup(store: &CollectionStore) -> HashMap<String, Value> {
    let mut rows = HashMap::new();
    let Some(result_docs) = store.collection("session_results") else {
        return rows;
    };

    for doc in result_docs.values() {
        let Some(list) =
            lookup_path(doc, "classification.classification").and_then(Value::as_array)
        else {
            continue;
        };
        for row in list {
            let Some(number) = pick_text(Some(row), &["number", "participant_number"]) else {
                continue;
            };
            rows.insert(number, row.clone());
        }
    }

    rows
}

fn result_row_for_car<'a>(rows: &'a HashMap<String, Value>, car_number: &str) -> Option<&'a Value> {
    rows.get(car_number)
        .or_else(|| rows.get(strip_leading_zeroes(car_number)))
}

fn entry_doc_for_car<'a>(
    entries: &'a HashMap<String, Value>,
    car_number: &str,
) -> Option<&'a Value> {
    entries
        .get(car_number)
        .or_else(|| entries.get(strip_leading_zeroes(car_number)))
}

fn strip_leading_zeroes(value: &str) -> &str {
    let stripped = value.trim_start_matches('0');
    if stripped.is_empty() {
        "0"
    } else {
        stripped
    }
}

fn display_driver(entry_doc: Option<&Value>, result_row: Option<&Value>) -> String {
    if let Some(row) = result_row {
        let first =
            pick_text(Some(row), &["drivers.0.firstname", "driver_firstname"]).unwrap_or_default();
        let last =
            pick_text(Some(row), &["drivers.0.surname", "driver_surname"]).unwrap_or_default();
        if let Some(formatted) = format_driver_name(&first, &last) {
            return formatted;
        }
    }

    if let Some(entry) = entry_doc {
        let first = pick_text(
            Some(entry),
            &["firstname", "drivers.1.firstname", "drivers.0.firstname"],
        )
        .unwrap_or_default();
        let last = pick_text(
            Some(entry),
            &["lastname", "drivers.1.lastname", "drivers.0.lastname"],
        )
        .unwrap_or_default();
        if let Some(formatted) = format_driver_name(&first, &last) {
            return formatted;
        }

        if let Some(name) = pick_text(Some(entry), &["name", "driver"]) {
            return name;
        }
    }

    "-".to_string()
}

fn format_driver_name(first: &str, last: &str) -> Option<String> {
    let first = first.trim();
    let last = last.trim();
    if first.is_empty() && last.is_empty() {
        return None;
    }
    if first.is_empty() {
        return Some(last.to_string());
    }
    if last.is_empty() {
        return Some(first.to_string());
    }

    let initial = first.chars().next().map(|ch| ch.to_ascii_uppercase())?;
    Some(format!("{initial}. {}", last.to_ascii_uppercase()))
}

fn sector_times_from_row(row: &Value) -> BTreeMap<String, String> {
    parse_sector_payload(
        lookup_path(row, "lastSectors")
            .and_then(read_text)
            .as_deref()
            .unwrap_or(""),
    )
    .or_else(|| {
        parse_sector_payload(
            lookup_path(row, "currentSectors")
                .and_then(read_text)
                .as_deref()
                .unwrap_or(""),
        )
    })
    .unwrap_or_default()
}

fn parse_sector_payload(raw: &str) -> Option<BTreeMap<String, String>> {
    if raw.trim().is_empty() {
        return None;
    }

    let parts: Vec<&str> = raw.split(';').collect();
    let mut idx = 0_usize;
    let mut out = BTreeMap::new();
    while idx + 1 < parts.len() {
        let sector = parts[idx].trim();
        let value = parts[idx + 1].trim();
        if ["1", "2", "3", "4", "5"].contains(&sector)
            && value.chars().all(|ch| ch.is_ascii_digit())
            && !value.is_empty()
        {
            if let Ok(ms) = value.parse::<u64>() {
                out.insert(sector.to_string(), format_millis(ms));
            }
            idx = idx.saturating_add(6);
            continue;
        }
        idx = idx.saturating_add(1);
    }

    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn first_doc<'a>(store: &'a CollectionStore, name: &str) -> Option<&'a Value> {
    store.collection(name)?.values().next()
}

fn lookup_path<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = root;
    for part in path.split('.') {
        current = current.get(part)?;
    }
    Some(current)
}

fn pick_text(root: Option<&Value>, paths: &[&str]) -> Option<String> {
    let root = root?;
    for path in paths {
        if let Some(value) = lookup_path(root, path).and_then(read_text) {
            return Some(value);
        }
    }
    None
}

fn read_text(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(if *value { "true" } else { "false" }.to_string()),
        _ => None,
    }
}

fn read_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.parse::<u64>().ok(),
        _ => None,
    }
}

fn format_millis(ms: u64) -> String {
    let total_seconds = ms / 1000;
    let millis = ms % 1000;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{minutes}:{seconds:02}.{millis:03}")
}

fn map_flag(raw: &str) -> String {
    match raw.trim().to_ascii_uppercase().as_str() {
        "GF" | "GREEN" => "Green".to_string(),
        "YF" | "YELLOW" => "Yellow".to_string(),
        "RF" | "RED" => "Red".to_string(),
        "SC" => "Safety Car".to_string(),
        "FCY" => "Full Course Yellow".to_string(),
        "CF" | "CHECKERED" | "CHEQUERED" => "Checkered".to_string(),
        other => other.to_string(),
    }
}

fn build_time_to_go(session_status_doc: Option<&Value>) -> String {
    let Some(status_doc) = session_status_doc else {
        return "-".to_string();
    };

    if lookup_path(status_doc, "status.isFinished")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return "FINISH".to_string();
    }

    if let Some(remaining) = pick_text(
        Some(status_doc),
        &[
            "timeToGo",
            "time_to_go",
            "remaining",
            "sessionTimeRemaining",
        ],
    ) {
        return remaining;
    }

    "-".to_string()
}

fn extract_class_colors(store: &CollectionStore) -> BTreeMap<String, TimingClassColor> {
    let mut out = BTreeMap::new();
    let Some(class_docs) = store.collection("session_classes") else {
        return out;
    };

    for doc in class_docs.values() {
        let Some(class_map) = lookup_path(doc, "classes.classes").and_then(Value::as_object) else {
            continue;
        };

        for (key, class_def) in class_map {
            let class_name = pick_text(Some(class_def), &["shortName", "name"])
                .unwrap_or_else(|| key.to_string());
            let foreground =
                pick_text(Some(class_def), &["foreground", "lightForeground"]).unwrap_or_default();
            let background =
                pick_text(Some(class_def), &["background", "lightBackground"]).unwrap_or_default();

            if !looks_like_hex_color(&foreground) || !looks_like_hex_color(&background) {
                continue;
            }

            out.insert(
                normalize_class_key(&class_name),
                TimingClassColor {
                    foreground,
                    background,
                },
            );
        }
    }

    out
}

fn normalize_class_key(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_whitespace() && *ch != '_' && *ch != '-')
        .collect::<String>()
        .to_ascii_uppercase()
}

fn looks_like_hex_color(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.len() == 7
        && trimmed.starts_with('#')
        && trimmed.chars().skip(1).all(|ch| ch.is_ascii_hexdigit())
}

fn infer_flag_from_race_control(race_control_doc: Option<&Value>) -> Option<String> {
    let race_control_doc = race_control_doc?;
    let messages = lookup_path(race_control_doc, "raceControlMessages.currentMessages")?;

    let message_text = if let Some(list) = messages.as_array() {
        list.last().and_then(|msg| {
            pick_text(Some(msg), &["message", "text", "title"]).or_else(|| read_text(msg))
        })
    } else if let Some(map) = messages.as_object() {
        let (_, latest) = map.iter().max_by_key(|(key, _)| *key)?;
        pick_text(Some(latest), &["message", "text", "title"]).or_else(|| read_text(latest))
    } else {
        None
    }?;

    let lower = message_text.to_ascii_lowercase();
    if lower.contains("green") {
        return Some("Green".to_string());
    }
    if lower.contains("yellow") {
        return Some("Yellow".to_string());
    }
    if lower.contains("red") {
        return Some("Red".to_string());
    }
    if lower.contains("checkered") || lower.contains("chequered") {
        return Some("Checkered".to_string());
    }

    None
}
