//! The popups: month and year lists, the tag list, the date range form,
//! the start form, the delete confirmation, the info dialog and the key
//! legend.

use std::env;

use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Clear, List, ListItem, Padding, Paragraph, Scrollbar, ScrollbarOrientation,
    ScrollbarState,
};

use crate::app::App;
use crate::filter::{Filter, MONTH_NAMES};
use crate::format::{format_hm, local_date, local_datetime, local_moment};
use crate::forms::DeleteDialog;
use crate::ui::{ACCENT, FOCUS, PAD, text_field};

pub(crate) fn draw_year_picker(frame: &mut Frame, area: Rect, app: &mut App) {
    let Some(p) = &mut app.year_picker else { return };
    let shown = (app.filter == Filter::Year).then(|| app.year.unwrap_or_else(|| jiff::Zoned::now().year()));
    let items: Vec<ListItem> = p
        .years
        .iter()
        .map(|year| {
            let style = if shown == Some(*year) { Style::new().fg(ACCENT).bold() } else { Style::new() };
            ListItem::new(Line::styled(year.to_string(), style))
        })
        .collect();

    let footer = " Enter select · ⌫ current · Esc ";
    let w = (footer.chars().count() as u16 + 4).min(area.width);
    let h = (items.len() as u16 + 2 + PAD.top + 1).min(area.height);
    let popup = Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(ACCENT))
        .title(" Year ")
        .title_bottom(Line::from(footer).right_aligned().dim())
        .padding(Padding::new(PAD.left, PAD.right, PAD.top, 1));
    app.year_picker_area = block.inner(popup);
    let list = List::new(items)
        .block(block)
        .highlight_symbol("▸ ")
        .highlight_style(Style::new().add_modifier(Modifier::REVERSED));
    frame.render_widget(Clear, popup);
    frame.render_stateful_widget(list, popup, &mut p.state);
}

pub(crate) fn draw_month_picker(frame: &mut Frame, area: Rect, app: &mut App) {
    let Some(p) = &mut app.month_picker else { return };
    let shown = match (app.filter, app.month) {
        (Filter::Month, Some(m)) => Some(m),
        (Filter::Month, None) => {
            let now = jiff::Zoned::now();
            Some((now.year(), now.month()))
        }
        _ => None,
    };
    let items: Vec<ListItem> = MONTH_NAMES
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let current = shown == Some((p.year, i as i8 + 1));
            let style = if current { Style::new().fg(ACCENT).bold() } else { Style::new() };
            ListItem::new(Line::styled(*name, style))
        })
        .collect();

    let footer = " Enter select · ←→ year · ⌫ current · Esc ";
    let w = (footer.chars().count() as u16 + 4).min(area.width);
    let h = (12 + 2 + PAD.top + 1).min(area.height);
    let popup = Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h);
    let title = Line::from(vec![
        Span::raw(" ◂ "),
        Span::styled(p.year.to_string(), Style::new().fg(ACCENT).bold()),
        Span::raw(" ▸ "),
    ]);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(ACCENT))
        .title(title)
        .title_bottom(Line::from(footer).right_aligned().dim())
        .padding(Padding::new(PAD.left, PAD.right, PAD.top, 1));
    app.month_picker_area = block.inner(popup);
    let list = List::new(items)
        .block(block)
        .highlight_symbol("▸ ")
        .highlight_style(Style::new().add_modifier(Modifier::REVERSED));
    frame.render_widget(Clear, popup);
    frame.render_stateful_widget(list, popup, &mut p.state);
}

