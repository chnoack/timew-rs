//! Tests of the whole TUI: they drive an [`App`] with keys and clicks
//! and check what is drawn, on throwaway databases.

mod chart;
mod completion;
mod edit;
mod filters;
mod help;
mod layout;
mod tracking;

use std::{env, fs};
use std::path::PathBuf;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use ratatui::layout::Position;
use ratatui::style::{Color, Modifier, Style};
use timew_core::summary;
use timew_core::database::Database;
use timew_core::datetime::expand_range_hint;
use timew_core::paths::Paths;
use timew_core::range::Range;

use crate::app::{App, Button};
use crate::filter::{Filter, MONTH_NAMES, month_range, parse_range_input, year_range};
use crate::format::{decimal_hours, format_hm, local_date, parse_hms, range_label, without_seconds};
use crate::forms::{ANNOTATION, TAGS, parse_tags, tags_to_text};
use crate::ui::{ACCENT, FOCUS, PAD, draw};
use crate::ui::chart::{GRAY_COLORS, NO_TAG, TAG_COLORS, TagTotal, bar_colors, stacked_bar, tag_totals};
use crate::ui::table::{GAP_COLOR, gap_after, week_parity};


/// A throwaway database with two intervals today and one yesterday.
fn sample_db(name: &str) -> PathBuf {
    let root = env::temp_dir().join(format!("timew-tui-test-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("data")).unwrap();
    let tz = jiff::tz::TimeZone::system();
    let today = jiff::Zoned::now().date();
    let at = |d: jiff::civil::Date, h: i8| {
        let ts = d.at(h, 0, 0, 0).to_zoned(tz.clone()).unwrap().timestamp();
        timew_core::interval::format_iso(ts.as_second())
    };
    let yesterday = today.yesterday().unwrap();
    let mut lines = vec![
        (yesterday, format!("inc {} - {} # planning", at(yesterday, 8), at(yesterday, 12))),
        (today, format!("inc {} - {} # meetings", at(today, 8), at(today, 10))),
        (today, format!("inc {} - {} # email", at(today, 10), at(today, 11))),
    ];
    lines.sort();
    for (d, line) in lines {
        let file = root.join("data").join(format!("{:04}-{:02}.data", d.year(), d.month()));
        let mut content = fs::read_to_string(&file).unwrap_or_default();
        content.push_str(&line);
        content.push('\n');
        fs::write(file, content).unwrap();
    }
    root
}

fn render(app: &mut App) -> String {
    render_at(app, 90, 24)
}

fn render_at(app: &mut App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| draw(f, app)).unwrap();
    let buf = terminal.backend().buffer().clone();
    (0..buf.area.height)
        .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

fn press(app: &mut App, c: char) {
    app.on_key(KeyEvent {
        code: KeyCode::Char(c),
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    });
}

fn key(app: &mut App, code: KeyCode) {
    app.on_key(KeyEvent { code, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: KeyEventState::NONE });
}

fn local(y: i16, mo: i8, d: i8, h: i8, mi: i8) -> i64 {
    jiff::civil::date(y, mo, d).at(h, mi, 0, 0).to_zoned(jiff::tz::TimeZone::system()).unwrap().timestamp().as_second()
}

fn type_text(app: &mut App, text: &str) {
    for c in text.chars() {
        press(app, c);
    }
}

fn clear_field(app: &mut App) {
    app.on_key(KeyEvent {
        code: KeyCode::Char('u'),
        modifiers: KeyModifiers::CONTROL,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    });
}

fn ctrl_u(app: &mut App) {
    app.on_key(KeyEvent {
        code: KeyCode::Char('u'),
        modifiers: KeyModifiers::CONTROL,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    });
}

/// Today's datafile of a sample database.
fn today_file(root: &std::path::Path) -> String {
    let today = jiff::Zoned::now().date();
    fs::read_to_string(root.join("data").join(format!("{:04}-{:02}.data", today.year(), today.month()))).unwrap()
}

fn today_at(h: i8, m: i8) -> String {
    let tz = jiff::tz::TimeZone::system();
    let ts = jiff::Zoned::now().date().at(h, m, 0, 0).to_zoned(tz).unwrap().timestamp();
    timew_core::interval::format_iso(ts.as_second())
}

fn key_with(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    app.on_key(KeyEvent { code, modifiers, kind: KeyEventKind::Press, state: KeyEventState::NONE });
}

fn footer(app: &mut App) -> String {
    render_at(app, 140, 24).lines().last().unwrap().to_string()
}

/// A database with a single entry yesterday 08-12, so that starting an
/// entry now never overlaps anything.
fn yesterday_db(name: &str) -> PathBuf {
    let root = env::temp_dir().join(format!("timew-tui-test-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("data")).unwrap();
    let tz = jiff::tz::TimeZone::system();
    let y = jiff::Zoned::now().date().yesterday().unwrap();
    let at = |h: i8| timew_core::interval::format_iso(y.at(h, 0, 0, 0).to_zoned(tz.clone()).unwrap().timestamp().as_second());
    let file = root.join("data").join(format!("{:04}-{:02}.data", y.year(), y.month()));
    fs::write(file, format!("inc {} - {} # planning\n", at(8), at(12))).unwrap();
    root
}

fn all_lines(root: &std::path::Path) -> String {
    let mut names: Vec<_> = fs::read_dir(root.join("data")).unwrap().map(|e| e.unwrap().path()).collect();
    names.sort();
    names.iter().filter(|p| p.extension().is_some_and(|e| e == "data") && !p.ends_with("tags.data")).map(|p| fs::read_to_string(p).unwrap()).collect()
}

/// The color of the top-left corner of the panel titled `title`.
fn panel_border(app: &mut App, title: &str) -> Color {
    let mut terminal = Terminal::new(TestBackend::new(140, 24)).unwrap();
    terminal.draw(|f| draw(f, app)).unwrap();
    let buf = terminal.backend().buffer().clone();
    for y in 0..buf.area.height {
        let line: String = (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect();
        if let Some(i) = line.find(title) {
            let col = line[..i].chars().count() as u16;
            let x = (0..col).rev().find(|&x| buf[(x, y)].symbol() == "╭").unwrap();
            return buf[(x, y)].fg;
        }
    }
    panic!("no panel {title}");
}

/// Screen column where `text` starts on line `y`.
fn col_of(buf: &ratatui::buffer::Buffer, y: u16, text: &str) -> u16 {
    let chars: Vec<char> = text.chars().collect();
    (0..buf.area.width)
        .find(|&x| chars.iter().enumerate().all(|(i, c)| x + (i as u16) < buf.area.width && buf[(x + i as u16, y)].symbol() == c.to_string()))
        .unwrap_or_else(|| panic!("'{text}' not on line {y}"))
}

/// Edit the selected entry's end time to `end` and save (with or
/// without adjusting overlaps).
fn edit_end(app: &mut App, end: &str, adjust: bool) {
    press(app, 'e');
    for _ in 0..3 {
        key(app, KeyCode::BackTab); // annotation, tags, end time
    }
    type_text(app, end);
    key_with(app, KeyCode::Enter, if adjust { KeyModifiers::ALT } else { KeyModifiers::NONE });
    assert!(app.edit.is_none(), "{:?}", app.edit.as_ref().and_then(|f| f.error.clone()));
}
