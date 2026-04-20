use std::{hash::Hasher, path::PathBuf, sync::mpsc::Sender, time::SystemTime};

use serde::{Deserialize, Serialize};

use crate::{
    snapshot_runtime::{
        base_snapshot_fingerprint, derive_session_identifier, hash_entry_common_fields,
    },
    timing::{TimingEntry, TimingHeader, TimingMessage},
    timing_persist::{
        data_local_snapshot_path, log_series_debug, read_json, write_json_pretty, PersistState,
        SeriesDebugOutput,
    },
};

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub header: TimingHeader,
    pub entries: Vec<TimingEntry>,
    pub session_id: Option<String>,
    pub fingerprint: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSnapshot {
    pub saved_unix_ms: u64,
    pub session_id: Option<String>,
    pub meaningful_fingerprint: u64,
    pub header: TimingHeader,
    pub entries: Vec<TimingEntry>,
}

pub fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn meaningful_snapshot_fingerprint(header: &TimingHeader, entries: &[TimingEntry]) -> u64 {
    let mut hasher = base_snapshot_fingerprint(header);
    for entry in entries {
        hash_entry_common_fields(&mut hasher, entry);
    }
    hasher.finish()
}

pub fn snapshot_path(file_name: &str) -> Option<PathBuf> {
    data_local_snapshot_path(file_name)
}

pub fn derive_session_id(header: &TimingHeader) -> Option<String> {
    derive_session_identifier(header)
}

pub fn persist_snapshot(
    runtime: &mut PersistState,
    snapshot: &Snapshot,
    file_name: &str,
    series_name: &str,
    debug: &SeriesDebugOutput,
) {
    let Some(path) = runtime.path.clone().or_else(|| snapshot_path(file_name)) else {
        return;
    };

    let payload = PersistedSnapshot {
        saved_unix_ms: now_unix_ms(),
        session_id: snapshot.session_id.clone(),
        meaningful_fingerprint: snapshot.fingerprint,
        header: snapshot.header.clone(),
        entries: snapshot.entries.clone(),
    };

    if let Err(err) = write_json_pretty(&path, &payload) {
        log_series_debug(
            debug,
            series_name,
            format!("snapshot persist failed: {err}"),
        );
        return;
    }

    runtime.last_persisted_hash = Some(snapshot.fingerprint);
    runtime.last_save_at = Some(SystemTime::now());
    runtime.dirty_since_last_save = false;
    log_series_debug(
        debug,
        series_name,
        format!("snapshot persisted to {}", path.display()),
    );
}

pub fn persist_snapshot_if_dirty(
    runtime: &mut PersistState,
    snapshot: &Snapshot,
    file_name: &str,
    series_name: &str,
    debug: &SeriesDebugOutput,
) {
    if !runtime.dirty_since_last_save {
        return;
    }
    persist_snapshot(runtime, snapshot, file_name, series_name, debug);
}

pub fn restore_snapshot_from_disk(
    runtime: &mut PersistState,
    header: &mut TimingHeader,
    entries: &mut Vec<TimingEntry>,
    tx: &Sender<TimingMessage>,
    source_id: u64,
    file_name: &str,
    series_name: &str,
    debug: &SeriesDebugOutput,
) -> Option<String> {
    let path = runtime.path.clone().or_else(|| snapshot_path(file_name))?;
    let saved = read_json::<PersistedSnapshot>(&path)?;

    *header = saved.header;
    *entries = saved.entries;
    runtime.last_persisted_hash = Some(saved.meaningful_fingerprint);
    runtime.last_save_at = Some(SystemTime::now());

    let _ = tx.send(TimingMessage::Snapshot {
        source_id,
        header: header.clone(),
        entries: entries.clone(),
    });

    log_series_debug(
        debug,
        series_name,
        format!("snapshot restored from {}", path.display()),
    );

    saved.session_id
}