/// The delete confirmation (`d`): what would be deleted.
pub(crate) fn draw_confirm_delete(frame: &mut Frame, area: Rect, app: &App) {
    let Some(DeleteDialog { interval: iv, id, previous }) = &app.confirm_delete else { return };
    let label = |s: &'static str| Span::styled(format!("{s:<11}"), Style::new().fg(ACCENT).bold());
    let none = || Span::styled("(none)", Style::new().dim());
    let end = if iv.is_open() { "still tracking".to_string() } else { local_datetime(iv.range.end) };
    let now = jiff::Timestamp::now().as_second();
    let days = {
        let date = |e: i64| local_date(e);
        let last = if iv.is_open() { now } else { iv.range.end - 1 };
        let (a, b) = (date(iv.range.start), date(last.max(iv.range.start)));
        if a == b { None } else { Some(format!("{a} – {b}")) }
    };
    let mut lines = vec![
        Line::styled("Delete this entry?", Style::new().bold()),
        Line::raw(""),
        Line::from(vec![label("ID"), Span::raw(format!("@{id}"))]),
        Line::from(vec![label("Tags"), if iv.tags.is_empty() { none() } else { Span::raw(iv.tags.join(", ")) }]),
        Line::from(vec![
            label("Annotation"),
            match &iv.annotation {
                Some(a) if !a.is_empty() => Span::raw(a.clone()),
                _ => none(),
            },
        ]),
        Line::from(vec![label("Start"), Span::raw(local_datetime(iv.range.start))]),
        Line::from(vec![label("End"), Span::raw(end)]),
        Line::from(vec![label("Duration"), Span::styled(format_hm(iv.range.total(now)), Style::new().fg(Color::Yellow).bold())]),
    ];
    if let Some(span) = days {
        lines.push(Line::from(vec![label("Spans"), Span::raw(span)]));
    }
    if let Some(p) = previous {
        let tags = if p.tags.is_empty() { "(no tags)".to_string() } else { p.tags.join(", ") };
        let new_end = if iv.is_open() { "still tracking".to_string() } else { local_moment(iv.range.end) };
        lines.extend([
            Line::raw(""),
            Line::from(vec![
                Span::styled("e", Style::new().fg(Color::Yellow).bold()),
                Span::raw(" instead deletes it and extends the previous entry:"),
            ]),
            Line::from(vec![
                label("  Previous"),
                Span::raw(format!("{tags}, {} – {}", local_datetime(p.range.start), local_moment(p.range.end))),
            ]),
            Line::from(vec![label("  New end"), Span::styled(new_end, Style::new().fg(Color::Yellow).bold())]),
        ]);
    }
    let text_w = lines.iter().map(|l| l.width()).max().unwrap_or(0) as u16;
    let w = (text_w + PAD.left + PAD.right + 2).max(44).min(area.width);
    let h = (lines.len() as u16 + 2 + PAD.top + 1).min(area.height);
    let popup = Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(Color::Red))
        .title(" Delete ")
        .title_bottom(
            Line::from(if previous.is_some() { " y delete · e delete + extend previous · n/Esc cancel " } else { " y delete · n/Esc cancel " })
                .right_aligned()
                .dim(),
        )
        .padding(Padding::new(PAD.left, PAD.right, PAD.top, 1));
    frame.render_widget(Clear, popup);
    frame.render_widget(Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: false }).block(block), popup);
}

/// The start dialog (`s`): tags and annotation for a new entry.
pub(crate) fn draw_start_form(frame: &mut Frame, area: Rect, app: &mut App) {
    const LABEL_W: u16 = 12;
    let w = 64u16.min(area.width);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(FOCUS))
        .title(" Start tracking ")
        .title_bottom(Line::from(" Enter start · Tab next · Esc cancel ").right_aligned().dim())
        .padding(Padding::new(PAD.left, PAD.right, PAD.top, 1));
    let Some(form) = &mut app.start_form else { return };
    let h = (6 + 2 + PAD.top + 1).min(area.height);
    let popup = Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h);
    let inner = block.inner(popup);
    let field_w = inner.width.saturating_sub(LABEL_W);
    let visible = (field_w as usize).saturating_sub(2).max(1);
    if form.cursor < form.scroll {
        form.scroll = form.cursor;
    } else if form.cursor >= form.scroll + visible {
        form.scroll = form.cursor + 1 - visible;
    }
    let form = &*form;
    let suggestion = form.suggestion(&app.all_tags);
    let label = |s: &'static str| Span::styled(format!("{s:<w$}", w = LABEL_W as usize), Style::new().fg(ACCENT).bold());
    let field = |i: usize| {
        let skip = if i == form.field { form.scroll } else { 0 };
        let text: String = form.fields[i].chars().skip(skip).take(visible).collect();
        let rest = if i == form.field { suggestion.as_deref() } else { None };
        text_field(text, rest, field_w as usize, i == form.field)
    };
    let last = match &form.error {
        Some(err) => Line::styled(err.clone(), Style::new().fg(Color::Red).bold()),
        None if suggestion.is_some() => Line::styled("Tab or → takes the suggested tag", Style::new().dim()),
        None if form.field == 0 => Line::styled("separated by spaces, \"in quotes\" with spaces", Style::new().dim()),
        None => Line::styled("any text, may be empty", Style::new().dim()),
    };
    let lines = vec![
        Line::from([vec![label("Tags")], field(0)].concat()),
        Line::raw(""),
        Line::from([vec![label("Annotation")], field(1)].concat()),
        Line::raw(""),
        Line::styled("The entry starts now.", Style::new().dim()),
        last,
    ];
    let fields = [Rect::new(inner.x + LABEL_W, inner.y, field_w, 1), Rect::new(inner.x + LABEL_W, inner.y + 2, field_w, 1)];
    let active = fields[form.field];
    let cursor = Position::new((active.x + 1 + (form.cursor - form.scroll) as u16).min(active.right().saturating_sub(1)), active.y);
    frame.render_widget(Clear, popup);
    frame.render_widget(Paragraph::new(lines).block(block), popup);
    if active.width > 0 {
        frame.set_cursor_position(cursor);
    }
    app.start_fields = fields;
}

