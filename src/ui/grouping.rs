use std::collections::HashSet;

use super::style::class_display_name;
use crate::{
   favourites,
   timing::{
      Series,
      TimingEntry,
   },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
   Overall,
   Grouped,
   Class(usize),
   Favourites,
}

pub fn grouped_entries(
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

   groups.sort_by(|(an, ae), (bn, be)| {
      let a_best = ae.iter().map(|e| e.position).min().unwrap_or(u32::MAX);
      let b_best = be.iter().map(|e| e.position).min().unwrap_or(u32::MAX);
      a_best.cmp(&b_best).then_with(|| an.cmp(bn))
   });

   groups
}

pub const fn next_view_mode(current: ViewMode, groups_len: usize) -> ViewMode {
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
      },
      ViewMode::Favourites => ViewMode::Overall,
   }
}

pub fn view_entries_for_mode<'a>(
   all_entries: &'a [TimingEntry],
   current_groups: &'a [(String, Vec<TimingEntry>)],
   view_mode: ViewMode,
   favourites_set: &HashSet<String>,
   active_series: Series,
) -> Vec<&'a TimingEntry> {
   match view_mode {
      ViewMode::Overall => all_entries.iter().collect(),
      ViewMode::Grouped => {
         current_groups
            .iter()
            .flat_map(|(_, class_entries)| class_entries.iter())
            .collect()
      },
      ViewMode::Class(idx) => {
         current_groups
            .get(idx)
            .map(|(_, class_entries)| class_entries.iter().collect())
            .unwrap_or_default()
      },
      ViewMode::Favourites => {
         all_entries
            .iter()
            .filter(|entry| {
               favourites_set.contains(&favourites::favourite_key(active_series, &entry.stable_id))
            })
            .collect()
      },
   }
}

#[must_use]
pub fn selected_group_idx(
   selected_row: usize,
   current_groups: &[(String, Vec<TimingEntry>)],
) -> usize {
   let mut running = 0usize;
   for (idx, (_, class_entries)) in current_groups.iter().enumerate() {
      if selected_row < running + class_entries.len() {
         return idx;
      }
      running += class_entries.len();
   }
   0
}

#[must_use]
pub fn group_start_row(current_groups: &[(String, Vec<TimingEntry>)], group_idx: usize) -> usize {
   current_groups
      .iter()
      .take(group_idx)
      .map(|(_, entries)| entries.len())
      .sum()
}

#[must_use]
pub fn step_group_selection(
   selected_row: usize,
   current_groups: &[(String, Vec<TimingEntry>)],
   delta: isize,
) -> usize {
   if current_groups.is_empty() {
      return 0;
   }

   let current_group = selected_group_idx(selected_row, current_groups);
   let len = current_groups.len();
   let steps = delta.unsigned_abs() % len;
   let next_group = if delta.is_negative() {
      (current_group + len - steps) % len
   } else {
      (current_group + steps) % len
   };

   group_start_row(current_groups, next_group)
}

pub fn view_mode_text(view_mode: ViewMode, group_names: &[String]) -> String {
   match view_mode {
      ViewMode::Overall => "Overall".to_string(),
      ViewMode::Grouped => "Grouped".to_string(),
      ViewMode::Favourites => "Favourites".to_string(),
      ViewMode::Class(idx) => {
         group_names
            .get(idx)
            .map_or_else(|| "Class".to_string(), |name| format!("Class {name}"))
      },
   }
}

pub fn selected_series_index(series: Series) -> usize {
   Series::all()
      .iter()
      .position(|candidate| *candidate == series)
      .unwrap_or(0)
}

pub fn favourites_count_for_series(series: Series, favourites: &HashSet<String>) -> usize {
   let prefix = format!("{}|", series.as_key_prefix());
   favourites
      .iter()
      .filter(|value| value.starts_with(&prefix))
      .count()
}

pub fn display_event_name(_series: Series, raw: &str) -> String {
   if raw.trim().is_empty() || raw == "-" {
      return "-".to_string();
   }

   raw.trim().to_string()
}

pub fn display_session_name(series: Series, raw: &str) -> String {
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

#[cfg(test)]
mod tests {
   use super::*;

   fn entry(stable_id: &str) -> TimingEntry {
      TimingEntry {
         stable_id: stable_id.to_string(),
         ..TimingEntry::default()
      }
   }

   fn groups() -> Vec<(String, Vec<TimingEntry>)> {
      vec![
         ("A".to_string(), vec![entry("a1"), entry("a2")]),
         ("B".to_string(), vec![entry("b1"), entry("b2"), entry("b3")]),
         ("C".to_string(), vec![entry("c1")]),
      ]
   }

   #[test]
   fn step_group_selection_moves_to_group_starts() {
      let groups = groups();

      assert_eq!(step_group_selection(0, &groups, 1), 2);
      assert_eq!(step_group_selection(3, &groups, 1), 5);
      assert_eq!(step_group_selection(3, &groups, -1), 0);
   }

   #[test]
   fn step_group_selection_wraps_between_groups() {
      let groups = groups();

      assert_eq!(step_group_selection(0, &groups, -1), 5);
      assert_eq!(step_group_selection(5, &groups, 1), 0);
      assert_eq!(step_group_selection(0, &groups, 10), 2);
   }
}
