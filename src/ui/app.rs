// Interactive TUI state machine:
// - consumes worker messages
// - derives view/group/search/favourite projections
// - renders one frame
// - handles one keyboard event

use std::{
    collections::{BTreeSet, HashMap, HashSet, VecDeque},
    io,
    sync::mpsc::{self, Sender},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crossterm::event::{self, Event, KeyCode};
use ratatui::{backend::CrosstermBackend, Terminal};

use super::{
    config::{load_config, save_config, AppConfig},
    feed::{
        drain_messages, drain_series_debug_logs, push_series_debug_log, start_feed, stop_feed,
        ActiveFeed, IMSA_DEBUG_LOG_CAPACITY,
    },
    gap::gap_anchor_from_entry,
    grouping::{
        grouped_entries, next_view_mode, selected_series_index, view_entries_for_mode, ViewMode,
    },
    pit::{refresh_pit_trackers, PitTracker},
    popups::{
        liveticker_line_count, GroupPickerState, LogsPanelState, MessagesPanelState,
        NlsLivetickerPanelState, SeriesPickerState,
    },
    render::{draw_frame, RenderCtx},
    search::{refresh_search_matches, SearchState},
    width_state::SeriesWidthBaselines,
};
use crate::adapters::nls::liveticker::{
    stop_liveticker_feed, ActiveLivetickerFeed, LivetickerEntry, LivetickerWorkerMessage,
};

use crate::demo;
use crate::{
    favourites,
    timing::{Series, TimingEntry, TimingHeader, TimingMessage, TimingNotice},
};

struct SeriesChangeCtx<'a> {
    active_series: &'a mut Series,
    feed: &'a mut Option<ActiveFeed>,
    tx: &'a Sender<TimingMessage>,
    source_id_ctr: &'a mut u64,
    demo_mode: bool,
    header: &'a mut TimingHeader,
    entries: &'a mut Vec<TimingEntry>,
    status: &'a mut String,
    favourites: &'a mut HashSet<String>,
    notices: &'a mut Vec<TimingNotice>,
    notice_keys: &'a mut HashSet<String>,
    highlighted_notice_cars: &'a mut HashSet<String>,
    message_flag_override: &'a mut Option<MessageFlagOverride>,
    message_flag_last_secs: &'a mut Option<u32>,
    last_error: &'a mut Option<String>,
    last_update: &'a mut Option<Instant>,
    selected_row: &'a mut usize,
    view_mode: &'a mut ViewMode,
    search: &'a mut SearchState,
    series_logs: &'a mut VecDeque<String>,
    config: &'a mut AppConfig,
    nls_liveticker_feed: &'a mut Option<ActiveLivetickerFeed>,
    nls_liveticker_entries: &'a mut Vec<LivetickerEntry>,
    nls_liveticker_last_update: &'a mut Option<Instant>,
    nls_liveticker_last_error: &'a mut Option<String>,
    nls_liveticker_panel: &'a mut NlsLivetickerPanelState,
}

#[derive(Debug, Clone)]
struct MessageFlagOverride {
    flag: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FlagMessageIntent {
    SetRed,
    SetCode60,
    SetYellow,
    Clear,
}

const DISMISSED_NOTICE_TTL_SECS: u64 = 7 * 24 * 60 * 60;
const DISMISSED_NOTICE_MAX_PER_SERIES: usize = 500;

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn parse_notice_time_seconds(raw: &str) -> Option<u32> {
    let trimmed = raw.trim();
    let mut parts = trimmed.split(':');
    let h = parts.next()?.parse::<u32>().ok()?;
    let m = parts.next()?.parse::<u32>().ok()?;
    let s = parts.next()?.parse::<u32>().ok()?;
    if parts.next().is_some() || h > 23 || m > 59 || s > 59 {
        return None;
    }
    Some(h * 3600 + m * 60 + s)
}

fn classify_flag_message_intent(text: &str) -> Option<FlagMessageIntent> {
    let normalized = text.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }

    if normalized.contains("green flag")
        || normalized.contains("all clear")
        || normalized.contains("track clear")
        || normalized.contains("resume")
        || normalized.contains("re-start")
        || normalized.contains("restart")
        || normalized.contains("grune flagge")
        || normalized.contains("gruene flagge")
    {
        return Some(FlagMessageIntent::Clear);
    }

    let looks_like_penalty = normalized.contains("non respect")
        || normalized.contains("penalty")
        || normalized.contains("time penalty")
        || normalized.contains("pit speed")
        || normalized.contains("after first lap");
    if looks_like_penalty {
        return None;
    }

    if normalized.contains("red flag") || normalized.contains("rote flagge") {
        return Some(FlagMessageIntent::SetRed);
    }
    if normalized.contains("full course yellow")
        || normalized.contains("fcy")
        || normalized.contains("yellow flag")
        || normalized.contains("gelb")
    {
        return Some(FlagMessageIntent::SetYellow);
    }
    if normalized.starts_with("code 60")
        || normalized.contains(" code 60 phase")
        || normalized.contains("code60 phase")
        || normalized.contains("code 60 in force")
    {
        return Some(FlagMessageIntent::SetCode60);
    }

    None
}

fn apply_flag_message_notice(
    notice: &TimingNotice,
    override_state: &mut Option<MessageFlagOverride>,
    last_secs: &mut Option<u32>,
) {
    let Some(intent) = classify_flag_message_intent(&notice.text) else {
        return;
    };
    let Some(notice_secs) = parse_notice_time_seconds(&notice.time) else {
        return;
    };

    if let Some(current_secs) = *last_secs {
        if notice_secs < current_secs {
            return;
        }
    }
    *last_secs = Some(notice_secs);

    match intent {
        FlagMessageIntent::SetRed => {
            *override_state = Some(MessageFlagOverride {
                flag: "Red".to_string(),
            });
        }
        FlagMessageIntent::SetCode60 => {
            *override_state = Some(MessageFlagOverride {
                flag: "Code 60".to_string(),
            });
        }
        FlagMessageIntent::SetYellow => {
            *override_state = Some(MessageFlagOverride {
                flag: "Yellow".to_string(),
            });
        }
        FlagMessageIntent::Clear => {
            *override_state = None;
        }
    }
}

