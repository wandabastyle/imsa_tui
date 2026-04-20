use std::net::TcpStream;

use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::Value;
use tungstenite::{
    client::IntoClientRequest,
    connect,
    http::header::{HeaderValue, ORIGIN, USER_AGENT},
    stream::MaybeTlsStream,
    Error as WsError, Message, WebSocket,
};

pub(crate) const NEGOTIATE_URL: &str =
    "https://insights.griiip.com/live-session-stream/negotiate?negotiateVersion=1";
pub(crate) const ORIGIN_URL: &str = "https://insights.griiip.com";
const SIGNALR_RS: char = '\u{1e}';

#[derive(Debug, Deserialize)]
pub(crate) struct NegotiateResponse {
    pub(crate) url: String,
    #[serde(rename = "accessToken")]
    pub(crate) access_token: String,
}

#[derive(Debug)]
pub(crate) enum SignalRFrame {
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

pub(crate) fn negotiate(client: &Client) -> Result<NegotiateResponse, String> {
    let response = client
        .post(NEGOTIATE_URL)
        .body("")
        .send()
        .map_err(|err| format!("negotiate request failed: {err}"))?;
    if !response.status().is_success() {
        return Err(format!("negotiate failed with HTTP {}", response.status()));
    }
    let body = response
        .text()
        .map_err(|err| format!("negotiate body read failed: {err}"))?;
    serde_json::from_str::<NegotiateResponse>(&body)
        .map_err(|err| format!("negotiate decode failed: {err}"))
}

pub(crate) fn websocket_url_from_negotiate(base_url: &str, token: &str) -> String {
    let ws_base = if let Some(rest) = base_url.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = base_url.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        base_url.to_string()
    };

    if ws_base.contains("access_token=") {
        return ws_base;
    }

    let separator = if ws_base.contains('?') { "&" } else { "?" };
    format!("{ws_base}{separator}access_token={token}")
}

pub(crate) fn build_request(url: &str) -> Result<tungstenite::handshake::client::Request, String> {
    let mut request = url
        .into_client_request()
        .map_err(|err| format!("websocket request build failed: {err}"))?;
    request
        .headers_mut()
        .insert(ORIGIN, HeaderValue::from_static(ORIGIN_URL));
    request
        .headers_mut()
        .insert(USER_AGENT, HeaderValue::from_static("Mozilla/5.0"));
    Ok(request)
}

pub(crate) fn connect_signalr(
    request: tungstenite::handshake::client::Request,
) -> Result<WebSocket<MaybeTlsStream<TcpStream>>, String> {
    let (socket, _) = connect(request).map_err(|err| format!("websocket connect failed: {err}"))?;
    Ok(socket)
}

pub(crate) fn send_signalr_handshake(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
) -> Result<(), String> {
    let payload = format!("{{\"protocol\":\"json\",\"version\":1}}{SIGNALR_RS}");
    socket
        .send(Message::Text(payload.into()))
        .map_err(|err| format!("handshake send failed: {err}"))
}

pub(crate) fn send_join_group(
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

pub(crate) fn send_signalr_json(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    payload: &Value,
) -> Result<(), String> {
    let mut encoded = serde_json::to_string(payload)
        .map_err(|err| format!("signalr payload encode failed: {err}"))?;
    encoded.push(SIGNALR_RS);
    socket
        .send(Message::Text(encoded.into()))
        .map_err(|err| format!("signalr send failed: {err}"))
}

pub(crate) fn split_signalr_frames(raw: &str) -> Vec<&str> {
    raw.split(SIGNALR_RS)
        .map(str::trim)
        .filter(|frame| !frame.is_empty())
        .collect()
}

pub(crate) fn parse_signalr_frame(frame: &str) -> SignalRFrame {
    if frame == "{}" {
        return SignalRFrame::HandshakeAck;
    }

    let value: Value = match serde_json::from_str(frame) {
        Ok(value) => value,
        Err(_) => return SignalRFrame::Unknown,
    };

    let Some(message_type) = value.get("type").and_then(Value::as_i64) else {
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

pub(crate) fn is_retriable_timeout(err: &WsError) -> bool {
    matches!(
        err,
        WsError::Io(io_err)
            if io_err.kind() == std::io::ErrorKind::TimedOut
                || io_err.kind() == std::io::ErrorKind::WouldBlock
    )
}
