// In-memory web app state:
// latest snapshot per series, per-profile preferences, and broadcast channels for SSE fanout.

use std::{
    collections::hash_map::DefaultHasher,
    collections::HashMap,
    hash::{Hash, Hasher},
    sync::{Arc, RwLock},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use tokio::sync::broadcast;

use crate::timing::{Series, TimingEntry, TimingHeader, TimingMessage};

use crate::demo;
use crate::favourites;

use super::bridge::FeedController;
use super::prefs::{load_preferences, reset_preferences, save_preferences, Preferences};

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

#[derive(Debug, Clone, Copy, Serialize)]
pub struct DemoSessionResponse {
    pub enabled: bool,
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

    pub fn demo_state_for_session(&self, session_token: &str) -> DemoSessionResponse {
        let now = now_unix_ms();
        let mut guard = match self.session_demo.write() {
            Ok(g) => g,
            Err(_) => {
                return DemoSessionResponse { enabled: false };
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

        DemoSessionResponse {
            enabled: entry.enabled,
        }
    }

    pub fn set_demo_for_session(&self, session_token: &str, enabled: bool) -> DemoSessionResponse {
        let now = now_unix_ms();
        let mut guard = match self.session_demo.write() {
            Ok(g) => g,
            Err(_) => {
                return DemoSessionResponse { enabled: false };
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

        DemoSessionResponse { enabled }
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

        Some(SnapshotResponse { series, snapshot })
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