pub(crate) fn draw_range_input(frame: &mut Frame, area: Rect, app: &mut App) {
    let Some(input) = &app.range_input else { return };
    const FIELD_W: u16 = 22;
    let label = |s: &'static str| Span::styled(format!("{s:<6}"), Style::new().fg(ACCENT).bold());
    let field = |text: &str, active: bool| {
        let style = if active { Style::new().add_modifier(Modifier::REVERSED) } else { Style::new().underlined() };
        Span::styled(format!(" {:<w$}", text, w = FIELD_W as usize - 1), style)
    };
    let hint = Style::new().dim();
    let mut lines = vec![
        Line::from(vec![label("From"), field(&input.from, input.field == 0)]),
        Line::raw(""),
        Line::from(vec![label("To"), field(&input.to, input.field == 1)]),
        Line::raw(""),
        Line::styled("YYYY-MM-DD or YYYY-MM-DD HH:MM", hint),
        Line::styled("'To' is inclusive; leave empty for open end", hint),
        Line::styled("'From' may be a hint, e.g. :lastyear", hint),
    ];
    if let Some(err) = &input.error {
        lines.push(Line::raw(""));
        lines.push(Line::styled(err.clone(), Style::new().fg(Color::Red).bold()));
    }

    let w = (46 + PAD.left + PAD.right + 2).min(area.width);
    let h = (lines.len() as u16 + 2 + PAD.top + 1).min(area.height);
    let popup = Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(ACCENT))
        .title(" Date range ")
        .title_bottom(Line::from(" Enter apply · Tab switch · Esc cancel ").right_aligned().dim())
        .padding(Padding::new(PAD.left, PAD.right, PAD.top, 1));
    let inner = block.inner(popup);
    let field_x = inner.x + 6;
    app.range_fields = [Rect::new(field_x, inner.y, FIELD_W, 1), Rect::new(field_x, inner.y + 2, FIELD_W, 1)];
    app.range_popup = popup;

    let active = app.range_fields[input.field];
    let cursor_x = active.x + 1 + input.active_len() as u16;
    frame.render_widget(Clear, popup);
    frame.render_widget(Paragraph::new(lines).block(block), popup);
    frame.set_cursor_position(Position::new(cursor_x.min(active.right() - 1), active.y));
}

