use std::io;

#[cfg(test)]
use std::time::Duration;

use serde_json::Value;
use tungstenite::Error as WsError;

use crate::timing::{TimingEntry, TimingHeader, TimingNotice};

use super::countdown::{now_millis, refresh_header_time_to_go, CountdownState};

fn get_str<'a>(obj: &'a Value, key: &str) -> Option<&'a str> {
    obj.get(key).and_then(|x| x.as_str())
}

fn first_non_empty<'a>(obj: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .filter_map(|key| get_str(obj, key))
        .map(str::trim)
        .find(|value| !value.is_empty())
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

fn raw_sector_field(v: &Value, sector_no: usize) -> String {
    let candidates: &[&str] = match sector_no {
        1 => &["S1TIME", "S1"],
        2 => &["S2TIME", "S2"],
        3 => &["S3TIME", "S3"],
        4 => &["S4TIME", "S4"],
        5 => &["S5TIME", "S5"],
        6 => &["S6TIME", "S6"],
        7 => &["S7TIME", "S7"],
        8 => &["S8TIME", "S8"],
        9 => &["S9TIME", "S9"],
        _ => &[],
    };

    if let Some(value) = candidates.iter().find_map(|key| non_empty_field(v, key)) {
        return value;
    }

    "-".to_string()
}

