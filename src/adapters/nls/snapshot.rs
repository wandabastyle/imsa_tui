use std::{hash::Hash, sync::mpsc::Sender, time::SystemTime};

use crate::{
    adapters::insights::snapshot as shared_snapshot,
    snapshot_runtime::derive_session_identifier,
    timing::{TimingEntry, TimingHeader, TimingMessage},
    timing_persist::{read_json, PersistState, SeriesDebugOutput},
};

pub(super) type NlsSnapshot = shared_snapshot::Snapshot;

pub(super) fn derive_session_id(header: &TimingHeader) -> Option<String> {
    derive_session_identifier(header)
}

pub(super) fn nls_snapshot_path() -> Option<std::path::PathBuf> {
    shared_snapshot::snapshot_path("nls_snapshot.json")
}

pub(super) fn meaningful_snapshot_fingerprint(
    header: &TimingHeader,
    entries: &[TimingEntry],
) -> u64 {
    shared_snapshot::meaningful_snapshot_fingerprint_with_extra(header, entries, |hasher, entry| {
        entry.sector_4.trim().hash(hasher);
        entry.sector_5.trim().hash(hasher);
    })
}

pub(super) fn persist_snapshot(
    runtime: &mut PersistState,
    snapshot: &NlsSnapshot,
    saved_unix_ms: u64,
    debug: &SeriesDebugOutput,
) {
    shared_snapshot::persist_snapshot(runtime, snapshot, saved_unix_ms, "NLS", debug);
}

pub(super) fn persist_snapshot_if_dirty(
    runtime: &mut PersistState,
    snapshot: &NlsSnapshot,
    saved_unix_ms: u64,
    debug: &SeriesDebugOutput,
) {
    shared_snapshot::persist_snapshot_if_dirty(runtime, snapshot, saved_unix_ms, "NLS", debug);
}

/// Checks if time_to_go matches near-zero patterns ("00:00:01", "00:00:00", "0:00", "0:00:00", or "0")
fn is_near_zero_time_to_go(value: &str) -> bool {
    let trimmed = value.trim();
    matches!(trimmed, "0" | "0:00" | "00:00" | "00:00:00" | "00:00:01")
}

/// Sanitizes the header for event 50 (24h) race sessions with stale near-zero time_to_go values.
/// Returns true if sanitization was applied.
fn sanitize_event50_race_header(header: &mut TimingHeader) -> bool {
    // Check if this is event 50 (24h) and a race session
    if header.event_id != "50" || header.session_type_raw != "R" {
        return false;
    }

    // Check if time_to_go is near-zero (stale value from saved snapshot)
    if !is_near_zero_time_to_go(&header.time_to_go) {
        return false;
    }

    // Sanitize: set time_to_go to exactly zero
    header.time_to_go = "00:00:00".to_string();

    // If flag is Green, promote to Checkered since race is over
    if header.flag.eq_ignore_ascii_case("green") {
        header.flag = "Checkered".to_string();
    }

    true
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
    let saved = read_json::<shared_snapshot::PersistedSnapshot<()>>(path)?;

    // Update persist state
    runtime.last_persisted_hash = Some(saved.meaningful_fingerprint);
    runtime.last_save_at = Some(SystemTime::now());
    runtime.dirty_since_last_save = false;

    // Restore header and entries
    *header = saved.header;
    *entries = saved.entries.clone();

    // Sanitize restored snapshot for event 50 race sessions with stale near-zero time_to_go
    let was_sanitized = sanitize_event50_race_header(header);

    // Send the (potentially sanitized) snapshot
    let _ = tx.send(TimingMessage::Snapshot {
        source_id,
        header: header.clone(),
        entries: entries.clone(),
    });

    if was_sanitized {
        crate::timing_persist::log_series_debug(
            debug,
            "NLS",
            format!(
                "snapshot restored and sanitized event 50 race from {} (time_to_go: {}, flag: {})",
                path.display(),
                header.time_to_go,
                header.flag
            ),
        );
    } else {
        crate::timing_persist::log_series_debug(
            debug,
            "NLS",
            format!("snapshot restored from {}", path.display()),
        );
    }

    saved.session_id
}
