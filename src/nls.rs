// NLS websocket adapter: subscribes to livetiming hub events and maps payloads to timing rows.

use std::{
    io,
    sync::mpsc::{Receiver, Sender},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use reqwest::blocking::Client;
use serde_json::{json, Value};
use tungstenite::{
    client::IntoClientRequest,
    connect,
    http::header::{HeaderValue, ORIGIN, USER_AGENT},
    stream::MaybeTlsStream,
    Error as WsError, Message,
};

use crate::timing::{TimingEntry, TimingHeader, TimingMessage};

const WS_URL: &str = "wss://livetiming.azurewebsites.net/";
const DEFAULT_NLS_EVENT_ID: &str = "20";
const N24_EVENT_ID: &str = "50";
const NLS_HOME_URL: &str = "https://www.nuerburgring-langstrecken-serie.de/language/de/startseite/";
const N24_TERMINE_URL: &str = "https://www.24h-rennen.de/termine/";
const N24_TARGET_EVENT_TITLE: &str = "ADAC RAVENOL 24h Nürburgring";
const WEBSITE_EVENT_REFRESH_INTERVAL: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone)]
struct CountdownState {
    end_time_raw: u64,
    time_state_raw: String,
    received_at_ms: u64,
    is_race_session: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct CalendarDate {
    year: i32,
    month: u32,
    day: u32,
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

fn first_non_empty<'a>(obj: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .filter_map(|key| get_str(obj, key))
        .map(str::trim)
        .find(|value| !value.is_empty())
}

fn strip_tags(raw: &str) -> String {
    let mut output = String::with_capacity(raw.len());
    let mut in_tag = false;
    for ch in raw.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => output.push(ch),
            _ => {}
        }
    }
    output
}

fn normalize_spaces(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn extract_between<'a>(haystack: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let from = haystack.find(start)? + start.len();
    let rest = &haystack[from..];
    let to = rest.find(end)?;
    Some(&rest[..to])
}

fn parse_event_name_from_homepage(html: &str) -> Option<String> {
    let h1_marker = "<h1 class=\"font-weight-600 alt-font text-white width-95 sm-width-100\">";
    let h1_start = html.find(h1_marker)?;
    let h1_raw = extract_between(html, h1_marker, "</h1>")?;
    let nls_code = normalize_spaces(&strip_tags(h1_raw));
    if nls_code.is_empty() {
        return None;
    }

    let h5_raw = extract_between(&html[h1_start..], "<h5>", "</h5>")?;
    let h5_text = normalize_spaces(&strip_tags(h5_raw));
    let race_title = if let Some((first, rest)) = h5_text.split_once(' ') {
        if first.chars().filter(|c| *c == '.').count() == 2 && !rest.trim().is_empty() {
            rest.trim().to_string()
        } else {
            h5_text
        }
    } else {
        h5_text
    };

    if race_title.is_empty() {
        return None;
    }

    Some(format!("{} - {}", nls_code, race_title))
}

fn fetch_homepage_event_name(client: &Client) -> Option<String> {
    let response = client.get(NLS_HOME_URL).send().ok()?;
    let html = response.text().ok()?;
    parse_event_name_from_homepage(&html)
}

fn decode_basic_html_entities(raw: &str) -> String {
    raw.replace("&#8211;", "-")
        .replace("&#8212;", "-")
        .replace("&ndash;", "-")
        .replace("&mdash;", "-")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
}

fn html_to_text_lines(html: &str) -> Vec<String> {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;

    for ch in html.chars() {
        match ch {
            '<' => {
                in_tag = true;
                text.push('\n');
            }
            '>' => {
                in_tag = false;
                text.push('\n');
            }
            _ if !in_tag => text.push(ch),
            _ => {}
        }
    }

    decode_basic_html_entities(&text)
        .lines()
        .map(str::trim)
        .map(normalize_spaces)
        .filter(|line| !line.is_empty())
        .collect()
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

fn days_in_month(year: i32, month: u32) -> Option<u32> {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => Some(31),
        4 | 6 | 9 | 11 => Some(30),
        2 if is_leap_year(year) => Some(29),
        2 => Some(28),
        _ => None,
    }
}

fn parse_u32_fragment(raw: &str) -> Option<u32> {
    let digits: String = raw.chars().filter(|ch| ch.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse::<u32>().ok()
    }
}

