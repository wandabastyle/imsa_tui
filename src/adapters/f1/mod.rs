use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    path::PathBuf,
    sync::mpsc::{Receiver, Sender},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{
    adapters::insights::{
        common::{
            format_gap, format_lap_time_ms, map_bool, map_i64, map_str, map_text, map_u32,
            normalize_driver_name,
        },
        session::{
            choose_latest_finished_race_session, fetch_live_json, fetch_meta_sessions_for_series,
            resolve_live_sid_for_series,
        },
        signalr_ws::{
            build_request as build_signalr_request, connect_signalr, negotiate,
            parse_signalr_frame, send_join_group, send_signalr_handshake, split_signalr_frames,
            SignalRFrame,
        },
    },
    snapshot_runtime::{
        base_snapshot_fingerprint, derive_session_identifier, hash_entry_common_fields,
    },
    timing::{TimingEntry, TimingHeader, TimingMessage},
    timing_persist::{
        data_local_snapshot_path, debounce_elapsed, log_series_debug, read_json, write_json_pretty,
        PersistState, SeriesDebugOutput,
    },
};

const F1_SERIES_ID: u64 = 370;
const RECONNECT_DELAY: Duration = Duration::from_secs(4);
const SNAPSHOT_SAVE_DEBOUNCE: Duration = Duration::from_secs(180);
const F1_SIGNALR_CHANNELS: &[&str] = &["session-info", "participants", "ranks", "gaps", "laps"];

#[derive(Debug, Clone)]
struct F1Snapshot {
    header: TimingHeader,
    entries: Vec<TimingEntry>,
    session_id: Option<String>,
    fingerprint: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedF1Snapshot {
    saved_unix_ms: u64,
    session_id: Option<String>,
    meaningful_fingerprint: u64,
    header: TimingHeader,
    entries: Vec<TimingEntry>,
}

#[derive(Debug, Deserialize)]
struct SessionResultsResponse {
    results: Vec<SessionResultRow>,
}

#[derive(Debug, Deserialize)]
struct SessionResultRow {
    #[serde(rename = "sessionParticipantId")]
    session_participant_id: u64,
    #[serde(rename = "overallFinishedAt")]
    overall_finished_at: Option<u32>,
    #[serde(rename = "overallGapFromFirst")]
    overall_gap_from_first: Option<i64>,
    #[serde(rename = "overallGapFromFirstLaps")]
    overall_gap_from_first_laps: Option<i64>,
    #[serde(rename = "gapFromFirst")]
    gap_from_first: Option<i64>,
    #[serde(rename = "gapFromFirstLaps")]
    gap_from_first_laps: Option<i64>,
    #[serde(rename = "numberOfLapsCompleted")]
    number_of_laps_completed: Option<u32>,
    #[serde(rename = "bestLapTime")]
    best_lap_time: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct SessionParticipantRow {
    id: u64,
    #[serde(rename = "carNumber")]
    car_number: Option<String>,
    #[serde(rename = "teamName")]
    team_name: Option<String>,
    manufacturer: Option<String>,
    #[serde(default)]
    drivers: Vec<ParticipantDriver>,
}

#[derive(Debug, Deserialize)]
struct ParticipantDriver {
    #[serde(rename = "displayName")]
    display_name: Option<String>,
}

#[derive(Debug, Default, Clone)]
struct F1LiveState {
    header: TimingHeader,
    rows: HashMap<String, F1CarState>,
}

#[derive(Debug, Default, Clone)]
struct F1CarState {
    car_number: String,
    position: Option<u32>,
    driver: Option<String>,
    team: Option<String>,
    laps: Option<u32>,
    gap_to_leader: Option<String>,
    interval: Option<String>,
    last_lap_ms: Option<i64>,
    best_lap_ms: Option<i64>,
    pit: Option<bool>,
}

pub fn worker(tx: Sender<TimingMessage>, source_id: u64, stop_rx: Receiver<()>) {
    worker_with_debug(tx, source_id, stop_rx, SeriesDebugOutput::Silent)
}

pub fn worker_with_debug(
    tx: Sender<TimingMessage>,
    source_id: u64,
    stop_rx: Receiver<()>,
    debug_output: SeriesDebugOutput,
) {
    let client = match Client::builder().timeout(Duration::from_secs(12)).build() {
        Ok(c) => c,
        Err(err) => {
            let _ = tx.send(TimingMessage::Error {
                source_id,
                text: format!("F1 HTTP client init failed: {err}"),
            });
            return;
        }
    };

    let mut persist = PersistState::new(snapshot_path());
    let mut last_snapshot = restore_snapshot_from_disk(&mut persist, &tx, source_id, &debug_output);
    let mut last_session_id = last_snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.session_id.clone());
    let mut offline_detail_logged = false;

