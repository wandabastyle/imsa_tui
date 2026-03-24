use std::{
    collections::HashSet,
    io,
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Wrap},
    Terminal,
};
use reqwest::blocking::Client;
use serde_json::Value;

const RESULTS_URL: &str = "https://dcqsrdkhg933g.cloudfront.net/RaceResults_JSONP.json";
const RESULTS_CALLBACK: &str = "jsonpRaceResults";
const RACE_DATA_URL: &str = "https://dcqsrdkhg933g.cloudfront.net/RaceData_JSONP.json";
const RACE_DATA_CALLBACK: &str = "jsonpRaceData";
const POLL_INTERVAL: Duration = Duration::from_millis(1500);

/// A single timing row for one car in the IMSA leaderboard.
#[derive(Debug, Clone)]
struct Entry {
    position: u32,
    car_number: String,
    class_name: String,
    class_rank: String,
    driver: String,
    vehicle: String,
    laps: String,
    gap_overall: String,
    gap_class: String,
    gap_next_in_class: String,
    last_lap: String,
    best_lap: String,
    best_lap_no: String,
    pit: String,
    pit_stops: String,
    fastest_driver: String,
}

/// Summary metadata shown in the TUI header bar.
#[derive(Debug, Clone, Default)]
struct HeaderInfo {
    session_name: String,
    event_name: String,
    track_name: String,
    day_time: String,
    flag: String,
    time_to_go: String,
}

/// Messages sent from the polling worker thread to the UI loop.
#[derive(Debug, Clone)]
enum UiMessage {
    Status(String),
    Error(String),
    Snapshot {
        header: HeaderInfo,
        entries: Vec<Entry>,
    },
}

/// Determines which leaderboard view is currently rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewMode {
    Overall,
    Grouped,
    Class(usize),
    Favourites,
}

/// Stores optional demo-flag override state for local UI testing.
#[derive(Debug, Clone, Default)]
struct DemoFlagState {
    enabled: bool,
    idx: usize,
}

/// Returns a synthetic flag name for demo mode rotation.
fn demo_flag_name(idx: usize) -> &'static str {
    match idx % 5 {
        0 => "Green",
        1 => "Yellow",
        2 => "Red",
        3 => "White",
        _ => "Checkered",
    }
}

/// Builds a centered rectangle sized by percentages of the full area.
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

/// Creates the keyboard-help popup paragraph widget.
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

/// Restores the terminal to canonical mode before exiting.
fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

/// Returns current UNIX epoch time in milliseconds for cache-busting requests.
fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_millis()
}

fn get_str<'a>(obj: &'a Value, key: &str) -> Option<&'a str> {
    obj.get(key).and_then(|x| x.as_str())
}

fn get_u64(obj: &Value, key: &str) -> Option<u64> {
    obj.get(key).and_then(|x| x.as_u64())
}

fn looks_like_mojibake(s: &str) -> bool {
    s.contains("Ã") || s.contains("Â") || s.contains("â€") || s.contains("â€“") || s.contains("â€”")
}

fn fix_mojibake(s: &str) -> String {
    if !looks_like_mojibake(s) {
        return s.to_string();
    }

    let bytes: Option<Vec<u8>> = s.chars().map(|c| u8::try_from(c as u32).ok()).collect();
    let Some(bytes) = bytes else {
        return s.to_string();
    };

    match String::from_utf8(bytes) {
        Ok(decoded) => decoded,
        Err(_) => s.to_string(),
    }
}

fn clean_string(s: &str) -> String {
    fix_mojibake(s.trim())
}

fn as_string(obj: &Value, key: &str) -> String {
    if let Some(s) = get_str(obj, key) {
        let cleaned = clean_string(s);
        if !cleaned.is_empty() {
            return cleaned;
        }
    }
    if let Some(n) = get_u64(obj, key) {
        return n.to_string();
    }
    "-".to_string()
}