fn parse_german_date_range(raw: &str) -> Option<(CalendarDate, CalendarDate)> {
    let normalized = normalize_spaces(&raw.replace(['–', '—', '−'], "-"));
    let (left, right) = normalized.split_once('-')?;

    let start_day = parse_u32_fragment(left)?;
    let mut right_parts = right.trim().split('.').map(str::trim);
    let end_day = parse_u32_fragment(right_parts.next()?)?;
    let month = parse_u32_fragment(right_parts.next()?)?;
    let year = i32::try_from(parse_u32_fragment(right_parts.next()?)?).ok()?;

    let max_day = days_in_month(year, month)?;
    if start_day == 0 || end_day == 0 || start_day > max_day || end_day > max_day {
        return None;
    }

    Some((
        CalendarDate {
            year,
            month,
            day: start_day,
        },
        CalendarDate {
            year,
            month,
            day: end_day,
        },
    ))
}

fn extract_target_date_range(lines: &[String], year: i32) -> Option<(CalendarDate, CalendarDate)> {
    let target_idx = lines
        .iter()
        .position(|line| line.contains(N24_TARGET_EVENT_TITLE))?;

    for line in lines.iter().skip(target_idx + 1).take(12) {
        let Some((start, end)) = parse_german_date_range(line) else {
            continue;
        };
        if start.year == year && end.year == year {
            return Some((start, end));
        }
    }

    None
}

fn local_today() -> Option<CalendarDate> {
    let mut timestamp: libc::time_t = 0;
    // SAFETY: `time` writes to a valid pointer and `localtime_r` initializes `tm`.
    unsafe {
        if libc::time(&mut timestamp) < 0 {
            return None;
        }
        let mut local_tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&timestamp, &mut local_tm).is_null() {
            return None;
        }
        Some(CalendarDate {
            year: local_tm.tm_year + 1900,
            month: u32::try_from(local_tm.tm_mon + 1).ok()?,
            day: u32::try_from(local_tm.tm_mday).ok()?,
        })
    }
}

fn determine_active_nuerburgring_event_id(client: &Client) -> Result<&'static str, String> {
    let today = local_today().ok_or_else(|| "failed to resolve local date".to_string())?;

    let response = client
        .get(N24_TERMINE_URL)
        .send()
        .map_err(|err| format!("failed to fetch 24h schedule: {err}"))?;
    let html = response
        .text()
        .map_err(|err| format!("failed to read 24h schedule: {err}"))?;

    let lines = html_to_text_lines(&html);
    let (start, end) = extract_target_date_range(&lines, today.year).ok_or_else(|| {
        format!(
            "could not parse {} date range for {}",
            N24_TARGET_EVENT_TITLE, today.year
        )
    })?;

    if today >= start && today <= end {
        Ok(N24_EVENT_ID)
    } else {
        Ok(DEFAULT_NLS_EVENT_ID)
    }
}

fn parse_u32_field(obj: &Value, key: &str) -> Option<u32> {
    if let Some(s) = get_str(obj, key) {
        return s.trim().parse::<u32>().ok();
    }
    obj.get(key)
        .and_then(|x| x.as_u64())
        .and_then(|n| u32::try_from(n).ok())
}

fn non_empty_field(obj: &Value, key: &str) -> Option<String> {
    if let Some(raw) = get_str(obj, key) {
        let value = raw.trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }

    if let Some(n) = obj.get(key).and_then(|x| x.as_u64()) {
        return Some(n.to_string());
    }

    None
}

fn sector_field(v: &Value, sector_no: usize) -> String {
    let candidates: &[&str] = match sector_no {
        1 => &["S1TIME", "S1"],
        2 => &["S2TIME", "S2"],
        3 => &["S3TIME", "S3"],
        4 => &["S4TIME", "S4"],
        5 => &["S5TIME", "S5"],
        _ => &[],
    };

    if let Some(value) = candidates.iter().find_map(|key| non_empty_field(v, key)) {
        return value;
    }

    "-".to_string()
}

fn pit_flag_from_inout_state(inout_state: &str) -> String {
    let normalized = inout_state.trim().to_ascii_uppercase();
    if normalized.is_empty() || normalized == "-" {
        return "-".to_string();
    }

    if normalized.contains("OUT") {
        return "No".to_string();
    }

    if normalized.contains("IN") || normalized.contains("PIT") || normalized.contains("BOX") {
        return "Yes".to_string();
    }

    "-".to_string()
}

