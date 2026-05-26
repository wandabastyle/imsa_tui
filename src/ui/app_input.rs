// Keyboard input handling for the TUI application

use crossterm::event::{Event, KeyCode};

use crate::{
   adapters::nls::liveticker::{stop_liveticker_feed, LivetickerFeedKind},
   timing::{Series, TimingEntry},
};

use super::{
   app_init::{class_color_source_log_line, retain_logs_for_series, series_log_prefix},
   app_state::{
       clear_dismissed_notice_keys_for_series, notice_key, persist_dismissed_notice_keys,
       persisted_notice_key, rebuild_highlighted_notice_cars, step_selection,
   },
   config::save_config,
   feed::{push_series_debug_log, stop_feed},
   grouping::{next_view_mode, selected_series_index, view_entries_for_mode, ViewMode},
   pit::refresh_pit_trackers,
   popups::{
       GroupPickerState, LogsPanelState, MessagesPanelState, NlsLivetickerPanelState,
       SeriesPickerState,
   },
   search::refresh_search_matches,
   gap::gap_anchor_from_entry,
};

/// State needed for input handling.
pub struct InputState<'a> {
   pub active_series: &'a mut Series,
   pub demo_mode: &'a mut bool,
   pub demo_started_at: &'a mut std::time::Instant,
   pub demo_seed: &'a mut u64,
   pub feed: &'a mut Option<super::feed::ActiveFeed>,
   pub tx: &'a std::sync::mpsc::Sender<crate::timing::TimingMessage>,
   pub source_id_ctr: &'a mut u64,
   pub header: &'a mut crate::timing::TimingHeader,
   pub entries: &'a mut Vec<TimingEntry>,
   pub status: &'a mut String,
   pub last_error: &'a mut Option<String>,
   pub last_update: &'a mut Option<std::time::Instant>,
   pub selected_row: &'a mut usize,
   pub view_mode: &'a mut ViewMode,
   pub gap_anchor_stable_id: &'a mut Option<String>,
   pub current_view_entries: &'a [TimingEntry],
   pub current_groups: &'a [(String, Vec<TimingEntry>)],
   pub favourites: &'a mut std::collections::HashSet<String>,
   pub config: &'a mut super::config::AppConfig,
   pub show_help: &'a mut bool,
   pub search: &'a mut super::search::SearchState,
   pub series_picker: &'a mut SeriesPickerState,
   pub group_picker: &'a mut GroupPickerState,
   pub logs_panel: &'a mut LogsPanelState,
   pub messages_panel: &'a mut MessagesPanelState,
   pub nls_liveticker_panel: &'a mut NlsLivetickerPanelState,
   pub notices: &'a mut Vec<crate::timing::TimingNotice>,
   pub notice_keys: &'a mut std::collections::HashSet<String>,
   pub dismissed_notice_keys: &'a mut std::collections::HashMap<String, u64>,
   pub highlighted_notice_cars: &'a mut std::collections::HashSet<String>,
   pub message_flag_override: &'a mut Option<super::app_state::MessageFlagOverride>,
   pub message_flag_last_secs: &'a mut Option<u32>,
   pub imsa_debug_logs: &'a mut std::collections::VecDeque<String>,
   pub nls_liveticker_feed: &'a mut Option<crate::adapters::nls::liveticker::ActiveLivetickerFeed>,
   pub nls_liveticker_entries: &'a mut Vec<crate::adapters::nls::liveticker::LivetickerEntry>,
   pub nls_liveticker_last_update: &'a mut Option<std::time::Instant>,
   pub nls_liveticker_last_error: &'a mut Option<String>,
   pub liveticker_max_scroll: usize,
}

