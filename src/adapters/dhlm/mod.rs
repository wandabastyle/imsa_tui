pub mod snapshot;

use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::PathBuf,
    sync::mpsc::{Receiver, Sender},
    time::Duration,
};

use serde_json::Value;
use tungstenite::{
    client::IntoClientRequest,
    connect,
    http::header::{HeaderValue, ORIGIN, USER_AGENT},
    stream::MaybeTlsStream,
    Message,
};

use crate::{
    adapters::nls::protocol::entry_from_value,
    timing::{TimingEntry, TimingHeader, TimingMessage},
    timing_persist::{data_local_snapshot_path, log_series_debug, PersistState, SeriesDebugOutput},
};

use self::snapshot::{
    derive_session_id, meaningful_snapshot_fingerprint, persist_snapshot,
    persist_snapshot_if_dirty, restore_snapshot_from_disk, DhlmSnapshot,
};

const WS_URL: &str = "wss://livetiming.azurewebsites.net/";
const DEFAULT_DHLM_EVENT_ID: &str = "50";

fn build_request() -> tungstenite::handshake::client::Request {
    let mut request = WS_URL
        .into_client_request()
        .expect("failed to create websocket request");

    request.headers_mut().insert(
        ORIGIN,
        HeaderValue::from_static("https://livetiming.azurewebsites.net"),
    );
    request
        .headers_mut()
        .insert(USER_AGENT, HeaderValue::from_static("Mozilla/5.0"));

    request
}

fn set_socket_timeout(socket: &mut tungstenite::WebSocket<MaybeTlsStream<std::net::TcpStream>>) {
    if let MaybeTlsStream::Plain(stream) = socket.get_mut() {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    }
}

fn dhlm_dump_path() -> Option<PathBuf> {
    data_local_snapshot_path("dhlm_dump.json")
}

fn extract_cup_from_message(text: &str) -> Option<String> {
    let parsed: Value = serde_json::from_str(text).ok()?;
    let cup = parsed.get("CUP")?.as_str()?.to_string();
    Some(cup)
}

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis()
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
    let mut header = TimingHeader {
        event_name: "DHLM Live Timing".to_string(),
        track_name: "Nürburgring".to_string(),
        ..TimingHeader::default()
    };
    let mut latest_entries: Vec<TimingEntry> = Vec::new();

    let mut persist = PersistState::new(dhlm_dump_path());
    let mut last_good_snapshot: Option<DhlmSnapshot> = None;
    let mut last_session_id: Option<String> = restore_snapshot_from_disk(
        &mut persist,
        &mut header,
        &mut latest_entries,
        &tx,
        source_id,
        &debug_output,
    );

    log_series_debug(&debug_output, "DHLM", "initializing");

    'outer: loop {
        if stop_rx.try_recv().is_ok() {
            if let Some(snapshot) = last_good_snapshot.as_ref() {
                persist_snapshot_if_dirty(
                    &mut persist,
                    snapshot,
                    now_millis() as u64,
                    &debug_output,
                );
            }
            break;
        }

        let _ = tx.send(TimingMessage::Status {
            source_id,
            text: "Connecting to DHLM websocket...".to_string(),
        });

        let request = build_request();
        let connection = connect(request);

        let (mut socket, response) = match connection {
            Ok(ok) => ok,
            Err(err) => {
                let _ = tx.send(TimingMessage::Error {
                    source_id,
                    text: format!("connect failed: {err}"),
                });
                if stop_rx.recv_timeout(Duration::from_secs(3)).is_ok() {
                    break;
                }
                continue;
            }
        };

        set_socket_timeout(&mut socket);

        let _ = tx.send(TimingMessage::Status {
            source_id,
            text: format!("DHLM connected ({})", response.status()),
        });

        let subscribe = serde_json::json!({
            "clientLocalTime": now_millis(),
            "eventId": DEFAULT_DHLM_EVENT_ID,
            "eventPid": [0, 4]
        });

        if let Err(err) = socket.send(Message::Text(subscribe.to_string())) {
            let _ = tx.send(TimingMessage::Error {
                source_id,
                text: format!("subscribe failed: {err}"),
            });
            if stop_rx.recv_timeout(Duration::from_secs(3)).is_ok() {
                break;
            }
            continue;
        }

        let mut use_dump_mode = false;

        match socket.read() {
            Ok(Message::Text(text)) => {
                if let Some(cup) = extract_cup_from_message(&text) {
                    let cup_is_dhlm = cup.to_ascii_lowercase().contains("dhlm");
                    if !cup_is_dhlm {
                        use_dump_mode = true;
                        let _ = tx.send(TimingMessage::Status {
                            source_id,
                            text: "[SNAPSHOT] Using dump file (CUP != DHLM)".to_string(),
                        });
                    }
                }
            }
            Ok(_) => {}
            Err(_) => {
                if stop_rx.recv_timeout(Duration::from_secs(3)).is_ok() {
                    break 'outer;
                }
                continue;
            }
        }

        if use_dump_mode {
            if let Some(lines) = load_dump_file(&dhlm_dump_path()) {
                for line in lines {
                    if parse_timing_message(&line, &mut header, &mut latest_entries) {
                        let session_id = derive_session_id(&header);
                        let snapshot = DhlmSnapshot {
                            header: header.clone(),
                            entries: latest_entries.clone(),
                            session_id: session_id.clone(),
                            fingerprint: meaningful_snapshot_fingerprint(&header, &latest_entries),
                        };

                        let should_persist = last_good_snapshot
                            .as_ref()
                            .map(|prev| prev.fingerprint != snapshot.fingerprint)
                            .unwrap_or(true);

                        if should_persist {
                            persist_snapshot(
                                &mut persist,
                                &snapshot,
                                now_millis() as u64,
                                &debug_output,
                            );
                        }
                        last_good_snapshot = Some(snapshot);
                        let _ = tx.send(TimingMessage::Snapshot {
                            source_id,
                            header: header.clone(),
                            entries: latest_entries.clone(),
                        });
                        if session_id.as_ref() != last_session_id.as_ref() {
                            last_session_id = session_id;
                        }
                    }
                }
            }
            break 'outer;
        }

        loop {
            if stop_rx.try_recv().is_ok() {
                if let Some(snapshot) = last_good_snapshot.as_ref() {
                    persist_snapshot_if_dirty(
                        &mut persist,
                        snapshot,
                        now_millis() as u64,
                        &debug_output,
                    );
                }
                break 'outer;
            }

            match socket.read() {
                Ok(Message::Text(text)) => {
                    if !parse_timing_message(&text, &mut header, &mut latest_entries) {
                        continue;
                    }
                    let session_id = derive_session_id(&header);
                    let snapshot = DhlmSnapshot {
                        header: header.clone(),
                        entries: latest_entries.clone(),
                        session_id: session_id.clone(),
                        fingerprint: meaningful_snapshot_fingerprint(&header, &latest_entries),
                    };

                    let first_real_of_session = last_good_snapshot.is_none()
                        || !latest_entries.is_empty()
                            && session_id.as_ref() != last_session_id.as_ref();
                    let session_complete = snapshot.header.flag.eq_ignore_ascii_case("checkered");
                    let materially_changed = last_good_snapshot
                        .as_ref()
                        .map(|prev| prev.fingerprint != snapshot.fingerprint)
                        .unwrap_or(true);

                    if first_real_of_session || materially_changed || session_complete {
                        if first_real_of_session || session_complete {
                            persist_snapshot(
                                &mut persist,
                                &snapshot,
                                now_millis() as u64,
                                &debug_output,
                            );
                        }
                        let _ = tx.send(TimingMessage::Snapshot {
                            source_id,
                            header: header.clone(),
                            entries: latest_entries.clone(),
                        });
                        last_good_snapshot = Some(snapshot);
                        if let Some(ref sid) = session_id {
                            last_session_id = Some(sid.clone());
                        }
                    }
                }
                Ok(Message::Binary(data)) => {
                    if let Ok(text) = String::from_utf8(data.to_vec()) {
                        if parse_timing_message(&text, &mut header, &mut latest_entries) {
                            let _ = tx.send(TimingMessage::Snapshot {
                                source_id,
                                header: header.clone(),
                                entries: latest_entries.clone(),
                            });
                        }
                    }
                }
                Ok(Message::Close(_)) => break,
                Err(_) => {
                    if stop_rx.recv_timeout(Duration::from_secs(3)).is_ok() {
                        break 'outer;
                    }
                    continue;
                }
                _ => {}
            }
        }
    }
}

