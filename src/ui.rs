use std::{
    collections::HashSet,
    fs, io,
    path::PathBuf,
    sync::mpsc::{self, Receiver},
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

use crate::imsa::{normalize_class_name, polling_worker, Entry, HeaderInfo, UiMessage};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct AppConfig {
    favourites: HashSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewMode {
    Overall,
    Grouped,
    Class(usize),
    Favourites,
}

#[derive(Debug, Clone, Default)]
struct DemoFlagState {
    enabled: bool,
    idx: usize,
}

#[derive(Debug, Clone, Default)]
struct SearchState {
    query: String,
    matches: Vec<usize>,
    current_match: usize,
    input_active: bool,
}

fn demo_flag_name(idx: usize) -> &'static str {
    match idx % 5 {
        0 => "Green",
        1 => "Yellow",
        2 => "Red",
        3 => "White",
        _ => "Checkered",
    }
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
        Line::from("o      switch to overall view"),
        Line::from("↑/↓    move selection"),
        Line::from("PgUp/PgDn  fast scroll"),
        Line::from("space  toggle favourite for selected car"),
        Line::from("f      jump to next favourite in current view"),
        Line::from("s      search by car #, driver, or team"),
        Line::from("n/p    next/prev search result"),
        Line::from("r      cycle demo flag"),
        Line::from("0      return to live flag"),
        Line::from("q      quit"),
        Line::from("Esc    close help / quit app"),
        Line::from(""),
        Line::from("Press h or Esc to close this popup."),
    ];

    Paragraph::new(text)
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: false })
        .block(Block::default().title("Help").borders(Borders::ALL))
}

fn config_path() -> Option<PathBuf> {
    let dirs = ProjectDirs::from("com", "imsa", "imsa_tui")?;
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

    let encoded =
        toml::to_string_pretty(config).map_err(|e| format!("encode config failed: {e}"))?;
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
        "green" => (
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

fn class_style(class_name: &str) -> Style {
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
        _ => Style::default(),
    }
}

fn class_display_name(name: &str) -> String {
    match normalize_class_name(name).as_str() {
        "GTP" => "GTP".to_string(),
        "LMP2" => "LMP2".to_string(),
        "GTDPRO" => "GTD PRO".to_string(),
        "GTD" => "GTD".to_string(),
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

fn table_widths() -> [Constraint; 16] {
    [
        Constraint::Length(4),
        Constraint::Length(5),
        Constraint::Length(7),
        Constraint::Length(4),
        Constraint::Length(24),
        Constraint::Min(16),
        Constraint::Length(6),
        Constraint::Length(11),
        Constraint::Length(11),
        Constraint::Length(11),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(5),
        Constraint::Length(5),
        Constraint::Length(5),
        Constraint::Length(18),
    ]
}

fn build_rows(
    entries: &[Entry],
    favourites: &HashSet<String>,
    marked_stable_id: Option<&str>,
) -> Vec<Row<'static>> {
    entries
        .iter()
        .map(|e| {
            let fav_marker = if favourites.contains(&e.stable_id) {
                "★ "
            } else {
                ""
            };
            Row::new(vec![
                Cell::from(e.position.to_string()),
                Cell::from(format!("{fav_marker}{}", e.car_number)),
                Cell::from(e.class_name.clone()),
                Cell::from(e.class_rank.clone()),
                Cell::from(e.driver.clone()),
                Cell::from(e.vehicle.clone()),
                Cell::from(e.laps.clone()),
                Cell::from(e.gap_overall.clone()),
                Cell::from(e.gap_class.clone()),
                Cell::from(e.gap_next_in_class.clone()),
                Cell::from(e.last_lap.clone()),
                Cell::from(e.best_lap.clone()),
                Cell::from(e.best_lap_no.clone()),
                Cell::from(e.pit.clone()),
                Cell::from(e.pit_stops.clone()),
                Cell::from(e.fastest_driver.clone()),
            ])
            .style(if marked_stable_id == Some(e.stable_id.as_str()) {
                class_style(&e.class_name)
                    .bg(Color::Rgb(34, 70, 122))
                    .add_modifier(Modifier::BOLD)
            } else {
                class_style(&e.class_name)
            })
        })
        .collect()
}

fn build_table<'a>(
    title: impl Into<String>,
    entries: &'a [Entry],
    favourites: &HashSet<String>,
    marked_stable_id: Option<&str>,
) -> Table<'a> {
    Table::new(
        build_rows(entries, favourites, marked_stable_id),
        table_widths(),
    )
    .header(
        Row::new(vec![
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
        ])
        .style(Style::default().add_modifier(Modifier::BOLD)),
    )
    .highlight_style(Style::default().bg(Color::Rgb(45, 45, 45)))
    .block(Block::default().title(title.into()).borders(Borders::ALL))
}

