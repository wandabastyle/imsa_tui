use std::{
    hash::{Hash, Hasher},
    path::PathBuf,
    time::SystemTime,
};

use crate::{
    snapshot_runtime::base_snapshot_fingerprint,
    timing::TimingMessage,
    timing_persist::{data_local_snapshot_path, read_json, write_json_pretty},
};

use super::{
    is_parsed_entry_transponder_placeholder, log_debug, now_millis, ImsaRuntimeState, ImsaSnapshot,
    PersistedImsaSnapshot,
};

pub(super) fn persist_snapshot_if_dirty(runtime: &mut ImsaRuntimeState, reason: &str) {
    if !runtime.persist.dirty_since_last_save {
        return;
    }

    let snapshot = runtime.last_good_live_snapshot.clone();
    if let Some(snapshot) = snapshot.as_ref() {
        persist_snapshot(runtime, snapshot, reason);
    }
}

pub(super) fn flush_dirty_snapshot_on_shutdown(runtime: &mut ImsaRuntimeState) {
    persist_snapshot_if_dirty(runtime, "shutdown flush");
}

pub(super) fn persist_snapshot(
    runtime: &mut ImsaRuntimeState,
    snapshot: &ImsaSnapshot,
    reason: &str,
) {
    let Some(path) = runtime.persist.path.as_ref() else {
        return;
    };

    let payload = PersistedImsaSnapshot {
        saved_unix_ms: now_unix_ms(),
        session_id: snapshot.session_id.clone(),
        meaningful_fingerprint: snapshot.fingerprint,
        header: snapshot.header.clone(),
        entries: snapshot.entries.clone(),
        raw_results_payload: snapshot.raw_results_payload.clone(),
        raw_race_data_payload: snapshot.raw_race_data_payload.clone(),
    };

    if let Err(err) = write_json_pretty(path, &payload) {
        log_debug(
            &runtime.debug_output,
            format!("snapshot persist failed: {err}"),
        );
        return;
    }

    runtime.persist.last_persisted_hash = Some(snapshot.fingerprint);
    runtime.persist.last_save_at = Some(SystemTime::now());
    runtime.persist.dirty_since_last_save = false;
    log_debug(
        &runtime.debug_output,
        format!("snapshot persisted ({reason}) to {}", path.display()),
    );
}

pub(super) fn restore_snapshot_from_disk(
    runtime: &mut ImsaRuntimeState,
    tx: &std::sync::mpsc::Sender<TimingMessage>,
    source_id: u64,
) {
    let Some(path) = runtime.persist.path.as_ref() else {
        return;
    };

    let Some(saved) = read_json::<PersistedImsaSnapshot>(path) else {
        return;
    };

    let entries: Vec<_> = saved
        .entries
        .into_iter()
        .filter(|entry| !is_parsed_entry_transponder_placeholder(entry))
        .collect();

    let snapshot = ImsaSnapshot {
        header: saved.header,
        entries,
        session_id: saved.session_id,
        fingerprint: saved.meaningful_fingerprint,
        raw_results_payload: saved.raw_results_payload,
        raw_race_data_payload: saved.raw_race_data_payload,
    };

    runtime.persist.last_persisted_hash = Some(snapshot.fingerprint);
    runtime.persist.last_save_at = Some(SystemTime::now());
    runtime.persist.dirty_since_last_save = false;
    runtime.last_session_id = snapshot.session_id.clone();
    runtime.last_good_live_snapshot = Some(snapshot.clone());

    let _ = tx.send(TimingMessage::Snapshot {
        source_id,
        header: snapshot.header,
        entries: snapshot.entries,
    });
    log_debug(
        &runtime.debug_output,
        format!("snapshot restored from {}", path.display()),
    );
}

pub(super) fn imsa_snapshot_path() -> Option<PathBuf> {
    data_local_snapshot_path("imsa_snapshot.json")
}

pub(super) fn meaningful_snapshot_fingerprint(
    header: &crate::timing::TimingHeader,
    entries: &[crate::timing::TimingEntry],
) -> u64 {
    let mut hasher = base_snapshot_fingerprint(header);
    for entry in entries {
        entry.position.hash(&mut hasher);
        entry.car_number.trim().hash(&mut hasher);
        entry
            .class_name
            .trim()
            .to_ascii_lowercase()
            .hash(&mut hasher);
        entry.class_rank.trim().hash(&mut hasher);
        entry.driver.trim().to_ascii_lowercase().hash(&mut hasher);
        entry.vehicle.trim().to_ascii_lowercase().hash(&mut hasher);
        entry.team.trim().to_ascii_lowercase().hash(&mut hasher);
        entry.laps.trim().hash(&mut hasher);
        entry.gap_overall.trim().hash(&mut hasher);
        entry.gap_class.trim().hash(&mut hasher);
        entry.gap_next_in_class.trim().hash(&mut hasher);
        entry.last_lap.trim().hash(&mut hasher);
        entry.best_lap.trim().hash(&mut hasher);
        entry.sector_1.trim().hash(&mut hasher);
        entry.sector_2.trim().hash(&mut hasher);
        entry.sector_3.trim().hash(&mut hasher);
        entry.best_lap_no.trim().hash(&mut hasher);
        entry.pit.trim().hash(&mut hasher);
        entry.pit_stops.trim().hash(&mut hasher);
        entry
            .fastest_driver
            .trim()
            .to_ascii_lowercase()
            .hash(&mut hasher);
        entry.stable_id.trim().hash(&mut hasher);
    }
    hasher.finish()
}

pub(super) fn now_unix_ms() -> u64 {
    now_millis() as u64
}