fn extract_notice_car_numbers(text: &str) -> HashSet<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut car_numbers = HashSet::new();
    let mut idx = 0usize;

    while idx < chars.len() {
        if chars[idx] != '#' {
            idx += 1;
            continue;
        }

        idx += 1;
        let start = idx;
        while idx < chars.len() && chars[idx].is_ascii_digit() {
            idx += 1;
        }
        if idx == start {
            continue;
        }

        let raw: String = chars[start..idx].iter().collect();
        if raw.is_empty() {
            continue;
        }

        car_numbers.insert(raw.clone());
        let normalized = raw.trim_start_matches('0');
        if !normalized.is_empty() {
            car_numbers.insert(normalized.to_string());
        }
    }

    car_numbers
}

fn notice_key(notice: &TimingNotice) -> String {
    format!(
        "{}|{}|{}",
        notice.id.trim(),
        notice.time.trim(),
        notice.text.trim()
    )
}

fn normalized_notice_text_for_dismissal_key(text: &str) -> String {
    let collapsed = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();

    let chars: Vec<char> = collapsed.chars().collect();
    let mut normalized = String::with_capacity(chars.len());
    let mut idx = 0usize;

    while idx < chars.len() {
        if chars[idx] != '#' {
            normalized.push(chars[idx]);
            idx += 1;
            continue;
        }

        normalized.push('#');
        idx += 1;
        while idx < chars.len() && chars[idx].is_ascii_whitespace() {
            idx += 1;
        }

        if idx < chars.len() && chars[idx].is_ascii_digit() {
            while idx < chars.len() && chars[idx].is_ascii_digit() {
                normalized.push(chars[idx]);
                idx += 1;
            }
            continue;
        }
    }

    normalized
}

fn persisted_notice_identity_key(notice: &TimingNotice) -> String {
    let text = normalized_notice_text_for_dismissal_key(&notice.text);
    format!("text|{text}")
}

fn persisted_notice_key(series: Series, notice: &TimingNotice) -> String {
    format!(
        "{}|{}",
        series.as_key_prefix(),
        persisted_notice_identity_key(notice)
    )
}

fn prune_dismissed_notice_keys(dismissed: &mut HashMap<String, u64>, now_unix_secs: u64) -> bool {
    let mut changed = false;

    for timestamp in dismissed.values_mut() {
        if *timestamp == 0 {
            *timestamp = now_unix_secs;
            changed = true;
        }
    }

    let before_ttl = dismissed.len();
    dismissed.retain(|_, timestamp| {
        now_unix_secs.saturating_sub(*timestamp) <= DISMISSED_NOTICE_TTL_SECS
    });
    if dismissed.len() != before_ttl {
        changed = true;
    }

    let mut by_series: HashMap<String, Vec<(String, u64)>> = HashMap::new();
    for (key, timestamp) in dismissed.iter() {
        let prefix = key
            .split_once('|')
            .map(|(series, _)| series)
            .unwrap_or_default()
            .to_string();
        by_series
            .entry(prefix)
            .or_default()
            .push((key.clone(), *timestamp));
    }

    let mut keys_to_remove = HashSet::new();
    for (_, mut keys) in by_series {
        if keys.len() <= DISMISSED_NOTICE_MAX_PER_SERIES {
            continue;
        }
        keys.sort_by_key(|entry| std::cmp::Reverse(entry.1));
        for (key, _) in keys.into_iter().skip(DISMISSED_NOTICE_MAX_PER_SERIES) {
            keys_to_remove.insert(key);
        }
    }

    if !keys_to_remove.is_empty() {
        dismissed.retain(|key, _| !keys_to_remove.contains(key));
        changed = true;
    }

    changed
}

fn clear_dismissed_notice_keys_for_series(series: Series, dismissed: &mut HashMap<String, u64>) {
    let prefix = format!("{}|", series.as_key_prefix());
    dismissed.retain(|key, _| !key.starts_with(&prefix));
}

fn persist_dismissed_notice_keys(
    config: &mut AppConfig,
    dismissed_notice_keys: &mut HashMap<String, u64>,
    last_error: &mut Option<String>,
) {
    prune_dismissed_notice_keys(dismissed_notice_keys, now_unix_secs());
    config.dismissed_notice_keys = dismissed_notice_keys.clone();
    if let Err(err) = save_config(config) {
        *last_error = Some(err);
    }
}

fn rebuild_highlighted_notice_cars(notices: &[TimingNotice]) -> HashSet<String> {
    let mut highlighted = HashSet::new();
    for notice in notices {
        highlighted.extend(extract_notice_car_numbers(&notice.text));
    }
    highlighted
}

fn favourite_key(series: Series, stable_id: &str) -> String {
    favourites::favourite_key(series, stable_id)
}

fn demo_snapshot(series: Series) -> (TimingHeader, Vec<TimingEntry>) {
    demo::demo_snapshot(series)
}

fn seed_demo_favourites(series: Series, favourites: &mut HashSet<String>) {
    demo::seed_demo_favourites(series, favourites);
}

fn step_selection(current: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    let max = (len - 1) as isize;
    ((current as isize + delta).clamp(0, max)) as usize
}

fn series_log_prefix(series: Series) -> String {
    format!("[{}]", series.label())
}

fn retain_logs_for_series(logs: &mut VecDeque<String>, series: Series) {
    let prefix = series_log_prefix(series);
    logs.retain(|line| line.starts_with(&prefix));
}

