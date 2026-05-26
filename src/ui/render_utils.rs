use ratatui::widgets::TableState;

use crate::timing::TimingEntry;

pub fn visible_slice(
    entries: &[TimingEntry],
    selected_idx: usize,
    table_area_height: u16,
) -> (&[TimingEntry], usize) {
    let visible_rows = table_area_height.saturating_sub(3) as usize;
    let window = visible_rows.max(1);
    if entries.is_empty() {
        return (&entries[0..0], 0);
    }

    let max_start = entries.len().saturating_sub(window);
    let start = selected_idx
        .saturating_sub(window.saturating_sub(1))
        .min(max_start);
    let end = (start + window).min(entries.len());
    (&entries[start..end], start)
}

pub fn messages_popup_scroll(
    notice_count: usize,
    selected_idx: usize,
    area: ratatui::layout::Rect,
) -> usize {
    if notice_count == 0 {
        return 0;
    }

    let inner_height = area.height.saturating_sub(2) as usize;
    if inner_height == 0 {
        return 0;
    }

    const HEADER_LINES: usize = 2;
    const FOOTER_LINES: usize = 3;

    let notice_window = inner_height
        .saturating_sub(HEADER_LINES + FOOTER_LINES)
        .max(1);
    let first_visible_notice = selected_idx.saturating_sub(notice_window.saturating_sub(1));
    let desired_scroll = HEADER_LINES + first_visible_notice;

    let total_lines = HEADER_LINES + notice_count + FOOTER_LINES;
    let max_scroll = total_lines.saturating_sub(inner_height);

    desired_scroll.min(max_scroll)
}

pub fn create_table_state(local_selected: usize) -> TableState {
    let mut state = TableState::default();
    state.select(Some(local_selected));
    state
}
