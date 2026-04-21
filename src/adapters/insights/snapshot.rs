use std::{
    collections::hash_map::DefaultHasher,
    hash::Hasher,
    path::PathBuf,
    sync::mpsc::Sender,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::{
    snapshot_runtime::{base_snapshot_fingerprint, hash_entry_common_fields},
    timing::{TimingEntry, TimingHeader, TimingMessage},
    timing_persist::{
        data_local_snapshot_path, log_series_debug, read_json, write_json_pretty, PersistState,
        SeriesDebugOutput,
    },
};

#[derive(Debug, Clone)]
pub(crate) struct Snapshot<Extra = ()> {
    pub(crate) header: TimingHeader,
    pub(crate) entries: Vec<TimingEntry>,
    pub(crate) session_id: Option<String>,
    pub(crate) fingerprint: u64,
    pub(crate) extra: Extra,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PersistedSnapshot<Extra = ()> {
    pub(crate) saved_unix_ms: u64,
    pub(crate) session_id: Option<String>,
    pub(crate) meaningful_fingerprint: u64,
    pub(crate) header: TimingHeader,
    pub(crate) entries: Vec<TimingEntry>,
    #[serde(default)]
    pub(crate) extra: Extra,
}

pub(crate) fn snapshot_path(file_name: &str) -> Option<PathBuf> {
    data_local_snapshot_path(file_name)
}

pub(crate) fn meaningful_snapshot_fingerprint(
    header: &TimingHeader,
    entries: &[TimingEntry],
) -> u64 {
    meaningful_snapshot_fingerprint_with_extra(header, entries, |_hasher, _entry| {})
}

pub(crate) fn meaningful_snapshot_fingerprint_with_extra(
    header: &TimingHeader,
    entries: &[TimingEntry],
    mut hash_extra: impl FnMut(&mut DefaultHasher, &TimingEntry),
) -> u64 {
    let mut hasher = base_snapshot_fingerprint(header);
    for entry in entries {
        hash_entry_common_fields(&mut hasher, entry);
        hash_extra(&mut hasher, entry);
    }
    hasher.finish()
}

pub(crate) fn persist_snapshot<Extra: Clone + Serialize>(
    runtime: &mut PersistState,
    snapshot: &Snapshot<Extra>,
    saved_unix_ms: u64,
    series_name: &str,
    debug: &SeriesDebugOutput,
) {
    let Some(path) = runtime.path.as_ref() else {
        return;
    };

    let payload = PersistedSnapshot {
        saved_unix_ms,
        session_id: snapshot.session_id.clone(),
        meaningful_fingerprint: snapshot.fingerprint,
        header: snapshot.header.clone(),
        entries: snapshot.entries.clone(),
        extra: snapshot.extra.clone(),
    };

    if let Err(err) = write_json_pretty(path, &payload) {
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
}

pub(crate) fn persist_snapshot_if_dirty<Extra: Clone + Serialize>(
    runtime: &mut PersistState,
    snapshot: &Snapshot<Extra>,
    saved_unix_ms: u64,
    series_name: &str,
    debug: &SeriesDebugOutput,
) {
    if !runtime.dirty_since_last_save {
        return;
    }
    persist_snapshot(runtime, snapshot, saved_unix_ms, series_name, debug);
}

pub(crate) fn restore_snapshot_from_disk<Extra: Clone + Default + DeserializeOwned>(
    runtime: &mut PersistState,
    tx: &Sender<TimingMessage>,
    source_id: u64,
    series_name: &str,
    debug: &SeriesDebugOutput,
) -> Option<Snapshot<Extra>> {
    let path = runtime.path.as_ref()?;
    let saved = read_json::<PersistedSnapshot<Extra>>(path)?;
    let snapshot = Snapshot {
        header: saved.header,
        entries: saved.entries,
        session_id: saved.session_id,
        fingerprint: saved.meaningful_fingerprint,
        extra: saved.extra,
    };

    runtime.last_persisted_hash = Some(snapshot.fingerprint);
    runtime.last_save_at = Some(SystemTime::now());
    runtime.dirty_since_last_save = false;

    let _ = tx.send(TimingMessage::Snapshot {
        source_id,
        header: snapshot.header.clone(),
        entries: snapshot.entries.clone(),
    });

    log_series_debug(
        debug,
        series_name,
        format!("snapshot restored from {}", path.display()),
    );

    Some(snapshot)
}

pub(crate) fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}
