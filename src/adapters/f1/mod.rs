// F1 SignalR-style adapter: negotiates session, subscribes to topics, and builds live leaderboard snapshots.

mod signalr;
mod snapshot;

use std::{
    collections::HashMap,
    io,
    sync::mpsc::{Receiver, Sender},
    time::Duration,
};

use reqwest::blocking::Client;
use serde_json::{Map, Value};
use tungstenite::{connect, Error as WsError, Message};

use crate::{
    snapshot_runtime::derive_session_identifier,
    timing::{TimingEntry, TimingHeader, TimingMessage},
    timing_persist::{debounce_elapsed, log_series_debug, PersistState, SeriesDebugOutput},
};

use self::signalr::{
    build_ws_request, negotiate, set_socket_timeout, start_session, subscribe_message,
};
use self::snapshot::{
    f1_snapshot_path, meaningful_snapshot_fingerprint, persist_snapshot,
    restore_snapshot_from_disk, F1Snapshot,
};

const SNAPSHOT_SAVE_DEBOUNCE: Duration = Duration::from_secs(180);

#[derive(Debug, Clone)]
struct DriverState {
    racing_number: String,
    code: String,
    full_name: String,
    team_name: String,
    team_colour: String,
    position: Option<u32>,
    class_rank: Option<String>,
    laps: Option<String>,
    interval: Option<String>,
    gap_to_leader: Option<String>,
    last_lap: Option<String>,
    best_lap: Option<String>,
    pit_count: Option<String>,
    in_pit: Option<String>,
}

impl DriverState {
    fn new(number: String) -> Self {
        Self {
            racing_number: number,
            code: "-".to_string(),
            full_name: "-".to_string(),
            team_name: "-".to_string(),
            team_colour: "-".to_string(),
            position: None,
            class_rank: None,
            laps: None,
            interval: None,
            gap_to_leader: None,
            last_lap: None,
            best_lap: None,
            pit_count: None,
            in_pit: None,
        }
    }

    fn driver_label(&self) -> String {
        if self.full_name != "-" {
            self.full_name.clone()
        } else if self.code != "-" {
            self.code.clone()
        } else {
            "-".to_string()
        }
    }

    fn to_timing_entry(&self) -> TimingEntry {
        let stable_id = if self.racing_number != "-" {
            format!("f1:driver:{}", self.racing_number)
        } else if self.code != "-" {
            format!("f1:code:{}", self.code)
        } else {
            format!("f1:team:{}", self.team_name)
        };

        TimingEntry {
            position: self.position.unwrap_or(999),
            car_number: self.racing_number.clone(),
            class_name: "F1".to_string(),
            class_rank: self.class_rank.clone().unwrap_or_else(|| "-".to_string()),
            driver: self.driver_label(),
            vehicle: "-".to_string(),
            team: self.team_name.clone(),
            laps: self.laps.clone().unwrap_or_else(|| "-".to_string()),
            gap_overall: self
                .gap_to_leader
                .clone()
                .unwrap_or_else(|| "-".to_string()),
            gap_class: self.interval.clone().unwrap_or_else(|| "-".to_string()),
            gap_next_in_class: "-".to_string(),
            last_lap: self.last_lap.clone().unwrap_or_else(|| "-".to_string()),
            best_lap: self.best_lap.clone().unwrap_or_else(|| "-".to_string()),
            sector_1: "-".to_string(),
            sector_2: "-".to_string(),
            sector_3: "-".to_string(),
            sector_4: "-".to_string(),
            sector_5: "-".to_string(),
            best_lap_no: "-".to_string(),
            pit: self.in_pit.clone().unwrap_or_else(|| "-".to_string()),
            pit_stops: self.pit_count.clone().unwrap_or_else(|| "-".to_string()),
            fastest_driver: "-".to_string(),
            stable_id,
        }
    }
}

#[derive(Debug, Default)]
struct F1State {
    header: TimingHeader,
    drivers: HashMap<String, DriverState>,
}