fn entry_from_value(v: &Value) -> Option<TimingEntry> {
    let car_number = parse_u32_field(v, "STNR")?.to_string();
    let class_name = get_str(v, "CLASSNAME").unwrap_or("-").to_string();
    let stable_id = format!("stnr:{car_number}");

    let sector_1 = sector_field(v, 1);
    let sector_2 = sector_field(v, 2);
    let sector_3 = sector_field(v, 3);
    let sector_4 = sector_field(v, 4);
    let sector_5 = sector_field(v, 5);

    Some(TimingEntry {
        position: parse_u32_field(v, "POSITION")?,
        car_number,
        class_name,
        class_rank: parse_u32_field(v, "CLASSRANK").unwrap_or(0).to_string(),
        driver: get_str(v, "NAME").unwrap_or("-").to_string(),
        vehicle: get_str(v, "CAR").unwrap_or("-").to_string(),
        team: get_str(v, "TEAM").unwrap_or("-").to_string(),
        laps: get_str(v, "LAPS").unwrap_or("-").to_string(),
        gap_overall: get_str(v, "GAP").unwrap_or("-").to_string(),
        gap_class: "-".to_string(),
        gap_next_in_class: "-".to_string(),
        last_lap: get_str(v, "LASTLAPTIME").unwrap_or("-").to_string(),
        best_lap: get_str(v, "FASTESTLAP").unwrap_or("-").to_string(),
        sector_1,
        sector_2,
        sector_3,
        sector_4,
        sector_5: sector_5.clone(),
        best_lap_no: "-".to_string(),
        pit: pit_flag_from_inout_state(&sector_5),
        pit_stops: "-".to_string(),
        fastest_driver: "-".to_string(),
        stable_id,
    })
}

fn format_duration_ms(ms: u64) -> String {
    let total_secs = ms / 1000;
    let h = total_secs / 3600;
    let m = (total_secs % 3600) / 60;
    let s = total_secs % 60;
    format!("{h:02}:{m:02}:{s:02}")
}

fn current_time_to_end(
    header: &TimingHeader,
    end_time_raw: u64,
    time_state_raw: &str,
    received_at_ms: u64,
) -> String {
    current_time_to_end_at(
        header,
        end_time_raw,
        time_state_raw,
        received_at_ms,
        now_millis() as u64,
    )
}

fn current_time_to_end_at(
    header: &TimingHeader,
    end_time_raw: u64,
    time_state_raw: &str,
    received_at_ms: u64,
    now_ms: u64,
) -> String {
    if end_time_raw == 0 {
        return header.time_to_go.clone();
    }

    let remaining_ms = if time_state_raw == "0" {
        let elapsed = now_ms.saturating_sub(received_at_ms);
        end_time_raw.saturating_sub(elapsed)
    } else {
        end_time_raw.saturating_sub(now_ms)
    };

    format_duration_ms(remaining_ms)
}

fn refresh_header_time_to_go(header: &mut TimingHeader, countdown: Option<&CountdownState>) {
    let Some(countdown) = countdown else {
        return;
    };

    header.time_to_go = current_time_to_end(
        header,
        countdown.end_time_raw,
        &countdown.time_state_raw,
        countdown.received_at_ms,
    );

    if should_promote_to_checkered(header, countdown.is_race_session) {
        header.flag = "Checkered".to_string();
    }
}

fn is_zero_time_to_go(value: &str) -> bool {
    let trimmed = value.trim();
    matches!(trimmed, "0" | "0:00" | "00:00" | "00:00:00")
}

fn is_unknown_time_to_go(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.is_empty() || trimmed == "-"
}

fn should_promote_to_checkered_with_inputs(
    flag: &str,
    time_to_go: &str,
    is_race_session: bool,
) -> bool {
    let normalized_flag = flag.trim();
    let flag_is_promotable =
        normalized_flag == "-" || normalized_flag.eq_ignore_ascii_case("green");
    if !flag_is_promotable {
        return false;
    }

    (is_zero_time_to_go(time_to_go) || is_unknown_time_to_go(time_to_go)) && is_race_session
}

fn should_promote_to_checkered(header: &TimingHeader, is_race_session: bool) -> bool {
    should_promote_to_checkered_with_inputs(&header.flag, &header.time_to_go, is_race_session)
}