fn sum_sector_times(time1: &str, time2: &str) -> String {
    if time1 == "-" || time2 == "-" {
        return "-".to_string();
    }

    fn parse_time_to_centisecs(s: &str) -> Option<u64> {
        let parts: Vec<&str> = s.split(':').collect();
        match parts.len() {
            1 => {
                let secs = parts[0].parse::<f64>().ok()?;
                Some((secs * 100.0) as u64)
            }
            2 => {
                let mins: u64 = parts[0].parse().ok()?;
                let secs: f64 = parts[1].parse().ok()?;
                Some(mins * 6000 + (secs * 100.0) as u64)
            }
            3 => {
                let hours: u64 = parts[0].parse().ok()?;
                let mins: u64 = parts[1].parse().ok()?;
                let secs: f64 = parts[2].parse().ok()?;
                Some(hours * 360000 + mins * 6000 + (secs * 100.0) as u64)
            }
            _ => None,
        }
    }

    fn format_centisecs(cs: u64) -> String {
        let hours = cs / 360000;
        let mins = (cs % 360000) / 6000;
        let secs = (cs % 6000) as f64 / 100.0;
        if hours > 0 {
            format!("{}:{:02}:{:05.2}", hours, mins, secs)
        } else if mins > 0 {
            format!("{}:{:05.2}", mins, secs)
        } else {
            format!("{:05.2}", secs)
        }
    }

    let t1 = match parse_time_to_centisecs(time1) {
        Some(v) => v,
        None => return time1.to_string(),
    };
    let t2 = match parse_time_to_centisecs(time2) {
        Some(v) => v,
        None => return time1.to_string(),
    };
    let sum = t1.saturating_add(t2);
    format_centisecs(sum)
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

pub fn entry_from_value(v: &Value, event_id: &str) -> Option<TimingEntry> {
    let car_number = parse_u32_field(v, "STNR")?.to_string();
    let class_name = get_str(v, "CLASSNAME").unwrap_or("-").to_string();
    let stable_id = format!("stnr:{car_number}");

    let is_24h = event_id == "50";

    let (sector_1, sector_2, sector_3, sector_4, sector_5) = if is_24h {
        (
            sum_sector_times(&raw_sector_field(v, 1), &raw_sector_field(v, 2)),
            sum_sector_times(&raw_sector_field(v, 3), &raw_sector_field(v, 4)),
            sum_sector_times(&raw_sector_field(v, 5), &raw_sector_field(v, 6)),
            raw_sector_field(v, 7),
            sum_sector_times(&raw_sector_field(v, 8), &raw_sector_field(v, 9)),
        )
    } else {
        (
            raw_sector_field(v, 1),
            raw_sector_field(v, 2),
            raw_sector_field(v, 3),
            raw_sector_field(v, 4),
            raw_sector_field(v, 5),
        )
    };

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

pub(crate) fn notices_from_ws_message(text: &str) -> Vec<TimingNotice> {
    let parsed: Value = match serde_json::from_str(text) {
        Ok(value) => value,
        Err(_) => return Vec::new(),
    };

    if get_str(&parsed, "PID") != Some("3") {
        return Vec::new();
    }

    parsed
        .get("MESSAGES")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let text = first_non_empty(row, &["MESSAGE", "MSG", "TEXT"])?;
            let id = first_non_empty(row, &["ID"]).unwrap_or("");
            let time = first_non_empty(row, &["MESSAGETIME", "TIME"]).unwrap_or("");

            Some(TimingNotice {
                id: id.to_string(),
                time: time.to_string(),
                text: text.to_string(),
            })
        })
        .collect()
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

pub(super) fn parse_ws_message(
    text: &str,
    header: &mut TimingHeader,
    termine_event_name: Option<&str>,
    homepage_event_name: Option<&str>,
    countdown: &mut Option<CountdownState>,
    is_race_session: &mut bool,
    event_id: &str,
) -> Option<(Option<Vec<TimingEntry>>, bool)> {
    let parsed: Value = serde_json::from_str(text).ok()?;
    let pid = get_str(&parsed, "PID")?;

    match pid {
        "0" => {
            if let Some(heat_type) = get_str(&parsed, "HEATTYPE") {
                header.session_type_raw = heat_type.trim().to_string();
            }
            if let Some(session_name) = first_non_empty(&parsed, &["HEAT"]) {
                header.session_name = session_name.to_string();
            } else {
                header.session_name = session_text(get_str(&parsed, "HEATTYPE").unwrap_or("-"));
            }

            let ws_cup = first_non_empty(&parsed, &["CUP", "EVENTNAME"]);
            let cup_is_dhlm = ws_cup
                .map(|name| name.to_ascii_lowercase().contains("dhlm"))
                .unwrap_or(false);

            if cup_is_dhlm {
                header.event_name = ws_cup.unwrap().to_string();
            } else if let Some(termine_name) = termine_event_name {
                header.event_name = termine_name.to_string();
            } else if let Some(homepage_name) = homepage_event_name {
                header.event_name = homepage_name.to_string();
            } else if let Some(cup) = ws_cup {
                header.event_name = cup.to_string();
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
            let mut entries: Vec<TimingEntry> = results
                .iter()
                .filter_map(|v| entry_from_value(v, event_id))
                .collect();
            entries.sort_by_key(|e| e.position);
            Some((Some(entries), false))
        }
        "4" => {
            if let Some(heat_type_raw) = get_str(&parsed, "HEATTYPE") {
                header.session_type_raw = heat_type_raw.trim().to_string();
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

            let ws_cup = first_non_empty(&parsed, &["CUP", "EVENTNAME"]);
            let cup_is_dhlm = ws_cup
                .map(|name| name.to_ascii_lowercase().contains("dhlm"))
                .unwrap_or(false);

            if cup_is_dhlm {
                header.event_name = ws_cup.unwrap().to_string();
            } else if let Some(termine_name) = termine_event_name {
                header.event_name = termine_name.to_string();
            } else if let Some(homepage_name) = homepage_event_name {
                header.event_name = homepage_name.to_string();
            } else if let Some(cup) = ws_cup {
                header.event_name = cup.to_string();
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

#[cfg(test)]
pub(super) fn set_tcp_read_timeout(stream: &mut std::net::TcpStream, timeout: Duration) {
    let _ = stream.set_read_timeout(Some(timeout));
}

pub(super) fn should_emit_connected_status_on_update(
    header_changed: bool,
    connected_status_already_sent: bool,
) -> bool {
    !header_changed && !connected_status_already_sent
}

pub(super) fn refresh_active_event_id(
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

pub(super) fn is_retriable_timeout(err: &WsError) -> bool {
    matches!(
        err,
        WsError::Io(io_err)
            if io_err.kind() == io::ErrorKind::WouldBlock || io_err.kind() == io::ErrorKind::TimedOut
    )
}
