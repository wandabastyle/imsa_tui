use std::{hash::Hash, path::PathBuf};

use crate::{
    adapters::insights::snapshot as shared_snapshot,
    timing::TimingMessage,
    timing_persist::{data_local_snapshot_path, read_json},
};

use super::{
    is_parsed_entry_transponder_placeholder, log_debug, now_millis, ImsaRuntimeState, ImsaSnapshot,
    ImsaSnapshotExtra,
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
    let path_display = path.display().to_string();

    let shared = shared_snapshot::Snapshot {
        header: snapshot.header.clone(),
        entries: snapshot.entries.clone(),
        session_id: snapshot.session_id.clone(),
        fingerprint: snapshot.fingerprint,
        extra: ImsaSnapshotExtra {
            raw_results_payload: snapshot.raw_results_payload.clone(),
            raw_race_data_payload: snapshot.raw_race_data_payload.clone(),
        },
    };

    shared_snapshot::persist_snapshot(
        &mut runtime.persist,
        &shared,
        now_unix_ms(),
        "IMSA",
        &runtime.debug_output,
    );
    log_debug(
        &runtime.debug_output,
        format!("snapshot persisted ({reason}) to {path_display}"),
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

    let Some(saved) = read_json::<shared_snapshot::PersistedSnapshot<ImsaSnapshotExtra>>(path)
    else {
        return;
    };

    runtime.persist.last_persisted_hash = Some(saved.meaningful_fingerprint);
    runtime.persist.last_save_at = Some(std::time::SystemTime::now());
    runtime.persist.dirty_since_last_save = false;

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
        raw_results_payload: saved.extra.raw_results_payload,
        raw_race_data_payload: saved.extra.raw_race_data_payload,
    };

    runtime.last_session_id = snapshot.session_id.clone();
    runtime.last_good_live_snapshot = Some(snapshot.clone());

    let _ = tx.send(TimingMessage::Snapshot {
        source_id,
        header: snapshot.header.clone(),
        entries: snapshot.entries.clone(),
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
    shared_snapshot::meaningful_snapshot_fingerprint_with_extra(header, entries, |hasher, entry| {
        entry.gap_class.trim().hash(hasher);
        entry.gap_next_in_class.trim().hash(hasher);
        entry.best_lap_no.trim().hash(hasher);
        entry.pit.trim().hash(hasher);
        entry.pit_stops.trim().hash(hasher);
        entry
            .fastest_driver
            .trim()
            .to_ascii_lowercase()
            .hash(hasher);
    })
}

pub(super) fn now_unix_ms() -> u64 {
    now_millis() as u64
}