    loop {
        if stop_rx.try_recv().is_ok() {
            if let Some(snapshot) = last_snapshot.as_ref() {
                if persist.dirty_since_last_save {
                    persist_snapshot(&mut persist, snapshot, &debug_output);
                }
            }
            break;
        }

        let snapshot_result = match resolve_live_f1_sid(&client) {
            Ok(sid) => {
                offline_detail_logged = false;
                let _ = tx.send(TimingMessage::Status {
                    source_id,
                    text: format!("F1 live session sid={sid}"),
                });
                build_live_snapshot_with_signalr_fallback(&client, sid, &debug_output)
            }
            Err(live_err) => {
                let _ = tx.send(TimingMessage::Status {
                    source_id,
                    text: "F1 offline: latest race results".to_string(),
                });
                if !offline_detail_logged {
                    log_series_debug(
                        &debug_output,
                        "F1",
                        format!(
                            "No active Formula 1 live session; showing latest finished race results ({live_err}) [ts={}]",
                            now_unix_ms()
                        ),
                    );
                    offline_detail_logged = true;
                }
                build_latest_finished_race_snapshot(&client)
            }
        };

        match snapshot_result {
            Ok(snapshot) => {
                emit_snapshot(
                    (&tx, source_id),
                    snapshot,
                    &mut persist,
                    &mut last_snapshot,
                    &mut last_session_id,
                    &debug_output,
                );
            }
            Err(err) => {
                let _ = tx.send(TimingMessage::Error {
                    source_id,
                    text: format!("F1 update failed: {err}"),
                });
            }
        }

        if stop_rx.recv_timeout(RECONNECT_DELAY).is_ok() {
            break;
        }
    }
}

fn resolve_live_f1_sid(client: &Client) -> Result<u64, String> {
    resolve_live_sid_for_series(client, F1_SERIES_ID)
        .map_err(|err| format!("no active Formula 1 live session found ({err})"))
}

fn build_live_snapshot_with_signalr_fallback(
    client: &Client,
    sid: u64,
    debug: &SeriesDebugOutput,
) -> Result<F1Snapshot, String> {
    match build_live_snapshot_via_signalr(client, sid) {
        Ok(snapshot) => Ok(snapshot),
        Err(err) => {
            log_series_debug(
                debug,
                "F1",
                format!("SignalR live path failed, falling back to REST live polling: {err}"),
            );
            build_live_snapshot(client, sid)
        }
    }
}

fn build_live_snapshot(client: &Client, sid: u64) -> Result<F1Snapshot, String> {
    let mut state = F1LiveState::default();
    apply_live_session_info(&mut state, &fetch_live_json(client, sid, "session-info")?);
    apply_live_participants(&mut state, &fetch_live_json(client, sid, "participants")?);
    apply_live_ranks(&mut state, &fetch_live_json(client, sid, "ranks")?);
    apply_live_gaps(&mut state, &fetch_live_json(client, sid, "gaps")?);
    apply_live_laps(&mut state, &fetch_live_json(client, sid, "laps")?);
    snapshot_from_live_state(state)
}

fn build_live_snapshot_via_signalr(client: &Client, sid: u64) -> Result<F1Snapshot, String> {
    let negotiated =
        negotiate(client).map_err(|err| format!("F1 SignalR negotiate failed: {err}"))?;
    let ws_url = crate::adapters::insights::signalr_ws::websocket_url_from_negotiate(
        &negotiated.url,
        &negotiated.access_token,
    );
    let request = build_signalr_request(&ws_url)
        .map_err(|err| format!("F1 SignalR request build failed: {err}"))?;
    let mut socket = connect_signalr(request)
        .map_err(|err| format!("F1 SignalR websocket connect failed: {err}"))?;

    send_signalr_handshake(&mut socket)
        .map_err(|err| format!("F1 SignalR handshake failed: {err}"))?;

    let mut invocation_id = 1_u64;
    for channel in F1_SIGNALR_CHANNELS {
        send_join_group(&mut socket, &mut invocation_id, sid, channel)
            .map_err(|err| format!("F1 SignalR JoinGroup failed for channel {channel}: {err}"))?;
    }

    let mut state = F1LiveState::default();
    let mut saw_invocation = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(4);
    while std::time::Instant::now() < deadline {
        let message = match socket.read() {
            Ok(message) => message,
            Err(err) => {
                if crate::adapters::insights::signalr_ws::is_retriable_timeout(&err) {
                    continue;
                }
                return Err(format!("F1 SignalR read failed: {err}"));
            }
        };
        let text = match message {
            tungstenite::Message::Text(text) => text,
            tungstenite::Message::Binary(data) => match String::from_utf8(data.to_vec()) {
                Ok(text) => text.into(),
                Err(_) => continue,
            },
            tungstenite::Message::Ping(payload) => {
                let _ = socket.send(tungstenite::Message::Pong(payload));
                continue;
            }
            tungstenite::Message::Pong(_) => continue,
            tungstenite::Message::Close(_) => break,
            _ => continue,
        };

        for frame in split_signalr_frames(&text) {
            match parse_signalr_frame(frame) {
                SignalRFrame::Invocation { target, arguments } => {
                    if !target.starts_with("lv-") {
                        continue;
                    }
                    saw_invocation = true;
                    apply_live_signalr_arguments(&mut state, &target, &arguments);
                }
                SignalRFrame::Completion {
                    invocation_id,
                    error: Some(error),
                } => {
                    let label = invocation_id.unwrap_or_else(|| "?".to_string());
                    return Err(format!("F1 SignalR invocation {label} failed: {error}"));
                }
                SignalRFrame::Close => break,
                _ => {}
            }
        }
    }

    if !saw_invocation {
        return Err("F1 SignalR produced no live invocation data".to_string());
    }

    snapshot_from_live_state(state)
}

fn apply_live_signalr_arguments(state: &mut F1LiveState, target: &str, arguments: &[Value]) {
    for argument in arguments {
        match target {
            "lv-session-info" => apply_live_session_info(state, argument),
            "lv-participants" => apply_live_participants(state, argument),
            "lv-ranks" => apply_live_ranks(state, argument),
            "lv-gaps" => apply_live_gaps(state, argument),
            "lv-laps" => apply_live_laps(state, argument),
            _ => {}
        }
    }
}

fn build_latest_finished_race_snapshot(client: &Client) -> Result<F1Snapshot, String> {
    let sessions = fetch_meta_sessions_for_series(client, F1_SERIES_ID)
        .map_err(|err| format!("F1 meta sessions request failed: {err}"))?;
    let Some(session) = choose_latest_finished_race_session(&sessions) else {
        return Err("no finished Formula 1 race with results found".to_string());
    };

    let results_url = format!(
        "https://insights.griiip.com/meta/sessions/{}/results",
        session.id
    );
    let results_response = client.get(&results_url).send().map_err(|err| {
        format!(
            "F1 results request failed for session {}: {err}",
            session.id
        )
    })?;
    if !results_response.status().is_success() {
        return Err(format!(
            "F1 results request failed for session {} with HTTP {}",
            session.id,
            results_response.status()
        ));
    }
    let results_body = results_response.text().map_err(|err| {
        format!(
            "F1 results body read failed for session {}: {err}",
            session.id
        )
    })?;
    let results_payload = serde_json::from_str::<SessionResultsResponse>(&results_body)
        .map_err(|err| format!("F1 results decode failed for session {}: {err}", session.id))?;

    let participants_url = format!(
        "https://insights.griiip.com/meta/sessions/{}/participants",
        session.id
    );
    let participants_response = client.get(&participants_url).send().map_err(|err| {
        format!(
            "F1 participants request failed for session {}: {err}",
            session.id
        )
    })?;
    if !participants_response.status().is_success() {
        return Err(format!(
            "F1 participants request failed for session {} with HTTP {}",
            session.id,
            participants_response.status()
        ));
    }
    let participants_body = participants_response.text().map_err(|err| {
        format!(
            "F1 participants body read failed for session {}: {err}",
            session.id
        )
    })?;
    let participants = serde_json::from_str::<Vec<SessionParticipantRow>>(&participants_body)
        .map_err(|err| {
            format!(
                "F1 participants decode failed for session {}: {err}",
                session.id
            )
        })?;

    let mut participants_by_id = HashMap::new();
    for participant in participants {
        participants_by_id.insert(participant.id, participant);
    }

    let mut entries = Vec::new();
    for (idx, row) in results_payload.results.into_iter().enumerate() {
        let participant = participants_by_id.get(&row.session_participant_id);
        let car_number = participant
            .and_then(|item| item.car_number.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("-")
            .to_string();
        let team = participant
            .and_then(|item| item.team_name.clone())
            .unwrap_or_else(|| "-".to_string());
        let driver = participant
            .and_then(|item| item.drivers.first())
            .and_then(|driver| driver.display_name.as_deref())
            .map(normalize_driver_name)
            .unwrap_or_else(|| "-".to_string());

        entries.push(TimingEntry {
            position: row.overall_finished_at.unwrap_or((idx + 1) as u32),
            car_number: car_number.clone(),
            class_name: "F1".to_string(),
            class_rank: row
                .overall_finished_at
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string()),
            driver,
            vehicle: participant
                .and_then(|item| item.manufacturer.clone())
                .unwrap_or_else(|| "-".to_string()),
            team,
            laps: row
                .number_of_laps_completed
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string()),
            gap_overall: format_gap(row.overall_gap_from_first, row.overall_gap_from_first_laps)
                .unwrap_or_else(|| "-".to_string()),
            gap_class: format_gap(row.gap_from_first, row.gap_from_first_laps)
                .unwrap_or_else(|| "-".to_string()),
            gap_next_in_class: "-".to_string(),
            last_lap: "-".to_string(),
            best_lap: row
                .best_lap_time
                .map(format_lap_time_ms)
                .unwrap_or_else(|| "-".to_string()),
            sector_1: "-".to_string(),
            sector_2: "-".to_string(),
            sector_3: "-".to_string(),
            sector_4: "-".to_string(),
            sector_5: "-".to_string(),
            best_lap_no: "-".to_string(),
            pit: "No".to_string(),
            pit_stops: "-".to_string(),
            fastest_driver: "-".to_string(),
            stable_id: if car_number != "-" {
                format!("f1:{car_number}")
            } else {
                format!("f1:participant:{}", row.session_participant_id)
            },
        });
    }
    entries.sort_by_key(|entry| entry.position);