fn grouped_entries(entries: &[Entry]) -> Vec<(String, Vec<Entry>)> {
    let ordered = ["GTP", "LMP2", "GTDPRO", "GTD"];
    let mut groups: Vec<(String, Vec<Entry>)> = Vec::new();

    for class_key in ordered {
        let mut group: Vec<Entry> = entries
            .iter()
            .filter(|e| normalize_class_name(&e.class_name) == class_key)
            .cloned()
            .collect();
        if !group.is_empty() {
            group.sort_by(|a, b| {
                let ar = a.class_rank.parse::<u32>().unwrap_or(u32::MAX);
                let br = b.class_rank.parse::<u32>().unwrap_or(u32::MAX);
                ar.cmp(&br).then_with(|| a.position.cmp(&b.position))
            });
            groups.push((class_display_name(class_key), group));
        }
    }

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

fn drain_messages(
    rx: &Receiver<UiMessage>,
    header: &mut HeaderInfo,
    entries: &mut Option<Vec<Entry>>,
    status: &mut String,
    last_error: &mut Option<String>,
    last_update: &mut Option<Instant>,
) {
    while let Ok(msg) = rx.try_recv() {
        match msg {
            UiMessage::Status(s) => *status = s,
            UiMessage::Error(err) => *last_error = Some(err),
            UiMessage::Snapshot {
                header: new_header,
                entries: new_entries,
            } => {
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
                *entries = Some(new_entries);
                *status = "Live timing connected".to_string();
                *last_error = None;
                *last_update = Some(Instant::now());
            }
        }
    }
}

fn visible_slice<'a>(
    entries: &'a [Entry],
    selected_idx: usize,
    table_area_height: u16,
) -> (&'a [Entry], usize) {
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
    all_entries: &'a [Entry],
    current_groups: &'a [(String, Vec<Entry>)],
    view_mode: ViewMode,
    favourites: &HashSet<String>,
) -> Vec<&'a Entry> {
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
            .filter(|entry| favourites.contains(&entry.stable_id))
            .collect(),
    }
}

fn entry_matches_search(entry: &Entry, query: &str) -> bool {
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
}