fn get_str<'a>(obj: &'a Value, key: &str) -> Option<&'a str> {
    obj.get(key).and_then(|v| v.as_str())
}

fn as_text(v: Option<&Value>) -> Option<String> {
    let val = v?;
    match val {
        Value::String(s) => {
            let trimmed = s.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(if *b { "Yes" } else { "No" }.to_string()),
        _ => None,
    }
}

fn parse_u32(value: Option<&Value>) -> Option<u32> {
    let v = value?;
    if let Some(n) = v.as_u64() {
        return u32::try_from(n).ok();
    }
    v.as_str().and_then(|s| s.trim().parse::<u32>().ok())
}

fn format_short_driver_name(first: &str, last: &str) -> Option<String> {
    let first = first.trim();
    let last = last.trim();

    match (first.is_empty(), last.is_empty()) {
        (false, false) => first
            .chars()
            .next()
            .map(|initial| format!("{initial}. {last}")),
        (true, false) => Some(last.to_string()),
        (false, true) => Some(first.to_string()),
        (true, true) => None,
    }
}

fn format_short_driver_name_from_full(full_name: &str) -> Option<String> {
    let mut parts = full_name.split_whitespace();
    let first = parts.next()?;
    let mut last = first;

    for part in parts {
        last = part;
    }

    if first == last {
        return Some(first.to_string());
    }

    first
        .chars()
        .next()
        .map(|initial| format!("{initial}. {last}"))
}

fn merge_driver_list(drivers: &mut HashMap<String, DriverState>, payload: &Value) {
    let Some(map) = payload.as_object() else {
        return;
    };

    for (driver_key, info) in map {
        let Some(obj) = info.as_object() else {
            continue;
        };

        let number = as_text(obj.get("RacingNumber"))
            .or_else(|| as_text(obj.get("Number")))
            .unwrap_or_else(|| driver_key.to_string());

        let state = drivers
            .entry(driver_key.to_string())
            .or_insert_with(|| DriverState::new(number.clone()));

        if state.racing_number == "-" || state.racing_number == driver_key.as_str() {
            state.racing_number = number;
        }

        if let Some(code) = as_text(obj.get("Tla")) {
            state.code = code;
        }

        let first = as_text(obj.get("FirstName")).unwrap_or_default();
        let last = as_text(obj.get("LastName")).unwrap_or_default();
        if let Some(short_name) = format_short_driver_name(&first, &last) {
            state.full_name = short_name;
        } else if let Some(full_name) = as_text(obj.get("FullName"))
            .or_else(|| as_text(obj.get("BroadcastName")))
            .or_else(|| as_text(obj.get("Name")))
        {
            if let Some(short_name) = format_short_driver_name_from_full(&full_name) {
                state.full_name = short_name;
            }
        }

        if let Some(team) = as_text(obj.get("TeamName")) {
            state.team_name = team;
        }
        if let Some(colour) = as_text(obj.get("TeamColour")) {
            state.team_colour = colour;
        }
    }
}

fn merge_timing_data(drivers: &mut HashMap<String, DriverState>, payload: &Value) {
    let Some(lines) = payload.get("Lines").and_then(|v| v.as_object()) else {
        return;
    };

    for (driver_key, line_value) in lines {
        let Some(line) = line_value.as_object() else {
            continue;
        };
        let state = drivers
            .entry(driver_key.to_string())
            .or_insert_with(|| DriverState::new(driver_key.to_string()));

        if let Some(pos) = parse_u32(line.get("Position")) {
            state.position = Some(pos);
            state.class_rank = Some(pos.to_string());
        }
        if let Some(laps) = as_text(line.get("NumberOfLaps")) {
            state.laps = Some(laps);
        }

        if let Some(interval) = line
            .get("IntervalToPositionAhead")
            .and_then(|v| v.as_object())
        {
            if let Some(v) = as_text(interval.get("Value")) {
                state.interval = Some(v);
            }
        }

        if let Some(gap) = line.get("GapToLeader").and_then(|v| v.as_object()) {
            if let Some(v) = as_text(gap.get("Value")) {
                state.gap_to_leader = Some(v);
            }
        }

        if let Some(last) = line.get("LastLapTime").and_then(|v| v.as_object()) {
            if let Some(v) = as_text(last.get("Value")) {
                state.last_lap = Some(v);
            }
        }

        if let Some(best) = line.get("BestLapTime").and_then(|v| v.as_object()) {
            if let Some(v) = as_text(best.get("Value")) {
                state.best_lap = Some(v);
            }
        }

        if let Some(pit_count) = as_text(line.get("NumberOfPitStops")) {
            state.pit_count = Some(pit_count);
        }

        if let Some(in_pit) = as_text(line.get("InPit")) {
            state.in_pit = Some(in_pit);
        }
    }
}

fn merge_timing_stats(drivers: &mut HashMap<String, DriverState>, payload: &Value) {
    let Some(lines) = payload.get("Lines").and_then(|v| v.as_object()) else {
        return;
    };

    for (driver_key, line_value) in lines {
        let Some(line) = line_value.as_object() else {
            continue;
        };

        let state = drivers
            .entry(driver_key.to_string())
            .or_insert_with(|| DriverState::new(driver_key.to_string()));

        if let Some(best_laps) = line.get("BestLaps").and_then(|v| v.as_object()) {
            if let Some(best0) = best_laps.get("0").and_then(|v| v.as_object()) {
                if let Some(time) = as_text(best0.get("Value")) {
                    state.best_lap = Some(time);
                }
            }
        }

        if let Some(stops) = as_text(line.get("NumberOfPitStops")) {
            state.pit_count = Some(stops);
        }
    }
}

fn merge_session_info(header: &mut TimingHeader, payload: &Value) {
    if let Some(name) = get_str(payload, "Name") {
        header.session_name = name.to_string();
    }
    if let Some(meeting) = payload.get("Meeting").and_then(|v| v.as_object()) {
        if let Some(name) = meeting.get("Name").and_then(|v| v.as_str()) {
            header.event_name = name.to_string();
        }
        if let Some(circuit) = meeting.get("Circuit").and_then(|v| v.as_object()) {
            if let Some(short_name) = circuit.get("ShortName").and_then(|v| v.as_str()) {
                header.track_name = short_name.to_string();
            }
        }
    }
}

fn merge_lap_count(header: &mut TimingHeader, payload: &Value) {
    let current = as_text(payload.get("CurrentLap")).unwrap_or_else(|| "-".to_string());
    let total = as_text(payload.get("TotalLaps")).unwrap_or_else(|| "-".to_string());

    if current != "-" || total != "-" {
        header.day_time = format!("Lap {current}/{total}");
    }
}

fn merge_extrapolated_clock(header: &mut TimingHeader, payload: &Value) {
    if let Some(remaining) = as_text(payload.get("Remaining")) {
        header.time_to_go = remaining;
    }
}

fn merge_track_status(header: &mut TimingHeader, payload: &Value) {
    let status = get_str(payload, "Status").unwrap_or("-");
    header.flag = match status {
        "1" => "Green".to_string(),
        "2" => "Yellow".to_string(),
        "4" => "Safety Car".to_string(),
        "5" => "Red".to_string(),
        other => other.to_string(),
    };
}

fn merge_race_control(header: &mut TimingHeader, payload: &Value) {
    let Some(messages) = payload.get("Messages").and_then(|v| v.as_object()) else {
        return;
    };

    let latest = messages
        .values()
        .filter_map(|msg| msg.as_object())
        .max_by_key(|obj| obj.get("Utc").and_then(|v| v.as_str()).unwrap_or(""));

    if let Some(msg) = latest {
        if let Some(flag) = msg.get("Flag").and_then(|v| v.as_str()) {
            header.flag = if flag.is_empty() {
                header.flag.clone()
            } else {
                flag.to_string()
            };
        }
    }
}

fn process_topic(state: &mut F1State, topic: &str, payload: &Value) {
    match topic {
        "DriverList" => merge_driver_list(&mut state.drivers, payload),
        "TimingData" => merge_timing_data(&mut state.drivers, payload),
        "TimingStats" => merge_timing_stats(&mut state.drivers, payload),
        "SessionInfo" => merge_session_info(&mut state.header, payload),
        "LapCount" => merge_lap_count(&mut state.header, payload),
        "ExtrapolatedClock" => merge_extrapolated_clock(&mut state.header, payload),
        "TrackStatus" => merge_track_status(&mut state.header, payload),
        "RaceControlMessages" => merge_race_control(&mut state.header, payload),
        "Position.z" | "CarData.z" => {}
        _ => {}
    }
}

fn process_batch_map(state: &mut F1State, map: &Map<String, Value>) {
    for (topic, payload) in map {
        process_topic(state, topic, payload);
    }
}

fn process_signalr_message(state: &mut F1State, text: &str) -> bool {
    let Ok(root) = serde_json::from_str::<Value>(text) else {
        return false;
    };

    let mut changed = false;

    if let Some(initial) = root.get("R") {
        if let Some(map) = initial.as_object() {
            process_batch_map(state, map);
            changed = true;
        }
    }

    if let Some(messages) = root.get("M").and_then(|v| v.as_array()) {
        for msg in messages {
            if let Some(args) = msg.get("A").and_then(|v| v.as_array()) {
                if args.len() >= 2 {
                    if let Some(topic) = args.first().and_then(|v| v.as_str()) {
                        process_topic(state, topic, &args[1]);
                        changed = true;
                    }
                } else if args.len() == 1 {
                    if let Some(map) = args[0].as_object() {
                        process_batch_map(state, map);
                        changed = true;
                    }
                }
            }

            if let Some(target) = msg.get("M").and_then(|v| v.as_str()) {
                if let Some(args) = msg.get("A").and_then(|v| v.as_array()) {
                    if target.eq_ignore_ascii_case("feed") && args.len() >= 2 {
                        if let Some(topic) = args[0].as_str() {
                            process_topic(state, topic, &args[1]);
                            changed = true;
                        }
                    }
                }
            }
        }
    }

    changed
}

fn snapshot_from_state(state: &F1State) -> (TimingHeader, Vec<TimingEntry>) {
    let mut entries: Vec<TimingEntry> = state
        .drivers
        .values()
        .map(DriverState::to_timing_entry)
        .collect();
    entries.sort_by_key(|e| (e.position, e.car_number.clone()));

    let mut header = state.header.clone();
    if header.event_name.is_empty() {
        header.event_name = "F1 Live Timing".to_string();
    }
    if header.track_name.is_empty() {
        header.track_name = "-".to_string();
    }
    if header.session_name.is_empty() {
        header.session_name = "-".to_string();
    }
    if header.flag.is_empty() {
        header.flag = "-".to_string();
    }

    (header, entries)
}

pub fn signalr_worker(tx: Sender<TimingMessage>, source_id: u64, stop_rx: Receiver<()>) {
    signalr_worker_with_debug(tx, source_id, stop_rx, SeriesDebugOutput::Silent)
}

pub fn signalr_worker_with_debug(
    tx: Sender<TimingMessage>,
    source_id: u64,
    stop_rx: Receiver<()>,
    debug_output: SeriesDebugOutput,
) {
    let mut persist = PersistState::new(f1_snapshot_path());
    let mut last_snapshot = restore_snapshot_from_disk(&mut persist, &tx, source_id, &debug_output);
    if last_snapshot.is_some() {
        let _ = tx.send(TimingMessage::Status {
            source_id,
            text: "[SNAPSHOT] Restored from saved data".to_string(),
        });
    }
    let mut last_session_id = last_snapshot
        .as_ref()
        .and_then(|snap| snap.session_id.clone());

    let client = match Client::builder().timeout(Duration::from_secs(12)).build() {
        Ok(c) => c,
        Err(err) => {
            let _ = tx.send(TimingMessage::Error {
                source_id,
                text: format!("http client init failed: {err}"),
            });
            return;
        }
    };

    'outer: loop {
        if stop_rx.try_recv().is_ok() {
            if let Some(snapshot) = last_snapshot.as_ref() {
                if persist.dirty_since_last_save {
                    persist_snapshot(&mut persist, snapshot, &debug_output);
                }
            }
            break;
        }

        let _ = tx.send(TimingMessage::Status {
            source_id,
            text: "Negotiating F1 SignalR connection...".to_string(),
        });
        log_series_debug(&debug_output, "F1", "negotiating SignalR connection");

        let negotiated = match negotiate(&client) {
            Ok(n) => n,
            Err(err) => {
                let _ = tx.send(TimingMessage::Error {
                    source_id,
                    text: err,
                });
                if stop_rx.recv_timeout(Duration::from_secs(4)).is_ok() {
                    break;
                }
                continue;
            }
        };

        let request = build_ws_request(&negotiated.connection_token);

        let (mut socket, response) = match connect(request) {
            Ok(ok) => ok,
            Err(err) => {
                let _ = tx.send(TimingMessage::Error {
                    source_id,
                    text: format!("F1 websocket connect failed: {err}"),
                });
                if stop_rx.recv_timeout(Duration::from_secs(4)).is_ok() {
                    break;
                }
                continue;
            }
        };

        set_socket_timeout(&mut socket);

        let _ = tx.send(TimingMessage::Status {
            source_id,
            text: format!("F1 websocket connected ({})", response.status()),
        });
        log_series_debug(
            &debug_output,
            "F1",
            format!("websocket connected ({})", response.status()),
        );

        if let Err(err) = start_session(&client, &negotiated.connection_token) {
            let _ = tx.send(TimingMessage::Error {
                source_id,
                text: err,
            });
        }

        let subscribe = subscribe_message(1).to_string();
        if let Err(err) = socket.send(Message::Text(subscribe.into())) {
            let _ = tx.send(TimingMessage::Error {
                source_id,
                text: format!("F1 subscribe failed: {err}"),
            });
            if stop_rx.recv_timeout(Duration::from_secs(4)).is_ok() {
                break;
            }
            continue;
        }

        let mut state = F1State {
            header: TimingHeader {
                event_name: "F1 Live Timing".to_string(),
                ..TimingHeader::default()
            },
            ..F1State::default()
        };

        loop {
            if stop_rx.try_recv().is_ok() {
                if let Some(snapshot) = last_snapshot.as_ref() {
                    if persist.dirty_since_last_save {
                        persist_snapshot(&mut persist, snapshot, &debug_output);
                    }
                }
                break 'outer;
            }

            match socket.read() {
                Ok(Message::Text(text)) => {
                    if process_signalr_message(&mut state, &text) {
                        let (header, entries) = snapshot_from_state(&state);
                        let session_id = derive_session_identifier(&header);
                        let snapshot = F1Snapshot {
                            header: header.clone(),
                            entries: entries.clone(),
                            session_id: session_id.clone(),
                            fingerprint: meaningful_snapshot_fingerprint(&header, &entries),
                        };
                        let first_real_of_session =
                            !snapshot.entries.is_empty() && session_id != last_session_id;
                        let session_complete =
                            snapshot.header.flag.eq_ignore_ascii_case("checkered");
                        let materially_changed = last_snapshot
                            .as_ref()
                            .map(|prev| prev.fingerprint != snapshot.fingerprint)
                            .unwrap_or(true);
                        if materially_changed {
                            persist.dirty_since_last_save = true;
                        }
                        let never_persisted = persist.last_persisted_hash.is_none();
                        let save_now = never_persisted
                            || first_real_of_session
                            || session_complete
                            || (persist.dirty_since_last_save
                                && debounce_elapsed(persist.last_save_at, SNAPSHOT_SAVE_DEBOUNCE));
                        if save_now {
                            persist_snapshot(&mut persist, &snapshot, &debug_output);
                        }
                        last_session_id = session_id;
                        last_snapshot = Some(snapshot);

                        let _ = tx.send(TimingMessage::Snapshot {
                            source_id,
                            header,
                            entries,
                        });
                        let _ = tx.send(TimingMessage::Status {
                            source_id,
                            text: "F1 live timing connected".to_string(),
                        });
                    }
                }
                Ok(Message::Binary(data)) => {
                    if let Ok(text) = std::str::from_utf8(&data) {
                        if process_signalr_message(&mut state, text) {
                            let (header, entries) = snapshot_from_state(&state);
                            let session_id = derive_session_identifier(&header);
                            let snapshot = F1Snapshot {
                                header: header.clone(),
                                entries: entries.clone(),
                                session_id: session_id.clone(),
                                fingerprint: meaningful_snapshot_fingerprint(&header, &entries),
                            };
                            let first_real_of_session =
                                !snapshot.entries.is_empty() && session_id != last_session_id;
                            let session_complete =
                                snapshot.header.flag.eq_ignore_ascii_case("checkered");
                            let materially_changed = last_snapshot
                                .as_ref()
                                .map(|prev| prev.fingerprint != snapshot.fingerprint)
                                .unwrap_or(true);
                            if materially_changed {
                                persist.dirty_since_last_save = true;
                            }
                            let never_persisted = persist.last_persisted_hash.is_none();
                            let save_now = never_persisted
                                || first_real_of_session
                                || session_complete
                                || (persist.dirty_since_last_save
                                    && debounce_elapsed(
                                        persist.last_save_at,
                                        SNAPSHOT_SAVE_DEBOUNCE,
                                    ));
                            if save_now {
                                persist_snapshot(&mut persist, &snapshot, &debug_output);
                            }
                            last_session_id = session_id;
                            last_snapshot = Some(snapshot);

                            let _ = tx.send(TimingMessage::Snapshot {
                                source_id,
                                header,
                                entries,
                            });
                        }
                    }
                }
                Ok(Message::Ping(data)) => {
                    if let Err(err) = socket.send(Message::Pong(data)) {
                        let _ = tx.send(TimingMessage::Error {
                            source_id,
                            text: format!("F1 pong failed: {err}"),
                        });
                        break;
                    }
                }
                Ok(Message::Pong(_)) => {}
                Ok(Message::Close(frame)) => {
                    let _ = tx.send(TimingMessage::Error {
                        source_id,
                        text: format!("F1 socket closed: {frame:?}"),
                    });
                    break;
                }
                Ok(Message::Frame(_)) => {}
                Err(WsError::Io(err))
                    if err.kind() == io::ErrorKind::WouldBlock
                        || err.kind() == io::ErrorKind::TimedOut =>
                {
                    continue;
                }
                Err(err) => {
                    let _ = tx.send(TimingMessage::Error {
                        source_id,
                        text: format!("F1 read failed: {err}"),
                    });
                    break;
                }
            }
        }

        let _ = tx.send(TimingMessage::Status {
            source_id,
            text: "F1 reconnecting in 4s...".to_string(),
        });
        log_series_debug(&debug_output, "F1", "reconnecting in 4s");
        if stop_rx.recv_timeout(Duration::from_secs(4)).is_ok() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{format_short_driver_name, format_short_driver_name_from_full};

    #[test]
    fn short_name_uses_first_initial_and_last_name() {
        assert_eq!(
            format_short_driver_name("Lando", "Norris"),
            Some("L. Norris".to_string())
        );
    }

    #[test]
    fn short_name_handles_missing_name_parts() {
        assert_eq!(
            format_short_driver_name("", "Leclerc"),
            Some("Leclerc".to_string())
        );
        assert_eq!(
            format_short_driver_name("Oscar", ""),
            Some("Oscar".to_string())
        );
        assert_eq!(format_short_driver_name("", ""), None);
    }

    #[test]
    fn short_name_can_be_derived_from_full_name() {
        assert_eq!(
            format_short_driver_name_from_full("  Max   Verstappen  "),
            Some("M. Verstappen".to_string())
        );
        assert_eq!(
            format_short_driver_name_from_full("Hamilton"),
            Some("Hamilton".to_string())
        );
    }
}
