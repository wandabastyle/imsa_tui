use std::{
    io,
    sync::mpsc::{Receiver, Sender},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde_json::{json, Value};
use tungstenite::{
    client::IntoClientRequest,
    connect,
    http::header::{HeaderValue, ORIGIN, USER_AGENT},
    stream::MaybeTlsStream,
    Error as WsError, Message,
};

use crate::{
    imsa::normalize_class_name,
    timing::{TimingEntry, TimingHeader, TimingMessage},
};

const WS_URL: &str = "wss://livetiming.azurewebsites.net/";
const EVENT_ID: &str = "20";

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_millis()
}

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

fn get_str<'a>(obj: &'a Value, key: &str) -> Option<&'a str> {
    obj.get(key).and_then(|x| x.as_str())
}

fn parse_u32_field(obj: &Value, key: &str) -> Option<u32> {
    if let Some(s) = get_str(obj, key) {
        return s.trim().parse::<u32>().ok();
    }
    obj.get(key)
        .and_then(|x| x.as_u64())
        .and_then(|n| u32::try_from(n).ok())
}

fn entry_from_value(v: &Value) -> Option<TimingEntry> {
    let car_number = parse_u32_field(v, "STNR")?.to_string();
    let class_name = get_str(v, "CLASSNAME").unwrap_or("-").to_string();
    let stable_id = format!("stnr:{}:{}", car_number, normalize_class_name(&class_name));

    Some(TimingEntry {
        position: parse_u32_field(v, "POSITION")?,
        car_number,
        class_name,
        class_rank: parse_u32_field(v, "CLASSRANK").unwrap_or(0).to_string(),
        driver: get_str(v, "NAME").unwrap_or("-").to_string(),
        vehicle: get_str(v, "CAR").unwrap_or("-").to_string(),
        team: get_str(v, "TEAM").unwrap_or("-").to_string(),
        laps: get_str(v, "LAPS").unwrap_or("-").to_string(),
        gap_overall: get_str(v, "GAP").unwrap_or("-").to_string(),
        gap_class: "-".to_string(),
        gap_next_in_class: "-".to_string(),
        last_lap: get_str(v, "LASTLAPTIME").unwrap_or("-").to_string(),
        best_lap: get_str(v, "FASTESTLAP").unwrap_or("-").to_string(),
        best_lap_no: "-".to_string(),
        pit: "-".to_string(),
        pit_stops: "-".to_string(),
        fastest_driver: "-".to_string(),
        stable_id,
    })
}

fn format_duration_ms(ms: u64) -> String {
    let total_secs = ms / 1000;
    let h = total_secs / 3600;
    let m = (total_secs % 3600) / 60;
    let s = total_secs % 60;
    format!("{h:02}:{m:02}:{s:02}")
}

fn current_time_to_end(
    header: &TimingHeader,
    end_time_raw: u64,
    time_state_raw: &str,
    received_at_ms: u64,
) -> String {
    if end_time_raw == 0 {
        return header.time_to_go.clone();
    }

    let now = now_millis() as u64;

    let remaining_ms = if time_state_raw == "0" {
        let elapsed = now.saturating_sub(received_at_ms);
        end_time_raw.saturating_sub(elapsed)
    } else {
        end_time_raw.saturating_sub(now)
    };

    format_duration_ms(remaining_ms)
}

fn track_state_text(raw: &str) -> String {
    match raw {
        "0" => "Green".to_string(),
        "1" => "Yellow".to_string(),
        "2" => "Code 60".to_string(),
        other => other.to_string(),
    }
}

fn session_text(raw: &str) -> String {
    match raw {
        "R" => "Race".to_string(),
        "Q" => "Qualifying".to_string(),
        "T" => "Practice".to_string(),
        other => other.to_string(),
    }
}

