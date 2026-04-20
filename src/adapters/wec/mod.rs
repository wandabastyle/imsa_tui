use std::{
    collections::{BTreeMap, HashMap},
    hash::Hasher,
    net::TcpStream,
    path::PathBuf,
    sync::mpsc::{Receiver, Sender},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tungstenite::{
    connect,
    http::header::{HeaderValue, ORIGIN, USER_AGENT},
    stream::MaybeTlsStream,
    Error as WsError, Message, WebSocket,
};

use crate::{
    adapters::insights::session::{
        fetch_meta_sessions_for_series, resolve_live_sid_for_series, MetaSessionItem,
    },
    snapshot_runtime::{
        base_snapshot_fingerprint, derive_session_identifier, hash_entry_common_fields,
    },
    timing::{TimingClassColor, TimingEntry, TimingHeader, TimingMessage},
    timing_persist::{
        data_local_snapshot_path, debounce_elapsed, log_series_debug, read_json, write_json_pretty,
        PersistState, SeriesDebugOutput,
    },
};

const WEC_SERIES_ID: u64 = 10;
const NEGOTIATE_URL: &str =
    "https://insights.griiip.com/live-session-stream/negotiate?negotiateVersion=1";
const ORIGIN_URL: &str = "https://insights.griiip.com";
const LIVE_BASE_URL: &str = "https://insights.griiip.com/live";
const RECONNECT_DELAY: Duration = Duration::from_secs(4);
const SNAPSHOT_SAVE_DEBOUNCE: Duration = Duration::from_secs(180);
const SIGNALR_RS: char = '\u{1e}';
const WEC_SIGNALR_CHANNELS: &[&str] = &[
    "session-info",
    "participants",
    "ranks",
    "gaps",
    "laps",
    "sectors",
    "race-flags",
    "session-clock",
];

#[derive(Debug, Deserialize)]
struct NegotiateResponse {
    url: String,
    #[serde(rename = "accessToken")]
    access_token: String,
}

#[derive(Debug)]
enum SignalRFrame {
    HandshakeAck,
    Invocation {
        target: String,
        arguments: Vec<Value>,
    },
    Completion {
        invocation_id: Option<String>,
        error: Option<String>,
    },
    Ping,
    Close,
    Unknown,
}

#[derive(Debug, Clone)]
struct WecSnapshot {
    header: TimingHeader,
    entries: Vec<TimingEntry>,
    session_id: Option<String>,
    fingerprint: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedWecSnapshot {
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
    #[serde(rename = "finishedAt")]
    finished_at: Option<u32>,
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
    #[serde(rename = "bestSectorsMillis1")]
    best_sector_1_ms: Option<i64>,
    #[serde(rename = "bestSectorsMillis2")]
    best_sector_2_ms: Option<i64>,
    #[serde(rename = "bestSectorsMillis3")]
    best_sector_3_ms: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct SessionParticipantRow {
    id: u64,
    #[serde(rename = "carNumber")]
    car_number: Option<String>,
    #[serde(rename = "classId")]
    class_id: Option<String>,
    #[serde(rename = "teamName")]
    team_name: Option<String>,
    manufacturer: Option<String>,
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    #[serde(default)]
    drivers: Vec<ParticipantDriver>,
}

#[derive(Debug, Deserialize)]
struct ParticipantDriver {
    #[serde(rename = "displayName")]
    display_name: Option<String>,
}

#[derive(Debug, Default, Clone)]
struct WecLiveState {
    header: TimingHeader,
    rows: HashMap<String, WecCarState>,
    class_names: HashMap<String, String>,
    class_colors: BTreeMap<String, TimingClassColor>,
}

#[derive(Debug, Default, Clone)]
struct WecCarState {
    car_number: String,
    class_id: Option<String>,
    class_name: Option<String>,
    position: Option<u32>,
    class_rank: Option<u32>,
    driver: Option<String>,
    vehicle: Option<String>,
    team: Option<String>,
    laps: Option<u32>,
    gap_overall: Option<String>,
    gap_next_in_class: Option<String>,
    last_lap_ms: Option<i64>,
    best_lap_ms: Option<i64>,
    best_lap_no: Option<u32>,
    pit: Option<bool>,
    sector_times: [Option<i64>; 3],
    sector_laps: [Option<u32>; 3],
}

pub fn websocket_worker(tx: Sender<TimingMessage>, source_id: u64, stop_rx: Receiver<()>) {
    websocket_worker_with_debug(tx, source_id, stop_rx, SeriesDebugOutput::Silent)
}

pub fn websocket_worker_with_debug(
    tx: Sender<TimingMessage>,
    source_id: u64,
    stop_rx: Receiver<()>,
    debug_output: SeriesDebugOutput,
) {
    let client = match Client::builder().timeout(Duration::from_secs(12)).build() {
        Ok(client) => client,
        Err(err) => {
            let _ = tx.send(TimingMessage::Error {
                source_id,
                text: format!("WEC HTTP client init failed: {err}"),
            });
            return;
        }
    };

    let mut persist = PersistState::new(wec_snapshot_path());
    let mut last_snapshot = restore_snapshot_from_disk(&mut persist, &tx, source_id, &debug_output);
    if last_snapshot.is_some() {
        let _ = tx.send(TimingMessage::Status {
            source_id,
            text: "[SNAPSHOT] Restored from saved data".to_string(),
        });
    }
    let mut last_session_id = last_snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.session_id.clone());
    let mut fallback_detail_logged = false;

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
            text: "Connecting to WEC live stream...".to_string(),
        });

        let sid = match resolve_active_sid(&client) {
            Ok(sid) => {
                fallback_detail_logged = false;
                sid
            }
            Err(err) => {
                match fetch_latest_finished_race_snapshot(&client) {
                    Ok(snapshot) => {
                        emit_snapshot(
                            (&tx, source_id),
                            snapshot.header,
                            snapshot.entries,
                            &mut persist,
                            &mut last_snapshot,
                            &mut last_session_id,
                            &debug_output,
                        );
                        if !fallback_detail_logged {
                            log_series_debug(
                                &debug_output,
                                "WEC",
                                format!(
                                    "No active FIA WEC live session; showing latest finished race results [ts={}]",
                                    now_unix_ms()
                                ),
                            );
                            fallback_detail_logged = true;
                        }
                        let _ = tx.send(TimingMessage::Status {
                            source_id,
                            text: "WEC offline: latest race results".to_string(),
                        });
                    }
                    Err(fallback_err) => {
                        let _ = tx.send(TimingMessage::Error {
                            source_id,
                            text: format!("{err}; fallback failed: {fallback_err}"),
                        });
                    }
                }
                if stop_rx.recv_timeout(RECONNECT_DELAY).is_ok() {
                    break;
                }
                continue;
            }
        };

        let negotiated = match negotiate(&client) {
            Ok(negotiated) => negotiated,
            Err(err) => {
                let _ = tx.send(TimingMessage::Error {
                    source_id,
                    text: err,
                });
                if stop_rx.recv_timeout(RECONNECT_DELAY).is_ok() {
                    break;
                }
                continue;
            }
        };

        let ws_url = websocket_url_from_negotiate(&negotiated.url, &negotiated.access_token);
        let request = match build_request(&ws_url) {
            Ok(request) => request,
            Err(err) => {
                let _ = tx.send(TimingMessage::Error {
                    source_id,
                    text: err,
                });
                if stop_rx.recv_timeout(RECONNECT_DELAY).is_ok() {
                    break;
                }
                continue;
            }
        };

        let (mut socket, _) = match connect(request) {
            Ok(pair) => pair,
            Err(err) => {
                let _ = tx.send(TimingMessage::Error {
                    source_id,
                    text: format!("WEC websocket connect failed: {err}"),
                });
                if stop_rx.recv_timeout(RECONNECT_DELAY).is_ok() {
                    break;
                }
                continue;
            }
        };
        set_socket_timeout(&mut socket);

        if let Err(err) = send_signalr_handshake(&mut socket) {
            let _ = tx.send(TimingMessage::Error {
                source_id,
                text: err,
            });
            if stop_rx.recv_timeout(RECONNECT_DELAY).is_ok() {
                break;
            }
            continue;
        }

        let mut handshake_complete = false;
        for _ in 0..6 {
            match read_signalr_text(&mut socket) {
                Ok(Some(raw)) => {
                    let mut failed = false;
                    for frame in split_signalr_frames(&raw) {
                        match parse_signalr_frame(frame) {
                            SignalRFrame::HandshakeAck => handshake_complete = true,
                            SignalRFrame::Close => {
                                failed = true;
                                break;
                            }
                            _ => {}
                        }
                    }
                    if failed || handshake_complete {
                        break;
                    }
                }
                Ok(None) => {}
                Err(err) => {
                    let _ = tx.send(TimingMessage::Error {
                        source_id,
                        text: err,
                    });
                    break;
                }
            }
        }

        if !handshake_complete {
            let _ = tx.send(TimingMessage::Error {
                source_id,
                text: "WEC SignalR handshake did not complete".to_string(),
            });
            if stop_rx.recv_timeout(RECONNECT_DELAY).is_ok() {
                break;
            }
            continue;
        }

        let mut invocation_id = 1_u64;
        for channel in WEC_SIGNALR_CHANNELS {
            if let Err(err) = join_group(&mut socket, &mut invocation_id, sid, channel) {
                let _ = tx.send(TimingMessage::Error {
                    source_id,
                    text: err,
                });
                if stop_rx.recv_timeout(RECONNECT_DELAY).is_ok() {
                    break 'outer;
                }
                continue 'outer;
            }
        }

        let mut live_state = WecLiveState::default();
        if let Err(err) = bootstrap_live_state(&client, sid, &mut live_state) {
            let _ = tx.send(TimingMessage::Error {
                source_id,
                text: err,
            });
        } else if let Some((header, entries)) = snapshot_from_live_state(&live_state) {
            emit_snapshot(
                (&tx, source_id),
                header,
                entries,
                &mut persist,
                &mut last_snapshot,
                &mut last_session_id,
                &debug_output,
            );
        }

        let _ = tx.send(TimingMessage::Status {
            source_id,
            text: format!("WEC stream connected (sid={sid})"),
        });

        loop {
            if stop_rx.try_recv().is_ok() {
                if let Some(snapshot) = last_snapshot.as_ref() {
                    if persist.dirty_since_last_save {
                        persist_snapshot(&mut persist, snapshot, &debug_output);
                    }
                }
                break 'outer;
            }

            let raw = match read_signalr_text(&mut socket) {
                Ok(raw) => raw,
                Err(err) => {
                    let _ = tx.send(TimingMessage::Error {
                        source_id,
                        text: err,
                    });
                    break;
                }
            };

            let Some(raw) = raw else {
                continue;
            };

            let mut closed = false;
            for frame in split_signalr_frames(&raw) {
                match parse_signalr_frame(frame) {
                    SignalRFrame::Invocation { target, arguments } => {
                        if !target.starts_with("lv-") {
                            continue;
                        }
                        if apply_signalr_arguments(&mut live_state, &target, &arguments) {
                            if let Some((header, entries)) = snapshot_from_live_state(&live_state) {
                                emit_snapshot(
                                    (&tx, source_id),
                                    header,
                                    entries,
                                    &mut persist,
                                    &mut last_snapshot,
                                    &mut last_session_id,
                                    &debug_output,
                                );
                            }
                        }
                    }
                    SignalRFrame::Completion {
                        invocation_id,
                        error,
                    } => {
                        if let Some(error) = error {
                            let label = invocation_id.unwrap_or_else(|| "?".to_string());
                            let _ = tx.send(TimingMessage::Error {
                                source_id,
                                text: format!("WEC SignalR invocation {label} failed: {error}"),
                            });
                        }
                    }
                    SignalRFrame::Close => {
                        closed = true;
                        break;
                    }
                    SignalRFrame::Ping | SignalRFrame::HandshakeAck | SignalRFrame::Unknown => {}
                }
            }

            if closed {
                let _ = tx.send(TimingMessage::Error {
                    source_id,
                    text: "WEC websocket closed".to_string(),
                });
                break;
            }
        }

        let _ = tx.send(TimingMessage::Status {
            source_id,
            text: "WEC reconnecting in 4s...".to_string(),
        });
        if stop_rx.recv_timeout(RECONNECT_DELAY).is_ok() {
            break;
        }
    }
}