    let header = TimingHeader {
        session_name: session.name.clone().unwrap_or_else(|| "Race".to_string()),
        session_type_raw: session
            .session_type
            .clone()
            .unwrap_or_else(|| "Race".to_string()),
        event_name: session
            .event
            .as_ref()
            .and_then(|event| event.name.clone())
            .unwrap_or_else(|| "Formula 1".to_string()),
        track_name: session
            .track_config
            .as_ref()
            .and_then(|track| track.name.clone())
            .or_else(|| {
                session
                    .event
                    .as_ref()
                    .and_then(|event| event.track_config.as_ref())
                    .and_then(|track| track.name.clone())
            })
            .unwrap_or_else(|| "-".to_string()),
        day_time: "-".to_string(),
        flag: "Checkered".to_string(),
        time_to_go: "00:00".to_string(),
        ..TimingHeader::default()
    };

    snapshot_from_parts(header, entries)
}

fn apply_live_session_info(state: &mut F1LiveState, payload: &Value) {
    let Some(map) = payload.as_object() else {
        return;
    };
    set_header_text(&mut state.header.event_name, map_str(map, "eventName"));
    set_header_text(&mut state.header.session_name, map_str(map, "sessionName"));
    set_header_text(&mut state.header.track_name, map_str(map, "trackName"));
    set_header_text(
        &mut state.header.session_type_raw,
        map_text(map, "sessionType"),
    );
    set_header_text(
        &mut state.header.flag,
        map_str(map, "connectionStatus").map(|flag| normalize_flag(&flag)),
    );
}

