// NLS state management

use std::{
    sync::mpsc::Sender,
    time::{Duration, Instant},
};

use crate::{
    adapters::nls::{
        countdown::{now_unix_ms, refresh_header_time_to_go, CountdownState},
        protocol::{refresh_active_event_id, should_emit_connected_status_on_update},
        schedule::determine_active_nuerburgring_event_id,
        snapshot::{
            derive_session_id, meaningful_snapshot_fingerprint, nls_snapshot_path,
            persist_snapshot, restore_snapshot_from_disk, NlsSnapshot,
        },
    },
    timing::{TimingEntry, TimingHeader, TimingMessage},
    timing_persist::{debounce_elapsed, PersistState, SeriesDebugOutput},
};

const WEBSITE_EVENT_REFRESH_INTERVAL: Duration = Duration::from_secs(600);
const SNAPSHOT_SAVE_DEBOUNCE: Duration = Duration::from_secs(180);

/// Context for NLS websocket operations.
pub struct NlsWorkerContext {
    pub header: TimingHeader,
    pub latest_entries: Vec<TimingEntry>,
    pub termine_event_name: Option<String>,
    pub homepage_event_name: Option<String>,
    pub next_website_refresh: Instant,
    pub countdown: Option<CountdownState>,
    pub is_race_session: bool,
    pub active_event_id: String,
    pub persist: PersistState,
    pub last_good_live_snapshot: Option<NlsSnapshot>,
    pub last_session_id: Option<String>,
}

impl NlsWorkerContext {
    #[must_use]
    pub fn new(
        tx: &Sender<TimingMessage>,
        source_id: u64,
        debug_output: &SeriesDebugOutput,
    ) -> Self {
        let mut header = TimingHeader {
            event_name: "NLS Live Timing".to_string(),
            track_name: "Nürburgring".to_string(),
            ..TimingHeader::default()
        };
        let mut latest_entries: Vec<TimingEntry> = Vec::new();
        let mut persist = PersistState::new(nls_snapshot_path());

        let last_session_id = restore_snapshot_from_disk(
            &mut persist,
            &mut header,
            &mut latest_entries,
            tx,
            source_id,
            debug_output,
        );

        let last_good_live_snapshot = if latest_entries.is_empty() {
            None
        } else {
            Some(NlsSnapshot {
                header: header.clone(),
                entries: latest_entries.clone(),
                session_id: last_session_id.clone(),
                fingerprint: meaningful_snapshot_fingerprint(&header, &latest_entries),
                extra: (),
            })
        };

        Self {
            header,
            latest_entries,
            termine_event_name: None,
            homepage_event_name: None,
            next_website_refresh: Instant::now(),
            countdown: None,
            is_race_session: false,
            active_event_id: "20".to_string(),
            persist,
            last_good_live_snapshot,
            last_session_id,
        }
    }
}

/// Refresh website data (event names, event IDs) if needed.
pub fn refresh_website_data(
    client: &Option<reqwest::blocking::Client>,
    ctx: &mut NlsWorkerContext,
    tx: &Sender<TimingMessage>,
    source_id: u64,
) {
    if Instant::now() < ctx.next_website_refresh {
        return;
    }

    let Some(client) = client.as_ref() else {
        return;
    };

    if let Ok(parsed_name) = crate::adapters::nls::schedule::fetch_termine_event_name(client) {
        ctx.termine_event_name = Some(parsed_name.clone());
        ctx.header.event_name = parsed_name;
    }

    ctx.homepage_event_name = crate::adapters::nls::schedule::fetch_homepage_event_name(client);

    if ctx.termine_event_name.is_none() {
        if let Some(parsed_name) = ctx.homepage_event_name.as_ref() {
            ctx.header.event_name.clone_from(parsed_name);
        }
    }

    if let Some(status_text) = refresh_active_event_id(
        &mut ctx.active_event_id,
        determine_active_nuerburgring_event_id(client),
    ) {
        let _ = tx.send(TimingMessage::Status {
            source_id,
            text: status_text,
        });
    }

    ctx.next_website_refresh = Instant::now() + WEBSITE_EVENT_REFRESH_INTERVAL;
}

