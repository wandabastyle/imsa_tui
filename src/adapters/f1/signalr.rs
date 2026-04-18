use std::{
    net::TcpStream,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use reqwest::blocking::Client;
use serde_json::{json, Value};
use tungstenite::{
    client::IntoClientRequest,
    http::header::{HeaderValue, ORIGIN, USER_AGENT},
    stream::MaybeTlsStream,
};

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

#[derive(Debug)]
pub(super) struct SignalRConnection {
    pub(super) connection_token: String,
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_millis()
}

fn hub_connection_data() -> String {
    format!(r#"[{{\"name\":\"{}\"}}]"#, HUB_NAME)
}

fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len() * 3);
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(char::from(b))
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

pub(super) fn set_socket_timeout(socket: &mut tungstenite::WebSocket<MaybeTlsStream<TcpStream>>) {
    if let MaybeTlsStream::Rustls(stream) = socket.get_mut() {
        let tcp = stream.get_mut();
        let _ = tcp.set_read_timeout(Some(Duration::from_secs(2)));
    }
}

pub(super) fn build_ws_request(connection_token: &str) -> tungstenite::handshake::client::Request {
    let conn_data = percent_encode(&hub_connection_data());
    let token = percent_encode(connection_token);
    let url = format!(
        "{WS_CONNECT_URL}?transport=webSockets&clientProtocol={CLIENT_PROTOCOL}&connectionToken={token}&connectionData={conn_data}&tid=8"
    );

    let mut request = url
        .into_client_request()
        .expect("failed to create websocket request");
    request.headers_mut().insert(
        ORIGIN,
        HeaderValue::from_static("https://livetiming.formula1.com"),
    );
    request
        .headers_mut()
        .insert(USER_AGENT, HeaderValue::from_static("Mozilla/5.0"));
    request
}

pub(super) fn negotiate(client: &Client) -> Result<SignalRConnection, String> {
    let conn_data = percent_encode(&hub_connection_data());
    let url = format!(
        "{NEGOTIATE_URL}?_={}&clientProtocol={CLIENT_PROTOCOL}&connectionData={conn_data}",
        now_millis()
    );
    let response = client
        .get(&url)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .map_err(|e| format!("negotiate request failed: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("negotiate http {}", response.status()));
    }
    let payload_text = response
        .text()
        .map_err(|e| format!("negotiate body read failed: {e}"))?;
    let payload: Value =
        serde_json::from_str(&payload_text).map_err(|e| format!("negotiate decode failed: {e}"))?;
    let token = payload
        .get("ConnectionToken")
        .and_then(Value::as_str)
        .ok_or_else(|| "negotiate missing ConnectionToken".to_string())?
        .to_string();
    Ok(SignalRConnection {
        connection_token: token,
    })
}

pub(super) fn start_session(client: &Client, connection_token: &str) -> Result<(), String> {
    let conn_data = percent_encode(&hub_connection_data());
    let token = percent_encode(connection_token);
    let url = format!(
        "{START_URL}?_={}&transport=webSockets&clientProtocol={CLIENT_PROTOCOL}&connectionToken={token}&connectionData={conn_data}",
        now_millis()
    );
    let response = client
        .get(&url)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .map_err(|e| format!("start request failed: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("start http {}", response.status()));
    }
    Ok(())
}

pub(super) fn subscribe_message(invoke_id: u64) -> Value {
    json!({
        "H": HUB_NAME,
        "M": "Subscribe",
        "A": [SUBSCRIBE_TOPICS],
        "I": invoke_id,
    })
}
