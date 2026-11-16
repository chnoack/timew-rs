//! The summary table: its columns, week colors and the gap lines.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, Paragraph, Row, Scrollbar, ScrollbarOrientation,
    ScrollbarState, Table,
};
use timew_core::summary::{self, Summary};

use crate::app::App;
use crate::format::{decimal_hours, parse_hms, range_label, without_seconds};
use crate::ui::{ACCENT, FOCUS, PAD};

/// Wk/Date/Day colors, alternating from one ISO week to the next.
const WEEK_COLORS: [Color; 2] = [Color::Cyan, Color::Magenta];

/// Color of the line that marks a gap between two entries of a day.
pub(crate) const GAP_COLOR: Color = Color::Indexed(244);

/// For each summary row: whether there is untracked time between it and
/// the next row of the same day. Never after the last row of a day (or of
/// the list), and not after a running entry.
pub(crate) fn gap_after(app: &App) -> Vec<bool> {
    let rows = &app.summary.rows;
    (0..rows.len())
        .map(|i| {
            let Some(next) = rows.get(i + 1) else { return false };
            if next.date.is_some() {
                return false; // the next row starts another day
            }
            let (this, next) = (&app.tracked[rows[i].source].interval, &app.tracked[next.source].interval);
            this.range.is_ended() && this.range.end < next.range.start
        })
        .collect()
}

/// For each summary row, 0 or 1: flips whenever the ISO week (week-year +
/// week number) of the row's day changes, so consecutive weeks alternate.
/// Rows without a date belong to the day (and week) of the row above.
pub(crate) fn week_parity(rows: &[summary::Row]) -> Vec<usize> {
    let mut parity = 0;
    let mut current: Option<(i16, i8)> = None;
    rows.iter()
        .map(|r| {
            let week = r.date.as_deref().and_then(|d| d.parse::<jiff::civil::Date>().ok()).map(|d| {
                let iso = d.iso_week_date();
                (iso.year(), iso.week())
            });
            if let Some(week) = week {
                if current.is_some_and(|c| c != week) {
                    parity = 1 - parity;
                }
                current = Some(week);
            }
            parity
        })
        .collect()
}

// ID and Annotation are deliberately not shown in the TUI table.
const HEADERS: [&str; 8] = ["Wk", "Date", "Day", "Tags", "Start", "End", "Time", "Total"];

const RIGHT: [bool; 8] = [false, false, false, false, true, true, true, true];

fn cells_of(r: &summary::Row) -> [String; 8] {
    [
        r.week.clone().unwrap_or_default(),
        r.date.clone().unwrap_or_default(),
        r.day_name.clone().unwrap_or_default(),
        r.tags.clone(),
        without_seconds(&r.start),
        without_seconds(&r.end),
        without_seconds(&r.time),
        parse_hms(&r.total).map(decimal_hours).unwrap_or_default(),
    ]
}

/// Content-driven width of each table column (max of header and cells).
fn column_widths(summary: &Summary) -> [usize; 8] {
    let mut widths: [usize; 8] = HEADERS.map(str::len);
    for row in &summary.rows {
        for (w, c) in widths.iter_mut().zip(cells_of(row)) {
            *w = (*w).max(c.chars().count());
        }
    }
    widths
}

/// Width the summary table needs: columns + spacing + borders + scrollbar gap.
pub(crate) fn table_width(summary: &Summary) -> u16 {
    let cols: usize = column_widths(summary).iter().sum();
    (cols + HEADERS.len() - 1 + 2) as u16 + PAD.left + PAD.right
}

/// The summary frame's title, e.g. " Summary · Month · 2026-11-01 – 2026-11-30 · tag meetings ".
pub(crate) fn summary_title(app: &App) -> Line<'static> {
    let mut spans = vec![
        Span::raw(" Summary · "),
        Span::styled(app.filter.label(), Style::new().fg(ACCENT).bold()),
        Span::raw(format!(" · {} ", range_label(&app.range))),
    ];
    if let Some(tag) = &app.tag {
        spans.extend([Span::raw("· tag "), Span::styled(tag.clone(), Style::new().fg(ACCENT).bold()), Span::raw(" ")]);
    }
    Line::from(spans)
}

