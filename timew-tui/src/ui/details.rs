//! The details panel of the selected entry and the edit form in it.

use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

use crate::app::App;
use crate::format::{format_hm, local_datetime};
use crate::forms::{ANNOTATION, EDIT_FIELD_WIDTHS, TAGS, is_text};
use crate::ui::{ACCENT, FOCUS, PAD, suggestion_hint, text_field};

pub(crate) fn draw_details(frame: &mut Frame, area: Rect, app: &mut App) {
    if app.edit.is_some() {
        return draw_edit(frame, area, app);
    }
    let Some(t) = app.selected() else { return };
    let iv = &t.interval;
    let now = jiff::Timestamp::now().as_second();
    // day and ISO week of the entry's start, in local time
    let start_day = jiff::Timestamp::from_second(iv.range.start).unwrap().to_zoned(jiff::tz::TimeZone::system()).date();
    let label = |s: &'static str| Span::styled(format!("{s:<11}"), Style::new().fg(ACCENT).bold());

    let tags: Vec<Span> = if iv.tags.is_empty() {
        vec![Span::styled("(none)", Style::new().dim())]
    } else {
        iv.tags
            .iter()
            .flat_map(|t| [Span::styled(format!(" {t} "), Style::new().fg(Color::Black).bg(ACCENT)), Span::raw(" ")])
            .collect()
    };
    let end = if iv.is_open() {
        Span::styled("still tracking", Style::new().fg(Color::Green).italic())
    } else {
        Span::raw(local_datetime(iv.range.end))
    };
    let days = app.summary.rows.iter().filter(|r| r.source == app.summary.rows[app.table.selected().unwrap()].source).count();

    let mut lines = vec![
        Line::from(vec![label("ID"), Span::raw(format!("@{}", t.id))]),
        Line::from([vec![label("Tags")], tags].concat()),
        Line::from(vec![
            label("Annotation"),
            match &iv.annotation {
                Some(a) if !a.is_empty() => Span::raw(a.clone()),
                _ => Span::styled("(none)", Style::new().dim()),
            },
        ]),
        Line::raw(""),
        Line::from(vec![label("Day"), Span::raw(start_day.strftime("%A").to_string())]),
        Line::from(vec![
            label("Week"),
            Span::raw(format!("W{:02} {}", start_day.iso_week_date().week(), start_day.iso_week_date().year())),
        ]),
        Line::from(vec![label("Start"), Span::raw(local_datetime(iv.range.start))]),
        Line::from(vec![label("End"), end]),
        Line::from(vec![
            label("Duration"),
            Span::styled(format_hm(iv.range.total(now)), Style::new().fg(Color::Yellow).bold()),
        ]),
    ];
    if days > 1 {
        lines.push(Line::from(vec![label("Spans"), Span::raw(format!("{days} days"))]));
    }

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        // the running entry is framed in red until it is stopped
        .border_style(Style::new().fg(if iv.is_open() { Color::Red } else { ACCENT }))
        .title(if iv.is_open() { " Entry details · running " } else { " Entry details " })
        .title_bottom(if app.wide { Line::raw("") } else { Line::from(" ↑↓ next/prev · Enter/Esc close ").right_aligned().dim() });
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: false }).block(block.padding(PAD)),
        area,
    );
}

/// The details panel as a form for the start and end of the entry.
fn draw_edit(frame: &mut Frame, area: Rect, app: &mut App) {
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(FOCUS))
        .title(" Edit entry ")
        .title_bottom(Line::from(" Enter save · ⇧Enter adjust · Esc cancel ").right_aligned().dim())
        .padding(PAD);
    let inner = block.inner(area);
    let x = inner.x + 11;
    // the tags field scrolls sideways to keep the cursor visible
    let tags_w = inner.right().saturating_sub(x);
    let Some(form) = &mut app.edit else { return };
    let visible = (tags_w as usize).saturating_sub(2).max(1);
    if is_text(form.field) {
        if form.cursor < form.scroll {
            form.scroll = form.cursor;
        } else if form.cursor >= form.scroll + visible {
            form.scroll = form.cursor + 1 - visible;
        }
    } else {
        form.scroll = 0;
    }
    let form = &*form;
    let suggestion = form.suggestion(&app.all_tags);
    let iv = &form.original;
    let label = |s: &'static str| Span::styled(format!("{s:<11}"), Style::new().fg(ACCENT).bold());
    let field = |i: usize| -> Vec<Span<'static>> {
        if is_text(i) {
            let skip = if i == form.field { form.scroll } else { 0 };
            let text: String = form.fields[i].chars().skip(skip).take(visible).collect();
            let rest = if i == form.field { suggestion.as_ref().map(|c| c.rest.as_str()) } else { None };
            return text_field(text, rest, tags_w as usize, i == form.field);
        }
        let style = if form.field == i { Style::new().add_modifier(Modifier::REVERSED) } else { Style::new().underlined() };
        vec![Span::styled(format!(" {:<w$}", form.fields[i], w = EDIT_FIELD_WIDTHS[i] as usize - 1), style)]
    };
    let now = jiff::Timestamp::now().as_second();
    let duration = match form.edited() {
        Ok(e) => Span::styled(format_hm(e.range.total(now)), Style::new().fg(Color::Yellow).bold()),
        Err(_) => Span::styled("–", Style::new().dim()),
    };
    let last = match &form.error {
        Some(err) => Line::styled(err.clone(), Style::new().fg(Color::Red).bold()),
        None if suggestion.is_some() => Line::styled(suggestion_hint(suggestion.as_ref()), Style::new().dim()),
        None if form.field == TAGS => Line::styled("separated by spaces, \"in quotes\" with spaces", Style::new().dim()),
        None if form.field == ANNOTATION => Line::styled("any text; empty removes the annotation", Style::new().dim()),
        None if iv.is_open() => Line::styled("YYYY-MM-DD  HH:MM · empty end: still tracking", Style::new().dim()),
        None => Line::styled("YYYY-MM-DD  HH:MM", Style::new().dim()),
    };
    let lines = vec![
        Line::from(vec![label("ID"), Span::raw(format!("@{}", form.id))]),
        Line::from([vec![label("Tags")], field(TAGS)].concat()),
        Line::from([vec![label("Annotation")], field(ANNOTATION)].concat()),
        Line::raw(""),
        Line::from([vec![label("Start")], field(0), vec![Span::raw(" ")], field(1)].concat()),
        Line::from([vec![label("End")], field(2), vec![Span::raw(" ")], field(3)].concat()),
        Line::from(vec![label("Duration"), duration]),
        Line::raw(""),
        last,
    ];
    let rect = |dx: u16, dy: u16, w: u16| Rect::new(x + dx, inner.y + dy, w, 1).intersection(inner);
    let w = EDIT_FIELD_WIDTHS;
    let fields = [
        rect(0, 4, w[0]),
        rect(w[0] + 1, 4, w[1]),
        rect(0, 5, w[2]),
        rect(w[2] + 1, 5, w[3]),
        rect(0, 1, tags_w),
        rect(0, 2, tags_w),
    ];
    let active = fields[form.field];
    let column = form.cursor - if is_text(form.field) { form.scroll } else { 0 };
    let cursor = Position::new((active.x + 1 + column as u16).min(active.right().saturating_sub(1)), active.y);
    let has_cursor = active.width > 0 && active.height > 0;
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(lines).block(block), area);
    app.edit_fields = fields;
    if has_cursor {
        frame.set_cursor_position(cursor);
    }
}