fn parse_timing_message(
    text: &str,
    header: &mut TimingHeader,
    entries: &mut Vec<TimingEntry>,
) -> bool {
    let parsed: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return false,
    };

    let pid = match parsed.get("PID").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return false,
    };

    match pid {
        "0" => {
            if let Some(cup) = parsed.get("CUP").and_then(|v| v.as_str()) {
                header.event_name = cup.to_string();
            }
            if let Some(track) = parsed.get("TRACKNAME").or_else(|| parsed.get("TRACK")) {
                if let Some(name) = track.as_str() {
                    header.track_name = name.to_string();
                }
            }
            if let Some(heat) = parsed.get("HEATTYPE").and_then(|v| v.as_str()) {
                header.session_name = match heat {
                    "R" => "Race".to_string(),
                    "Q" => "Qualifying".to_string(),
                    _ => heat.to_string(),
                };
            }
            if let Some(results) = parsed.get("RESULT").and_then(|v| v.as_array()) {
                *entries = results.iter().filter_map(parse_entry).collect();
                entries.sort_by_key(|e| e.position);
            }
            true
        }
        "4" => {
            if let Some(time) = parsed.get("TIME").and_then(|v| v.as_str()) {
                header.day_time = time.to_string();
            }
            if let Some(track) = parsed.get("TRACKNAME").or_else(|| parsed.get("TRACK")) {
                if let Some(name) = track.as_str() {
                    header.track_name = name.to_string();
                }
            }
            if let Some(state) = parsed.get("TRACKSTATE").and_then(|v| v.as_str()) {
                header.flag = match state {
                    "1" => "Yellow".to_string(),
                    "2" => "Red".to_string(),
                    "3" => "FCY".to_string(),
                    _ => String::new(),
                };
            }
            if let Some(heat) = parsed.get("HEATTYPE").and_then(|v| v.as_str()) {
                header.session_name = match heat {
                    "R" => "Race".to_string(),
                    "Q" => "Qualifying".to_string(),
                    _ => heat.to_string(),
                };
            }
            true
        }
        _ => false,
    }
}

fn parse_entry(value: &Value) -> Option<TimingEntry> {
    entry_from_value(value, "50")
}

fn load_dump_file(path: &Option<PathBuf>) -> Option<Vec<String>> {
    let path = path.as_ref()?;
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);
    let mut lines = Vec::new();
    for line in reader.lines() {
        match line {
            Ok(l) => lines.push(l),
            Err(_) => break,
        }
    }
    Some(lines)
}