fn refresh_search_matches(search: &mut SearchState, view_entries: &[&Entry]) {
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

pub fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    let (tx, rx) = mpsc::channel::<UiMessage>();
    thread::spawn(move || polling_worker(tx));

    let tick_rate = Duration::from_millis(250);

    let mut header = HeaderInfo::default();
    let mut entries: Option<Vec<Entry>> = None;
    let mut status = "Starting IMSA live timing...".to_string();
    let mut last_error: Option<String> = None;
    let mut last_update: Option<Instant> = None;
    let mut previous_flag = "-".to_string();
    let mut transition_started_at = Instant::now();
    let mut view_mode = ViewMode::Overall;
    let mut selected_row = 0usize;
    let mut config = load_config();
    let mut favourites: HashSet<String> = config.favourites.clone();
    let mut demo_flag = DemoFlagState::default();
    let mut show_help = false;
    let mut search = SearchState::default();

    loop {
        drain_messages(
            &rx,
            &mut header,
            &mut entries,
            &mut status,
            &mut last_error,
            &mut last_update,
        );

        let current_groups = entries
            .as_ref()
            .map(|all_entries| grouped_entries(all_entries))
            .unwrap_or_default();

        if let ViewMode::Class(idx) = view_mode {
            if current_groups.is_empty() {
                view_mode = ViewMode::Overall;
            } else if idx >= current_groups.len() {
                view_mode = ViewMode::Class(current_groups.len() - 1);
            }
        }

        let current_view_len = entries
            .as_ref()
            .map(|all_entries| {
                view_entries_for_mode(all_entries, &current_groups, view_mode, &favourites).len()
            })
            .unwrap_or(0);
        selected_row = selected_row.min(current_view_len.saturating_sub(1));

        let current_view_entries = entries
            .as_ref()
            .map(|all_entries| {
                view_entries_for_mode(all_entries, &current_groups, view_mode, &favourites)
            })
            .unwrap_or_default();
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

        let live_flag = if header.flag.is_empty() {
            "-"
        } else {
            &header.flag
        };
        let effective_flag = if demo_flag.enabled {
            demo_flag_name(demo_flag.idx)
        } else {
            live_flag
        };

        let transition_from_flag = previous_flag.clone();
        if effective_flag != previous_flag {
            previous_flag = effective_flag.to_string();
            transition_started_at = Instant::now();
        }

        terminal.draw(|f| {
            let size = f.size();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(4), Constraint::Min(10)])
                .split(size);

            let age = match last_update {
                Some(t) => format!("Last update: {}s ago", t.elapsed().as_secs()),
                None => "Last update: -".to_string(),
            };

            let tte_text = if header.time_to_go.is_empty() {
                "-"
            } else {
                &header.time_to_go
            };
            let flag_raw = effective_flag;
            let (flag_text, flag_span_style, header_style) =
                animated_flag_theme(flag_raw, &transition_from_flag, transition_started_at);

            let mode_text = view_mode_text(
                view_mode,
                &current_groups
                    .iter()
                    .map(|(name, _)| name.clone())
                    .collect::<Vec<_>>(),
            );

            let event_text = if header.event_name.is_empty() {
                "-"
            } else {
                &header.event_name
            };
            let session_text = if header.session_name.is_empty() {
                "-"
            } else {
                &header.session_name
            };
            let track_text = if header.track_name.is_empty() {
                "-"
            } else {
                &header.track_name
            };

            let header_lead = if track_text != "-" && track_text != event_text {
                format!(
                    "{} | {} | {} | {} | TTE {} | Mode {} | ",
                    status, event_text, session_text, track_text, tte_text, mode_text,
                )
            } else {
                format!(
                    "{} | {} | {} | TTE {} | Mode {} | ",
                    status, event_text, session_text, tte_text, mode_text,
                )
            };

            let mut header_spans = vec![
                Span::styled(header_lead, header_style),
                Span::styled(flag_text, flag_span_style),
            ];

            if demo_flag.enabled {
                header_spans.push(Span::styled(
                    " | DEMO",
                    header_style.add_modifier(Modifier::BOLD),
                ));
            }

            header_spans.push(Span::styled(
                format!(
                    " | Day {} | {} | Favs {}",
                    if header.day_time.is_empty() {
                        "-"
                    } else {
                        &header.day_time
                    },
                    age,
                    favourites.len(),
                ),
                header_style,
            ));
            if search.input_active {
                header_spans.push(Span::styled(
                    format!(" | Search: {}_", search.query),
                    header_style.add_modifier(Modifier::BOLD),
                ));
            } else if !search.query.trim().is_empty() {
                header_spans.push(Span::styled(
                    format!(
                        " | Search: {} ({}/{})",
                        search.query,
                        if search.matches.is_empty() {
                            0
                        } else {
                            search.current_match + 1
                        },
                        search.matches.len(),
                    ),
                    header_style,
                ));
            }

            if let Some(err) = &last_error {
                header_spans.push(Span::styled(format!(" | Error: {}", err), header_style));
            }

            header_spans.push(Span::styled(" | h help | q quit", header_style));

            let status_widget = Paragraph::new(Line::from(header_spans))
                .style(header_style)
                .wrap(Wrap { trim: false })
                .block(
                    Block::default()
                        .title("IMSA TUI")
                        .borders(Borders::ALL)
                        .style(header_style),
                );
            f.render_widget(status_widget, chunks[0]);

            match &entries {
                Some(all_entries) => match view_mode {
                    ViewMode::Overall => {
                        let (visible_entries, start) =
                            visible_slice(all_entries, selected_row, chunks[1].height);
                        let mut state = ratatui::widgets::TableState::default();
                        state.select(Some(selected_row.saturating_sub(start)));
                        let table =
                            build_table("Overall", visible_entries, &favourites, marked_stable_id);
                        f.render_stateful_widget(table, chunks[1], &mut state);
                    }
                    ViewMode::Grouped => {
                        let groups = grouped_entries(all_entries);
                        if groups.is_empty() {
                            let waiting = Paragraph::new("No grouped class data available yet.")
                                .block(Block::default().title("Grouped").borders(Borders::ALL));
                            f.render_widget(waiting, chunks[1]);
                        } else {
                            let constraints: Vec<Constraint> = groups
                                .iter()
                                .map(|_| Constraint::Ratio(1, groups.len() as u32))
                                .collect();
                            let group_chunks = Layout::default()
                                .direction(Direction::Vertical)
                                .constraints(constraints)
                                .split(chunks[1]);

                            let mut global_offset = 0usize;
                            for ((class_name, class_entries), area) in
                                groups.iter().zip(group_chunks.iter())
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
                                let table = build_table(
                                    title,
                                    visible_entries,
                                    &favourites,
                                    marked_stable_id,
                                );
                                f.render_stateful_widget(table, *area, &mut state);
                                global_offset += class_entries.len();
                            }
                        }
                    }
                    ViewMode::Class(idx) => {
                        let groups = grouped_entries(all_entries);
                        if let Some((class_name, class_entries)) = groups.get(idx) {
                            let (visible_entries, start) =
                                visible_slice(class_entries, selected_row, chunks[1].height);
                            let mut state = ratatui::widgets::TableState::default();
                            state.select(Some(selected_row.saturating_sub(start)));
                            let table = build_table(
                                format!("{} ({} cars)", class_name, class_entries.len()),
                                visible_entries,
                                &favourites,
                                marked_stable_id,
                            );
                            f.render_stateful_widget(table, chunks[1], &mut state);
                        } else {
                            let waiting = Paragraph::new("No class data available yet.")
                                .block(Block::default().title("Class").borders(Borders::ALL));
                            f.render_widget(waiting, chunks[1]);
                        }
                    }
                    ViewMode::Favourites => {
                        let favourite_entries: Vec<Entry> = all_entries
                            .iter()
                            .filter(|entry| favourites.contains(&entry.stable_id))
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
                            let mut state = ratatui::widgets::TableState::default();
                            state.select(Some(selected_row.saturating_sub(start)));
                            let table = build_table(
                                format!("Favourites ({} cars)", favourite_entries.len()),
                                visible_entries,
                                &favourites,
                                marked_stable_id,
                            );
                            f.render_stateful_widget(table, chunks[1], &mut state);
                        }
                    }
                },
                None => {
                    let waiting = Paragraph::new(
                        "No timing data yet. Waiting for first successful IMSA snapshot... Press h for help.",
                    )
                    .block(Block::default().title("Overall").borders(Borders::ALL));
                    f.render_widget(waiting, chunks[1]);
                }
            }

            if show_help {
                let area = centered_rect(40, 38, size);
                f.render_widget(Clear, area);
                f.render_widget(help_popup(), area);
            }
        })?;

        if event::poll(tick_rate)? {
            if let Event::Key(key) = event::read()? {
                if search.input_active {
                    match key.code {
                        KeyCode::Esc => {
                            search.input_active = false;
                        }
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
                        KeyCode::Char(c) => {
                            if !c.is_control() {
                                search.query.push(c);
                            }
                        }
                        _ => {}
                    }
                    continue;
                }
                match key.code {
                    KeyCode::Char('h') => {
                        show_help = !show_help;
                    }
                    KeyCode::Esc => {
                        if show_help {
                            show_help = false;
                        } else {
                            return Ok(());
                        }
                    }
                    KeyCode::Char('q') => {
                        if show_help {
                            show_help = false;
                        } else {
                            return Ok(());
                        }
                    }
                    KeyCode::Char('g') if !show_help => {
                        view_mode = next_view_mode(view_mode, current_groups.len());
                        selected_row = 0;
                    }
                    KeyCode::Char('o') if !show_help => {
                        view_mode = ViewMode::Overall;
                        selected_row = 0;
                    }
                    KeyCode::Down | KeyCode::Char('j') if !show_help => {
                        let view_len = current_view_entries.len();
                        selected_row = step_selection(selected_row, view_len, 1);
                    }
                    KeyCode::Up | KeyCode::Char('k') if !show_help => {
                        let view_len = current_view_entries.len();
                        selected_row = step_selection(selected_row, view_len, -1);
                    }
                    KeyCode::PageDown if !show_help => {
                        let jump = 10;
                        let view_len = current_view_entries.len();
                        selected_row = step_selection(selected_row, view_len, jump);
                    }
                    KeyCode::PageUp if !show_help => {
                        let jump = -10;
                        let view_len = current_view_entries.len();
                        selected_row = step_selection(selected_row, view_len, jump);
                    }
                    KeyCode::Home if !show_help => {
                        selected_row = 0;
                    }
                    KeyCode::End if !show_help => {
                        let view_len = current_view_entries.len();
                        selected_row = view_len.saturating_sub(1);
                    }
                    KeyCode::Char(' ') if !show_help => {
                        if let Some(entry) = current_view_entries.get(selected_row) {
                            if favourites.contains(&entry.stable_id) {
                                favourites.remove(&entry.stable_id);
                            } else {
                                favourites.insert(entry.stable_id.clone());
                            }
                            config.favourites = favourites.clone();
                            if let Err(err) = save_config(&config) {
                                last_error = Some(err);
                            }
                        }
                    }
                    KeyCode::Char('f') if !show_help => {
                        if !current_view_entries.is_empty() {
                            for offset in 1..=current_view_entries.len() {
                                let idx = (selected_row + offset) % current_view_entries.len();
                                if favourites.contains(&current_view_entries[idx].stable_id) {
                                    selected_row = idx;
                                    break;
                                }
                            }
                        }
                    }
                    KeyCode::Char('s') if !show_help => {
                        search.query.clear();
                        search.matches.clear();
                        search.current_match = 0;
                        search.input_active = true;
                    }
                    KeyCode::Char('n') if !show_help => {
                        if !search.matches.is_empty() {
                            search.current_match =
                                (search.current_match + 1) % search.matches.len();
                            selected_row = search.matches[search.current_match];
                        }
                    }
                    KeyCode::Char('p') if !show_help => {
                        if !search.matches.is_empty() {
                            if search.current_match == 0 {
                                search.current_match = search.matches.len() - 1;
                            } else {
                                search.current_match -= 1;
                            }
                            selected_row = search.matches[search.current_match];
                        }
                    }
                    KeyCode::Char('r') if !show_help => {
                        if demo_flag.enabled {
                            demo_flag.idx = (demo_flag.idx + 1) % 5;
                        } else {
                            demo_flag.enabled = true;
                            demo_flag.idx = 0;
                        }
                    }
                    KeyCode::Char('0') if !show_help => {
                        demo_flag.enabled = false;
                    }
                    _ => {}
                }
            }
        }
    }
}
