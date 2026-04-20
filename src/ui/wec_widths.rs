use ratatui::layout::Constraint;
use serde::{Deserialize, Serialize};

use crate::timing::TimingEntry;

use super::width_math::{distribute_extra_space, max_text_width, reduce_widths_in_order};

const WEC_COLUMN_COUNT: usize = 14;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WecColumnWidths {
    pos: u16,
    car_number: u16,
    class: u16,
    pic: u16,
    driver: u16,
    vehicle: u16,
    team: u16,
    laps: u16,
    gap: u16,
    last: u16,
    best: u16,
    s1: u16,
    s2: u16,
    s3: u16,
}

impl WecColumnWidths {
    const fn header_minimums() -> Self {
        Self {
            pos: 3,
            car_number: 1,
            class: 5,
            pic: 3,
            driver: 6,
            vehicle: 7,
            team: 6,
            laps: 4,
            gap: 3,
            last: 4,
            best: 4,
            s1: 2,
            s2: 2,
            s3: 2,
        }
    }

    pub(crate) fn from_entries(entries: &[TimingEntry]) -> Option<Self> {
        if entries.is_empty() {
            return None;
        }
        let pos = entries
            .iter()
            .map(|entry| entry.position.to_string().chars().count())
            .max()
            .unwrap_or(1) as u16;

        Some(Self {
            pos,
            car_number: max_text_width(entries, |entry| &entry.car_number),
            class: max_text_width(entries, |entry| &entry.class_name),
            pic: max_text_width(entries, |entry| &entry.class_rank),
            driver: max_text_width(entries, |entry| &entry.driver),
            vehicle: max_text_width(entries, |entry| &entry.vehicle),
            team: max_text_width(entries, |entry| &entry.team),
            laps: max_text_width(entries, |entry| &entry.laps),
            gap: max_text_width(entries, |entry| &entry.gap_overall),
            last: max_text_width(entries, |entry| &entry.last_lap),
            best: max_text_width(entries, |entry| &entry.best_lap),
            s1: max_text_width(entries, |entry| &entry.sector_1),
            s2: max_text_width(entries, |entry| &entry.sector_2),
            s3: max_text_width(entries, |entry| &entry.sector_3),
        })
    }

    fn enforce_header_minimums(self) -> Self {
        let mins = Self::header_minimums();
        Self {
            pos: self.pos.max(mins.pos),
            car_number: self.car_number.max(mins.car_number),
            class: self.class.max(mins.class),
            pic: self.pic.max(mins.pic),
            driver: self.driver.max(mins.driver),
            vehicle: self.vehicle.max(mins.vehicle),
            team: self.team.max(mins.team),
            laps: self.laps.max(mins.laps),
            gap: self.gap.max(mins.gap),
            last: self.last.max(mins.last),
            best: self.best.max(mins.best),
            s1: self.s1.max(mins.s1),
            s2: self.s2.max(mins.s2),
            s3: self.s3.max(mins.s3),
        }
    }

    fn to_array(self) -> [u16; WEC_COLUMN_COUNT] {
        [
            self.pos,
            self.car_number,
            self.class,
            self.pic,
            self.driver,
            self.vehicle,
            self.team,
            self.laps,
            self.gap,
            self.last,
            self.best,
            self.s1,
            self.s2,
            self.s3,
        ]
    }

    fn from_array(values: [u16; WEC_COLUMN_COUNT]) -> Self {
        Self {
            pos: values[0],
            car_number: values[1],
            class: values[2],
            pic: values[3],
            driver: values[4],
            vehicle: values[5],
            team: values[6],
            laps: values[7],
            gap: values[8],
            last: values[9],
            best: values[10],
            s1: values[11],
            s2: values[12],
            s3: values[13],
        }
    }

    pub(crate) fn driver_width(self) -> usize {
        self.driver as usize
    }

    pub(crate) fn vehicle_width(self) -> usize {
        self.vehicle as usize
    }

    pub(crate) fn team_width(self) -> usize {
        self.team as usize
    }

    pub(crate) fn merge_keep_larger(self, other: Self) -> Self {
        let a = self.to_array();
        let b = other.to_array();
        let mut merged = [0_u16; WEC_COLUMN_COUNT];
        for idx in 0..WEC_COLUMN_COUNT {
            merged[idx] = a[idx].max(b[idx]);
        }
        Self::from_array(merged)
    }
}

pub(crate) fn calculate_wec_widths(
    terminal_width: u16,
    entries: &[TimingEntry],
    baseline: Option<&WecColumnWidths>,
) -> WecColumnWidths {
    let observed = WecColumnWidths::from_entries(entries);
    let target = match (baseline.copied(), observed) {
        (Some(base), Some(obs)) => base.merge_keep_larger(obs),
        (Some(base), None) => base,
        (None, Some(obs)) => obs,
        (None, None) => WecColumnWidths::header_minimums(),
    }
    .enforce_header_minimums();

    let mut widths = target.to_array();
    let minimums = WecColumnWidths::header_minimums().to_array();
    let gutters = (WEC_COLUMN_COUNT.saturating_sub(1)) as u16;
    let available_width = terminal_width.saturating_sub(gutters);
    let total_width: u16 = widths.iter().sum();

    if total_width < available_width {
        distribute_extra_space(&mut widths, available_width - total_width);
    } else if total_width > available_width {
        let mut deficit = total_width - available_width;

        deficit = reduce_widths_in_order(&mut widths, &minimums, deficit, &[6, 5]);
        deficit = reduce_widths_in_order(
            &mut widths,
            &minimums,
            deficit,
            &[1, 2, 3, 7, 8, 9, 10, 11, 12, 13],
        );
        deficit = reduce_widths_in_order(&mut widths, &minimums, deficit, &[4, 0]);

        if deficit > 0 {
            widths = minimums;
        }
    }

    WecColumnWidths::from_array(widths)
}

pub(crate) fn wec_constraints(widths: WecColumnWidths) -> Vec<Constraint> {
    widths
        .to_array()
        .into_iter()
        .map(Constraint::Length)
        .collect()
}
