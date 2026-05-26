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
use crate::{
    favourites,
    timing::{Series, TimingEntry},
};

pub fn render_favourites(
    f: &mut Frame<'_>,
    ctx: &RenderCtx<'_>,
    entries: &[TimingEntry],
    area: Rect,
) {
    let favourite_entries: Vec<TimingEntry> = entries
        .iter()
        .filter(|entry| {
            ctx.favourites.contains(&favourites::favourite_key(
                ctx.active_series,
                &entry.stable_id,
            ))
        })
        .cloned()
        .collect();

    if favourite_entries.is_empty() {
        render_empty(f, area);
    } else {
        render_favourites_table(f, ctx, &favourite_entries, area);
    }
}

fn render_empty(f: &mut Frame<'_>, area: Rect) {
    let waiting = Paragraph::new("No favourites yet. Select a car and press space.")
        .block(Block::default().title("Favourites").borders(Borders::ALL));
    f.render_widget(waiting, area);
}

fn render_favourites_table(
    f: &mut Frame<'_>,
    ctx: &RenderCtx<'_>,
    favourite_entries: &[TimingEntry],
    area: Rect,
) {
    let (visible_entries, start) = visible_slice(favourite_entries, ctx.selected_row, area.height);
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
        format!("Favourites ({} cars)", favourite_entries.len()),
        visible_entries,
        &table_ctx,
        area.width,
        ctx.table_width_baselines,
    );
    f.render_stateful_widget(table, area, &mut state);
}

use super::render_utils::visible_slice as visible_slice_impl;

fn visible_slice(entries: &[TimingEntry], selected_row: usize, height: u16) -> (&[TimingEntry], usize) {
    visible_slice_impl(entries, selected_row, height)
}
