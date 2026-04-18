use std::{
    hash::{Hash, Hasher},
    path::PathBuf,
    sync::mpsc::Sender,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{
    snapshot_runtime::base_snapshot_fingerprint,
    timing::{TimingEntry, TimingHeader, TimingMessage},
    timing_persist::{
        data_local_snapshot_path, log_series_debug, read_json, write_json_pretty, PersistState,
        SeriesDebugOutput,
    },
};

#[derive(Debug, Clone)]
pub(super) struct F1Snapshot {
    pub(super) header: TimingHeader,
    pub(super) entries: Vec<TimingEntry>,
    pub(super) session_id: Option<String>,
    pub(super) fingerprint: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedF1Snapshot {
    saved_unix_ms: u64,
    session_id: Option<String>,
    meaningful_fingerprint: u64,
    header: TimingHeader,
    entries: Vec<TimingEntry>,
}

pub(super) fn f1_snapshot_path() -> Option<PathBuf> {
    data_local_snapshot_path("f1_snapshot.json")
}

pub(super) fn meaningful_snapshot_fingerprint(
    header: &TimingHeader,
    entries: &[TimingEntry],
) -> u64 {
    let mut hasher = base_snapshot_fingerprint(header);
    for entry in entries {
        entry.position.hash(&mut hasher);
        entry.car_number.trim().hash(&mut hasher);
        entry.driver.trim().to_ascii_lowercase().hash(&mut hasher);
        entry.vehicle.trim().to_ascii_lowercase().hash(&mut hasher);
        entry.laps.trim().hash(&mut hasher);
        entry.gap_overall.trim().hash(&mut hasher);
        entry.gap_next_in_class.trim().hash(&mut hasher);
        entry.last_lap.trim().hash(&mut hasher);
        entry.best_lap.trim().hash(&mut hasher);
        entry.pit.trim().to_ascii_lowercase().hash(&mut hasher);
        entry.pit_stops.trim().hash(&mut hasher);
        entry.stable_id.trim().hash(&mut hasher);
    }
    hasher.finish()
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub(super) fn persist_snapshot(
    runtime: &mut PersistState,
    snapshot: &F1Snapshot,
    debug: &SeriesDebugOutput,
) {
    let Some(path) = runtime.path.as_ref() else {
        return;
    };

    let payload = PersistedF1Snapshot {
        saved_unix_ms: now_unix_ms(),
        session_id: snapshot.session_id.clone(),
        meaningful_fingerprint: snapshot.fingerprint,
        header: snapshot.header.clone(),
        entries: snapshot.entries.clone(),
    };

    if let Err(err) = write_json_pretty(path, &payload) {
        log_series_debug(debug, "F1", format!("snapshot persist failed: {err}"));
        return;
    }

    runtime.last_persisted_hash = Some(snapshot.fingerprint);
    runtime.last_save_at = Some(SystemTime::now());
    runtime.dirty_since_last_save = false;
    log_series_debug(
        debug,
        "F1",
        format!("snapshot persisted to {}", path.display()),
    );
}

pub(super) fn restore_snapshot_from_disk(
    runtime: &mut PersistState,
    tx: &Sender<TimingMessage>,
    source_id: u64,
    debug: &SeriesDebugOutput,
) -> Option<F1Snapshot> {
    let path = runtime.path.as_ref()?;
    let saved = read_json::<PersistedF1Snapshot>(path)?;

    let snapshot = F1Snapshot {
        header: saved.header,
        entries: saved.entries,
        session_id: saved.session_id,
        fingerprint: saved.meaningful_fingerprint,
    };

    runtime.last_persisted_hash = Some(snapshot.fingerprint);
    runtime.last_save_at = Some(SystemTime::now());

    let _ = tx.send(TimingMessage::Snapshot {
        source_id,
        header: snapshot.header.clone(),
        entries: snapshot.entries.clone(),
    });

    log_series_debug(
        debug,
        "F1",
        format!("snapshot restored from {}", path.display()),
    );

    Some(snapshot)
}
