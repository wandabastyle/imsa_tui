// AppState - Core application state and types

use std::{
   collections::{HashMap, HashSet, VecDeque},
   sync::mpsc::Sender,
   time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::{
   adapters::nls::liveticker::{ActiveLivetickerFeed, LivetickerEntry},
   timing::{TimingEntry, TimingHeader, TimingMessage, TimingNotice},
};

use super::{
   config::AppConfig,
   feed::ActiveFeed,
   grouping::ViewMode,
   popups::NlsLivetickerPanelState,
   search::SearchState,
};

/// Context for series change operations.
pub struct SeriesChangeCtx<'a> {
   pub active_series:              &'a mut crate::timing::Series,
   pub feed:                       &'a mut Option<ActiveFeed>,
   pub tx:                         &'a Sender<TimingMessage>,
   pub source_id_ctr:              &'a mut u64,
   pub demo_mode:                  bool,
   pub header:                     &'a mut TimingHeader,
   pub entries:                    &'a mut Vec<TimingEntry>,
   pub status:                     &'a mut String,
   pub favourites:                 &'a mut HashSet<String>,
   pub notices:                    &'a mut Vec<TimingNotice>,
   pub notice_keys:                &'a mut HashSet<String>,
   pub highlighted_notice_cars:    &'a mut HashSet<String>,
   pub message_flag_override:      &'a mut Option<MessageFlagOverride>,
   pub message_flag_last_secs:     &'a mut Option<u32>,
   pub last_error:                 &'a mut Option<String>,
   pub last_update:                &'a mut Option<Instant>,
   pub selected_row:               &'a mut usize,
   pub view_mode:                  &'a mut ViewMode,
   pub search:                     &'a mut SearchState,
   pub series_logs:                &'a mut VecDeque<String>,
   pub config:                     &'a mut AppConfig,
   pub nls_liveticker_feed:        &'a mut Option<ActiveLivetickerFeed>,
   pub nls_liveticker_entries:     &'a mut Vec<LivetickerEntry>,
   pub nls_liveticker_last_update: &'a mut Option<Instant>,
   pub nls_liveticker_last_error:  &'a mut Option<String>,
   pub nls_liveticker_panel:       &'a mut NlsLivetickerPanelState,
}