fn parse_position(obj: &Value) -> Option<u32> {
    if let Some(n) = obj.get("A").and_then(|v| v.as_u64()) {
        return u32::try_from(n).ok();
    }
    if let Some(s) = get_str(obj, "A") {
        return s.trim().parse::<u32>().ok();
    }
    None
}

fn parse_pit(obj: &Value) -> String {
    match obj.get("P") {
        Some(Value::Bool(true)) => "Yes".to_string(),
        Some(Value::Bool(false)) => "No".to_string(),
        Some(Value::Number(n)) if n.as_i64() == Some(1) => "Yes".to_string(),
        Some(Value::Number(n)) if n.as_i64() == Some(0) => "No".to_string(),
        Some(Value::String(s)) if s == "1" => "Yes".to_string(),
        Some(Value::String(s)) if s == "0" => "No".to_string(),
        Some(v) => {
            let s = v.to_string();
            if s == "\"\"" {
                "-".to_string()
            } else {
                s.trim_matches('"').to_string()
            }
        }
        None => "-".to_string(),
    }
}

fn parse_entry(obj: &Value) -> Option<Entry> {
    let position = parse_position(obj)?;

    Some(Entry {
        position,
        car_number: as_string(obj, "N"),
        class_name: as_string(obj, "C"),
        class_rank: as_string(obj, "PIC"),
        driver: as_string(obj, "F"),
        vehicle: as_string(obj, "V"),
        laps: as_string(obj, "L"),
        gap_overall: as_string(obj, "D"),
        gap_class: as_string(obj, "DIC"),
        gap_next_in_class: as_string(obj, "GIC"),
        last_lap: as_string(obj, "LL"),
        best_lap: as_string(obj, "BL"),
        best_lap_no: as_string(obj, "IN"),
        pit: parse_pit(obj),
        pit_stops: as_string(obj, "PS"),
        fastest_driver: as_string(obj, "FD"),
    })
}

/// Parses either raw JSON or JSONP payload text into a JSON value.
fn parse_jsonp_body(text: &str, callback: &str) -> Result<Value, String> {
    let trimmed = text.trim();

    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return serde_json::from_str(trimmed).map_err(|e| format!("json parse failed: {e}"));
    }

    let prefix = format!("{callback}(");
    if !trimmed.starts_with(&prefix) {
        return Err(format!(
            "response is neither raw JSON nor expected JSONP callback {callback}"
        ));
    }

    let start = prefix.len();
    let end = trimmed
        .rfind(')')
        .ok_or_else(|| "jsonp closing ')' not found".to_string())?;

    let inner = trimmed[start..end].trim();
    serde_json::from_str(inner).map_err(|e| format!("jsonp inner json parse failed: {e}"))
}

fn first_present_string(root: &Value, keys: &[&str]) -> String {
    for key in keys {
        let v = as_string(root, key);
        if v != "-" {
            return v;
        }
    }
    "-".to_string()
}

fn parse_flag_code(code: &str) -> String {
    match code.trim() {
        "0" | "1" | "" => "Green".to_string(),
        "2" => "Yellow".to_string(),
        "3" => "Red".to_string(),
        "4" => "Checkered".to_string(),
        other if !other.is_empty() => other.to_string(),
        _ => "Green".to_string(),
    }
}

fn build_results_header(root: &Value) -> HeaderInfo {
    HeaderInfo {
        session_name: first_present_string(root, &["S", "Session", "session", "sessionName"]),
        event_name: first_present_string(root, &["E", "Event", "event", "eventName"]),
        track_name: first_present_string(root, &["T", "Track", "track", "trackName"]),
        day_time: first_present_string(root, &["DT", "Day", "day", "dayTime", "timestamp"]),
        flag: "-".to_string(),
        time_to_go: "-".to_string(),
    }
}

