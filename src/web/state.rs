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
    preferences: Arc<RwLock<Preferences>>,
    streams: Arc<HashMap<Series, broadcast::Sender<()>>>,
}

impl WebAppState {
    pub fn new() -> Self {
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

        let preferences = load_preferences();
        let mut streams = HashMap::new();
        for series in Series::all() {
            let (tx, _) = broadcast::channel(64);
            streams.insert(series, tx);
        }

        Self {
            snapshots: Arc::new(RwLock::new(snapshots)),
            preferences: Arc::new(RwLock::new(preferences)),
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

    pub fn current_preferences(&self) -> Option<Preferences> {
        self.preferences.read().ok().map(|prefs| (*prefs).clone())
    }

    pub fn update_preferences(&self, mut next: Preferences) -> Result<Preferences, String> {
        // Keep only well-formed favourite keys so stale garbage does not spread.
        next.favourites = next
            .favourites
            .into_iter()
            .filter(|value| value.contains('|'))
            .collect::<HashSet<_>>();

        save_preferences(&next)?;

        let mut guard = self
            .preferences
            .write()
            .map_err(|_| "preferences lock poisoned".to_string())?;
        *guard = next.clone();
        Ok(next)
    }
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
