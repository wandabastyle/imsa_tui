use std::{
    io,
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
    },
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap},
    Terminal,
};
use serde_json::{json, Value};
use tungstenite::{
    client::IntoClientRequest,
    connect,
    http::header::{HeaderValue, ORIGIN, USER_AGENT},
    stream::MaybeTlsStream,
    Error as WsError, Message,
};

const WS_URL: &str = "wss://livetiming.azurewebsites.net/";
const EVENT_ID: &str = "20";
const TARGET_CAR: u32 = 632;

#[derive(Debug, Clone)]
struct Entry {
    position: u32,
    car_number: u32,
    class_name: String,
    class_rank: u32,
    driver: String,
    vehicle: String,
    team: String,
    laps: String,
    gap: String,
    last_lap: String,
    best_lap: String,
}

#[derive(Debug, Clone, Default)]
struct SessionInfo {
    session: String,
    track_state: String,
    end_time_raw: u64,
    time_state_raw: String,
    received_at_ms: u64,
}

#[derive(Debug, Clone)]
enum UiMessage {
    Status(String),
    Error(String),
    Entries(Vec<Entry>),
    Session(SessionInfo),
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_millis()
}

fn build_request() -> tungstenite::handshake::client::Request {
    let mut request = WS_URL
        .into_client_request()
        .expect("failed to create websocket request");

    request.headers_mut().insert(
        ORIGIN,
        HeaderValue::from_static("https://livetiming.azurewebsites.net"),
    );
    request
        .headers_mut()
        .insert(USER_AGENT, HeaderValue::from_static("Mozilla/5.0"));

    request
}

fn get_str<'a>(obj: &'a Value, key: &str) -> Option<&'a str> {
    obj.get(key).and_then(|x| x.as_str())
}

fn parse_u32_field(obj: &Value, key: &str) -> Option<u32> {
    if let Some(s) = get_str(obj, key) {
        return s.trim().parse::<u32>().ok();
    }
    obj.get(key)
        .and_then(|x| x.as_u64())
        .and_then(|n| u32::try_from(n).ok())
}

fn normalize_class_name(name: &str) -> String {
    name.chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_uppercase()
}

fn is_at2(name: &str) -> bool {
    normalize_class_name(name) == "AT2"
}

fn entry_from_value(v: &Value) -> Option<Entry> {
    Some(Entry {
        position: parse_u32_field(v, "POSITION")?,
        car_number: parse_u32_field(v, "STNR")?,
        class_name: get_str(v, "CLASSNAME").unwrap_or("-").to_string(),
        class_rank: parse_u32_field(v, "CLASSRANK").unwrap_or(0),
        driver: get_str(v, "NAME").unwrap_or("-").to_string(),
        vehicle: get_str(v, "CAR").unwrap_or("-").to_string(),
        team: get_str(v, "TEAM").unwrap_or("-").to_string(),
        laps: get_str(v, "LAPS").unwrap_or("-").to_string(),
        gap: get_str(v, "GAP").unwrap_or("-").to_string(),
        last_lap: get_str(v, "LASTLAPTIME").unwrap_or("-").to_string(),
        best_lap: get_str(v, "FASTESTLAP").unwrap_or("-").to_string(),
    })
}

fn format_duration_ms(ms: u64) -> String {
    let total_secs = ms / 1000;
    let h = total_secs / 3600;
    let m = (total_secs % 3600) / 60;
    let s = total_secs % 60;
    format!("{h:02}:{m:02}:{s:02}")
}

fn current_time_to_end(session: &SessionInfo) -> String {
    if session.end_time_raw == 0 {
        return "-".to_string();
    }

    let now = now_millis() as u64;

    let remaining_ms = if session.time_state_raw == "0" {
        let elapsed = now.saturating_sub(session.received_at_ms);
        session.end_time_raw.saturating_sub(elapsed)
    } else {
        session.end_time_raw.saturating_sub(now)
    };

    format_duration_ms(remaining_ms)
}

fn track_state_text(raw: &str) -> String {
    match raw {
        "0" => "Normal".to_string(),
        "1" => "Yellow".to_string(),
        "2" => "Code 60".to_string(),
        other => other.to_string(),
    }
}

