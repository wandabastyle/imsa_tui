use std::{
	collections::{
		BTreeMap,
		HashMap,
		HashSet,
	},
	time::Instant,
};

use ratatui::{
	layout::Constraint,
	style::{Color, Modifier, Style},
	widgets::{Cell, Row},
};

use super::{
	gap::{
		relative_gap_class_text,
		relative_gap_next_in_class_text,
		relative_gap_overall_text,
		GapAnchorInfo,
	},
	imsa_widths::{
		calculate_imsa_widths, imsa_constraints, ImsaColumnWidths,
	},
	pit::{pit_style_for_entry, PitTracker},
	series_widths::{
		calculate_f1_widths,
		calculate_nls_widths,
		f1_constraints,
		nls_constraints,
		F1ColumnWidths,
		NlsColumnWidths,
	},
	style::class_style,
	table_utils::marquee_if_needed,
	table_widths::TableWidthBaselines,
	wec_widths::{
		calculate_wec_widths, wec_constraints, WecColumnWidths,
	},
};
use crate::{
	favourites,
	timing::{Series, TimingClassColor, TimingEntry},
};

pub struct TableLayout {
	pub headers: Vec<&'static str>,
	pub widths: Vec<Constraint>,
	pub imsa_widths: Option<ImsaColumnWidths>,
	pub nls_widths: Option<NlsColumnWidths>,
	pub f1_widths: Option<F1ColumnWidths>,
	pub wec_widths: Option<WecColumnWidths>,
}

pub struct TableRenderCtx<'a> {
	pub favourites: &'a HashSet<String>,
	pub marked_stable_id: Option<&'a str>,
	pub active_series: Series,
	pub selected_row_in_view: Option<usize>,
	pub marquee_tick: usize,
	pub gap_anchor: Option<&'a GapAnchorInfo>,
	pub pit_trackers: &'a HashMap<String, PitTracker>,
	pub class_colors: &'a BTreeMap<String, TimingClassColor>,
	pub now: Instant,
	pub session_type_raw: &'a str,
	pub session_name: &'a str,
	pub highlighted_cars: &'a HashSet<String>,
}

pub fn build_table_layout(
	ctx: &TableRenderCtx<'_>,
	table_width: u16,
	entries: &[TimingEntry],
	baselines: TableWidthBaselines<'_>,
) -> TableLayout {
	match ctx.active_series {
		Series::Imsa => build_imsa_layout(table_width, entries, baselines),
		Series::Nls | Series::Dhlm => build_nls_layout(table_width, entries, baselines),
		Series::F1 => build_f1_layout(table_width, entries, baselines),
		Series::Wec => build_wec_layout(table_width, entries, baselines),
	}
}

fn build_imsa_layout(
	table_width: u16,
	entries: &[TimingEntry],
	baselines: TableWidthBaselines<'_>,
) -> TableLayout {
	let imsa_widths = calculate_imsa_widths(table_width, entries, baselines.imsa);
	TableLayout {
		headers: vec![
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
		widths: imsa_constraints(imsa_widths),
		imsa_widths: Some(imsa_widths),
		nls_widths: None,
		f1_widths: None,
		wec_widths: None,
	}
}

fn build_nls_layout(
	table_width: u16,
	entries: &[TimingEntry],
	baselines: TableWidthBaselines<'_>,
) -> TableLayout {
	let nls_widths = calculate_nls_widths(table_width, entries, baselines.nls);
	TableLayout {
		headers: vec![
			"Pos", "#", "Class", "PIC", "Driver", "Vehicle", "Team", "Laps", "Gap", "Last",
			"Best", "S1", "S2", "S3", "S4", "S5",
		],
		widths: nls_constraints(nls_widths),
		imsa_widths: None,
		nls_widths: Some(nls_widths),
		f1_widths: None,
		wec_widths: None,
	}
}

fn build_f1_layout(
	table_width: u16,
	entries: &[TimingEntry],
	baselines: TableWidthBaselines<'_>,
) -> TableLayout {
	let f1_widths = calculate_f1_widths(table_width, entries, baselines.f1);
	TableLayout {
		headers: vec![
			"Pos", "#", "Driver", "Team", "Laps", "Gap", "Int", "Last", "Best", "Pit", "Stops",
		],
		widths: f1_constraints(f1_widths),
		imsa_widths: None,
		nls_widths: None,
		f1_widths: Some(f1_widths),
		wec_widths: None,
	}
}

fn build_wec_layout(
	table_width: u16,
	entries: &[TimingEntry],
	baselines: TableWidthBaselines<'_>,
) -> TableLayout {
	let wec_widths = calculate_wec_widths(table_width, entries, baselines.wec);
	TableLayout {
		headers: vec![
			"Pos", "#", "Class", "PIC", "Driver", "Vehicle", "Team", "Laps", "Gap", "Last",
			"Best", "S1", "S2", "S3",
		],
		widths: wec_constraints(wec_widths),
		imsa_widths: None,
		nls_widths: None,
		f1_widths: None,
		wec_widths: Some(wec_widths),
	}
}

pub fn build_rows(
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
		.map(|(idx, e)| build_single_row(e, idx, ctx, imsa_widths, nls_widths, f1_widths, wec_widths))
		.collect()
}

fn build_single_row(
	e: &TimingEntry,
	idx: usize,
	ctx: &TableRenderCtx<'_>,
	imsa_widths: Option<ImsaColumnWidths>,
	nls_widths: Option<NlsColumnWidths>,
	f1_widths: Option<F1ColumnWidths>,
	wec_widths: Option<WecColumnWidths>,
) -> Row<'static> {
	let fav_key = favourites::favourite_key(ctx.active_series, &e.stable_id);
	let fav_marker = if ctx.favourites.contains(&fav_key) { "★ " } else { "" };
	let selected = ctx.selected_row_in_view == Some(idx);
	let car_cell = build_car_cell(e, fav_marker, ctx.highlighted_cars);

	let row = match ctx.active_series {
		Series::Imsa => build_imsa_row(e, car_cell, selected, ctx, imsa_widths),
		Series::Nls | Series::Dhlm => build_nls_row(e, car_cell, selected, ctx, nls_widths),
		Series::F1 => build_f1_row(e, car_cell, selected, ctx, f1_widths),
		Series::Wec => build_wec_row(e, car_cell, selected, ctx, wec_widths),
	};

	let style = build_row_style(e, ctx);
	row.style(style)
}

