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
    series_widths::{
        calculate_f1_widths, calculate_nls_widths, f1_constraints, nls_constraints, F1ColumnWidths,
        NlsColumnWidths,
    },
    style::class_style,
    wec_widths::{calculate_wec_widths, wec_constraints, WecColumnWidths},
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
    pub(crate) session_type_raw: &'a str,
    pub(crate) session_name: &'a str,
    pub(crate) highlighted_cars: &'a HashSet<String>,
}

type TableLayout = (
    Vec<&'static str>,
    Vec<Constraint>,
    Option<ImsaColumnWidths>,
    Option<NlsColumnWidths>,
    Option<F1ColumnWidths>,
    Option<WecColumnWidths>,
);

#[derive(Clone, Copy, Default)]
pub(crate) struct TableWidthBaselines<'a> {
    pub(crate) imsa: Option<&'a ImsaColumnWidths>,
    pub(crate) nls: Option<&'a NlsColumnWidths>,
    pub(crate) f1: Option<&'a F1ColumnWidths>,
    pub(crate) wec: Option<&'a WecColumnWidths>,
}

pub(crate) fn build_table<'a>(
    title: impl Into<String>,
    entries: &'a [TimingEntry],
    ctx: &TableRenderCtx<'_>,
    table_width: u16,
    baselines: TableWidthBaselines<'_>,
) -> Table<'a> {
    let (headers, widths, imsa_widths, nls_widths, f1_widths, wec_widths): TableLayout =
        match ctx.active_series {
            Series::Imsa => {
                let imsa_widths = calculate_imsa_widths(table_width, entries, baselines.imsa);
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
                    None,
                    None,
                    None,
                )
            }
            Series::Nls | Series::Dhlm => {
                let nls_widths = calculate_nls_widths(table_width, entries, baselines.nls);
                (
                    vec![
                        "Pos", "#", "Class", "PIC", "Driver", "Vehicle", "Team", "Laps", "Gap",
                        "Last", "Best", "S1", "S2", "S3", "S4", "S5",
                    ],
                    nls_constraints(nls_widths),
                    None,
                    Some(nls_widths),
                    None,
                    None,
                )
            }
            Series::F1 => {
                let f1_widths = calculate_f1_widths(table_width, entries, baselines.f1);
                (
                    vec![
                        "Pos", "#", "Driver", "Team", "Laps", "Gap", "Int", "Last", "Best", "Pit",
                        "Stops",
                    ],
                    f1_constraints(f1_widths),
                    None,
                    None,
                    Some(f1_widths),
                    None,
                )
            }
            Series::Wec => {
                let wec_widths = calculate_wec_widths(table_width, entries, baselines.wec);
                (
                    vec![
                        "Pos", "#", "Class", "PIC", "Driver", "Vehicle", "Team", "Laps", "Gap",
                        "Last", "Best", "S1", "S2", "S3",
                    ],
                    wec_constraints(wec_widths),
                    None,
                    None,
                    None,
                    Some(wec_widths),
                )
            }
        };

    Table::new(
        build_rows(entries, ctx, imsa_widths, nls_widths, f1_widths, wec_widths),
        widths,
    )
    .header(Row::new(headers).style(Style::default().add_modifier(Modifier::BOLD)))
    .row_highlight_style(Style::default().bg(Color::Rgb(45, 45, 45)))
    .block(Block::default().title(title.into()).borders(Borders::ALL))
}

