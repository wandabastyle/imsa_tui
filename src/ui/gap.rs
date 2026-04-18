use crate::timing::TimingEntry;

#[derive(Clone)]
pub(crate) struct GapAnchorInfo {
    stable_id: String,
    laps: String,
    gap_overall: String,
    gap_class: String,
    gap_next_in_class: String,
    best_lap: String,
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

pub(crate) fn gap_anchor_from_entry(entry: &TimingEntry) -> GapAnchorInfo {
    GapAnchorInfo {
        stable_id: entry.stable_id.clone(),
        laps: entry.laps.clone(),
        gap_overall: entry.gap_overall.clone(),
        gap_class: entry.gap_class.clone(),
        gap_next_in_class: entry.gap_next_in_class.clone(),
        best_lap: entry.best_lap.clone(),
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

fn parse_best_lap_time(raw: &str) -> Option<i64> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "-" {
        return None;
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_digit() || ch == ':' || ch == '.' || ch == ',')
    {
        return None;
    }
    let normalized = trimmed.replace(',', ".");
    let total_ms = if let Some((left, right)) = normalized.rsplit_once(':') {
        let secs = right.parse::<f64>().ok()?;
        let mins = left.parse::<u64>().ok()?;
        ((mins as f64 * 60.0 + secs) * 1000.0).round() as i64
    } else {
        (normalized.parse::<f64>().ok()? * 1000.0).round() as i64
    };
    Some(total_ms)
}

fn is_qualifying_or_practice(session_name: &str) -> bool {
    let normalized = session_name.trim().to_ascii_lowercase();
    normalized != "race" && !normalized.is_empty() && normalized != "-"
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

pub(crate) fn relative_gap_overall_text(
    entry: &TimingEntry,
    raw_value: &str,
    anchor: Option<&GapAnchorInfo>,
    session_name: &str,
) -> String {
    relative_gap_text(entry, raw_value, GapColumn::Overall, anchor, session_name)
}

pub(crate) fn relative_gap_class_text(
    entry: &TimingEntry,
    raw_value: &str,
    anchor: Option<&GapAnchorInfo>,
    session_name: &str,
) -> String {
    relative_gap_text(entry, raw_value, GapColumn::Class, anchor, session_name)
}

pub(crate) fn relative_gap_next_in_class_text(
    entry: &TimingEntry,
    raw_value: &str,
    anchor: Option<&GapAnchorInfo>,
    session_name: &str,
) -> String {
    relative_gap_text(
        entry,
        raw_value,
        GapColumn::NextInClass,
        anchor,
        session_name,
    )
}

fn relative_gap_text(
    entry: &TimingEntry,
    raw_value: &str,
    column: GapColumn,
    anchor: Option<&GapAnchorInfo>,
    session_name: &str,
) -> String {
    let Some(anchor) = anchor else {
        return raw_value.to_string();
    };

    if entry.stable_id == anchor.stable_id {
        if is_qualifying_or_practice(session_name) {
            return anchor.best_lap.clone();
        }
        return anchor_gap_label(&anchor.laps);
    }

    if is_qualifying_or_practice(session_name) {
        let row_best = parse_best_lap_time(&entry.best_lap);
        let anchor_best = parse_best_lap_time(&anchor.best_lap);
        if let (Some(row_best), Some(anchor_best)) = (row_best, anchor_best) {
            return format_time_delta(row_best - anchor_best);
        }
        return raw_value.to_string();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(stable_id: &str, laps: &str, gap_overall: &str) -> TimingEntry {
        TimingEntry {
            stable_id: stable_id.to_string(),
            laps: laps.to_string(),
            gap_overall: gap_overall.to_string(),
            ..TimingEntry::default()
        }
    }

    #[test]
    fn relative_gap_uses_lap_delta_when_laps_differ() {
        let anchor_entry = entry("car-a", "101", "12.300");
        let row_entry = entry("car-b", "100", "13.100");
        let anchor = gap_anchor_from_entry(&anchor_entry);

        assert_eq!(
            relative_gap_overall_text(&row_entry, &row_entry.gap_overall, Some(&anchor), "Race"),
            "+1 LAP"
        );
    }

    #[test]
    fn relative_gap_uses_time_delta_when_on_same_lap() {
        let anchor_entry = entry("car-a", "90", "10.400");
        let row_entry = entry("car-b", "90", "12.000");
        let anchor = gap_anchor_from_entry(&anchor_entry);

        assert_eq!(
            relative_gap_overall_text(&row_entry, &row_entry.gap_overall, Some(&anchor), "Race"),
            "+1.600"
        );
    }

    #[test]
    fn relative_gap_uses_best_lap_when_practice() {
        let anchor_entry = TimingEntry {
            stable_id: "car-a".to_string(),
            laps: "5".to_string(),
            best_lap: "1:44.500".to_string(),
            ..TimingEntry::default()
        };
        let row_entry = TimingEntry {
            stable_id: "car-b".to_string(),
            laps: "5".to_string(),
            best_lap: "1:43.200".to_string(),
            ..TimingEntry::default()
        };
        let anchor = gap_anchor_from_entry(&anchor_entry);

        assert_eq!(
            relative_gap_overall_text(
                &row_entry,
                &row_entry.gap_overall,
                Some(&anchor),
                "Practice"
            ),
            "-1.300"
        );
    }

    #[test]
    fn relative_gap_uses_best_lap_when_qualifying() {
        let anchor_entry = TimingEntry {
            stable_id: "car-a".to_string(),
            laps: "8".to_string(),
            best_lap: "2:01.500".to_string(),
            ..TimingEntry::default()
        };
        let row_entry = TimingEntry {
            stable_id: "car-b".to_string(),
            laps: "8".to_string(),
            best_lap: "2:02.100".to_string(),
            ..TimingEntry::default()
        };
        let anchor = gap_anchor_from_entry(&anchor_entry);

        assert_eq!(
            relative_gap_overall_text(
                &row_entry,
                &row_entry.gap_overall,
                Some(&anchor),
                "Qualifying"
            ),
            "+0.600"
        );
    }
}