fn build_car_cell(
	e: &TimingEntry,
	fav_marker: &str,
	highlighted_cars: &HashSet<String>,
) -> Cell<'static> {
	let highlighted_car =
		super::table_utils::is_highlighted_car_number(&e.car_number, highlighted_cars);
	let car_cell_style = if highlighted_car {
		Style::default()
			.fg(Color::Black)
			.bg(Color::Rgb(255, 221, 0))
			.add_modifier(Modifier::BOLD)
	} else {
		Style::default()
	};
	Cell::from(format!("{fav_marker}{}", e.car_number)).style(car_cell_style)
}

fn build_imsa_row(
	e: &TimingEntry,
	car_cell: Cell<'static>,
	selected: bool,
	ctx: &TableRenderCtx<'_>,
	imsa_widths: Option<ImsaColumnWidths>,
) -> Row<'static> {
	Row::new(vec![
		Cell::from(e.position.to_string()),
		car_cell,
		Cell::from(e.class_name.clone()),
		Cell::from(e.class_rank.clone()),
		Cell::from(marquee_if_needed(
			&e.driver,
			imsa_widths.map_or(28, ImsaColumnWidths::driver_width),
			selected,
			ctx.marquee_tick,
		)),
		Cell::from(marquee_if_needed(
			&e.vehicle,
			imsa_widths.map_or(45, ImsaColumnWidths::vehicle_width),
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
			imsa_widths.map_or(28, ImsaColumnWidths::fastest_width),
			selected,
			ctx.marquee_tick,
		)),
	])
}

fn build_nls_row(
	e: &TimingEntry,
	car_cell: Cell<'static>,
	selected: bool,
	ctx: &TableRenderCtx<'_>,
	nls_widths: Option<NlsColumnWidths>,
) -> Row<'static> {
	Row::new(vec![
		Cell::from(e.position.to_string()),
		car_cell,
		Cell::from(e.class_name.clone()),
		Cell::from(e.class_rank.clone()),
		Cell::from(marquee_if_needed(
			&e.driver,
			nls_widths.map_or(18, NlsColumnWidths::driver_width),
			selected,
			ctx.marquee_tick,
		)),
		Cell::from(marquee_if_needed(
			&e.vehicle,
			nls_widths.map_or(18, NlsColumnWidths::vehicle_width),
			selected,
			ctx.marquee_tick,
		)),
		Cell::from(marquee_if_needed(
			&e.team,
			nls_widths.map_or(24, NlsColumnWidths::team_width),
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
	])
}

fn build_f1_row(
	e: &TimingEntry,
	car_cell: Cell<'static>,
	selected: bool,
	ctx: &TableRenderCtx<'_>,
	f1_widths: Option<F1ColumnWidths>,
) -> Row<'static> {
	Row::new(vec![
		Cell::from(e.position.to_string()),
		car_cell,
		Cell::from(marquee_if_needed(
			&e.driver,
			f1_widths.map_or(32, F1ColumnWidths::driver_width),
			selected,
			ctx.marquee_tick,
		)),
		Cell::from(marquee_if_needed(
			&e.team,
			f1_widths.map_or(22, F1ColumnWidths::team_width),
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
	])
}

fn build_wec_row(
	e: &TimingEntry,
	car_cell: Cell<'static>,
	selected: bool,
	ctx: &TableRenderCtx<'_>,
	wec_widths: Option<WecColumnWidths>,
) -> Row<'static> {
	Row::new(vec![
		Cell::from(e.position.to_string()),
		car_cell,
		Cell::from(e.class_name.clone()),
		Cell::from(e.class_rank.clone()),
		Cell::from(marquee_if_needed(
			&e.driver,
			wec_widths.map_or(18, WecColumnWidths::driver_width),
			selected,
			ctx.marquee_tick,
		)),
		Cell::from(marquee_if_needed(
			&e.vehicle,
			wec_widths.map_or(18, WecColumnWidths::vehicle_width),
			selected,
			ctx.marquee_tick,
		)),
		Cell::from(marquee_if_needed(
			&e.team,
			wec_widths.map_or(24, WecColumnWidths::team_width),
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
	])
}

fn build_row_style(e: &TimingEntry, ctx: &TableRenderCtx<'_>) -> Style {
	let mut style = class_style(&e.class_name, ctx.active_series, ctx.class_colors);
	if let Some(pit_style) = pit_style_for_entry(ctx.pit_trackers, e, ctx.now) {
		style = style.patch(pit_style);
	}
	if ctx.marked_stable_id == Some(e.stable_id.as_str()) {
		style = style
			.bg(Color::Rgb(34, 70, 122))
			.add_modifier(Modifier::BOLD);
	}
	style
}
