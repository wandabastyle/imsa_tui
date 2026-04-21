use std::{hash::Hash, sync::mpsc::Sender};

use crate::{
    adapters::insights::snapshot as shared_snapshot,
    snapshot_runtime::derive_session_identifier,
    timing::{TimingEntry, TimingHeader, TimingMessage},
    timing_persist::{PersistState, SeriesDebugOutput},
};

pub(super) type DhlmSnapshot = shared_snapshot::Snapshot;

pub(super) fn dhlm_snapshot_path() -> Option<std::path::PathBuf> {
    shared_snapshot::snapshot_path("dhlm_snapshot.json")
}

pub(super) fn derive_session_id(header: &TimingHeader) -> Option<String> {
    derive_session_identifier(header)
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
    snapshot: &DhlmSnapshot,
    saved_unix_ms: u64,
    debug: &SeriesDebugOutput,
) {
    shared_snapshot::persist_snapshot(runtime, snapshot, saved_unix_ms, "DHLM", debug);
}

pub(super) fn persist_snapshot_if_dirty(
    runtime: &mut PersistState,
    snapshot: &DhlmSnapshot,
    saved_unix_ms: u64,
    debug: &SeriesDebugOutput,
) {
    shared_snapshot::persist_snapshot_if_dirty(runtime, snapshot, saved_unix_ms, "DHLM", debug);
}

pub(super) fn restore_snapshot_from_disk(
    runtime: &mut PersistState,
    header: &mut TimingHeader,
    entries: &mut Vec<TimingEntry>,
    tx: &Sender<TimingMessage>,
    source_id: u64,
    debug: &SeriesDebugOutput,
) -> Option<String> {
    let saved =
        shared_snapshot::restore_snapshot_from_disk::<()>(runtime, tx, source_id, "DHLM", debug)?;
    *header = saved.header;
    *entries = saved.entries;
    saved.session_id
}