fn parse_ws_message(
    text: &str,
    header: &mut TimingHeader,
) -> Option<(Option<Vec<TimingEntry>>, bool)> {
    let parsed: Value = serde_json::from_str(text).ok()?;
    let pid = get_str(&parsed, "PID")?;

    match pid {
        "0" => {
            let results = parsed.get("RESULT")?.as_array()?;
            let mut entries: Vec<TimingEntry> =
                results.iter().filter_map(entry_from_value).collect();
            entries.sort_by_key(|e| e.position);
            Some((Some(entries), false))
        }
        "4" => {
            header.session_name = session_text(get_str(&parsed, "HEATTYPE").unwrap_or("-"));
            header.flag = track_state_text(get_str(&parsed, "TRACKSTATE").unwrap_or("-"));
            header.track_name = get_str(&parsed, "TRACK").unwrap_or("NLS").to_string();
            header.event_name = get_str(&parsed, "EVENTNAME")
                .unwrap_or("NLS Live Timing")
                .to_string();
            let end_time_raw = get_str(&parsed, "ENDTIME")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            let time_state_raw = get_str(&parsed, "TIMESTATE").unwrap_or("0");
            header.day_time = get_str(&parsed, "TIME").unwrap_or("-").to_string();
            header.time_to_go =
                current_time_to_end(header, end_time_raw, time_state_raw, now_millis() as u64);
            Some((None, true))
        }
        "LTS_TIMESYNC" => None,
        _ => None,
    }
}

fn set_socket_timeout(socket: &mut tungstenite::WebSocket<MaybeTlsStream<std::net::TcpStream>>) {
    match socket.get_mut() {
        MaybeTlsStream::Plain(stream) => {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        }
        MaybeTlsStream::NativeTls(stream) => {
            let _ = stream
                .get_ref()
                .set_read_timeout(Some(Duration::from_secs(2)));
        }
        _ => {}
    }
}

pub fn websocket_worker(tx: Sender<TimingMessage>, source_id: u64, stop_rx: Receiver<()>) {
    let mut header = TimingHeader {
        event_name: "NLS Live Timing".to_string(),
        track_name: "Nürburgring".to_string(),
        ..TimingHeader::default()
    };
    let mut latest_entries: Vec<TimingEntry> = Vec::new();

    'outer: loop {
        if stop_rx.try_recv().is_ok() {
            break;
        }

        let _ = tx.send(TimingMessage::Status {
            source_id,
            text: "Connecting to NLS websocket...".to_string(),
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
            text: format!("NLS connected ({})", response.status()),
        });

        let subscribe = json!({
            "clientLocalTime": now_millis(),
            "eventId": EVENT_ID,
            "eventPid": [0, 4]
        });

        if let Err(err) = socket.send(Message::Text(subscribe.to_string().into())) {
            let _ = tx.send(TimingMessage::Error {
                source_id,
                text: format!("subscribe failed: {err}"),
            });
            if stop_rx.recv_timeout(Duration::from_secs(3)).is_ok() {
                break;
            }
            continue;
        }

        loop {
            if stop_rx.try_recv().is_ok() {
                break 'outer;
            }

            match socket.read() {
                Ok(Message::Text(text)) => {
                    if let Some((entries, header_changed)) = parse_ws_message(&text, &mut header) {
                        if let Some(new_entries) = entries {
                            latest_entries = new_entries;
                        }
                        let _ = tx.send(TimingMessage::Snapshot {
                            source_id,
                            header: header.clone(),
                            entries: latest_entries.clone(),
                        });
                        if header_changed {
                            let _ = tx.send(TimingMessage::Status {
                                source_id,
                                text: "NLS live timing connected".to_string(),
                            });
                        }
                    }
                }
                Ok(Message::Binary(data)) => {
                    if let Ok(text) = std::str::from_utf8(&data) {
                        if let Some((entries, _)) = parse_ws_message(text, &mut header) {
                            if let Some(new_entries) = entries {
                                latest_entries = new_entries;
                            }
                            let _ = tx.send(TimingMessage::Snapshot {
                                source_id,
                                header: header.clone(),
                                entries: latest_entries.clone(),
                            });
                        }
                    }
                }
                Ok(Message::Ping(data)) => {
                    if let Err(err) = socket.send(Message::Pong(data)) {
                        let _ = tx.send(TimingMessage::Error {
                            source_id,
                            text: format!("pong failed: {err}"),
                        });
                        break;
                    }
                }
                Ok(Message::Pong(_)) => {}
                Ok(Message::Close(frame)) => {
                    let _ = tx.send(TimingMessage::Error {
                        source_id,
                        text: format!("socket closed: {frame:?}"),
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
                        text: format!("read failed: {err}"),
                    });
                    break;
                }
            }
        }

        let _ = tx.send(TimingMessage::Status {
            source_id,
            text: "NLS reconnecting in 3s...".to_string(),
        });
        if stop_rx.recv_timeout(Duration::from_secs(3)).is_ok() {
            break;
        }
    }
}