fn class_color_source_log_line(
    series: Series,
    header: &TimingHeader,
    entries: &[TimingEntry],
) -> String {
    if matches!(series, Series::Nls | Series::Dhlm) {
        return format!(
            "{} class colors: disabled for this series",
            series_log_prefix(series)
        );
    }

    let visible_classes: BTreeSet<String> = entries
        .iter()
        .map(|entry| entry.class_name.trim())
        .filter(|name| !name.is_empty() && *name != "-")
        .map(ToString::to_string)
        .collect();

    let dynamic: Vec<String> = visible_classes
        .iter()
        .filter(|class_name| header.class_colors.contains_key(*class_name))
        .cloned()
        .collect();
    let static_fallback: Vec<String> = visible_classes
        .iter()
        .filter(|class_name| !header.class_colors.contains_key(*class_name))
        .cloned()
        .collect();

    let dynamic_part = if dynamic.is_empty() {
        "-".to_string()
    } else {
        dynamic.join(",")
    };
    let static_part = if static_fallback.is_empty() {
        "-".to_string()
    } else {
        static_fallback.join(",")
    };

    format!(
        "{} class colors dynamic={} [{}] static={} [{}]",
        series_log_prefix(series),
        dynamic.len(),
        dynamic_part,
        static_fallback.len(),
        static_part
    )
}

// Switching feeds is centralized so both keyboard shortcuts and popup confirmation
// run the exact same state-reset flow as more series are added.
fn apply_series_change(next_series: Series, ctx: &mut SeriesChangeCtx<'_>) {
    if *ctx.active_series == next_series {
        return;
    }

    stop_feed(ctx.feed);
    *ctx.active_series = next_series;

    if ctx.demo_mode {
        (*ctx.header, *ctx.entries) = demo_snapshot(*ctx.active_series);
        *ctx.status = format!("{} demo data", ctx.active_series.label());
        seed_demo_favourites(*ctx.active_series, ctx.favourites);
    } else {
        *ctx.source_id_ctr += 1;
        *ctx.feed = Some(start_feed(
            *ctx.active_series,
            ctx.tx.clone(),
            *ctx.source_id_ctr,
        ));
        *ctx.header = TimingHeader::default();
        ctx.entries.clear();
        *ctx.status = format!("Starting {} live timing...", ctx.active_series.label());
    }

    *ctx.last_error = None;
    *ctx.last_update = None;
    *ctx.selected_row = 0;
    *ctx.view_mode = ViewMode::Overall;
    *ctx.search = SearchState::default();
    ctx.notices.clear();
    ctx.notice_keys.clear();
    ctx.highlighted_notice_cars.clear();
    *ctx.message_flag_override = None;
    *ctx.message_flag_last_secs = None;
    ctx.series_logs.clear();

    if *ctx.active_series != Series::Nls {
        stop_liveticker_feed(ctx.nls_liveticker_feed);
        ctx.nls_liveticker_entries.clear();
        *ctx.nls_liveticker_last_update = None;
        *ctx.nls_liveticker_last_error = None;
        *ctx.nls_liveticker_panel = NlsLivetickerPanelState::closed();
    } else if ctx.nls_liveticker_feed.is_none() {
        *ctx.nls_liveticker_feed = Some(crate::adapters::nls::liveticker::start_liveticker_feed());
    }

    ctx.config.selected_series = *ctx.active_series;
    if let Err(err) = save_config(ctx.config) {
        *ctx.last_error = Some(err);
    }
}