fn merge_race_data_into_header(header: &mut HeaderInfo, race_data: &Value) {
    let day_time = first_present_string(race_data, &["A"]);
    if day_time != "-" {
        header.day_time = day_time;
    }

    let raw_time_to_go = first_present_string(race_data, &["T", "B"]);
    let time_to_go = clean_time_to_go(&raw_time_to_go);
    if time_to_go != "-" {
        header.time_to_go = time_to_go;
    }

    let raw_flag = first_present_string(race_data, &["C"]);
    let parsed_flag = parse_flag_code(&raw_flag);
    if parsed_flag != "-" {
        header.flag = parsed_flag;
    }

    let maybe_session = first_present_string(race_data, &["Session", "S"]);
    if maybe_session != "-" {
        header.session_name = maybe_session;
    }
}

fn parse_results_snapshot(root: &Value) -> Result<(HeaderInfo, Vec<Entry>), String> {
    if let Some(cars) = root.get("B").and_then(|v| v.as_array()) {
        let mut entries: Vec<Entry> = cars.iter().filter_map(parse_entry).collect();
        entries.sort_by_key(|e| e.position);
        return Ok((build_results_header(root), entries));
    }

    if let Some(cars) = root.get("RaceResults").and_then(|v| v.as_array()) {
        let mut entries: Vec<Entry> = cars.iter().filter_map(parse_entry).collect();
        entries.sort_by_key(|e| e.position);
        return Ok((build_results_header(root), entries));
    }

    if let Some(cars) = root.as_array() {
        let mut entries: Vec<Entry> = cars.iter().filter_map(parse_entry).collect();
        entries.sort_by_key(|e| e.position);
        return Ok((build_results_header(root), entries));
    }

    if let Some(obj) = root.as_object() {
        let mut keys: Vec<String> = obj.keys().cloned().collect();
        keys.sort();
        return Err(format!(
            "unexpected JSON shape; top-level keys: {}",
            keys.join(", ")
        ));
    }

    Err("unexpected JSON shape; top-level value is not object/array".to_string())
}

fn fetch_url_text(client: &Client, url: &str) -> Result<String, String> {
    let response = client
        .get(url)
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/123 Safari/537.36",
        )
        .header("Accept", "application/javascript, application/json, text/plain, */*")
        .header("Accept-Language", "en-US,en;q=0.9")
        .header("Referer", "https://www.imsa.com/scoring/")
        .header("Origin", "https://www.imsa.com")
        .header("Cache-Control", "no-cache")
        .header("Pragma", "no-cache")
        .send()
        .map_err(|e| format!("request failed: {e}"))?;

    let status = response.status();
    if !status.is_success() {
        return Err(format!("http {status}"));
    }

    response
        .text()
        .map_err(|e| format!("body read failed: {e}"))
}

/// Combines race-results and race-data endpoints into one snapshot for rendering.
fn fetch_snapshot(client: &Client) -> Result<(HeaderInfo, Vec<Entry>), String> {
    let results_url = format!(
        "{RESULTS_URL}?callback={RESULTS_CALLBACK}&_={}",
        now_millis()
    );
    let results_text = fetch_url_text(client, &results_url)?;
    let results_root = parse_jsonp_body(&results_text, RESULTS_CALLBACK)?;
    let (mut header, entries) = parse_results_snapshot(&results_root)?;

    let race_data_url = format!(
        "{RACE_DATA_URL}?callback={RACE_DATA_CALLBACK}&_={}",
        now_millis()
    );
    let race_data_text = fetch_url_text(client, &race_data_url)?;
    let race_data_root = parse_jsonp_body(&race_data_text, RACE_DATA_CALLBACK)?;
    merge_race_data_into_header(&mut header, &race_data_root);

    Ok((header, entries))
}

fn normalize_class_name(name: &str) -> String {
    name.chars()
        .filter(|c| !c.is_whitespace() && *c != '_')
        .collect::<String>()
        .to_uppercase()
}

