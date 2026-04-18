use std::{collections::HashMap, collections::HashSet, collections::VecDeque, time::Instant};

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    style::Modifier,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, TableState, Wrap},
    Frame,
};

use crate::{
    favourites,
    timing::{Series, TimingEntry, TimingHeader, TimingNotice},
};

use super::{
    gap::GapAnchorInfo,
    grouping::{
        display_event_name, display_session_name, favourites_count_for_series, view_mode_text,
        ViewMode,
    },
    imsa_widths::ImsaColumnWidths,
    pit::PitTracker,
    popups::{
        centered_rect, group_picker_popup, help_popup, messages_popup, series_picker_popup,
        GroupPickerState, LogsPanelState, MessagesPanelState, SeriesPickerState,
    },
    search::SearchState,
    style::animated_flag_theme,
    table::{build_table, TableRenderCtx},
};

pub(crate) struct RenderCtx<'a> {
    pub(crate) active_series: Series,
    pub(crate) status: &'a str,
    pub(crate) header: &'a TimingHeader,
    pub(crate) entries: &'a [TimingEntry],
    pub(crate) current_groups: &'a [(String, Vec<TimingEntry>)],
    pub(crate) selected_row: usize,
    pub(crate) favourites: &'a HashSet<String>,
    pub(crate) marked_stable_id: Option<&'a str>,
    pub(crate) marquee_tick: usize,
    pub(crate) gap_anchor: Option<&'a GapAnchorInfo>,
    pub(crate) pit_trackers: &'a HashMap<String, PitTracker>,
    pub(crate) imsa_width_baseline: Option<&'a ImsaColumnWidths>,
    pub(crate) now: Instant,
    pub(crate) view_mode: ViewMode,
    pub(crate) search: &'a SearchState,
    pub(crate) show_help: bool,
    pub(crate) series_picker: SeriesPickerState,
    pub(crate) group_picker: GroupPickerState,
    pub(crate) logs_panel: LogsPanelState,
    pub(crate) messages_panel: MessagesPanelState,
    pub(crate) active_notices: &'a [TimingNotice],
    pub(crate) highlighted_notice_cars: &'a HashSet<String>,
    pub(crate) imsa_debug_logs: &'a VecDeque<String>,
    pub(crate) demo_mode: bool,
    pub(crate) last_error: Option<&'a String>,
    pub(crate) last_update: Option<Instant>,
    pub(crate) effective_flag: &'a str,
    pub(crate) transition_from_flag: &'a str,
    pub(crate) transition_started_at: Instant,
    pub(crate) debug_log_capacity: usize,
}