fn apply_live_participants(state: &mut F1LiveState, payload: &Value) {
    for row in payload_rows(payload) {
        let Some(map) = row.as_object() else {
            continue;
        };
        let Some((_key, entry)) = upsert_car_state(state, map) else {
            continue;
        };
        set_opt_string(
            &mut entry.team,
            map_str(map, "teamName").or_else(|| map_str(map, "displayName")),
        );
        set_opt_string(
            &mut entry.driver,
            current_driver_name(map).map(|name| normalize_driver_name(&name)),
        );
    }
}

fn apply_live_ranks(state: &mut F1LiveState, payload: &Value) {
    for row in payload_rows(payload) {
        let Some(map) = row.as_object() else {
            continue;
        };
        let Some((_key, entry)) = upsert_car_state(state, map) else {
            continue;
        };
        set_opt_u32(&mut entry.position, map_u32(map, "overallPosition"));
        set_opt_u32(&mut entry.laps, map_u32(map, "lapNumber"));
    }
}

fn apply_live_gaps(state: &mut F1LiveState, payload: &Value) {
    for row in payload_rows(payload) {
        let Some(map) = row.as_object() else {
            continue;
        };
        let Some((_key, entry)) = upsert_car_state(state, map) else {
            continue;
        };
        set_opt_string(
            &mut entry.gap_to_leader,
            format_gap(
                map_i64(map, "gapToFirstMillis"),
                map_i64(map, "gapToFirstLaps"),
            ),
        );
        set_opt_string(
            &mut entry.interval,
            format_gap(
                map_i64(map, "gapToAheadMillis"),
                map_i64(map, "gapToAheadLaps"),
            ),
        );
    }
}