pub(crate) fn draw_picker(frame: &mut Frame, area: Rect, app: &mut App) {
    let Some(state) = &mut app.picker else { return };
    let name_w = app.all_tags.iter().map(|(t, _)| t.chars().count()).max().unwrap_or(0).max(8);
    let count_w = app.all_tags.iter().map(|(_, c)| c.to_string().len()).max().unwrap_or(1);
    let total: usize = app.all_tags.iter().map(|(_, c)| c).sum();

    let mut items = vec![ListItem::new(Line::from(vec![
        Span::styled(format!("{:<name_w$}", "All tags"), Style::new().italic()),
        Span::styled(format!("  {:>count_w$}", total), Style::new().dim()),
    ]))];
    for (tag, count) in &app.all_tags {
        let current = app.tag.as_deref() == Some(tag.as_str());
        let marker = if current { Style::new().fg(ACCENT).bold() } else { Style::new() };
        items.push(ListItem::new(Line::from(vec![
            Span::styled(format!("{tag:<name_w$}"), marker),
            Span::styled(format!("  {count:>count_w$}"), Style::new().dim()),
        ])));
    }

    let content_w = (name_w + 2 + count_w + 2) as u16; // + highlight symbol
    let w = (content_w + 2 + PAD.left + PAD.right).max(30).min(area.width);
    let h = (items.len() as u16 + 2 + PAD.top + 1).min(area.height);
    let popup = Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(ACCENT))
        .title(" Filter by tag ")
        .title_bottom(Line::from(" Enter select · ⌫ clear · Esc cancel ").right_aligned().dim())
        .padding(Padding::new(PAD.left, PAD.right, PAD.top, 1));
    app.picker_area = block.inner(popup);
    let list = List::new(items)
        .block(block)
        .highlight_symbol("▸ ")
        .highlight_style(Style::new().add_modifier(Modifier::REVERSED));
    frame.render_widget(Clear, popup);
    frame.render_stateful_widget(list, popup, state);
}

/// The key legend shown by `?`: section headings (`None` key) and key rows.
pub(crate) fn legend_lines() -> Vec<(Option<&'static str>, &'static str)> {
    let h = |t| (None, t);
    let k = |k, t| (Some(k), t);
    vec![
        h("Moving"),
        k("↑ ↓  k j", "previous / next row"),
        k("PgUp PgDn  Space", "page up / down"),
        k("Home End  g G", "first / last row"),
        k("mouse wheel", "scroll"),
        k("click a row", "select it (again: details, narrow window)"),
        h("Periods"),
        k("1", "today"),
        k("2  y", "yesterday"),
        k("3  m", "current month"),
        k("⇧3  § #  M", "pick a month of the shown year"),
        k("4  Y", "current year"),
        k("⇧4  $", "pick one of the last 5 years"),
        k("← →  h l  Tab", "previous / next of the four periods"),
        k("5  c", "your own date range (From / To)"),
        k("right-click 3 / 4", "month / year list"),
        h("Filter by tag"),
        k("6  t", "tag list"),
        h("Panels"),
        k("Enter", "details (narrow window)"),
        k("b", "hours-per-tag chart (narrow window)"),
        k("Esc", "close the chart / details (narrow window)"),
        h("Editing"),
        k("e", "edit start, end, tags and annotation of the entry"),
        k("Tab ↑ ↓", "next / previous field"),
        k("0-9", "overwrite at the cursor (skips - and :)"),
        k("text", "tags: separated by spaces, \"two words\" in quotes"),
        k("text", "annotation: any text, empty removes it"),
        k("← → Home End", "move the cursor"),
        k("⌫ Del  Ctrl+U", "delete / clear the field"),
        k("Enter", "save (overlaps are refused)"),
        k("⇧Enter  Alt+Enter", "save, adjusting overlapping entries"),
        k("Esc", "cancel"),
        k("d", "delete the selected entry (asks first: y deletes)"),
        k("e", "in the delete dialog: delete, extend the previous entry"),
        k("u", "undo the last saved edit, start, stop or delete"),
        h("Tracking"),
        k("s", "start a new entry (asks for tags and annotation)"),
        k("s", "stop the running entry (when one is running)"),
        h("In lists and the date form"),
        k("↑ ↓  j k", "move"),
        k("← →", "change the year (month list)"),
        k("Enter  Space", "select / apply"),
        k("⌫", "back to current month / year, no tag filter"),
        k("Tab", "switch From / To (date form)"),
        k("Ctrl+U", "clear the field (date form)"),
        k("Esc", "close without changes"),
        h("Other"),
        k("r", "reload from disk"),
        k("?", "this legend"),
        k("i", "info: version, license and project page"),
        k("Esc", "reset all filters (current month, all tags)"),
        k("q", "quit"),
        k("Ctrl+C", "quit, from anywhere"),
    ]
}

