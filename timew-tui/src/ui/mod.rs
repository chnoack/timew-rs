//! Drawing the screen. [`draw`] lays out the areas and calls the
//! submodules; colors and paddings shared by all parts are defined here.

pub(crate) mod chart;
pub(crate) mod details;
pub(crate) mod footer;
pub(crate) mod popups;
pub(crate) mod table;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Padding, Paragraph};

use crate::app::{App, Button};
use crate::filter::Filter;
use crate::format::range_label;
use crate::forms::{Completion, tags_to_text};
use crate::ui::chart::{chart_height, draw_chart, tag_totals};
use crate::ui::details::draw_details;
use crate::ui::footer::{draw_help, draw_messages, draw_status, messages_height};
use crate::ui::popups::{
    draw_confirm_delete, draw_info, draw_legend, draw_month_picker, draw_picker, draw_range_input,
    draw_start_form, draw_year_picker,
};
use crate::ui::table::{draw_summary, summary_title, table_width};

pub(crate) const ACCENT: Color = Color::Cyan;

/// Border of the view that has the keyboard focus.
pub(crate) const FOCUS: Color = Color::Yellow;

/// A suggested tag after the text of a field (dark gray on the active
/// field's light background).
pub(crate) const SUGGESTION: Color = Color::DarkGray;

/// The hint under a tags field with a suggestion; it names the tag, which
/// shows its case even when only the case differs from what was typed.
pub(crate) fn suggestion_hint(completion: Option<&Completion>) -> String {
    let tag = completion.map(|c| tags_to_text(std::slice::from_ref(&c.tag))).unwrap_or_default();
    format!("Tab or → takes the suggested tag {tag}")
}

/// A one-line text field `width` cells wide: a space, the visible `text`,
/// then the suggested completion (if any, as far as it fits), padded.
/// The active field is drawn reversed, the others underlined.
pub(crate) fn text_field(text: String, suggestion: Option<&str>, width: usize, active: bool) -> Vec<Span<'static>> {
    let style = if active { Style::new().add_modifier(Modifier::REVERSED) } else { Style::new().underlined() };
    let mut used = 1 + text.chars().count();
    let mut spans = vec![Span::styled(format!(" {text}"), style)];
    if let Some(rest) = suggestion {
        // reversed, so the background color is what shows as the text color
        let rest: String = rest.chars().take(width.saturating_sub(used + 1)).collect();
        used += rest.chars().count();
        spans.push(Span::styled(rest, style.bg(SUGGESTION)));
    }
    spans.push(Span::styled(" ".repeat(width.saturating_sub(used)), style));
    spans
}

pub(crate) fn draw(frame: &mut Frame, app: &mut App) {
    let [header, filters, body, status, help] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Min(3),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    draw_header(frame, header, app);
    draw_filters(frame, filters, app);
    // the table is at least as wide as its title, so the title is never cut off
    let table_w = table_width(&app.summary).max(summary_title(app).width() as u16 + 4);
    app.wide = body.width >= table_w + DETAILS_MIN_WIDTH;
    let has_selection = app.selected().is_some();
    // Outside the side column (narrow window, or nothing selected), a
    // message gets its own panel below everything else in the body.
    let mut body = body;
    if !(app.wide && has_selection) && app.notice.is_some() {
        let h = messages_height(app, body.width);
        let [rest, messages] = Layout::vertical([Constraint::Min(3), Constraint::Length(h)]).areas(body);
        draw_messages(frame, messages, app);
        body = rest;
    }
    if app.wide && has_selection {
        // wide: table at its natural width; on the right the details, the
        // chart below them, and a message (if any) at the bottom
        let [table, side] = Layout::horizontal([Constraint::Length(table_w), Constraint::Min(DETAILS_MIN_WIDTH)]).areas(body);
        draw_summary(frame, table, app);
        let messages_h = if app.notice.is_some() { messages_height(app, side.width) } else { 0 };
        let [details, chart, messages] = Layout::vertical([
            Constraint::Length(DETAILS_HEIGHT),
            Constraint::Min(0),
            Constraint::Length(messages_h),
        ])
        .areas(side);
        draw_details(frame, details, app);
        if app.notice.is_some() {
            draw_messages(frame, messages, app);
        }
        // the chart needs room for its frame, padding and at least one bar
        if chart.height >= 3 + PAD.top + PAD.bottom {
            draw_chart(frame, chart, app);
        }
    } else if !app.wide && app.chart && app.edit.is_none() && has_selection {
        // narrow: the chart below the table
        let h = chart_height(&tag_totals(app));
        let [table, chart] = Layout::vertical([Constraint::Min(5), Constraint::Length(h)]).areas(body);
        draw_summary(frame, table, app);
        draw_chart(frame, chart, app);
    } else if !app.wide && (app.details || app.edit.is_some()) && has_selection {
        // narrow: details below the table, only after Enter
        let [table, details] = Layout::vertical([Constraint::Min(5), Constraint::Length(DETAILS_HEIGHT)]).areas(body);
        draw_summary(frame, table, app);
        draw_details(frame, details, app);
    } else {
        draw_summary(frame, body, app);
    }
    draw_status(frame, status, app);
    draw_help(frame, help, app.wide, app.last_edit.is_some(), app.running.is_some());
    draw_picker(frame, body, app);
    draw_range_input(frame, body, app);
    draw_month_picker(frame, body, app);
    draw_year_picker(frame, body, app);
    draw_legend(frame, frame.area(), app);
    draw_start_form(frame, body, app);
    draw_confirm_delete(frame, body, app);
    draw_info(frame, frame.area(), app);
}

