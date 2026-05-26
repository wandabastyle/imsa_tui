use ratatui::{
	style::{Modifier, Style},
	widgets::{Block, Borders, Row, Table},
};

use super::{
	table_builder::{build_rows, build_table_layout},
	table_widths::TableWidthBaselines,
};
use super::table_builder::TableRenderCtx;
use crate::timing::TimingEntry;

pub fn build_table<'a>(
	title: impl Into<String>,
	entries: &'a [TimingEntry],
	ctx: &TableRenderCtx<'_>,
	table_width: u16,
	baselines: TableWidthBaselines<'_>,
) -> Table<'a> {
	let layout = build_table_layout(ctx, table_width, entries, baselines);

	Table::new(
		build_rows(
			entries,
			ctx,
			layout.imsa_widths,
			layout.nls_widths,
			layout.f1_widths,
			layout.wec_widths,
		),
		layout.widths,
	)
	.header(Row::new(layout.headers).style(Style::default().add_modifier(Modifier::BOLD)))
	.row_highlight_style(Style::default().bg(ratatui::style::Color::Rgb(45, 45, 45)))
	.block(Block::default().title(title.into()).borders(Borders::ALL))
}

// Re-exports
pub use super::table_utils::{
	is_highlighted_car_number, marquee_if_needed, normalize_car_number,
};

pub use super::{
	table_builder::TableRenderCtx,
	table_widths::TableWidthBaselines,
};