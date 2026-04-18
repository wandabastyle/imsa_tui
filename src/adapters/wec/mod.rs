mod ddp;
mod model;
mod normalize;
mod store;
mod transport;

use std::{
    collections::HashSet,
    hash::Hasher,
    path::PathBuf,
    sync::mpsc::{Receiver, Sender},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    snapshot_runtime::{
        base_snapshot_fingerprint, derive_session_identifier, hash_entry_common_fields,
    },
    timing::{TimingEntry, TimingHeader, TimingMessage},
    timing_persist::{
        data_local_snapshot_path, debounce_elapsed, log_series_debug, read_json, write_json_pretty,
        PersistState, SeriesDebugOutput,
    },
};

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
const SNAPSHOT_SAVE_DEBOUNCE: Duration = Duration::from_secs(180);

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

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_millis() as u64
}

fn wec_snapshot_path() -> Option<PathBuf> {
    data_local_snapshot_path("wec_snapshot.json")
}

fn meaningful_snapshot_fingerprint(header: &TimingHeader, entries: &[TimingEntry]) -> u64 {
    let mut hasher = base_snapshot_fingerprint(header);
    for entry in entries {
        hash_entry_common_fields(&mut hasher, entry);
    }
    hasher.finish()
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
    log_series_debug(
        debug,
        "WEC",
        format!("snapshot persisted to {}", path.display()),
    );
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

/// Reverse engineered public LT2 flow (first pass):
/// 1) SockJS websocket open
/// 2) DDP connect
/// 3) subscribe(livetimingFeed, ["fiawec"])
/// 4) discover sessions -> subscribe(sessionInfo, [sessions])
/// 5) discover current session oid -> subscribe session-scoped publications
///
/// TODO: tighten session discovery to exact collection once enough live captures are archived.
pub fn websocket_worker(tx: Sender<TimingMessage>, source_id: u64, stop_rx: Receiver<()>) {
    websocket_worker_with_debug(tx, source_id, stop_rx, SeriesDebugOutput::Silent)
}

pub fn websocket_worker_with_debug(
    tx: Sender<TimingMessage>,
    source_id: u64,
    stop_rx: Receiver<()>,
    debug_output: SeriesDebugOutput,
) {
    let debug_unknown = env_flag("WEC_DEBUG_UNKNOWN", false);
    let debug_subscriptions = env_flag("WEC_DEBUG_SUBS", false);
    let emit_collection_stats = env_flag("WEC_COLLECTION_COUNTS", false);
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
        .and_then(|snap| snap.session_id.clone());

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
            text: "Connecting to WEC websocket...".to_string(),
        });
        log_series_debug(&debug_output, "WEC", "connecting websocket");

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
                if let Some(snapshot) = last_snapshot.as_ref() {
                    if persist.dirty_since_last_save {
                        persist_snapshot(&mut persist, snapshot, &debug_output);
                    }
                }
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
                                log_series_debug(&debug_output, "WEC", "DDP connected");
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
                                emit_snapshot(
                                    &tx,
                                    source_id,
                                    &store,
                                    &mut persist,
                                    &mut last_snapshot,
                                    &mut last_session_id,
                                    &debug_output,
                                );
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
                                emit_snapshot(
                                    &tx,
                                    source_id,
                                    &store,
                                    &mut persist,
                                    &mut last_snapshot,
                                    &mut last_session_id,
                                    &debug_output,
                                );
                            }
                            DdpIncoming::Removed { collection, id } => {
                                store.apply_removed(&collection, &id);
                                emit_snapshot(
                                    &tx,
                                    source_id,
                                    &store,
                                    &mut persist,
                                    &mut last_snapshot,
                                    &mut last_session_id,
                                    &debug_output,
                                );
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
        log_series_debug(&debug_output, "WEC", "reconnecting in 4s");
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

fn emit_snapshot(
    tx: &Sender<TimingMessage>,
    source_id: u64,
    store: &CollectionStore,
    persist: &mut PersistState,
    last_snapshot: &mut Option<WecSnapshot>,
    last_session_id: &mut Option<String>,
    debug_output: &SeriesDebugOutput,
) {
    let Some((header, entries)) = snapshot_from_store(store) else {
        return;
    };

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