fn session_text(raw: &str) -> String {
    match raw {
        "R" => "R".to_string(),
        "Q" => "Q".to_string(),
        "T" => "T".to_string(),
        other => other.to_string(),
    }
}

fn parse_ws_message(text: &str) -> Option<UiMessage> {
    let parsed: Value = serde_json::from_str(text).ok()?;
    let pid = get_str(&parsed, "PID")?;

    match pid {
        "0" => {
            let results = parsed.get("RESULT")?.as_array()?;
            let mut entries: Vec<Entry> = results.iter().filter_map(entry_from_value).collect();
            entries.sort_by_key(|e| e.position);
            Some(UiMessage::Entries(entries))
        }
        "4" => {
            let end_time_raw = get_str(&parsed, "ENDTIME")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);

            let time_state_raw = get_str(&parsed, "TIMESTATE").unwrap_or("0").to_string();

            Some(UiMessage::Session(SessionInfo {
                session: session_text(get_str(&parsed, "HEATTYPE").unwrap_or("-")),
                track_state: track_state_text(get_str(&parsed, "TRACKSTATE").unwrap_or("-")),
                end_time_raw,
                time_state_raw,
                received_at_ms: now_millis() as u64,
            }))
        }
        "LTS_TIMESYNC" => None,
        _ => None,
    }
}

fn set_socket_timeout(socket: &mut tungstenite::WebSocket<MaybeTlsStream<std::net::TcpStream>>) {
    match socket.get_mut() {
        MaybeTlsStream::Plain(stream) => {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
        }
        MaybeTlsStream::NativeTls(stream) => {
            let _ = stream.get_ref().set_read_timeout(Some(Duration::from_secs(10)));
        }
        _ => {}
    }
}

fn websocket_worker(tx: Sender<UiMessage>) {
    loop {
        let _ = tx.send(UiMessage::Status("Connecting...".to_string()));

        let request = build_request();
        let connection = connect(request);

        let (mut socket, response) = match connection {
            Ok(ok) => ok,
            Err(err) => {
                let _ = tx.send(UiMessage::Error(format!("connect failed: {err}")));
                thread::sleep(Duration::from_secs(3));
                continue;
            }
        };

        set_socket_timeout(&mut socket);

        let _ = tx.send(UiMessage::Status(format!(
            "Connected ({})",
            response.status()
        )));

        let subscribe = json!({
            "clientLocalTime": now_millis(),
            "eventId": EVENT_ID,
            "eventPid": [0, 4]
        });

        if let Err(err) = socket.send(Message::Text(subscribe.to_string())) {
            let _ = tx.send(UiMessage::Error(format!("subscribe failed: {err}")));
            thread::sleep(Duration::from_secs(3));
            continue;
        }

        let _ = tx.send(UiMessage::Status("Live timing connected".to_string()));

        loop {
            match socket.read() {
                Ok(Message::Text(text)) => {
                    if let Some(msg) = parse_ws_message(&text) {
                        let _ = tx.send(msg);
                    }
                }
                Ok(Message::Binary(data)) => {
                    if let Ok(text) = std::str::from_utf8(&data) {
                        if let Some(msg) = parse_ws_message(text) {
                            let _ = tx.send(msg);
                        }
                    }
                }
                Ok(Message::Ping(data)) => {
                    if let Err(err) = socket.send(Message::Pong(data)) {
                        let _ = tx.send(UiMessage::Error(format!("pong failed: {err}")));
                        break;
                    }
                }
                Ok(Message::Pong(_)) => {}
                Ok(Message::Close(frame)) => {
                    let _ = tx.send(UiMessage::Error(format!("socket closed: {frame:?}")));
                    break;
                }
                Ok(Message::Frame(_)) => {}
                Err(WsError::Io(err))
                    if err.kind() == io::ErrorKind::WouldBlock
                        || err.kind() == io::ErrorKind::TimedOut =>
                {
                    let _ = tx.send(UiMessage::Status("Waiting for live frame...".to_string()));
                }
                Err(err) => {
                    let _ = tx.send(UiMessage::Error(format!("read failed: {err}")));
                    break;
                }
            }
        }

        let _ = tx.send(UiMessage::Status("Reconnecting in 3s...".to_string()));
        thread::sleep(Duration::from_secs(3));
    }
}