fn track_state_text(raw: &str) -> String {
    match raw {
        "0" => "Green".to_string(),
        "1" => "Yellow".to_string(),
        "2" => "Code 60".to_string(),
        other => other.to_string(),
    }
}

fn session_text(raw: &str) -> String {
    match raw {
        "R" => "Race".to_string(),
        "Q" => "Qualifying".to_string(),
        "T" => "Practice".to_string(),
        other => other.to_string(),
    }
}

fn parse_ws_message(
    text: &str,
    header: &mut TimingHeader,
    website_event_name: Option<&str>,
    countdown: &mut Option<CountdownState>,
    is_race_session: &mut bool,
) -> Option<(Option<Vec<TimingEntry>>, bool)> {
    let parsed: Value = serde_json::from_str(text).ok()?;
    let pid = get_str(&parsed, "PID")?;

    match pid {
        "0" => {
            // PID 0 carries the "what event/session is this" metadata in addition
            // to ranking rows. Prefer HEAT/CUP/TRACKNAME so header wording matches
            // the official NLS leaderboard labels.
            if let Some(session_name) = first_non_empty(&parsed, &["HEAT"]) {
                header.session_name = session_name.to_string();
            } else {
                header.session_name = session_text(get_str(&parsed, "HEATTYPE").unwrap_or("-"));
            }

            if let Some(event_name) =
                website_event_name.or_else(|| first_non_empty(&parsed, &["CUP", "EVENTNAME"]))
            {
                header.event_name = event_name.to_string();
            }

            if let Some(track_name) = first_non_empty(&parsed, &["TRACKNAME", "TRACK"]) {
                header.track_name = track_name.to_string();
            }

            if let Some(heat_type) = get_str(&parsed, "HEATTYPE") {
                *is_race_session = heat_type.trim() == "R";
            }
            if let Some(countdown_state) = countdown.as_mut() {
                countdown_state.is_race_session = *is_race_session;
            }

            let results = parsed.get("RESULT")?.as_array()?;
            let mut entries: Vec<TimingEntry> =
                results.iter().filter_map(entry_from_value).collect();
            entries.sort_by_key(|e| e.position);
            Some((Some(entries), false))
        }
        "4" => {
            if let Some(heat_type_raw) = get_str(&parsed, "HEATTYPE") {
                *is_race_session = heat_type_raw.trim() == "R";
            }
            if header.session_name.is_empty() || header.session_name == "-" {
                header.session_name = session_text(get_str(&parsed, "HEATTYPE").unwrap_or("-"));
            }
            header.flag = track_state_text(get_str(&parsed, "TRACKSTATE").unwrap_or("-"));
            if let Some(track_name) = first_non_empty(&parsed, &["TRACKNAME", "TRACK"]) {
                header.track_name = track_name.to_string();
            } else if header.track_name.is_empty() {
                header.track_name = "NLS".to_string();
            }

            if let Some(event_name) =
                website_event_name.or_else(|| first_non_empty(&parsed, &["CUP", "EVENTNAME"]))
            {
                header.event_name = event_name.to_string();
            } else if header.event_name.is_empty() {
                header.event_name = "NLS Live Timing".to_string();
            }
            let end_time_raw = get_str(&parsed, "ENDTIME")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            let time_state_raw = get_str(&parsed, "TIMESTATE").unwrap_or("0");
            header.day_time = get_str(&parsed, "TIME").unwrap_or("-").to_string();

            *countdown = Some(CountdownState {
                end_time_raw,
                time_state_raw: time_state_raw.to_string(),
                received_at_ms: now_millis() as u64,
                is_race_session: *is_race_session,
            });

            refresh_header_time_to_go(header, countdown.as_ref());
            Some((None, true))
        }
        "LTS_TIMESYNC" => None,
        _ => None,
    }
}

fn set_socket_timeout(socket: &mut tungstenite::WebSocket<MaybeTlsStream<std::net::TcpStream>>) {
    const READ_TIMEOUT: Duration = Duration::from_secs(2);

    match socket.get_mut() {
        MaybeTlsStream::Plain(stream) => set_tcp_read_timeout(stream, READ_TIMEOUT),
        MaybeTlsStream::Rustls(stream) => {
            set_tcp_read_timeout(stream.get_mut(), READ_TIMEOUT);
        }
        _ => {}
    }
}