fn build_rows(
    entries: &[TimingEntry],
    ctx: &TableRenderCtx<'_>,
    imsa_widths: Option<ImsaColumnWidths>,
    nls_widths: Option<NlsColumnWidths>,
    f1_widths: Option<F1ColumnWidths>,
    wec_widths: Option<WecColumnWidths>,
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
            let highlighted_car = is_highlighted_car_number(&e.car_number, ctx.highlighted_cars);
            let car_cell_style = if highlighted_car {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Rgb(255, 221, 0))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            let car_cell =
                Cell::from(format!("{fav_marker}{}", e.car_number)).style(car_cell_style);

            let row = match ctx.active_series {
                Series::Imsa => Row::new(vec![
                    Cell::from(e.position.to_string()),
                    car_cell,
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
                        ctx.session_type_raw,
                        ctx.session_name,
                    )),
                    Cell::from(relative_gap_class_text(
                        e,
                        &e.gap_class,
                        ctx.gap_anchor,
                        ctx.session_type_raw,
                        ctx.session_name,
                    )),
                    Cell::from(relative_gap_next_in_class_text(
                        e,
                        &e.gap_next_in_class,
                        ctx.gap_anchor,
                        ctx.session_type_raw,
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
                Series::Nls | Series::Dhlm => Row::new(vec![
                    Cell::from(e.position.to_string()),
                    car_cell,
                    Cell::from(e.class_name.clone()),
                    Cell::from(e.class_rank.clone()),
                    Cell::from(marquee_if_needed(
                        &e.driver,
                        nls_widths.map(NlsColumnWidths::driver_width).unwrap_or(18),
                        selected,
                        ctx.marquee_tick,
                    )),
                    Cell::from(marquee_if_needed(
                        &e.vehicle,
                        nls_widths.map(NlsColumnWidths::vehicle_width).unwrap_or(18),
                        selected,
                        ctx.marquee_tick,
                    )),
                    Cell::from(marquee_if_needed(
                        &e.team,
                        nls_widths.map(NlsColumnWidths::team_width).unwrap_or(24),
                        selected,
                        ctx.marquee_tick,
                    )),
                    Cell::from(e.laps.clone()),
                    Cell::from(relative_gap_overall_text(
                        e,
                        &e.gap_overall,
                        ctx.gap_anchor,
                        ctx.session_type_raw,
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
                    car_cell,
                    Cell::from(marquee_if_needed(
                        &e.driver,
                        f1_widths.map(F1ColumnWidths::driver_width).unwrap_or(32),
                        selected,
                        ctx.marquee_tick,
                    )),
                    Cell::from(marquee_if_needed(
                        &e.team,
                        f1_widths.map(F1ColumnWidths::team_width).unwrap_or(22),
                        selected,
                        ctx.marquee_tick,
                    )),
                    Cell::from(e.laps.clone()),
                    Cell::from(relative_gap_overall_text(
                        e,
                        &e.gap_overall,
                        ctx.gap_anchor,
                        ctx.session_type_raw,
                        ctx.session_name,
                    )),
                    Cell::from(relative_gap_class_text(
                        e,
                        &e.gap_class,
                        ctx.gap_anchor,
                        ctx.session_type_raw,
                        ctx.session_name,
                    )),
                    Cell::from(e.last_lap.clone()),
                    Cell::from(e.best_lap.clone()),
                    Cell::from(e.pit.clone()),
                    Cell::from(e.pit_stops.clone()),
                ]),
                Series::Wec => Row::new(vec![
                    Cell::from(e.position.to_string()),
                    car_cell,
                    Cell::from(e.class_name.clone()),
                    Cell::from(e.class_rank.clone()),
                    Cell::from(marquee_if_needed(
                        &e.driver,
                        wec_widths.map(WecColumnWidths::driver_width).unwrap_or(18),
                        selected,
                        ctx.marquee_tick,
                    )),
                    Cell::from(marquee_if_needed(
                        &e.vehicle,
                        wec_widths.map(WecColumnWidths::vehicle_width).unwrap_or(18),
                        selected,
                        ctx.marquee_tick,
                    )),
                    Cell::from(marquee_if_needed(
                        &e.team,
                        wec_widths.map(WecColumnWidths::team_width).unwrap_or(24),
                        selected,
                        ctx.marquee_tick,
                    )),
                    Cell::from(e.laps.clone()),
                    Cell::from(relative_gap_overall_text(
                        e,
                        &e.gap_overall,
                        ctx.gap_anchor,
                        ctx.session_type_raw,
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

fn normalize_car_number(value: &str) -> &str {
    let trimmed = value.trim();
    let without_zeroes = trimmed.trim_start_matches('0');
    if without_zeroes.is_empty() {
        trimmed
    } else {
        without_zeroes
    }
}

fn is_highlighted_car_number(car_number: &str, highlighted: &HashSet<String>) -> bool {
    let trimmed = car_number.trim();
    highlighted.contains(trimmed) || highlighted.contains(normalize_car_number(trimmed))
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
