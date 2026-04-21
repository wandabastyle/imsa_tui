use std::{
    collections::HashSet,
    time::{SystemTime, UNIX_EPOCH},
};

use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::Value;

use super::common::is_closed_status;

pub(crate) const SCHEDULE_URL: &str = "https://insights.griiip.com/meta/sessions-schedule-live";
pub(crate) const META_SESSIONS_URL: &str = "https://insights.griiip.com/meta/sessions";
pub(crate) const LIVE_BASE_URL: &str = "https://insights.griiip.com/live";
pub(crate) const REALWORLD_DOMAIN: &str = "RealWorld";
const WEC_SERIES_ID: u64 = 10;
const F1_SERIES_ID: u64 = 370;
const SCHEDULE_WINDOW_TOLERANCE_DAYS: i32 = 2;
const F1_CALENDAR_URL: &str =
    "https://api.formula1.com/v1/editorial-content/basicEntity/atomRaceCalendar/race-calendar";
const F1_PUBLIC_API_KEY: &str = "BQ1SiSmLUOsp460VzXBlLrh689kGgYEZ";
const WEC_CALENDAR_URL: &str = "https://www.fiawec.com/en/calendar/80";

#[derive(Debug, Clone)]
struct OfficialEventWindow {
    label: String,
    start_yyyymmdd: i32,
    end_yyyymmdd: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct MetaSessionItem {
    pub(crate) id: u64,
    pub(crate) name: Option<String>,
    #[serde(rename = "sessionType")]
    pub(crate) session_type: Option<String>,
    #[serde(rename = "isRunning", default)]
    pub(crate) is_running: bool,
    #[serde(rename = "hasResult", default)]
    pub(crate) has_result: bool,
    #[serde(rename = "startTime")]
    pub(crate) start_time: Option<String>,
    #[serde(rename = "endTime")]
    pub(crate) end_time: Option<String>,
    #[serde(default)]
    pub(crate) event: Option<MetaEventInfo>,
    #[serde(rename = "trackConfig")]
    pub(crate) track_config: Option<MetaTrackConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct MetaEventInfo {
    pub(crate) name: Option<String>,
    #[serde(rename = "trackConfig")]
    pub(crate) track_config: Option<MetaTrackConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct MetaTrackConfig {
    pub(crate) name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ScheduleItem {
    sid: u64,
    #[serde(rename = "isStarted", default)]
    is_started: bool,
    #[serde(rename = "connectionStatus")]
    connection_status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct F1CalendarResponse {
    #[serde(rename = "meetingDates", default)]
    meeting_dates: Vec<F1MeetingDate>,
}

#[derive(Debug, Deserialize)]
struct F1MeetingDate {
    city: Option<String>,
    #[serde(rename = "startDate")]
    start_date: Option<i32>,
    #[serde(rename = "endDate")]
    end_date: Option<i32>,
}

pub(crate) fn resolve_live_sid_for_series(client: &Client, series_id: u64) -> Result<u64, String> {
    let maybe_official_windows = fetch_official_event_windows(client, series_id)?;
    let today = current_utc_yyyymmdd();

    let response = client
        .get(SCHEDULE_URL)
        .send()
        .map_err(|err| format!("schedule request failed: {err}"))?;
    if !response.status().is_success() {
        return Err(format!("schedule failed with HTTP {}", response.status()));
    }
    let body = response
        .text()
        .map_err(|err| format!("schedule body read failed: {err}"))?;
    let sessions = serde_json::from_str::<Vec<ScheduleItem>>(&body)
        .map_err(|err| format!("schedule decode failed: {err}"))?;

    let mut candidate_sids = Vec::with_capacity(sessions.len());
    for session in sessions.iter().filter(|session| {
        session.is_started && !is_closed_status(session.connection_status.as_deref())
    }) {
        candidate_sids.push(session.sid);
    }
    for session in &sessions {
        if !candidate_sids.contains(&session.sid) {
            candidate_sids.push(session.sid);
        }
    }

    for sid in candidate_sids {
        let info = fetch_live_json(client, sid, "session-info")?;
        let Some(map) = info.as_object() else {
            continue;
        };
        let sid_series = map.get("seriesId").and_then(Value::as_u64);
        let domain = map.get("domain").and_then(Value::as_str);
        if sid_series == Some(series_id)
            && domain
                .map(|value| value.eq_ignore_ascii_case(REALWORLD_DOMAIN))
                .unwrap_or(true)
        {
            if let Some(windows) = maybe_official_windows.as_ref() {
                if !official_schedule_allows_live_session(series_id, map, windows, today) {
                    continue;
                }
            }
            return Ok(sid);
        }
    }

    Err(format!(
        "no active live session found for seriesId={series_id}"
    ))
}

fn fetch_official_event_windows(
    client: &Client,
    series_id: u64,
) -> Result<Option<Vec<OfficialEventWindow>>, String> {
    match series_id {
        WEC_SERIES_ID => fetch_wec_official_event_windows(client).map(Some),
        F1_SERIES_ID => fetch_f1_official_event_windows(client).map(Some),
        _ => Ok(None),
    }
}

fn fetch_f1_official_event_windows(client: &Client) -> Result<Vec<OfficialEventWindow>, String> {
    let response = client
        .get(F1_CALENDAR_URL)
        .header("apikey", F1_PUBLIC_API_KEY)
        .send()
        .map_err(|err| format!("F1 official schedule request failed: {err}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "F1 official schedule failed with HTTP {}",
            response.status()
        ));
    }
    let body = response
        .text()
        .map_err(|err| format!("F1 official schedule body read failed: {err}"))?;
    let payload = serde_json::from_str::<F1CalendarResponse>(&body)
        .map_err(|err| format!("F1 official schedule decode failed: {err}"))?;
    let mut entries = Vec::with_capacity(payload.meeting_dates.len());
    for meeting in payload.meeting_dates {
        let Some(start_yyyymmdd) = meeting.start_date else {
            continue;
        };
        let end_yyyymmdd = meeting.end_date.unwrap_or(start_yyyymmdd);
        let label = meeting.city.unwrap_or_else(|| "f1-event".to_string());
        entries.push(OfficialEventWindow {
            label,
            start_yyyymmdd,
            end_yyyymmdd,
        });
    }
    if entries.is_empty() {
        return Err("F1 official schedule returned no events".to_string());
    }
    Ok(entries)
}

fn fetch_wec_official_event_windows(client: &Client) -> Result<Vec<OfficialEventWindow>, String> {
    let response = client
        .get(WEC_CALENDAR_URL)
        .send()
        .map_err(|err| format!("WEC official schedule request failed: {err}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "WEC official schedule failed with HTTP {}",
            response.status()
        ));
    }
    let body = response
        .text()
        .map_err(|err| format!("WEC official schedule body read failed: {err}"))?;
    parse_wec_official_event_windows(&body)
}

fn parse_wec_official_event_windows(html: &str) -> Result<Vec<OfficialEventWindow>, String> {
    let mut entries = Vec::new();
    let mut dedupe = HashSet::new();
    let mut cursor = 0usize;

    while let Some(found) = html[cursor..].find("href=\"/en/race/") {
        let href_start = cursor + found + "href=\"".len();
        let Some(href_end_rel) = html[href_start..].find('"') else {
            break;
        };
        let href_end = href_start + href_end_rel;
        let href = &html[href_start..href_end];

        let year = extract_year_from_slug(href);
        let lookahead_end = html.len().min(href_end + 450);
        let fragment = &html[href_end..lookahead_end];
        let day = extract_tag_number(fragment, "strong");
        let month = extract_tag_month(fragment, "small");

        if let (Some(year), Some(day), Some(month)) = (year, day, month) {
            let yyyymmdd = year * 10000 + month * 100 + day;
            let label = label_from_wec_href(href);
            let key = format!("{label}:{yyyymmdd}");
            if dedupe.insert(key) {
                entries.push(OfficialEventWindow {
                    label,
                    start_yyyymmdd: yyyymmdd,
                    end_yyyymmdd: yyyymmdd,
                });
            }
        }

        cursor = href_end;
    }

    if entries.is_empty() {
        return Err("WEC official schedule parse found no events".to_string());
    }

    Ok(entries)
}

fn extract_year_from_slug(href: &str) -> Option<i32> {
    let mut last_match = None;
    let chars: Vec<char> = href.chars().collect();
    if chars.len() < 4 {
        return None;
    }

    for idx in 0..=(chars.len() - 4) {
        let slice = [chars[idx], chars[idx + 1], chars[idx + 2], chars[idx + 3]];
        if slice.iter().all(|ch| ch.is_ascii_digit()) {
            let candidate = slice.iter().collect::<String>().parse::<i32>().ok();
            if candidate.is_some_and(|year| (2000..=2100).contains(&year)) {
                last_match = candidate;
            }
        }
    }

    last_match
}

fn extract_tag_number(fragment: &str, tag: &str) -> Option<i32> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let start = fragment.find(&open)?;
    let tag_after_open = &fragment[start..];
    let content_start_rel = tag_after_open.find('>')? + 1;
    let content_end_rel = tag_after_open[content_start_rel..].find(&close)?;
    let raw = &tag_after_open[content_start_rel..content_start_rel + content_end_rel];
    raw.trim().parse::<i32>().ok()
}

fn extract_tag_month(fragment: &str, tag: &str) -> Option<i32> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let start = fragment.find(&open)?;
    let tag_after_open = &fragment[start..];
    let content_start_rel = tag_after_open.find('>')? + 1;
    let content_end_rel = tag_after_open[content_start_rel..].find(&close)?;
    let raw = &tag_after_open[content_start_rel..content_start_rel + content_end_rel];
    parse_month(raw)
}

fn parse_month(value: &str) -> Option<i32> {
    let token = value.trim().to_ascii_lowercase();
    let short = token.chars().take(3).collect::<String>();
    match short.as_str() {
        "jan" => Some(1),
        "feb" => Some(2),
        "mar" => Some(3),
        "apr" => Some(4),
        "may" => Some(5),
        "jun" => Some(6),
        "jul" => Some(7),
        "aug" => Some(8),
        "sep" => Some(9),
        "oct" => Some(10),
        "nov" => Some(11),
        "dec" => Some(12),
        _ => None,
    }
}

fn label_from_wec_href(href: &str) -> String {
    let slug = href
        .strip_prefix("/en/race/")
        .or_else(|| href.strip_prefix("/fr/race/"))
        .unwrap_or(href);
    slug.replace('-', " ")
}

fn official_schedule_allows_live_session(
    series_id: u64,
    session_info: &serde_json::Map<String, Value>,
    windows: &[OfficialEventWindow],
    today_yyyymmdd: i32,
) -> bool {
    let active_windows: Vec<&OfficialEventWindow> = windows
        .iter()
        .filter(|entry| {
            is_within_window(
                today_yyyymmdd,
                entry.start_yyyymmdd,
                entry.end_yyyymmdd,
                SCHEDULE_WINDOW_TOLERANCE_DAYS,
            )
        })
        .collect();

    if active_windows.is_empty() {
        return false;
    }

    if series_id != WEC_SERIES_ID {
        return true;
    }

    let session_label = session_info_label(session_info);
    if session_label.trim().is_empty() {
        return false;
    }

    active_windows
        .iter()
        .any(|entry| labels_overlap(&session_label, &entry.label))
}

fn session_info_label(session_info: &serde_json::Map<String, Value>) -> String {
    ["eventName", "sessionName", "name"]
        .iter()
        .filter_map(|key| session_info.get(*key))
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join(" ")
}

fn labels_overlap(left: &str, right: &str) -> bool {
    let left_tokens = normalized_tokens(left);
    let right_tokens = normalized_tokens(right);
    right_tokens
        .iter()
        .filter(|token| token.len() >= 4)
        .any(|token| left_tokens.contains(token))
}

fn normalized_tokens(value: &str) -> HashSet<String> {
    let mut normalized = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
        } else {
            normalized.push(' ');
        }
    }
    normalized
        .split_whitespace()
        .map(ToString::to_string)
        .collect()
}

