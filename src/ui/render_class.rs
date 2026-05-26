use ratatui::{
    layout::Rect,
    widgets::{Block, Borders, Paragraph, TableState},
    Frame,
};

use crate::timing::TimingEntry;
use crate::ui::{
    render::RenderCtx,
    render_utils::visible_slice,
    table::{build_table, TableRenderCtx},
};

pub fn render_class(
    f: &mut Frame<'_>,
    ctx: &RenderCtx<'_>,
    idx: usize,
    current_groups: &[(String, Vec<TimingEntry>)],
    area: Rect,
) {
    if let Some((class_name, class_entries)) = current_groups.get(idx) {
        render_class_table(f, ctx, class_name, class_entries, area);
    } else {
        render_empty(f, area);
    }
}

fn render_class_table(
    f: &mut Frame<'_>,
    ctx: &RenderCtx<'_>,
    class_name: &str,
    class_entries: &[TimingEntry],
    area: Rect,
) {
    let (visible_entries, start) = visible_slice(class_entries, ctx.selected_row, area.height);
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
        format!("{} ({} cars)", class_name, class_entries.len()),
        visible_entries,
        &table_ctx,
        area.width,
        ctx.table_width_baselines,
    );
    f.render_stateful_widget(table, area, &mut state);
}

fn render_empty(f: &mut Frame<'_>, area: Rect) {
    let waiting = Paragraph::new("No class data available yet.")
        .block(Block::default().title("Class").borders(Borders::ALL));
    f.render_widget(waiting, area);
}