pub(crate) fn draw_summary(frame: &mut Frame, area: Rect, app: &mut App) {
    let title = summary_title(app);
    // the view with the focus has a yellow border (like the edit form)
    let border = if app.table_has_focus() { Style::new().fg(FOCUS) } else { Style::new() };
    let block = Block::new().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(border).title(title).padding(PAD);

    if let Some(err) = &app.error {
        let p = Paragraph::new(format!("Error reading database:\n{err}")).fg(Color::Red).block(block);
        frame.render_widget(p, area);
        return;
    }
    if app.summary.rows.is_empty() {
        let tagged = app.tag.as_ref().map(|t| format!(" tagged with {t}")).unwrap_or_default();
        let p = Paragraph::new(format!("\nNo data found in the range {}{tagged}.", range_label(&app.range)))
            .alignment(Alignment::Center)
            .dim()
            .block(block);
        frame.render_widget(p, area);
        return;
    }

    let all: Vec<[String; 8]> = app.summary.rows.iter().map(cells_of).collect();
    let widths = column_widths(&app.summary);

    let align = |s: String, right: bool| {
        let line = Line::from(s);
        if right { line.alignment(Alignment::Right) } else { line }
    };

    let header = Row::new(HEADERS.iter().zip(RIGHT).map(|(h, r)| Cell::from(align(h.to_string(), r))))
        .style(Style::new().bold().underlined());

    let weeks = week_parity(&app.summary.rows);
    let gaps = gap_after(app);
    let mut day_index = 0usize;
    let rows = all.into_iter().enumerate().map(|(i, cells)| {
        if i > 0 && app.summary.rows[i].date.is_some() {
            day_index += 1;
        }
        let stripe = if day_index % 2 == 1 { Style::new().bg(Color::Indexed(235)) } else { Style::new() };
        let cells = cells.into_iter().enumerate().map(|(c, s)| {
            let style = match c {
                0..=2 => Style::new().fg(WEEK_COLORS[weeks[i]]).bold(),
                7 => Style::new().fg(Color::Yellow).bold(),
                _ => Style::new(),
            };
            Cell::from(align(s, RIGHT[c])).style(style)
        });
        Row::new(cells).style(stripe)
    });

    let constraints = widths.map(|w| Constraint::Length(w as u16));
    let table = Table::new(rows, constraints)
        .header(header)
        .column_spacing(1)
        .row_highlight_style(Style::new().add_modifier(Modifier::REVERSED))
        .block(block);

    // inner height minus borders, padding and the header row
    app.page = area.height.saturating_sub(2 + PAD.top + PAD.bottom + 1) as usize;
    app.table_area = area;
    frame.render_stateful_widget(table, area, &mut app.table);

    // A gap before the next entry of the same day: a thin dashed grey line
    // under the row, from the Tags column to the Total column (underlined
    // segments of two columns, one column apart).
    let inner = Rect::new(
        area.x + 1 + PAD.left,
        area.y + 1 + PAD.top,
        area.width.saturating_sub(2 + PAD.left + PAD.right),
        area.height.saturating_sub(2 + PAD.top + PAD.bottom),
    );
    let first_row_y = inner.y + 1; // below the header
    // columns are laid out from the left with one column of spacing
    let column_x = |c: usize| inner.x + widths[..c].iter().map(|w| *w as u16 + 1).sum::<u16>();
    let line_from = column_x(3);
    let line_to = (column_x(7) + widths[7] as u16).min(inner.right());
    let offset = app.table.offset();
    let buf = frame.buffer_mut();
    for (i, gap) in gaps.iter().enumerate().skip(offset) {
        let y = first_row_y + (i - offset) as u16;
        if y >= inner.bottom() {
            break;
        }
        if !gap {
            continue;
        }
        for x in line_from..line_to {
            if (x - line_from) % 3 != 2 {
                buf[(x, y)].set_style(Style::new().underlined().underline_color(GAP_COLOR));
            }
        }
    }

    if app.summary.rows.len() <= app.page {
        return;
    }
    let mut sb = ScrollbarState::new(app.summary.rows.len()).position(app.table.selected().unwrap_or(0));
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight).begin_symbol(None).end_symbol(None),
        area.inner(ratatui::layout::Margin { vertical: 1, horizontal: 0 }),
        &mut sb,
    );
}