fn is_within_window(today: i32, start: i32, end: i32, tolerance_days: i32) -> bool {
    let today_days = yyyymmdd_to_epoch_days(today);
    let start_days = yyyymmdd_to_epoch_days(start);
    let end_days = yyyymmdd_to_epoch_days(end.max(start));
    today_days >= start_days - tolerance_days && today_days <= end_days + tolerance_days
}

fn current_utc_yyyymmdd() -> i32 {
    let now_secs = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs() as i64,
        Err(_) => 0,
    };
    epoch_seconds_to_yyyymmdd(now_secs)
}

fn current_utc_iso8601() -> String {
    let now_secs = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_secs() as i64,
        Err(_) => 0,
    };
    let days_since_epoch = now_secs.div_euclid(86_400);
    let secs_of_day = now_secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days_since_epoch);
    let hour = secs_of_day / 3_600;
    let minute = (secs_of_day % 3_600) / 60;
    let second = secs_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

fn epoch_seconds_to_yyyymmdd(epoch_seconds: i64) -> i32 {
    let days_since_epoch = epoch_seconds.div_euclid(86_400);
    let (year, month, day) = civil_from_days(days_since_epoch);
    year * 10000 + month * 100 + day
}

fn yyyymmdd_to_epoch_days(value: i32) -> i32 {
    let year = value / 10_000;
    let month = (value / 100) % 100;
    let day = value % 100;
    days_from_civil(year, month, day) as i32
}