pub fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    let (tx, rx) = mpsc::channel::<TimingMessage>();
    let tick_rate = Duration::from_millis(250);

    let mut config = load_config();
    let mut config_load_error = None;
    if prune_dismissed_notice_keys(&mut config.dismissed_notice_keys, now_unix_secs()) {
        if let Err(err) = save_config(&config) {
            config_load_error = Some(err);
        }
    }

    let mut active_series = config.selected_series;
    let mut source_id_ctr = 1_u64;
    let mut demo_mode = false;
    let mut demo_started_at = Instant::now();
    let mut demo_seed = 1_u64;
    let mut feed = Some(start_feed(active_series, tx.clone(), source_id_ctr));

    let (mut header, mut entries) = (TimingHeader::default(), Vec::new());
    let mut status = format!("Starting {} live timing...", active_series.label());
    let mut last_error: Option<String> = config_load_error;
    let mut last_update: Option<Instant> = None;
    let mut previous_flag = "-".to_string();
    let mut transition_started_at = Instant::now();
    let mut view_mode = ViewMode::Overall;
    let mut selected_row = 0usize;
    let mut favourites: HashSet<String> = config.favourites.clone();
    let mut dismissed_notice_keys: HashMap<String, u64> = config.dismissed_notice_keys.clone();
    let mut show_help = false;
    let mut search = SearchState::default();
    let mut series_picker = SeriesPickerState::closed();
    let mut group_picker = GroupPickerState::closed();
    let mut logs_panel = LogsPanelState::closed();
    let mut messages_panel = MessagesPanelState::closed();
    let mut nls_liveticker_panel = NlsLivetickerPanelState::closed();
    let mut notices: Vec<TimingNotice> = Vec::new();
    let mut nls_liveticker_entries: Vec<LivetickerEntry> = Vec::new();
    let mut nls_liveticker_last_update: Option<Instant> = None;
    let mut nls_liveticker_last_error: Option<String> = None;
    let mut nls_liveticker_feed = if active_series == Series::Nls {
        Some(crate::adapters::nls::liveticker::start_liveticker_feed())
    } else {
        None
    };
    let mut notice_keys: HashSet<String> = HashSet::new();
    let mut highlighted_notice_cars: HashSet<String> = HashSet::new();
    let mut message_flag_override: Option<MessageFlagOverride> = None;
    let mut message_flag_last_secs: Option<u32> = None;
    let mut imsa_debug_logs = VecDeque::new();
    let mut gap_anchor_stable_id: Option<String> = None;
    let mut pit_trackers: HashMap<String, PitTracker> = HashMap::new();
    let mut width_baselines = SeriesWidthBaselines::load();
    let ui_started_at = Instant::now();

    loop {
        // This loop drives the app like a tiny state machine:
        // 1) pull feed updates, 2) compute derived view data, 3) render, 4) process one key event.
        if demo_mode {
            let elapsed_secs = demo_started_at.elapsed().as_secs();
            let (next_header, next_entries) =
                demo::demo_snapshot_at(active_series, demo_seed, elapsed_secs);
            header = next_header;
            entries = next_entries;
            status = format!("{} demo data", active_series.label());
            last_error = None;
            last_update = Some(Instant::now());
        } else if let Some(active_feed) = &feed {
            let incoming_notices = drain_messages(
                &rx,
                active_feed.source_id,
                &mut header,
                &mut entries,
                &mut status,
                &mut last_error,
                &mut last_update,
            );

            for notice in incoming_notices {
                apply_flag_message_notice(
                    &notice,
                    &mut message_flag_override,
                    &mut message_flag_last_secs,
                );
                let key = notice_key(&notice);
                if dismissed_notice_keys.contains_key(&persisted_notice_key(active_series, &notice))
                {
                    continue;
                }
                if notice_keys.insert(key) {
                    notices.push(notice);
                }
            }
            highlighted_notice_cars = rebuild_highlighted_notice_cars(&notices);
            messages_panel.selected_idx = messages_panel
                .selected_idx
                .min(notices.len().saturating_sub(1));
        }
        drain_series_debug_logs(&feed, &mut imsa_debug_logs);

        if let Some(liveticker_feed) = nls_liveticker_feed.as_ref() {
            while let Ok(message) = liveticker_feed.rx.try_recv() {
                match message {
                    LivetickerWorkerMessage::Snapshot { entries } => {
                        nls_liveticker_entries = entries;
                        nls_liveticker_last_update = Some(Instant::now());
                        nls_liveticker_last_error = None;
                    }
                    LivetickerWorkerMessage::Error { text } => {
                        nls_liveticker_last_error = Some(text);
                    }
                }
            }
        }

        let liveticker_max_scroll =
            liveticker_line_count(&nls_liveticker_entries, nls_liveticker_last_error.is_some())
                .saturating_sub(1);
        nls_liveticker_panel.scroll = nls_liveticker_panel.scroll.min(liveticker_max_scroll);

        if !demo_mode {
            width_baselines.capture_if_missing(active_series, &entries);
            width_baselines.persist_if_dirty();
        }

        let current_groups = grouped_entries(&entries, active_series);
        let now = Instant::now();
        refresh_pit_trackers(&mut pit_trackers, &entries, active_series, now);

        if let ViewMode::Class(idx) = view_mode {
            if current_groups.is_empty() {
                view_mode = ViewMode::Overall;
            } else if idx >= current_groups.len() {
                view_mode = ViewMode::Class(current_groups.len() - 1);
            }
        }

        let current_view_entries = view_entries_for_mode(
            &entries,
            &current_groups,
            view_mode,
            &favourites,
            active_series,
        );

        if let Some(anchor_id) = &gap_anchor_stable_id {
            if !current_view_entries
                .iter()
                .any(|entry| entry.stable_id == *anchor_id)
            {
                gap_anchor_stable_id = None;
            }
        }

        let gap_anchor = gap_anchor_stable_id.as_ref().and_then(|anchor_id| {
            current_view_entries
                .iter()
                .find(|entry| entry.stable_id == *anchor_id)
                .map(|entry| gap_anchor_from_entry(entry))
        });
        selected_row = selected_row.min(current_view_entries.len().saturating_sub(1));

        refresh_search_matches(&mut search, &current_view_entries);
        if !search.matches.is_empty() {
            let idx = search.matches[search.current_match];
            selected_row = idx.min(current_view_entries.len().saturating_sub(1));
        }

        let marked_stable_id = search
            .matches
            .get(search.current_match)
            .and_then(|idx| current_view_entries.get(*idx))
            .map(|entry| entry.stable_id.as_str());

        let effective_flag = if let Some(flag_override) = &message_flag_override {
            flag_override.flag.as_str()
        } else if header.flag.is_empty() {
            "-"
        } else {
            &header.flag
        };

        let transition_from_flag = previous_flag.clone();
        if effective_flag != previous_flag {
            previous_flag = effective_flag.to_string();
            transition_started_at = Instant::now();
        }

        let marquee_tick = (ui_started_at.elapsed().as_millis() / 240) as usize;

        terminal.draw(|f| {
            let table_width_baselines = width_baselines.table_baselines(active_series);
            let render_ctx = RenderCtx {
                active_series,
                status: &status,
                header: &header,
                entries: &entries,
                current_groups: &current_groups,
                selected_row,
                favourites: &favourites,
                marked_stable_id,
                marquee_tick,
                gap_anchor: gap_anchor.as_ref(),
                pit_trackers: &pit_trackers,
                table_width_baselines,
                now,
                view_mode,
                search: &search,
                show_help,
                series_picker,
                group_picker,
                logs_panel,
                messages_panel,
                nls_liveticker_panel,
                active_notices: &notices,
                nls_liveticker_entries: &nls_liveticker_entries,
                nls_liveticker_last_update,
                nls_liveticker_last_error: nls_liveticker_last_error.as_deref(),
                highlighted_notice_cars: &highlighted_notice_cars,
                imsa_debug_logs: &imsa_debug_logs,
                demo_mode,
                last_error: last_error.as_ref(),
                last_update,
                effective_flag,
                transition_from_flag: &transition_from_flag,
                transition_started_at,
                debug_log_capacity: IMSA_DEBUG_LOG_CAPACITY,
            };
            draw_frame(f, &render_ctx);
        })?;

        if event::poll(tick_rate)? {
            if let Event::Key(key) = event::read()? {
                if search.input_active {
                    match key.code {
                        KeyCode::Esc => search.input_active = false,
                        KeyCode::Enter => {
                            search.input_active = false;
                            refresh_search_matches(&mut search, &current_view_entries);
                            if !search.matches.is_empty() {
                                search.current_match = 0;
                                selected_row = search.matches[0];
                            }
                        }
                        KeyCode::Backspace => {
                            search.query.pop();
                        }
                        KeyCode::Char(c) if !c.is_control() => {
                            search.query.push(c);
                        }
                        _ => {}
                    }
                    continue;
                }

                if series_picker.is_open {
                    let series_list = Series::all();
                    match key.code {
                        KeyCode::Esc => series_picker.is_open = false,
                        KeyCode::Down | KeyCode::Char('j') => {
                            series_picker.selected_idx =
                                (series_picker.selected_idx + 1) % series_list.len();
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            if series_picker.selected_idx == 0 {
                                series_picker.selected_idx = series_list.len() - 1;
                            } else {
                                series_picker.selected_idx -= 1;
                            }
                        }
                        KeyCode::Enter => {
                            let next_series = series_list[series_picker.selected_idx];
                            let mut series_change_ctx = SeriesChangeCtx {
                                active_series: &mut active_series,
                                feed: &mut feed,
                                tx: &tx,
                                source_id_ctr: &mut source_id_ctr,
                                demo_mode,
                                header: &mut header,
                                entries: &mut entries,
                                status: &mut status,
                                favourites: &mut favourites,
                                notices: &mut notices,
                                notice_keys: &mut notice_keys,
                                highlighted_notice_cars: &mut highlighted_notice_cars,
                                message_flag_override: &mut message_flag_override,
                                message_flag_last_secs: &mut message_flag_last_secs,
                                last_error: &mut last_error,
                                last_update: &mut last_update,
                                selected_row: &mut selected_row,
                                view_mode: &mut view_mode,
                                search: &mut search,
                                series_logs: &mut imsa_debug_logs,
                                config: &mut config,
                                nls_liveticker_feed: &mut nls_liveticker_feed,
                                nls_liveticker_entries: &mut nls_liveticker_entries,
                                nls_liveticker_last_update: &mut nls_liveticker_last_update,
                                nls_liveticker_last_error: &mut nls_liveticker_last_error,
                                nls_liveticker_panel: &mut nls_liveticker_panel,
                            };
                            apply_series_change(next_series, &mut series_change_ctx);
                            gap_anchor_stable_id = None;
                            series_picker.is_open = false;
                            messages_panel = MessagesPanelState::closed();
                            nls_liveticker_panel = NlsLivetickerPanelState::closed();
                        }
                        _ => {}
                    }
                    continue;
                }

                if group_picker.is_open {
                    match key.code {
                        KeyCode::Esc => group_picker.is_open = false,
                        KeyCode::Down | KeyCode::Char('j') if !current_groups.is_empty() => {
                            group_picker.selected_idx =
                                (group_picker.selected_idx + 1) % current_groups.len();
                        }
                        KeyCode::Up | KeyCode::Char('k') if !current_groups.is_empty() => {
                            if group_picker.selected_idx == 0 {
                                group_picker.selected_idx = current_groups.len() - 1;
                            } else {
                                group_picker.selected_idx -= 1;
                            }
                        }
                        KeyCode::Enter => {
                            if !current_groups.is_empty() {
                                let idx = group_picker.selected_idx.min(current_groups.len() - 1);
                                view_mode = ViewMode::Class(idx);
                                selected_row = 0;
                                gap_anchor_stable_id = None;
                            }
                            group_picker.is_open = false;
                        }
                        _ => {}
                    }
                    continue;
                }

                if logs_panel.is_open {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('L') => logs_panel.is_open = false,
                        KeyCode::Down | KeyCode::Char('j') => {
                            logs_panel.scroll = logs_panel.scroll.saturating_sub(1);
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            logs_panel.scroll = logs_panel
                                .scroll
                                .saturating_add(1)
                                .min(imsa_debug_logs.len().saturating_sub(1));
                        }
                        KeyCode::PageDown => {
                            logs_panel.scroll = logs_panel.scroll.saturating_sub(10);
                        }
                        KeyCode::PageUp => {
                            logs_panel.scroll = logs_panel
                                .scroll
                                .saturating_add(10)
                                .min(imsa_debug_logs.len().saturating_sub(1));
                        }
                        KeyCode::Home => {
                            logs_panel.scroll = imsa_debug_logs.len().saturating_sub(1);
                        }
                        KeyCode::End => {
                            logs_panel.scroll = 0;
                        }
                        KeyCode::Char('c') => {
                            imsa_debug_logs.clear();
                            logs_panel.scroll = 0;
                        }
                        _ => {}
                    }
                    continue;
                }

                if messages_panel.is_open {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('m') => messages_panel.is_open = false,
                        KeyCode::Down | KeyCode::Char('j') if !notices.is_empty() => {
                            messages_panel.selected_idx =
                                (messages_panel.selected_idx + 1) % notices.len();
                        }
                        KeyCode::Up | KeyCode::Char('k') if !notices.is_empty() => {
                            if messages_panel.selected_idx == 0 {
                                messages_panel.selected_idx = notices.len() - 1;
                            } else {
                                messages_panel.selected_idx -= 1;
                            }
                        }
                        KeyCode::Enter | KeyCode::Char('d') if !notices.is_empty() => {
                            let idx = messages_panel.selected_idx.min(notices.len() - 1);
                            let removed = notices.remove(idx);
                            notice_keys.remove(&notice_key(&removed));
                            dismissed_notice_keys.insert(
                                persisted_notice_key(active_series, &removed),
                                now_unix_secs(),
                            );
                            persist_dismissed_notice_keys(
                                &mut config,
                                &mut dismissed_notice_keys,
                                &mut last_error,
                            );
                            highlighted_notice_cars = rebuild_highlighted_notice_cars(&notices);
                            messages_panel.selected_idx = messages_panel
                                .selected_idx
                                .min(notices.len().saturating_sub(1));
                        }
                        KeyCode::Char('c') => {
                            for notice in &notices {
                                dismissed_notice_keys.insert(
                                    persisted_notice_key(active_series, notice),
                                    now_unix_secs(),
                                );
                            }
                            persist_dismissed_notice_keys(
                                &mut config,
                                &mut dismissed_notice_keys,
                                &mut last_error,
                            );
                            notices.clear();
                            notice_keys.clear();
                            highlighted_notice_cars.clear();
                            messages_panel.selected_idx = 0;
                        }
                        KeyCode::Char('C') => {
                            clear_dismissed_notice_keys_for_series(
                                active_series,
                                &mut dismissed_notice_keys,
                            );
                            persist_dismissed_notice_keys(
                                &mut config,
                                &mut dismissed_notice_keys,
                                &mut last_error,
                            );
                        }
                        _ => {}
                    }
                    continue;
                }

                if nls_liveticker_panel.is_open {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('l') => nls_liveticker_panel.is_open = false,
                        KeyCode::Down | KeyCode::Char('j') => {
                            nls_liveticker_panel.scroll = nls_liveticker_panel
                                .scroll
                                .saturating_add(1)
                                .min(liveticker_max_scroll);
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            nls_liveticker_panel.scroll =
                                nls_liveticker_panel.scroll.saturating_sub(1);
                        }
                        KeyCode::PageDown => {
                            nls_liveticker_panel.scroll = nls_liveticker_panel
                                .scroll
                                .saturating_add(10)
                                .min(liveticker_max_scroll);
                        }
                        KeyCode::PageUp => {
                            nls_liveticker_panel.scroll =
                                nls_liveticker_panel.scroll.saturating_sub(10);
                        }
                        KeyCode::Home => nls_liveticker_panel.scroll = 0,
                        KeyCode::End => nls_liveticker_panel.scroll = liveticker_max_scroll,
                        _ => {}
                    }
                    continue;
                }

                match key.code {
                    KeyCode::Char('h') => show_help = !show_help,
                    KeyCode::Char('m') if !show_help => {
                        messages_panel.is_open = !messages_panel.is_open;
                        messages_panel.selected_idx = messages_panel
                            .selected_idx
                            .min(notices.len().saturating_sub(1));
                        logs_panel.is_open = false;
                        series_picker.is_open = false;
                        group_picker.is_open = false;
                        nls_liveticker_panel.is_open = false;
                    }
                    KeyCode::Char('l') if !show_help && active_series == Series::Nls => {
                        nls_liveticker_panel.is_open = !nls_liveticker_panel.is_open;
                        nls_liveticker_panel.scroll = 0;
                        messages_panel.is_open = false;
                        logs_panel.is_open = false;
                        series_picker.is_open = false;
                        group_picker.is_open = false;
                    }
                    KeyCode::Char('L') if !show_help => {
                        logs_panel.is_open = !logs_panel.is_open;
                        if logs_panel.is_open {
                            retain_logs_for_series(&mut imsa_debug_logs, active_series);
                            let entry_count = imsa_debug_logs.len();
                            push_series_debug_log(
                                &mut imsa_debug_logs,
                                format!(
                                    "{} logs panel opened ({entry_count} entries)",
                                    series_log_prefix(active_series)
                                ),
                            );
                            push_series_debug_log(
                                &mut imsa_debug_logs,
                                class_color_source_log_line(active_series, &header, &entries),
                            );
                        }
                        logs_panel.scroll = 0;
                        messages_panel.is_open = false;
                        series_picker.is_open = false;
                        group_picker.is_open = false;
                        nls_liveticker_panel.is_open = false;
                    }
                    KeyCode::Esc => {
                        if show_help {
                            show_help = false;
                        } else {
                            stop_feed(&mut feed);
                            stop_liveticker_feed(&mut nls_liveticker_feed);
                            return Ok(());
                        }
                    }
                    KeyCode::Char('q') => {
                        if show_help {
                            show_help = false;
                        } else {
                            stop_feed(&mut feed);
                            stop_liveticker_feed(&mut nls_liveticker_feed);
                            return Ok(());
                        }
                    }
                    KeyCode::Char('t') if !show_help => {
                        group_picker.is_open = false;
                        messages_panel.is_open = false;
                        nls_liveticker_panel.is_open = false;
                        series_picker.is_open = true;
                        series_picker.selected_idx = selected_series_index(active_series);
                    }
                    KeyCode::Char('G') if !show_help => {
                        messages_panel.is_open = false;
                        nls_liveticker_panel.is_open = false;
                        group_picker.is_open = true;
                        group_picker.selected_idx = match view_mode {
                            ViewMode::Class(idx) => idx.min(current_groups.len().saturating_sub(1)),
                            _ => 0,
                        };
                    }
                    KeyCode::Char('g') if !show_help => {
                        view_mode = next_view_mode(view_mode, current_groups.len());
                        selected_row = 0;
                        gap_anchor_stable_id = None;
                    }
                    KeyCode::Char('o') if !show_help => {
                        view_mode = ViewMode::Overall;
                        selected_row = 0;
                        gap_anchor_stable_id = None;
                    }
                    KeyCode::Down | KeyCode::Char('j') if !show_help => {
                        selected_row = step_selection(selected_row, current_view_entries.len(), 1);
                    }
                    KeyCode::Up | KeyCode::Char('k') if !show_help => {
                        selected_row = step_selection(selected_row, current_view_entries.len(), -1);
                    }
                    KeyCode::PageDown if !show_help => {
                        selected_row = step_selection(selected_row, current_view_entries.len(), 10);
                    }
                    KeyCode::PageUp if !show_help => {
                        selected_row =
                            step_selection(selected_row, current_view_entries.len(), -10);
                    }
                    KeyCode::Home if !show_help => selected_row = 0,
                    KeyCode::End if !show_help => {
                        selected_row = current_view_entries.len().saturating_sub(1)
                    }
                    KeyCode::Char(' ') if !show_help => {
                        if let Some(entry) = current_view_entries.get(selected_row) {
                            let fav_key = favourite_key(active_series, &entry.stable_id);
                            if favourites.contains(&fav_key) {
                                favourites.remove(&fav_key);
                            } else {
                                favourites.insert(fav_key);
                            }
                            config.favourites = favourites.clone();
                            if let Err(err) = save_config(&config) {
                                last_error = Some(err);
                            }
                        }
                    }
                    KeyCode::Char('f') if !show_help && !current_view_entries.is_empty() => {
                        for offset in 1..=current_view_entries.len() {
                            let idx = (selected_row + offset) % current_view_entries.len();
                            let fav_key =
                                favourite_key(active_series, &current_view_entries[idx].stable_id);
                            if favourites.contains(&fav_key) {
                                selected_row = idx;
                                gap_anchor_stable_id =
                                    Some(current_view_entries[idx].stable_id.clone());
                                break;
                            }
                        }
                    }
                    KeyCode::Char('s') if !show_help => {
                        search.query.clear();
                        search.matches.clear();
                        search.current_match = 0;
                        search.input_active = true;
                    }
                    KeyCode::Char('n') if !show_help && !search.matches.is_empty() => {
                        search.current_match = (search.current_match + 1) % search.matches.len();
                        selected_row = search.matches[search.current_match];
                    }
                    KeyCode::Char('p') if !show_help && !search.matches.is_empty() => {
                        if search.current_match == 0 {
                            search.current_match = search.matches.len() - 1;
                        } else {
                            search.current_match -= 1;
                        }
                        selected_row = search.matches[search.current_match];
                    }
                    KeyCode::Char('d') if !show_help => {
                        demo_mode = !demo_mode;
                        last_error = None;
                        gap_anchor_stable_id = None;
                        selected_row = 0;
                        view_mode = ViewMode::Overall;
                        search = SearchState::default();
                        notices.clear();
                        notice_keys.clear();
                        highlighted_notice_cars.clear();
                        message_flag_override = None;
                        message_flag_last_secs = None;
                        messages_panel = MessagesPanelState::closed();
                        nls_liveticker_panel = NlsLivetickerPanelState::closed();

                        if demo_mode {
                            stop_feed(&mut feed);
                            demo_started_at = Instant::now();
                            demo_seed = demo_seed.saturating_add(1);
                            let (next_header, next_entries) =
                                demo::demo_snapshot_at(active_series, demo_seed, 0);
                            header = next_header;
                            entries = next_entries;
                            status = format!("{} demo data", active_series.label());
                            last_update = Some(Instant::now());
                            seed_demo_favourites(active_series, &mut favourites);
                        } else {
                            source_id_ctr += 1;
                            feed = Some(start_feed(active_series, tx.clone(), source_id_ctr));
                            header = TimingHeader::default();
                            entries.clear();
                            status = format!("Starting {} live timing...", active_series.label());
                            last_update = None;
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{feed, grouping};

    fn test_entry(
        position: u32,
        class_name: &str,
        class_rank: &str,
        stable_id: &str,
    ) -> TimingEntry {
        TimingEntry {
            position,
            class_name: class_name.to_string(),
            class_rank: class_rank.to_string(),
            stable_id: stable_id.to_string(),
            ..TimingEntry::default()
        }
    }

    #[test]
    fn grouped_entries_orders_classes_by_best_position() {
        let entries = vec![
            test_entry(5, "GTD", "2", "car-gtd-2"),
            test_entry(1, "GTP", "1", "car-gtp-1"),
            test_entry(3, "GTD", "1", "car-gtd-1"),
        ];

        let grouped = grouping::grouped_entries(&entries, Series::Imsa);

        assert_eq!(grouped.len(), 2);
        assert_eq!(grouped[0].0, "GTP");
        assert_eq!(grouped[1].0, "GTD");
        assert_eq!(grouped[1].1[0].stable_id, "car-gtd-1");
        assert_eq!(grouped[1].1[1].stable_id, "car-gtd-2");
    }

    #[test]
    fn favourites_count_is_scoped_per_series_prefix() {
        let favourites = HashSet::from([
            "imsa|car-1".to_string(),
            "imsa|car-2".to_string(),
            "nls|car-7".to_string(),
            "f1|car-44".to_string(),
            "imsaX|car-invalid".to_string(),
        ]);

        assert_eq!(
            grouping::favourites_count_for_series(Series::Imsa, &favourites),
            2
        );
        assert_eq!(
            grouping::favourites_count_for_series(Series::Nls, &favourites),
            1
        );
        assert_eq!(
            grouping::favourites_count_for_series(Series::F1, &favourites),
            1
        );
    }

    #[test]
    fn header_formatting_normalizes_imsa_labels_and_fallbacks() {
        assert_eq!(
            grouping::display_event_name(Series::Imsa, "  Twelve Hours of Sebring  "),
            "Twelve Hours of Sebring"
        );
        assert_eq!(grouping::display_event_name(Series::Imsa, "-"), "-");
        assert_eq!(grouping::display_session_name(Series::Imsa, "-"), "-");

        assert_eq!(
            grouping::display_session_name(
                Series::Imsa,
                "IMSA WeatherTech SportsCar Championship - Qualifying"
            ),
            "Qualifying"
        );
        assert_eq!(
            grouping::display_session_name(
                Series::Imsa,
                "IMSA WeatherTech SportsCar Championship — Race"
            ),
            "Race"
        );
        assert_eq!(
            grouping::display_session_name(Series::Nls, "  ADAC NLS  "),
            "  ADAC NLS  "
        );
    }

    #[test]
    fn favourite_key_is_series_prefixed_passthrough() {
        assert_eq!(favourite_key(Series::Imsa, "fallback:7"), "imsa|fallback:7");
        assert_eq!(favourite_key(Series::Nls, "stnr:632"), "nls|stnr:632");
        assert_eq!(favourite_key(Series::F1, "f1:driver:12"), "f1|f1:driver:12");
    }

    #[test]
    fn imsa_debug_log_ring_buffer_drops_oldest_lines() {
        let mut logs = VecDeque::new();
        for idx in 0..(IMSA_DEBUG_LOG_CAPACITY + 10) {
            feed::push_series_debug_log(&mut logs, format!("line-{idx}"));
        }

        assert_eq!(logs.len(), IMSA_DEBUG_LOG_CAPACITY);
        assert_eq!(logs.front().map(String::as_str), Some("line-10"));
        let expected_last = format!("line-{}", IMSA_DEBUG_LOG_CAPACITY + 9);
        assert_eq!(
            logs.back().map(String::as_str),
            Some(expected_last.as_str())
        );
    }

    #[test]
    fn extract_notice_car_numbers_collects_hash_numbers() {
        let cars = extract_notice_car_numbers(
            "#999 non respect of code 60 | #155 penalty | reminder | #007 warning",
        );
        assert!(cars.contains("999"));
        assert!(cars.contains("155"));
        assert!(cars.contains("007"));
        assert!(cars.contains("7"));
    }

    #[test]
    fn rebuild_highlighted_notice_cars_aggregates_all_notices() {
        let notices = vec![
            TimingNotice {
                id: "1".to_string(),
                time: "15:04:42".to_string(),
                text: "#999 penalty".to_string(),
            },
            TimingNotice {
                id: "2".to_string(),
                time: "15:04:25".to_string(),
                text: "#155 and #089 warning".to_string(),
            },
        ];

        let highlighted = rebuild_highlighted_notice_cars(&notices);
        assert!(highlighted.contains("999"));
        assert!(highlighted.contains("155"));
        assert!(highlighted.contains("089"));
        assert!(highlighted.contains("89"));
    }

    #[test]
    fn persisted_notice_key_is_series_scoped() {
        let notice = TimingNotice {
            id: "1".to_string(),
            time: "15:04:42".to_string(),
            text: "#999 penalty".to_string(),
        };

        let nls_key = persisted_notice_key(Series::Nls, &notice);
        let dhlm_key = persisted_notice_key(Series::Dhlm, &notice);

        assert_ne!(nls_key, dhlm_key);
        assert!(nls_key.starts_with("nls|"));
        assert!(dhlm_key.starts_with("dhlm|"));
    }

    #[test]
    fn persisted_notice_key_ignores_notice_time_and_id_changes() {
        let first = TimingNotice {
            id: "42".to_string(),
            time: "15:04:42".to_string(),
            text: "#999 penalty".to_string(),
        };
        let updated = TimingNotice {
            id: "97".to_string(),
            time: "15:05:11".to_string(),
            text: "#999    penalty".to_string(),
        };

        let first_key = persisted_notice_key(Series::Nls, &first);
        let updated_key = persisted_notice_key(Series::Nls, &updated);

        assert_eq!(first_key, updated_key);
    }

    #[test]
    fn persisted_notice_key_normalizes_hash_spacing() {
        let with_space = TimingNotice {
            id: "1".to_string(),
            time: "15:00:00".to_string(),
            text: "# 275 non respect of speed limit in pit lane".to_string(),
        };
        let no_space = TimingNotice {
            id: "2".to_string(),
            time: "15:00:10".to_string(),
            text: "#275 non respect of speed limit in pit lane".to_string(),
        };

        let with_space_key = persisted_notice_key(Series::Nls, &with_space);
        let no_space_key = persisted_notice_key(Series::Nls, &no_space);

        assert_eq!(with_space_key, no_space_key);
    }

    #[test]
    fn prune_dismissed_notice_keys_expires_old_entries_and_caps_per_series() {
        let now = 1_000_000_u64;
        let mut dismissed = HashMap::from([
            (
                "nls|old|entry".to_string(),
                now.saturating_sub(DISMISSED_NOTICE_TTL_SECS + 1),
            ),
            ("nls|new-1".to_string(), now - 2),
            ("nls|new-2".to_string(), now - 1),
            ("nls|new-3".to_string(), now),
        ]);

        let changed = prune_dismissed_notice_keys(&mut dismissed, now);

        assert!(changed);
        assert!(!dismissed.contains_key("nls|old|entry"));

        // reduce cap locally by trimming to top timestamps manually expected behavior
        // (real cap is larger, so we emulate overflow with synthetic extra keys)
        for idx in 0..(DISMISSED_NOTICE_MAX_PER_SERIES + 5) {
            dismissed.insert(format!("dhlm|k-{idx}"), now + idx as u64);
        }
        let changed_again = prune_dismissed_notice_keys(&mut dismissed, now + 100);
        assert!(changed_again);
        let dhlm_count = dismissed
            .keys()
            .filter(|key| key.starts_with("dhlm|"))
            .count();
        assert_eq!(dhlm_count, DISMISSED_NOTICE_MAX_PER_SERIES);
    }

    #[test]
    fn clear_dismissed_notice_keys_for_series_only_removes_selected_prefix() {
        let mut dismissed = HashMap::from([
            ("nls|one".to_string(), 1_u64),
            ("nls|two".to_string(), 2_u64),
            ("dhlm|one".to_string(), 3_u64),
        ]);

        clear_dismissed_notice_keys_for_series(Series::Nls, &mut dismissed);

        assert!(!dismissed.contains_key("nls|one"));
        assert!(!dismissed.contains_key("nls|two"));
        assert!(dismissed.contains_key("dhlm|one"));
    }

    #[test]
    fn clear_history_removes_persisted_key_for_current_series() {
        let notice = TimingNotice {
            id: "42".to_string(),
            time: "15:04:42".to_string(),
            text: "#999 penalty".to_string(),
        };
        let nls_key = persisted_notice_key(Series::Nls, &notice);
        let dhlm_key = persisted_notice_key(Series::Dhlm, &notice);
        let mut dismissed = HashMap::from([(nls_key.clone(), 1_u64), (dhlm_key.clone(), 2_u64)]);

        clear_dismissed_notice_keys_for_series(Series::Nls, &mut dismissed);

        assert!(!dismissed.contains_key(&nls_key));
        assert!(dismissed.contains_key(&dhlm_key));
    }

    #[test]
    fn classify_flag_message_ignores_penalty_code_60_messages() {
        let penalty = "#999 non respect of code 60 - time penalty 95 sec after first lap in race";
        assert_eq!(classify_flag_message_intent(penalty), None);
    }

    #[test]
    fn apply_flag_message_notice_tracks_newer_and_clears_on_resume() {
        let mut override_state = None;
        let mut last_secs = None;

        let red = TimingNotice {
            id: "1".to_string(),
            time: "15:04:42".to_string(),
            text: "Red Flag".to_string(),
        };
        apply_flag_message_notice(&red, &mut override_state, &mut last_secs);
        assert_eq!(
            override_state.as_ref().map(|flag| flag.flag.as_str()),
            Some("Red")
        );

        let stale_green = TimingNotice {
            id: "2".to_string(),
            time: "15:04:00".to_string(),
            text: "Green flag".to_string(),
        };
        apply_flag_message_notice(&stale_green, &mut override_state, &mut last_secs);
        assert_eq!(
            override_state.as_ref().map(|flag| flag.flag.as_str()),
            Some("Red")
        );

        let newer_green = TimingNotice {
            id: "3".to_string(),
            time: "15:05:00".to_string(),
            text: "Green flag".to_string(),
        };
        apply_flag_message_notice(&newer_green, &mut override_state, &mut last_secs);
        assert!(override_state.is_none());
        assert_eq!(last_secs, parse_notice_time_seconds("15:05:00"));
    }
}
