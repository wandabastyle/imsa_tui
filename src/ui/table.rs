use std::{collections::BTreeMap, collections::HashMap, collections::HashSet, time::Instant};

use ratatui::{
    layout::Constraint,
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Row, Table},
};

use crate::{
    favourites,
    timing::{Series, TimingClassColor, TimingEntry},
};

use super::{
    gap::{
        relative_gap_class_text, relative_gap_next_in_class_text, relative_gap_overall_text,
        GapAnchorInfo,
    },
    imsa_widths::{calculate_imsa_widths, imsa_constraints, ImsaColumnWidths},
    pit::{pit_style_for_entry, PitTracker},
    style::class_style,
};

pub(crate) struct TableRenderCtx<'a> {
    pub(crate) favourites: &'a HashSet<String>,
    pub(crate) marked_stable_id: Option<&'a str>,
    pub(crate) active_series: Series,
    pub(crate) selected_row_in_view: Option<usize>,
    pub(crate) marquee_tick: usize,
    pub(crate) gap_anchor: Option<&'a GapAnchorInfo>,
    pub(crate) pit_trackers: &'a HashMap<String, PitTracker>,
    pub(crate) class_colors: &'a BTreeMap<String, TimingClassColor>,
    pub(crate) now: Instant,
    pub(crate) session_name: &'a str,
}

pub(crate) fn build_table<'a>(
    title: impl Into<String>,
    entries: &'a [TimingEntry],
    ctx: &TableRenderCtx<'_>,
    table_width: u16,
    imsa_baseline: Option<&ImsaColumnWidths>,
) -> Table<'a> {
    let (headers, widths, imsa_widths): (Vec<&str>, Vec<Constraint>, Option<ImsaColumnWidths>) =
        match ctx.active_series {
            Series::Imsa => {
                let imsa_widths = calculate_imsa_widths(table_width, entries, imsa_baseline);
                (
                    vec![
                        "Pos",
                        "#",
                        "Class",
                        "PIC",
                        "Driver",
                        "Vehicle",
                        "Laps",
                        "Gap O",
                        "Gap C",
                        "Next C",
                        "Last",
                        "Best",
                        "BL#",
                        "Pit",
                        "Stop",
                        "Fastest Driver",
                    ],
                    imsa_constraints(imsa_widths),
                    Some(imsa_widths),
                )
            }
            Series::Nls => (
                vec![
                    "Pos", "#", "Class", "PIC", "Driver", "Vehicle", "Team", "Laps", "Gap", "Last",
                    "Best", "S1", "S2", "S3", "S4", "S5",
                ],
                nls_table_widths().to_vec(),
                None,
            ),
            Series::F1 => (
                vec![
                    "Pos", "#", "Driver", "Team", "Laps", "Gap", "Int", "Last", "Best", "Pit",
                    "Stops",
                ],
                f1_table_widths().to_vec(),
                None,
            ),
            Series::Wec => (
                vec![
                    "Pos", "#", "Class", "PIC", "Driver", "Vehicle", "Team", "Laps", "Gap", "Last",
                    "Best", "S1", "S2", "S3",
                ],
                wec_table_widths().to_vec(),
                None,
            ),
        };

    Table::new(build_rows(entries, ctx, imsa_widths), widths)
        .header(Row::new(headers).style(Style::default().add_modifier(Modifier::BOLD)))
        .highlight_style(Style::default().bg(Color::Rgb(45, 45, 45)))
        .block(Block::default().title(title.into()).borders(Borders::ALL))
}