fn set_tcp_read_timeout(stream: &mut std::net::TcpStream, timeout: Duration) {
    let _ = stream.set_read_timeout(Some(timeout));
}

fn should_emit_connected_status_on_update(
    header_changed: bool,
    connected_status_already_sent: bool,
) -> bool {
    !header_changed && !connected_status_already_sent
}

fn refresh_active_event_id(
    active_event_id: &mut &'static str,
    refresh_result: Result<&'static str, String>,
) -> Option<String> {
    match refresh_result {
        Ok(event_id) => {
            if *active_event_id != event_id {
                *active_event_id = event_id;
                Some(format!("NLS switching to eventId {event_id}"))
            } else {
                None
            }
        }
        Err(err) => Some(format!(
            "NLS 24h schedule refresh failed ({err}); keeping eventId {}",
            *active_event_id
        )),
    }
}

pub fn websocket_worker(tx: Sender<TimingMessage>, source_id: u64, stop_rx: Receiver<()>) {
    let mut header = TimingHeader {
        event_name: "NLS Live Timing".to_string(),
        track_name: "Nürburgring".to_string(),
        ..TimingHeader::default()
    };
    let mut latest_entries: Vec<TimingEntry> = Vec::new();
    let website_client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .ok();
    let mut website_event_name: Option<String> = None;
    let mut next_website_refresh = Instant::now();
    let mut countdown: Option<CountdownState> = None;
    let mut is_race_session = false;
    let mut active_event_id = DEFAULT_NLS_EVENT_ID;

    'outer: loop {
        if stop_rx.try_recv().is_ok() {
            break;
        }

        if Instant::now() >= next_website_refresh {
            if let Some(client) = website_client.as_ref() {
                if let Some(parsed_name) = fetch_homepage_event_name(client) {
                    website_event_name = Some(parsed_name.clone());
                    header.event_name = parsed_name;
                }

                if let Some(status_text) = refresh_active_event_id(
                    &mut active_event_id,
                    determine_active_nuerburgring_event_id(client),
                ) {
                    let _ = tx.send(TimingMessage::Status {
                        source_id,
                        text: status_text,
                    });
                }
            }
            next_website_refresh = Instant::now() + WEBSITE_EVENT_REFRESH_INTERVAL;
        }

        let _ = tx.send(TimingMessage::Status {
            source_id,
            text: "Connecting to NLS websocket...".to_string(),
        });

        let request = build_request();
        let connection = connect(request);

        let (mut socket, response) = match connection {
            Ok(ok) => ok,
            Err(err) => {
                let _ = tx.send(TimingMessage::Error {
                    source_id,
                    text: format!("connect failed: {err}"),
                });
                if stop_rx.recv_timeout(Duration::from_secs(3)).is_ok() {
                    break;
                }
                continue;
            }
        };

        set_socket_timeout(&mut socket);

        let _ = tx.send(TimingMessage::Status {
            source_id,
            text: format!("NLS connected ({})", response.status()),
        });
        let mut connected_status_sent = true;

        let subscribe = json!({
            "clientLocalTime": now_millis(),
            "eventId": active_event_id,
            "eventPid": [0, 4]
        });

        if let Err(err) = socket.send(Message::Text(subscribe.to_string())) {
            let _ = tx.send(TimingMessage::Error {
                source_id,
                text: format!("subscribe failed: {err}"),
            });
            if stop_rx.recv_timeout(Duration::from_secs(3)).is_ok() {
                break;
            }
            continue;
        }

        loop {
            if stop_rx.try_recv().is_ok() {
                break 'outer;
            }

            match socket.read() {
                Ok(Message::Text(text)) => {
                    if let Some((entries, header_changed)) = parse_ws_message(
                        &text,
                        &mut header,
                        website_event_name.as_deref(),
                        &mut countdown,
                        &mut is_race_session,
                    ) {
                        if let Some(new_entries) = entries {
                            latest_entries = new_entries;
                        }
                        refresh_header_time_to_go(&mut header, countdown.as_ref());
                        let _ = tx.send(TimingMessage::Snapshot {
                            source_id,
                            header: header.clone(),
                            entries: latest_entries.clone(),
                        });
                        if should_emit_connected_status_on_update(
                            header_changed,
                            connected_status_sent,
                        ) {
                            let _ = tx.send(TimingMessage::Status {
                                source_id,
                                text: "NLS live timing connected".to_string(),
                            });
                            connected_status_sent = true;
                        }
                    }
                }
                Ok(Message::Binary(data)) => {
                    if let Ok(text) = std::str::from_utf8(&data) {
                        if let Some((entries, header_changed)) = parse_ws_message(
                            text,
                            &mut header,
                            website_event_name.as_deref(),
                            &mut countdown,
                            &mut is_race_session,
                        ) {
                            if let Some(new_entries) = entries {
                                latest_entries = new_entries;
                            }
                            refresh_header_time_to_go(&mut header, countdown.as_ref());
                            let _ = tx.send(TimingMessage::Snapshot {
                                source_id,
                                header: header.clone(),
                                entries: latest_entries.clone(),
                            });
                            if should_emit_connected_status_on_update(
                                header_changed,
                                connected_status_sent,
                            ) {
                                let _ = tx.send(TimingMessage::Status {
                                    source_id,
                                    text: "NLS live timing connected".to_string(),
                                });
                                connected_status_sent = true;
                            }
                        }
                    }
                }
                Ok(Message::Ping(data)) => {
                    if let Err(err) = socket.send(Message::Pong(data)) {
                        let _ = tx.send(TimingMessage::Error {
                            source_id,
                            text: format!("pong failed: {err}"),
                        });
                        break;
                    }
                }
                Ok(Message::Pong(_)) => {}
                Ok(Message::Close(frame)) => {
                    let _ = tx.send(TimingMessage::Error {
                        source_id,
                        text: format!("socket closed: {frame:?}"),
                    });
                    break;
                }
                Ok(Message::Frame(_)) => {}
                Err(WsError::Io(err))
                    if err.kind() == io::ErrorKind::WouldBlock
                        || err.kind() == io::ErrorKind::TimedOut =>
                {
                    continue;
                }
                Err(err) => {
                    let _ = tx.send(TimingMessage::Error {
                        source_id,
                        text: format!("read failed: {err}"),
                    });
                    break;
                }
            }
        }

        let _ = tx.send(TimingMessage::Status {
            source_id,
            text: "NLS reconnecting in 3s...".to_string(),
        });
        if stop_rx.recv_timeout(Duration::from_secs(3)).is_ok() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn current_time_to_end_at_counts_down_for_relative_mode() {
        let header = TimingHeader {
            time_to_go: "-".to_string(),
            ..TimingHeader::default()
        };

        let rendered = current_time_to_end_at(&header, 120_000, "0", 1_000_000, 1_030_500);
        assert_eq!(rendered, "00:01:29");
    }

    #[test]
    fn current_time_to_end_at_uses_absolute_timestamp_mode() {
        let header = TimingHeader {
            time_to_go: "-".to_string(),
            ..TimingHeader::default()
        };

        let rendered = current_time_to_end_at(&header, 2_000_000, "1", 0, 1_940_000);
        assert_eq!(rendered, "00:01:00");
    }

    #[test]
    fn refresh_sets_checkered_when_tte_reaches_zero_on_green() {
        let mut header = TimingHeader {
            flag: "Green".to_string(),
            session_name: "Race".to_string(),
            ..TimingHeader::default()
        };
        let countdown = CountdownState {
            end_time_raw: 0,
            time_state_raw: "0".to_string(),
            received_at_ms: 0,
            is_race_session: true,
        };

        header.time_to_go = "0:00".to_string();
        refresh_header_time_to_go(&mut header, Some(&countdown));

        assert_eq!(header.flag, "Checkered");
    }

    #[test]
    fn refresh_keeps_non_green_flags_when_tte_reaches_zero() {
        let mut header = TimingHeader {
            flag: "Yellow".to_string(),
            session_name: "Race".to_string(),
            time_to_go: "0:00".to_string(),
            ..TimingHeader::default()
        };
        let countdown = CountdownState {
            end_time_raw: 0,
            time_state_raw: "0".to_string(),
            received_at_ms: 0,
            is_race_session: true,
        };

        refresh_header_time_to_go(&mut header, Some(&countdown));

        assert_eq!(header.flag, "Yellow");
    }

    #[test]
    fn refresh_sets_checkered_when_tte_unknown_in_race_session() {
        let mut header = TimingHeader {
            flag: "Green".to_string(),
            session_name: "Rennen".to_string(),
            time_to_go: "-".to_string(),
            ..TimingHeader::default()
        };
        let countdown = CountdownState {
            end_time_raw: 0,
            time_state_raw: "0".to_string(),
            received_at_ms: 0,
            is_race_session: true,
        };

        refresh_header_time_to_go(&mut header, Some(&countdown));

        assert_eq!(header.flag, "Checkered");
    }

    #[test]
    fn refresh_sets_checkered_when_tte_empty_in_race_session() {
        let mut header = TimingHeader {
            flag: "Green".to_string(),
            session_name: "Rennen".to_string(),
            time_to_go: String::new(),
            ..TimingHeader::default()
        };
        let countdown = CountdownState {
            end_time_raw: 0,
            time_state_raw: "0".to_string(),
            received_at_ms: 0,
            is_race_session: true,
        };

        refresh_header_time_to_go(&mut header, Some(&countdown));

        assert_eq!(header.flag, "Checkered");
    }

    #[test]
    fn refresh_keeps_green_when_tte_zero_but_not_race_session() {
        let mut header = TimingHeader {
            flag: "Green".to_string(),
            session_name: "Qualifying".to_string(),
            time_to_go: "0:00".to_string(),
            ..TimingHeader::default()
        };
        let countdown = CountdownState {
            end_time_raw: 0,
            time_state_raw: "0".to_string(),
            received_at_ms: 0,
            is_race_session: false,
        };

        refresh_header_time_to_go(&mut header, Some(&countdown));

        assert_eq!(header.flag, "Green");
    }

    #[test]
    fn pid0_race_session_stays_true_when_follow_up_payload_omits_heattype() {
        let mut header = TimingHeader::default();
        let mut countdown: Option<CountdownState> = None;
        let mut is_race_session = false;

        let first = r#"{"PID":"0","HEATTYPE":"R","RESULT":[]}"#;
        let second = r#"{"PID":"0","RESULT":[]}"#;

        let _ = parse_ws_message(
            first,
            &mut header,
            None,
            &mut countdown,
            &mut is_race_session,
        );
        assert!(is_race_session);

        let _ = parse_ws_message(
            second,
            &mut header,
            None,
            &mut countdown,
            &mut is_race_session,
        );
        assert!(is_race_session);
    }

    #[test]
    fn entry_from_value_reads_all_five_sectors() {
        let row = json!({
            "POSITION": "1",
            "STNR": "77",
            "CLASSNAME": "SP9",
            "CLASSRANK": "1",
            "NAME": "Driver",
            "CAR": "Car",
            "TEAM": "Team",
            "LAPS": "12",
            "GAP": "Leader",
            "LASTLAPTIME": "8:01.234",
            "FASTESTLAP": "7:59.111",
            "S1": "1:31.001",
            "S2": "2:00.002",
            "S3": "1:11.003",
            "S4": "1:45.004",
            "S5": "1:34.005"
        });

        let entry = entry_from_value(&row).expect("entry");
        assert_eq!(entry.sector_1, "1:31.001");
        assert_eq!(entry.sector_2, "2:00.002");
        assert_eq!(entry.sector_3, "1:11.003");
        assert_eq!(entry.sector_4, "1:45.004");
        assert_eq!(entry.sector_5, "1:34.005");
    }

    #[test]
    fn entry_from_value_ignores_non_standard_sector_keys() {
        let row = json!({
            "POSITION": "4",
            "STNR": "911",
            "CLASSNAME": "SP9",
            "CLASSRANK": "3",
            "NAME": "Driver",
            "CAR": "Car",
            "TEAM": "Team",
            "LAPS": "7",
            "GAP": "+12.300",
            "LASTLAPTIME": "8:12.340",
            "FASTESTLAP": "8:05.900",
            "SECTOR_1": "1:32.100",
            "SEC2": "2:01.200",
            "SEKTOR3": "1:10.300",
            "SECTOR4": "1:46.400"
        });

        let entry = entry_from_value(&row).expect("entry");
        assert_eq!(entry.sector_1, "-");
        assert_eq!(entry.sector_2, "-");
        assert_eq!(entry.sector_3, "-");
        assert_eq!(entry.sector_4, "-");
        assert_eq!(entry.sector_5, "-");
    }

    #[test]
    fn entry_from_value_prefers_sxtime_sector_keys() {
        let row = json!({
            "POSITION": "8",
            "STNR": "44",
            "CLASSNAME": "SP9",
            "CLASSRANK": "5",
            "NAME": "Driver",
            "CAR": "Car",
            "TEAM": "Team",
            "LAPS": "20",
            "GAP": "+23.000",
            "LASTLAPTIME": "8:11.111",
            "FASTESTLAP": "8:02.222",
            "S1TIME": "1:32.555",
            "S2TIME": "2:01.666",
            "S3TIME": "1:10.777",
            "S4TIME": "1:45.888",
            "S5TIME": "1:33.999"
        });

        let entry = entry_from_value(&row).expect("entry");
        assert_eq!(entry.sector_1, "1:32.555");
        assert_eq!(entry.sector_2, "2:01.666");
        assert_eq!(entry.sector_3, "1:10.777");
        assert_eq!(entry.sector_4, "1:45.888");
        assert_eq!(entry.sector_5, "1:33.999");
        assert_eq!(entry.pit, "-");
    }

    #[test]
    fn entry_from_value_maps_s5_inout_to_pit_flag() {
        let row = json!({
            "POSITION": "9",
            "STNR": "632",
            "CLASSNAME": "AT",
            "CLASSRANK": "1",
            "NAME": "Driver",
            "CAR": "Car",
            "TEAM": "Team",
            "LAPS": "22",
            "GAP": "+44.000",
            "LASTLAPTIME": "8:20.000",
            "FASTESTLAP": "8:10.000",
            "S5TIME": "OUT"
        });

        let entry = entry_from_value(&row).expect("entry");
        assert_eq!(entry.sector_5, "OUT");
        assert_eq!(entry.pit, "No");
    }

    #[test]
    fn parse_german_date_range_handles_24h_format() {
        let parsed = parse_german_date_range("14. – 17.05.2026").expect("range");
        assert_eq!(
            parsed.0,
            CalendarDate {
                year: 2026,
                month: 5,
                day: 14
            }
        );
        assert_eq!(
            parsed.1,
            CalendarDate {
                year: 2026,
                month: 5,
                day: 17
            }
        );
    }

    #[test]
    fn extract_target_date_range_picks_current_year() {
        let html = r#"
            <div>ADAC RAVENOL 24h Nürburgring</div>
            <div>14. &#8211; 17.05.2026</div>
            <div>27. &#8211; 30.05.2027</div>
        "#;
        let lines = html_to_text_lines(html);

        let parsed = extract_target_date_range(&lines, 2027).expect("year range");
        assert_eq!(parsed.0.day, 27);
        assert_eq!(parsed.1.day, 30);
        assert_eq!(parsed.0.month, 5);
    }

    #[test]
    fn chooses_24h_event_when_today_in_range() {
        let start = CalendarDate {
            year: 2026,
            month: 5,
            day: 14,
        };
        let end = CalendarDate {
            year: 2026,
            month: 5,
            day: 17,
        };
        let today = CalendarDate {
            year: 2026,
            month: 5,
            day: 15,
        };

        let event_id = if today >= start && today <= end {
            N24_EVENT_ID
        } else {
            DEFAULT_NLS_EVENT_ID
        };
        assert_eq!(event_id, N24_EVENT_ID);
    }

    #[test]
    fn timeout_helper_sets_tcp_read_timeout() {
        use std::net::{TcpListener, TcpStream};

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let addr = listener.local_addr().expect("local addr");

        let mut client = TcpStream::connect(addr).expect("connect client");
        let _server = listener.accept().expect("accept client");

        set_tcp_read_timeout(&mut client, Duration::from_millis(1234));
        assert_eq!(
            client.read_timeout().expect("read timeout"),
            Some(Duration::from_millis(1234))
        );
    }

    #[test]
    fn pid4_header_updates_do_not_emit_connected_status_updates() {
        assert!(!should_emit_connected_status_on_update(true, true));
        assert!(!should_emit_connected_status_on_update(true, false));
    }

    #[test]
    fn refresh_failure_keeps_previous_event_id() {
        let mut active_event_id = N24_EVENT_ID;
        let status = refresh_active_event_id(
            &mut active_event_id,
            Err("temporary schedule parse error".to_string()),
        )
        .expect("status message");

        assert_eq!(active_event_id, N24_EVENT_ID);
        assert!(status.contains("keeping eventId"));
        assert!(status.contains(N24_EVENT_ID));
    }
}
