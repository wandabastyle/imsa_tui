use ratatui::layout::Constraint;
use serde::{Deserialize, Serialize};

use crate::timing::TimingEntry;

use super::width_math::{distribute_extra_space, max_text_width, reduce_widths_in_order};

const IMSA_COLUMN_COUNT: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ImsaColumnWidths {
    pos: u16,
    car_number: u16,
    class: u16,
    pic: u16,
    driver: u16,
    vehicle: u16,
    laps: u16,
    gap_o: u16,
    gap_c: u16,
    next_c: u16,
    last: u16,
    best: u16,
    bl: u16,
    pit: u16,
    stop: u16,
    fastest: u16,
}

impl ImsaColumnWidths {
    const fn header_minimums() -> Self {
        Self {
            pos: 3,
            car_number: 1,
            class: 5,
            pic: 3,
            driver: 6,
            vehicle: 7,
            laps: 4,
            gap_o: 5,
            gap_c: 5,
            next_c: 6,
            last: 4,
            best: 4,
            bl: 3,
            pit: 3,
            stop: 4,
            fastest: 14,
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
            laps: max_text_width(entries, |entry| &entry.laps),
            gap_o: max_text_width(entries, |entry| &entry.gap_overall),
            gap_c: max_text_width(entries, |entry| &entry.gap_class),
            next_c: max_text_width(entries, |entry| &entry.gap_next_in_class),
            last: max_text_width(entries, |entry| &entry.last_lap),
            best: max_text_width(entries, |entry| &entry.best_lap),
            bl: max_text_width(entries, |entry| &entry.best_lap_no),
            pit: max_text_width(entries, |entry| &entry.pit),
            stop: max_text_width(entries, |entry| &entry.pit_stops),
            fastest: max_text_width(entries, |entry| &entry.fastest_driver),
        })
    }

    pub(crate) fn merge_keep_larger(self, other: Self) -> Self {
        Self {
            pos: self.pos.max(other.pos),
            car_number: self.car_number.max(other.car_number),
            class: self.class.max(other.class),
            pic: self.pic.max(other.pic),
            driver: self.driver.max(other.driver),
            vehicle: self.vehicle.max(other.vehicle),
            laps: self.laps.max(other.laps),
            gap_o: self.gap_o.max(other.gap_o),
            gap_c: self.gap_c.max(other.gap_c),
            next_c: self.next_c.max(other.next_c),
            last: self.last.max(other.last),
            best: self.best.max(other.best),
            bl: self.bl.max(other.bl),
            pit: self.pit.max(other.pit),
            stop: self.stop.max(other.stop),
            fastest: self.fastest.max(other.fastest),
        }
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
            laps: self.laps.max(mins.laps),
            gap_o: self.gap_o.max(mins.gap_o),
            gap_c: self.gap_c.max(mins.gap_c),
            next_c: self.next_c.max(mins.next_c),
            last: self.last.max(mins.last),
            best: self.best.max(mins.best),
            bl: self.bl.max(mins.bl),
            pit: self.pit.max(mins.pit),
            stop: self.stop.max(mins.stop),
            fastest: self.fastest.max(mins.fastest),
        }
    }

    pub(crate) fn to_array(self) -> [u16; 16] {
        [
            self.pos,
            self.car_number,
            self.class,
            self.pic,
            self.driver,
            self.vehicle,
            self.laps,
            self.gap_o,
            self.gap_c,
            self.next_c,
            self.last,
            self.best,
            self.bl,
            self.pit,
            self.stop,
            self.fastest,
        ]
    }

    fn from_array(values: [u16; 16]) -> Self {
        Self {
            pos: values[0],
            car_number: values[1],
            class: values[2],
            pic: values[3],
            driver: values[4],
            vehicle: values[5],
            laps: values[6],
            gap_o: values[7],
            gap_c: values[8],
            next_c: values[9],
            last: values[10],
            best: values[11],
            bl: values[12],
            pit: values[13],
            stop: values[14],
            fastest: values[15],
        }
    }

    pub(crate) fn driver_width(self) -> usize {
        self.driver as usize
    }

    pub(crate) fn vehicle_width(self) -> usize {
        self.vehicle as usize
    }

    pub(crate) fn fastest_width(self) -> usize {
        self.fastest as usize
    }
}

pub(crate) fn calculate_imsa_widths(
    terminal_width: u16,
    entries: &[TimingEntry],
    baseline: Option<&ImsaColumnWidths>,
) -> ImsaColumnWidths {
    let observed = ImsaColumnWidths::from_entries(entries);
    let target = match (baseline.copied(), observed) {
        (Some(base), Some(obs)) => base.merge_keep_larger(obs).enforce_header_minimums(),
        (Some(base), None) => base.enforce_header_minimums(),
        (None, Some(obs)) => obs.enforce_header_minimums(),
        (None, None) => ImsaColumnWidths::header_minimums(),
    };

    let mut widths = target.to_array();
    let minimums = ImsaColumnWidths::header_minimums().to_array();
    let gutters = (IMSA_COLUMN_COUNT.saturating_sub(1)) as u16;
    let available_width = terminal_width.saturating_sub(gutters);
    let total_width: u16 = widths.iter().sum();

    if total_width < available_width {
        distribute_extra_space(&mut widths, available_width - total_width);
    } else if total_width > available_width {
        let mut deficit = total_width - available_width;

        deficit = reduce_widths_in_order(&mut widths, &minimums, deficit, &[5]);
        deficit = reduce_widths_in_order(
            &mut widths,
            &minimums,
            deficit,
            &[1, 2, 3, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
        );
        deficit = reduce_widths_in_order(&mut widths, &minimums, deficit, &[4, 0]);

        if deficit > 0 {
            widths = minimums;
        }
    }

    ImsaColumnWidths::from_array(widths)
}

pub(crate) fn imsa_constraints(widths: ImsaColumnWidths) -> Vec<Constraint> {
    widths
        .to_array()
        .into_iter()
        .map(Constraint::Length)
        .collect()
}