fn apply_live_laps(state: &mut F1LiveState, payload: &Value) {
    for row in payload_rows(payload) {
        let Some(map) = row.as_object() else {
            continue;
        };
        let Some((_key, entry)) = upsert_car_state(state, map) else {
            continue;
        };
        if let Some(ms) = map_i64(map, "lapTimeMillis") {
            if ms > 0 {
                if entry.best_lap_ms.is_none() || Some(ms) < entry.best_lap_ms {
                    entry.best_lap_ms = Some(ms);
                }
                entry.last_lap_ms = Some(ms);
            }
        }
        if let Some(pit) = map_bool(map, "isEndedInPit").or_else(|| map_bool(map, "isStartedInPit"))
        {
            entry.pit = Some(pit);
        }
    }
}

fn snapshot_from_live_state(state: F1LiveState) -> Result<F1Snapshot, String> {
    let mut entries: Vec<TimingEntry> = state
        .rows
        .values()
        .filter(|row| !row.car_number.trim().is_empty())
        .cloned()
        .map(|row| TimingEntry {
            position: row.position.unwrap_or(999),
            car_number: row.car_number.clone(),
            class_name: "F1".to_string(),
            class_rank: row
                .position
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string()),
            driver: row.driver.unwrap_or_else(|| "-".to_string()),
            vehicle: "-".to_string(),
            team: row.team.unwrap_or_else(|| "-".to_string()),
            laps: row
                .laps
                .map(|value| value.to_string())
                .unwrap_or_else(|| "-".to_string()),
            gap_overall: row.gap_to_leader.unwrap_or_else(|| "-".to_string()),
            gap_class: row.interval.unwrap_or_else(|| "-".to_string()),
            gap_next_in_class: "-".to_string(),
            last_lap: row
                .last_lap_ms
                .map(format_lap_time_ms)
                .unwrap_or_else(|| "-".to_string()),
            best_lap: row
                .best_lap_ms
                .map(format_lap_time_ms)
                .unwrap_or_else(|| "-".to_string()),
            sector_1: "-".to_string(),
            sector_2: "-".to_string(),
            sector_3: "-".to_string(),
            sector_4: "-".to_string(),
            sector_5: "-".to_string(),
            best_lap_no: "-".to_string(),
            pit: if row.pit.unwrap_or(false) {
                "Yes".to_string()
            } else {
                "No".to_string()
            },
            pit_stops: "-".to_string(),
            fastest_driver: "-".to_string(),
            stable_id: format!("f1:{}", row.car_number),
        })
        .collect();
    entries.sort_by_key(|entry| entry.position);

    let mut header = state.header;
    if header.event_name.trim().is_empty() {
        header.event_name = "Formula 1".to_string();
    }
    if header.session_name.trim().is_empty() {
        header.session_name = "-".to_string();
    }
    if header.track_name.trim().is_empty() {
        header.track_name = "-".to_string();
    }
    if header.flag.trim().is_empty() {
        header.flag = "-".to_string();
    }
    if entries.is_empty() {
        return Err("F1 live snapshot contains no entries".to_string());
    }

    snapshot_from_parts(header, entries)
}

