// Interactive TUI state machine:
// - consumes worker messages
// - derives view/group/search/favourite projections
// - renders one frame
// - handles one keyboard event

use std::{
    collections::{HashMap, HashSet, VecDeque},
    io,
    sync::mpsc::{self, Sender},
    time::{Duration, Instant},
};

use crossterm::event::{self, Event, KeyCode};
use ratatui::{backend::CrosstermBackend, Terminal};

use super::{
    config::{load_config, save_config, AppConfig},
    feed::{
        drain_messages, drain_series_debug_logs, start_feed, stop_feed, ActiveFeed,
        IMSA_DEBUG_LOG_CAPACITY,
    },
    gap::gap_anchor_from_entry,
    grouping::{
        grouped_entries, next_view_mode, selected_series_index, view_entries_for_mode, ViewMode,
    },
    imsa_widths::{init_imsa_widths_baseline, save_imsa_column_widths_baseline, ImsaColumnWidths},
    pit::{refresh_pit_trackers, PitTracker},
    popups::{GroupPickerState, LogsPanelState, SeriesPickerState},
    render::{draw_frame, RenderCtx},
    search::{refresh_search_matches, SearchState},
};

use crate::demo;
use crate::{
    favourites,
    timing::{Series, TimingEntry, TimingHeader, TimingMessage},
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
    last_error: &'a mut Option<String>,
    last_update: &'a mut Option<Instant>,
    selected_row: &'a mut usize,
    view_mode: &'a mut ViewMode,
    search: &'a mut SearchState,
    config: &'a mut AppConfig,
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

    ctx.config.selected_series = *ctx.active_series;
    if let Err(err) = save_config(ctx.config) {
        *ctx.last_error = Some(err);
    }
}

pub fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    let (tx, rx) = mpsc::channel::<TimingMessage>();
    let tick_rate = Duration::from_millis(250);

    let mut config = load_config();
    let mut active_series = config.selected_series;
    let mut source_id_ctr = 1_u64;
    let mut demo_mode = false;
    let mut demo_started_at = Instant::now();
    let mut demo_seed = 1_u64;
    let mut feed = Some(start_feed(active_series, tx.clone(), source_id_ctr));

    let (mut header, mut entries) = (TimingHeader::default(), Vec::new());
    let mut status = format!("Starting {} live timing...", active_series.label());
    let mut last_error: Option<String> = None;
    let mut last_update: Option<Instant> = None;
    let mut previous_flag = "-".to_string();
    let mut transition_started_at = Instant::now();
    let mut view_mode = ViewMode::Overall;
    let mut selected_row = 0usize;
    let mut favourites: HashSet<String> = config.favourites.clone();
    let mut show_help = false;
    let mut search = SearchState::default();
    let mut series_picker = SeriesPickerState::closed();
    let mut group_picker = GroupPickerState::closed();
    let mut logs_panel = LogsPanelState::closed();
    let mut imsa_debug_logs = VecDeque::new();
    let mut gap_anchor_stable_id: Option<String> = None;
    let mut pit_trackers: HashMap<String, PitTracker> = HashMap::new();
    let mut imsa_width_baseline = init_imsa_widths_baseline();
    let mut imsa_live_baseline_saved = false;
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
            drain_messages(
                &rx,
                active_feed.source_id,
                &mut header,
                &mut entries,
                &mut status,
                &mut last_error,
                &mut last_update,
            );
        }
        drain_series_debug_logs(&feed, &mut imsa_debug_logs);

        if !demo_mode && active_series == Series::Imsa && !imsa_live_baseline_saved {
            if let Some(observed_live) = ImsaColumnWidths::from_entries(&entries) {
                let merged = match imsa_width_baseline {
                    Some(existing) => existing.merge_keep_larger(observed_live),
                    None => observed_live,
                };
                save_imsa_column_widths_baseline(&merged);
                imsa_width_baseline = Some(merged);
                imsa_live_baseline_saved = true;
            }
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

        let effective_flag = if header.flag.is_empty() {
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
                imsa_width_baseline: imsa_width_baseline.as_ref(),
                now,
                view_mode,
                search: &search,
                show_help,
                series_picker,
                group_picker,
                logs_panel,
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
                                last_error: &mut last_error,
                                last_update: &mut last_update,
                                selected_row: &mut selected_row,
                                view_mode: &mut view_mode,
                                search: &mut search,
                                config: &mut config,
                            };
                            apply_series_change(next_series, &mut series_change_ctx);
                            gap_anchor_stable_id = None;
                            series_picker.is_open = false;
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

                match key.code {
                    KeyCode::Char('h') => show_help = !show_help,
                    KeyCode::Char('L') if !show_help => {
                        logs_panel.is_open = !logs_panel.is_open;
                        logs_panel.scroll = 0;
                        series_picker.is_open = false;
                        group_picker.is_open = false;
                    }
                    KeyCode::Esc => {
                        if show_help {
                            show_help = false;
                        } else {
                            stop_feed(&mut feed);
                            return Ok(());
                        }
                    }
                    KeyCode::Char('q') => {
                        if show_help {
                            show_help = false;
                        } else {
                            stop_feed(&mut feed);
                            return Ok(());
                        }
                    }
                    KeyCode::Char('t') if !show_help => {
                        group_picker.is_open = false;
                        series_picker.is_open = true;
                        series_picker.selected_idx = selected_series_index(active_series);
                    }
                    KeyCode::Char('G') if !show_help => {
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
}