/// Handle keyboard input. Returns true if the app should exit.
pub fn handle_input(state: &mut InputState<'_>, event: Event) -> bool {
   let key = if let Event::Key(k) = event {
       k
   } else {
       return false;
   };

   // Handle search input mode
   if state.search.input_active {
       match key.code {
           KeyCode::Esc => {
               state.search.query.clear();
               state.search.matches.clear();
               state.search.current_match = 0;
               state.search.input_active = false;
           }
           KeyCode::Enter => {
               state.search.input_active = false;
               refresh_search_matches(state.search, state.current_view_entries);
               if !state.search.matches.is_empty() {
                   state.search.current_match = 0;
                   *state.selected_row = state.search.matches[0];
               }
           }
           KeyCode::Backspace => {
               state.search.query.pop();
           }
           KeyCode::Char(c) if !c.is_control() => {
               state.search.query.push(c);
           }
           _ => {}
       }
       return false;
   }

   // Handle series picker
   if state.series_picker.is_open {
       let series_list = Series::all();
       match key.code {
           KeyCode::Esc => state.series_picker.is_open = false,
           KeyCode::Down | KeyCode::Char('j') => {
               state.series_picker.selected_idx =
                   (state.series_picker.selected_idx + 1) % series_list.len();
           }
           KeyCode::Up | KeyCode::Char('k') => {
               if state.series_picker.selected_idx == 0 {
                   state.series_picker.selected_idx = series_list.len() - 1;
               } else {
                   state.series_picker.selected_idx -= 1;
               }
           }
           KeyCode::Enter => {
               let next_series = series_list[state.series_picker.selected_idx];
               super::app_state::apply_series_change(next_series, state);
               state.series_picker.is_open = false;
           }
           _ => {}
       }
       return false;
   }

   // Handle group picker
   if state.group_picker.is_open {
       match key.code {
           KeyCode::Esc => state.group_picker.is_open = false,
           KeyCode::Down | KeyCode::Char('j') if !state.current_groups.is_empty() => {
               state.group_picker.selected_idx =
                   (state.group_picker.selected_idx + 1) % state.current_groups.len();
           }
           KeyCode::Up | KeyCode::Char('k') if !state.current_groups.is_empty() => {
               if state.group_picker.selected_idx == 0 {
                   state.group_picker.selected_idx = state.current_groups.len() - 1;
               } else {
                   state.group_picker.selected_idx -= 1;
               }
           }
           KeyCode::Enter => {
               if !state.current_groups.is_empty() {
                   let idx = state.group_picker.selected_idx.min(state.current_groups.len() - 1);
                   *state.view_mode = ViewMode::Class(idx);
                   *state.selected_row = 0;
                   *state.gap_anchor_stable_id = None;
               }
               state.group_picker.is_open = false;
           }
           _ => {}
       }
       return false;
   }

   // Handle logs panel
   if state.logs_panel.is_open {
       match key.code {
           KeyCode::Esc | KeyCode::Char('L') => state.logs_panel.is_open = false,
           KeyCode::Down | KeyCode::Char('j') => {
               state.logs_panel.scroll = state.logs_panel.scroll.saturating_sub(1);
           }
           KeyCode::Up | KeyCode::Char('k') => {
               state.logs_panel.scroll = state
                   .logs_panel
                   .scroll
                   .saturating_add(1)
                   .min(state.imsa_debug_logs.len().saturating_sub(1));
           }
           KeyCode::PageDown => {
               state.logs_panel.scroll = state.logs_panel.scroll.saturating_sub(10);
           }
           KeyCode::PageUp => {
               state.logs_panel.scroll = state
                   .logs_panel
                   .scroll
                   .saturating_add(10)
                   .min(state.imsa_debug_logs.len().saturating_sub(1));
           }
           KeyCode::Home => {
               state.logs_panel.scroll = state.imsa_debug_logs.len().saturating_sub(1);
           }
           KeyCode::End => {
               state.logs_panel.scroll = 0;
           }
           KeyCode::Char('c') => {
               state.imsa_debug_logs.clear();
               state.logs_panel.scroll = 0;
           }
           _ => {}
       }
       return false;
   }

   // Handle messages panel
   if state.messages_panel.is_open {
       match key.code {
           KeyCode::Esc | KeyCode::Char('m') => state.messages_panel.is_open = false,
           KeyCode::Down | KeyCode::Char('j') if !state.notices.is_empty() => {
               state.messages_panel.selected_idx =
                   (state.messages_panel.selected_idx + 1) % state.notices.len();
           }
           KeyCode::Up | KeyCode::Char('k') if !state.notices.is_empty() => {
               if state.messages_panel.selected_idx == 0 {
                   state.messages_panel.selected_idx = state.notices.len() - 1;
               } else {
                   state.messages_panel.selected_idx -= 1;
               }
           }
           KeyCode::Enter | KeyCode::Char('d') if !state.notices.is_empty() => {
               let idx = state.messages_panel.selected_idx.min(state.notices.len() - 1);
               let removed = state.notices.remove(idx);
               state.notice_keys.remove(&notice_key(&removed));
               state.dismissed_notice_keys.insert(
                   persisted_notice_key(*state.active_series, &removed),
                   super::app_state::now_unix_secs(),
               );
               persist_dismissed_notice_keys(
                   state.config,
                   state.dismissed_notice_keys,
                   state.last_error,
               );
               *state.highlighted_notice_cars = rebuild_highlighted_notice_cars(state.notices);
               state.messages_panel.selected_idx = state
                   .messages_panel
                   .selected_idx
                   .min(state.notices.len().saturating_sub(1));
           }
           KeyCode::Char('c') => {
               for notice in state.notices.iter() {
                   state
                       .dismissed_notice_keys
                       .insert(persisted_notice_key(*state.active_series, notice), super::app_state::now_unix_secs());
               }
               persist_dismissed_notice_keys(
                   state.config,
                   state.dismissed_notice_keys,
                   state.last_error,
               );
               state.notices.clear();
               state.notice_keys.clear();
               state.highlighted_notice_cars.clear();
               state.messages_panel.selected_idx = 0;
           }
           KeyCode::Char('C') => {
               clear_dismissed_notice_keys_for_series(*state.active_series, state.dismissed_notice_keys);
               persist_dismissed_notice_keys(
                   state.config,
                   state.dismissed_notice_keys,
                   state.last_error,
               );
           }
           _ => {}
       }
       return false;
   }

   // Handle liveticker panel
   if state.nls_liveticker_panel.is_open {
       match key.code {
           KeyCode::Esc | KeyCode::Char('l') => state.nls_liveticker_panel.is_open = false,
           KeyCode::Down | KeyCode::Char('j') => {
               state.nls_liveticker_panel.scroll = state
                   .nls_liveticker_panel
                   .scroll
                   .saturating_add(1)
                   .min(state.liveticker_max_scroll);
           }
           KeyCode::Up | KeyCode::Char('k') => {
               state.nls_liveticker_panel.scroll =
                   state.nls_liveticker_panel.scroll.saturating_sub(1);
           }
           KeyCode::PageDown => {
               state.nls_liveticker_panel.scroll = state
                   .nls_liveticker_panel
                   .scroll
                   .saturating_add(10)
                   .min(state.liveticker_max_scroll);
           }
           KeyCode::PageUp => {
               state.nls_liveticker_panel.scroll =
                   state.nls_liveticker_panel.scroll.saturating_sub(10);
           }
           KeyCode::Home => state.nls_liveticker_panel.scroll = 0,
           KeyCode::End => state.nls_liveticker_panel.scroll = state.liveticker_max_scroll,
           _ => {}
       }
       return false;
   }

   // Main keyboard shortcuts
   match key.code {
       KeyCode::Char('h') => *state.show_help = !*state.show_help,
       KeyCode::Char('m') if !*state.show_help => {
           state.messages_panel.is_open = !state.messages_panel.is_open;
           state.messages_panel.selected_idx = state
               .messages_panel
               .selected_idx
               .min(state.notices.len().saturating_sub(1));
           state.logs_panel.is_open = false;
           state.series_picker.is_open = false;
           state.group_picker.is_open = false;
           state.nls_liveticker_panel.is_open = false;
       }
       KeyCode::Char('l') if !*state.show_help && *state.active_series == Series::Nls => {
           state.nls_liveticker_panel.is_open = !state.nls_liveticker_panel.is_open;
           state.nls_liveticker_panel.scroll = 0;
           state.messages_panel.is_open = false;
           state.logs_panel.is_open = false;
           state.series_picker.is_open = false;
           state.group_picker.is_open = false;
       }
       KeyCode::Char('L') if !*state.show_help => {
           state.logs_panel.is_open = !state.logs_panel.is_open;
           if state.logs_panel.is_open {
               retain_logs_for_series(state.imsa_debug_logs, *state.active_series);
               let entry_count = state.imsa_debug_logs.len();
               push_series_debug_log(
                   state.imsa_debug_logs,
                   format!(
                       "{} logs panel opened ({entry_count} entries)",
                       series_log_prefix(*state.active_series)
                   ),
               );
               push_series_debug_log(
                   state.imsa_debug_logs,
                   class_color_source_log_line(*state.active_series, state.header, state.entries),
               );
           }
           state.logs_panel.scroll = 0;
           state.messages_panel.is_open = false;
           state.series_picker.is_open = false;
           state.group_picker.is_open = false;
           state.nls_liveticker_panel.is_open = false;
       }
       KeyCode::Esc => {
           if *state.show_help {
               *state.show_help = false;
           } else if !state.search.query.trim().is_empty() || !state.search.matches.is_empty() {
               state.search.query.clear();
               state.search.matches.clear();
               state.search.current_match = 0;
               state.search.input_active = false;
           } else {
               stop_feed(state.feed);
               stop_liveticker_feed(state.nls_liveticker_feed);
               return true;
           }
       }
       KeyCode::Char('q') => {
           if *state.show_help {
               *state.show_help = false;
           } else {
               stop_feed(state.feed);
               stop_liveticker_feed(state.nls_liveticker_feed);
               return true;
           }
       }
       KeyCode::Char('t') if !*state.show_help => {
           state.group_picker.is_open = false;
           state.messages_panel.is_open = false;
           state.nls_liveticker_panel.is_open = false;
           state.series_picker.is_open = true;
           state.series_picker.selected_idx = selected_series_index(*state.active_series);
       }
       KeyCode::Char('G') if !*state.show_help => {
           state.messages_panel.is_open = false;
           state.nls_liveticker_panel.is_open = false;
           state.group_picker.is_open = true;
           state.group_picker.selected_idx = match *state.view_mode {
               ViewMode::Class(idx) => state.current_groups.len().saturating_sub(1).min(idx),
               _ => 0,
           };
       }
       KeyCode::Char('g') if !*state.show_help => {
           *state.view_mode = next_view_mode(*state.view_mode, state.current_groups.len());
           *state.selected_row = 0;
           *state.gap_anchor_stable_id = None;
       }
       KeyCode::Char('o') if !*state.show_help => {
           *state.view_mode = ViewMode::Overall;
           *state.selected_row = 0;
           *state.gap_anchor_stable_id = None;
       }
       KeyCode::Down | KeyCode::Char('j') if !*state.show_help => {
           *state.selected_row =
               step_selection(*state.selected_row, state.current_view_entries.len(), 1);
       }
       KeyCode::Up | KeyCode::Char('k') if !*state.show_help => {
           *state.selected_row =
               step_selection(*state.selected_row, state.current_view_entries.len(), -1);
       }
       KeyCode::PageDown if !*state.show_help => {
           *state.selected_row =
               step_selection(*state.selected_row, state.current_view_entries.len(), 10);
       }
       KeyCode::PageUp if !*state.show_help => {
           *state.selected_row =
               step_selection(*state.selected_row, state.current_view_entries.len(), -10);
       }
       KeyCode::Home if !*state.show_help => *state.selected_row = 0,
       KeyCode::End if !*state.show_help => {
           *state.selected_row = state.current_view_entries.len().saturating_sub(1);
       }
       KeyCode::Char(' ') if !*state.show_help => {
           if let Some(entry) = state.current_view_entries.get(*state.selected_row) {
               let fav_key = super::favourites::favourite_key(*state.active_series, &entry.stable_id);
               if state.favourites.contains(&fav_key) {
                   state.favourites.remove(&fav_key);
               } else {
                   state.favourites.insert(fav_key);
               }
               state.config.favourites.clone_from(state.favourites);
               if let Err(err) = save_config(state.config) {
                   state.last_error.clone_from(&Some(err));
               }
           }
       }
       KeyCode::Char('-') if !*state.show_help && *state.view_mode == ViewMode::Grouped => {
           if state.config.grouped_min_rows > 3 {
               state.config.grouped_min_rows -= 1;
               if let Err(err) = save_config(state.config) {
                   *state.last_error = Some(err);
               }
           }
       }
       KeyCode::Char('+') if !*state.show_help && *state.view_mode == ViewMode::Grouped => {
           if state.config.grouped_min_rows < 20 {
               state.config.grouped_min_rows += 1;
               if let Err(err) = save_config(state.config) {
                   *state.last_error = Some(err);
               }
           }
       }
       KeyCode::Char('f') if !*state.show_help && !state.current_view_entries.is_empty() => {
           for offset in 1..=state.current_view_entries.len() {
               let idx = (*state.selected_row + offset) % state.current_view_entries.len();
               let fav_key =
                   super::favourites::favourite_key(*state.active_series, &state.current_view_entries[idx].stable_id);
               if state.favourites.contains(&fav_key) {
                   *state.selected_row = idx;
                   *state.gap_anchor_stable_id = Some(state.current_view_entries[idx].stable_id.clone());
                   break;
               }
           }
       }
       KeyCode::Char('s') if !*state.show_help => {
           state.search.query.clear();
           state.search.matches.clear();
           state.search.current_match = 0;
           state.search.input_active = true;
       }
       KeyCode::Char('n') if !*state.show_help && !state.search.matches.is_empty() => {
           state.search.current_match = (state.search.current_match + 1) % state.search.matches.len();
           *state.selected_row = state.search.matches[state.search.current_match];
       }
       KeyCode::Char('p') if !*state.show_help && !state.search.matches.is_empty() => {
           if state.search.current_match == 0 {
               state.search.current_match = state.search.matches.len() - 1;
           } else {
               state.search.current_match -= 1;
           }
           *state.selected_row = state.search.matches[state.search.current_match];
       }
       KeyCode::Char('d') if !*state.show_help => {
           *state.demo_mode = !*state.demo_mode;
           *state.last_error = None;
           *state.gap_anchor_stable_id = None;
           *state.selected_row = 0;
           *state.view_mode = ViewMode::Overall;
           *state.search = super::search::SearchState::default();
           state.notices.clear();
           state.notice_keys.clear();
           state.highlighted_notice_cars.clear();
           *state.message_flag_override = None;
           *state.message_flag_last_secs = None;
           state.messages_panel.is_open = false;
           state.nls_liveticker_panel.is_open = false;

           if *state.demo_mode {
               stop_feed(state.feed);
               *state.demo_started_at = std::time::Instant::now();
               *state.demo_seed = state.demo_seed.saturating_add(1);
               let (next_header, next_entries) =
                   crate::demo::demo_snapshot_at(*state.active_series, *state.demo_seed, 0);
               *state.header = next_header;
               *state.entries = next_entries;
               *state.status = format!("{} demo data", state.active_series.label());
               *state.last_update = Some(std::time::Instant::now());
               crate::demo::seed_demo_favourites(*state.active_series, state.favourites);
           } else {
               *state.source_id_ctr += 1;
               *state.feed = Some(super::feed::start_feed(*state.active_series, state.tx.clone(), *state.source_id_ctr));
               *state.header = crate::timing::TimingHeader::default();
               state.entries.clear();
               *state.status = format!("Starting {} live timing...", state.active_series.label());
               *state.last_update = None;
           }
       }
       _ => {}
   }

   false
}
