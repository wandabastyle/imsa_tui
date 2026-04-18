// Interactive TUI state machine:
// - consumes worker messages
// - derives view/group/search/favourite projections
// - renders one frame
// - handles one keyboard event

use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    fs, io,
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant},
};

use crossterm::event::{self, Event, KeyCode};
use directories::ProjectDirs;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Wrap},
    Terminal,
};
use serde::{Deserialize, Serialize};

use crate::demo;
use crate::{
    adapters::wec::websocket_worker_with_debug as wec_websocket_worker,
    f1::signalr_worker_with_debug,
    favourites,
    imsa::{normalize_class_name, polling_worker_with_debug},
    nls::websocket_worker_with_debug,
    timing::{Series, TimingClassColor, TimingEntry, TimingHeader, TimingMessage},
    timing_persist::SeriesDebugOutput,
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct AppConfig {
    favourites: HashSet<String>,
    #[serde(default)]
    selected_series: Series,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct ImsaColumnWidths {
    pos: u16,
    car_number: u16,
    class: u16,
    pic: u16,
    driver: u16,
    vehicle: u16,
    laps: u16,
    gap_o: u16,
    gap_c: u16,
    next_c: u16,
    last: u16,
    best: u16,
    bl: u16,
    pit: u16,
    stop: u16,
    fastest: u16,
}

impl ImsaColumnWidths {
    const fn header_minimums() -> Self {
        Self {
            pos: 3,
            car_number: 1,
            class: 5,
            pic: 3,
            driver: 6,
            vehicle: 7,
            laps: 4,
            gap_o: 5,
            gap_c: 5,
            next_c: 6,
            last: 4,
            best: 4,
            bl: 3,
            pit: 3,
            stop: 4,
            fastest: 14,
        }
    }

    fn from_entries(entries: &[TimingEntry]) -> Option<Self> {
        if entries.is_empty() {
            return None;
        }

        let pos = entries
            .iter()
            .map(|entry| entry.position.to_string().chars().count())
            .max()
            .unwrap_or(1) as u16;

        Some(Self {
            pos,
            car_number: max_text_width(entries, |entry| &entry.car_number),
            class: max_text_width(entries, |entry| &entry.class_name),
            pic: max_text_width(entries, |entry| &entry.class_rank),
            driver: max_text_width(entries, |entry| &entry.driver),
            vehicle: max_text_width(entries, |entry| &entry.vehicle),
            laps: max_text_width(entries, |entry| &entry.laps),
            gap_o: max_text_width(entries, |entry| &entry.gap_overall),
            gap_c: max_text_width(entries, |entry| &entry.gap_class),
            next_c: max_text_width(entries, |entry| &entry.gap_next_in_class),
            last: max_text_width(entries, |entry| &entry.last_lap),
            best: max_text_width(entries, |entry| &entry.best_lap),
            bl: max_text_width(entries, |entry| &entry.best_lap_no),
            pit: max_text_width(entries, |entry| &entry.pit),
            stop: max_text_width(entries, |entry| &entry.pit_stops),
            fastest: max_text_width(entries, |entry| &entry.fastest_driver),
        })
    }

    fn merge_keep_larger(self, other: Self) -> Self {
        Self {
            pos: self.pos.max(other.pos),
            car_number: self.car_number.max(other.car_number),
            class: self.class.max(other.class),
            pic: self.pic.max(other.pic),
            driver: self.driver.max(other.driver),
            vehicle: self.vehicle.max(other.vehicle),
            laps: self.laps.max(other.laps),
            gap_o: self.gap_o.max(other.gap_o),
            gap_c: self.gap_c.max(other.gap_c),
            next_c: self.next_c.max(other.next_c),
            last: self.last.max(other.last),
            best: self.best.max(other.best),
            bl: self.bl.max(other.bl),
            pit: self.pit.max(other.pit),
            stop: self.stop.max(other.stop),
            fastest: self.fastest.max(other.fastest),
        }
    }

    fn enforce_header_minimums(self) -> Self {
        let mins = Self::header_minimums();
        Self {
            pos: self.pos.max(mins.pos),
            car_number: self.car_number.max(mins.car_number),
            class: self.class.max(mins.class),
            pic: self.pic.max(mins.pic),
            driver: self.driver.max(mins.driver),
            vehicle: self.vehicle.max(mins.vehicle),
            laps: self.laps.max(mins.laps),
            gap_o: self.gap_o.max(mins.gap_o),
            gap_c: self.gap_c.max(mins.gap_c),
            next_c: self.next_c.max(mins.next_c),
            last: self.last.max(mins.last),
            best: self.best.max(mins.best),
            bl: self.bl.max(mins.bl),
            pit: self.pit.max(mins.pit),
            stop: self.stop.max(mins.stop),
            fastest: self.fastest.max(mins.fastest),
        }
    }

    fn to_array(self) -> [u16; 16] {
        [
            self.pos,
            self.car_number,
            self.class,
            self.pic,
            self.driver,
            self.vehicle,
            self.laps,
            self.gap_o,
            self.gap_c,
            self.next_c,
            self.last,
            self.best,
            self.bl,
            self.pit,
            self.stop,
            self.fastest,
        ]
    }

    fn from_array(values: [u16; 16]) -> Self {
        Self {
            pos: values[0],
            car_number: values[1],
            class: values[2],
            pic: values[3],
            driver: values[4],
            vehicle: values[5],
            laps: values[6],
            gap_o: values[7],
            gap_c: values[8],
            next_c: values[9],
            last: values[10],
            best: values[11],
            bl: values[12],
            pit: values[13],
            stop: values[14],
            fastest: values[15],
        }
    }

    fn driver_width(self) -> usize {
        self.driver as usize
    }

    fn vehicle_width(self) -> usize {
        self.vehicle as usize
    }

    fn fastest_width(self) -> usize {
        self.fastest as usize
    }
}

#[derive(Debug, Clone, Deserialize)]
struct PersistedImsaSnapshotStub {
    entries: Vec<TimingEntry>,
}

fn max_text_width<F>(entries: &[TimingEntry], accessor: F) -> u16
where
    F: Fn(&TimingEntry) -> &str,
{
    entries
        .iter()
        .map(|entry| accessor(entry).chars().count())
        .max()
        .unwrap_or(1) as u16
}

#[derive(Debug)]
struct ActiveFeed {
    source_id: u64,
    stop_tx: Sender<()>,
    debug_rx: Option<Receiver<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewMode {
    Overall,
    Grouped,
    Class(usize),
    Favourites,
}

#[derive(Debug, Clone, Default)]
struct SearchState {
    query: String,
    matches: Vec<usize>,
    current_match: usize,
    input_active: bool,
}

#[derive(Debug, Clone, Copy)]
struct SeriesPickerState {
    is_open: bool,
    selected_idx: usize,
}

impl SeriesPickerState {
    fn closed() -> Self {
        Self {
            is_open: false,
            selected_idx: 0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct GroupPickerState {
    is_open: bool,
    selected_idx: usize,
}

#[derive(Debug, Clone, Copy)]
struct LogsPanelState {
    is_open: bool,
    scroll: usize,
}

impl LogsPanelState {
    fn closed() -> Self {
        Self {
            is_open: false,
            scroll: 0,
        }
    }
}

const IMSA_DEBUG_LOG_CAPACITY: usize = 150;

impl GroupPickerState {
    fn closed() -> Self {
        Self {
            is_open: false,
            selected_idx: 0,
        }
    }
}

struct TableRenderCtx<'a> {
    favourites: &'a HashSet<String>,
    marked_stable_id: Option<&'a str>,
    active_series: Series,
    selected_row_in_view: Option<usize>,
    marquee_tick: usize,
    gap_anchor: Option<&'a GapAnchorInfo>,
    pit_trackers: &'a HashMap<String, PitTracker>,
    class_colors: &'a BTreeMap<String, TimingClassColor>,
    now: Instant,
}

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

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

fn help_popup() -> Paragraph<'static> {
    let text = vec![
        Line::from(vec![Span::styled(
            "Keyboard Help",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from("h      toggle help"),
        Line::from("g      cycle views"),
        Line::from("G      open group selector popup"),
        Line::from("o      switch to overall view"),
        Line::from("t      open series selector popup"),
        Line::from("↑/↓    move selection"),
        Line::from("PgUp/PgDn  fast scroll"),
        Line::from("space  toggle favourite for selected car"),
        Line::from("f      jump to next favourite in current view"),
        Line::from("s      search by car #, driver, or team"),
        Line::from("n/p    next/prev search result"),
        Line::from("d      toggle demo/live data source"),
        Line::from("L      toggle IMSA debug logs"),
        Line::from("q      quit"),
        Line::from("Enter  confirm popup selection"),
        Line::from("Esc    close popup/help / quit app"),
        Line::from(""),
        Line::from("Press h or Esc to close this popup."),
    ];

    Paragraph::new(text)
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: false })
        .block(Block::default().title("Help").borders(Borders::ALL))
}

fn series_picker_popup(active_series: Series, selected_idx: usize) -> Paragraph<'static> {
    let mut lines = vec![
        Line::from(vec![Span::styled(
            "Select Series",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
    ];

    for (idx, series) in Series::all().iter().copied().enumerate() {
        let marker = if idx == selected_idx { ">" } else { " " };
        let current = if series == active_series {
            " (current)"
        } else {
            ""
        };
        let style = if idx == selected_idx {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        lines.push(Line::from(vec![Span::styled(
            format!("{marker} {}{current}", series.label()),
            style,
        )]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(
        "Use ↑/↓ to choose, Enter to switch, Esc to cancel.",
    ));

    Paragraph::new(lines)
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: false })
        .block(Block::default().title("Series").borders(Borders::ALL))
}

fn group_picker_popup(groups: &[String], selected_idx: usize) -> Paragraph<'static> {
    let mut lines = vec![
        Line::from(vec![Span::styled(
            "Select Group",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
    ];

    if groups.is_empty() {
        lines.push(Line::from("No groups available for current series."));
    } else {
        for (idx, group_name) in groups.iter().enumerate() {
            let marker = if idx == selected_idx { ">" } else { " " };
            let style = if idx == selected_idx {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            lines.push(Line::from(vec![Span::styled(
                format!("{marker} {group_name}"),
                style,
            )]));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(
        "Use ↑/↓ to choose, Enter to open class view, Esc to cancel.",
    ));

    Paragraph::new(lines)
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: false })
        .block(Block::default().title("Group").borders(Borders::ALL))
}

fn config_path() -> Option<PathBuf> {
    let dirs = ProjectDirs::from("", "", "imsa_tui")?;
    Some(dirs.config_dir().join("config.toml"))
}

fn load_config() -> AppConfig {
    let Some(path) = config_path() else {
        return AppConfig::default();
    };

    let Ok(text) = fs::read_to_string(path) else {
        return AppConfig::default();
    };

    toml::from_str::<AppConfig>(&text).unwrap_or_default()
}

fn save_config(config: &AppConfig) -> Result<(), String> {
    let Some(path) = config_path() else {
        return Err("unable to resolve platform config directory".to_string());
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create config directory failed: {e}"))?;
    }

    let mut filtered = config.clone();
    filtered.favourites = favourites::normalize_favourites(filtered.favourites);
    let encoded =
        toml::to_string_pretty(&filtered).map_err(|e| format!("encode config failed: {e}"))?;
    fs::write(path, encoded).map_err(|e| format!("write config failed: {e}"))
}

fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    let t = t.clamp(0.0, 1.0);
    ((a as f32) + ((b as f32) - (a as f32)) * t).round() as u8
}

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    match (a, b) {
        (Color::Rgb(ar, ag, ab), Color::Rgb(br, bg, bb)) => {
            Color::Rgb(lerp_u8(ar, br, t), lerp_u8(ag, bg, t), lerp_u8(ab, bb, t))
        }
        _ => b,
    }
}

fn base_flag_colors(flag: &str) -> (String, Color, Color, bool) {
    match flag.trim().to_ascii_lowercase().as_str() {
        "green" | "normal" => (
            "Green".to_string(),
            Color::Rgb(0, 153, 68),
            Color::Black,
            false,
        ),
        "yellow" => (
            "Yellow".to_string(),
            Color::Rgb(255, 221, 0),
            Color::Black,
            true,
        ),
        "red" => (
            "Red".to_string(),
            Color::Rgb(200, 16, 46),
            Color::White,
            false,
        ),
        "checkered" | "chequered" => (
            "Checkered".to_string(),
            Color::Rgb(245, 245, 245),
            Color::Black,
            false,
        ),
        "-" | "" => (
            "Green".to_string(),
            Color::Rgb(0, 153, 68),
            Color::Black,
            false,
        ),
        other => (
            other.to_string(),
            Color::Rgb(0, 153, 68),
            Color::Black,
            false,
        ),
    }
}

fn animated_flag_theme(
    flag: &str,
    previous_flag: &str,
    transition_started_at: Instant,
) -> (String, Style, Style) {
    let (flag_text, target_bg, target_fg, _) = base_flag_colors(flag);
    let (_, previous_bg, _, _) = base_flag_colors(previous_flag);

    let transition_t = (transition_started_at.elapsed().as_millis() as f32 / 450.0).clamp(0.0, 1.0);
    let bg = lerp_color(previous_bg, target_bg, transition_t);

    let header_style = Style::default().fg(target_fg).bg(bg);
    let flag_span_style = header_style.add_modifier(Modifier::BOLD);

    (flag_text, flag_span_style, header_style)
}

fn class_style(
    class_name: &str,
    active_series: Series,
    class_colors: &BTreeMap<String, TimingClassColor>,
) -> Style {
    if active_series == Series::Wec {
        let key = normalize_class_key(class_name);
        if let Some(color) = class_colors.get(&key) {
            if let Some(fg) = parse_hex_color(&color.foreground) {
                return Style::default().fg(fg).add_modifier(Modifier::BOLD);
            }
        }
        return class_style_wec_static(&key);
    }

    match normalize_class_name(class_name).as_str() {
        "GTP" => Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
        "LMP2" => Style::default()
            .fg(Color::Rgb(63, 144, 218))
            .add_modifier(Modifier::BOLD),
        "GTDPRO" => Style::default()
            .fg(Color::Rgb(210, 38, 48))
            .add_modifier(Modifier::BOLD),
        "GTD" => Style::default()
            .fg(Color::Rgb(0, 166, 81))
            .add_modifier(Modifier::BOLD),
        "SP9" => Style::default()
            .fg(Color::Rgb(255, 140, 0))
            .add_modifier(Modifier::BOLD),
        "LMH" => Style::default()
            .fg(Color::Rgb(220, 20, 60))
            .add_modifier(Modifier::BOLD),
        "LMGT3" => Style::default()
            .fg(Color::Rgb(30, 144, 255))
            .add_modifier(Modifier::BOLD),
        _ => Style::default(),
    }
}

fn normalize_class_key(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_whitespace() && *ch != '_' && *ch != '-')
        .collect::<String>()
        .to_ascii_uppercase()
}

fn parse_hex_color(value: &str) -> Option<Color> {
    let trimmed = value.trim();
    if trimmed.len() != 7 || !trimmed.starts_with('#') {
        return None;
    }
    let r = u8::from_str_radix(&trimmed[1..3], 16).ok()?;
    let g = u8::from_str_radix(&trimmed[3..5], 16).ok()?;
    let b = u8::from_str_radix(&trimmed[5..7], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}

fn class_style_wec_static(class_key: &str) -> Style {
    match class_key {
        "LMH" => Style::default()
            .fg(Color::Rgb(220, 20, 60))
            .add_modifier(Modifier::BOLD),
        "LMGT3" => Style::default()
            .fg(Color::Rgb(30, 144, 255))
            .add_modifier(Modifier::BOLD),
        "LMP1" => Style::default()
            .fg(Color::Rgb(255, 16, 83))
            .add_modifier(Modifier::BOLD),
        "LMP2" => Style::default()
            .fg(Color::Rgb(63, 144, 218))
            .add_modifier(Modifier::BOLD),
        "LMGTE" => Style::default()
            .fg(Color::Rgb(255, 169, 18))
            .add_modifier(Modifier::BOLD),
        "INV" => Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
        _ => Style::default(),
    }
}

fn class_display_name(name: &str) -> String {
    let normalized = normalize_class_name(name);
    match normalized.as_str() {
        "GTP" => "GTP".to_string(),
        "LMP2" => "LMP2".to_string(),
        "LMP1" => "LMP1".to_string(),
        "LMGTE" => "LMGTE".to_string(),
        "INV" => "INV".to_string(),
        "GTDPRO" => "GTD PRO".to_string(),
        "GTD" => "GTD".to_string(),
        "LMH" => "LMH".to_string(),
        "LMGT3" => "LMGT3".to_string(),
        _ => {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                "-".to_string()
            } else {
                trimmed.to_string()
            }
        }
    }
}

fn view_mode_text(view_mode: ViewMode, group_names: &[String]) -> String {
    match view_mode {
        ViewMode::Overall => "Overall".to_string(),
        ViewMode::Grouped => "Grouped".to_string(),
        ViewMode::Favourites => "Favourites".to_string(),
        ViewMode::Class(idx) => {
            if let Some(name) = group_names.get(idx) {
                format!("Class {name}")
            } else {
                "Class".to_string()
            }
        }
    }
}

const IMSA_COLUMN_COUNT: usize = 16;

fn imsa_column_widths_path() -> Option<PathBuf> {
    let dirs = ProjectDirs::from("", "", "imsa_tui")?;
    Some(dirs.data_local_dir().join("imsa_column_widths.json"))
}

fn imsa_snapshot_dump_path() -> Option<PathBuf> {
    let dirs = ProjectDirs::from("", "", "imsa_tui")?;
    Some(dirs.data_local_dir().join("imsa_snapshot.json"))
}

fn load_imsa_column_widths_baseline() -> Option<ImsaColumnWidths> {
    let path = imsa_column_widths_path()?;
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str::<ImsaColumnWidths>(&text).ok()
}

fn save_imsa_column_widths_baseline(widths: &ImsaColumnWidths) {
    let Some(path) = imsa_column_widths_path() else {
        return;
    };

    if let Some(parent) = path.parent() {
        if fs::create_dir_all(parent).is_err() {
            return;
        }
    }

    let Ok(encoded) = serde_json::to_string_pretty(widths) else {
        return;
    };
    let _ = fs::write(path, encoded);
}

fn load_imsa_widths_from_snapshot_dump() -> Option<ImsaColumnWidths> {
    let path = imsa_snapshot_dump_path()?;
    let text = fs::read_to_string(path).ok()?;
    let parsed: PersistedImsaSnapshotStub = serde_json::from_str(&text).ok()?;
    ImsaColumnWidths::from_entries(&parsed.entries)
}

fn init_imsa_widths_baseline() -> Option<ImsaColumnWidths> {
    if let Some(saved) = load_imsa_column_widths_baseline() {
        return Some(saved);
    }

    let from_dump = load_imsa_widths_from_snapshot_dump()?;
    save_imsa_column_widths_baseline(&from_dump);
    Some(from_dump)
}

fn distribute_extra_space(widths: &mut [u16; IMSA_COLUMN_COUNT], mut extra: u16) {
    if extra == 0 {
        return;
    }

    let total: u32 = widths.iter().map(|w| *w as u32).sum();
    if total == 0 {
        return;
    }

    for width in widths.iter_mut() {
        let share = ((extra as u32 * *width as u32) / total) as u16;
        *width = width.saturating_add(share);
        extra = extra.saturating_sub(share);
    }

    let mut idx = 0usize;
    while extra > 0 {
        widths[idx] = widths[idx].saturating_add(1);
        extra -= 1;
        idx = (idx + 1) % IMSA_COLUMN_COUNT;
    }
}

fn reduce_widths_in_order(
    widths: &mut [u16; IMSA_COLUMN_COUNT],
    minimums: &[u16; IMSA_COLUMN_COUNT],
    mut deficit: u16,
    indexes: &[usize],
) -> u16 {
    if deficit == 0 || indexes.is_empty() {
        return deficit;
    }

    let mut progressed = true;
    while deficit > 0 && progressed {
        progressed = false;
        for idx in indexes {
            if deficit == 0 {
                break;
            }
            if widths[*idx] > minimums[*idx] {
                widths[*idx] -= 1;
                deficit -= 1;
                progressed = true;
            }
        }
    }

    deficit
}

fn calculate_imsa_widths(
    terminal_width: u16,
    entries: &[TimingEntry],
    baseline: Option<&ImsaColumnWidths>,
) -> ImsaColumnWidths {
    let observed = ImsaColumnWidths::from_entries(entries);
    let target = match (baseline.copied(), observed) {
        (Some(base), Some(obs)) => base.merge_keep_larger(obs).enforce_header_minimums(),
        (Some(base), None) => base.enforce_header_minimums(),
        (None, Some(obs)) => obs.enforce_header_minimums(),
        (None, None) => ImsaColumnWidths::header_minimums(),
    };

    let mut widths = target.to_array();
    let minimums = ImsaColumnWidths::header_minimums().to_array();
    let gutters = (IMSA_COLUMN_COUNT.saturating_sub(1)) as u16;
    let available_width = terminal_width.saturating_sub(gutters);
    let total_width: u16 = widths.iter().sum();

    if total_width < available_width {
        distribute_extra_space(&mut widths, available_width - total_width);
    } else if total_width > available_width {
        let mut deficit = total_width - available_width;

        // Lowest-priority columns are reduced first.
        deficit = reduce_widths_in_order(&mut widths, &minimums, deficit, &[5]);
        deficit = reduce_widths_in_order(
            &mut widths,
            &minimums,
            deficit,
            &[1, 2, 3, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
        );
        deficit = reduce_widths_in_order(&mut widths, &minimums, deficit, &[4, 0]);

        if deficit > 0 {
            widths = minimums;
        }
    }

    ImsaColumnWidths::from_array(widths)
}

fn imsa_constraints(widths: ImsaColumnWidths) -> Vec<Constraint> {
    widths
        .to_array()
        .into_iter()
        .map(Constraint::Length)
        .collect()
}

fn nls_table_widths() -> [Constraint; 16] {
    [
        Constraint::Length(4),
        Constraint::Length(5),
        Constraint::Length(9),
        Constraint::Length(5),
        Constraint::Length(18),
        Constraint::Min(18),
        Constraint::Length(24),
        Constraint::Length(7),
        Constraint::Length(11),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(10),
    ]
}

fn f1_table_widths() -> [Constraint; 11] {
    [
        Constraint::Length(4),
        Constraint::Length(5),
        Constraint::Min(24),
        Constraint::Min(16),
        Constraint::Length(7),
        Constraint::Length(11),
        Constraint::Length(11),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(5),
        Constraint::Length(5),
    ]
}

fn wec_table_widths() -> [Constraint; 14] {
    [
        Constraint::Length(4),
        Constraint::Length(5),
        Constraint::Length(9),
        Constraint::Length(5),
        Constraint::Length(18),
        Constraint::Min(18),
        Constraint::Length(24),
        Constraint::Length(7),
        Constraint::Length(11),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(10),
    ]
}

#[derive(Clone)]
struct GapAnchorInfo {
    stable_id: String,
    laps: String,
    gap_overall: String,
    gap_class: String,
    gap_next_in_class: String,
}

#[derive(Clone, Copy)]
enum GapColumn {
    Overall,
    Class,
    NextInClass,
}

enum GapValue {
    TimeMs(i64),
    Laps(i32),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PitHighlightPhase {
    None,
    In,
    Pit,
    Out,
}

#[derive(Clone)]
struct PitTracker {
    in_pit: bool,
    in_until: Option<Instant>,
    out_until: Option<Instant>,
}

impl PitTracker {
    fn new() -> Self {
        Self {
            in_pit: false,
            in_until: None,
            out_until: None,
        }
    }
}

fn gap_anchor_from_entry(entry: &TimingEntry) -> GapAnchorInfo {
    GapAnchorInfo {
        stable_id: entry.stable_id.clone(),
        laps: entry.laps.clone(),
        gap_overall: entry.gap_overall.clone(),
        gap_class: entry.gap_class.clone(),
        gap_next_in_class: entry.gap_next_in_class.clone(),
    }
}

fn anchor_gap_label(laps: &str) -> String {
    if laps.trim().chars().all(|ch| ch.is_ascii_digit()) && !laps.trim().is_empty() {
        return format!("----LAP {}", laps.trim());
    }
    "----".to_string()
}

fn anchor_gap_value(anchor: &GapAnchorInfo, column: GapColumn) -> &str {
    match column {
        GapColumn::Overall => &anchor.gap_overall,
        GapColumn::Class => &anchor.gap_class,
        GapColumn::NextInClass => &anchor.gap_next_in_class,
    }
}

fn parse_gap_value(raw: &str) -> Option<GapValue> {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed == "-"
        || trimmed.eq_ignore_ascii_case("leader")
        || trimmed.to_ascii_uppercase().starts_with("----LAP")
    {
        return None;
    }

    let upper = trimmed.to_ascii_uppercase();
    if upper.contains("LAP") {
        let token = trimmed.split_whitespace().find(|part| {
            let cleaned =
                part.trim_matches(|ch: char| !ch.is_ascii_digit() && ch != '+' && ch != '-');
            !cleaned.is_empty() && cleaned.chars().any(|ch| ch.is_ascii_digit())
        })?;
        let cleaned = token.trim_matches(|ch: char| !ch.is_ascii_digit() && ch != '+' && ch != '-');
        let laps = cleaned.parse::<i32>().ok()?;
        return Some(GapValue::Laps(laps));
    }

    let normalized = trimmed.trim_start_matches('+');
    if !normalized
        .chars()
        .all(|ch| ch.is_ascii_digit() || ch == ':' || ch == '.')
    {
        return None;
    }

    let total_ms = if let Some((left, right)) = normalized.rsplit_once(':') {
        let secs = right.parse::<f64>().ok()?;
        let mins = left.parse::<u64>().ok()?;
        ((mins as f64 * 60.0 + secs) * 1000.0).round() as i64
    } else {
        (normalized.parse::<f64>().ok()? * 1000.0).round() as i64
    };
    Some(GapValue::TimeMs(total_ms))
}

fn format_time_delta(ms: i64) -> String {
    let sign = if ms >= 0 { '+' } else { '-' };
    let abs_ms = ms.unsigned_abs();
    let minutes = abs_ms / 60_000;
    let secs = (abs_ms % 60_000) as f64 / 1000.0;
    if minutes > 0 {
        format!("{sign}{minutes}:{secs:06.3}")
    } else {
        format!("{sign}{secs:.3}")
    }
}

fn format_lap_delta(laps: i32) -> String {
    let sign = if laps >= 0 { '+' } else { '-' };
    let abs = laps.abs();
    if abs == 1 {
        format!("{sign}{abs} LAP")
    } else {
        format!("{sign}{abs} LAPS")
    }
}

fn relative_gap_text(
    entry: &TimingEntry,
    raw_value: &str,
    column: GapColumn,
    anchor: Option<&GapAnchorInfo>,
) -> String {
    let Some(anchor) = anchor else {
        return raw_value.to_string();
    };

    if entry.stable_id == anchor.stable_id {
        return anchor_gap_label(&anchor.laps);
    }

    let row_laps = entry.laps.trim().parse::<i32>().ok();
    let anchor_laps = anchor.laps.trim().parse::<i32>().ok();
    if let (Some(row_laps), Some(anchor_laps)) = (row_laps, anchor_laps) {
        if row_laps != anchor_laps {
            return format_lap_delta(anchor_laps - row_laps);
        }
    }

    let Some(row_gap) = parse_gap_value(raw_value) else {
        return raw_value.to_string();
    };
    let Some(anchor_gap) = parse_gap_value(anchor_gap_value(anchor, column)) else {
        return raw_value.to_string();
    };

    match (row_gap, anchor_gap) {
        (GapValue::TimeMs(row), GapValue::TimeMs(base)) => format_time_delta(row - base),
        (GapValue::Laps(row), GapValue::Laps(base)) => format_lap_delta(row - base),
        _ => raw_value.to_string(),
    }
}

fn build_rows(
    entries: &[TimingEntry],
    ctx: &TableRenderCtx<'_>,
    imsa_widths: Option<ImsaColumnWidths>,
) -> Vec<Row<'static>> {
    entries
        .iter()
        .enumerate()
        .map(|e| {
            let (idx, e) = e;
            let fav_key = favourite_key(ctx.active_series, &e.stable_id);
            let fav_marker = if ctx.favourites.contains(&fav_key) {
                "★ "
            } else {
                ""
            };
            let selected = ctx.selected_row_in_view == Some(idx);

            let row = match ctx.active_series {
                Series::Imsa => Row::new(vec![
                    Cell::from(e.position.to_string()),
                    Cell::from(format!("{fav_marker}{}", e.car_number)),
                    Cell::from(e.class_name.clone()),
                    Cell::from(e.class_rank.clone()),
                    Cell::from(marquee_if_needed(
                        &e.driver,
                        imsa_widths
                            .map(ImsaColumnWidths::driver_width)
                            .unwrap_or(28),
                        selected,
                        ctx.marquee_tick,
                    )),
                    Cell::from(marquee_if_needed(
                        &e.vehicle,
                        imsa_widths
                            .map(ImsaColumnWidths::vehicle_width)
                            .unwrap_or(45),
                        selected,
                        ctx.marquee_tick,
                    )),
                    Cell::from(e.laps.clone()),
                    Cell::from(relative_gap_text(
                        e,
                        &e.gap_overall,
                        GapColumn::Overall,
                        ctx.gap_anchor,
                    )),
                    Cell::from(relative_gap_text(
                        e,
                        &e.gap_class,
                        GapColumn::Class,
                        ctx.gap_anchor,
                    )),
                    Cell::from(relative_gap_text(
                        e,
                        &e.gap_next_in_class,
                        GapColumn::NextInClass,
                        ctx.gap_anchor,
                    )),
                    Cell::from(e.last_lap.clone()),
                    Cell::from(e.best_lap.clone()),
                    Cell::from(e.best_lap_no.clone()),
                    Cell::from(e.pit.clone()),
                    Cell::from(e.pit_stops.clone()),
                    Cell::from(marquee_if_needed(
                        &e.fastest_driver,
                        imsa_widths
                            .map(ImsaColumnWidths::fastest_width)
                            .unwrap_or(28),
                        selected,
                        ctx.marquee_tick,
                    )),
                ]),
                Series::Nls => Row::new(vec![
                    Cell::from(e.position.to_string()),
                    Cell::from(format!("{fav_marker}{}", e.car_number)),
                    Cell::from(e.class_name.clone()),
                    Cell::from(e.class_rank.clone()),
                    Cell::from(marquee_if_needed(&e.driver, 18, selected, ctx.marquee_tick)),
                    Cell::from(marquee_if_needed(
                        &e.vehicle,
                        18,
                        selected,
                        ctx.marquee_tick,
                    )),
                    Cell::from(marquee_if_needed(&e.team, 24, selected, ctx.marquee_tick)),
                    Cell::from(e.laps.clone()),
                    Cell::from(relative_gap_text(
                        e,
                        &e.gap_overall,
                        GapColumn::Overall,
                        ctx.gap_anchor,
                    )),
                    Cell::from(e.last_lap.clone()),
                    Cell::from(e.best_lap.clone()),
                    Cell::from(e.sector_1.clone()),
                    Cell::from(e.sector_2.clone()),
                    Cell::from(e.sector_3.clone()),
                    Cell::from(e.sector_4.clone()),
                    Cell::from(e.sector_5.clone()),
                ]),
                Series::F1 => Row::new(vec![
                    Cell::from(e.position.to_string()),
                    Cell::from(format!("{fav_marker}{}", e.car_number)),
                    Cell::from(marquee_if_needed(&e.driver, 32, selected, ctx.marquee_tick)),
                    Cell::from(marquee_if_needed(&e.team, 22, selected, ctx.marquee_tick)),
                    Cell::from(e.laps.clone()),
                    Cell::from(relative_gap_text(
                        e,
                        &e.gap_overall,
                        GapColumn::Overall,
                        ctx.gap_anchor,
                    )),
                    Cell::from(relative_gap_text(
                        e,
                        &e.gap_class,
                        GapColumn::Class,
                        ctx.gap_anchor,
                    )),
                    Cell::from(e.last_lap.clone()),
                    Cell::from(e.best_lap.clone()),
                    Cell::from(e.pit.clone()),
                    Cell::from(e.pit_stops.clone()),
                ]),
                Series::Wec => Row::new(vec![
                    Cell::from(e.position.to_string()),
                    Cell::from(format!("{fav_marker}{}", e.car_number)),
                    Cell::from(e.class_name.clone()),
                    Cell::from(e.class_rank.clone()),
                    Cell::from(marquee_if_needed(&e.driver, 18, selected, ctx.marquee_tick)),
                    Cell::from(marquee_if_needed(
                        &e.vehicle,
                        18,
                        selected,
                        ctx.marquee_tick,
                    )),
                    Cell::from(marquee_if_needed(&e.team, 24, selected, ctx.marquee_tick)),
                    Cell::from(e.laps.clone()),
                    Cell::from(relative_gap_text(
                        e,
                        &e.gap_overall,
                        GapColumn::Overall,
                        ctx.gap_anchor,
                    )),
                    Cell::from(e.last_lap.clone()),
                    Cell::from(e.best_lap.clone()),
                    Cell::from(e.sector_1.clone()),
                    Cell::from(e.sector_2.clone()),
                    Cell::from(e.sector_3.clone()),
                ]),
            };

            let mut style = class_style(&e.class_name, ctx.active_series, ctx.class_colors);
            let pit_phase = pit_phase_for_entry(ctx.pit_trackers, e, ctx.now);
            if let Some(pit_style) = pit_phase_style(pit_phase) {
                style = style.patch(pit_style);
            }
            if ctx.marked_stable_id == Some(e.stable_id.as_str()) {
                style = style
                    .bg(Color::Rgb(34, 70, 122))
                    .add_modifier(Modifier::BOLD);
            }

            row.style(style)
        })
        .collect()
}

fn marquee_if_needed(text: &str, width_hint: usize, selected: bool, tick: usize) -> String {
    if !selected {
        return text.to_string();
    }

    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= width_hint {
        return text.to_string();
    }

    let gap = 3;
    let cycle_len = chars.len() + gap;
    let offset = tick % cycle_len;

    if offset < chars.len() {
        let mut out = String::new();
        out.extend(chars[offset..].iter());
        out.push_str("   ");
        out.extend(chars[..offset].iter());
        out
    } else {
        let leading_spaces = offset - chars.len();
        let mut out = " ".repeat(leading_spaces);
        out.push_str(text);
        out
    }
}

fn pit_signal_active(active_series: Series, entry: &TimingEntry) -> bool {
    match active_series {
        Series::Imsa | Series::F1 | Series::Wec => entry.pit.eq_ignore_ascii_case("yes"),
        Series::Nls => entry.sector_5.trim().eq_ignore_ascii_case("PIT"),
    }
}

fn pit_phase_style(phase: PitHighlightPhase) -> Option<Style> {
    match phase {
        PitHighlightPhase::None => None,
        PitHighlightPhase::In => Some(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        PitHighlightPhase::Pit => Some(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        PitHighlightPhase::Out => Some(
            Style::default()
                .fg(Color::LightMagenta)
                .add_modifier(Modifier::BOLD),
        ),
    }
}

fn refresh_pit_trackers(
    trackers: &mut HashMap<String, PitTracker>,
    entries: &[TimingEntry],
    active_series: Series,
    now: Instant,
) {
    const IN_HIGHLIGHT_WINDOW: Duration = Duration::from_millis(1200);
    const OUT_HIGHLIGHT_WINDOW: Duration = Duration::from_millis(1800);

    let current_ids: HashSet<String> = entries
        .iter()
        .map(|entry| entry.stable_id.clone())
        .collect();
    trackers.retain(|stable_id, _| current_ids.contains(stable_id));

    for entry in entries {
        let signal = pit_signal_active(active_series, entry);
        let tracker = trackers
            .entry(entry.stable_id.clone())
            .or_insert_with(PitTracker::new);

        if signal {
            if !tracker.in_pit {
                tracker.in_pit = true;
                tracker.in_until = Some(now + IN_HIGHLIGHT_WINDOW);
            }
            tracker.out_until = None;
        } else if tracker.in_pit {
            tracker.in_pit = false;
            tracker.in_until = None;
            tracker.out_until = Some(now + OUT_HIGHLIGHT_WINDOW);
        }
    }
}

fn pit_phase_for_entry(
    trackers: &HashMap<String, PitTracker>,
    entry: &TimingEntry,
    now: Instant,
) -> PitHighlightPhase {
    let Some(tracker) = trackers.get(&entry.stable_id) else {
        return PitHighlightPhase::None;
    };

    if tracker.in_pit {
        if tracker.in_until.map(|until| now <= until).unwrap_or(false) {
            PitHighlightPhase::In
        } else {
            PitHighlightPhase::Pit
        }
    } else if tracker.out_until.map(|until| now <= until).unwrap_or(false) {
        PitHighlightPhase::Out
    } else {
        PitHighlightPhase::None
    }
}

fn build_table<'a>(
    title: impl Into<String>,
    entries: &'a [TimingEntry],
    ctx: &TableRenderCtx<'_>,
    table_width: u16,
    imsa_baseline: Option<&ImsaColumnWidths>,
) -> Table<'a> {
    let (headers, widths, imsa_widths): (Vec<&str>, Vec<Constraint>, Option<ImsaColumnWidths>) =
        match ctx.active_series {
            Series::Imsa => {
                let imsa_widths = calculate_imsa_widths(table_width, entries, imsa_baseline);
                (
                    vec![
                        "Pos",
                        "#",
                        "Class",
                        "PIC",
                        "Driver",
                        "Vehicle",
                        "Laps",
                        "Gap O",
                        "Gap C",
                        "Next C",
                        "Last",
                        "Best",
                        "BL#",
                        "Pit",
                        "Stop",
                        "Fastest Driver",
                    ],
                    imsa_constraints(imsa_widths),
                    Some(imsa_widths),
                )
            }
            Series::Nls => (
                vec![
                    "Pos", "#", "Class", "PIC", "Driver", "Vehicle", "Team", "Laps", "Gap", "Last",
                    "Best", "S1", "S2", "S3", "S4", "S5",
                ],
                nls_table_widths().to_vec(),
                None,
            ),
            Series::F1 => (
                vec![
                    "Pos", "#", "Driver", "Team", "Laps", "Gap", "Int", "Last", "Best", "Pit",
                    "Stops",
                ],
                f1_table_widths().to_vec(),
                None,
            ),
            Series::Wec => (
                vec![
                    "Pos", "#", "Class", "PIC", "Driver", "Vehicle", "Team", "Laps", "Gap", "Last",
                    "Best", "S1", "S2", "S3",
                ],
                wec_table_widths().to_vec(),
                None,
            ),
        };

    Table::new(build_rows(entries, ctx, imsa_widths), widths)
        .header(Row::new(headers).style(Style::default().add_modifier(Modifier::BOLD)))
        .highlight_style(Style::default().bg(Color::Rgb(45, 45, 45)))
        .block(Block::default().title(title.into()).borders(Borders::ALL))
}

fn grouped_entries(
    entries: &[TimingEntry],
    _active_series: Series,
) -> Vec<(String, Vec<TimingEntry>)> {
    let mut grouped = std::collections::HashMap::<String, Vec<TimingEntry>>::new();
    for entry in entries {
        grouped
            .entry(class_display_name(&entry.class_name))
            .or_default()
            .push(entry.clone());
    }

    let mut groups: Vec<(String, Vec<TimingEntry>)> = grouped.into_iter().collect();
    for (_, entries) in &mut groups {
        entries.sort_by(|a, b| {
            let ar = a.class_rank.parse::<u32>().unwrap_or(u32::MAX);
            let br = b.class_rank.parse::<u32>().unwrap_or(u32::MAX);
            ar.cmp(&br).then_with(|| a.position.cmp(&b.position))
        });
    }

    // Order grouped classes by their best overall-running position so the
    // most competitive class appears first in grouped view.
    groups.sort_by(|(an, ae), (bn, be)| {
        let a_best = ae.iter().map(|e| e.position).min().unwrap_or(u32::MAX);
        let b_best = be.iter().map(|e| e.position).min().unwrap_or(u32::MAX);
        a_best.cmp(&b_best).then_with(|| an.cmp(bn))
    });

    groups
}

fn next_view_mode(current: ViewMode, groups_len: usize) -> ViewMode {
    if groups_len == 0 {
        return match current {
            ViewMode::Overall => ViewMode::Grouped,
            ViewMode::Grouped => ViewMode::Favourites,
            _ => ViewMode::Overall,
        };
    }

    match current {
        ViewMode::Overall => ViewMode::Grouped,
        ViewMode::Grouped => ViewMode::Class(0),
        ViewMode::Class(idx) => {
            if idx + 1 < groups_len {
                ViewMode::Class(idx + 1)
            } else {
                ViewMode::Favourites
            }
        }
        ViewMode::Favourites => ViewMode::Overall,
    }
}

fn start_feed(series: Series, tx: Sender<TimingMessage>, source_id: u64) -> ActiveFeed {
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let (debug_tx, debug_rx) = mpsc::channel::<String>();
    let debug_output = SeriesDebugOutput::Channel(debug_tx);

    thread::spawn(move || match series {
        Series::Imsa => polling_worker_with_debug(tx, source_id, stop_rx, debug_output),
        Series::Nls => websocket_worker_with_debug(tx, source_id, stop_rx, debug_output),
        Series::F1 => signalr_worker_with_debug(tx, source_id, stop_rx, debug_output),
        Series::Wec => wec_websocket_worker(tx, source_id, stop_rx, debug_output),
    });

    ActiveFeed {
        source_id,
        stop_tx,
        debug_rx: Some(debug_rx),
    }
}

fn stop_feed(feed: &mut Option<ActiveFeed>) {
    if let Some(active_feed) = feed.take() {
        let _ = active_feed.stop_tx.send(());
    }
}

fn push_series_debug_log(logs: &mut VecDeque<String>, line: String) {
    logs.push_back(line);
    while logs.len() > IMSA_DEBUG_LOG_CAPACITY {
        logs.pop_front();
    }
}

fn drain_series_debug_logs(feed: &Option<ActiveFeed>, logs: &mut VecDeque<String>) {
    let Some(active_feed) = feed.as_ref() else {
        return;
    };
    let Some(debug_rx) = active_feed.debug_rx.as_ref() else {
        return;
    };

    while let Ok(line) = debug_rx.try_recv() {
        push_series_debug_log(logs, line);
    }
}

fn drain_messages(
    rx: &Receiver<TimingMessage>,
    active_source_id: u64,
    header: &mut TimingHeader,
    entries: &mut Vec<TimingEntry>,
    status: &mut String,
    last_error: &mut Option<String>,
    last_update: &mut Option<Instant>,
) {
    while let Ok(msg) = rx.try_recv() {
        match msg {
            TimingMessage::Status { source_id, text } if source_id == active_source_id => {
                *status = text
            }
            TimingMessage::Error { source_id, text } if source_id == active_source_id => {
                *last_error = Some(text)
            }
            TimingMessage::Snapshot {
                source_id,
                header: new_header,
                entries: new_entries,
            } if source_id == active_source_id => {
                if new_header.event_name != "-" {
                    header.event_name = new_header.event_name;
                }
                if new_header.session_name != "-" {
                    header.session_name = new_header.session_name;
                }
                if new_header.track_name != "-" {
                    header.track_name = new_header.track_name;
                }
                if new_header.day_time != "-" {
                    header.day_time = new_header.day_time;
                }
                if new_header.flag != "-" {
                    header.flag = new_header.flag;
                }
                if new_header.time_to_go != "-" {
                    header.time_to_go = new_header.time_to_go;
                }
                *entries = new_entries;
                *status = "Live timing connected".to_string();
                *last_error = None;
                *last_update = Some(Instant::now());
            }
            _ => {}
        }
    }
}

fn visible_slice(
    entries: &[TimingEntry],
    selected_idx: usize,
    table_area_height: u16,
) -> (&[TimingEntry], usize) {
    let visible_rows = table_area_height.saturating_sub(3) as usize;
    let window = visible_rows.max(1);
    if entries.is_empty() {
        return (&entries[0..0], 0);
    }

    let max_start = entries.len().saturating_sub(window);
    let start = selected_idx
        .saturating_sub(window.saturating_sub(1))
        .min(max_start);
    let end = (start + window).min(entries.len());
    (&entries[start..end], start)
}

fn step_selection(current: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    let max = (len - 1) as isize;
    ((current as isize + delta).clamp(0, max)) as usize
}

fn view_entries_for_mode<'a>(
    all_entries: &'a [TimingEntry],
    current_groups: &'a [(String, Vec<TimingEntry>)],
    view_mode: ViewMode,
    favourites: &HashSet<String>,
    active_series: Series,
) -> Vec<&'a TimingEntry> {
    match view_mode {
        ViewMode::Overall => all_entries.iter().collect(),
        ViewMode::Grouped => current_groups
            .iter()
            .flat_map(|(_, class_entries)| class_entries.iter())
            .collect(),
        ViewMode::Class(idx) => current_groups
            .get(idx)
            .map(|(_, class_entries)| class_entries.iter().collect())
            .unwrap_or_default(),
        ViewMode::Favourites => all_entries
            .iter()
            .filter(|entry| favourites.contains(&favourite_key(active_series, &entry.stable_id)))
            .collect(),
    }
}

fn entry_matches_search(entry: &TimingEntry, query: &str) -> bool {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return false;
    }

    if trimmed.chars().all(|c| c.is_ascii_digit()) {
        return entry.car_number.trim() == trimmed;
    }

    let needle = trimmed.to_ascii_lowercase();
    entry.car_number.to_ascii_lowercase().contains(&needle)
        || entry.driver.to_ascii_lowercase().contains(&needle)
        || entry.vehicle.to_ascii_lowercase().contains(&needle)
        || entry.team.to_ascii_lowercase().contains(&needle)
}

fn refresh_search_matches(search: &mut SearchState, view_entries: &[&TimingEntry]) {
    if search.query.trim().is_empty() {
        search.matches.clear();
        search.current_match = 0;
        return;
    }

    search.matches = view_entries
        .iter()
        .enumerate()
        .filter_map(|(idx, entry)| entry_matches_search(entry, &search.query).then_some(idx))
        .collect();

    if search.matches.is_empty() || search.current_match >= search.matches.len() {
        search.current_match = 0;
    }
}

fn selected_series_index(series: Series) -> usize {
    Series::all()
        .iter()
        .position(|candidate| *candidate == series)
        .unwrap_or(0)
}

fn favourites_count_for_series(series: Series, favourites: &HashSet<String>) -> usize {
    let prefix = format!("{}|", series.as_key_prefix());
    favourites
        .iter()
        .filter(|value| value.starts_with(&prefix))
        .count()
}

fn display_event_name(_series: Series, raw: &str) -> String {
    if raw.trim().is_empty() || raw == "-" {
        return "-".to_string();
    }

    raw.trim().to_string()
}

fn display_session_name(series: Series, raw: &str) -> String {
    if raw.trim().is_empty() || raw == "-" {
        return "-".to_string();
    }

    if series == Series::Imsa {
        let cleaned = normalize_imsa_label(raw);
        if !cleaned.is_empty() {
            return cleaned;
        }
    }

    raw.to_string()
}

fn normalize_imsa_label(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("weathertech") {
        if let Some((idx, ch)) = raw
            .char_indices()
            .rev()
            .find(|(_, ch)| matches!(ch, '-' | '–' | '—'))
        {
            return raw[idx + ch.len_utf8()..].trim().to_string();
        }
    }
    raw.trim().to_string()
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
            let size = f.size();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(4), Constraint::Min(10)])
                .split(size);

            let age = match last_update {
                Some(t) => format!("Upd {}s", t.elapsed().as_secs()),
                None => "Upd -".to_string(),
            };

            let tte_text = if header.time_to_go.is_empty() { "-" } else { &header.time_to_go };
            let (flag_text, flag_span_style, header_style) =
                animated_flag_theme(effective_flag, &transition_from_flag, transition_started_at);

            let mode_text = view_mode_text(
                view_mode,
                &current_groups
                    .iter()
                    .map(|(name, _)| name.clone())
                    .collect::<Vec<_>>(),
            );

            let event_text = display_event_name(
                active_series,
                if header.event_name.is_empty() { "-" } else { &header.event_name },
            );
            let session_display = display_session_name(
                active_series,
                if header.session_name.is_empty() {
                    "-"
                } else {
                    &header.session_name
                },
            );

            let header_lead = format!(
                "{} | {} | {} | TTE {} | Mode {} | ",
                status,
                event_text,
                session_display,
                tte_text,
                mode_text,
            );

            let mut header_spans = vec![
                Span::styled(header_lead, header_style),
                Span::styled(flag_text, flag_span_style),
            ];

            if demo_mode {
                header_spans.push(Span::styled(
                    " | DEMO",
                    header_style.add_modifier(Modifier::BOLD),
                ));
            }

            header_spans.push(Span::styled(
                format!(
                    " | {} | Favs {}",
                    age,
                    favourites_count_for_series(active_series, &favourites),
                ),
                header_style,
            ));

            let mut key_hint_spans = vec![Span::styled(
                "Keys: h help | L logs | d demo | q quit",
                header_style,
            )];

            if search.input_active {
                key_hint_spans.push(Span::styled(
                    format!(" | Search: {}_", search.query),
                    header_style.add_modifier(Modifier::BOLD),
                ));
            } else if !search.query.trim().is_empty() {
                key_hint_spans.push(Span::styled(
                    format!(
                        " | Search: {} ({}/{})",
                        search.query,
                        if search.matches.is_empty() { 0 } else { search.current_match + 1 },
                        search.matches.len(),
                    ),
                    header_style,
                ));
            }

            if let Some(err) = &last_error {
                key_hint_spans.push(Span::styled(format!(" | Error: {}", err), header_style));
            }

            let status_widget = Paragraph::new(vec![Line::from(header_spans), Line::from(key_hint_spans)])
                .style(header_style)
                .wrap(Wrap { trim: false })
                .block(
                    Block::default()
                        .title(format!("{} TUI", active_series.label()))
                        .borders(Borders::ALL)
                        .style(header_style),
                );
            f.render_widget(status_widget, chunks[0]);

            if entries.is_empty() {
                let waiting = Paragraph::new(format!(
                    "No timing data yet. Waiting for first successful {} snapshot... Press h for help.",
                    active_series.label(),
                ))
                .block(Block::default().title("Overall").borders(Borders::ALL));
                f.render_widget(waiting, chunks[1]);
            } else {
                match view_mode {
                    ViewMode::Overall => {
                        let (visible_entries, start) =
                            visible_slice(&entries, selected_row, chunks[1].height);
                        let local_selected = selected_row.saturating_sub(start);
                        let mut state = ratatui::widgets::TableState::default();
                        state.select(Some(local_selected));
                        let table_ctx = TableRenderCtx {
                            favourites: &favourites,
                            marked_stable_id,
                            active_series,
                            selected_row_in_view: Some(local_selected),
                            marquee_tick,
                            gap_anchor: gap_anchor.as_ref(),
                            pit_trackers: &pit_trackers,
                            class_colors: &header.class_colors,
                            now,
                        };
                        let table = build_table(
                            "Overall",
                            visible_entries,
                            &table_ctx,
                            chunks[1].width,
                            imsa_width_baseline.as_ref(),
                        );
                        f.render_stateful_widget(table, chunks[1], &mut state);
                    }
                    ViewMode::Grouped => {
                        if current_groups.is_empty() {
                            let waiting = Paragraph::new("No grouped class data available yet.")
                                .block(Block::default().title("Grouped").borders(Borders::ALL));
                            f.render_widget(waiting, chunks[1]);
                        } else {
                            let mut selected_group_idx = 0usize;
                            let mut running = 0usize;
                            for (idx, (_, class_entries)) in current_groups.iter().enumerate() {
                                if selected_row < running + class_entries.len() {
                                    selected_group_idx = idx;
                                    break;
                                }
                                running += class_entries.len();
                            }

                            let minimum_rows_per_group = 7_u16;
                            let max_visible_groups =
                                (chunks[1].height / minimum_rows_per_group).max(1) as usize;

                            // Grouped mode should remain grouped for every series. When many
                            // groups exist (common in NLS), we render a moving window of groups
                            // around the current selection so users can scroll down naturally.
                            let visible_group_count = current_groups.len().min(max_visible_groups.max(1));
                            let start_group_idx = if current_groups.len() <= visible_group_count {
                                0
                            } else {
                                let half = visible_group_count / 2;
                                selected_group_idx
                                    .saturating_sub(half)
                                    .min(current_groups.len() - visible_group_count)
                            };
                            let end_group_idx = start_group_idx + visible_group_count;
                            let visible_groups = &current_groups[start_group_idx..end_group_idx];

                            let constraints: Vec<Constraint> = visible_groups
                                .iter()
                                .map(|_| Constraint::Ratio(1, visible_groups.len() as u32))
                                .collect();
                            let group_chunks = Layout::default()
                                .direction(Direction::Vertical)
                                .constraints(constraints)
                                .split(chunks[1]);

                            let mut global_offset = current_groups
                                .iter()
                                .take(start_group_idx)
                                .map(|(_, entries)| entries.len())
                                .sum::<usize>();

                            for ((class_name, class_entries), area) in
                                visible_groups.iter().zip(group_chunks.iter())
                            {
                                let local_selected = selected_row
                                    .saturating_sub(global_offset)
                                    .min(class_entries.len().saturating_sub(1));
                                let (visible_entries, start) =
                                    visible_slice(class_entries, local_selected, area.height);
                                let mut state = ratatui::widgets::TableState::default();
                                let highlight = if selected_row >= global_offset
                                    && selected_row < global_offset + class_entries.len()
                                {
                                    Some(local_selected.saturating_sub(start))
                                } else {
                                    None
                                };
                                state.select(highlight);
                                let title = format!("{} ({} cars)", class_name, class_entries.len());
                                let table_ctx = TableRenderCtx {
                                    favourites: &favourites,
                                    marked_stable_id,
                                    active_series,
                                    selected_row_in_view: highlight,
                                    marquee_tick,
                                    gap_anchor: gap_anchor.as_ref(),
                                    pit_trackers: &pit_trackers,
                                    class_colors: &header.class_colors,
                                    now,
                                };
                                let table = build_table(
                                    title,
                                    visible_entries,
                                    &table_ctx,
                                    area.width,
                                    imsa_width_baseline.as_ref(),
                                );
                                f.render_stateful_widget(table, *area, &mut state);
                                global_offset += class_entries.len();
                            }
                        }
                    }
                    ViewMode::Class(idx) => {
                        if let Some((class_name, class_entries)) = current_groups.get(idx) {
                            let (visible_entries, start) =
                                visible_slice(class_entries, selected_row, chunks[1].height);
                            let local_selected = selected_row.saturating_sub(start);
                            let mut state = ratatui::widgets::TableState::default();
                            state.select(Some(local_selected));
                            let table_ctx = TableRenderCtx {
                                favourites: &favourites,
                                marked_stable_id,
                                active_series,
                                selected_row_in_view: Some(local_selected),
                                marquee_tick,
                                gap_anchor: gap_anchor.as_ref(),
                                pit_trackers: &pit_trackers,
                                class_colors: &header.class_colors,
                                now,
                            };
                            let table = build_table(
                                format!("{} ({} cars)", class_name, class_entries.len()),
                                visible_entries,
                                &table_ctx,
                                chunks[1].width,
                                imsa_width_baseline.as_ref(),
                            );
                            f.render_stateful_widget(table, chunks[1], &mut state);
                        } else {
                            let waiting = Paragraph::new("No class data available yet.")
                                .block(Block::default().title("Class").borders(Borders::ALL));
                            f.render_widget(waiting, chunks[1]);
                        }
                    }
                    ViewMode::Favourites => {
                        let favourite_entries: Vec<TimingEntry> = entries
                            .iter()
                            .filter(|entry| {
                                favourites.contains(&favourite_key(active_series, &entry.stable_id))
                            })
                            .cloned()
                            .collect();
                        if favourite_entries.is_empty() {
                            let waiting =
                                Paragraph::new("No favourites yet. Select a car and press space.")
                                    .block(
                                        Block::default().title("Favourites").borders(Borders::ALL),
                                    );
                            f.render_widget(waiting, chunks[1]);
                        } else {
                            let (visible_entries, start) =
                                visible_slice(&favourite_entries, selected_row, chunks[1].height);
                            let local_selected = selected_row.saturating_sub(start);
                            let mut state = ratatui::widgets::TableState::default();
                            state.select(Some(local_selected));
                            let table_ctx = TableRenderCtx {
                                favourites: &favourites,
                                marked_stable_id,
                                active_series,
                                selected_row_in_view: Some(local_selected),
                                marquee_tick,
                                gap_anchor: gap_anchor.as_ref(),
                                pit_trackers: &pit_trackers,
                                class_colors: &header.class_colors,
                                now,
                            };
                            let table = build_table(
                                format!("Favourites ({} cars)", favourite_entries.len()),
                                visible_entries,
                                &table_ctx,
                                chunks[1].width,
                                imsa_width_baseline.as_ref(),
                            );
                            f.render_stateful_widget(table, chunks[1], &mut state);
                        }
                    }
                }
            }

            if show_help {
                let area = centered_rect(40, 40, size);
                f.render_widget(Clear, area);
                f.render_widget(help_popup(), area);
            }

            if series_picker.is_open {
                let area = centered_rect(35, 35, size);
                f.render_widget(Clear, area);
                f.render_widget(
                    series_picker_popup(active_series, series_picker.selected_idx),
                    area,
                );
            }

            if group_picker.is_open {
                let area = centered_rect(40, 45, size);
                f.render_widget(Clear, area);
                let group_names: Vec<String> = current_groups
                    .iter()
                    .map(|(group_name, entries)| format!("{} ({} cars)", group_name, entries.len()))
                    .collect();
                f.render_widget(group_picker_popup(&group_names, group_picker.selected_idx), area);
            }

            if logs_panel.is_open {
                let area = centered_rect(65, 60, size);
                f.render_widget(Clear, area);

                let visible_lines = area.height.saturating_sub(3) as usize;
                let total = imsa_debug_logs.len();
                let max_scroll = total.saturating_sub(1);
                let scroll = logs_panel.scroll.min(max_scroll);
                let end_exclusive = total.saturating_sub(scroll);
                let start = end_exclusive.saturating_sub(visible_lines);

                let mut lines = vec![];
                if imsa_debug_logs.is_empty() {
                    lines.push(Line::from("No IMSA debug events yet."));
                } else {
                    for entry in imsa_debug_logs.range(start..end_exclusive) {
                        lines.push(Line::from(entry.as_str()));
                    }
                }
                lines.push(Line::from(""));
                lines.push(Line::from("↑/↓ scroll | c clear | Esc or L close"));

                let title = format!("{} Logs ({total}/{IMSA_DEBUG_LOG_CAPACITY})", active_series.label());

                let logs_popup = Paragraph::new(lines)
                    .alignment(Alignment::Left)
                    .wrap(Wrap { trim: false })
                    .block(Block::default().title(title).borders(Borders::ALL));
                f.render_widget(logs_popup, area);
            }
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

        let grouped = grouped_entries(&entries, Series::Imsa);

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

        assert_eq!(favourites_count_for_series(Series::Imsa, &favourites), 2);
        assert_eq!(favourites_count_for_series(Series::Nls, &favourites), 1);
        assert_eq!(favourites_count_for_series(Series::F1, &favourites), 1);
    }

    #[test]
    fn header_formatting_normalizes_imsa_labels_and_fallbacks() {
        assert_eq!(
            display_event_name(Series::Imsa, "  Twelve Hours of Sebring  "),
            "Twelve Hours of Sebring"
        );
        assert_eq!(display_event_name(Series::Imsa, "-"), "-");
        assert_eq!(display_session_name(Series::Imsa, "-"), "-");

        assert_eq!(
            display_session_name(
                Series::Imsa,
                "IMSA WeatherTech SportsCar Championship - Qualifying"
            ),
            "Qualifying"
        );
        assert_eq!(
            display_session_name(
                Series::Imsa,
                "IMSA WeatherTech SportsCar Championship — Race"
            ),
            "Race"
        );
        assert_eq!(
            display_session_name(Series::Nls, "  ADAC NLS  "),
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
            push_series_debug_log(&mut logs, format!("line-{idx}"));
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
