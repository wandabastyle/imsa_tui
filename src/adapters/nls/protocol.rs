use std::{
   collections::BTreeSet,
   io,
   time::{
      Duration,
      Instant,
   },
};

use serde_json::Value;
use tungstenite::Error as WsError;
use whichlang::Lang;

use super::{
   countdown::{
      now_unix_ms,
      refresh_header_time_to_go,
      CountdownState,
   },
   schedule::{
      N24_EVENT_ID,
      N24_TARGET_EVENT_TITLE,
   },
};
use crate::timing::{
   TimingEntry,
   TimingHeader,
   TimingNotice,
};

fn get_str<'a>(obj: &'a Value, key: &str) -> Option<&'a str> {
   obj.get(key).and_then(serde_json::Value::as_str)
}

fn first_non_empty<'a>(obj: &'a Value, keys: &[&str]) -> Option<&'a str> {
   keys
      .iter()
      .filter_map(|key| get_str(obj, key))
      .map(str::trim)
      .find(|value| !value.is_empty())
}

fn parse_u32_field(obj: &Value, key: &str) -> Option<u32> {
   get_str(obj, key)
      .and_then(|s| s.trim().parse::<u32>().ok())
      .or_else(|| {
         obj.get(key)
            .and_then(serde_json::Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
      })
}

fn non_empty_field(obj: &Value, key: &str) -> Option<String> {
   if let Some(raw) = get_str(obj, key) {
      let value = raw.trim();
      if !value.is_empty() {
         return Some(value.to_string());
      }
   }

   if let Some(n) = obj.get(key).and_then(serde_json::Value::as_u64) {
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

fn parse_seconds_centis(secs_text: &str) -> Option<u64> {
   let trimmed = secs_text.trim();
   if trimmed.is_empty() {
      return None;
   }
   let (whole_text, frac_text) = trimmed.split_once('.').unwrap_or((trimmed, ""));
   let whole = whole_text.parse::<u64>().ok()?;
   let mut frac_digits = frac_text.chars().filter(char::is_ascii_digit);
   let d1 = frac_digits.next().and_then(|d| d.to_digit(10)).unwrap_or(0);
   let d2 = frac_digits.next().and_then(|d| d.to_digit(10)).unwrap_or(0);
   let d3 = frac_digits.next().and_then(|d| d.to_digit(10)).unwrap_or(0);
   let mut centis = whole
      .saturating_mul(100)
      .saturating_add(u64::from(d1 * 10 + d2));
   if d3 >= 5 {
      centis = centis.saturating_add(1);
   }
   Some(centis)
}

fn parse_time_to_centisecs(s: &str) -> Option<u64> {
   let parts: Vec<&str> = s.split(':').collect();
   match parts.len() {
      1 => parse_seconds_centis(parts[0]),
      2 => {
         let mins: u64 = parts[0].parse().ok()?;
         let centis = parse_seconds_centis(parts[1])?;
         Some(mins.saturating_mul(6000).saturating_add(centis))
      },
      3 => {
         let hours: u64 = parts[0].parse().ok()?;
         let mins: u64 = parts[1].parse().ok()?;
         let centis = parse_seconds_centis(parts[2])?;
         Some(
            hours
               .saturating_mul(360_000)
               .saturating_add(mins.saturating_mul(6000))
               .saturating_add(centis),
         )
      },
      _ => None,
   }
}

fn format_centisecs(cs: u64) -> String {
   let hours = cs / 360_000;
   let mins = (cs % 360_000) / 6000;
   let secs_remainder = u32::try_from(cs % 6000).unwrap_or(5999);
   let secs = f64::from(secs_remainder) / 100.0;
   if hours > 0 {
      format!("{hours}:{mins:02}:{secs:05.2}")
   } else if mins > 0 {
      format!("{mins}:{secs:05.2}")
   } else {
      format!("{secs:05.2}")
   }
}

fn sum_sector_times(time1: &str, time2: &str) -> String {
   if time1 == "-" || time2 == "-" {
      return "-".to_string();
   }

   let Some(t1) = parse_time_to_centisecs(time1) else {
      return time1.to_string();
   };
   let Some(t2) = parse_time_to_centisecs(time2) else {
      return time1.to_string();
   };
   let sum = t1.saturating_add(t2);
   format_centisecs(sum)
}

fn resolve_event_name(
   ws_cup: Option<&str>,
   termine_event_name: Option<&str>,
   homepage_event_name: Option<&str>,
   event_id: &str,
) -> String {
   // Check if ws_cup is available (DHLM check handled separately by caller)
   if let Some(cup) = ws_cup {
      return cup.to_string();
   }

   // 24h event fallback
   if event_id == N24_EVENT_ID {
      return N24_TARGET_EVENT_TITLE.to_string();
   }

   // Fall back to termine or homepage names
   if let Some(termine_name) = termine_event_name {
      return termine_name.to_string();
   }

   if let Some(homepage_name) = homepage_event_name {
      return homepage_name.to_string();
   }

   "NLS Live Timing".to_string()
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

#[must_use]
pub fn entry_from_value(v: &Value, event_id: &str) -> Option<TimingEntry> {
   let car_number = parse_u32_field(v, "STNR")?.to_string();
   let class_name = get_str(v, "CLASSNAME").unwrap_or("-").to_string();
   let stable_id = format!("stnr:{car_number}");

   let is_24h = event_id == "50";

   // For 24h, raw S9 contains pit state (PIT/IN/OUT), not time
   let s9_raw = raw_sector_field(v, 9);

   let (sector_1, sector_2, sector_3, sector_4, sector_5) = if is_24h {
      // For 24h, display pit state in sector_5 when S9 contains PIT/IN/OUT
      let s5_display = if s9_raw.eq_ignore_ascii_case("PIT")
         || s9_raw.eq_ignore_ascii_case("IN")
         || s9_raw.eq_ignore_ascii_case("OUT")
      {
         s9_raw.clone()
      } else {
         sum_sector_times(&raw_sector_field(v, 8), &s9_raw)
      };
      (
         sum_sector_times(&raw_sector_field(v, 1), &raw_sector_field(v, 2)),
         sum_sector_times(&raw_sector_field(v, 3), &raw_sector_field(v, 4)),
         sum_sector_times(&raw_sector_field(v, 5), &raw_sector_field(v, 6)),
         raw_sector_field(v, 7),
         s5_display,
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
      // For 24h, derive pit state from raw S9 (contains PIT/IN/OUT)
      // For NLS, derive pit state from S5 (which contains IN/OUT state)
      pit: pit_flag_from_inout_state(if is_24h { &s9_raw } else { &sector_5 }),
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

   let messages = parsed
      .get("MESSAGES")
      .and_then(|value| value.as_array())
      .into_iter()
      .flatten()
      .filter_map(|row| {
         let text = first_non_empty(row, &["MESSAGE", "MSG", "TEXT"])?;
         let id = first_non_empty(row, &["ID"]).unwrap_or("");
         let time = first_non_empty(row, &["MESSAGETIME", "TIME"]).unwrap_or("");

         Some(TimingNotice {
            id:   id.to_string(),
            time: time.to_string(),
            text: text.to_string(),
         })
      })
      .collect::<Vec<_>>();

   filter_german_duplicates(messages)
      .into_iter()
      .map(clean_notice_placeholder_marker)
      .collect()
}

fn clean_notice_placeholder_marker(mut notice: TimingNotice) -> TimingNotice {
   notice.text = notice.text.replace(" ? ", " ");
   notice
}

/// Detects if a notice is German using whichlang.
/// Assumes the feed contains only English and German, so any non-German is
/// treated as English.
fn is_detected_german_notice(text: &str) -> bool {
   whichlang::detect_language(text) == Lang::Deu
}

/// Parses a time string "HH:MM:SS" into seconds since midnight.
fn notice_time_seconds(time: &str) -> Option<u32> {
   let parts: Vec<&str> = time.trim().split(':').collect();
   if parts.len() != 3 {
      return None;
   }
   let h: u32 = parts[0].parse().ok()?;
   let m: u32 = parts[1].parse().ok()?;
   let s: u32 = parts[2].parse().ok()?;
   if h > 23 || m > 59 || s > 59 {
      return None;
   }
   Some(h * 3600 + m * 60 + s)
}

/// Extracts normalized car numbers from a notice text.
fn notice_car_numbers(text: &str) -> BTreeSet<String> {
   let mut cars = BTreeSet::new();
   let mut chars = text.chars().peekable();
   let mut saw_hash = false;

   while let Some(c) = chars.next() {
      if c == '#' {
         saw_hash = true;
         continue;
      }

      if saw_hash {
         // Skip whitespace after hash
         if c.is_whitespace() {
            continue;
         }
         // Check for comma-separated numbers: "#11, #111, #492, #808"
         if c.is_ascii_digit() {
            let mut num = String::new();
            num.push(c);
            while let Some(&next) = chars.peek() {
               if next.is_ascii_digit() {
                  num.push(chars.next().unwrap());
               } else {
                  break;
               }
            }
            // Skip trailing whitespace and check for comma
            while let Some(&next) = chars.peek() {
               if next.is_whitespace() {
                  chars.next();
               } else if next == ',' {
                  chars.next();
                  break;
               } else {
                  break;
               }
            }
            // Normalize by trimming leading zeros
            let normalized = num.trim_start_matches('0');
            if normalized.is_empty() {
               cars.insert("0".to_string());
            } else {
               cars.insert(normalized.to_string());
            }
         }
         saw_hash = false;
      }
   }

   cars
}

/// Internal struct for classified notices to avoid complex tuple type.
struct ClassifiedNotice {
   idx:       usize,
   notice:    TimingNotice,
   is_german: bool,
   time_secs: Option<u32>,
   cars:      BTreeSet<String>,
}

/// Filters German notices that have an English duplicate in the same batch.
/// For each German notice, looks for an English notice with:
/// - Same non-empty car-number set
/// - Timestamp delta <= 20 seconds
/// - Batch index distance <= 3
fn filter_german_duplicates(notices: Vec<TimingNotice>) -> Vec<TimingNotice> {
   if notices.len() < 2 {
      return notices;
   }

   let classified: Vec<ClassifiedNotice> = notices
      .into_iter()
      .enumerate()
      .map(|(idx, notice)| {
         let is_german = is_detected_german_notice(&notice.text);
         let time_secs = notice_time_seconds(&notice.time);
         let cars = notice_car_numbers(&notice.text);
         ClassifiedNotice {
            idx,
            notice,
            is_german,
            time_secs,
            cars,
         }
      })
      .collect();

   let mut filtered = Vec::new();

   for classified_notice in &classified {
      if !classified_notice.is_german {
         // Keep all non-German (English or unknown) notices
         filtered.push(classified_notice.notice.clone());
         continue;
      }

      // For German notices, check if there's an English duplicate
      let has_english_duplicate = classified.iter().any(|other_classified| {
         // Must be English (not German)
         if other_classified.is_german {
            return false;
         }

         // Must have car numbers and they must match
         if classified_notice.cars.is_empty()
            || other_classified.cars.is_empty()
            || classified_notice.cars != other_classified.cars
         {
            return false;
         }

         // Index distance check (max 3)
         let idx_diff = classified_notice.idx.abs_diff(other_classified.idx);
         if idx_diff > 3 {
            return false;
         }

         // Time check (max 20 seconds)
         match (classified_notice.time_secs, other_classified.time_secs) {
            (Some(t1), Some(t2)) => {
               let time_diff = t1.abs_diff(t2);
               time_diff <= 20
            },
            _ => false, // Can't compare times, don't consider duplicate
         }
      });

      if !has_english_duplicate {
         // Keep German notices without an English duplicate
         filtered.push(classified_notice.notice.clone());
      }
   }

   filtered
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

pub(crate) fn parse_ws_message(
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

   // Track event_id in header for web liveticker switching
   header.event_id = event_id.to_string();

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
         let cup_is_dhlm = ws_cup.is_some_and(|name| name.to_ascii_lowercase().contains("dhlm"));

         if cup_is_dhlm {
            header.event_name = ws_cup.unwrap().to_string();
         } else if let Some(cup) = ws_cup {
            // Always prefer websocket CUP/EVENTNAME when available
            header.event_name = cup.to_string();
         } else if event_id == N24_EVENT_ID {
            // 24h fallback when websocket doesn't send CUP/EVENTNAME
            header.event_name = N24_TARGET_EVENT_TITLE.to_string();
         } else if let Some(termine_name) = termine_event_name {
            header.event_name = termine_name.to_string();
         } else if let Some(homepage_name) = homepage_event_name {
            header.event_name = homepage_name.to_string();
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
      },
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
         let cup_is_dhlm = ws_cup.is_some_and(|name| name.to_ascii_lowercase().contains("dhlm"));

         if cup_is_dhlm {
            header.event_name = ws_cup.unwrap().to_string();
         } else {
            header.event_name =
               resolve_event_name(ws_cup, termine_event_name, homepage_event_name, event_id);
         }
         let end_time_raw = get_str(&parsed, "ENDTIME")
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
         let time_state_raw = get_str(&parsed, "TIMESTATE").unwrap_or("0");
         header.day_time = get_str(&parsed, "TIME").unwrap_or("-").to_string();

         *countdown = Some(CountdownState {
            end_time_raw,
            time_state_raw: time_state_raw.to_string(),
            received_at_ms: now_unix_ms(),
            is_race_session: *is_race_session,
         });

         refresh_header_time_to_go(header, countdown.as_ref());
         Some((None, true))
      },
      _ => None,
   }
}

#[cfg(test)]
pub fn set_tcp_read_timeout(stream: &mut std::net::TcpStream, timeout: Duration) {
   let _ = stream.set_read_timeout(Some(timeout));
}

pub(crate) const fn should_emit_connected_status_on_update(
   header_changed: bool,
   connected_status_already_sent: bool,
) -> bool {
   !header_changed && !connected_status_already_sent
}

pub(crate) fn refresh_active_event_id(
   active_event_id: &mut String,
   refresh_result: Result<&str, String>,
) -> Option<String> {
   match refresh_result {
      Ok(event_id) => {
         if *active_event_id == event_id {
            None
         } else {
            *active_event_id = event_id.to_string();
            Some(format!("NLS switching to eventId {event_id}"))
         }
      },
      Err(err) => {
         Some(format!(
            "NLS 24h schedule refresh failed ({err}); keeping eventId {}",
            *active_event_id
         ))
      },
   }
}

pub(crate) fn is_retriable_timeout(err: &WsError) -> bool {
   matches!(
       err,
       WsError::Io(io_err)
           if io_err.kind() == io::ErrorKind::WouldBlock || io_err.kind() == io::ErrorKind::TimedOut
   )
}

pub(crate) fn is_transient_disconnect(err: &WsError) -> bool {
   matches!(
       err,
       WsError::Io(io_err)
           if matches!(
               io_err.kind(),
               io::ErrorKind::ConnectionReset
                   | io::ErrorKind::ConnectionAborted
                   | io::ErrorKind::BrokenPipe
           )
   )
}

pub(crate) fn websocket_stale_elapsed(
   last_activity_at: Instant,
   now: Instant,
   timeout: Duration,
) -> bool {
   now.duration_since(last_activity_at) >= timeout
}

pub(crate) fn websocket_ping_due(last_ping_at: Instant, now: Instant, interval: Duration) -> bool {
   now.duration_since(last_ping_at) >= interval
}
