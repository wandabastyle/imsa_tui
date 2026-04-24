// In-memory web app state:
// latest snapshot per series, per-profile preferences, and broadcast channels for SSE fanout.

use std::{
    collections::hash_map::DefaultHasher,
    collections::HashMap,
    hash::{Hash, Hasher},
    sync::{Arc, RwLock},
    time::{SystemTime, UNIX_EPOCH},
};

use tokio::sync::broadcast;
use web_shared::{DemoStateResponse, SnapshotResponse};

use crate::timing::{Series, TimingEntry, TimingHeader, TimingMessage};

use crate::demo;
use crate::favourites;

use super::bridge::FeedController;
use super::prefs::{load_preferences, reset_preferences, save_preferences, Preferences};

#[derive(Debug, Clone, Default)]
pub struct SeriesSnapshot {
    pub header: TimingHeader,
    pub entries: Vec<TimingEntry>,
    pub status: String,
    pub last_error: Option<String>,
    pub last_update_unix_ms: Option<u64>,
}

#[derive(Clone)]
pub struct WebAppState {
    snapshots: Arc<RwLock<HashMap<Series, SeriesSnapshot>>>,
    preferences: Arc<RwLock<HashMap<String, Preferences>>>,
    session_demo: Arc<RwLock<HashMap<String, SessionDemoState>>>,
    feed_controller: Arc<RwLock<Option<FeedController>>>,
    profile_cookie_secure: bool,
    streams: Arc<HashMap<Series, broadcast::Sender<()>>>,
}

pub struct LiveSeriesGuard {
    controller: Option<FeedController>,
    series: Series,
}