/// The info dialog (`i`): version, license and project page.
pub(crate) fn draw_info(frame: &mut Frame, area: Rect, app: &App) {
    if !app.info {
        return;
    }
    let label = |s: &'static str| Span::styled(format!("{s:<10}"), Style::new().fg(ACCENT).bold());
    let mut lines = vec![
        Line::from(vec![
            Span::styled("timew-tui ", Style::new().bold()),
            Span::styled(env!("CARGO_PKG_VERSION"), Style::new().fg(Color::Yellow).bold()),
        ]),
        Line::styled(env!("CARGO_PKG_DESCRIPTION"), Style::new().dim()),
        Line::raw(""),
        Line::from(vec![label("Version"), Span::raw(env!("CARGO_PKG_VERSION"))]),
        Line::from(vec![label("Library"), Span::raw(format!("timew-core {}", timew_core::VERSION))]),
        Line::from(vec![label("License"), Span::raw(timew_core::LICENSE)]),
    ];
    for (i, notice) in timew_core::COPYRIGHT.iter().enumerate() {
        let label = if i == 0 { label("Copyright") } else { Span::raw(" ".repeat(10)) };
        lines.push(Line::from(vec![label, Span::raw(notice.trim_start_matches("Copyright "))]));
    }
    lines.push(Line::from(vec![
        label("Project"),
        Span::styled(env!("CARGO_PKG_REPOSITORY"), Style::new().fg(ACCENT).underlined()),
    ]));
    let text_w = lines.iter().map(|l| l.width()).max().unwrap_or(0) as u16;
    let w = (text_w + PAD.left + PAD.right + 2).max(30).min(area.width);
    let h = (lines.len() as u16 + 2 + PAD.top + 1).min(area.height);
    let popup = Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(FOCUS))
        .title(" Info ")
        .title_bottom(Line::from(" any key closes ").right_aligned().dim())
        .padding(Padding::new(PAD.left, PAD.right, PAD.top, 1));
    frame.render_widget(Clear, popup);
    frame.render_widget(Paragraph::new(lines).block(block), popup);
}

pub(crate) fn draw_legend(frame: &mut Frame, area: Rect, app: &mut App) {
    let Some(offset) = app.legend else { return };
    let entries = legend_lines();
    let key_w = entries.iter().filter_map(|(k, _)| k.map(|k| k.chars().count())).max().unwrap_or(0);
    let lines: Vec<Line> = entries
        .iter()
        .map(|(key, text)| match key {
            None => Line::styled(*text, Style::new().fg(ACCENT).bold().underlined()),
            Some(key) => Line::from(vec![
                Span::styled(format!("  {key:<key_w$}  "), Style::new().fg(Color::Yellow).bold()),
                Span::raw(*text),
            ]),
        })
        .collect();

    let text_w = entries.iter().map(|(_, t)| t.chars().count()).max().unwrap_or(0);
    let w = (2 + key_w + 2 + text_w) as u16 + PAD.left + PAD.right + 2;
    let w = w.min(area.width.saturating_sub(2)).max(1);
    let h = (lines.len() as u16 + 2 + PAD.top + 1).min(area.height.saturating_sub(2)).max(1);
    let popup = Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(ACCENT))
        .title(" Keys ")
        .title_bottom(Line::from(" ↑↓ scroll · ?/Esc close ").right_aligned().dim())
        .padding(Padding::new(PAD.left, PAD.right, PAD.top, 1));
    let inner = block.inner(popup);
    app.legend_page = inner.height;
    let max = lines.len().saturating_sub(inner.height as usize) as u16;
    let offset = offset.min(max);
    app.legend = Some(offset);

    frame.render_widget(Clear, popup);
    frame.render_widget(Paragraph::new(lines).scroll((offset, 0)).block(block), popup);
    if max > 0 {
        let mut sb = ScrollbarState::new(max as usize + 1).position(offset as usize);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight).begin_symbol(None).end_symbol(None),
            popup.inner(ratatui::layout::Margin { vertical: 1, horizontal: 0 }),
            &mut sb,
        );
    }
}
