//! The bottom of the screen: the messages panel, the status line and the
//! help line.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

use crate::app::App;
use crate::format::decimal_hours;
use crate::ui::{ACCENT, PAD};

/// Height of the messages panel for an area `width` columns wide: the
/// message wrapped to the panel (at least one line, at most three).
pub(crate) fn messages_height(app: &App, width: u16) -> u16 {
    let inner = width.saturating_sub(2 + PAD.left + PAD.right).max(1) as usize;
    let text = app.notice.as_deref().map_or(1, |n| n.chars().count().div_ceil(inner));
    text.clamp(1, 3) as u16 + 2 + PAD.top + PAD.bottom
}

/// The messages panel: the result of the last action (green, or red if it
/// failed), until the next key. Only shown while there is a message.
pub(crate) fn draw_messages(frame: &mut Frame, area: Rect, app: &App) {
    let Some(notice) = &app.notice else { return };
    let color = if app.notice_failed { Color::Red } else { Color::Green };
    let line = Line::styled(notice.clone(), Style::new().fg(color).bold());
    let block = Block::bordered().border_type(BorderType::Rounded).title(" Messages ").padding(PAD);
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(line).wrap(ratatui::widgets::Wrap { trim: true }).block(block), area);
}

pub(crate) fn draw_status(frame: &mut Frame, area: Rect, app: &App) {
    let days = app.summary.rows.iter().filter(|r| r.date.is_some()).count();
    let intervals = app.summary.rows.len();
    let total = app.summary.grand_total;
    let avg = if days > 0 { format!("{} h", decimal_hours(total / days as i64)) } else { "-".into() };
    let pos = match app.table.selected() {
        Some(i) if intervals > 0 => format!("row {}/{}", i + 1, intervals),
        _ => String::new(),
    };
    let line = Line::from(vec![
        Span::raw(" Total "),
        Span::styled(format!("{} h", decimal_hours(total)), Style::new().fg(Color::Yellow).bold()),
        Span::raw(format!(
            "  ·  {days} {}  ·  {intervals} {}  ·  avg/day ",
            if days == 1 { "day" } else { "days" },
            if intervals == 1 { "interval" } else { "intervals" },
        )),
        Span::styled(avg, Style::new().bold()),
        Span::raw("  ·  "),
        Span::styled(pos, Style::new().dim()),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

pub(crate) fn draw_help(frame: &mut Frame, area: Rect, wide: bool, can_undo: bool, running: bool) {
    // "? help" (the full key legend) always comes first; then the actions,
    // so that they are visible in a narrow window too
    let mut items: Vec<(&str, &str)> = vec![("?", "help")];
    items.push(("s", if running { "stop" } else { "start" }));
    if can_undo {
        // first, so that it is visible in a narrow window too
        items.push(("u", "undo"));
    }
    items.extend([
        ("↑↓", "move"),
        ("1-4", "range"),
        ("⇧1/⇧3/⇧4", "pick"),
        ("5", "range"),
        ("6", "tag"),
    ]);
    if !wide {
        items.extend([("b", "chart"), ("Enter", "details")]);
    }
    items.extend([("e", "edit"), ("a", "add"), ("d", "delete")]);
    items.extend([("PgUp/Dn", "page"), ("r", "reload"), ("i", "info")]);

    // Keep "q quit" at the end; if not everything fits, drop items from
    // the end (the legend lists them all).
    let width = |k: &str, l: &str| k.chars().count() + 1 + l.chars().count() + 2;
    let tail = [("q", "quit")];
    let tail_w: usize = tail.iter().map(|(k, l)| width(k, l)).sum();
    let mut room = (area.width as usize).saturating_sub(1 + tail_w);
    let mut spans = vec![Span::raw(" ")];
    for (k, l) in items {
        let w = width(k, l);
        if w > room {
            break;
        }
        room -= w;
        spans.extend([Span::styled(k, Style::new().fg(ACCENT).bold()), Span::raw(format!(" {l}  "))]);
    }
    for (i, (k, l)) in tail.iter().enumerate() {
        let sep = if i + 1 < tail.len() { "  " } else { "" };
        spans.extend([Span::styled(*k, Style::new().fg(ACCENT).bold()), Span::raw(format!(" {l}{sep}"))]);
    }
    frame.render_widget(Paragraph::new(Line::from(spans).dim()), area);
}
