use std::{
   collections::{
      HashMap,
      HashSet,
      VecDeque,
   },
   time::Instant,
};

use ratatui::{
   layout::{
      Alignment,
      Constraint,
      Direction,
      Layout,
   },
   style::Modifier,
   text::{
      Line,
      Span,
   },
   widgets::{
      Block,
      Borders,
      Clear,
      Paragraph,
      Wrap,
   },
   Frame,
};

use super::{
   config::AppConfig,
   gap::GapAnchorInfo,
   grouping::{
      display_event_name,
      display_session_name,
      favourites_count_for_series,
      view_mode_text,
      ViewMode,
   },
   pit::PitTracker,
   popups::{
      centered_rect,
      group_picker_popup,
      help_popup,
      messages_popup,
      nls_liveticker_popup,
      series_picker_popup,
      GroupPickerState,
      LogsPanelState,
      MessagesPanelState,
      NlsLivetickerPanelState,
      SeriesPickerState,
   },
   render_class::render_class,
   render_favourites::render_favourites,
   render_grouped::render_grouped,
   render_overall::{
      render_overall,
      render_waiting,
   },
   render_utils::messages_popup_scroll,
   search::SearchState,
   style::animated_flag_theme,
   table::TableWidthBaselines,
};
use crate::{
   adapters::nls::liveticker::LivetickerEntry,
   timing::{
      Series,
      TimingEntry,
      TimingHeader,
      TimingNotice,
   },
};

pub struct RenderCtx<'a> {
   pub(crate) active_series:              Series,
   pub(crate) status:                     &'a str,
   pub(crate) header:                     &'a TimingHeader,
   pub(crate) entries:                    &'a [TimingEntry],
   pub(crate) current_groups:             &'a [(String, Vec<TimingEntry>)],
   pub(crate) selected_row:               usize,
   pub(crate) favourites:                 &'a HashSet<String>,
   pub(crate) marked_stable_id:           Option<&'a str>,
   pub(crate) marquee_tick:               usize,
   pub(crate) gap_anchor:                 Option<&'a GapAnchorInfo>,
   pub(crate) pit_trackers:               &'a HashMap<String, PitTracker>,
   pub(crate) table_width_baselines:      TableWidthBaselines<'a>,
   pub(crate) now:                        Instant,
   pub(crate) view_mode:                  ViewMode,
   pub(crate) search:                     &'a SearchState,
   pub(crate) show_help:                  bool,
   pub(crate) series_picker:              SeriesPickerState,
   pub(crate) group_picker:               GroupPickerState,
   pub(crate) logs_panel:                 LogsPanelState,
   pub(crate) messages_panel:             MessagesPanelState,
   pub(crate) nls_liveticker_panel:       NlsLivetickerPanelState,
   pub(crate) active_notices:             &'a [TimingNotice],
   pub(crate) nls_liveticker_entries:     &'a [LivetickerEntry],
   pub(crate) nls_liveticker_last_update: Option<Instant>,
   pub(crate) nls_liveticker_last_error:  Option<&'a str>,
   pub(crate) highlighted_notice_cars:    &'a HashSet<String>,
   pub(crate) imsa_debug_logs:            &'a VecDeque<String>,
   pub(crate) demo_mode:                  bool,
   pub(crate) last_error:                 Option<&'a String>,
   pub(crate) last_update:                Option<Instant>,
   pub(crate) effective_flag:             &'a str,
   pub(crate) transition_from_flag:       &'a str,
   pub(crate) transition_started_at:      Instant,
   pub(crate) debug_log_capacity:         usize,
   pub(crate) config:                     &'a AppConfig,
}

pub fn draw_frame(f: &mut Frame<'_>, ctx: &RenderCtx<'_>) {
   let size = f.area();
   let chunks = Layout::default()
      .direction(Direction::Vertical)
      .constraints([Constraint::Length(4), Constraint::Min(10)])
      .split(size);

   render_header(f, ctx, chunks[0]);

   if ctx.entries.is_empty() {
      render_waiting(f, ctx, chunks[1]);
   } else {
      match ctx.view_mode {
         ViewMode::Overall => render_overall(f, ctx, ctx.entries, chunks[1]),
         ViewMode::Grouped => render_grouped(f, ctx, ctx.current_groups, chunks[1]),
         ViewMode::Class(idx) => render_class(f, ctx, idx, ctx.current_groups, chunks[1]),
         ViewMode::Favourites => render_favourites(f, ctx, ctx.entries, chunks[1]),
      }
   }

   render_popups(f, ctx, size);
}

fn render_header(f: &mut Frame<'_>, ctx: &RenderCtx<'_>, area: ratatui::layout::Rect) {
   let age = ctx.last_update.map_or_else(
      || "Upd -".to_string(),
      |t| format!("Upd {}s", t.elapsed().as_secs()),
   );

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
      &ctx
         .current_groups
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
      format!("Keys: h help | m messages ({})", ctx.active_notices.len()),
      header_style,
   )];

   if ctx.active_series == Series::Nls {
      key_hint_spans.push(Span::styled(
         format!(" | l ticker ({})", ctx.nls_liveticker_entries.len()),
         header_style,
      ));
   }

   key_hint_spans.push(Span::styled(" | L logs | d demo | q quit", header_style));

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
      key_hint_spans.push(Span::styled(format!(" | Error: {err}"), header_style));
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
   f.render_widget(status_widget, area);
}

fn render_popups(f: &mut Frame<'_>, ctx: &RenderCtx<'_>, size: ratatui::layout::Rect) {
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
      render_logs_popup(f, ctx, size);
   }

   if ctx.messages_panel.is_open {
      render_messages_popup(f, ctx, size);
   }

   if ctx.nls_liveticker_panel.is_open {
      render_liveticker_popup(f, ctx, size);
   }
}

fn render_logs_popup(f: &mut Frame<'_>, ctx: &RenderCtx<'_>, size: ratatui::layout::Rect) {
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

fn render_messages_popup(f: &mut Frame<'_>, ctx: &RenderCtx<'_>, size: ratatui::layout::Rect) {
   let area = centered_rect(70, 60, size);
   f.render_widget(Clear, area);
   let scroll = messages_popup_scroll(
      ctx.active_notices.len(),
      ctx.messages_panel.selected_idx,
      area,
   );
   f.render_widget(
      messages_popup(ctx.active_notices, ctx.messages_panel.selected_idx, scroll),
      area,
   );
}

fn render_liveticker_popup(f: &mut Frame<'_>, ctx: &RenderCtx<'_>, size: ratatui::layout::Rect) {
   let area = centered_rect(78, 72, size);
   f.render_widget(Clear, area);
   let age_secs = ctx
      .nls_liveticker_last_update
      .map(|updated_at| updated_at.elapsed().as_secs());
   f.render_widget(
      nls_liveticker_popup(
         ctx.nls_liveticker_entries,
         ctx.nls_liveticker_panel.scroll,
         age_secs,
         ctx.nls_liveticker_last_error,
      ),
      area,
   );
}