fn send_signalr_handshake(socket: &mut WebSocket<MaybeTlsStream<TcpStream>>) -> Result<(), String> {
    let payload = format!("{{\"protocol\":\"json\",\"version\":1}}{SIGNALR_RS}");
    socket
        .send(Message::Text(payload.into()))
        .map_err(|err| format!("WEC handshake send failed: {err}"))
}

fn resolve_active_sid(client: &Client) -> Result<u64, String> {
    resolve_live_sid_for_series(client, WEC_SERIES_ID)
        .map_err(|err| format!("No active FIA WEC session found in live schedule ({err})"))
}

#[cfg(test)]
#[derive(Debug, Clone)]
struct SessionScheduleItem {
    sid: u64,
    is_started: bool,
    connection_status: Option<String>,
}

#[cfg(test)]
fn choose_candidate_sids(sessions: &[SessionScheduleItem]) -> Result<Vec<u64>, String> {
    if sessions.is_empty() {
        return Err("WEC session schedule returned no sessions".to_string());
    }

    let mut prioritized = Vec::with_capacity(sessions.len());
    for session in sessions.iter().filter(|session| {
        session.is_started && !is_closed_status(session.connection_status.as_deref())
    }) {
        prioritized.push(session.sid);
    }
    for session in sessions {
        if !prioritized.contains(&session.sid) {
            prioritized.push(session.sid);
        }
    }
    Ok(prioritized)
}

#[cfg(test)]
fn is_closed_status(status: Option<&str>) -> bool {
    let Some(status) = status else {
        return false;
    };
    let normalized = status.trim().to_ascii_lowercase();
    normalized == "closed" || normalized == "ended" || normalized == "finished"
}