fn build_rows(
    entries: &[TimingEntry],
    ctx: &TableRenderCtx<'_>,
    imsa_widths: Option<ImsaColumnWidths>,
) -> Vec<Row<'static>> {
    entries
        .iter()
        .enumerate()
        .map(|e| {
            let (idx, e) = e;
            let fav_key = favourites::favourite_key(ctx.active_series, &e.stable_id);
            let fav_marker = if ctx.favourites.contains(&fav_key) {
                "★ "
            } else {
                ""
            };
            let selected = ctx.selected_row_in_view == Some(idx);

            let row = match ctx.active_series {
                Series::Imsa => Row::new(vec![
                    Cell::from(e.position.to_string()),
                    Cell::from(format!("{fav_marker}{}", e.car_number)),
                    Cell::from(e.class_name.clone()),
                    Cell::from(e.class_rank.clone()),
                    Cell::from(marquee_if_needed(
                        &e.driver,
                        imsa_widths
                            .map(ImsaColumnWidths::driver_width)
                            .unwrap_or(28),
                        selected,
                        ctx.marquee_tick,
                    )),
                    Cell::from(marquee_if_needed(
                        &e.vehicle,
                        imsa_widths
                            .map(ImsaColumnWidths::vehicle_width)
                            .unwrap_or(45),
                        selected,
                        ctx.marquee_tick,
                    )),
                    Cell::from(e.laps.clone()),
                    Cell::from(relative_gap_overall_text(
                        e,
                        &e.gap_overall,
                        ctx.gap_anchor,
                        ctx.session_name,
                    )),
                    Cell::from(relative_gap_class_text(
                        e,
                        &e.gap_class,
                        ctx.gap_anchor,
                        ctx.session_name,
                    )),
                    Cell::from(relative_gap_next_in_class_text(
                        e,
                        &e.gap_next_in_class,
                        ctx.gap_anchor,
                        ctx.session_name,
                    )),
                    Cell::from(e.last_lap.clone()),
                    Cell::from(e.best_lap.clone()),
                    Cell::from(e.best_lap_no.clone()),
                    Cell::from(e.pit.clone()),
                    Cell::from(e.pit_stops.clone()),
                    Cell::from(marquee_if_needed(
                        &e.fastest_driver,
                        imsa_widths
                            .map(ImsaColumnWidths::fastest_width)
                            .unwrap_or(28),
                        selected,
                        ctx.marquee_tick,
                    )),
                ]),
                Series::Nls => Row::new(vec![
                    Cell::from(e.position.to_string()),
                    Cell::from(format!("{fav_marker}{}", e.car_number)),
                    Cell::from(e.class_name.clone()),
                    Cell::from(e.class_rank.clone()),
                    Cell::from(marquee_if_needed(&e.driver, 18, selected, ctx.marquee_tick)),
                    Cell::from(marquee_if_needed(
                        &e.vehicle,
                        18,
                        selected,
                        ctx.marquee_tick,
                    )),
                    Cell::from(marquee_if_needed(&e.team, 24, selected, ctx.marquee_tick)),
                    Cell::from(e.laps.clone()),
                    Cell::from(relative_gap_overall_text(
                        e,
                        &e.gap_overall,
                        ctx.gap_anchor,
                        ctx.session_name,
                    )),
                    Cell::from(e.last_lap.clone()),
                    Cell::from(e.best_lap.clone()),
                    Cell::from(e.sector_1.clone()),
                    Cell::from(e.sector_2.clone()),
                    Cell::from(e.sector_3.clone()),
                    Cell::from(e.sector_4.clone()),
                    Cell::from(e.sector_5.clone()),
                ]),
                Series::F1 => Row::new(vec![
                    Cell::from(e.position.to_string()),
                    Cell::from(format!("{fav_marker}{}", e.car_number)),
                    Cell::from(marquee_if_needed(&e.driver, 32, selected, ctx.marquee_tick)),
                    Cell::from(marquee_if_needed(&e.team, 22, selected, ctx.marquee_tick)),
                    Cell::from(e.laps.clone()),
                    Cell::from(relative_gap_overall_text(
                        e,
                        &e.gap_overall,
                        ctx.gap_anchor,
                        ctx.session_name,
                    )),
                    Cell::from(relative_gap_class_text(
                        e,
                        &e.gap_class,
                        ctx.gap_anchor,
                        ctx.session_name,
                    )),
                    Cell::from(e.last_lap.clone()),
                    Cell::from(e.best_lap.clone()),
                    Cell::from(e.pit.clone()),
                    Cell::from(e.pit_stops.clone()),
                ]),
                Series::Wec => Row::new(vec![
                    Cell::from(e.position.to_string()),
                    Cell::from(format!("{fav_marker}{}", e.car_number)),
                    Cell::from(e.class_name.clone()),
                    Cell::from(e.class_rank.clone()),
                    Cell::from(marquee_if_needed(&e.driver, 18, selected, ctx.marquee_tick)),
                    Cell::from(marquee_if_needed(
                        &e.vehicle,
                        18,
                        selected,
                        ctx.marquee_tick,
                    )),
                    Cell::from(marquee_if_needed(&e.team, 24, selected, ctx.marquee_tick)),
                    Cell::from(e.laps.clone()),
                    Cell::from(relative_gap_overall_text(
                        e,
                        &e.gap_overall,
                        ctx.gap_anchor,
                        ctx.session_name,
                    )),
                    Cell::from(e.last_lap.clone()),
                    Cell::from(e.best_lap.clone()),
                    Cell::from(e.sector_1.clone()),
                    Cell::from(e.sector_2.clone()),
                    Cell::from(e.sector_3.clone()),
                ]),
            };

            let mut style = class_style(&e.class_name, ctx.active_series, ctx.class_colors);
            if let Some(pit_style) = pit_style_for_entry(ctx.pit_trackers, e, ctx.now) {
                style = style.patch(pit_style);
            }
            if ctx.marked_stable_id == Some(e.stable_id.as_str()) {
                style = style
                    .bg(Color::Rgb(34, 70, 122))
                    .add_modifier(Modifier::BOLD);
            }

            row.style(style)
        })
        .collect()
}

fn marquee_if_needed(text: &str, width_hint: usize, selected: bool, tick: usize) -> String {
    if !selected {
        return text.to_string();
    }

    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= width_hint {
        return text.to_string();
    }

    let gap = 3;
    let cycle_len = chars.len() + gap;
    let offset = tick % cycle_len;

    if offset < chars.len() {
        let mut out = String::new();
        out.extend(chars[offset..].iter());
        out.push_str("   ");
        out.extend(chars[..offset].iter());
        out
    } else {
        let leading_spaces = offset - chars.len();
        let mut out = " ".repeat(leading_spaces);
        out.push_str(text);
        out
    }
}

fn nls_table_widths() -> [Constraint; 16] {
    [
        Constraint::Length(4),
        Constraint::Length(5),
        Constraint::Length(9),
        Constraint::Length(5),
        Constraint::Length(18),
        Constraint::Min(18),
        Constraint::Length(24),
        Constraint::Length(7),
        Constraint::Length(11),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(10),
    ]
}

fn f1_table_widths() -> [Constraint; 11] {
    [
        Constraint::Length(4),
        Constraint::Length(5),
        Constraint::Min(24),
        Constraint::Min(16),
        Constraint::Length(7),
        Constraint::Length(11),
        Constraint::Length(11),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(5),
        Constraint::Length(5),
    ]
}

fn wec_table_widths() -> [Constraint; 14] {
    [
        Constraint::Length(4),
        Constraint::Length(5),
        Constraint::Length(9),
        Constraint::Length(5),
        Constraint::Length(18),
        Constraint::Min(18),
        Constraint::Length(24),
        Constraint::Length(7),
        Constraint::Length(11),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(10),
    ]
}