impl Drop for LiveSeriesGuard {
    fn drop(&mut self) {
        if let Some(controller) = self.controller.as_ref() {
            controller.unregister_client(self.series);
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct SessionDemoState {
    enabled: bool,
    seed: u64,
    started_unix_ms: u64,
    last_seen_unix_ms: u64,
}

impl WebAppState {
    pub fn new() -> Self {
        Self::with_profile_cookie_secure(false)
    }

    pub fn with_profile_cookie_secure(profile_cookie_secure: bool) -> Self {
        let mut snapshots = HashMap::new();
        for series in Series::all() {
            snapshots.insert(
                series,
                SeriesSnapshot {
                    status: format!("Starting {} live timing...", series.label()),
                    ..SeriesSnapshot::default()
                },
            );
        }

        let mut streams = HashMap::new();
        for series in Series::all() {
            let (tx, _) = broadcast::channel(64);
            streams.insert(series, tx);
        }

        Self {
            snapshots: Arc::new(RwLock::new(snapshots)),
            preferences: Arc::new(RwLock::new(HashMap::new())),
            session_demo: Arc::new(RwLock::new(HashMap::new())),
            feed_controller: Arc::new(RwLock::new(None)),
            profile_cookie_secure,
            streams: Arc::new(streams),
        }
    }

    pub fn snapshot_for(&self, series: Series) -> Option<SeriesSnapshot> {
        self.snapshots.read().ok()?.get(&series).cloned()
    }

    pub fn snapshot_response_for(&self, series: Series) -> Option<SnapshotResponse> {
        self.snapshot_for(series).map(|snapshot| SnapshotResponse {
            series: to_api_series(series),
            snapshot: to_api_series_snapshot(snapshot),
        })
    }

    pub fn apply_timing_message(&self, series: Series, message: &TimingMessage) {
        let mut guard = match self.snapshots.write() {
            Ok(g) => g,
            Err(_) => return,
        };

        let Some(snapshot) = guard.get_mut(&series) else {
            return;
        };

        // This mirrors the TUI update semantics: status updates frequently, while
        // successful snapshots clear previous errors and refresh age.
        // Special case for IMSA: suppress the transient "Fetching IMSA live timing..."
        // status after first successful snapshot unless we're recovering from an error.
        match message {
            TimingMessage::Status { text, .. } => {
                let is_imsa_fetching_status =
                    series == Series::Imsa && text == "Fetching IMSA live timing...";
                if is_imsa_fetching_status {
                    // Only show fetching status before first snapshot or during error recovery.
                    if snapshot.last_update_unix_ms.is_none() || snapshot.last_error.is_some() {
                        snapshot.status = text.clone();
                    }
                } else {
                    snapshot.status = text.clone();
                }
            }
            TimingMessage::Error { text, .. } => {
                snapshot.last_error = Some(text.clone());
            }
            TimingMessage::Snapshot {
                header, entries, ..
            } => {
                snapshot.header = header.clone();
                snapshot.entries = entries.clone();
                snapshot.last_error = None;
                snapshot.status = "Live timing connected".to_string();
                snapshot.last_update_unix_ms = Some(now_unix_ms());
            }
            TimingMessage::Notice { .. } => {}
        }
    }

    pub fn notify_series_update(&self, series: Series) {
        if let Some(stream) = self.streams.get(&series) {
            let _ = stream.send(());
        }
    }

    pub fn demo_state_for_session(&self, session_token: &str) -> DemoStateResponse {
        let now = now_unix_ms();
        let mut guard = match self.session_demo.write() {
            Ok(g) => g,
            Err(_) => {
                return DemoStateResponse { enabled: false };
            }
        };
        retain_recent_sessions(&mut guard, now);

        let entry = guard
            .entry(session_token.to_string())
            .or_insert_with(|| SessionDemoState {
                enabled: false,
                seed: session_seed(session_token),
                started_unix_ms: now,
                last_seen_unix_ms: now,
            });
        entry.last_seen_unix_ms = now;

        DemoStateResponse {
            enabled: entry.enabled,
        }
    }

    pub fn set_demo_for_session(&self, session_token: &str, enabled: bool) -> DemoStateResponse {
        let now = now_unix_ms();
        let mut guard = match self.session_demo.write() {
            Ok(g) => g,
            Err(_) => {
                return DemoStateResponse { enabled: false };
            }
        };
        retain_recent_sessions(&mut guard, now);

        let entry = guard
            .entry(session_token.to_string())
            .or_insert_with(|| SessionDemoState {
                enabled: false,
                seed: session_seed(session_token),
                started_unix_ms: now,
                last_seen_unix_ms: now,
            });
        if enabled && !entry.enabled {
            entry.started_unix_ms = now;
        }
        entry.enabled = enabled;
        entry.last_seen_unix_ms = now;

        DemoStateResponse { enabled }
    }

    pub fn demo_snapshot_response_for(
        &self,
        series: Series,
        session_token: &str,
    ) -> Option<SnapshotResponse> {
        let now = now_unix_ms();
        let mut guard = self.session_demo.write().ok()?;
        retain_recent_sessions(&mut guard, now);

        let entry = guard
            .entry(session_token.to_string())
            .or_insert_with(|| SessionDemoState {
                enabled: false,
                seed: session_seed(session_token),
                started_unix_ms: now,
                last_seen_unix_ms: now,
            });
        entry.last_seen_unix_ms = now;
        if !entry.enabled {
            return None;
        }

        let elapsed_secs = now.saturating_sub(entry.started_unix_ms) / 1000;
        let (header, entries) = demo::demo_snapshot_at(series, entry.seed, elapsed_secs);
        let snapshot = SeriesSnapshot {
            header,
            entries,
            status: format!("{} demo data", series.label()),
            last_error: None,
            last_update_unix_ms: Some(now),
        };

        Some(SnapshotResponse {
            series: to_api_series(series),
            snapshot: to_api_series_snapshot(snapshot),
        })
    }

    pub fn subscribe_series(&self, series: Series) -> Option<broadcast::Receiver<()>> {
        self.streams.get(&series).map(|tx| tx.subscribe())
    }

    pub fn set_feed_controller(&self, controller: FeedController) {
        if let Ok(mut guard) = self.feed_controller.write() {
            *guard = Some(controller);
        }
    }

    pub fn open_live_series(&self, series: Series) -> LiveSeriesGuard {
        let controller = self
            .feed_controller
            .read()
            .ok()
            .and_then(|guard| guard.as_ref().cloned());

        if let Some(active) = controller.as_ref() {
            active.register_client(series);
        }

        LiveSeriesGuard { controller, series }
    }

    pub fn profile_cookie_secure(&self) -> bool {
        self.profile_cookie_secure
    }

    pub fn current_preferences_for(&self, profile_id: &str) -> Result<Preferences, String> {
        {
            let guard = self
                .preferences
                .read()
                .map_err(|_| "preferences lock poisoned".to_string())?;
            if let Some(prefs) = guard.get(profile_id) {
                return Ok(prefs.clone());
            }
        }

        let loaded = load_preferences(profile_id);
        let mut guard = self
            .preferences
            .write()
            .map_err(|_| "preferences lock poisoned".to_string())?;
        guard.insert(profile_id.to_string(), loaded.clone());
        Ok(loaded)
    }

    pub fn update_preferences_for(
        &self,
        profile_id: &str,
        mut next: Preferences,
    ) -> Result<Preferences, String> {
        // Keep only well-formed favourite keys so stale garbage does not spread.
        next.favourites = favourites::normalize_favourites(next.favourites);

        save_preferences(profile_id, &next)?;

        let mut guard = self
            .preferences
            .write()
            .map_err(|_| "preferences lock poisoned".to_string())?;
        guard.insert(profile_id.to_string(), next.clone());
        Ok(next)
    }

    pub fn reset_preferences_for(&self, profile_id: &str) -> Result<Preferences, String> {
        let defaults = reset_preferences(profile_id)?;
        let mut guard = self
            .preferences
            .write()
            .map_err(|_| "preferences lock poisoned".to_string())?;
        guard.insert(profile_id.to_string(), defaults.clone());
        Ok(defaults)
    }
}

fn to_api_series(value: Series) -> web_shared::Series {
    match value {
        Series::Imsa => web_shared::Series::Imsa,
        Series::Nls => web_shared::Series::Nls,
        Series::F1 => web_shared::Series::F1,
        Series::Wec => web_shared::Series::Wec,
        Series::Dhlm => web_shared::Series::Dhlm,
    }
}

fn to_api_series_snapshot(snapshot: SeriesSnapshot) -> web_shared::SeriesSnapshot {
    web_shared::SeriesSnapshot {
        header: web_shared::TimingHeader {
            session_name: snapshot.header.session_name,
            session_type_raw: snapshot.header.session_type_raw,
            event_name: snapshot.header.event_name,
            track_name: snapshot.header.track_name,
            day_time: snapshot.header.day_time,
            flag: snapshot.header.flag,
            time_to_go: snapshot.header.time_to_go,
            class_colors: snapshot
                .header
                .class_colors
                .into_iter()
                .map(|(key, value)| (key, web_shared::TimingClassColor { color: value.color }))
                .collect(),
        },
        entries: snapshot
            .entries
            .into_iter()
            .map(|entry| web_shared::TimingEntry {
                position: entry.position,
                car_number: entry.car_number,
                class_name: entry.class_name,
                class_rank: entry.class_rank,
                driver: entry.driver,
                vehicle: entry.vehicle,
                team: entry.team,
                laps: entry.laps,
                gap_overall: entry.gap_overall,
                gap_class: entry.gap_class,
                gap_next_in_class: entry.gap_next_in_class,
                last_lap: entry.last_lap,
                best_lap: entry.best_lap,
                sector_1: entry.sector_1,
                sector_2: entry.sector_2,
                sector_3: entry.sector_3,
                sector_4: entry.sector_4,
                sector_5: entry.sector_5,
                best_lap_no: entry.best_lap_no,
                pit: entry.pit,
                pit_stops: entry.pit_stops,
                fastest_driver: entry.fastest_driver,
                stable_id: entry.stable_id,
            })
            .collect(),
        status: snapshot.status,
        last_error: snapshot.last_error,
        last_update_unix_ms: snapshot.last_update_unix_ms,
    }
}

impl Default for WebAppState {
    fn default() -> Self {
        Self::new()
    }
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn session_seed(token: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    token.hash(&mut hasher);
    hasher.finish()
}

fn retain_recent_sessions(sessions: &mut HashMap<String, SessionDemoState>, now_unix_ms: u64) {
    const SESSION_RETENTION_MS: u64 = 60 * 60 * 1000;
    sessions.retain(|_, state| {
        now_unix_ms.saturating_sub(state.last_seen_unix_ms) <= SESSION_RETENTION_MS
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_header() -> TimingHeader {
        TimingHeader {
            session_name: "Race1".to_string(),
            session_type_raw: "R".to_string(),
            event_name: "Test Event".to_string(),
            track_name: "Daytona".to_string(),
            day_time: "12:00".to_string(),
            flag: "green".to_string(),
            time_to_go: "1:00:00".to_string(),
            ..TimingHeader::default()
        }
    }

    fn test_entries() -> Vec<TimingEntry> {
        vec![TimingEntry {
            position: 1,
            car_number: "31".to_string(),
            class_name: "GTP".to_string(),
            class_rank: "1".to_string(),
            driver: "Driver A".to_string(),
            vehicle: "Porsche 963".to_string(),
            team: "Team 1".to_string(),
            laps: "10".to_string(),
            gap_overall: "-".to_string(),
            gap_class: "-".to_string(),
            gap_next_in_class: "-".to_string(),
            last_lap: "1:45.000".to_string(),
            best_lap: "1:44.500".to_string(),
            sector_1: "30.000".to_string(),
            sector_2: "35.000".to_string(),
            sector_3: "39.500".to_string(),
            sector_4: "".to_string(),
            sector_5: "".to_string(),
            best_lap_no: "5".to_string(),
            pit: "No".to_string(),
            pit_stops: "0".to_string(),
            fastest_driver: "".to_string(),
            stable_id: "31:A".to_string(),
        }]
    }

    #[test]
    fn imsa_fetching_status_appears_before_first_snapshot() {
        let state = WebAppState::new();
        state.apply_timing_message(
            Series::Imsa,
            &TimingMessage::Status {
                source_id: 1,
                text: "Fetching IMSA live timing...".to_string(),
            },
        );

        let snapshot = state.snapshot_for(Series::Imsa).unwrap();
        assert_eq!(snapshot.status, "Fetching IMSA live timing...");
    }

    #[test]
    fn imsa_fetching_status_ignored_after_connection() {
        let state = WebAppState::new();

        // First, simulate a successful connection.
        state.apply_timing_message(
            Series::Imsa,
            &TimingMessage::Snapshot {
                source_id: 1,
                header: test_header(),
                entries: test_entries(),
            },
        );

        assert_eq!(
            state.snapshot_for(Series::Imsa).unwrap().status,
            "Live timing connected"
        );

        // Now send another fetching status - it should be ignored.
        state.apply_timing_message(
            Series::Imsa,
            &TimingMessage::Status {
                source_id: 1,
                text: "Fetching IMSA live timing...".to_string(),
            },
        );

        let snapshot = state.snapshot_for(Series::Imsa).unwrap();
        assert_eq!(snapshot.status, "Live timing connected");
    }

    #[test]
    fn imsa_fetching_status_allowed_during_error_recovery() {
        let state = WebAppState::new();

        // First, simulate a successful connection.
        state.apply_timing_message(
            Series::Imsa,
            &TimingMessage::Snapshot {
                source_id: 1,
                header: test_header(),
                entries: test_entries(),
            },
        );

        // Then an error occurs.
        state.apply_timing_message(
            Series::Imsa,
            &TimingMessage::Error {
                source_id: 1,
                text: "Connection failed".to_string(),
            },
        );

        assert!(state
            .snapshot_for(Series::Imsa)
            .unwrap()
            .last_error
            .is_some());

        // Now send fetching status - it should be allowed during recovery.
        state.apply_timing_message(
            Series::Imsa,
            &TimingMessage::Status {
                source_id: 1,
                text: "Fetching IMSA live timing...".to_string(),
            },
        );

        let snapshot = state.snapshot_for(Series::Imsa).unwrap();
        assert_eq!(snapshot.status, "Fetching IMSA live timing...");
    }

    #[test]
    fn other_series_status_unaffected() {
        let state = WebAppState::new();

        // For NLS, fetching status should work normally.
        state.apply_timing_message(
            Series::Nls,
            &TimingMessage::Status {
                source_id: 2,
                text: "Fetching NLS live timing...".to_string(),
            },
        );

        let snapshot = state.snapshot_for(Series::Nls).unwrap();
        assert_eq!(snapshot.status, "Fetching NLS live timing...");

        // After snapshot, send another status.
        state.apply_timing_message(
            Series::Nls,
            &TimingMessage::Snapshot {
                source_id: 2,
                header: test_header(),
                entries: test_entries(),
            },
        );

        state.apply_timing_message(
            Series::Nls,
            &TimingMessage::Status {
                source_id: 2,
                text: "Fetching NLS live timing...".to_string(),
            },
        );

        let snapshot = state.snapshot_for(Series::Nls).unwrap();
        assert_eq!(snapshot.status, "Fetching NLS live timing...");
    }

    #[test]
    fn non_fetching_imsa_status_unaffected() {
        let state = WebAppState::new();

        // First, simulate a successful connection.
        state.apply_timing_message(
            Series::Imsa,
            &TimingMessage::Snapshot {
                source_id: 1,
                header: test_header(),
                entries: test_entries(),
            },
        );

        // Send a non-fetching status - it should be applied.
        state.apply_timing_message(
            Series::Imsa,
            &TimingMessage::Status {
                source_id: 1,
                text: "Idle (waiting for client)".to_string(),
            },
        );

        let snapshot = state.snapshot_for(Series::Imsa).unwrap();
        assert_eq!(snapshot.status, "Idle (waiting for client)");
    }
}
