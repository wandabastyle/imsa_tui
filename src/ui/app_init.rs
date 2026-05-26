// Initialization functions for the TUI application

use std::{
   collections::{HashMap, HashSet, VecDeque},
   sync::mpsc::{self, Sender},
   time::{Duration, Instant},
};

use ratatui::{
   backend::CrosstermBackend,
   Terminal,
};

use crate::{
   adapters::nls::liveticker::{
      start_liveticker_feed, ActiveLivetickerFeed, LivetickerEntry, LivetickerFeedKind,
   },
   demo,
   timing::{Series, TimingHeader, TimingMessage, TimingEntry},
};

use super::{
   app_state::{now_unix_secs, prune_dismissed_notice_keys},
   config::{load_config, save_config, AppConfig},
   feed::{start_feed, ActiveFeed, IMSA_DEBUG_LOG_CAPACITY},
   grouping::ViewMode,
   popups::NlsLivetickerPanelState,
   search::SearchState,
   width_state::SeriesWidthBaselines,
};

/// Application state container - holds all state needed by run_app.
pub struct AppInit {
   pub tx: Sender<TimingMessage>,
   pub rx: mpsc::Receiver<TimingMessage>,
   pub tick_rate: Duration,
   pub config: AppConfig,
   pub config_load_error: Option<String>,
   pub active_series: Series,
   pub source_id_ctr: u64,
   pub demo_mode: bool,
   pub demo_started_at: Instant,
   pub demo_seed: u64,
   pub feed: Option<ActiveFeed>,
   pub header: TimingHeader,
   pub entries: Vec<TimingEntry>,
   pub status: String,
   pub last_error: Option<String>,
   pub last_update: Option<Instant>,
   pub previous_flag: String,
   pub transition_started_at: Instant,
   pub view_mode: ViewMode,
   pub selected_row: usize,
   pub favourites: HashSet<String>,
   pub dismissed_notice_keys: HashMap<String, u64>,
   pub show_help: bool,
   pub search: SearchState,
   pub series_picker_open: bool,
   pub series_picker_idx: usize,
   pub group_picker_open: bool,
   pub group_picker_idx: usize,
   pub logs_panel_open: bool,
   pub logs_panel_scroll: usize,
   pub messages_panel_open: bool,
   pub messages_panel_idx: usize,
   pub nls_liveticker_panel_open: bool,
   pub nls_liveticker_panel_scroll: usize,
   pub notices: Vec<TimingNotice>,
   pub nls_liveticker_entries: Vec<LivetickerEntry>,
   pub nls_liveticker_last_update: Option<Instant>,
   pub nls_liveticker_last_error: Option<String>,
   pub nls_liveticker_feed: Option<ActiveLivetickerFeed>,
   pub notice_keys: HashSet<String>,
   pub highlighted_notice_cars: HashSet<String>,
   pub message_flag_override: Option<super::app_state::MessageFlagOverride>,
   pub message_flag_last_secs: Option<u32>,
   pub imsa_debug_logs: VecDeque<String>,
   pub gap_anchor_stable_id: Option<String>,
   pub pit_trackers: HashMap<String, super::pit::PitTracker>,
   pub width_baselines: SeriesWidthBaselines,
   pub ui_started_at: Instant,
}