pub(crate) fn draw_frame(f: &mut Frame<'_>, ctx: &RenderCtx<'_>) {
    let size = f.size();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(4), Constraint::Min(10)])
        .split(size);

    let age = match ctx.last_update {
        Some(t) => format!("Upd {}s", t.elapsed().as_secs()),
        None => "Upd -".to_string(),
    };

    let tte_text = if ctx.header.time_to_go.is_empty() {
        "-"
    } else {
        &ctx.header.time_to_go
    };
    let (flag_text, flag_span_style, header_style) = animated_flag_theme(
        ctx.effective_flag,
        ctx.transition_from_flag,
        ctx.transition_started_at,
    );

    let mode_text = view_mode_text(
        ctx.view_mode,
        &ctx.current_groups
            .iter()
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>(),
    );

    let event_text = display_event_name(
        ctx.active_series,
        if ctx.header.event_name.is_empty() {
            "-"
        } else {
            &ctx.header.event_name
        },
    );
    let session_display = display_session_name(
        ctx.active_series,
        if ctx.header.session_name.is_empty() {
            "-"
        } else {
            &ctx.header.session_name
        },
    );

    let header_lead = format!(
        "{} | {} | {} | TTE {} | Mode {} | ",
        ctx.status, event_text, session_display, tte_text, mode_text,
    );

    let mut header_spans = vec![
        Span::styled(header_lead, header_style),
        Span::styled(flag_text, flag_span_style),
    ];

    if ctx.demo_mode {
        header_spans.push(Span::styled(
            " | DEMO",
            header_style.add_modifier(Modifier::BOLD),
        ));
    }

    header_spans.push(Span::styled(
        format!(
            " | {} | Favs {}",
            age,
            favourites_count_for_series(ctx.active_series, ctx.favourites),
        ),
        header_style,
    ));

    let mut key_hint_spans = vec![Span::styled(
        format!(
            "Keys: h help | m messages ({}) | L logs | d demo | q quit",
            ctx.active_notices.len()
        ),
        header_style,
    )];

    if ctx.search.input_active {
        key_hint_spans.push(Span::styled(
            format!(" | Search: {}_", ctx.search.query),
            header_style.add_modifier(Modifier::BOLD),
        ));
    } else if !ctx.search.query.trim().is_empty() {
        key_hint_spans.push(Span::styled(
            format!(
                " | Search: {} ({}/{})",
                ctx.search.query,
                if ctx.search.matches.is_empty() {
                    0
                } else {
                    ctx.search.current_match + 1
                },
                ctx.search.matches.len(),
            ),
            header_style,
        ));
    }

    if let Some(err) = ctx.last_error {
        key_hint_spans.push(Span::styled(format!(" | Error: {}", err), header_style));
    }

    let status_widget = Paragraph::new(vec![Line::from(header_spans), Line::from(key_hint_spans)])
        .style(header_style)
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .title(format!("{} TUI", ctx.active_series.label()))
                .borders(Borders::ALL)
                .style(header_style),
        );
    f.render_widget(status_widget, chunks[0]);

    if ctx.entries.is_empty() {
        let waiting = Paragraph::new(format!(
            "No timing data yet. Waiting for first successful {} snapshot... Press h for help.",
            ctx.active_series.label(),
        ))
        .block(Block::default().title("Overall").borders(Borders::ALL));
        f.render_widget(waiting, chunks[1]);
    } else {
        match ctx.view_mode {
            ViewMode::Overall => {
                let (visible_entries, start) =
                    visible_slice(ctx.entries, ctx.selected_row, chunks[1].height);
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
                    chunks[1].width,
                    ctx.imsa_width_baseline,
                );
                f.render_stateful_widget(table, chunks[1], &mut state);
            }
            ViewMode::Grouped => {
                if ctx.current_groups.is_empty() {
                    let waiting = Paragraph::new("No grouped class data available yet.")
                        .block(Block::default().title("Grouped").borders(Borders::ALL));
                    f.render_widget(waiting, chunks[1]);
                } else {
                    let mut selected_group_idx = 0usize;
                    let mut running = 0usize;
                    for (idx, (_, class_entries)) in ctx.current_groups.iter().enumerate() {
                        if ctx.selected_row < running + class_entries.len() {
                            selected_group_idx = idx;
                            break;
                        }
                        running += class_entries.len();
                    }

                    let minimum_rows_per_group = 7_u16;
                    let max_visible_groups =
                        (chunks[1].height / minimum_rows_per_group).max(1) as usize;
                    let visible_group_count =
                        ctx.current_groups.len().min(max_visible_groups.max(1));
                    let start_group_idx = if ctx.current_groups.len() <= visible_group_count {
                        0
                    } else {
                        let half = visible_group_count / 2;
                        selected_group_idx
                            .saturating_sub(half)
                            .min(ctx.current_groups.len() - visible_group_count)
                    };
                    let end_group_idx = start_group_idx + visible_group_count;
                    let visible_groups = &ctx.current_groups[start_group_idx..end_group_idx];

                    let constraints: Vec<Constraint> = visible_groups
                        .iter()
                        .map(|_| Constraint::Ratio(1, visible_groups.len() as u32))
                        .collect();
                    let group_chunks = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints(constraints)
                        .split(chunks[1]);

                    let mut global_offset = ctx
                        .current_groups
                        .iter()
                        .take(start_group_idx)
                        .map(|(_, entries)| entries.len())
                        .sum::<usize>();

                    for ((class_name, class_entries), area) in
                        visible_groups.iter().zip(group_chunks.iter())
                    {
                        let local_selected = ctx
                            .selected_row
                            .saturating_sub(global_offset)
                            .min(class_entries.len().saturating_sub(1));
                        let (visible_entries, start) =
                            visible_slice(class_entries, local_selected, area.height);
                        let mut state = TableState::default();
                        let highlight = if ctx.selected_row >= global_offset
                            && ctx.selected_row < global_offset + class_entries.len()
                        {
                            Some(local_selected.saturating_sub(start))
                        } else {
                            None
                        };
                        state.select(highlight);
                        let title = format!("{} ({} cars)", class_name, class_entries.len());
                        let table_ctx = TableRenderCtx {
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
                        };
                        let table = build_table(
                            title,
                            visible_entries,
                            &table_ctx,
                            area.width,
                            ctx.imsa_width_baseline,
                        );
                        f.render_stateful_widget(table, *area, &mut state);
                        global_offset += class_entries.len();
                    }
                }
            }
            ViewMode::Class(idx) => {
                if let Some((class_name, class_entries)) = ctx.current_groups.get(idx) {
                    let (visible_entries, start) =
                        visible_slice(class_entries, ctx.selected_row, chunks[1].height);
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
                        chunks[1].width,
                        ctx.imsa_width_baseline,
                    );
                    f.render_stateful_widget(table, chunks[1], &mut state);
                } else {
                    let waiting = Paragraph::new("No class data available yet.")
                        .block(Block::default().title("Class").borders(Borders::ALL));
                    f.render_widget(waiting, chunks[1]);
                }
            }
            ViewMode::Favourites => {
                let favourite_entries: Vec<TimingEntry> = ctx
                    .entries
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
                    let waiting =
                        Paragraph::new("No favourites yet. Select a car and press space.")
                            .block(Block::default().title("Favourites").borders(Borders::ALL));
                    f.render_widget(waiting, chunks[1]);
                } else {
                    let (visible_entries, start) =
                        visible_slice(&favourite_entries, ctx.selected_row, chunks[1].height);
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
                        chunks[1].width,
                        ctx.imsa_width_baseline,
                    );
                    f.render_stateful_widget(table, chunks[1], &mut state);
                }
            }
        }
    }

    if ctx.show_help {
        let area = centered_rect(40, 40, size);
        f.render_widget(Clear, area);
        f.render_widget(help_popup(), area);
    }

    if ctx.series_picker.is_open {
        let area = centered_rect(35, 35, size);
        f.render_widget(Clear, area);
        f.render_widget(
            series_picker_popup(ctx.active_series, ctx.series_picker.selected_idx),
            area,
        );
    }

    if ctx.group_picker.is_open {
        let area = centered_rect(40, 45, size);
        f.render_widget(Clear, area);
        let group_names: Vec<String> = ctx
            .current_groups
            .iter()
            .map(|(group_name, entries)| format!("{} ({} cars)", group_name, entries.len()))
            .collect();
        f.render_widget(
            group_picker_popup(&group_names, ctx.group_picker.selected_idx),
            area,
        );
    }

    if ctx.logs_panel.is_open {
        let area = centered_rect(65, 60, size);
        f.render_widget(Clear, area);

        let visible_lines = area.height.saturating_sub(3) as usize;
        let total = ctx.imsa_debug_logs.len();
        let max_scroll = total.saturating_sub(1);
        let scroll = ctx.logs_panel.scroll.min(max_scroll);
        let end_exclusive = total.saturating_sub(scroll);
        let start = end_exclusive.saturating_sub(visible_lines);

        let mut lines = vec![];
        if ctx.imsa_debug_logs.is_empty() {
            lines.push(Line::from("No IMSA debug events yet."));
        } else {
            for entry in ctx.imsa_debug_logs.range(start..end_exclusive) {
                lines.push(Line::from(entry.as_str()));
            }
        }
        lines.push(Line::from(""));
        lines.push(Line::from("↑/↓ scroll | c clear | Esc or L close"));

        let title = format!(
            "{} Logs ({total}/{})",
            ctx.active_series.label(),
            ctx.debug_log_capacity
        );

        let logs_popup = Paragraph::new(lines)
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: false })
            .block(Block::default().title(title).borders(Borders::ALL));
        f.render_widget(logs_popup, area);
    }

    if ctx.messages_panel.is_open {
        let area = centered_rect(70, 60, size);
        f.render_widget(Clear, area);
        f.render_widget(
            messages_popup(ctx.active_notices, ctx.messages_panel.selected_idx),
            area,
        );
    }
}

fn visible_slice(
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