fn clean_time_to_go(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "-" {
        return "-".to_string();
    }

    trimmed
        .strip_prefix("Time to go:")
        .unwrap_or(trimmed)
        .trim()
        .to_string()
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

/// Produces the display theme for the current flag, including transition animation.
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
            let trimmed = clean_string(name);
            if trimmed.is_empty() {
                "-".to_string()
            } else {
                trimmed
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
        Constraint::Length(4),  // Pos
        Constraint::Length(5),  // #
        Constraint::Length(7),  // Class
        Constraint::Length(4),  // PIC
        Constraint::Length(24), // Driver
        Constraint::Min(16),    // Vehicle
        Constraint::Length(6),  // Laps
        Constraint::Length(11), // Gap O
        Constraint::Length(11), // Gap C
        Constraint::Length(11), // Next C
        Constraint::Length(10), // Last
        Constraint::Length(10), // Best
        Constraint::Length(5),  // BL#
        Constraint::Length(5),  // Pit
        Constraint::Length(5),  // Stop
        Constraint::Length(18), // Fastest Driver
    ]
}

fn build_table<'a>(
    title: impl Into<String>,
    entries: &'a [Entry],
    favourites: &HashSet<String>,
) -> Table<'a> {
    Table::new(build_rows(entries, favourites), table_widths())
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

