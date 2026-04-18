use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::timing::{Series, TimingNotice};

#[derive(Debug, Clone, Copy)]
pub(crate) struct SeriesPickerState {
    pub(crate) is_open: bool,
    pub(crate) selected_idx: usize,
}

impl SeriesPickerState {
    pub(crate) fn closed() -> Self {
        Self {
            is_open: false,
            selected_idx: 0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct GroupPickerState {
    pub(crate) is_open: bool,
    pub(crate) selected_idx: usize,
}

impl GroupPickerState {
    pub(crate) fn closed() -> Self {
        Self {
            is_open: false,
            selected_idx: 0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LogsPanelState {
    pub(crate) is_open: bool,
    pub(crate) scroll: usize,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct MessagesPanelState {
    pub(crate) is_open: bool,
    pub(crate) selected_idx: usize,
}

impl MessagesPanelState {
    pub(crate) fn closed() -> Self {
        Self {
            is_open: false,
            selected_idx: 0,
        }
    }
}

impl LogsPanelState {
    pub(crate) fn closed() -> Self {
        Self {
            is_open: false,
            scroll: 0,
        }
    }
}

pub(crate) fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
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

pub(crate) fn help_popup() -> Paragraph<'static> {
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

pub(crate) fn messages_popup(notices: &[TimingNotice], selected_idx: usize) -> Paragraph<'static> {
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
        .block(Block::default().title("Messages").borders(Borders::ALL))
}

pub(crate) fn series_picker_popup(
    active_series: Series,
    selected_idx: usize,
) -> Paragraph<'static> {
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

pub(crate) fn group_picker_popup(groups: &[String], selected_idx: usize) -> Paragraph<'static> {
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
