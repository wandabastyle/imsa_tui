use std::{
    hash::{Hash, Hasher},
    path::PathBuf,
    sync::mpsc::Sender,
    time::SystemTime,
};

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
pub(super) struct NlsSnapshot {
    pub(super) header: TimingHeader,
    pub(super) entries: Vec<TimingEntry>,
    pub(super) session_id: Option<String>,
    pub(super) fingerprint: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedNlsSnapshot {
    saved_unix_ms: u64,
    session_id: Option<String>,
    meaningful_fingerprint: u64,
    header: TimingHeader,
    entries: Vec<TimingEntry>,
}

pub(super) fn derive_session_id(header: &TimingHeader) -> Option<String> {
    derive_session_identifier(header)
}

pub(super) fn nls_snapshot_path() -> Option<PathBuf> {
    data_local_snapshot_path("nls_snapshot.json")
}

pub(super) fn meaningful_snapshot_fingerprint(
    header: &TimingHeader,
    entries: &[TimingEntry],
) -> u64 {
    let mut hasher = base_snapshot_fingerprint(header);

    for entry in entries {
        hash_entry_common_fields(&mut hasher, entry);
        entry.sector_4.trim().hash(&mut hasher);
        entry.sector_5.trim().hash(&mut hasher);
    }

    hasher.finish()
}

pub(super) fn persist_snapshot(
    runtime: &mut PersistState,
    snapshot: &NlsSnapshot,
    saved_unix_ms: u64,
    debug: &SeriesDebugOutput,
) {
    let Some(path) = runtime.path.as_ref() else {
        return;
    };

    let payload = PersistedNlsSnapshot {
        saved_unix_ms,
        session_id: snapshot.session_id.clone(),
        meaningful_fingerprint: snapshot.fingerprint,
        header: snapshot.header.clone(),
        entries: snapshot.entries.clone(),
    };

    if let Err(err) = write_json_pretty(path, &payload) {
        log_series_debug(debug, "NLS", format!("snapshot persist failed: {err}"));
        return;
    }

    runtime.last_persisted_hash = Some(snapshot.fingerprint);
    runtime.last_save_at = Some(SystemTime::now());
    runtime.dirty_since_last_save = false;
    log_series_debug(
        debug,
        "NLS",
        format!("snapshot persisted to {}", path.display()),
    );
}

pub(super) fn persist_snapshot_if_dirty(
    runtime: &mut PersistState,
    snapshot: &NlsSnapshot,
    saved_unix_ms: u64,
    debug: &SeriesDebugOutput,
) {
    if !runtime.dirty_since_last_save {
        return;
    }
    persist_snapshot(runtime, snapshot, saved_unix_ms, debug);
}

pub(super) fn restore_snapshot_from_disk(
    runtime: &mut PersistState,
    header: &mut TimingHeader,
    entries: &mut Vec<TimingEntry>,
    tx: &Sender<TimingMessage>,
    source_id: u64,
    debug: &SeriesDebugOutput,
) -> Option<String> {
    let path = runtime.path.as_ref()?;
    let saved = read_json::<PersistedNlsSnapshot>(path)?;

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
        "NLS",
        format!("snapshot restored from {}", path.display()),
    );

    saved.session_id
}
