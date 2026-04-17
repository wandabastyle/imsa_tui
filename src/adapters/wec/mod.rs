mod ddp;
mod model;
mod normalize;
mod store;
mod transport;

use std::{
    collections::HashSet,
    sync::mpsc::{Receiver, Sender},
    time::{Duration, Instant},
};

use serde_json::Value;

use crate::timing::TimingMessage;

use self::{
    ddp::{
        connect_message, oid_value, parse_ddp_message, pong_message, sub_message, DdpIncoming,
        SockJsPacket,
    },
    normalize::snapshot_from_store,
    store::CollectionStore,
    transport::SockJsTransport,
};

const FEED_NAME: &str = "fiawec";
const RECONNECT_DELAY: Duration = Duration::from_secs(4);
const COLLECTION_STATS_INTERVAL: Duration = Duration::from_secs(15);

const SESSION_SCOPED_PUBLICATIONS: &[&str] = &[
    "sessionClasses",
    "trackInfo",
    "standings",
    "entry",
    "pitInfo",
    "raceControl",
    "sessionResults",
    "sessionStatus",
    "weather",
    "countStates",
    "bestResults",
    "sessionBestResultsByClass",
];

/// Reverse engineered public LT2 flow (first pass):
/// 1) SockJS websocket open
/// 2) DDP connect
/// 3) subscribe(livetimingFeed, ["fiawec"])
/// 4) discover sessions -> subscribe(sessionInfo, [sessions])
/// 5) discover current session oid -> subscribe session-scoped publications
///
/// TODO: tighten session discovery to exact collection once enough live captures are archived.
pub fn websocket_worker(tx: Sender<TimingMessage>, source_id: u64, stop_rx: Receiver<()>) {
    let debug_unknown = env_flag("WEC_DEBUG_UNKNOWN", false);
    let debug_subscriptions = env_flag("WEC_DEBUG_SUBS", false);
    let emit_collection_stats = env_flag("WEC_COLLECTION_COUNTS", false);

    'outer: loop {
        if stop_rx.try_recv().is_ok() {
            break;
        }

        let _ = tx.send(TimingMessage::Status {
            source_id,
            text: "Connecting to WEC websocket...".to_string(),
        });

        let mut transport = match SockJsTransport::connect_from_env() {
            Ok(transport) => transport,
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

        let mut got_sockjs_open = false;
        let open_deadline = Instant::now() + Duration::from_secs(6);
        while Instant::now() < open_deadline {
            if stop_rx.try_recv().is_ok() {
                break 'outer;
            }
            match transport.read_packet() {
                Ok(Some(SockJsPacket::Open)) => {
                    got_sockjs_open = true;
                    break;
                }
                Ok(Some(SockJsPacket::Close)) => break,
                Ok(_) => continue,
                Err(err) => {
                    let _ = tx.send(TimingMessage::Error {
                        source_id,
                        text: err,
                    });
                    break;
                }
            }
        }

        if !got_sockjs_open {
            let _ = tx.send(TimingMessage::Error {
                source_id,
                text: "WEC SockJS open handshake did not complete".to_string(),
            });
            if stop_rx.recv_timeout(RECONNECT_DELAY).is_ok() {
                break;
            }
            continue;
        }

        if let Err(err) = transport.send_ddp(connect_message()) {
            let _ = tx.send(TimingMessage::Error {
                source_id,
                text: err,
            });
            if stop_rx.recv_timeout(RECONNECT_DELAY).is_ok() {
                break;
            }
            continue;
        }

        let mut store = CollectionStore::default();
        let mut connected = false;
        let mut subscribed = HashSet::<String>::new();
        let mut next_sub_id: u64 = 1;
        let mut last_stats_emit = Instant::now();

        loop {
            if stop_rx.try_recv().is_ok() {
                break 'outer;
            }

            let packet = match transport.read_packet() {
                Ok(packet) => packet,
                Err(err) => {
                    let _ = tx.send(TimingMessage::Error {
                        source_id,
                        text: err,
                    });
                    break;
                }
            };

            let Some(packet) = packet else {
                maybe_emit_collection_stats(
                    &tx,
                    source_id,
                    &store,
                    emit_collection_stats,
                    &mut last_stats_emit,
                );
                continue;
            };

            match packet {
                SockJsPacket::Open | SockJsPacket::Heartbeat => {}
                SockJsPacket::Close => {
                    let _ = tx.send(TimingMessage::Error {
                        source_id,
                        text: "WEC websocket closed".to_string(),
                    });
                    break;
                }
                SockJsPacket::Messages(messages) => {
                    for raw in messages {
                        match parse_ddp_message(&raw) {
                            DdpIncoming::Connected => {
                                connected = true;
                                let _ = tx.send(TimingMessage::Status {
                                    source_id,
                                    text: "WEC DDP connected".to_string(),
                                });
                                subscribe_if_needed(
                                    &mut transport,
                                    &mut subscribed,
                                    &mut next_sub_id,
                                    "livetimingFeed",
                                    vec![Value::String(FEED_NAME.to_string())],
                                    debug_subscriptions,
                                    Some((&tx, source_id)),
                                );
                            }
                            DdpIncoming::Ready => {}
                            DdpIncoming::Ping { id } => {
                                let _ = transport.send_ddp(pong_message(id));
                            }
                            DdpIncoming::Pong => {}
                            DdpIncoming::Added {
                                collection,
                                id,
                                fields,
                            } => {
                                store.apply_added(&collection, &id, fields);
                                subscribe_from_discovery(
                                    &mut transport,
                                    &mut subscribed,
                                    &mut next_sub_id,
                                    &store,
                                    debug_subscriptions,
                                    Some((&tx, source_id)),
                                );
                                emit_snapshot(&tx, source_id, &store);
                            }
                            DdpIncoming::Changed {
                                collection,
                                id,
                                fields,
                                cleared,
                            } => {
                                store.apply_changed(&collection, &id, fields, &cleared);
                                subscribe_from_discovery(
                                    &mut transport,
                                    &mut subscribed,
                                    &mut next_sub_id,
                                    &store,
                                    debug_subscriptions,
                                    Some((&tx, source_id)),
                                );
                                emit_snapshot(&tx, source_id, &store);
                            }
                            DdpIncoming::Removed { collection, id } => {
                                store.apply_removed(&collection, &id);
                                emit_snapshot(&tx, source_id, &store);
                            }
                            DdpIncoming::Unknown => {
                                if debug_unknown {
                                    let _ = tx.send(TimingMessage::Status {
                                        source_id,
                                        text: format!("WEC unknown DDP frame: {raw}"),
                                    });
                                }
                            }
                        }
                    }
                }
                SockJsPacket::Unknown => {
                    if debug_unknown {
                        let _ = tx.send(TimingMessage::Status {
                            source_id,
                            text: "WEC unknown SockJS packet".to_string(),
                        });
                    }
                }
            }

            if connected {
                maybe_emit_collection_stats(
                    &tx,
                    source_id,
                    &store,
                    emit_collection_stats,
                    &mut last_stats_emit,
                );
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

fn subscribe_from_discovery(
    transport: &mut SockJsTransport,
    subscribed: &mut HashSet<String>,
    next_sub_id: &mut u64,
    store: &CollectionStore,
    debug_subscriptions: bool,
    debug_sink: Option<(&Sender<TimingMessage>, u64)>,
) {
    if let Some(sessions_param) = discover_sessions_param(store) {
        subscribe_if_needed(
            transport,
            subscribed,
            next_sub_id,
            "sessions",
            vec![sessions_param.clone()],
            debug_subscriptions,
            debug_sink,
        );
        subscribe_if_needed(
            transport,
            subscribed,
            next_sub_id,
            "events",
            vec![sessions_param.clone()],
            debug_subscriptions,
            debug_sink,
        );
        subscribe_if_needed(
            transport,
            subscribed,
            next_sub_id,
            "sessionInfo",
            vec![sessions_param],
            debug_subscriptions,
            debug_sink,
        );
    }

    if let Some(session_param) = discover_current_session_param(store) {
        for publication in SESSION_SCOPED_PUBLICATIONS {
            subscribe_if_needed(
                transport,
                subscribed,
                next_sub_id,
                publication,
                vec![session_param.clone()],
                debug_subscriptions,
                debug_sink,
            );
        }
    }
}

fn subscribe_if_needed(
    transport: &mut SockJsTransport,
    subscribed: &mut HashSet<String>,
    next_sub_id: &mut u64,
    publication: &str,
    params: Vec<Value>,
    debug_subscriptions: bool,
    debug_sink: Option<(&Sender<TimingMessage>, u64)>,
) {
    let key = format!(
        "{}:{}",
        publication,
        serde_json::to_string(&params).unwrap_or_default()
    );
    if subscribed.contains(&key) {
        return;
    }

    let sub_id = format!("sub-{}", *next_sub_id);
    *next_sub_id = next_sub_id.saturating_add(1);

    let message = sub_message(sub_id, publication, params);
    if transport.send_ddp(message).is_ok() {
        subscribed.insert(key);
        if debug_subscriptions {
            if let Some((tx, source_id)) = debug_sink {
                let _ = tx.send(TimingMessage::Status {
                    source_id,
                    text: format!("WEC sub sent: {publication}"),
                });
            }
        }
    }
}

fn discover_sessions_param(store: &CollectionStore) -> Option<Value> {
    for docs in store.collections().values() {
        for doc in docs.values() {
            if let Some(value) = doc.get("sessions") {
                return Some(value.clone());
            }
        }
    }
    None
}

fn discover_current_session_param(store: &CollectionStore) -> Option<Value> {
    for collection_name in [
        "session_live",
        "session_info",
        "session_status",
        "standings",
        "track_info",
    ] {
        let Some(docs) = store.collection(collection_name) else {
            continue;
        };
        for doc in docs.values() {
            if let Some(session_value) = find_session_value(doc) {
                return Some(normalize_session_param(session_value));
            }
        }
    }
    None
}

fn find_session_value(doc: &Value) -> Option<&Value> {
    if let Some(value) = doc.get("session") {
        return Some(value);
    }
    for path in [
        "info.session",
        "sessionInfo.session",
        "currentSession",
        "sessionId",
        "session._id",
    ] {
        if let Some(value) = lookup_path(doc, path) {
            return Some(value);
        }
    }
    None
}

fn lookup_path<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = root;
    for part in path.split('.') {
        current = current.get(part)?;
    }
    Some(current)
}

fn normalize_session_param(value: &Value) -> Value {
    if let Some(oid) = value
        .as_object()
        .and_then(|map| map.get("$value"))
        .and_then(Value::as_str)
    {
        return oid_value(oid);
    }

    if let Some(raw) = value.as_str() {
        let candidate = raw.trim();
        if candidate.len() == 24 && candidate.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return oid_value(candidate);
        }
        return Value::String(candidate.to_string());
    }

    value.clone()
}

fn emit_snapshot(tx: &Sender<TimingMessage>, source_id: u64, store: &CollectionStore) {
    let Some((header, entries)) = snapshot_from_store(store) else {
        return;
    };

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

fn maybe_emit_collection_stats(
    tx: &Sender<TimingMessage>,
    source_id: u64,
    store: &CollectionStore,
    enabled: bool,
    last_emit: &mut Instant,
) {
    if !enabled || last_emit.elapsed() < COLLECTION_STATS_INTERVAL {
        return;
    }

    *last_emit = Instant::now();
    let _ = tx.send(TimingMessage::Status {
        source_id,
        text: format!("WEC collections: {}", store.collection_count_summary()),
    });
}

fn env_flag(key: &str, default: bool) -> bool {
    match std::env::var(key) {
        Ok(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => default,
    }
}