fn fetch_latest_finished_race_snapshot(client: &Client) -> Result<WecSnapshot, String> {
    let sessions = fetch_meta_sessions_for_series(client, WEC_SERIES_ID)
        .map_err(|err| format!("WEC meta sessions request failed: {err}"))?;
    let Some(session) = choose_latest_finished_race_session(&sessions) else {
        return Err("No finished FIA WEC race session with results found".to_string());
    };

    let results_url = format!(
        "https://insights.griiip.com/meta/sessions/{}/results",
        session.id
    );
    let results_response = client.get(&results_url).send().map_err(|err| {
        format!(
            "WEC results request failed for session {}: {err}",
            session.id
        )
    })?;
    if !results_response.status().is_success() {
        return Err(format!(
            "WEC results request failed for session {} with HTTP {}",
            session.id,
            results_response.status()
        ));
    }
    let results_body = results_response.text().map_err(|err| {
        format!(
            "WEC results body read failed for session {}: {err}",
            session.id
        )
    })?;
    let results_payload =
        serde_json::from_str::<SessionResultsResponse>(&results_body).map_err(|err| {
            format!(
                "WEC results decode failed for session {}: {err}",
                session.id
            )
        })?;

    let participants_url = format!(
        "https://insights.griiip.com/meta/sessions/{}/participants",
        session.id
    );
    let participants_response = client.get(&participants_url).send().map_err(|err| {
        format!(
            "WEC participants request failed for session {}: {err}",
            session.id
        )
    })?;
    if !participants_response.status().is_success() {
        return Err(format!(
            "WEC participants request failed for session {} with HTTP {}",
            session.id,
            participants_response.status()
        ));
    }
    let participants_body = participants_response.text().map_err(|err| {
        format!(
            "WEC participants body read failed for session {}: {err}",
            session.id
        )
    })?;
    let participants = serde_json::from_str::<Vec<SessionParticipantRow>>(&participants_body)
        .map_err(|err| {
            format!(
                "WEC participants decode failed for session {}: {err}",
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

        let class_name = participant
            .and_then(|item| item.class_id.as_deref())
            .map(format_wec_class_name)
            .unwrap_or_else(|| "-".to_string());

        let driver_name = participant
            .and_then(|item| item.drivers.first())
            .and_then(|driver| driver.display_name.as_deref())
            .map(normalize_driver_name)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "-".to_string());

        let team_name = participant
            .and_then(|item| item.team_name.clone())
            .or_else(|| participant.and_then(|item| item.display_name.clone()))
            .unwrap_or_else(|| "-".to_string());

        let vehicle = participant
            .and_then(|item| item.manufacturer.clone())
            .unwrap_or_else(|| "-".to_string());

        let position = row.overall_finished_at.unwrap_or((idx + 1) as u32);
        let class_rank = row
            .finished_at
            .map(|rank| rank.to_string())
            .unwrap_or_else(|| "-".to_string());

        let stable_id = if car_number != "-" {
            format!("wec:{car_number}")
        } else {
            format!("wec:participant:{}", row.session_participant_id)
        };

        entries.push(TimingEntry {
            position,
            car_number,
            class_name,
            class_rank,
            driver: driver_name,
            vehicle,
            team: team_name,
            laps: row
                .number_of_laps_completed
                .map(|laps| laps.to_string())
                .unwrap_or_else(|| "-".to_string()),
            gap_overall: format_gap(row.overall_gap_from_first, row.overall_gap_from_first_laps)
                .unwrap_or_else(|| "-".to_string()),
            gap_class: "-".to_string(),
            gap_next_in_class: format_gap(row.gap_from_first, row.gap_from_first_laps)
                .unwrap_or_else(|| "-".to_string()),
            last_lap: "-".to_string(),
            best_lap: row
                .best_lap_time
                .map(format_lap_time_ms)
                .unwrap_or_else(|| "-".to_string()),
            sector_1: row
                .best_sector_1_ms
                .map(format_sector_time_ms)
                .unwrap_or_else(|| "-".to_string()),
            sector_2: row
                .best_sector_2_ms
                .map(format_sector_time_ms)
                .unwrap_or_else(|| "-".to_string()),
            sector_3: row
                .best_sector_3_ms
                .map(format_sector_time_ms)
                .unwrap_or_else(|| "-".to_string()),
            sector_4: "-".to_string(),
            sector_5: "-".to_string(),
            best_lap_no: "-".to_string(),
            pit: "No".to_string(),
            pit_stops: "-".to_string(),
            fastest_driver: "-".to_string(),
            stable_id,
        });
    }

    entries.sort_by_key(|entry| entry.position);

    let mut header = TimingHeader {
        session_name: session.name.clone().unwrap_or_else(|| "Race".to_string()),
        session_type_raw: session
            .session_type
            .clone()
            .unwrap_or_else(|| "Race".to_string()),
        event_name: session
            .event
            .as_ref()
            .and_then(|event| event.name.clone())
            .unwrap_or_else(|| "FIA WEC".to_string()),
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
    header.class_colors.insert(
        "HYPER".to_string(),
        TimingClassColor {
            foreground: "#ffffff".to_string(),
            background: "#e21e19".to_string(),
        },
    );
    header.class_colors.insert(
        "LMGT3".to_string(),
        TimingClassColor {
            foreground: "#ffffff".to_string(),
            background: "#0b9314".to_string(),
        },
    );

    let session_id = derive_session_identifier(&header);
    let fingerprint = meaningful_snapshot_fingerprint(&header, &entries);

    Ok(WecSnapshot {
        header,
        entries,
        session_id,
        fingerprint,
    })
}

fn choose_latest_finished_race_session(sessions: &[MetaSessionItem]) -> Option<MetaSessionItem> {
    sessions
        .iter()
        .filter(|session| {
            session
                .session_type
                .as_deref()
                .map(|value| value.eq_ignore_ascii_case("Race"))
                .unwrap_or(false)
                && session.has_result
                && !session.is_running
        })
        .max_by_key(|session| {
            session
                .end_time
                .clone()
                .or_else(|| session.start_time.clone())
                .unwrap_or_default()
        })
        .cloned()
}

fn format_wec_class_name(raw: &str) -> String {
    let normalized = raw.trim().to_ascii_uppercase();
    match normalized.as_str() {
        "HYPERCAR" => "HYPER".to_string(),
        "LMGT3" => "LMGT3".to_string(),
        _ => raw.trim().to_string(),
    }
}

fn negotiate(client: &Client) -> Result<NegotiateResponse, String> {
    let response = client
        .post(NEGOTIATE_URL)
        .body("")
        .send()
        .map_err(|err| format!("WEC negotiate request failed: {err}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "WEC negotiate failed with HTTP {}",
            response.status()
        ));
    }
    let body = response
        .text()
        .map_err(|err| format!("WEC negotiate body read failed: {err}"))?;
    serde_json::from_str::<NegotiateResponse>(&body)
        .map_err(|err| format!("WEC negotiate decode failed: {err}"))
}

fn websocket_url_from_negotiate(base_url: &str, token: &str) -> String {
    let mut ws_url = if let Some(rest) = base_url.strip_prefix("https://") {
        format!("wss://{rest}")
    } else {
        base_url.to_string()
    };
    let separator = if ws_url.contains('?') { '&' } else { '?' };
    ws_url.push(separator);
    ws_url.push_str("access_token=");
    ws_url.push_str(token);
    ws_url
}

fn join_group(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    invocation_id: &mut u64,
    sid: u64,
    channel: &str,
) -> Result<(), String> {
    let group = format!("SID-{sid}-{channel}");
    let payload = serde_json::json!({
        "type": 1,
        "invocationId": invocation_id.to_string(),
        "target": "JoinGroup",
        "arguments": [group],
    });
    *invocation_id += 1;
    send_signalr_json(socket, &payload)
}

fn send_signalr_json(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    payload: &Value,
) -> Result<(), String> {
    let mut encoded = serde_json::to_string(payload)
        .map_err(|err| format!("WEC SignalR payload encode failed: {err}"))?;
    encoded.push(SIGNALR_RS);
    socket
        .send(Message::Text(encoded.into()))
        .map_err(|err| format!("WEC SignalR send failed: {err}"))
}

fn build_request(url: &str) -> Result<tungstenite::handshake::client::Request, String> {
    let mut request = tungstenite::client::IntoClientRequest::into_client_request(url)
        .map_err(|err| format!("failed to build websocket request: {err}"))?;
    request
        .headers_mut()
        .insert(ORIGIN, HeaderValue::from_static(ORIGIN_URL));
    request
        .headers_mut()
        .insert(USER_AGENT, HeaderValue::from_static("Mozilla/5.0"));
    Ok(request)
}

fn set_socket_timeout(socket: &mut WebSocket<MaybeTlsStream<TcpStream>>) {
    if let MaybeTlsStream::Plain(stream) = socket.get_mut() {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    }
}

fn read_signalr_text(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
) -> Result<Option<String>, String> {
    match socket.read() {
        Ok(Message::Text(text)) => Ok(Some(text.to_string())),
        Ok(Message::Binary(data)) => Ok(String::from_utf8(data.to_vec()).ok()),
        Ok(Message::Ping(data)) => {
            socket
                .send(Message::Pong(data))
                .map_err(|err| format!("WEC ping/pong handling failed: {err}"))?;
            Ok(None)
        }
        Ok(Message::Pong(_)) => Ok(None),
        Ok(Message::Close(_)) => Ok(Some(format!("{{\"type\":7}}{SIGNALR_RS}"))),
        Ok(Message::Frame(_)) => Ok(None),
        Err(WsError::Io(err))
            if err.kind() == std::io::ErrorKind::WouldBlock
                || err.kind() == std::io::ErrorKind::TimedOut =>
        {
            Ok(None)
        }
        Err(err) => Err(format!("WEC websocket read failed: {err}")),
    }
}

fn split_signalr_frames(raw: &str) -> Vec<&str> {
    raw.split(SIGNALR_RS)
        .map(str::trim)
        .filter(|frame| !frame.is_empty())
        .collect()
}

fn parse_signalr_frame(frame: &str) -> SignalRFrame {
    if frame == "{}" {
        return SignalRFrame::HandshakeAck;
    }
    let Ok(value) = serde_json::from_str::<Value>(frame) else {
        return SignalRFrame::Unknown;
    };
    let Some(message_type) = value.get("type").and_then(Value::as_u64) else {
        return SignalRFrame::Unknown;
    };

    match message_type {
        1 => {
            let target = value
                .get("target")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let arguments = value
                .get("arguments")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            SignalRFrame::Invocation { target, arguments }
        }
        3 => SignalRFrame::Completion {
            invocation_id: value
                .get("invocationId")
                .and_then(Value::as_str)
                .map(str::to_string),
            error: value
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_string),
        },
        6 => SignalRFrame::Ping,
        7 => SignalRFrame::Close,
        _ => SignalRFrame::Unknown,
    }
}