fn civil_from_days(days_since_epoch: i64) -> (i32, i32, i32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (year as i32, m as i32, d as i32)
}

fn days_from_civil(year: i32, month: i32, day: i32) -> i64 {
    let y = year - if month <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe - 719_468) as i64
}

pub(crate) fn fetch_meta_sessions_for_series(
    client: &Client,
    series_id: u64,
) -> Result<Vec<MetaSessionItem>, String> {
    let reference_time = resolve_meta_sessions_reference_time(client);
    let series_id_text = series_id.to_string();
    let response = client
        .get(META_SESSIONS_URL)
        .query(&[
            ("dateTime", reference_time.as_str()),
            ("forward", "false"),
            ("seriesIds", series_id_text.as_str()),
            ("domains", REALWORLD_DOMAIN),
        ])
        .send()
        .map_err(|err| format!("meta sessions request failed: {err}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "meta sessions request failed with HTTP {}",
            response.status()
        ));
    }
    let body = response
        .text()
        .map_err(|err| format!("meta sessions body read failed: {err}"))?;
    serde_json::from_str::<Vec<MetaSessionItem>>(&body)
        .map_err(|err| format!("meta sessions decode failed: {err}"))
}

pub(crate) fn choose_latest_finished_race_session(
    sessions: &[MetaSessionItem],
) -> Option<MetaSessionItem> {
    let mut candidates: Vec<_> = sessions
        .iter()
        .filter(|session| {
            session
                .session_type
                .as_deref()
                .map(|value| value.eq_ignore_ascii_case("Race"))
                .unwrap_or(false)
                && session.has_result
        })
        .cloned()
        .collect();
    candidates.sort_by_key(|session| {
        session
            .end_time
            .clone()
            .or_else(|| session.start_time.clone())
            .unwrap_or_default()
    });
    candidates.reverse();

    candidates
        .iter()
        .find(|session| !session.is_running)
        .cloned()
        .or_else(|| candidates.first().cloned())
}