#[derive(Debug, Clone)]
pub struct MessageFlagOverride {
   pub flag: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagMessageIntent {
   SetRed,
   SetCode60,
   SetYellow,
   Clear,
}

pub const DISMISSED_NOTICE_TTL_SECS: u64 = 7 * 24 * 60 * 60;
pub const DISMISSED_NOTICE_MAX_PER_SERIES: usize = 500;

pub fn now_unix_secs() -> u64 {
   SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .map_or(0, |duration| duration.as_secs())
}

pub fn parse_notice_time_seconds(raw: &str) -> Option<u32> {
   let trimmed = raw.trim();
   let mut parts = trimmed.split(':');
   let h = parts.next()?.parse::<u32>().ok()?;
   let m = parts.next()?.parse::<u32>().ok()?;
   let s = parts.next()?.parse::<u32>().ok()?;
   if parts.next().is_some() || h > 23 || m > 59 || s > 59 {
      return None;
   }
   Some(h * 3600 + m * 60 + s)
}

pub fn classify_flag_message_intent(text: &str) -> Option<FlagMessageIntent> {
   let normalized = text.trim().to_ascii_lowercase();
   if normalized.is_empty() {
      return None;
   }

   if normalized.contains("green flag")
      || normalized.contains("all clear")
      || normalized.contains("track clear")
      || normalized.contains("resume")
      || normalized.contains("re-start")
      || normalized.contains("restart")
      || normalized.contains("grune flagge")
      || normalized.contains("gruene flagge")
   {
      return Some(FlagMessageIntent::Clear);
   }

   let looks_like_penalty = normalized.contains("non respect")
      || normalized.contains("penalty")
      || normalized.contains("time penalty")
      || normalized.contains("pit speed")
      || normalized.contains("after first lap");
   if looks_like_penalty {
      return None;
   }

   if normalized.contains("red flag") || normalized.contains("rote flagge") {
      return Some(FlagMessageIntent::SetRed);
   }
   if normalized.contains("full course yellow")
      || normalized.contains("fcy")
      || normalized.contains("yellow flag")
      || normalized.contains("gelb")
   {
      return Some(FlagMessageIntent::SetYellow);
   }
   if normalized.starts_with("code 60")
      || normalized.contains(" code 60 phase")
      || normalized.contains("code60 phase")
      || normalized.contains("code 60 in force")
   {
      return Some(FlagMessageIntent::SetCode60);
   }

   None
}

pub fn apply_flag_message_notice(
   notice: &TimingNotice,
   override_state: &mut Option<MessageFlagOverride>,
   last_secs: &mut Option<u32>,
) {
   let Some(intent) = classify_flag_message_intent(&notice.text) else {
      return;
   };
   let Some(notice_secs) = parse_notice_time_seconds(&notice.time) else {
      return;
   };

   if let Some(current_secs) = *last_secs {
      if notice_secs < current_secs {
         return;
      }
   }
   *last_secs = Some(notice_secs);

   match intent {
      FlagMessageIntent::SetRed => {
         *override_state = Some(MessageFlagOverride {
            flag: "Red".to_string(),
         });
      },
      FlagMessageIntent::SetCode60 => {
         *override_state = Some(MessageFlagOverride {
            flag: "Code 60".to_string(),
         });
      },
      FlagMessageIntent::SetYellow => {
         *override_state = Some(MessageFlagOverride {
            flag: "Yellow".to_string(),
         });
      },
      FlagMessageIntent::Clear => {
         *override_state = None;
      },
   }
}

pub fn extract_notice_car_numbers(text: &str) -> HashSet<String> {
   let chars: Vec<char> = text.chars().collect();
   let mut car_numbers = HashSet::new();
   let mut idx = 0usize;

   while idx < chars.len() {
      // Look for #NUMBER (NLS style)
      if chars[idx] == '#' {
         idx += 1;
         let start = idx;
         while idx < chars.len() && chars[idx].is_ascii_digit() {
            idx += 1;
         }
         if idx == start {
            continue;
         }

         let raw: String = chars[start..idx].iter().collect();
         if raw.is_empty() {
            continue;
         }

         car_numbers.insert(raw.clone());
         let normalized = raw.trim_start_matches('0');
         if !normalized.is_empty() {
            car_numbers.insert(normalized.to_string());
         }
         continue;
      }

      // Look for CAR NUMBER (WEC style) - singular CAR only
      if idx + 3 < chars.len()
         && chars[idx] == 'C'
         && chars[idx + 1] == 'A'
         && chars[idx + 2] == 'R'
      {
         // Check for singular "CAR" not "CARS" - next char after CAR must be whitespace or end
         let after_car = idx + 3;
         if after_car < chars.len() && chars[after_car].is_ascii_whitespace() {
            // Check if next non-whitespace char is a digit
            let mut num_start = after_car + 1;
            while num_start < chars.len() && chars[num_start].is_ascii_whitespace() {
               num_start += 1;
            }

            if num_start < chars.len() && chars[num_start].is_ascii_digit() {
               // Collect digits (1-3 digits for car numbers)
               let mut num_end = num_start;
               while num_end < chars.len()
                  && chars[num_end].is_ascii_digit()
                  && num_end - num_start < 3
               {
                  num_end += 1;
               }

               let raw: String = chars[num_start..num_end].iter().collect();
               if !raw.is_empty() {
                  // For CAR XXX, insert only exact raw number (no normalization)
                  car_numbers.insert(raw);
               }

               idx = num_end;
               continue;
            }
         }
      }

      idx += 1;
   }

   car_numbers
}

pub fn notice_key(notice: &TimingNotice) -> String {
   format!(
      "{}|{}|{}",
      notice.id.trim(),
      notice.time.trim(),
      notice.text.trim()
   )
}

pub fn normalized_notice_text_for_dismissal_key(text: &str) -> String {
   let collapsed = text
      .split_whitespace()
      .collect::<Vec<_>>()
      .join(" ")
      .to_ascii_lowercase();

   let chars: Vec<char> = collapsed.chars().collect();
   let mut normalized = String::with_capacity(chars.len());
   let mut idx = 0usize;

   while idx < chars.len() {
      if chars[idx] != '#' {
         normalized.push(chars[idx]);
         idx += 1;
         continue;
      }

      normalized.push('#');
      idx += 1;
      while idx < chars.len() && chars[idx].is_ascii_whitespace() {
         idx += 1;
      }

      if idx < chars.len() && chars[idx].is_ascii_digit() {
         while idx < chars.len() && chars[idx].is_ascii_digit() {
            normalized.push(chars[idx]);
            idx += 1;
         }
      }
   }

   normalized
}

pub fn persisted_notice_identity_key(notice: &TimingNotice) -> String {
   let text = normalized_notice_text_for_dismissal_key(&notice.text);
   format!("text|{text}")
}

pub fn persisted_notice_key(
   series: crate::timing::Series,
   notice: &TimingNotice,
) -> String {
   format!(
      "{}|{}",
      series.as_key_prefix(),
      persisted_notice_identity_key(notice)
   )
}

pub fn prune_dismissed_notice_keys(
   dismissed: &mut HashMap<String, u64>,
   now_unix_secs: u64,
) -> bool {
   let mut changed = false;

   for timestamp in dismissed.values_mut() {
      if *timestamp == 0 {
         *timestamp = now_unix_secs;
         changed = true;
      }
   }

   let before_ttl = dismissed.len();
   dismissed
      .retain(|_, timestamp| now_unix_secs.saturating_sub(*timestamp) <= DISMISSED_NOTICE_TTL_SECS);
   if dismissed.len() != before_ttl {
      changed = true;
   }

   let mut by_series: HashMap<String, Vec<(String, u64)>> = HashMap::new();
   for (key, timestamp) in dismissed.iter() {
      let prefix = key
         .split_once('|')
         .map(|(series, _)| series)
         .unwrap_or_default()
         .to_string();
      by_series
         .entry(prefix)
         .or_default()
         .push((key.clone(), *timestamp));
   }

   let mut keys_to_remove = HashSet::new();
   for (_, mut keys) in by_series {
      if keys.len() <= DISMISSED_NOTICE_MAX_PER_SERIES {
         continue;
      }
      keys.sort_by_key(|entry| std::cmp::Reverse(entry.1));
      for (key, _) in keys.into_iter().skip(DISMISSED_NOTICE_MAX_PER_SERIES) {
         keys_to_remove.insert(key);
      }
   }

   if !keys_to_remove.is_empty() {
      dismissed.retain(|key, _| !keys_to_remove.contains(key));
      changed = true;
   }

   changed
}

pub fn clear_dismissed_notice_keys_for_series(
   series: crate::timing::Series,
   dismissed: &mut HashMap<String, u64>,
) {
   let prefix = format!("{}|", series.as_key_prefix());
   dismissed.retain(|key, _| !key.starts_with(&prefix));
}

pub fn persist_dismissed_notice_keys(
   config: &mut AppConfig,
   dismissed_notice_keys: &mut HashMap<String, u64>,
   last_error: &mut Option<String>,
) {
   prune_dismissed_notice_keys(dismissed_notice_keys, now_unix_secs());
   config
      .dismissed_notice_keys
      .clone_from(dismissed_notice_keys);
   if let Err(err) = super::config::save_config(config) {
      *last_error = Some(err);
   }
}

pub fn rebuild_highlighted_notice_cars(notices: &[TimingNotice]) -> HashSet<String> {
   let mut highlighted = HashSet::new();
   for notice in notices {
      highlighted.extend(extract_notice_car_numbers(&notice.text));
   }
   highlighted
}

pub fn step_selection(current: usize, len: usize, delta: isize) -> usize {
   if len == 0 {
      return 0;
   }
   let max = (len - 1) as isize;
   ((current as isize + delta).clamp(0, max)) as usize
}
