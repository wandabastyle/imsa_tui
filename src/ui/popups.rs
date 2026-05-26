use ratatui::{
   layout::{
      Alignment,
      Constraint,
      Direction,
      Layout,
      Rect,
   },
   style::{
      Color,
      Modifier,
      Style,
   },
   text::{
      Line,
      Span,
   },
   widgets::{
      Block,
      Borders,
      Paragraph,
      Wrap,
   },
};

use crate::{
   adapters::nls::liveticker::LivetickerEntry,
   timing::{
      Series,
      TimingNotice,
   },
};

#[derive(Debug, Clone, Copy)]
pub struct SeriesPickerState {
   pub is_open:      bool,
   pub selected_idx: usize,
}

impl SeriesPickerState {
   pub const fn closed() -> Self {
      Self {
         is_open:      false,
         selected_idx: 0,
      }
   }
}

#[derive(Debug, Clone, Copy)]
pub struct GroupPickerState {
   pub is_open:      bool,
   pub selected_idx: usize,
}

impl GroupPickerState {
   pub const fn closed() -> Self {
      Self {
         is_open:      false,
         selected_idx: 0,
      }
   }
}

#[derive(Debug, Clone, Copy)]
pub struct LogsPanelState {
   pub is_open:       bool,
   pub(crate) scroll: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct MessagesPanelState {
   pub is_open:      bool,
   pub selected_idx: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct NlsLivetickerPanelState {
   pub is_open:       bool,
   pub(crate) scroll: usize,
}

impl MessagesPanelState {
   pub const fn closed() -> Self {
      Self {
         is_open:      false,
         selected_idx: 0,
      }
   }
}

impl NlsLivetickerPanelState {
   pub const fn closed() -> Self {
      Self {
         is_open: false,
         scroll:  0,
      }
   }
}

impl LogsPanelState {
   pub const fn closed() -> Self {
      Self {
         is_open: false,
         scroll:  0,
      }
   }
}

pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
   let vertical = Layout::default()
      .direction(Direction::Vertical)
      .constraints([
         Constraint::Percentage((100 - percent_y) / 2),
         Constraint::Percentage(percent_y),
         Constraint::Percentage((100 - percent_y) / 2),
      ])
      .split(area);

   Layout::default()
      .direction(Direction::Horizontal)
      .constraints([
         Constraint::Percentage((100 - percent_x) / 2),
         Constraint::Percentage(percent_x),
         Constraint::Percentage((100 - percent_x) / 2),
      ])
      .split(vertical[1])[1]
}

pub fn help_popup() -> Paragraph<'static> {
   let text = vec![
      Line::from(vec![Span::styled(
         "Keyboard Help",
         Style::default().add_modifier(Modifier::BOLD),
      )]),
      Line::from(""),
      Line::from("h      toggle help"),
      Line::from("g      cycle views"),
      Line::from("G      open group selector popup"),
      Line::from("o      switch to overall view"),
      Line::from("t      open series selector popup"),
      Line::from("↑/↓    move selection"),
      Line::from("PgUp/PgDn  fast scroll"),
      Line::from("space  toggle favourite for selected car"),
      Line::from("f      jump to next favourite in current view"),
      Line::from("s      search by car #, driver, or team"),
      Line::from("n/p    next/prev search result"),
      Line::from("d      toggle demo/live data source"),
      Line::from("m      toggle race messages popup"),
      Line::from("l      toggle NLS liveticker popup"),
      Line::from("C      clear persisted message dismissals (in messages popup)"),
      Line::from("L      toggle IMSA debug logs"),
      Line::from("q      quit"),
      Line::from("Enter  confirm popup selection"),
      Line::from("Esc    close popup/help / quit app"),
      Line::from(""),
      Line::from("Press h or Esc to close this popup."),
   ];

   Paragraph::new(text)
      .alignment(Alignment::Left)
      .wrap(Wrap { trim: false })
      .block(Block::default().title("Help").borders(Borders::ALL))
}

pub fn messages_popup(
   notices: &[TimingNotice],
   selected_idx: usize,
   scroll: usize,
) -> Paragraph<'static> {
   let mut lines = vec![
      Line::from(vec![Span::styled(
         "Race Messages",
         Style::default().add_modifier(Modifier::BOLD),
      )]),
      Line::from(""),
   ];

   if notices.is_empty() {
      lines.push(Line::from("No active race messages."));
   } else {
      for (idx, notice) in notices.iter().enumerate() {
         let marker = if idx == selected_idx { ">" } else { " " };
         let style = if idx == selected_idx {
            Style::default()
               .fg(Color::Yellow)
               .add_modifier(Modifier::BOLD)
         } else {
            Style::default()
         };
         let time = if notice.time.trim().is_empty() {
            "--:--:--"
         } else {
            notice.time.trim()
         };
         lines.push(Line::from(vec![Span::styled(
            format!("{marker} {time}  {}", notice.text.trim()),
            style,
         )]));
      }
   }

   lines.push(Line::from(""));
   lines.push(Line::from(
      "↑/↓ select | Enter/d dismiss selected | c clear all | C reset history",
   ));
   lines.push(Line::from("Esc or m close"));

    Paragraph::new(lines)
       .alignment(Alignment::Left)
       .wrap(Wrap { trim: false })
       .scroll((u16::try_from(scroll).expect("scroll should fit in u16"), 0))
       .block(Block::default().title("Messages").borders(Borders::ALL))
}

