use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    widgets::{Block, Borders, Paragraph, TableState},
    Frame,
};

use super::{
    render_utils::visible_slice,
    table::{build_table, TableRenderCtx, TableWidthBaselines},
    RenderCtx,
};
use crate::timing::TimingEntry;

pub fn render_grouped(
    f: &mut Frame<'_>,
    ctx: &RenderCtx<'_>,
    current_groups: &[(String, Vec<TimingEntry>)],
    area: Rect,
) {
    if current_groups.is_empty() {
        render_empty(f, area);
        return;
    }

    let selected_group_idx = calculate_selected_group_idx(ctx.selected_row, current_groups);
    let minimum_rows_per_group = ctx.config.grouped_min_rows.max(3);
    let max_visible_groups = (area.height / minimum_rows_per_group).max(1) as usize;
    let visible_group_count = current_groups.len().min(max_visible_groups.max(1));

    let start_group_idx = calculate_start_group_idx(
        selected_group_idx,
        current_groups.len(),
        visible_group_count,
    );
    let end_group_idx = start_group_idx + visible_group_count;
    let visible_groups = &current_groups[start_group_idx..end_group_idx];

    let group_chunks = create_group_layout(visible_groups, area, minimum_rows_per_group);
    let mut global_offset = calculate_global_offset(current_groups, start_group_idx);

    render_visible_groups(f, ctx, visible_groups, &group_chunks, &mut global_offset);
}

fn render_empty(f: &mut Frame<'_>, area: Rect) {
    let waiting = Paragraph::new("No grouped class data available yet.")
        .block(Block::default().title("Grouped").borders(Borders::ALL));
    f.render_widget(waiting, area);
}

fn calculate_selected_group_idx(
    selected_row: usize,
    current_groups: &[(String, Vec<TimingEntry>)],
) -> usize {
    let mut running = 0usize;
    for (idx, (_, class_entries)) in current_groups.iter().enumerate() {
        if selected_row < running + class_entries.len() {
            return idx;
        }
        running += class_entries.len();
    }
    0
}

fn calculate_start_group_idx(
    selected_group_idx: usize,
    total_groups: usize,
    visible_group_count: usize,
) -> usize {
    if total_groups <= visible_group_count {
        return 0;
    }
    let half = visible_group_count / 2;
    selected_group_idx
        .saturating_sub(half)
        .min(total_groups - visible_group_count)
}

fn create_group_layout(
    visible_groups: &[(String, Vec<TimingEntry>)],
    area: Rect,
    minimum_rows_per_group: u16,
) -> Vec<Rect> {
    let total_cars: usize = visible_groups
        .iter()
        .map(|(_, entries)| entries.len())
        .sum();

    let constraints: Vec<Constraint> = visible_groups
        .iter()
        .map(|(_, entries)| {
            let ratio = if total_cars > 0 {
                entries.len() as f64 / total_cars as f64
            } else {
                1.0 / visible_groups.len() as f64
            };
            let min_rows = minimum_rows_per_group.clamp(3, entries.len() as u16);
            let target_rows = (ratio * area.height as f64).round() as u16;
            Constraint::Length(target_rows.clamp(min_rows, area.height))
        })
        .collect();

    Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area)
}

fn calculate_global_offset(
    current_groups: &[(String, Vec<TimingEntry>)],
    start_group_idx: usize,
) -> usize {
    current_groups
        .iter()
        .take(start_group_idx)
        .map(|(_, entries)| entries.len())
        .sum::<usize>()
}

fn render_visible_groups(
    f: &mut Frame<'_>,
    ctx: &RenderCtx<'_>,
    visible_groups: &[(String, Vec<TimingEntry>)],
    group_chunks: &[Rect],
    global_offset: &mut usize,
) {
    for ((class_name, class_entries), area) in visible_groups.iter().zip(group_chunks.iter()) {
        let local_selected = ctx
            .selected_row
            .saturating_sub(*global_offset)
            .min(class_entries.len().saturating_sub(1));
        let (visible_entries, start) = visible_slice(class_entries, local_selected, area.height);

        let highlight = calculate_highlight(ctx.selected_row, *global_offset, class_entries.len(), local_selected, start);
        let mut state = TableState::default();
        state.select(highlight);

        let title = format!("{} ({} cars)", class_name, class_entries.len());
        let table_ctx = create_table_ctx(ctx, highlight);

        let table = build_table(
            title,
            visible_entries,
            &table_ctx,
            area.width,
            ctx.table_width_baselines,
        );
        f.render_stateful_widget(table, *area, &mut state);

        *global_offset += class_entries.len();
    }
}

fn calculate_highlight(
    selected_row: usize,
    global_offset: usize,
    class_entries_len: usize,
    local_selected: usize,
    start: usize,
) -> Option<usize> {
    if selected_row >= global_offset && selected_row < global_offset + class_entries_len {
        Some(local_selected.saturating_sub(start))
    } else {
        None
    }
}

fn create_table_ctx<'a>(ctx: &'a RenderCtx<'_>, highlight: Option<usize>) -> TableRenderCtx<'a> {
    TableRenderCtx {
        favourites: ctx.favourites,
        marked_stable_id: ctx.marked_stable_id,
        active_series: ctx.active_series,
        selected_row_in_view: highlight,
        marquee_tick: ctx.marquee_tick,
        gap_anchor: ctx.gap_anchor,
        pit_trackers: ctx.pit_trackers,
        class_colors: &ctx.header.class_colors,
        now: ctx.now,
        session_type_raw: &ctx.header.session_type_raw,
        session_name: &ctx.header.session_name,
        highlighted_cars: ctx.highlighted_notice_cars,
    }
}

use super::render_utils::visible_slice as visible_slice_impl;

fn visible_slice(entries: &[TimingEntry], local_selected: usize, height: u16) -> (&[TimingEntry], usize) {
    visible_slice_impl(entries, local_selected, height)
}