/// Inner spacing between a view's frame and its content.
pub(crate) const PAD: Padding = Padding::new(2, 2, 1, 0);

/// Height of the bottom details panel: 10 content lines + borders + padding.
const DETAILS_HEIGHT: u16 = 10 + 2 + PAD.top + PAD.bottom;

/// Minimum width of the side details panel; below that the narrow layout is used.
const DETAILS_MIN_WIDTH: u16 = 44;

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let line = Line::from(vec![
        Span::styled(" timew-tui ", Style::new().fg(Color::Black).bg(ACCENT).bold()),
        Span::raw("  "),
        Span::styled(app.db_root.display().to_string(), Style::new().dim()),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn draw_filters(frame: &mut Frame, area: Rect, app: &mut App) {
    app.buttons.clear();
    let mut buttons: Vec<(String, bool, Button)> = Filter::ALL
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let label = match (f, app.month, app.year) {
                (Filter::Month, Some((y, m)), _) if app.filter == Filter::Month => {
                    format!(" {} {y}-{m:02} ▾ ", i + 1)
                }
                (Filter::Year, _, Some(y)) if app.filter == Filter::Year => format!(" {} {y} ▾ ", i + 1),
                _ => format!(" {} {} ", i + 1, f.label()),
            };
            (label, *f == app.filter, Button::Range(*f))
        })
        .collect();
    let custom_label = if app.filter == Filter::Custom {
        format!(" 5 {} ▾ ", range_label(&app.range))
    } else {
        " 5 Range ".to_string()
    };
    buttons.push((custom_label, app.filter == Filter::Custom || app.range_input.is_some(), Button::CustomRange));
    let tag_label = match &app.tag {
        Some(t) => format!(" 6 {t} ▾ "),
        None => " 6 Tag ".to_string(),
    };
    buttons.push((tag_label, app.tag.is_some() || app.picker.is_some(), Button::Tag));

    let mut x = area.x;
    for (text, active, button) in buttons {
        if button == Button::Tag {
            x += 2; // visually separate the tag filter from the range filters
        }
        let w = text.chars().count() as u16 + 2;
        if x + w > area.right() {
            break;
        }
        let rect = Rect::new(x, area.y, w, area.height);
        let (border, label) = if active {
            (Style::new().fg(ACCENT), Style::new().fg(Color::Black).bg(ACCENT).bold())
        } else {
            (Style::new().dim(), Style::new())
        };
        let widget = Paragraph::new(Line::from(text).style(label))
            .alignment(Alignment::Center)
            .block(Block::bordered().border_type(BorderType::Rounded).border_style(border));
        frame.render_widget(widget, rect);
        app.buttons.push((rect, button));
        x += w + 1;
    }
}
