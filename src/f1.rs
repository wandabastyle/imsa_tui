// F1 SignalR-style adapter: negotiates session, subscribes to topics, and builds live leaderboard snapshots.

use std::{
    collections::HashMap,
    io,
    sync::mpsc::{Receiver, Sender},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use reqwest::blocking::Client;
use serde_json::{json, Map, Value};
use tungstenite::{
    client::IntoClientRequest,
    connect,
    http::header::{HeaderValue, ORIGIN, USER_AGENT},
    stream::MaybeTlsStream,
    Error as WsError, Message,
};

use crate::timing::{TimingEntry, TimingHeader, TimingMessage};

const HUB_NAME: &str = "streaming";
const CLIENT_PROTOCOL: &str = "1.5";
const NEGOTIATE_URL: &str = "https://livetiming.formula1.com/signalr/negotiate";
const START_URL: &str = "https://livetiming.formula1.com/signalr/start";
const WS_CONNECT_URL: &str = "wss://livetiming.formula1.com/signalr/connect";

const SUBSCRIBE_TOPICS: &[&str] = &[
    "SessionInfo",
    "ExtrapolatedClock",
    "LapCount",
    "DriverList",
    "TimingData",
    "TimingStats",
    "TrackStatus",
    "RaceControlMessages",
    "Position.z",
    "CarData.z",
];

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
        if self.code != "-" {
            if self.full_name != "-" {
                format!("{} ({})", self.code, self.full_name)
            } else {
                self.code.clone()
            }
        } else {
            self.full_name.clone()
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

#[derive(Debug)]
struct SignalRConnection {
    connection_token: String,
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_millis()
}

fn hub_connection_data() -> String {
    format!("[{{\"name\":\"{HUB_NAME}\"}}]")
}

fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn set_socket_timeout(socket: &mut tungstenite::WebSocket<MaybeTlsStream<std::net::TcpStream>>) {
    if let MaybeTlsStream::Plain(stream) = socket.get_mut() {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    }
}

fn build_ws_request(
    connection_token: &str,
) -> Result<tungstenite::handshake::client::Request, String> {
    let url = format!(
        "{WS_CONNECT_URL}?transport=webSockets&clientProtocol={CLIENT_PROTOCOL}&connectionToken={}&connectionData={}&tid=9",
        percent_encode(connection_token),
        percent_encode(&hub_connection_data())
    );

    let mut request = url
        .into_client_request()
        .map_err(|e| format!("failed to build websocket request: {e}"))?;

    request.headers_mut().insert(
        ORIGIN,
        HeaderValue::from_static("https://livetiming.formula1.com"),
    );
    request
        .headers_mut()
        .insert(USER_AGENT, HeaderValue::from_static("Mozilla/5.0"));

    Ok(request)
}

fn negotiate(client: &Client) -> Result<SignalRConnection, String> {
    let connection_data = hub_connection_data();
    let url = format!(
        "{NEGOTIATE_URL}?clientProtocol={CLIENT_PROTOCOL}&connectionData={}&_={}",
        percent_encode(&connection_data),
        now_millis()
    );

    let response_text = client
        .get(url)
        .header("User-Agent", "Mozilla/5.0")
        .header("Accept", "application/json")
        .header("Origin", "https://livetiming.formula1.com")
        .send()
        .map_err(|e| format!("negotiate request failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("negotiate http error: {e}"))?
        .text()
        .map_err(|e| format!("negotiate body read failed: {e}"))?;

    let root: Value = serde_json::from_str(&response_text)
        .map_err(|e| format!("negotiate json parse failed: {e}"))?;

    let connection_token = root
        .get("ConnectionToken")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing ConnectionToken in negotiate response".to_string())?
        .to_string();

    Ok(SignalRConnection { connection_token })
}

fn start_session(client: &Client, connection_token: &str) -> Result<(), String> {
    let url = format!(
        "{START_URL}?transport=webSockets&clientProtocol={CLIENT_PROTOCOL}&connectionToken={}&connectionData={}&_={}",
        percent_encode(connection_token),
        percent_encode(&hub_connection_data()),
        now_millis()
    );

    client
        .get(url)
        .header("User-Agent", "Mozilla/5.0")
        .header("Accept", "application/json")
        .header("Origin", "https://livetiming.formula1.com")
        .send()
        .map_err(|e| format!("start request failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("start http error: {e}"))?;

    Ok(())
}

fn subscribe_message(invoke_id: u64) -> Value {
    json!({
        "H": HUB_NAME,
        "M": "Subscribe",
        "A": [SUBSCRIBE_TOPICS],
        "I": invoke_id,
    })
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
        if !first.is_empty() || !last.is_empty() {
            state.full_name = format!("{} {}", first, last).trim().to_string();
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
            break;
        }

        let _ = tx.send(TimingMessage::Status {
            source_id,
            text: "Negotiating F1 SignalR connection...".to_string(),
        });

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

        let request = match build_ws_request(&negotiated.connection_token) {
            Ok(r) => r,
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

        if let Err(err) = start_session(&client, &negotiated.connection_token) {
            let _ = tx.send(TimingMessage::Error {
                source_id,
                text: err,
            });
        }

        let subscribe = subscribe_message(1).to_string();
        if let Err(err) = socket.send(Message::Text(subscribe)) {
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
                break 'outer;
            }

            match socket.read() {
                Ok(Message::Text(text)) => {
                    if process_signalr_message(&mut state, &text) {
                        let (header, entries) = snapshot_from_state(&state);
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
        if stop_rx.recv_timeout(Duration::from_secs(4)).is_ok() {
            break;
        }
    }
}