fn bootstrap_live_state(client: &Client, sid: u64, state: &mut WecLiveState) -> Result<(), String> {
    apply_session_info(state, &fetch_live_json(client, sid, "session-info")?);
    apply_session_clock(state, &fetch_live_json(client, sid, "session-clock")?);
    apply_race_flags(state, &fetch_live_json(client, sid, "race-flags")?);
    apply_participants(state, &fetch_live_json(client, sid, "participants")?);
    apply_ranks(state, &fetch_live_json(client, sid, "ranks")?);
    apply_gaps(state, &fetch_live_json(client, sid, "gaps")?);
    apply_laps(state, &fetch_live_json(client, sid, "laps")?);
    apply_sectors(state, &fetch_live_json(client, sid, "sectors")?);
    Ok(())
}

fn fetch_live_json(client: &Client, sid: u64, route: &str) -> Result<Value, String> {
    let url = format!("{LIVE_BASE_URL}/{route}/{sid}");
    let response = client
        .get(&url)
        .send()
        .map_err(|err| format!("WEC bootstrap request failed ({route}): {err}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "WEC bootstrap endpoint {route} failed with HTTP {}",
            response.status()
        ));
    }
    let body = response
        .text()
        .map_err(|err| format!("WEC bootstrap body read failed ({route}): {err}"))?;
    serde_json::from_str::<Value>(&body)
        .map_err(|err| format!("WEC bootstrap decode failed ({route}): {err}"))
}