impl AppInit {
   /// Initialize the application state.
   /// 
   /// # Errors
   /// Returns an IO error if terminal operations fail.
   pub fn new() -> std::io::Result<Self> {
      let (tx, rx) = mpsc::channel::<TimingMessage>();
      let tick_rate = Duration::from_millis(250);

      let mut config = load_config();
      let mut config_load_error = None;
      if prune_dismissed_notice_keys(&mut config.dismissed_notice_keys, now_unix_secs()) {
         if let Err(err) = save_config(&config) {
            config_load_error = Some(err);
         }
      }

      let active_series = config.selected_series;
      let source_id_ctr = 1_u64;
      let demo_mode = false;
      let demo_started_at = Instant::now();
      let demo_seed = 1_u64;
      let feed = Some(start_feed(active_series, tx.clone(), source_id_ctr));

      let (header, entries) = (TimingHeader::default(), Vec::new());
      let status = format!("Starting {} live timing...", active_series.label());
      let last_error: Option<String> = config_load_error.clone();
      let last_update: Option<Instant> = None;
      let previous_flag = "-".to_string();
      let transition_started_at = Instant::now();
      let view_mode = ViewMode::Overall;
      let selected_row = 0usize;
      let favourites: HashSet<String> = config.favourites.clone();
      let dismissed_notice_keys: HashMap<String, u64> = config.dismissed_notice_keys.clone();
      let show_help = false;
      let search = SearchState::default();
      let series_picker_open = false;
      let series_picker_idx = 0usize;
      let group_picker_open = false;
      let group_picker_idx = 0usize;
      let logs_panel_open = false;
      let logs_panel_scroll = 0usize;
      let messages_panel_open = false;
      let messages_panel_idx = 0usize;
      let nls_liveticker_panel_open = false;
      let nls_liveticker_panel_scroll = 0usize;
      let notices: Vec<TimingNotice> = Vec::new();
      let nls_liveticker_entries: Vec<LivetickerEntry> = Vec::new();
      let nls_liveticker_last_update: Option<Instant> = None;
      let nls_liveticker_last_error: Option<String> = None;
      let nls_liveticker_feed = if active_series == Series::Nls {
         let initial_kind = if header.event_id == "50" {
            LivetickerFeedKind::N24
         } else {
            LivetickerFeedKind::Nls
         };
         Some(start_liveticker_feed(initial_kind))
      } else {
         None
      };
      let notice_keys: HashSet<String> = HashSet::new();
      let highlighted_notice_cars: HashSet<String> = HashSet::new();
      let message_flag_override: Option<super::app_state::MessageFlagOverride> = None;
      let message_flag_last_secs: Option<u32> = None;
      let imsa_debug_logs = VecDeque::new();
      let gap_anchor_stable_id: Option<String> = None;
      let pit_trackers: HashMap<String, super::pit::PitTracker> = HashMap::new();
      let width_baselines = SeriesWidthBaselines::load();
      let ui_started_at = Instant::now();

      Ok(Self {
         tx,
         rx,
         tick_rate,
         config,
         config_load_error,
         active_series,
         source_id_ctr,
         demo_mode,
         demo_started_at,
         demo_seed,
         feed,
         header,
         entries,
         status,
         last_error,
         last_update,
         previous_flag,
         transition_started_at,
         view_mode,
         selected_row,
         favourites,
         dismissed_notice_keys,
         show_help,
         search,
         series_picker_open,
         series_picker_idx,
         group_picker_open,
         group_picker_idx,
         logs_panel_open,
         logs_panel_scroll,
         messages_panel_open,
         messages_panel_idx,
         nls_liveticker_panel_open,
         nls_liveticker_panel_scroll,
         notices,
         nls_liveticker_entries,
         nls_liveticker_last_update,
         nls_liveticker_last_error,
         nls_liveticker_feed,
         notice_keys,
         highlighted_notice_cars,
         message_flag_override,
         message_flag_last_secs,
         imsa_debug_logs,
         gap_anchor_stable_id,
         pit_trackers,
         width_baselines,
         ui_started_at,
      })
   }
}

/// Demo snapshot helper function.
pub fn demo_snapshot(series: Series) -> (TimingHeader, Vec<TimingEntry>) {
   demo::demo_snapshot(series)
}

/// Seed demo favourites helper function.
pub fn seed_demo_favourites(series: Series, favourites: &mut HashSet<String>) {
   demo::seed_demo_favourites(series, favourites);
}

/// Series log prefix helper function.
pub fn series_log_prefix(series: Series) -> String {
   format!("[{}]", series.label())
}

/// Retain logs for series helper function.
pub fn retain_logs_for_series(logs: &mut VecDeque<String>, series: Series) {
   let prefix = series_log_prefix(series);
   logs.retain(|line| line.starts_with(&prefix));
}

/// Class color source log line generation.
pub fn class_color_source_log_line(
   series: Series,
   header: &TimingHeader,
   entries: &[TimingEntry],
) -> String {
   use std::collections::BTreeSet;

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