fn snapshot_from_parts(
    header: TimingHeader,
    entries: Vec<TimingEntry>,
) -> Result<F1Snapshot, String> {
    if entries.is_empty() {
        return Err("F1 snapshot contains no entries".to_string());
    }
    let session_id = derive_session_identifier(&header);
    Ok(F1Snapshot {
        fingerprint: meaningful_snapshot_fingerprint(&header, &entries),
        session_id,
        header,
        entries,
    })
}

fn emit_snapshot(
    emitter: (&Sender<TimingMessage>, u64),
    snapshot: F1Snapshot,
    persist: &mut PersistState,
    last_snapshot: &mut Option<F1Snapshot>,
    last_session_id: &mut Option<String>,
    debug_output: &SeriesDebugOutput,
) {
    let materially_changed = last_snapshot
        .as_ref()
        .map(|prev| prev.fingerprint != snapshot.fingerprint)
        .unwrap_or(true);
    if materially_changed {
        persist.dirty_since_last_save = true;
    }

    let first_real_of_session =
        !snapshot.entries.is_empty() && snapshot.session_id != *last_session_id;
    let session_complete = snapshot.header.flag.eq_ignore_ascii_case("checkered");
    let never_persisted = persist.last_persisted_hash.is_none();
    let save_now = never_persisted
        || first_real_of_session
        || session_complete
        || (persist.dirty_since_last_save
            && debounce_elapsed(persist.last_save_at, SNAPSHOT_SAVE_DEBOUNCE));
    if save_now {
        persist_snapshot(persist, &snapshot, debug_output);
    }

    *last_session_id = snapshot.session_id.clone();
    *last_snapshot = Some(snapshot.clone());

    let (tx, source_id) = emitter;
    let _ = tx.send(TimingMessage::Snapshot {
        source_id,
        header: snapshot.header,
        entries: snapshot.entries,
    });
}

fn payload_rows(payload: &Value) -> Vec<&Value> {
    match payload {
        Value::Array(rows) => rows.iter().collect(),
        Value::Object(map) => {
            if let Some(items) = map.get("items").and_then(Value::as_array) {
                items.iter().collect()
            } else {
                vec![payload]
            }
        }
        _ => Vec::new(),
    }
}

fn upsert_car_state<'a>(
    state: &'a mut F1LiveState,
    row: &Map<String, Value>,
) -> Option<(String, &'a mut F1CarState)> {
    let key = map_i64(row, "pid")
        .filter(|pid| *pid > 0)
        .map(|pid| format!("pid:{pid}"))
        .or_else(|| map_str(row, "carNumber").map(|car| format!("car:{car}")))?;
    let car_number = map_str(row, "carNumber").unwrap_or_else(|| "-".to_string());
    let entry = state.rows.entry(key.clone()).or_default();
    if entry.car_number.trim().is_empty() && !car_number.trim().is_empty() {
        entry.car_number = car_number;
    }
    Some((key, entry))
}

