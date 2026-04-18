use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

#[derive(Debug, Clone)]
pub enum DdpIncoming {
    Connected,
    Ready,
    Ping {
        id: Option<String>,
    },
    Pong,
    Added {
        collection: String,
        id: String,
        fields: Map<String, Value>,
    },
    Changed {
        collection: String,
        id: String,
        fields: Map<String, Value>,
        cleared: Vec<String>,
    },
    Removed {
        collection: String,
        id: String,
    },
    Unknown,
}

#[derive(Debug, Clone)]
pub enum SockJsPacket {
    Open,
    Heartbeat,
    Messages(Vec<String>),
    Close,
    Unknown,
}

#[derive(Debug, Serialize)]
struct DdpConnect<'a> {
    msg: &'a str,
    version: &'a str,
    support: [&'a str; 1],
}

#[derive(Debug, Serialize)]
struct DdpSub<'a> {
    msg: &'a str,
    id: String,
    name: &'a str,
    params: Vec<Value>,
}

#[derive(Debug, Serialize)]
struct DdpPong {
    msg: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IncomingEnvelope {
    msg: String,
    #[serde(default)]
    collection: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    fields: Option<Map<String, Value>>,
    #[serde(default)]
    cleared: Option<Vec<String>>,
}

pub fn connect_message() -> String {
    serde_json::to_string(&DdpConnect {
        msg: "connect",
        version: "1",
        support: ["1"],
    })
    .unwrap_or_else(|_| "{\"msg\":\"connect\",\"version\":\"1\",\"support\":[\"1\"]}".to_string())
}

pub fn sub_message(id: String, name: &str, params: Vec<Value>) -> String {
    serde_json::to_string(&DdpSub {
        msg: "sub",
        id,
        name,
        params,
    })
    .unwrap_or_else(|_| {
        json!({
            "msg": "sub",
            "id": "sub-fallback",
            "name": name,
            "params": []
        })
        .to_string()
    })
}

pub fn pong_message(id: Option<String>) -> String {
    serde_json::to_string(&DdpPong { msg: "pong", id })
        .unwrap_or_else(|_| "{\"msg\":\"pong\"}".to_string())
}

pub fn oid_value(value: &str) -> Value {
    json!({
        "$type": "oid",
        "$value": value,
    })
}

pub fn parse_sockjs_packet(text: &str) -> SockJsPacket {
    let trimmed = text.trim();
    if trimmed == "o" {
        return SockJsPacket::Open;
    }
    if trimmed == "h" {
        return SockJsPacket::Heartbeat;
    }
    if trimmed.starts_with('a') {
        let payload = trimmed.strip_prefix('a').unwrap_or("");
        if let Ok(messages) = serde_json::from_str::<Vec<String>>(payload) {
            return SockJsPacket::Messages(messages);
        }
        return SockJsPacket::Unknown;
    }
    if trimmed.starts_with('c') {
        return SockJsPacket::Close;
    }
    SockJsPacket::Unknown
}

pub fn parse_ddp_message(text: &str) -> DdpIncoming {
    let Ok(envelope) = serde_json::from_str::<IncomingEnvelope>(text) else {
        return DdpIncoming::Unknown;
    };

    match envelope.msg.as_str() {
        "connected" => DdpIncoming::Connected,
        "ready" => DdpIncoming::Ready,
        "ping" => {
            let id = serde_json::from_str::<Value>(text)
                .ok()
                .and_then(|v| v.get("id").cloned())
                .and_then(|v| v.as_str().map(ToString::to_string));
            DdpIncoming::Ping { id }
        }
        "pong" => DdpIncoming::Pong,
        "added" => {
            let Some(collection) = envelope.collection else {
                return DdpIncoming::Unknown;
            };
            let Some(id) = envelope.id else {
                return DdpIncoming::Unknown;
            };
            DdpIncoming::Added {
                collection,
                id,
                fields: envelope.fields.unwrap_or_default(),
            }
        }
        "changed" => {
            let Some(collection) = envelope.collection else {
                return DdpIncoming::Unknown;
            };
            let Some(id) = envelope.id else {
                return DdpIncoming::Unknown;
            };
            DdpIncoming::Changed {
                collection,
                id,
                fields: envelope.fields.unwrap_or_default(),
                cleared: envelope.cleared.unwrap_or_default(),
            }
        }
        "removed" => {
            let Some(collection) = envelope.collection else {
                return DdpIncoming::Unknown;
            };
            let Some(id) = envelope.id else {
                return DdpIncoming::Unknown;
            };
            DdpIncoming::Removed { collection, id }
        }
        _ => DdpIncoming::Unknown,
    }
}