fn apply_signalr_arguments(state: &mut WecLiveState, target: &str, arguments: &[Value]) -> bool {
    let mut changed = false;
    for argument in arguments {
        let was_changed = match target {
            "lv-session-info" => apply_session_info(state, argument),
            "lv-session-clock" => apply_session_clock(state, argument),
            "lv-race-flags" => apply_race_flags(state, argument),
            "lv-participants" => apply_participants(state, argument),
            "lv-ranks" => apply_ranks(state, argument),
            "lv-gaps" => apply_gaps(state, argument),
            "lv-laps" => apply_laps(state, argument),
            "lv-sectors" => apply_sectors(state, argument),
            _ => false,
        };
        changed |= was_changed;
    }
    changed
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

fn apply_session_info(state: &mut WecLiveState, payload: &Value) -> bool {
    let Some(map) = payload.as_object() else {
        return false;
    };
    let mut changed = false;
    changed |= set_header_text(&mut state.header.event_name, map_str(map, "eventName"));
    changed |= set_header_text(&mut state.header.session_name, map_str(map, "sessionName"));
    changed |= set_header_text(&mut state.header.track_name, map_str(map, "trackName"));
    changed |= set_header_text(
        &mut state.header.session_type_raw,
        map_text(map, "sessionType"),
    );
    changed |= set_header_text(
        &mut state.header.flag,
        map_str(map, "connectionStatus").map(|flag| normalize_flag(&flag)),
    );

    if let Some(classes) = map.get("sessionClasses").and_then(Value::as_array) {
        let mut class_names = HashMap::new();
        let mut class_colors = BTreeMap::new();
        for class_row in classes {
            let Some(class_map) = class_row.as_object() else {
                continue;
            };
            let Some(class_id) = map_str(class_map, "classId") else {
                continue;
            };
            let class_label = map_str(class_map, "classThreeLettersName")
                .or_else(|| map_str(class_map, "className"))
                .unwrap_or_else(|| class_id.clone());
            class_names.insert(class_id.clone(), class_label.clone());
            if let Some(color_hex) = map_str(class_map, "classColor") {
                class_colors.insert(
                    class_label,
                    TimingClassColor {
                        foreground: "#ffffff".to_string(),
                        background: color_hex,
                    },
                );
            }
        }
        if !class_names.is_empty() {
            state.class_names = class_names;
            changed = true;
        }
        if !class_colors.is_empty() {
            state.class_colors = class_colors;
            state.header.class_colors = state.class_colors.clone();
            changed = true;
        }
    }
    changed
}

fn apply_session_clock(state: &mut WecLiveState, payload: &Value) -> bool {
    let Some(map) = payload.as_object() else {
        return false;
    };
    let mut changed = false;
    changed |= set_header_text(
        &mut state.header.day_time,
        map_str(map, "tsNow").map(|raw| compact_iso_timestamp(&raw)),
    );
    if let Some(ms) = map_i64(map, "elapsedTimeMillisNow") {
        if ms >= 0 {
            changed |= set_header_text(
                &mut state.header.time_to_go,
                Some(format_clock_ms(ms as u64)),
            );
        }
    }
    changed
}

fn apply_race_flags(state: &mut WecLiveState, payload: &Value) -> bool {
    let rows = payload_rows(payload);
    let latest = rows
        .into_iter()
        .filter_map(|row| row.as_object())
        .max_by_key(|row| map_str(row, "ts").unwrap_or_default());
    let Some(latest) = latest else {
        return false;
    };
    set_header_text(
        &mut state.header.flag,
        map_str(latest, "flag").map(|flag| normalize_flag(&flag)),
    )
}

fn apply_participants(state: &mut WecLiveState, payload: &Value) -> bool {
    let mut changed = false;
    for row in payload_rows(payload) {
        let Some(map) = row.as_object() else {
            continue;
        };
        let Some((key, entry)) = upsert_car_state(state, map) else {
            continue;
        };
        changed |= set_row_text(&mut entry.car_number, map_str(map, "carNumber"));
        changed |= set_opt_string(
            &mut entry.team,
            map_str(map, "teamName").or_else(|| map_str(map, "displayName")),
        );
        changed |= set_opt_string(&mut entry.vehicle, map_str(map, "manufacturer"));
        changed |= set_opt_string(
            &mut entry.driver,
            current_driver_name(map).map(|name| normalize_driver_name(&name)),
        );
        changed |= set_opt_string(&mut entry.class_id, map_str(map, "classId"));
        refresh_class_name(state, &key);
    }
    changed
}

fn apply_ranks(state: &mut WecLiveState, payload: &Value) -> bool {
    let mut changed = false;
    for row in payload_rows(payload) {
        let Some(map) = row.as_object() else {
            continue;
        };
        let Some((key, entry)) = upsert_car_state(state, map) else {
            continue;
        };
        changed |= set_opt_u32(&mut entry.position, map_u32(map, "overallPosition"));
        changed |= set_opt_u32(&mut entry.class_rank, map_u32(map, "position"));
        changed |= set_opt_u32(&mut entry.laps, map_u32(map, "lapNumber"));
        changed |= set_opt_string(&mut entry.class_id, map_str(map, "classId"));
        refresh_class_name(state, &key);
    }
    changed
}

fn apply_sectors(state: &mut WecLiveState, payload: &Value) -> bool {
    let mut changed = false;
    for row in payload_rows(payload) {
        let Some(map) = row.as_object() else {
            continue;
        };
        let Some((_key, entry)) = upsert_car_state(state, map) else {
            continue;
        };
        let Some(sector_number) = map_u32(map, "sectorNumber") else {
            continue;
        };
        if !(1..=3).contains(&sector_number) {
            continue;
        }
        let Some(sector_ms) = map_i64(map, "sectorTimeMillis") else {
            continue;
        };
        if sector_ms <= 0 {
            continue;
        }
        let idx = (sector_number - 1) as usize;
        let incoming_lap = map_u32(map, "lapNumber").unwrap_or(0);
        let previous_lap = entry.sector_laps[idx].unwrap_or(0);
        let should_update = incoming_lap >= previous_lap || entry.sector_times[idx].is_none();
        if should_update {
            if entry.sector_times[idx] != Some(sector_ms) {
                entry.sector_times[idx] = Some(sector_ms);
                changed = true;
            }
            if entry.sector_laps[idx] != Some(incoming_lap) {
                entry.sector_laps[idx] = Some(incoming_lap);
                changed = true;
            }
        }
    }
    changed
}

fn apply_gaps(state: &mut WecLiveState, payload: &Value) -> bool {
    let mut changed = false;
    for row in payload_rows(payload) {
        let Some(map) = row.as_object() else {
            continue;
        };
        let Some((_key, entry)) = upsert_car_state(state, map) else {
            continue;
        };
        changed |= set_opt_string(
            &mut entry.gap_overall,
            format_gap(
                map_i64(map, "gapToFirstMillis"),
                map_i64(map, "gapToFirstLaps"),
            ),
        );
        changed |= set_opt_string(
            &mut entry.gap_next_in_class,
            format_gap(
                map_i64(map, "gapToAheadMillis"),
                map_i64(map, "gapToAheadLaps"),
            ),
        );
        changed |= set_opt_u32(&mut entry.laps, map_u32(map, "lapNumber"));
    }
    changed
}

fn apply_laps(state: &mut WecLiveState, payload: &Value) -> bool {
    let mut changed = false;
    for row in payload_rows(payload) {
        let Some(map) = row.as_object() else {
            continue;
        };
        let Some((_key, entry)) = upsert_car_state(state, map) else {
            continue;
        };

        let lap_no = map_u32(map, "lapNumber");
        if let Some(lap_ms) = map_i64(map, "lapTimeMillis") {
            if lap_ms > 0 {
                if lap_no.unwrap_or(0) >= entry.best_lap_no.unwrap_or(0)
                    || entry.last_lap_ms.is_none()
                {
                    changed |= set_opt_i64(&mut entry.last_lap_ms, Some(lap_ms));
                }
                if entry.best_lap_ms.is_none() || Some(lap_ms) < entry.best_lap_ms {
                    changed |= set_opt_i64(&mut entry.best_lap_ms, Some(lap_ms));
                    changed |= set_opt_u32(&mut entry.best_lap_no, lap_no);
                }
            }
        }
        changed |= set_opt_u32(&mut entry.laps, lap_no);
        changed |= set_opt_string(&mut entry.class_id, map_str(map, "classId"));
        if let Some(in_pit) =
            map_bool(map, "isEndedInPit").or_else(|| map_bool(map, "isStartedInPit"))
        {
            changed |= set_opt_bool(&mut entry.pit, Some(in_pit));
        }
    }
    changed
}

fn snapshot_from_live_state(state: &WecLiveState) -> Option<(TimingHeader, Vec<TimingEntry>)> {
    let mut entries: Vec<TimingEntry> = state
        .rows
        .values()
        .filter(|row| !row.car_number.trim().is_empty())
        .cloned()
        .map(|row| row_to_timing_entry(state, row))
        .collect();
    entries.sort_by_key(|entry| (entry.position, entry.car_number.clone()));
    for (idx, entry) in entries.iter_mut().enumerate() {
        if entry.position == 0 {
            entry.position = (idx + 1) as u32;
        }
    }
    if entries.is_empty() {
        return None;
    }

    let mut header = state.header.clone();
    if header.event_name.trim().is_empty() {
        header.event_name = "WEC Live Timing".to_string();
    }
    if header.session_name.trim().is_empty() {
        header.session_name = "-".to_string();
    }
    if header.track_name.trim().is_empty() {
        header.track_name = "-".to_string();
    }
    if header.day_time.trim().is_empty() {
        header.day_time = "-".to_string();
    }
    if header.time_to_go.trim().is_empty() {
        header.time_to_go = "-".to_string();
    }
    if header.flag.trim().is_empty() {
        header.flag = "-".to_string();
    }
    header.class_colors = state.class_colors.clone();
    Some((header, entries))
}

fn row_to_timing_entry(state: &WecLiveState, row: WecCarState) -> TimingEntry {
    let class_name = row
        .class_name
        .clone()
        .or_else(|| {
            row.class_id
                .as_ref()
                .and_then(|id| state.class_names.get(id).cloned())
        })
        .unwrap_or_else(|| "-".to_string());
    let stable_id = format!("wec:{}", row.car_number);
    TimingEntry {
        position: row.position.unwrap_or(0),
        car_number: row.car_number,
        class_name,
        class_rank: row
            .class_rank
            .map(|rank| rank.to_string())
            .unwrap_or_else(|| "-".to_string()),
        driver: row.driver.unwrap_or_else(|| "-".to_string()),
        vehicle: row.vehicle.unwrap_or_else(|| "-".to_string()),
        team: row.team.unwrap_or_else(|| "-".to_string()),
        laps: row
            .laps
            .map(|lap| lap.to_string())
            .unwrap_or_else(|| "-".to_string()),
        gap_overall: row.gap_overall.unwrap_or_else(|| "-".to_string()),
        gap_class: "-".to_string(),
        gap_next_in_class: row.gap_next_in_class.unwrap_or_else(|| "-".to_string()),
        last_lap: row
            .last_lap_ms
            .map(format_lap_time_ms)
            .unwrap_or_else(|| "-".to_string()),
        best_lap: row
            .best_lap_ms
            .map(format_lap_time_ms)
            .unwrap_or_else(|| "-".to_string()),
        sector_1: row
            .sector_times
            .first()
            .copied()
            .flatten()
            .map(format_sector_time_ms)
            .unwrap_or_else(|| "-".to_string()),
        sector_2: row
            .sector_times
            .get(1)
            .copied()
            .flatten()
            .map(format_sector_time_ms)
            .unwrap_or_else(|| "-".to_string()),
        sector_3: row
            .sector_times
            .get(2)
            .copied()
            .flatten()
            .map(format_sector_time_ms)
            .unwrap_or_else(|| "-".to_string()),
        sector_4: "-".to_string(),
        sector_5: "-".to_string(),
        best_lap_no: row
            .best_lap_no
            .map(|lap| lap.to_string())
            .unwrap_or_else(|| "-".to_string()),
        pit: if row.pit.unwrap_or(false) {
            "Yes".to_string()
        } else {
            "No".to_string()
        },
        pit_stops: "-".to_string(),
        fastest_driver: "-".to_string(),
        stable_id,
    }
}

fn upsert_car_state<'a>(
    state: &'a mut WecLiveState,
    row: &Map<String, Value>,
) -> Option<(String, &'a mut WecCarState)> {
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

fn refresh_class_name(state: &mut WecLiveState, key: &str) {
    if let Some(row) = state.rows.get_mut(key) {
        if row.class_name.is_none() {
            if let Some(class_id) = row.class_id.as_ref() {
                if let Some(class_name) = state.class_names.get(class_id) {
                    row.class_name = Some(class_name.clone());
                }
            }
        }
    }
}

fn current_driver_name(row: &Map<String, Value>) -> Option<String> {
    if let Some(drivers) = row.get("drivers").and_then(Value::as_array) {
        let current_driver_id = map_str(row, "currentDriverId");
        if let Some(current_driver_id) = current_driver_id {
            for driver in drivers {
                let Some(driver_map) = driver.as_object() else {
                    continue;
                };
                if map_text(driver_map, "driverId").as_deref() == Some(current_driver_id.as_str()) {
                    if let Some(name) = map_str(driver_map, "displayName") {
                        return Some(name);
                    }
                }
            }
        }
        for driver in drivers {
            let Some(driver_map) = driver.as_object() else {
                continue;
            };
            if let Some(name) = map_str(driver_map, "displayName") {
                return Some(name);
            }
        }
    }

    if let (Some(first), Some(last)) = (map_str(row, "firstname"), map_str(row, "lastname")) {
        return Some(format!("{first} {last}"));
    }
    map_str(row, "displayName")
}

fn format_gap(gap_ms: Option<i64>, gap_laps: Option<i64>) -> Option<String> {
    if let Some(laps) = gap_laps {
        if laps > 0 {
            return Some(format!("+{laps} L"));
        }
    }
    let millis = gap_ms?;
    if millis <= 0 {
        return Some("-".to_string());
    }
    let secs = millis / 1000;
    let rem = millis % 1000;
    Some(format!("+{secs}.{rem:03}"))
}

fn format_lap_time_ms(ms: i64) -> String {
    if ms <= 0 {
        return "-".to_string();
    }
    let total_ms = ms as u64;
    let minutes = total_ms / 60_000;
    let seconds = (total_ms % 60_000) / 1000;
    let millis = total_ms % 1000;
    format!("{minutes}:{seconds:02}.{millis:03}")
}

fn format_sector_time_ms(ms: i64) -> String {
    if ms <= 0 {
        return "-".to_string();
    }
    let total_ms = ms as u64;
    if total_ms >= 60_000 {
        let minutes = total_ms / 60_000;
        let seconds = (total_ms % 60_000) / 1000;
        let millis = total_ms % 1000;
        return format!("{minutes}:{seconds:02}.{millis:03}");
    }
    let seconds = total_ms / 1000;
    let millis = total_ms % 1000;
    format!("{seconds}.{millis:03}")
}

fn normalize_driver_name(raw: &str) -> String {
    raw.split_whitespace()
        .map(normalize_driver_name_token)
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_driver_name_token(token: &str) -> String {
    if token.chars().all(|ch| !ch.is_alphabetic()) {
        return token.to_string();
    }
    let letters: String = token.chars().filter(|ch| ch.is_alphabetic()).collect();
    let needs_normalization = !letters.is_empty()
        && (letters.chars().all(|ch| ch.is_uppercase())
            || letters.chars().all(|ch| ch.is_lowercase()));
    if !needs_normalization {
        return token.to_string();
    }

    let mut out = String::with_capacity(token.len());
    let mut seen_alpha = false;
    for ch in token.chars() {
        if ch.is_alphabetic() {
            if !seen_alpha {
                out.extend(ch.to_uppercase());
                seen_alpha = true;
            } else {
                out.extend(ch.to_lowercase());
            }
        } else {
            seen_alpha = false;
            out.push(ch);
        }
    }
    out
}

fn format_clock_ms(ms: u64) -> String {
    let total_seconds = ms / 1000;
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

fn compact_iso_timestamp(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some((_, rest)) = trimmed.split_once('T') {
        return rest
            .split(['+', 'Z'])
            .next()
            .unwrap_or(rest)
            .trim()
            .to_string();
    }
    trimmed.to_string()
}

fn map_str(map: &Map<String, Value>, key: &str) -> Option<String> {
    let raw = map.get(key)?.as_str()?.trim();
    if raw.is_empty() {
        None
    } else {
        Some(raw.to_string())
    }
}

fn map_i64(map: &Map<String, Value>, key: &str) -> Option<i64> {
    map.get(key).and_then(|value| {
        value
            .as_i64()
            .or_else(|| value.as_u64().and_then(|number| i64::try_from(number).ok()))
            .or_else(|| value.as_str()?.trim().parse::<i64>().ok())
    })
}

fn map_u32(map: &Map<String, Value>, key: &str) -> Option<u32> {
    map_i64(map, key).and_then(|number| u32::try_from(number).ok())
}

fn map_bool(map: &Map<String, Value>, key: &str) -> Option<bool> {
    map.get(key).and_then(|value| {
        value.as_bool().or_else(|| {
            value
                .as_str()
                .and_then(|raw| match raw.trim().to_ascii_lowercase().as_str() {
                    "true" | "1" | "yes" => Some(true),
                    "false" | "0" | "no" => Some(false),
                    _ => None,
                })
        })
    })
}

fn map_text(map: &Map<String, Value>, key: &str) -> Option<String> {
    let value = map.get(key)?;
    match value {
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(value) => Some(if *value { "true" } else { "false" }.to_string()),
        _ => None,
    }
}

fn set_header_text(slot: &mut String, incoming: Option<String>) -> bool {
    let Some(incoming) = incoming else {
        return false;
    };
    if incoming.trim().is_empty() || *slot == incoming {
        return false;
    }
    *slot = incoming;
    true
}

fn set_row_text(slot: &mut String, incoming: Option<String>) -> bool {
    let Some(incoming) = incoming else {
        return false;
    };
    if incoming.trim().is_empty() || *slot == incoming {
        return false;
    }
    *slot = incoming;
    true
}

fn set_opt_string(slot: &mut Option<String>, incoming: Option<String>) -> bool {
    let Some(incoming) = incoming else {
        return false;
    };
    if incoming.trim().is_empty() || slot.as_ref() == Some(&incoming) {
        return false;
    }
    *slot = Some(incoming);
    true
}

fn set_opt_u32(slot: &mut Option<u32>, incoming: Option<u32>) -> bool {
    if incoming.is_none() || *slot == incoming {
        return false;
    }
    *slot = incoming;
    true
}

fn set_opt_i64(slot: &mut Option<i64>, incoming: Option<i64>) -> bool {
    if incoming.is_none() || *slot == incoming {
        return false;
    }
    *slot = incoming;
    true
}

fn set_opt_bool(slot: &mut Option<bool>, incoming: Option<bool>) -> bool {
    if incoming.is_none() || *slot == incoming {
        return false;
    }
    *slot = incoming;
    true
}

fn normalize_flag(raw: &str) -> String {
    let normalized = raw.trim().to_ascii_lowercase();
    if normalized.contains("check") || normalized.contains("finish") {
        "Checkered".to_string()
    } else if normalized.contains("red") {
        "Red".to_string()
    } else if normalized.contains("yellow") {
        "Yellow".to_string()
    } else if normalized.contains("code 60") || normalized == "60" {
        "Code 60".to_string()
    } else if normalized.contains("green") {
        "Green".to_string()
    } else if raw.trim().is_empty() {
        "-".to_string()
    } else {
        raw.trim().to_string()
    }
}

fn emit_snapshot(
    emitter: (&Sender<TimingMessage>, u64),
    header: TimingHeader,
    entries: Vec<TimingEntry>,
    persist: &mut PersistState,
    last_snapshot: &mut Option<WecSnapshot>,
    last_session_id: &mut Option<String>,
    debug_output: &SeriesDebugOutput,
) {
    let (tx, source_id) = emitter;
    let session_id = derive_session_identifier(&header);
    let snapshot = WecSnapshot {
        header: header.clone(),
        entries: entries.clone(),
        session_id: session_id.clone(),
        fingerprint: meaningful_snapshot_fingerprint(&header, &entries),
    };
    let first_real_of_session = !snapshot.entries.is_empty() && session_id != *last_session_id;
    let session_complete = snapshot.header.flag.eq_ignore_ascii_case("checkered");
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
        persist_snapshot(persist, &snapshot, debug_output);
    }

    *last_session_id = session_id;
    *last_snapshot = Some(snapshot);

    let _ = tx.send(TimingMessage::Snapshot {
        source_id,
        header,
        entries,
    });
    let _ = tx.send(TimingMessage::Status {
        source_id,
        text: "WEC live timing connected".to_string(),
    });
}

fn meaningful_snapshot_fingerprint(header: &TimingHeader, entries: &[TimingEntry]) -> u64 {
    let mut hasher = base_snapshot_fingerprint(header);
    for entry in entries {
        hash_entry_common_fields(&mut hasher, entry);
    }
    hasher.finish()
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_millis() as u64
}

fn wec_snapshot_path() -> Option<PathBuf> {
    data_local_snapshot_path("wec_snapshot.json")
}

fn persist_snapshot(runtime: &mut PersistState, snapshot: &WecSnapshot, debug: &SeriesDebugOutput) {
    let Some(path) = runtime.path.as_ref() else {
        return;
    };
    let payload = PersistedWecSnapshot {
        saved_unix_ms: now_unix_ms(),
        session_id: snapshot.session_id.clone(),
        meaningful_fingerprint: snapshot.fingerprint,
        header: snapshot.header.clone(),
        entries: snapshot.entries.clone(),
    };
    if let Err(err) = write_json_pretty(path, &payload) {
        log_series_debug(debug, "WEC", format!("snapshot persist failed: {err}"));
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
) -> Option<WecSnapshot> {
    let path = runtime.path.as_ref()?;
    let saved = read_json::<PersistedWecSnapshot>(path)?;
    runtime.last_persisted_hash = Some(saved.meaningful_fingerprint);
    runtime.last_save_at = Some(SystemTime::now());
    let snapshot = WecSnapshot {
        header: saved.header,
        entries: saved.entries,
        session_id: saved.session_id,
        fingerprint: saved.meaningful_fingerprint,
    };
    let _ = tx.send(TimingMessage::Snapshot {
        source_id,
        header: snapshot.header.clone(),
        entries: snapshot.entries.clone(),
    });
    log_series_debug(
        debug,
        "WEC",
        format!("snapshot restored from {}", path.display()),
    );
    Some(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_signalr_frames_handles_record_separator() {
        let raw = "{}\u{1e}{\"type\":6}\u{1e}";
        let frames = split_signalr_frames(raw);
        assert_eq!(frames, vec!["{}", "{\"type\":6}"]);
    }

    #[test]
    fn parse_signalr_frame_recognizes_invocation() {
        let frame =
            "{\"type\":1,\"target\":\"x\",\"arguments\":[{\"cars\":[{\"carNumber\":\"50\"}]}]}";
        match parse_signalr_frame(frame) {
            SignalRFrame::Invocation { target, arguments } => {
                assert_eq!(target, "x");
                assert_eq!(arguments.len(), 1);
            }
            _ => panic!("expected invocation"),
        }
    }

    #[test]
    fn choose_candidate_sids_prefers_started_session() {
        let sessions = vec![
            SessionScheduleItem {
                sid: 10,
                is_started: false,
                connection_status: Some("Green".to_string()),
            },
            SessionScheduleItem {
                sid: 11,
                is_started: true,
                connection_status: Some("Green".to_string()),
            },
        ];
        assert_eq!(choose_candidate_sids(&sessions).unwrap(), vec![11, 10]);
    }

    #[test]
    fn choose_candidate_sids_falls_back_to_first() {
        let sessions = vec![SessionScheduleItem {
            sid: 20,
            is_started: false,
            connection_status: None,
        }];
        assert_eq!(choose_candidate_sids(&sessions).unwrap(), vec![20]);
    }

    #[test]
    fn choose_candidate_sids_rejects_empty_schedule() {
        let sessions = Vec::<SessionScheduleItem>::new();
        assert!(choose_candidate_sids(&sessions).is_err());
    }

    #[test]
    fn apply_signalr_arguments_builds_clean_snapshot() {
        let mut state = WecLiveState::default();

        apply_signalr_arguments(
            &mut state,
            "lv-session-info",
            &[serde_json::json!({
                "eventName": "WEC",
                "sessionName": "Race",
                "trackName": "Imola",
                "connectionStatus": "Green",
                "sessionClasses": [
                    {"classId":"HYPERCAR","classThreeLettersName":"HYPER","classColor":"#ff0000"}
                ]
            })],
        );
        apply_signalr_arguments(
            &mut state,
            "lv-participants",
            &[serde_json::json!({
                "items": [
                    {
                        "pid": 1,
                        "carNumber": "50",
                        "classId": "HYPERCAR",
                        "teamName": "Ferrari AF Corse",
                        "manufacturer": "Ferrari 499P",
                        "drivers": [{"displayName":"ALESSANDRO PIER GUIDI"}]
                    }
                ]
            })],
        );
        apply_signalr_arguments(
            &mut state,
            "lv-ranks",
            &[serde_json::json!({
                "items": [
                    {
                        "pid": 1,
                        "carNumber": "50",
                        "overallPosition": 1,
                        "position": 1,
                        "lapNumber": 160,
                        "sectorNumber": 2,
                        "classId": "HYPERCAR"
                    }
                ]
            })],
        );
        apply_signalr_arguments(
            &mut state,
            "lv-gaps",
            &[serde_json::json!({
                "items": [
                    {
                        "pid": 1,
                        "carNumber": "50",
                        "gapToFirstMillis": -1,
                        "gapToAheadMillis": 0,
                        "gapToAheadLaps": 0,
                        "lapNumber": 160
                    }
                ]
            })],
        );
        apply_signalr_arguments(
            &mut state,
            "lv-laps",
            &[serde_json::json!({
                "pid": 1,
                "carNumber": "50",
                "lapNumber": 160,
                "lapTimeMillis": 95321,
                "isEndedInPit": false
            })],
        );
        apply_signalr_arguments(
            &mut state,
            "lv-sectors",
            &[serde_json::json!({
                "pid": 1,
                "carNumber": "50",
                "sectorNumber": 1,
                "lapNumber": 160,
                "sectorTimeMillis": 19512
            })],
        );
        apply_signalr_arguments(
            &mut state,
            "lv-sectors",
            &[serde_json::json!({
                "pid": 1,
                "carNumber": "50",
                "sectorNumber": 2,
                "lapNumber": 160,
                "sectorTimeMillis": 31856
            })],
        );
        apply_signalr_arguments(
            &mut state,
            "lv-sectors",
            &[serde_json::json!({
                "pid": 1,
                "carNumber": "50",
                "sectorNumber": 3,
                "lapNumber": 160,
                "sectorTimeMillis": 44169
            })],
        );

        let (header, entries) = snapshot_from_live_state(&state).expect("snapshot");
        assert_eq!(header.session_name, "Race");
        assert_eq!(header.event_name, "WEC");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].car_number, "50");
        assert_eq!(entries[0].team, "Ferrari AF Corse");
        assert_eq!(entries[0].driver, "Alessandro Pier Guidi");
        assert_eq!(entries[0].class_name, "HYPER");
        assert_eq!(entries[0].best_lap, "1:35.321");
        assert_eq!(entries[0].gap_overall, "-");
        assert_eq!(entries[0].sector_1, "19.512");
        assert_eq!(entries[0].sector_2, "31.856");
        assert_eq!(entries[0].sector_3, "44.169");
    }

    #[test]
    fn apply_signalr_arguments_merges_deltas_without_dropping_rows() {
        let mut state = WecLiveState::default();
        apply_signalr_arguments(
            &mut state,
            "lv-ranks",
            &[serde_json::json!({
                "items": [
                    {"pid": 1, "carNumber":"50", "overallPosition":1, "position":1, "lapNumber":10},
                    {"pid": 2, "carNumber":"6", "overallPosition":2, "position":2, "lapNumber":10}
                ]
            })],
        );
        apply_signalr_arguments(
            &mut state,
            "lv-gaps",
            &[serde_json::json!({
                "items": [
                    {"pid": 2, "carNumber":"6", "gapToFirstMillis":1200, "gapToAheadMillis":1200, "gapToAheadLaps":0}
                ]
            })],
        );

        let (_header, entries) = snapshot_from_live_state(&state).expect("snapshot");
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().any(|entry| entry.car_number == "50"));
        assert!(entries.iter().any(|entry| entry.car_number == "6"));
    }

    #[test]
    fn normalize_driver_name_handles_mixed_case_tokens() {
        assert_eq!(
            normalize_driver_name("António FÉLIX DA COSTA"),
            "António Félix Da Costa"
        );
        assert_eq!(normalize_driver_name("Kevin MAGNUSSEN"), "Kevin Magnussen");
        assert_eq!(normalize_driver_name("Mike Conway"), "Mike Conway");
    }

    #[test]
    fn choose_latest_finished_race_session_picks_newest_finished_race() {
        let sessions = vec![
            MetaSessionItem {
                id: 1,
                name: Some("Practice".to_string()),
                session_type: Some("Practice".to_string()),
                is_running: false,
                has_result: true,
                start_time: Some("2026-04-18T08:00:00+00:00".to_string()),
                end_time: Some("2026-04-18T09:00:00+00:00".to_string()),
                event: None,
                track_config: None,
            },
            MetaSessionItem {
                id: 2,
                name: Some("Race Old".to_string()),
                session_type: Some("Race".to_string()),
                is_running: false,
                has_result: true,
                start_time: Some("2026-04-18T10:00:00+00:00".to_string()),
                end_time: Some("2026-04-18T12:00:00+00:00".to_string()),
                event: None,
                track_config: None,
            },
            MetaSessionItem {
                id: 3,
                name: Some("Race New".to_string()),
                session_type: Some("Race".to_string()),
                is_running: false,
                has_result: true,
                start_time: Some("2026-04-19T10:00:00+00:00".to_string()),
                end_time: Some("2026-04-19T12:00:00+00:00".to_string()),
                event: None,
                track_config: None,
            },
            MetaSessionItem {
                id: 4,
                name: Some("Race Running".to_string()),
                session_type: Some("Race".to_string()),
                is_running: true,
                has_result: true,
                start_time: Some("2026-04-20T10:00:00+00:00".to_string()),
                end_time: Some("2026-04-20T12:00:00+00:00".to_string()),
                event: None,
                track_config: None,
            },
        ];

        let picked = choose_latest_finished_race_session(&sessions).expect("expected race session");
        assert_eq!(picked.id, 3);
    }
}