/// Check if event ID has changed and needs reconnection.
/// Returns true if event ID changed (requires reconnect).
pub fn check_event_id_change(
    ctx: &mut NlsWorkerContext,
    tx: &Sender<TimingMessage>,
    source_id: u64,
    client: &Option<reqwest::blocking::Client>,
    subscribed_event_id: &str,
) -> bool {
    let Some(c) = client.as_ref() else {
        ctx.next_website_refresh = Instant::now() + WEBSITE_EVENT_REFRESH_INTERVAL;
        return false;
    };

    if let Some(status_text) =
        refresh_active_event_id(&mut ctx.active_event_id, determine_active_nuerburgring_event_id(c))
    {
        if ctx.active_event_id != subscribed_event_id {
            let _ = tx.send(TimingMessage::Status {
                source_id,
                text: status_text,
            });
            ctx.next_website_refresh = Instant::now() + WEBSITE_EVENT_REFRESH_INTERVAL;
            return true;
        }
    }

    ctx.next_website_refresh = Instant::now() + WEBSITE_EVENT_REFRESH_INTERVAL;
    false
}

/// Update timing state from parsed data, emit snapshot, and persist if needed.
pub fn update_timing_state(
    entries: Option<Vec<TimingEntry>>,
    header_changed: bool,
    tx: &Sender<TimingMessage>,
    source_id: u64,
    debug_output: &SeriesDebugOutput,
    ctx: &mut NlsWorkerContext,
    connected_status_sent: &mut bool,
) {
    if let Some(new_entries) = entries {
        ctx.latest_entries = new_entries;
    }

    refresh_header_time_to_go(&mut ctx.header, ctx.countdown.as_ref());

    let session_id = derive_session_id(&ctx.header);
    let snapshot = NlsSnapshot {
        header: ctx.header.clone(),
        entries: ctx.latest_entries.clone(),
        session_id: session_id.clone(),
        fingerprint: meaningful_snapshot_fingerprint(&ctx.header, &ctx.latest_entries),
        extra: (),
    };

    let first_real_of_session = !snapshot.entries.is_empty() && session_id != ctx.last_session_id;
    let session_complete = snapshot.header.flag.eq_ignore_ascii_case("checkered");
    let materially_changed = ctx
        .last_good_live_snapshot
        .as_ref()
        .is_none_or(|prev| prev.fingerprint != snapshot.fingerprint);

    if materially_changed {
        ctx.persist.dirty_since_last_save = true;
    }

    let never_persisted = ctx.persist.last_persisted_hash.is_none();
    let save_now = never_persisted
        || first_real_of_session
        || session_complete
        || (ctx.persist.dirty_since_last_save
            && debounce_elapsed(ctx.persist.last_save_at, SNAPSHOT_SAVE_DEBOUNCE));

    if save_now {
        persist_snapshot(&mut ctx.persist, &snapshot, now_unix_ms(), debug_output);
    }

    ctx.last_session_id = session_id;
    ctx.last_good_live_snapshot = Some(snapshot);

    let _ = tx.send(TimingMessage::Snapshot {
        source_id,
        header: ctx.header.clone(),
        entries: ctx.latest_entries.clone(),
    });

    if should_emit_connected_status_on_update(header_changed, *connected_status_sent) {
        let _ = tx.send(TimingMessage::Status {
            source_id,
            text: "NLS live timing connected".to_string(),
        });
        *connected_status_sent = true;
    }
}

/// Build HTTP client for website data fetching.
#[must_use]
pub fn build_website_client() -> Option<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent(
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 \
             Safari/537.36",
        )
        .build()
        .ok()
}