fn restore_terminal(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn drain_messages(
    rx: &Receiver<UiMessage>,
    entries: &mut Option<Vec<Entry>>,
    session: &mut SessionInfo,
    status: &mut String,
    last_error: &mut Option<String>,
    last_update: &mut Option<Instant>,
) {
    while let Ok(msg) = rx.try_recv() {
        match msg {
            UiMessage::Status(s) => *status = s,
            UiMessage::Error(err) => *last_error = Some(err),
            UiMessage::Entries(new_entries) => {
                *entries = Some(new_entries);
                *status = "Live timing connected".to_string();
                *last_error = None;
                *last_update = Some(Instant::now());
            }
            UiMessage::Session(info) => {
                if info.session != "-" {
                    session.session = info.session;
                }
                if info.track_state != "-" {
                    session.track_state = info.track_state;
                }
                session.end_time_raw = info.end_time_raw;
                session.time_state_raw = info.time_state_raw;
                session.received_at_ms = info.received_at_ms;
            }
        }
    }
}

fn build_rows(entries: &[Entry]) -> Vec<Row<'static>> {
    entries
        .iter()
        .map(|e| {
            let style = if e.car_number == TARGET_CAR {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else if is_at2(&e.class_name) {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            };

            Row::new(vec![
                Cell::from(e.position.to_string()),
                Cell::from(e.car_number.to_string()),
                Cell::from(e.class_name.clone()),
                Cell::from(e.class_rank.to_string()),
                Cell::from(e.driver.clone()),
                Cell::from(e.vehicle.clone()),
                Cell::from(e.laps.clone()),
                Cell::from(e.gap.clone()),
                Cell::from(e.last_lap.clone()),
                Cell::from(e.best_lap.clone()),
            ])
            .style(style)
        })
        .collect()
}

fn jimmy_summary(entries: &[Entry]) -> String {
    if let Some(e) = entries.iter().find(|e| e.car_number == TARGET_CAR) {
        format!(
            "#{} | Overall P{} | {} P{} | Driver: {} | Team: {} | Vehicle: {} | Laps: {} | Gap: {} | Last: {} | Best: {}",
            e.car_number,
            e.position,
            e.class_name,
            e.class_rank,
            e.driver,
            e.team,
            e.vehicle,
            e.laps,
            e.gap,
            e.last_lap,
            e.best_lap
        )
    } else {
        "Car #632 not present in current frame".to_string()
    }
}

fn slice_around(entries: &[Entry], target_car: u32, before: usize, after: usize) -> &[Entry] {
    if entries.is_empty() {
        return entries;
    }

    let idx = entries
        .iter()
        .position(|e| e.car_number == target_car)
        .unwrap_or(0);

    let start = idx.saturating_sub(before);
    let end = (idx + after + 1).min(entries.len());
    &entries[start..end]
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    let (tx, rx) = mpsc::channel::<UiMessage>();
    thread::spawn(move || websocket_worker(tx));

    let tick_rate = Duration::from_millis(250);

    let mut entries: Option<Vec<Entry>> = None;
    let mut session = SessionInfo {
        session: "-".to_string(),
        track_state: "-".to_string(),
        end_time_raw: 0,
        time_state_raw: "0".to_string(),
        received_at_ms: 0,
    };
    let mut status = "Starting live timing...".to_string();
    let mut last_error: Option<String> = None;
    let mut last_update: Option<Instant> = None;

    loop {
        drain_messages(
            &rx,
            &mut entries,
            &mut session,
            &mut status,
            &mut last_error,
            &mut last_update,
        );

        terminal.draw(|f| {
            let size = f.size();

            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(4),
                    Constraint::Length(5),
                    Constraint::Percentage(50),
                    Constraint::Percentage(41),
                ])
                .split(size);

            let age = match last_update {
                Some(t) => format!("Last update: {}s ago", t.elapsed().as_secs()),
                None => "Last update: -".to_string(),
            };

            let tte = current_time_to_end(&session);

            let status_text = match &last_error {
                Some(err) => format!(
                    "{} | Session {} | TTE {} | Track {} | {} | Error: {} | q quit",
                    status, session.session, tte, session.track_state, age, err
                ),
                None => format!(
                    "{} | Session {} | TTE {} | Track {} | {} | q quit",
                    status, session.session, tte, session.track_state, age
                ),
            };

            let status_widget = Paragraph::new(status_text)
                .wrap(Wrap { trim: false })
                .block(Block::default().title("NLS TUI").borders(Borders::ALL));
            f.render_widget(status_widget, chunks[0]);

            match &entries {
                Some(all_entries) => {
                    let jimmy_widget = Paragraph::new(jimmy_summary(all_entries))
                        .style(
                            Style::default()
                                .fg(Color::Green)
                                .add_modifier(Modifier::BOLD),
                        )
                        .wrap(Wrap { trim: false })
                        .block(
                            Block::default()
                                .title("Jimmy Broadbent / Car #632")
                                .borders(Borders::ALL),
                        );
                    f.render_widget(jimmy_widget, chunks[1]);

                    let leader_count = chunks[2].height.saturating_sub(3) as usize;
                    let leader_entries: Vec<Entry> =
                        all_entries.iter().take(leader_count.max(1)).cloned().collect();

                    let leader_table = Table::new(
                        build_rows(&leader_entries),
                        [
                            Constraint::Length(4),
                            Constraint::Length(5),
                            Constraint::Length(8),
                            Constraint::Length(4),
                            Constraint::Length(18),
                            Constraint::Min(22),
                            Constraint::Length(5),
                            Constraint::Length(10),
                            Constraint::Length(10),
                            Constraint::Length(10),
                        ],
                    )
                    .header(
                        Row::new(vec![
                            "Pos", "#", "Class", "C", "Driver", "Vehicle", "Lap", "Gap", "Last",
                            "Best",
                        ])
                        .style(Style::default().add_modifier(Modifier::BOLD)),
                    )
                    .block(Block::default().title("Leaders").borders(Borders::ALL));
                    f.render_widget(leader_table, chunks[2]);

                    let battle_visible = chunks[3].height.saturating_sub(3) as usize;
                    let before = battle_visible / 2;
                    let after = battle_visible.saturating_sub(before + 1);
                    let battle_slice = slice_around(all_entries, TARGET_CAR, before, after);

                    let battle_table = Table::new(
                        build_rows(battle_slice),
                        [
                            Constraint::Length(4),
                            Constraint::Length(5),
                            Constraint::Length(8),
                            Constraint::Length(4),
                            Constraint::Length(18),
                            Constraint::Min(22),
                            Constraint::Length(5),
                            Constraint::Length(10),
                            Constraint::Length(10),
                            Constraint::Length(10),
                        ],
                    )
                    .header(
                        Row::new(vec![
                            "Pos", "#", "Class", "C", "Driver", "Vehicle", "Lap", "Gap", "Last",
                            "Best",
                        ])
                        .style(Style::default().add_modifier(Modifier::BOLD)),
                    )
                    .block(
                        Block::default()
                            .title("Jimmy Battle (#632)")
                            .borders(Borders::ALL),
                    );
                    f.render_widget(battle_table, chunks[3]);
                }
                None => {
                    let waiting = Paragraph::new(
                        "No timing data yet. Waiting for first successful live frame...",
                    )
                    .block(
                        Block::default()
                            .title("Jimmy Broadbent / Car #632")
                            .borders(Borders::ALL),
                    );

                    let waiting2 = Paragraph::new(
                        "Leaders table will appear once live timing data arrives.",
                    )
                    .block(Block::default().title("Leaders").borders(Borders::ALL));

                    let waiting3 = Paragraph::new(
                        "Jimmy battle table will appear once live timing data arrives.",
                    )
                    .block(
                        Block::default()
                            .title("Jimmy Battle (#632)")
                            .borders(Borders::ALL),
                    );

                    f.render_widget(waiting, chunks[1]);
                    f.render_widget(waiting2, chunks[2]);
                    f.render_widget(waiting3, chunks[3]);
                }
            }
        })?;

        if event::poll(tick_rate)? {
            if let Event::Key(key) = event::read()? {
                if matches!(key.code, KeyCode::Char('q') | KeyCode::Esc) {
                    return Ok(());
                }
            }
        }
    }
}

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
