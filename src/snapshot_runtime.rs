use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;

use crate::timing::{TimingEntry, TimingHeader};

pub fn derive_session_identifier(header: &TimingHeader) -> Option<String> {
    let event = header.event_name.trim();
    let session = header.session_name.trim();
    let track = header.track_name.trim();
    if [event, session, track]
        .iter()
        .all(|value| value.is_empty() || *value == "-")
    {
        return None;
    }
    Some(format!("{event}|{session}|{track}").to_ascii_lowercase())
}

pub fn base_snapshot_fingerprint(header: &TimingHeader) -> DefaultHasher {
    let mut hasher = DefaultHasher::new();
    header
        .event_name
        .trim()
        .to_ascii_lowercase()
        .hash(&mut hasher);
    header
        .session_name
        .trim()
        .to_ascii_lowercase()
        .hash(&mut hasher);
    header
        .track_name
        .trim()
        .to_ascii_lowercase()
        .hash(&mut hasher);
    header.flag.trim().to_ascii_lowercase().hash(&mut hasher);
    header.time_to_go.trim().hash(&mut hasher);
    hasher
}

pub fn hash_entry_common_fields(hasher: &mut DefaultHasher, entry: &TimingEntry) {
    entry.position.hash(hasher);
    entry.car_number.trim().hash(hasher);
    entry.class_name.trim().to_ascii_lowercase().hash(hasher);
    entry.class_rank.trim().hash(hasher);
    entry.driver.trim().to_ascii_lowercase().hash(hasher);
    entry.vehicle.trim().to_ascii_lowercase().hash(hasher);
    entry.team.trim().to_ascii_lowercase().hash(hasher);
    entry.laps.trim().hash(hasher);
    entry.gap_overall.trim().hash(hasher);
    entry.last_lap.trim().hash(hasher);
    entry.best_lap.trim().hash(hasher);
    entry.sector_1.trim().hash(hasher);
    entry.sector_2.trim().hash(hasher);
    entry.sector_3.trim().hash(hasher);
    entry.stable_id.trim().hash(hasher);
}
