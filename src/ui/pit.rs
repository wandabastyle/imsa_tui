use std::{collections::HashMap, time::Duration, time::Instant};

use ratatui::style::{Color, Modifier, Style};

use crate::timing::{Series, TimingEntry};

#[derive(Clone, Copy, PartialEq, Eq)]
enum PitHighlightPhase {
    None,
    In,
    Pit,
    Out,
}

#[derive(Clone)]
pub(crate) struct PitTracker {
    in_pit: bool,
    in_until: Option<Instant>,
    out_until: Option<Instant>,
}

impl PitTracker {
    fn new() -> Self {
        Self {
            in_pit: false,
            in_until: None,
            out_until: None,
        }
    }
}

fn pit_signal_active(active_series: Series, entry: &TimingEntry) -> bool {
    match active_series {
        Series::Imsa | Series::F1 | Series::Wec => entry.pit.eq_ignore_ascii_case("yes"),
        Series::Nls => entry.sector_5.trim().eq_ignore_ascii_case("PIT"),
    }
}

fn pit_phase_style(phase: PitHighlightPhase) -> Option<Style> {
    match phase {
        PitHighlightPhase::None => None,
        PitHighlightPhase::In => Some(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        PitHighlightPhase::Pit => Some(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        PitHighlightPhase::Out => Some(
            Style::default()
                .fg(Color::LightMagenta)
                .add_modifier(Modifier::BOLD),
        ),
    }
}

pub(crate) fn refresh_pit_trackers(
    trackers: &mut HashMap<String, PitTracker>,
    entries: &[TimingEntry],
    active_series: Series,
    now: Instant,
) {
    const IN_HIGHLIGHT_WINDOW: Duration = Duration::from_millis(1200);
    const OUT_HIGHLIGHT_WINDOW: Duration = Duration::from_millis(1800);

    let current_ids: std::collections::HashSet<String> = entries
        .iter()
        .map(|entry| entry.stable_id.clone())
        .collect();
    trackers.retain(|stable_id, _| current_ids.contains(stable_id));

    for entry in entries {
        let signal = pit_signal_active(active_series, entry);
        let tracker = trackers
            .entry(entry.stable_id.clone())
            .or_insert_with(PitTracker::new);

        if signal {
            if !tracker.in_pit {
                tracker.in_pit = true;
                tracker.in_until = Some(now + IN_HIGHLIGHT_WINDOW);
            }
            tracker.out_until = None;
        } else if tracker.in_pit {
            tracker.in_pit = false;
            tracker.in_until = None;
            tracker.out_until = Some(now + OUT_HIGHLIGHT_WINDOW);
        }
    }
}

pub(crate) fn pit_style_for_entry(
    trackers: &HashMap<String, PitTracker>,
    entry: &TimingEntry,
    now: Instant,
) -> Option<Style> {
    pit_phase_style(pit_phase_for_entry(trackers, entry, now))
}

fn pit_phase_for_entry(
    trackers: &HashMap<String, PitTracker>,
    entry: &TimingEntry,
    now: Instant,
) -> PitHighlightPhase {
    let Some(tracker) = trackers.get(&entry.stable_id) else {
        return PitHighlightPhase::None;
    };

    if tracker.in_pit {
        if tracker.in_until.map(|until| now <= until).unwrap_or(false) {
            PitHighlightPhase::In
        } else {
            PitHighlightPhase::Pit
        }
    } else if tracker.out_until.map(|until| now <= until).unwrap_or(false) {
        PitHighlightPhase::Out
    } else {
        PitHighlightPhase::None
    }
}
