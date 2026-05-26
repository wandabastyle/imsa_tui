use ratatui::{
    layout::Rect,
    widgets::{Block, Borders, Paragraph, TableState},
    Frame,
};

use super::{
    render_utils::visible_slice,
    table::{build_table, TableRenderCtx, TableWidthBaselines},
    RenderCtx,
};
use crate::timing::TimingEntry;

pub fn render_overall(
    f: &mut Frame<'_>,
    ctx: &RenderCtx<'_>,
    entries: &[TimingEntry],
    area: Rect,
) {
    let (visible_entries, start) = visible_slice(entries, ctx.selected_row, area.height);
    let local_selected = ctx.selected_row.saturating_sub(start);
    let mut state = TableState::default();
    state.select(Some(local_selected));

    let table_ctx = TableRenderCtx {
        favourites: ctx.favourites,
        marked_stable_id: ctx.marked_stable_id,
        active_series: ctx.active_series,
        selected_row_in_view: Some(local_selected),
        marquee_tick: ctx.marquee_tick,
        gap_anchor: ctx.gap_anchor,
        pit_trackers: ctx.pit_trackers,
        class_colors: &ctx.header.class_colors,
        now: ctx.now,
        session_type_raw: &ctx.header.session_type_raw,
        session_name: &ctx.header.session_name,
        highlighted_cars: ctx.highlighted_notice_cars,
    };

    let table = build_table(
        "Overall",
        visible_entries,
        &table_ctx,
        area.width,
        ctx.table_width_baselines,
    );
    f.render_stateful_widget(table, area, &mut state);
}

pub fn render_waiting(f: &mut Frame<'_>, ctx: &RenderCtx<'_>, area: Rect) {
    let waiting = Paragraph::new(format!(
        "No timing data yet. Waiting for first successful {} snapshot... Press h for help.",
        ctx.active_series.label(),
    ))
    .block(Block::default().title("Overall").borders(Borders::ALL));
    f.render_widget(waiting, area);
}