fn current_driver_name(row: &Map<String, Value>) -> Option<String> {
    if let Some(drivers) = row.get("drivers").and_then(Value::as_array) {
        for driver in drivers {
            let Some(driver_map) = driver.as_object() else {
                continue;
            };
            if let Some(name) = map_str(driver_map, "displayName") {
                return Some(name);
            }
        }
    }
    map_str(row, "displayName")
}

fn normalize_flag(raw: &str) -> String {
    let cleaned = raw.trim();
    if cleaned.is_empty() {
        return "-".to_string();
    }
    let mut chars = cleaned.chars();
    let Some(first) = chars.next() else {
        return "-".to_string();
    };
    let mut out = String::new();
    out.extend(first.to_uppercase());
    for ch in chars {
        out.extend(ch.to_lowercase());
    }
    out
}

fn set_header_text(slot: &mut String, incoming: Option<String>) {
    let Some(incoming) = incoming else {
        return;
    };
    if incoming.trim().is_empty() {
        return;
    }
    *slot = incoming;
}

fn set_opt_string(slot: &mut Option<String>, incoming: Option<String>) {
    let Some(incoming) = incoming else {
        return;
    };
    if incoming.trim().is_empty() {
        return;
    }
    *slot = Some(incoming);
}

fn set_opt_u32(slot: &mut Option<u32>, incoming: Option<u32>) {
    if incoming.is_some() {
        *slot = incoming;
    }
}

fn meaningful_snapshot_fingerprint(header: &TimingHeader, entries: &[TimingEntry]) -> u64 {
    let mut hasher = base_snapshot_fingerprint(header);
    for entry in entries {
        hash_entry_common_fields(&mut hasher, entry);
        entry.gap_class.trim().hash(&mut hasher);
        entry.pit_stops.trim().hash(&mut hasher);
    }
    hasher.finish()
}

fn snapshot_path() -> Option<PathBuf> {
    data_local_snapshot_path("f1_snapshot.json")
}

fn persist_snapshot(runtime: &mut PersistState, snapshot: &F1Snapshot, debug: &SeriesDebugOutput) {
    let Some(path) = runtime.path.as_ref() else {
        return;
    };
    let payload = PersistedF1Snapshot {
        saved_unix_ms: now_unix_ms(),
        session_id: snapshot.session_id.clone(),
        meaningful_fingerprint: snapshot.fingerprint,
        header: snapshot.header.clone(),
        entries: snapshot.entries.clone(),
    };
    if let Err(err) = write_json_pretty(path, &payload) {
        log_series_debug(debug, "F1", format!("snapshot persist failed: {err}"));
        return;
    }
    runtime.last_persisted_hash = Some(snapshot.fingerprint);
    runtime.last_save_at = Some(SystemTime::now());
    runtime.dirty_since_last_save = false;
}

fn restore_snapshot_from_disk(
    runtime: &mut PersistState,
    tx: &Sender<TimingMessage>,
    source_id: u64,
    debug: &SeriesDebugOutput,
) -> Option<F1Snapshot> {
    let path = runtime.path.as_ref()?;
    let saved = read_json::<PersistedF1Snapshot>(path)?;
    let snapshot = F1Snapshot {
        header: saved.header,
        entries: saved.entries,
        session_id: saved.session_id,
        fingerprint: saved.meaningful_fingerprint,
    };
    runtime.last_persisted_hash = Some(snapshot.fingerprint);
    runtime.last_save_at = Some(SystemTime::now());
    let _ = tx.send(TimingMessage::Snapshot {
        source_id,
        header: snapshot.header.clone(),
        entries: snapshot.entries.clone(),
    });
    log_series_debug(
        debug,
        "F1",
        format!("snapshot restored from {}", path.display()),
    );
    Some(snapshot)
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}
