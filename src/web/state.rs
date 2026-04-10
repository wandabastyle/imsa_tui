// In-memory web app state:
// latest snapshot per series, per-profile preferences, and broadcast channels for SSE fanout.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, RwLock},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use tokio::sync::broadcast;

use crate::timing::{Series, TimingEntry, TimingHeader, TimingMessage};

use super::prefs::{load_preferences, save_preferences, Preferences};

#[derive(Debug, Clone, Default, Serialize)]
pub struct SeriesSnapshot {
    pub header: TimingHeader,
    pub entries: Vec<TimingEntry>,
    pub status: String,
    pub last_error: Option<String>,
    pub last_update_unix_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SnapshotResponse {
    pub series: Series,
    pub snapshot: SeriesSnapshot,
}

#[derive(Clone)]
pub struct WebAppState {
    snapshots: Arc<RwLock<HashMap<Series, SeriesSnapshot>>>,
    preferences: Arc<RwLock<HashMap<String, Preferences>>>,
    profile_cookie_secure: bool,
    streams: Arc<HashMap<Series, broadcast::Sender<()>>>,
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
            profile_cookie_secure,
            streams: Arc::new(streams),
        }
    }

    pub fn snapshot_for(&self, series: Series) -> Option<SeriesSnapshot> {
        self.snapshots.read().ok()?.get(&series).cloned()
    }

    pub fn snapshot_response_for(&self, series: Series) -> Option<SnapshotResponse> {
        self.snapshot_for(series)
            .map(|snapshot| SnapshotResponse { series, snapshot })
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
        match message {
            TimingMessage::Status { text, .. } => {
                snapshot.status = text.clone();
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
        }
    }

    pub fn notify_series_update(&self, series: Series) {
        if let Some(stream) = self.streams.get(&series) {
            let _ = stream.send(());
        }
    }

    pub fn subscribe_series(&self, series: Series) -> Option<broadcast::Receiver<()>> {
        self.streams.get(&series).map(|tx| tx.subscribe())
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
        next.favourites = next
            .favourites
            .into_iter()
            .filter(|value| value.contains('|'))
            .collect::<HashSet<_>>();

        save_preferences(profile_id, &next)?;

        let mut guard = self
            .preferences
            .write()
            .map_err(|_| "preferences lock poisoned".to_string())?;
        guard.insert(profile_id.to_string(), next.clone());
        Ok(next)
    }
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