pub(crate) fn resolve_meta_sessions_reference_time(client: &Client) -> String {
    let Ok(response) = client.get(SCHEDULE_URL).send() else {
        return current_utc_iso8601();
    };
    if !response.status().is_success() {
        return current_utc_iso8601();
    }
    let Ok(body) = response.text() else {
        return current_utc_iso8601();
    };
    let Ok(payload) = serde_json::from_str::<Vec<Value>>(&body) else {
        return current_utc_iso8601();
    };
    for item in payload {
        let Some(clock) = item.get("clock") else {
            continue;
        };
        if let Some(ts_now) = clock.get("tsNow").and_then(Value::as_str) {
            return ts_now.to_string();
        }
        if let Some(ts) = clock.get("ts").and_then(Value::as_str) {
            return ts.to_string();
        }
    }
    current_utc_iso8601()
}

pub(crate) fn fetch_live_json(client: &Client, sid: u64, route: &str) -> Result<Value, String> {
    let url = format!("{LIVE_BASE_URL}/{route}/{sid}");
    let response = client
        .get(&url)
        .send()
        .map_err(|err| format!("live request failed ({route}): {err}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "live endpoint {route} failed with HTTP {}",
            response.status()
        ));
    }
    let body = response
        .text()
        .map_err(|err| format!("live body read failed ({route}): {err}"))?;
    serde_json::from_str::<Value>(&body)
        .map_err(|err| format!("live decode failed ({route}): {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_wec_schedule_entries_from_official_calendar_html() {
        let html = r#"
            <a href="/en/race/official-prologue-imola-2026">
              <strong class="fs-8 lh-sm">14</strong>
              <small class="fs-11">Apr</small>
            </a>
            <a href="/en/race/6-hours-of-imola-2026">
              <strong class="fs-8 lh-sm">19</strong>
              <small class="fs-11">Apr</small>
            </a>
        "#;

        let parsed = parse_wec_official_event_windows(html).expect("parse WEC calendar");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].start_yyyymmdd, 20260414);
        assert_eq!(parsed[1].start_yyyymmdd, 20260419);
        assert!(parsed[1].label.contains("imola"));
    }

    #[test]
    fn wec_schedule_validation_accepts_scheduled_testing_session() {
        let session_info = json!({
            "eventName": "Official Prologue Imola",
            "sessionName": "TEST session"
        });
        let windows = vec![OfficialEventWindow {
            label: "official prologue imola".to_string(),
            start_yyyymmdd: 20260414,
            end_yyyymmdd: 20260414,
        }];

        let allowed = official_schedule_allows_live_session(
            WEC_SERIES_ID,
            session_info
                .as_object()
                .expect("session-info object for WEC test"),
            &windows,
            20260414,
        );

        assert!(allowed);
    }

    #[test]
    fn wec_schedule_validation_rejects_out_of_window_session() {
        let session_info = json!({
            "eventName": "Bapco Energies 8 Hours of Bahrain",
            "sessionName": "Race"
        });
        let windows = vec![OfficialEventWindow {
            label: "bapco energies 8 hours of bahrain".to_string(),
            start_yyyymmdd: 20251108,
            end_yyyymmdd: 20251108,
        }];

        let allowed = official_schedule_allows_live_session(
            WEC_SERIES_ID,
            session_info
                .as_object()
                .expect("session-info object for out-of-window test"),
            &windows,
            20260421,
        );

        assert!(!allowed);
    }

    #[test]
    fn f1_schedule_validation_accepts_active_window_without_name_match() {
        let session_info = json!({
            "eventName": "FORMULA 1 QATAR AIRWAYS AUSTRALIAN GRAND PRIX 2026"
        });
        let windows = vec![OfficialEventWindow {
            label: "melbourne".to_string(),
            start_yyyymmdd: 20260306,
            end_yyyymmdd: 20260308,
        }];

        let allowed = official_schedule_allows_live_session(
            F1_SERIES_ID,
            session_info
                .as_object()
                .expect("session-info object for F1 test"),
            &windows,
            20260307,
        );

        assert!(allowed);
    }

    #[test]
    fn window_check_respects_tolerance() {
        assert!(is_within_window(20260421, 20260419, 20260419, 2));
        assert!(!is_within_window(20260425, 20260419, 20260419, 2));
    }

    #[test]
    fn current_utc_iso8601_uses_expected_shape() {
        let value = current_utc_iso8601();
        assert_eq!(value.len(), 20);
        assert_eq!(&value[4..5], "-");
        assert_eq!(&value[7..8], "-");
        assert_eq!(&value[10..11], "T");
        assert_eq!(&value[13..14], ":");
        assert_eq!(&value[16..17], ":");
        assert_eq!(&value[19..20], "Z");
    }
}