pub fn series_picker_popup(active_series: Series, selected_idx: usize) -> Paragraph<'static> {
   let mut lines = vec![
      Line::from(vec![Span::styled(
         "Select Series",
         Style::default().add_modifier(Modifier::BOLD),
      )]),
      Line::from(""),
   ];

   for (idx, series) in Series::all().iter().copied().enumerate() {
      let marker = if idx == selected_idx { ">" } else { " " };
      let current = if series == active_series {
         " (current)"
      } else {
         ""
      };
      let style = if idx == selected_idx {
         Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
      } else {
         Style::default()
      };
      lines.push(Line::from(vec![Span::styled(
         format!("{marker} {}{current}", series.label()),
         style,
      )]));
   }

   lines.push(Line::from(""));
   lines.push(Line::from(
      "Use ↑/↓ to choose, Enter to switch, Esc to cancel.",
   ));

   Paragraph::new(lines)
      .alignment(Alignment::Left)
      .wrap(Wrap { trim: false })
      .block(Block::default().title("Series").borders(Borders::ALL))
}

pub fn group_picker_popup(groups: &[String], selected_idx: usize) -> Paragraph<'static> {
   let mut lines = vec![
      Line::from(vec![Span::styled(
         "Select Group",
         Style::default().add_modifier(Modifier::BOLD),
      )]),
      Line::from(""),
   ];

   if groups.is_empty() {
      lines.push(Line::from("No groups available for current series."));
   } else {
      for (idx, group_name) in groups.iter().enumerate() {
         let marker = if idx == selected_idx { ">" } else { " " };
         let style = if idx == selected_idx {
            Style::default()
               .fg(Color::Yellow)
               .add_modifier(Modifier::BOLD)
         } else {
            Style::default()
         };
         lines.push(Line::from(vec![Span::styled(
            format!("{marker} {group_name}"),
            style,
         )]));
      }
   }

   lines.push(Line::from(""));
   lines.push(Line::from(
      "Use ↑/↓ to choose, Enter to open class view, Esc to cancel.",
   ));

   Paragraph::new(lines)
      .alignment(Alignment::Left)
      .wrap(Wrap { trim: false })
      .block(Block::default().title("Group").borders(Borders::ALL))
}

pub fn nls_liveticker_popup(
   entries: &[LivetickerEntry],
   scroll: usize,
   updated_age_secs: Option<u64>,
   last_error: Option<&str>,
) -> Paragraph<'static> {
   let mut lines = vec![];

   let update_text = updated_age_secs.map_or_else(
      || "updated -".to_string(),
      |age| format!("updated {age}s ago"),
   );
   lines.push(Line::from(format!(
      "{} entries | {}",
      entries.len(),
      update_text
   )));
   if let Some(err) = last_error {
      lines.push(Line::from(format!("last error: {err}")));
   }
   lines.push(Line::from(""));

   if entries.is_empty() {
      lines.push(Line::from("No liveticker entries yet."));
   } else {
      for entry in entries.iter().rev() {
         lines.push(Line::from(vec![Span::styled(
            format!("{} {} Uhr", entry.day_label, entry.time_text),
            Style::default()
               .fg(Color::Yellow)
               .add_modifier(Modifier::BOLD),
         )]));

         if entry.message.is_empty() {
            lines.push(Line::from("-"));
         } else {
            for line in entry.message.lines() {
               lines.push(Line::from(line.to_string()));
            }
         }
         lines.push(Line::from(""));
      }
   }

   lines.push(Line::from(
      "↑/↓ scroll | PgUp/PgDn fast scroll | Home/End jump | Esc or l close",
   ));

    Paragraph::new(lines)
       .alignment(Alignment::Left)
       .wrap(Wrap { trim: false })
       .scroll((u16::try_from(scroll).expect("scroll should fit in u16"), 0))
       .block(
          Block::default()
             .title("NLS Liveticker")
             .borders(Borders::ALL),
       )
}

pub fn liveticker_line_count(entries: &[LivetickerEntry], has_error: bool) -> usize {
   let mut lines = 2usize;
   lines += 2;
   if has_error {
      lines += 1;
   }
   lines += 1;

   if entries.is_empty() {
      lines += 1;
   } else {
      for entry in entries {
         lines += 1;
         let msg_lines = entry.message.lines().count().max(1);
         lines += msg_lines;
         lines += 1;
      }
   }

   lines + 1
}