/// Groups entries by supported IMSA classes in display order.
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
/// Worker loop that polls IMSA endpoints and streams snapshots to the UI.
fn polling_worker(tx: Sender<UiMessage>) {
    let client = match Client::builder()
        .timeout(Duration::from_secs(12))
        .brotli(true)
        .gzip(true)
        .deflate(true)
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            let _ = tx.send(UiMessage::Error(format!("client init failed: {e}")));
            return;
        }
    };

    loop {
        let _ = tx.send(UiMessage::Status(
            "Fetching IMSA live timing...".to_string(),
        ));

        match fetch_snapshot(&client) {
            Ok((header, entries)) => {
                let _ = tx.send(UiMessage::Snapshot { header, entries });
            }
            Err(err) => {
                let _ = tx.send(UiMessage::Error(err));
            }
        }

        thread::sleep(POLL_INTERVAL);
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

fn build_rows(entries: &[Entry], favourites: &HashSet<String>) -> Vec<Row<'static>> {
    entries
        .iter()
        .map(|e| {
            let fav_marker = if favourites.contains(&e.car_number) {
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
            .style(class_style(&e.class_name))
        })
        .collect()
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

fn cycle_to_next_favourite(
    entries: &[Entry],
    favourites: &HashSet<String>,
    selected_idx: usize,
) -> usize {
    if entries.is_empty() || favourites.is_empty() {
        return selected_idx.min(entries.len().saturating_sub(1));
    }

    for offset in 1..=entries.len() {
        let idx = (selected_idx + offset) % entries.len();
        if favourites.contains(&entries[idx].car_number) {
            return idx;
        }
    }
    selected_idx
}

/// Draws the full terminal UI and handles keyboard events until exit.
fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
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
    let mut favourites: HashSet<String> = HashSet::new();
    let mut demo_flag = DemoFlagState::default();
    let mut show_help = false;

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

        let current_view_len = match (&entries, view_mode) {
            (Some(all_entries), ViewMode::Overall) => all_entries.len(),
            (Some(all_entries), ViewMode::Grouped) => all_entries.len(),
            (Some(_), ViewMode::Class(idx)) => current_groups
                .get(idx)
                .map(|(_, class_entries)| class_entries.len())
                .unwrap_or(0),
            (Some(all_entries), ViewMode::Favourites) => all_entries
                .iter()
                .filter(|entry| favourites.contains(&entry.car_number))
                .count(),
            _ => 0,
        };
        selected_row = selected_row.min(current_view_len.saturating_sub(1));

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
                header_spans.push(Span::styled(" | DEMO", header_style.add_modifier(Modifier::BOLD)));
            }

            header_spans.push(Span::styled(
                format!(
                    " | Day {} | {} | Favs {}",
                    if header.day_time.is_empty() { "-" } else { &header.day_time },
                    age,
                    favourites.len(),
                ),
                header_style,
            ));

            if let Some(err) = &last_error {
                header_spans.push(Span::styled(format!(" | Error: {}", err), header_style));
            }

            header_spans.push(Span::styled(" | h help | q quit", header_style));

            let status_widget = Paragraph::new(Line::from(header_spans))
                .style(header_style)
                .wrap(Wrap { trim: false })
                .block(Block::default().title("IMSA TUI").borders(Borders::ALL).style(header_style));
            f.render_widget(status_widget, chunks[0]);

            match &entries {
                Some(all_entries) => match view_mode {
                    ViewMode::Overall => {
                        let (visible_entries, start) = visible_slice(all_entries, selected_row, chunks[1].height);
                        let mut state = ratatui::widgets::TableState::default();
                        state.select(Some(selected_row.saturating_sub(start)));
                        let table = build_table("Overall", visible_entries, &favourites);
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
                            for ((class_name, class_entries), area) in groups.iter().zip(group_chunks.iter()) {
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
                                let table = build_table(title, visible_entries, &favourites);
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
                            .filter(|entry| favourites.contains(&entry.car_number))
                            .cloned()
                            .collect();
                        if favourite_entries.is_empty() {
                            let waiting = Paragraph::new("No favourites yet. Select a car and press space.")
                                .block(Block::default().title("Favourites").borders(Borders::ALL));
                            f.render_widget(waiting, chunks[1]);
                        } else {
                            let (visible_entries, start) =
                                visible_slice(&favourite_entries, selected_row, chunks[1].height);
                            let mut state = ratatui::widgets::TableState::default();
                            state.select(Some(selected_row.saturating_sub(start)));
                            let table =
                                build_table(format!("Favourites ({} cars)", favourite_entries.len()), visible_entries, &favourites);
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
                        let view_len = match (&entries, view_mode) {
                            (Some(all_entries), ViewMode::Overall) => all_entries.len(),
                            (Some(all_entries), ViewMode::Grouped) => all_entries.len(),
                            (Some(_), ViewMode::Class(idx)) => current_groups
                                .get(idx)
                                .map(|(_, class_entries)| class_entries.len())
                                .unwrap_or(0),
                            (Some(all_entries), ViewMode::Favourites) => all_entries
                                .iter()
                                .filter(|entry| favourites.contains(&entry.car_number))
                                .count(),
                            _ => 0,
                        };
                        selected_row = step_selection(selected_row, view_len, 1);
                    }
                    KeyCode::Up | KeyCode::Char('k') if !show_help => {
                        let view_len = match (&entries, view_mode) {
                            (Some(all_entries), ViewMode::Overall) => all_entries.len(),
                            (Some(all_entries), ViewMode::Grouped) => all_entries.len(),
                            (Some(_), ViewMode::Class(idx)) => current_groups
                                .get(idx)
                                .map(|(_, class_entries)| class_entries.len())
                                .unwrap_or(0),
                            (Some(all_entries), ViewMode::Favourites) => all_entries
                                .iter()
                                .filter(|entry| favourites.contains(&entry.car_number))
                                .count(),
                            _ => 0,
                        };
                        selected_row = step_selection(selected_row, view_len, -1);
                    }
                    KeyCode::PageDown if !show_help => {
                        let jump = 10;
                        let view_len = match (&entries, view_mode) {
                            (Some(all_entries), ViewMode::Overall) => all_entries.len(),
                            (Some(all_entries), ViewMode::Grouped) => all_entries.len(),
                            (Some(_), ViewMode::Class(idx)) => current_groups
                                .get(idx)
                                .map(|(_, class_entries)| class_entries.len())
                                .unwrap_or(0),
                            (Some(all_entries), ViewMode::Favourites) => all_entries
                                .iter()
                                .filter(|entry| favourites.contains(&entry.car_number))
                                .count(),
                            _ => 0,
                        };
                        selected_row = step_selection(selected_row, view_len, jump);
                    }
                    KeyCode::PageUp if !show_help => {
                        let jump = -10;
                        let view_len = match (&entries, view_mode) {
                            (Some(all_entries), ViewMode::Overall) => all_entries.len(),
                            (Some(all_entries), ViewMode::Grouped) => all_entries.len(),
                            (Some(_), ViewMode::Class(idx)) => current_groups
                                .get(idx)
                                .map(|(_, class_entries)| class_entries.len())
                                .unwrap_or(0),
                            (Some(all_entries), ViewMode::Favourites) => all_entries
                                .iter()
                                .filter(|entry| favourites.contains(&entry.car_number))
                                .count(),
                            _ => 0,
                        };
                        selected_row = step_selection(selected_row, view_len, jump);
                    }
                    KeyCode::Home if !show_help => {
                        selected_row = 0;
                    }
                    KeyCode::End if !show_help => {
                        let view_len = match (&entries, view_mode) {
                            (Some(all_entries), ViewMode::Overall) => all_entries.len(),
                            (Some(all_entries), ViewMode::Grouped) => all_entries.len(),
                            (Some(_), ViewMode::Class(idx)) => current_groups
                                .get(idx)
                                .map(|(_, class_entries)| class_entries.len())
                                .unwrap_or(0),
                            (Some(all_entries), ViewMode::Favourites) => all_entries
                                .iter()
                                .filter(|entry| favourites.contains(&entry.car_number))
                                .count(),
                            _ => 0,
                        };
                        selected_row = view_len.saturating_sub(1);
                    }
                    KeyCode::Char(' ') if !show_help => {
                        if let Some(all_entries) = &entries {
                            let selected = match view_mode {
                                ViewMode::Overall | ViewMode::Grouped => {
                                    all_entries.get(selected_row)
                                }
                                ViewMode::Class(idx) => current_groups
                                    .get(idx)
                                    .and_then(|(_, class_entries)| class_entries.get(selected_row)),
                                ViewMode::Favourites => all_entries
                                    .iter()
                                    .filter(|entry| favourites.contains(&entry.car_number))
                                    .nth(selected_row),
                            };
                            if let Some(entry) = selected {
                                if favourites.contains(&entry.car_number) {
                                    favourites.remove(&entry.car_number);
                                } else {
                                    favourites.insert(entry.car_number.clone());
                                }
                            }
                        }
                    }
                    KeyCode::Char('f') if !show_help => {
                        if let Some(all_entries) = &entries {
                            match view_mode {
                                ViewMode::Overall | ViewMode::Grouped => {
                                    selected_row = cycle_to_next_favourite(
                                        all_entries,
                                        &favourites,
                                        selected_row,
                                    );
                                }
                                ViewMode::Class(idx) => {
                                    if let Some((_, class_entries)) = current_groups.get(idx) {
                                        selected_row = cycle_to_next_favourite(
                                            class_entries,
                                            &favourites,
                                            selected_row,
                                        );
                                    }
                                }
                                ViewMode::Favourites => {
                                    let count = all_entries
                                        .iter()
                                        .filter(|entry| favourites.contains(&entry.car_number))
                                        .count();
                                    selected_row = step_selection(selected_row, count, 1);
                                    if count > 0 && selected_row >= count {
                                        selected_row = 0;
                                    }
                                }
                            }
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

/// Initializes terminal state, runs the app loop, and ensures cleanup on exit.
fn main() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let app_result = run_app(&mut terminal);
    let restore_result = restore_terminal(&mut terminal);

    match (app_result, restore_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(app_err), Ok(())) => Err(app_err),
        (Ok(()), Err(restore_err)) => Err(restore_err),
        (Err(app_err), Err(_)) => Err(app_err),
    }
}
