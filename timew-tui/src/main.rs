//! `timew-tui`: a terminal UI for a Timewarrior database.
//!
//!   - open the database from `$TIMEWARRIORDB`, else the standard location
//!   - show a scrollable summary (same rows as `timew summary`)
//!   - quick filters: Day, Yesterday, Month (default), Year, a date range, a tag
//!   - edit the start, end, tags and annotation of an entry (`e`); everything
//!     else is read-only.
//!     Edits go through the `timew-core` library, so overlaps are refused;
//!     `u` undoes the last edit.

use std::env;
use std::io;
use std::path::PathBuf;

use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
    KeyboardEnhancementFlags, MouseButton, MouseEvent, MouseEventKind, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use ratatui::crossterm::execute;
use ratatui::layout::{Alignment, Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Cell, Clear, List, ListItem, ListState, Padding, Paragraph, Row, Scrollbar, ScrollbarOrientation, ScrollbarState, Table,
    TableState,
};
use ratatui::{DefaultTerminal, Frame};

use timew_core::summary::{self, Summary};
use timew_core::database::{Database, Tracked};
use timew_core::datetime::{expand_range_hint, make_local, parse_datetime};
use timew_core::interval::Interval;
use timew_core::paths::Paths;
use timew_core::range::Range;

// ---------------------------------------------------------------------------
// Database location
// ---------------------------------------------------------------------------

/// Resolve the database root like Timewarrior's `paths.cpp`:
///   1. `$TIMEWARRIORDB` if set
///   2. `~/.timewarrior` if it exists (legacy layout)
///   3. `$XDG_DATA_HOME/timewarrior`, defaulting to `~/.local/share/timewarrior`
fn resolve_db_root() -> Result<PathBuf, String> {
    if let Some(db) = env::var_os("TIMEWARRIORDB").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(db));
    }
    let home = env::var_os("HOME").filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(home) = &home {
        let legacy = home.join(".timewarrior");
        if legacy.is_dir() {
            return Ok(legacy);
        }
    }
    let data_home = env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| home.map(|h| h.join(".local").join("share")))
        .ok_or("Cannot locate the database: neither TIMEWARRIORDB nor HOME is set.")?;
    Ok(data_home.join("timewarrior"))
}

// ---------------------------------------------------------------------------
// Filters
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Filter {
    Day,
    Yesterday,
    Month,
    Year,
    /// A user-entered date range (`App::custom_range`).
    Custom,
}

impl Filter {
    /// The preset filters (quick buttons 1-4, cycled with ←→).
    const ALL: [Filter; 4] = [Filter::Day, Filter::Yesterday, Filter::Month, Filter::Year];

    fn label(self) -> &'static str {
        match self {
            Filter::Day => "Day",
            Filter::Yesterday => "Yesterday",
            Filter::Month => "Month",
            Filter::Year => "Year",
            Filter::Custom => "Range",
        }
    }

    /// The `:hint` a preset filter expands to (None for `Custom`).
    fn hint(self) -> Option<&'static str> {
        match self {
            Filter::Day => Some("day"),
            Filter::Yesterday => Some("yesterday"),
            Filter::Month => Some("month"),
            Filter::Year => Some("year"),
            Filter::Custom => None,
        }
    }

    fn cycle(self, delta: isize) -> Filter {
        let n = Filter::ALL.len() as isize;
        let Some(i) = Filter::ALL.iter().position(|f| *f == self) else {
            // from a custom range, step onto the first/last preset
            return if delta > 0 { Filter::ALL[0] } else { Filter::ALL[Filter::ALL.len() - 1] };
        };
        Filter::ALL[(i as isize + delta).rem_euclid(n) as usize]
    }
}

// ---------------------------------------------------------------------------
// Custom date range input
// ---------------------------------------------------------------------------

/// State of the open "date range" form.
#[derive(Debug, Clone, Default)]
struct RangeInput {
    from: String,
    to: String,
    /// 0 = From, 1 = To.
    field: usize,
    error: Option<String>,
}

impl RangeInput {
    fn active(&mut self) -> &mut String {
        if self.field == 0 { &mut self.from } else { &mut self.to }
    }

    fn active_len(&self) -> usize {
        if self.field == 0 { self.from.chars().count() } else { self.to.chars().count() }
    }
}

/// State of the edit form (`e`), shown in the details panel: the start
/// and end of the selected entry as date and time fields.
#[derive(Debug, Clone)]
struct EditForm {
    /// The entry as it is stored.
    original: Interval,
    id: u32,
    /// Start date, start time, end date, end time, tags, annotation.
    fields: [String; FIELDS],
    /// The fields as first shown; unchanged ones keep their exact value
    /// (times their seconds).
    initial: [String; FIELDS],
    /// Index into `fields` of the field being typed in.
    field: usize,
    /// Cursor position (in characters) in that field.
    cursor: usize,
    /// First visible character of the active text field (tags and
    /// annotation scroll sideways).
    scroll: usize,
    error: Option<String>,
}

/// Index of the tags field; the fields from here on are free text.
const TAGS: usize = 4;
const ANNOTATION: usize = 5;
const FIELDS: usize = 6;
/// Screen widths of the date and time fields: a leading space, the text,
/// room for the cursor. The tags field takes the rest of the line.
const EDIT_FIELD_WIDTHS: [u16; 4] = [12, 10, 12, 10];
const MAX_TEXT_LEN: usize = 500;

/// Tags and annotation are text fields: typing inserts, they scroll.
fn is_text(field: usize) -> bool {
    field >= TAGS
}

/// Tags as one line of text: separated by spaces, a tag with a space or a
/// quote in double quotes.
fn tags_to_text(tags: &[String]) -> String {
    tags.iter()
        .map(|t| if t.contains([' ', '"']) { format!("\"{}\"", t.replace('"', "\\\"")) } else { t.clone() })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The reverse of `tags_to_text`: words, or "quoted words" (with `\"` for
/// a quote inside).
fn parse_tags(text: &str) -> Result<Vec<String>, String> {
    let mut tags = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        let mut tag = String::new();
        if c == '"' {
            chars.next();
            loop {
                match chars.next() {
                    None => return Err("Tags: a quote is not closed.".into()),
                    Some('\\') if chars.peek() == Some(&'"') => tag.push(chars.next().unwrap()),
                    Some('"') => break,
                    Some(ch) => tag.push(ch),
                }
            }
        } else {
            while let Some(&ch) = chars.peek() {
                if ch.is_whitespace() {
                    break;
                }
                tag.push(ch);
                chars.next();
            }
        }
        if !tag.is_empty() {
            tags.push(tag);
        }
    }
    Ok(tags)
}

fn is_separator(c: char) -> bool {
    c == '-' || c == ':'
}

impl EditForm {
    fn new(t: &Tracked) -> Self {
        let iv = &t.interval;
        let date = |e: i64| local_date(e);
        let time = |e: i64| jiff::Timestamp::from_second(e).unwrap().to_zoned(jiff::tz::TimeZone::system()).strftime("%H:%M").to_string();
        let fields = [
            date(iv.range.start),
            time(iv.range.start),
            if iv.is_open() { String::new() } else { date(iv.range.end) },
            if iv.is_open() { String::new() } else { time(iv.range.end) },
            tags_to_text(&iv.tags),
            iv.annotation.clone().unwrap_or_default(),
        ];
        EditForm { original: iv.clone(), id: t.id, initial: fields.clone(), fields, field: 0, cursor: 0, scroll: 0, error: None }
    }

    fn len(&self) -> usize {
        self.fields[self.field].chars().count()
    }

    /// Go to field `i`, with the cursor at its start (or end).
    fn focus(&mut self, i: usize, at_end: bool) {
        self.field = i % FIELDS;
        self.cursor = if at_end { self.len() } else { 0 };
    }

    /// Type `c` at the cursor. Over existing text it replaces the character
    /// under the cursor, stepping over the `-`/`:` separators, so a date or
    /// time can be retyped digit by digit; at the end it appends.
    /// In the tags and annotation fields, it inserts like an ordinary text
    /// field.
    fn type_char(&mut self, c: char) {
        let mut chars: Vec<char> = self.fields[self.field].chars().collect();
        if is_text(self.field) {
            if chars.len() < MAX_TEXT_LEN {
                chars.insert(self.cursor.min(chars.len()), c);
                self.cursor += 1;
                self.fields[self.field] = chars.into_iter().collect();
            }
            self.error = None;
            return;
        }
        let max = EDIT_FIELD_WIDTHS[self.field] as usize - 2;
        if !is_separator(c) && chars.get(self.cursor).copied().is_some_and(is_separator) {
            self.cursor += 1;
        }
        if self.cursor < chars.len() {
            chars[self.cursor] = c;
        } else if chars.len() < max {
            chars.push(c);
        } else {
            return;
        }
        self.cursor += 1;
        if chars.get(self.cursor).copied().is_some_and(is_separator) && !is_separator(c) {
            self.cursor += 1;
        }
        self.fields[self.field] = chars.into_iter().collect();
        self.error = None;
    }

    fn remove(&mut self, at: usize) {
        let mut chars: Vec<char> = self.fields[self.field].chars().collect();
        if at < chars.len() {
            chars.remove(at);
            self.fields[self.field] = chars.into_iter().collect();
        }
        self.error = None;
    }

    /// The entry with the start, end, tags and annotation from the fields.
    fn edited(&self) -> Result<Interval, String> {
        let f: Vec<&str> = self.fields.iter().map(|s| s.trim()).collect();
        let unchanged = |a: usize, b: usize| f[a] == self.initial[a].trim() && f[b] == self.initial[b].trim();
        let mut edited = self.original.clone();
        if !unchanged(0, 1) {
            edited.range.start = parse_date_time(f[0], f[1], "Start")?;
        }
        if f[2].is_empty() && f[3].is_empty() {
            if !self.original.is_open() {
                return Err("Enter the end date and time.".into());
            }
            edited.range.end = 0;
        } else if !unchanged(2, 3) {
            // an empty end date means the day the entry starts
            let date = if f[2].is_empty() { f[0] } else { f[2] };
            edited.range.end = parse_date_time(date, f[3], "End")?;
        }
        if edited.range.is_ended() && edited.range.end <= edited.range.start {
            return Err("The end must be after the start.".into());
        }
        if self.fields[TAGS] != self.initial[TAGS] {
            edited.tags.clear();
            for tag in parse_tags(&self.fields[TAGS])? {
                edited.add_tag(tag);
            }
        }
        if self.fields[ANNOTATION] != self.initial[ANNOTATION] {
            let text = self.fields[ANNOTATION].trim();
            edited.annotation = if text.is_empty() { None } else { Some(text.to_string()) };
        }
        Ok(edited)
    }
}

/// `YYYY-MM-DD` and `HH:MM[:SS]` (local time) as an instant.
fn parse_date_time(date: &str, time: &str, what: &str) -> Result<i64, String> {
    let d = jiff::civil::Date::strptime("%Y-%m-%d", date).map_err(|_| format!("{what} date: use YYYY-MM-DD."))?;
    let parts: Vec<&str> = time.split(':').collect();
    let num = |s: &str, max: i64| s.parse::<i64>().ok().filter(|v| (0..=max).contains(v) && !s.is_empty() && s.len() <= 2);
    let bad = || format!("{what} time: use HH:MM.");
    let (h, m, sec) = match parts.as_slice() {
        [h, m] => (num(h, 23).ok_or_else(bad)?, num(m, 59).ok_or_else(bad)?, 0),
        [h, m, sec] => (num(h, 23).ok_or_else(bad)?, num(m, 59).ok_or_else(bad)?, num(sec, 59).ok_or_else(bad)?),
        _ => return Err(bad()),
    };
    Ok(make_local(d.year() as i64, d.month() as i64, d.day() as i64, h, m, sec))
}

/// Accept `YYYY-MM-DD`, `YYYY-MM-DD HH:MM[:SS]` and `YYYY-MM-DDTHH:MM[:SS]`
/// and normalize to the `YYYY-MM-DD[THH:MM:SS]` form `parse_datetime` takes.
fn normalize_datetime(input: &str) -> String {
    let s = input.trim().replacen(' ', "T", 1);
    match s.split_once('T') {
        Some((date, time)) if time.len() == 5 => format!("{date}T{time}:00"),
        _ => s,
    }
}

/// Parse the form's From/To into a range. `To` given as a bare date is
/// inclusive (the range ends at the following midnight); an empty `To`
/// leaves the range open-ended. `From` may also be a `:hint` such as
/// `:lastyear` or `:week` (then `To` must be empty).
fn parse_range_input(from: &str, to: &str) -> Result<Range, String> {
    let (from, to) = (from.trim(), to.trim());
    if let Some(hint) = from.strip_prefix(':') {
        if !to.is_empty() {
            return Err("Leave 'To' empty when using a :hint.".into());
        }
        return expand_range_hint(hint).ok_or_else(|| format!("Unknown range hint ':{hint}'."));
    }
    if from.is_empty() {
        return Err("Enter a start date (YYYY-MM-DD).".into());
    }
    let start = parse_datetime(&normalize_datetime(from))?;
    if to.is_empty() {
        return Ok(Range::new(start, 0));
    }
    let to_norm = normalize_datetime(to);
    let mut end = parse_datetime(&to_norm)?;
    if !to_norm.contains('T') {
        // bare date: include the whole day
        let tz = jiff::tz::TimeZone::system();
        let day = jiff::Timestamp::from_second(end).unwrap().to_zoned(tz.clone()).date();
        end = day.tomorrow().unwrap().to_zoned(tz).map_err(|e| e.to_string())?.timestamp().as_second();
    }
    if end <= start {
        return Err("'To' must be after 'From'.".into());
    }
    Ok(Range::new(start, end))
}

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

/// State of the open month list (`Shift+3`).
struct MonthPicker {
    year: i16,
    state: ListState,
}

const MONTH_NAMES: [&str; 12] = [
    "January", "February", "March", "April", "May", "June",
    "July", "August", "September", "October", "November", "December",
];

/// State of the open year list (`Shift+4`).
struct YearPicker {
    /// Selectable years, newest first.
    years: Vec<i16>,
    state: ListState,
}

/// Local-time range covering the calendar year `year`.
fn year_range(year: i16) -> Range {
    Range::new(month_range(year, 1).start, month_range(year, 12).end)
}

/// Local-time range covering month `m` (1-12) of `year`.
fn month_range(year: i16, m: i8) -> Range {
    let tz = jiff::tz::TimeZone::system();
    let start = jiff::civil::date(year, m, 1);
    let end = start.checked_add(jiff::Span::new().months(1)).unwrap();
    let epoch = |d: jiff::civil::Date| d.to_zoned(tz.clone()).unwrap().timestamp().as_second();
    Range::new(epoch(start), epoch(end))
}

/// A clickable button in the filter bar.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Button {
    Range(Filter),
    CustomRange,
    Tag,
}

struct App {
    db_root: PathBuf,
    /// Where `timewarrior.cfg` is (read when saving an edit).
    config_dir: PathBuf,
    /// Open edit form (`e`) for the selected entry.
    edit: Option<EditForm>,
    /// Screen areas of the edit form's fields from the last draw.
    edit_fields: [Rect; FIELDS],
    /// A message for the status line, cleared by the next key.
    notice: Option<String>,
    /// Whether the notice reports a failure (red instead of green).
    notice_failed: bool,
    /// The last saved edit, for `u`: the entry before the edit and every
    /// change the save made (including adjusted entries). Only in memory.
    last_edit: Option<(Interval, Vec<timew_core::session::Change>)>,
    filter: Filter,
    range: Range,
    /// The range used by `Filter::Custom`, and the text it was entered as.
    custom_range: Range,
    custom_input: (String, String),
    /// Open date-range form.
    range_input: Option<RangeInput>,
    /// Screen areas of the From/To fields from the last draw (for mouse clicks).
    range_fields: [Rect; 2],
    range_popup: Rect,
    /// Only show intervals carrying this tag (None = all tags).
    tag: Option<String>,
    /// Every tag in the whole database with its interval count, sorted by name.
    all_tags: Vec<(String, usize)>,
    /// Open tag picker (entry 0 is "All tags", entry i+1 is `all_tags[i]`).
    picker: Option<ListState>,
    /// Screen area of the tag picker list from the last draw (for mouse clicks).
    picker_area: Rect,
    /// Month shown by `Filter::Month` (None = the current month).
    month: Option<(i16, i8)>,
    /// Open month picker.
    month_picker: Option<MonthPicker>,
    /// Year shown by `Filter::Year` (None = the current year).
    year: Option<i16>,
    /// Open year picker.
    year_picker: Option<YearPicker>,
    /// Screen area of the year picker list from the last draw.
    year_picker_area: Rect,
    /// Screen area of the month picker list from the last draw.
    month_picker_area: Rect,
    /// Intervals of the current range; `summary::Row::source` indexes this.
    tracked: Vec<Tracked>,
    summary: Summary,
    error: Option<String>,
    table: TableState,
    /// Whether the bottom details panel is open (narrow layout only; the
    /// side panel in the wide layout is always shown).
    details: bool,
    /// Whether the last draw used the wide (side-by-side) layout.
    wide: bool,
    /// Whether the hours-per-tag bar chart is shown (in place of the details).
    chart: bool,
    /// Whether the info dialog (`i`) is open.
    info: bool,
    /// Open key legend (`?`), with its scroll offset in lines.
    legend: Option<u16>,
    /// Lines of the legend that fit on screen at the last draw (page size).
    legend_page: u16,
    /// Screen area of the summary table from the last draw (for mouse clicks).
    table_area: Rect,
    /// Screen areas of the filter buttons from the last draw (for mouse clicks).
    buttons: Vec<(Rect, Button)>,
    /// Number of data rows visible in the table at the last draw (page size).
    page: usize,
    quit: bool,
}

impl App {
    fn new(db_root: PathBuf) -> Self {
        let config_dir = if env::var_os("TIMEWARRIORDB").is_some_and(|v| !v.is_empty()) {
            db_root.clone()
        } else {
            Paths::from_env().config_dir
        };
        let mut app = App {
            db_root,
            config_dir,
            edit: None,
            edit_fields: [Rect::default(); FIELDS],
            notice: None,
            notice_failed: false,
            last_edit: None,
            filter: Filter::Month,
            range: Range::empty(),
            custom_range: Range::empty(),
            custom_input: (String::new(), String::new()),
            range_input: None,
            range_fields: [Rect::default(); 2],
            range_popup: Rect::default(),
            tag: None,
            all_tags: Vec::new(),
            picker: None,
            picker_area: Rect::default(),
            month: None,
            month_picker: None,
            year: None,
            year_picker: None,
            year_picker_area: Rect::default(),
            month_picker_area: Rect::default(),
            tracked: Vec::new(),
            summary: Summary { rows: Vec::new(), grand_total: 0 },
            error: None,
            table: TableState::default(),
            details: false,
            wide: false,
            chart: false,
            info: false,
            legend: None,
            legend_page: 10,
            table_area: Rect::default(),
            buttons: Vec::new(),
            page: 10,
            quit: false,
        };
        app.load();
        app
    }

    /// (Re)read the database for the current filter. Opens the database
    /// fresh each time so external changes are picked up on reload.
    fn load(&mut self) {
        self.range = match (self.filter, self.month) {
            (Filter::Month, Some((y, m))) => month_range(y, m),
            (Filter::Year, _) if self.year.is_some() => year_range(self.year.unwrap()),
            (f, _) => match f.hint() {
                Some(hint) => expand_range_hint(hint).expect("built-in hint"),
                None => self.custom_range,
            },
        };
        let result = Database::open(self.db_root.join("data")).map_err(|e| e.to_string()).and_then(|mut db| {
            let mut counts = std::collections::BTreeMap::<String, usize>::new();
            for t in db.all_tracked()? {
                for tag in t.interval.tags {
                    *counts.entry(tag).or_default() += 1;
                }
            }
            self.all_tags = counts.into_iter().collect();
            let mut tracked = db.tracked_in_range(&self.range)?;
            if let Some(tag) = &self.tag {
                tracked.retain(|t| t.interval.has_tag(tag));
            }
            Ok(tracked)
        });
        match result {
            Ok(tracked) => {
                self.summary = summary::build(&self.range, &tracked);
                self.tracked = tracked;
                self.error = None;
            }
            Err(e) => {
                self.tracked = Vec::new();
                self.summary = Summary { rows: Vec::new(), grand_total: 0 };
                self.error = Some(e);
            }
        }
        self.table = TableState::default();
        if !self.summary.rows.is_empty() {
            self.table.select(Some(0));
        } else {
            self.details = false;
        }
    }

    /// Keys go to the summary table: no form, list or legend is open.
    fn table_has_focus(&self) -> bool {
        self.edit.is_none()
            && !self.info
            && self.legend.is_none()
            && self.picker.is_none()
            && self.month_picker.is_none()
            && self.year_picker.is_none()
            && self.range_input.is_none()
    }

    /// The interval behind the currently selected summary row.
    fn selected(&self) -> Option<&Tracked> {
        let row = self.summary.rows.get(self.table.selected()?)?;
        self.tracked.get(row.source)
    }

    fn toggle_details(&mut self) {
        if !self.wide {
            if self.chart {
                // the bottom panel shows the chart: switch it to the details
                self.chart = false;
                self.details = self.selected().is_some();
            } else {
                self.details = !self.details && self.selected().is_some();
            }
        }
    }

    /// Edit the start and end of the selected entry in the details panel.
    fn open_edit(&mut self) {
        let Some(t) = self.selected() else { return };
        self.edit = Some(EditForm::new(t));
        if !self.wide {
            self.chart = false;
            self.details = true;
        }
    }

    fn on_edit_key(&mut self, key: KeyEvent) {
        let Some(form) = &mut self.edit else { return };
        match key.code {
            KeyCode::Esc => self.edit = None,
            KeyCode::Tab | KeyCode::Down => form.focus(form.field + 1, false),
            KeyCode::BackTab | KeyCode::Up => form.focus(form.field + FIELDS - 1, false),
            // the cursor moves into the neighboring field at either end
            KeyCode::Left if form.cursor == 0 => form.focus(form.field + FIELDS - 1, true),
            KeyCode::Left => form.cursor -= 1,
            KeyCode::Right if form.cursor >= form.len() => form.focus(form.field + 1, false),
            KeyCode::Right => form.cursor += 1,
            KeyCode::Home => form.cursor = 0,
            KeyCode::End => form.cursor = form.len(),
            KeyCode::Backspace if form.cursor > 0 => {
                form.cursor -= 1;
                form.remove(form.cursor);
            }
            KeyCode::Delete => form.remove(form.cursor),
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                form.fields[form.field].clear();
                form.cursor = 0;
            }
            KeyCode::Char(c) => form.type_char(c),
            // Shift+Enter (where the terminal reports it) or Alt+Enter: save and
            // adjust overlapping entries, like `timew modify … :adjust`
            KeyCode::Enter if key.modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) => self.save_edit(true),
            KeyCode::Enter => self.save_edit(false),
            _ => {}
        }
    }

    fn save_edit(&mut self, adjust: bool) {
        let Some(form) = &mut self.edit else { return };
        let edited = match form.edited() {
            Ok(e) => e,
            Err(e) => return form.error = Some(e),
        };
        let mut paths = Paths::from_env();
        paths.db_dir = self.db_root.clone();
        paths.config_dir = self.config_dir.clone();
        let (adjusted, actions) = match timew_core::modify_interval(&paths, &form.original, &edited, adjust) {
            Ok(result) => result,
            Err(e) if e.starts_with("You cannot overlap") => {
                return form.error = Some("Overlaps another entry. Shift+Enter adjusts it.".into());
            }
            Err(e) => return form.error = Some(e),
        };
        let changed = !actions.is_empty();
        if changed {
            self.last_edit = Some((form.original.clone(), actions));
        }
        self.edit = None;
        self.load();
        let row = self.select_interval(&edited);
        let others = match adjusted {
            0 => String::new(),
            1 => " 1 overlapping entry adjusted.".into(),
            n => format!(" {n} overlapping entries adjusted."),
        };
        self.notice = Some(match (changed, row) {
            (false, _) => "No changes.".into(),
            (true, Some(_)) => format!("Saved.{others} u undoes it."),
            (true, None) => format!("Saved; the entry is now outside the shown period.{others} u undoes it."),
        });
        self.notice_failed = false;
    }

    /// Select the row of `interval`, if it is shown. Returns the row.
    fn select_interval(&mut self, interval: &Interval) -> Option<usize> {
        let row = self.summary.rows.iter().position(|r| self.tracked[r.source].interval == *interval);
        if let Some(i) = row {
            self.table.select(Some(i));
        }
        row
    }

    /// Revert the last saved edit (`u`). Only the last one can be undone.
    fn undo_edit(&mut self) {
        let Some((before, changes)) = self.last_edit.take() else { return };
        let mut paths = Paths::from_env();
        paths.db_dir = self.db_root.clone();
        paths.config_dir = self.config_dir.clone();
        match timew_core::undo_change(&paths, &changes) {
            Ok(()) => {
                self.load();
                self.select_interval(&before);
                self.notice = Some("Undone.".to_string());
                self.notice_failed = false;
            }
            Err(e) => {
                self.notice = Some(format!("Not undone: {e}"));
                self.notice_failed = true;
                self.load();
            }
        }
    }

    fn open_range_input(&mut self) {
        let (from, to) = if self.filter == Filter::Custom {
            self.custom_input.clone()
        } else {
            // prefill with the current range, end shown inclusive
            let date = |e: i64| local_date(e);
            (date(self.range.start), if self.range.is_ended() { date(self.range.end - 1) } else { String::new() })
        };
        self.range_input = Some(RangeInput { from, to, field: 0, error: None });
    }

    fn on_range_key(&mut self, key: KeyEvent) {
        let Some(input) = &mut self.range_input else { return };
        match key.code {
            KeyCode::Esc => self.range_input = None,
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down => input.field = 1 - input.field,
            KeyCode::Backspace => {
                input.active().pop();
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => input.active().clear(),
            KeyCode::Char(c) => {
                input.active().push(c);
                input.error = None;
            }
            KeyCode::Enter => match parse_range_input(&input.from, &input.to) {
                Ok(range) => {
                    self.custom_input = (input.from.trim().to_string(), input.to.trim().to_string());
                    self.custom_range = range;
                    self.range_input = None;
                    self.filter = Filter::Custom;
                    self.load();
                }
                Err(e) => input.error = Some(e),
            },
            _ => {}
        }
    }

    fn open_picker(&mut self) {
        let current = self
            .tag
            .as_ref()
            .and_then(|t| self.all_tags.iter().position(|(name, _)| name == t))
            .map_or(0, |i| i + 1);
        self.picker = Some(ListState::default().with_selected(Some(current)));
    }

    /// Apply picker entry `i` (0 = all tags) and close the picker.
    fn pick(&mut self, i: usize) {
        self.picker = None;
        let tag = i.checked_sub(1).and_then(|i| self.all_tags.get(i)).map(|(t, _)| t.clone());
        if tag != self.tag {
            self.tag = tag;
            self.load();
        }
    }

    fn picker_move(&mut self, delta: isize) {
        let len = self.all_tags.len() as isize + 1;
        if let Some(state) = &mut self.picker {
            let cur = state.selected().unwrap_or(0) as isize;
            state.select(Some((cur + delta).clamp(0, len - 1) as usize));
        }
    }

    fn on_picker_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('t') | KeyCode::Char('6') => self.picker = None,
            KeyCode::Down | KeyCode::Char('j') => self.picker_move(1),
            KeyCode::Up | KeyCode::Char('k') => self.picker_move(-1),
            KeyCode::PageDown => self.picker_move(10),
            KeyCode::PageUp => self.picker_move(-10),
            KeyCode::Home | KeyCode::Char('g') => self.picker_move(isize::MIN / 2),
            KeyCode::End | KeyCode::Char('G') => self.picker_move(isize::MAX / 2),
            KeyCode::Backspace => self.pick(0), // clear the tag filter
            KeyCode::Enter | KeyCode::Char(' ') => {
                let i = self.picker.as_ref().and_then(ListState::selected).unwrap_or(0);
                self.pick(i);
            }
            _ => {}
        }
    }

    /// Back to the start view: the current month, all tags (`Esc`).
    fn reset_filters(&mut self) {
        let default = self.filter == Filter::Month && self.month.is_none() && self.year.is_none() && self.tag.is_none();
        if default {
            return;
        }
        self.filter = Filter::Month;
        self.month = None;
        self.year = None;
        self.tag = None;
        self.load();
        self.notice = Some("All filters reset: current month, all tags.".to_string());
        self.notice_failed = false;
    }

    fn set_filter(&mut self, filter: Filter) {
        // choosing Month/Year directly always means the current month/year
        if filter != self.filter
            || (filter == Filter::Month && self.month.is_some())
            || (filter == Filter::Year && self.year.is_some())
        {
            self.filter = filter;
            self.month = None;
            self.year = None;
            self.load();
        }
    }

    /// Open the list of the last 5 years (current year first).
    fn open_year_picker(&mut self) {
        let this_year = jiff::Zoned::now().year();
        let years: Vec<i16> = (0..5).map(|i| this_year - i).collect();
        let shown = jiff::Timestamp::from_second(self.range.start).unwrap().to_zoned(jiff::tz::TimeZone::system()).year();
        let selected = years.iter().position(|y| *y == shown).unwrap_or(0);
        self.year_picker = Some(YearPicker { years, state: ListState::default().with_selected(Some(selected)) });
    }

    fn pick_year(&mut self, i: usize) {
        if let Some(p) = self.year_picker.take() {
            self.filter = Filter::Year;
            self.month = None;
            self.year = p.years.get(i).copied();
            self.load();
        }
    }

    fn on_year_picker_key(&mut self, key: KeyEvent) {
        let Some(p) = &mut self.year_picker else { return };
        let last = p.years.len() as isize - 1;
        let cur = p.state.selected().unwrap_or(0) as isize;
        let mut go = |d: isize| p.state.select(Some((cur + d).clamp(0, last) as usize));
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.year_picker = None,
            KeyCode::Down | KeyCode::Char('j') => go(1),
            KeyCode::Up | KeyCode::Char('k') => go(-1),
            KeyCode::Home | KeyCode::Char('g') => go(-last - 1),
            KeyCode::End | KeyCode::Char('G') => go(last + 1),
            KeyCode::Enter | KeyCode::Char(' ') => self.pick_year(cur as usize),
            // like pressing 4: back to the current year
            KeyCode::Backspace => {
                self.year_picker = None;
                self.set_filter(Filter::Year);
            }
            _ => {}
        }
    }

    /// Open the month list for the year currently shown.
    fn open_month_picker(&mut self) {
        let shown = jiff::Timestamp::from_second(self.range.start).unwrap().to_zoned(jiff::tz::TimeZone::system());
        let year = shown.year();
        let selected = shown.month() as usize - 1;
        self.month_picker = Some(MonthPicker { year, state: ListState::default().with_selected(Some(selected)) });
    }

    fn month_picker_year(&mut self, delta: i16) {
        if let Some(p) = &mut self.month_picker {
            p.year += delta;
        }
    }

    fn pick_month(&mut self, m: usize) {
        if let Some(p) = self.month_picker.take() {
            self.filter = Filter::Month;
            self.month = Some((p.year, m as i8 + 1));
            self.load();
        }
    }

    fn on_month_picker_key(&mut self, key: KeyEvent) {
        let Some(p) = &mut self.month_picker else { return };
        let cur = p.state.selected().unwrap_or(0) as isize;
        let mut go = |d: isize| p.state.select(Some((cur + d).clamp(0, 11) as usize));
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.month_picker = None,
            KeyCode::Down | KeyCode::Char('j') => go(1),
            KeyCode::Up | KeyCode::Char('k') => go(-1),
            KeyCode::Home | KeyCode::Char('g') => go(-12),
            KeyCode::End | KeyCode::Char('G') => go(12),
            KeyCode::Left | KeyCode::Char('h') => self.month_picker_year(-1),
            KeyCode::Right | KeyCode::Char('l') => self.month_picker_year(1),
            KeyCode::Enter | KeyCode::Char(' ') => self.pick_month(cur as usize),
            // like pressing 3: back to the current month
            KeyCode::Backspace => {
                self.month_picker = None;
                self.set_filter(Filter::Month);
            }
            _ => {}
        }
    }

    fn scroll_by(&mut self, delta: isize) {
        let len = self.summary.rows.len();
        if len == 0 {
            return;
        }
        let cur = self.table.selected().unwrap_or(0) as isize;
        let next = (cur + delta).clamp(0, len as isize - 1) as usize;
        self.table.select(Some(next));
    }

    fn on_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.quit = true;
            return;
        }
        self.notice = None;
        if self.edit.is_some() {
            return self.on_edit_key(key);
        }
        if self.info {
            // any key closes the info dialog
            self.info = false;
            return;
        }
        if self.legend.is_some() {
            return self.on_legend_key(key);
        }
        if self.picker.is_some() {
            return self.on_picker_key(key);
        }
        if self.month_picker.is_some() {
            return self.on_month_picker_key(key);
        }
        if self.year_picker.is_some() {
            return self.on_year_picker_key(key);
        }
        if self.range_input.is_some() {
            return self.on_range_key(key);
        }
        let page = self.page.max(1) as isize;
        match key.code {
            KeyCode::Esc if self.chart && !self.wide => self.chart = false,
            KeyCode::Esc if self.details && !self.wide => self.details = false,
            // the wide layout always shows the chart; b toggles it in the narrow one
            KeyCode::Char('b') if !self.wide => self.chart = !self.chart,
            KeyCode::Enter => self.toggle_details(),
            KeyCode::Char('i') => self.info = true,
            KeyCode::Char('e') => self.open_edit(),
            KeyCode::Char('u') => self.undo_edit(),
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Esc => self.reset_filters(),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => self.quit = true,
            KeyCode::Down | KeyCode::Char('j') => self.scroll_by(1),
            KeyCode::Up | KeyCode::Char('k') => self.scroll_by(-1),
            KeyCode::PageDown | KeyCode::Char(' ') => self.scroll_by(page),
            KeyCode::PageUp => self.scroll_by(-page),
            KeyCode::Home | KeyCode::Char('g') => self.scroll_by(isize::MIN / 2),
            KeyCode::End | KeyCode::Char('G') => self.scroll_by(isize::MAX / 2),
            KeyCode::Char('1') | KeyCode::Char('d') => self.set_filter(Filter::Day),
            KeyCode::Char('2') | KeyCode::Char('y') => self.set_filter(Filter::Yesterday),
            // Shift+3: '§' on German, '#' on US layouts (or '3' + SHIFT where reported)
            KeyCode::Char('§') | KeyCode::Char('#') | KeyCode::Char('M') => self.open_month_picker(),
            KeyCode::Char('3') if key.modifiers.contains(KeyModifiers::SHIFT) => self.open_month_picker(),
            KeyCode::Char('3') | KeyCode::Char('m') => self.set_filter(Filter::Month),
            // Shift+4: '$' on German and US layouts (or '4' + SHIFT where reported)
            KeyCode::Char('$') => self.open_year_picker(),
            KeyCode::Char('4') if key.modifiers.contains(KeyModifiers::SHIFT) => self.open_year_picker(),
            KeyCode::Char('4') | KeyCode::Char('Y') => self.set_filter(Filter::Year),
            KeyCode::Right | KeyCode::Tab | KeyCode::Char('l') => self.set_filter(self.filter.cycle(1)),
            KeyCode::Left | KeyCode::BackTab | KeyCode::Char('h') => self.set_filter(self.filter.cycle(-1)),
            KeyCode::Char('5') | KeyCode::Char('c') => self.open_range_input(),
            KeyCode::Char('6') | KeyCode::Char('t') => self.open_picker(),
            KeyCode::Char('?') => self.legend = Some(0),
            KeyCode::Char('r') => self.load(),
            _ => {}
        }
    }

    fn legend_scroll(&mut self, delta: i32) {
        if let Some(offset) = &mut self.legend {
            let max = legend_lines().len().saturating_sub(self.legend_page as usize) as i32;
            *offset = (*offset as i32 + delta).clamp(0, max) as u16;
        }
    }

    fn on_legend_key(&mut self, key: KeyEvent) {
        let page = self.legend_page.max(1) as i32;
        match key.code {
            KeyCode::Char('?') | KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => self.legend = None,
            KeyCode::Down | KeyCode::Char('j') => self.legend_scroll(1),
            KeyCode::Up | KeyCode::Char('k') => self.legend_scroll(-1),
            KeyCode::PageDown | KeyCode::Char(' ') => self.legend_scroll(page),
            KeyCode::PageUp => self.legend_scroll(-page),
            KeyCode::Home | KeyCode::Char('g') => self.legend_scroll(i32::MIN / 2),
            KeyCode::End | KeyCode::Char('G') => self.legend_scroll(i32::MAX / 2),
            _ => {}
        }
    }

    fn on_mouse(&mut self, mouse: MouseEvent) {
        let pos = Position::new(mouse.column, mouse.row);
        if self.info {
            if let MouseEventKind::Down(_) = mouse.kind {
                self.info = false;
            }
            return;
        }
        if let Some(form) = &mut self.edit {
            if let MouseEventKind::Down(MouseButton::Left) = mouse.kind
                && let Some(i) = self.edit_fields.iter().position(|r| r.contains(pos))
            {
                let scroll = if i == form.field && is_text(i) { form.scroll } else { 0 };
                form.field = i;
                form.cursor = (scroll + pos.x.saturating_sub(self.edit_fields[i].x + 1) as usize).min(form.len());
            }
            return;
        }
        if self.legend.is_some() {
            match mouse.kind {
                MouseEventKind::ScrollDown => self.legend_scroll(3),
                MouseEventKind::ScrollUp => self.legend_scroll(-3),
                MouseEventKind::Down(_) => self.legend = None,
                _ => {}
            }
            return;
        }
        if self.picker.is_some() {
            match mouse.kind {
                MouseEventKind::ScrollDown => self.picker_move(1),
                MouseEventKind::ScrollUp => self.picker_move(-1),
                MouseEventKind::Down(MouseButton::Left) => {
                    let a = self.picker_area;
                    if a.contains(pos) {
                        let offset = self.picker.as_ref().map_or(0, ListState::offset);
                        let i = offset + (pos.y - a.y) as usize;
                        if i <= self.all_tags.len() {
                            self.pick(i);
                        }
                    } else {
                        self.picker = None; // click outside closes
                    }
                }
                _ => {}
            }
            return;
        }
        if let Some(p) = &mut self.year_picker {
            let last = p.years.len() - 1;
            match mouse.kind {
                MouseEventKind::ScrollDown => p.state.select(Some((p.state.selected().unwrap_or(0) + 1).min(last))),
                MouseEventKind::ScrollUp => p.state.select(Some(p.state.selected().unwrap_or(0).saturating_sub(1))),
                MouseEventKind::Down(MouseButton::Left) => {
                    let a = self.year_picker_area;
                    if a.contains(pos) {
                        let i = p.state.offset() + (pos.y - a.y) as usize;
                        if i <= last {
                            self.pick_year(i);
                        }
                    } else {
                        self.year_picker = None;
                    }
                }
                _ => {}
            }
            return;
        }
        if let Some(p) = &mut self.month_picker {
            match mouse.kind {
                MouseEventKind::ScrollDown => p.state.select(Some((p.state.selected().unwrap_or(0) + 1).min(11))),
                MouseEventKind::ScrollUp => p.state.select(Some(p.state.selected().unwrap_or(0).saturating_sub(1))),
                MouseEventKind::Down(MouseButton::Left) => {
                    let a = self.month_picker_area;
                    if a.contains(pos) {
                        let i = p.state.offset() + (pos.y - a.y) as usize;
                        if i < 12 {
                            self.pick_month(i);
                        }
                    } else {
                        self.month_picker = None;
                    }
                }
                _ => {}
            }
            return;
        }
        if let Some(input) = &mut self.range_input {
            if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                match self.range_fields.iter().position(|r| r.contains(pos)) {
                    Some(i) => input.field = i,
                    None if !self.range_popup.contains(pos) => self.range_input = None,
                    None => {}
                }
            }
            return;
        }
        match mouse.kind {
            MouseEventKind::ScrollDown => self.scroll_by(3),
            MouseEventKind::ScrollUp => self.scroll_by(-3),
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some((_, b)) = self.buttons.iter().find(|(r, _)| r.contains(pos)) {
                    match b.clone() {
                        Button::Range(f) => self.set_filter(f),
                        Button::Tag => self.open_picker(),
                        Button::CustomRange => self.open_range_input(),
                    }
                } else if let Some(i) = self.row_at(pos) {
                    // first click selects, clicking the selected row toggles details
                    if self.table.selected() == Some(i) {
                        self.toggle_details();
                    } else {
                        self.table.select(Some(i));
                    }
                }
            }
            // right-click on the Month/Year button opens the month/year list
            MouseEventKind::Down(MouseButton::Right) => {
                match self.buttons.iter().find(|(r, _)| r.contains(pos)).map(|(_, b)| b) {
                    Some(Button::Range(Filter::Month)) => self.open_month_picker(),
                    Some(Button::Range(Filter::Year)) => self.open_year_picker(),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    /// Summary row index under a screen position, if any.
    fn row_at(&self, pos: Position) -> Option<usize> {
        let a = self.table_area;
        // border + top padding + header row above the first data row
        let first = a.y + 1 + PAD.top + 1;
        if pos.x <= a.x || pos.x + 1 >= a.right() || pos.y < first || pos.y + 1 >= a.bottom() {
            return None;
        }
        let i = self.table.offset() + (pos.y - first) as usize;
        (i < self.summary.rows.len()).then_some(i)
    }
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

const ACCENT: Color = Color::Cyan;
/// Border of the view that has the keyboard focus.
const FOCUS: Color = Color::Yellow;
/// Wk/Date/Day colors, alternating from one ISO week to the next.
const WEEK_COLORS: [Color; 2] = [Color::Cyan, Color::Magenta];
/// Colors of the chart bars as (strong, light) pairs from the 256-color
/// palette: strong for a tag's own share of the time, light for the time it
/// shares with other tags. The bars take them in order, top to bottom, so
/// the order is chosen such that every color clearly differs from the ones
/// before and after it (also from the last back to the first, for charts
/// with more bars than colors).
const TAG_COLORS: [(Color, Color); 12] = [
    (Color::Indexed(31), Color::Indexed(117)),  // blue
    (Color::Indexed(166), Color::Indexed(216)), // orange
    (Color::Indexed(28), Color::Indexed(120)),  // green
    (Color::Indexed(127), Color::Indexed(219)), // magenta
    (Color::Indexed(37), Color::Indexed(159)),  // cyan
    (Color::Indexed(124), Color::Indexed(210)), // red
    (Color::Indexed(64), Color::Indexed(193)),  // lime
    (Color::Indexed(55), Color::Indexed(147)),  // purple
    (Color::Indexed(136), Color::Indexed(229)), // olive
    (Color::Indexed(24), Color::Indexed(153)),  // steel blue
    (Color::Indexed(162), Color::Indexed(218)), // pink
    (Color::Indexed(94), Color::Indexed(180)),  // brown
];

/// The chart's name for time without tags.
const NO_TAG: &str = "(no tag)";

/// Gray (strong, light) pair, for "(no tag)" and the legend.
const GRAY_COLORS: (Color, Color) = (Color::Indexed(244), Color::Indexed(250));

/// The (strong, light) colors of each bar of the chart: the palette in
/// order, so that neighboring bars always differ; "(no tag)" is gray (and
/// doesn't use up a palette color).
fn bar_colors(totals: &[TagTotal]) -> Vec<(Color, Color)> {
    let mut next = 0;
    totals
        .iter()
        .map(|t| {
            if t.tag == NO_TAG {
                GRAY_COLORS
            } else {
                next += 1;
                TAG_COLORS[(next - 1) % TAG_COLORS.len()]
            }
        })
        .collect()
}

/// For each summary row, 0 or 1: flips whenever the ISO week (week-year +
/// week number) of the row's day changes, so consecutive weeks alternate.
/// Rows without a date belong to the day (and week) of the row above.
fn week_parity(rows: &[summary::Row]) -> Vec<usize> {
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

fn draw(frame: &mut Frame, app: &mut App) {
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
    if app.wide && has_selection {
        // wide: table at its natural width; on the right the details, with the chart below
        let [table, side] = Layout::horizontal([Constraint::Length(table_w), Constraint::Min(DETAILS_MIN_WIDTH)]).areas(body);
        draw_summary(frame, table, app);
        let [details, chart] = Layout::vertical([Constraint::Length(DETAILS_HEIGHT), Constraint::Min(0)]).areas(side);
        draw_details(frame, details, app);
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
    draw_help(frame, help, app.wide, app.last_edit.is_some());
    draw_picker(frame, body, app);
    draw_range_input(frame, body, app);
    draw_month_picker(frame, body, app);
    draw_year_picker(frame, body, app);
    draw_legend(frame, frame.area(), app);
    draw_info(frame, frame.area(), app);
}

/// One bar of the hours-per-tag chart (times in seconds).
#[derive(Debug, Clone, PartialEq)]
struct TagTotal {
    tag: String,
    /// The tag's even share: an entry with n tags gives 1/n of its time to
    /// each of them. The shares of all tags add up to the period's total.
    share: f64,
    /// The rest of the time of entries that have this tag together with
    /// others (their time minus this tag's share of it). `share + shared` is
    /// the full time of all entries that have the tag.
    shared: f64,
}

/// Hours per tag over the rows currently in the list, splitting each entry's
/// time evenly among its tags (see `TagTotal`). Entries without tags count
/// for "(no tag)". Each row's own (per-day, range-clipped) time is used, so
/// the numbers match the table. Sorted by share, then shared time, then name.
fn tag_totals(app: &App) -> Vec<TagTotal> {
    let mut totals = std::collections::BTreeMap::<String, (f64, f64)>::new();
    for row in &app.summary.rows {
        let secs = parse_hms(&row.time).unwrap_or(0) as f64;
        let tags = &app.tracked[row.source].interval.tags;
        if tags.is_empty() {
            totals.entry(NO_TAG.to_string()).or_default().0 += secs;
        }
        let share = secs / tags.len().max(1) as f64;
        for tag in tags {
            let t = totals.entry(tag.clone()).or_default();
            t.0 += share;
            t.1 += secs - share;
        }
    }
    let mut v: Vec<TagTotal> =
        totals.into_iter().map(|(tag, (share, shared))| TagTotal { tag, share, shared }).collect();
    v.sort_by(|a, b| b.share.total_cmp(&a.share).then(b.shared.total_cmp(&a.shared)).then_with(|| a.tag.cmp(&b.tag)));
    v
}

/// Decimal hours with two places from (possibly fractional) seconds.
fn decimal_hours_f(seconds: f64) -> String {
    format!("{:.2}", seconds / 3600.0)
}

/// Most bars shown in the bottom (narrow-layout) chart panel.
const CHART_MAX_BARS: usize = 10;

/// Height of the bottom chart panel for `totals`.
fn chart_height(totals: &[TagTotal]) -> u16 {
    let bars = totals.len().clamp(1, CHART_MAX_BARS) as u16;
    // bars + an "… more" line if cut off + blank + total line + borders + padding
    let more = u16::from(totals.len() > CHART_MAX_BARS);
    bars + more + 2 + 2 + PAD.top + PAD.bottom
}

/// A stacked bar of `width` cells using eighth blocks: the first `strong_frac`
/// (0..=1) in `strong`, then up to `strong_frac + light_frac` in `light`.
/// The cell where the strong part ends shows it on a light background.
/// Returns styled spans that are exactly `width` cells wide.
fn stacked_bar(strong_frac: f64, light_frac: f64, width: usize, strong: Color, light: Color) -> Vec<Span<'static>> {
    const PARTS: [&str; 8] = [" ", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];
    let eighths = |f: f64| (f.clamp(0.0, 1.0) * width as f64 * 8.0).round() as usize;
    let s = eighths(strong_frac);
    let t = eighths(strong_frac + light_frac).max(s);
    let mut spans: Vec<Span<'static>> = Vec::new();
    for cell in 0..width {
        let fill_s = s.saturating_sub(cell * 8).min(8);
        let fill_t = t.saturating_sub(cell * 8).min(8);
        let (text, style) = if fill_s == 8 {
            ("█", Style::new().fg(strong))
        } else if fill_s > 0 && fill_t > fill_s {
            (PARTS[fill_s], Style::new().fg(strong).bg(light))
        } else if fill_s > 0 {
            (PARTS[fill_s], Style::new().fg(strong))
        } else if fill_t == 8 {
            ("█", Style::new().fg(light))
        } else if fill_t > 0 {
            (PARTS[fill_t], Style::new().fg(light))
        } else {
            (" ", Style::new())
        };
        match spans.last_mut() {
            Some(last) if last.style == style => last.content.to_mut().push_str(text),
            _ => spans.push(Span::styled(text, style)),
        }
    }
    spans
}

fn draw_chart(frame: &mut Frame, area: Rect, app: &App) {
    let totals = tag_totals(app);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(ACCENT))
        .title_bottom(if app.wide { Line::raw("") } else { Line::from(" b/Esc close ").right_aligned().dim() })
        .padding(PAD);
    let inner = block.inner(area);

    // Fit the bars into the panel: all bars plus the total line if possible,
    // else without the total, else as many bars as fit plus a "… more" line.
    let avail = inner.height as usize;
    let limit = if app.wide { usize::MAX } else { CHART_MAX_BARS };
    let n = totals.len();
    let fits = |bars: usize, more: bool, total: bool| bars + usize::from(more) + 2 * usize::from(total) <= avail;
    let (shown, show_total) = if n <= limit && fits(n, false, true) {
        (n, true)
    } else if n > limit && fits(limit, true, true) {
        (limit, true)
    } else if n <= limit && fits(n, false, false) {
        (n, false)
    } else {
        (avail.saturating_sub(1).min(limit), false)
    };
    // the number of tags only matters when not all of them are shown
    let block = if shown < n {
        block.title(format!(" Hours per tag · {n} "))
    } else {
        block.title(" Hours per tag ")
    };
    // bars are scaled to the largest full time (share + shared)
    let max = totals.iter().map(|t| t.share + t.shared).fold(0.0, f64::max).max(1.0);
    let sum = app.summary.grand_total as f64;
    let any_shared = totals.iter().any(|t| t.shared > 0.0);
    let name_w = totals.iter().take(shown).map(|t| t.tag.chars().count()).max().unwrap_or(0);
    let value_w = totals.iter().take(shown).map(|t| decimal_hours_f(t.share).len()).max().unwrap_or(4);
    let shared_w =
        totals.iter().take(shown).filter(|t| t.shared > 0.0).map(|t| decimal_hours_f(t.shared).len()).max();
    // "name  ███▒▒  12.50 h  +3.25 h   42 %":
    // name + 2, bar, 2 + value + " h", [2 + "+" + shared + " h"], 2 + pct(3) + " %"
    let shared_cols = shared_w.map_or(0, |w| 2 + 1 + w + 2);
    let fixed = name_w + 2 + (2 + value_w + 2) + shared_cols + (2 + 3 + 2);
    let bar_w = (inner.width as usize).saturating_sub(fixed).max(1);

    let colors = bar_colors(&totals);
    let mut lines: Vec<Line> = totals
        .iter()
        .zip(colors)
        .take(shown)
        .map(|(t, (strong, light))| {
            let pct = if sum > 0.0 { t.share * 100.0 / sum } else { 0.0 };
            let mut spans = vec![Span::styled(format!("{:<name_w$}  ", t.tag), Style::new().fg(strong))];
            spans.extend(stacked_bar(t.share / max, t.shared / max, bar_w, strong, light));
            spans.push(Span::styled(
                format!("  {:>value_w$} h", decimal_hours_f(t.share)),
                Style::new().fg(Color::Yellow).bold(),
            ));
            if let Some(w) = shared_w {
                let text = if t.shared > 0.0 {
                    format!("  +{:>w$} h", decimal_hours_f(t.shared))
                } else {
                    " ".repeat(2 + 1 + w + 2)
                };
                spans.push(Span::styled(text, Style::new().fg(Color::Gray)));
            }
            spans.push(Span::styled(format!("  {pct:>3.0} %"), Style::new().dim()));
            Line::from(spans)
        })
        .collect();
    if shown < totals.len() {
        let rest: f64 = totals[shown..].iter().map(|t| t.share).sum();
        lines.push(Line::styled(
            format!("… {} more tags, {} h", totals.len() - shown, decimal_hours_f(rest)),
            Style::new().dim(),
        ));
    }
    if totals.is_empty() {
        lines.push(Line::styled("No entries.", Style::new().dim()));
    }
    if show_total {
        lines.push(Line::raw(""));
        let mut total = vec![
            Span::styled("Total ", Style::new().dim()),
            Span::styled(format!("{} h", decimal_hours(app.summary.grand_total)), Style::new().fg(Color::Yellow).bold()),
        ];
        if any_shared {
            // legend for the two shades
            total.extend([
                Span::raw("    "),
                Span::styled("█", Style::new().fg(GRAY_COLORS.0)),
                Span::styled(" own share  ", Style::new().dim()),
                Span::styled("█", Style::new().fg(GRAY_COLORS.1)),
                Span::styled(" shared (+)", Style::new().dim()),
            ]);
        }
        lines.push(Line::from(total));
    }

    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_year_picker(frame: &mut Frame, area: Rect, app: &mut App) {
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

fn draw_month_picker(frame: &mut Frame, area: Rect, app: &mut App) {
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

/// Inner spacing between a view's frame and its content.
const PAD: Padding = Padding::new(2, 2, 1, 0);
/// Height of the bottom details panel: 10 content lines + borders + padding.
const DETAILS_HEIGHT: u16 = 10 + 2 + PAD.top + PAD.bottom;
/// Minimum width of the side details panel; below that the narrow layout is used.
const DETAILS_MIN_WIDTH: u16 = 44;

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

/// The TUI shows no seconds: turn `H:MM:SS` into `H:MM` (seconds are cut
/// off, not rounded). Anything else, such as the `-` end of a running
/// entry or an empty cell, is returned unchanged.
fn without_seconds(s: &str) -> String {
    let b = s.as_bytes();
    let n = b.len();
    let is_hms = n >= 7
        && b[n - 3] == b':'
        && b[n - 6] == b':'
        && [n - 1, n - 2, n - 4, n - 5].iter().all(|&i| b[i].is_ascii_digit());
    if is_hms { s[..n - 3].to_string() } else { s.to_string() }
}

/// Seconds of an `H:MM:SS` duration string (None for anything else).
fn parse_hms(s: &str) -> Option<i64> {
    let mut parts = s.split(':');
    let (h, m, sec) = (parts.next()?.parse::<i64>().ok()?, parts.next()?.parse::<i64>().ok()?, parts.next()?.parse::<i64>().ok()?);
    parts.next().is_none().then_some(h * 3600 + m * 60 + sec)
}

/// A duration as decimal hours with two places, e.g. 7:30 -> `7.50`.
fn decimal_hours(seconds: i64) -> String {
    format!("{:.2}", seconds as f64 / 3600.0)
}

/// A duration in seconds as `H:MM` (hours unbounded, seconds cut off).
fn format_hm(seconds: i64) -> String {
    format!("{}:{:02}", seconds / 3600, (seconds % 3600) / 60)
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
fn table_width(summary: &Summary) -> u16 {
    let cols: usize = column_widths(summary).iter().sum();
    (cols + HEADERS.len() - 1 + 2) as u16 + PAD.left + PAD.right
}

fn draw_details(frame: &mut Frame, area: Rect, app: &mut App) {
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
        .border_style(Style::new().fg(ACCENT))
        .title(" Entry details ")
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
    let iv = &form.original;
    let label = |s: &'static str| Span::styled(format!("{s:<11}"), Style::new().fg(ACCENT).bold());
    let field = |i: usize| {
        let style = if form.field == i { Style::new().add_modifier(Modifier::REVERSED) } else { Style::new().underlined() };
        if is_text(i) {
            let skip = if i == form.field { form.scroll } else { 0 };
            let text: String = form.fields[i].chars().skip(skip).take(visible).collect();
            return Span::styled(format!(" {:<w$}", text, w = (tags_w as usize).saturating_sub(1)), style);
        }
        Span::styled(format!(" {:<w$}", form.fields[i], w = EDIT_FIELD_WIDTHS[i] as usize - 1), style)
    };
    let now = jiff::Timestamp::now().as_second();
    let duration = match form.edited() {
        Ok(e) => Span::styled(format_hm(e.range.total(now)), Style::new().fg(Color::Yellow).bold()),
        Err(_) => Span::styled("–", Style::new().dim()),
    };
    let last = match &form.error {
        Some(err) => Line::styled(err.clone(), Style::new().fg(Color::Red).bold()),
        None if form.field == TAGS => Line::styled("separated by spaces, \"in quotes\" with spaces", Style::new().dim()),
        None if form.field == ANNOTATION => Line::styled("any text; empty removes the annotation", Style::new().dim()),
        None if iv.is_open() => Line::styled("YYYY-MM-DD  HH:MM · empty end: still tracking", Style::new().dim()),
        None => Line::styled("YYYY-MM-DD  HH:MM", Style::new().dim()),
    };
    let lines = vec![
        Line::from(vec![label("ID"), Span::raw(format!("@{}", form.id))]),
        Line::from(vec![label("Tags"), field(TAGS)]),
        Line::from(vec![label("Annotation"), field(ANNOTATION)]),
        Line::raw(""),
        Line::from(vec![label("Start"), field(0), Span::raw(" "), field(1)]),
        Line::from(vec![label("End"), field(2), Span::raw(" "), field(3)]),
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

/// "2026-11-12 Thu 08:00" in local time.
fn local_datetime(epoch: i64) -> String {
    let z = jiff::Timestamp::from_second(epoch).unwrap().to_zoned(jiff::tz::TimeZone::system());
    z.strftime("%Y-%m-%d %a %H:%M").to_string()
}

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

fn draw_range_input(frame: &mut Frame, area: Rect, app: &mut App) {
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

fn draw_picker(frame: &mut Frame, area: Rect, app: &mut App) {
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

/// The summary frame's title, e.g. " Summary · Month · 2026-11-01 – 2026-11-30 · tag meetings ".
fn summary_title(app: &App) -> Line<'static> {
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

fn draw_summary(frame: &mut Frame, area: Rect, app: &mut App) {
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

fn draw_status(frame: &mut Frame, area: Rect, app: &App) {
    let days = app.summary.rows.iter().filter(|r| r.date.is_some()).count();
    let intervals = app.summary.rows.len();
    let total = app.summary.grand_total;
    let avg = if days > 0 { format!("{} h", decimal_hours(total / days as i64)) } else { "-".into() };
    if let Some(notice) = &app.notice {
        let color = if app.notice_failed { Color::Red } else { Color::Green };
        frame.render_widget(Paragraph::new(Line::styled(format!(" {notice}"), Style::new().fg(color).bold())), area);
        return;
    }
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

fn draw_help(frame: &mut Frame, area: Rect, wide: bool, can_undo: bool) {
    let mut items: Vec<(&str, &str)> = Vec::new();
    if can_undo {
        // first, so that it is visible in a narrow window too
        items.push(("u", "undo"));
    }
    items.extend([
        ("↑↓", "move"),
        ("1-4", "range"),
        ("⇧3/⇧4", "pick"),
        ("5", "range"),
        ("6", "tag"),
    ]);
    if !wide {
        items.extend([("b", "chart"), ("Enter", "details")]);
    }
    items.push(("e", "edit"));
    items.extend([("PgUp/Dn", "page"), ("r", "reload"), ("i", "info")]);

    // Keep "q quit" at the end. If not everything fits, put "? help" (the
    // full key legend) first and drop items from the end until it fits.
    let width = |k: &str, l: &str| k.chars().count() + 1 + l.chars().count() + 2;
    let all: usize = items.iter().map(|(k, l)| width(k, l)).sum();
    let tail = [("q", "quit")];
    if 1 + all + width("q", "quit") > area.width as usize {
        items.insert(0, ("?", "help"));
    }
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

/// The key legend shown by `?`: section headings (`None` key) and key rows.
fn legend_lines() -> Vec<(Option<&'static str>, &'static str)> {
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
        k("1  d", "today"),
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
        k("u", "undo the last saved edit"),
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
        k("i", "info: version and project page"),
        k("Esc", "reset all filters (current month, all tags)"),
        k("q", "quit"),
        k("Ctrl+C", "quit, from anywhere"),
    ]
}

/// The info dialog (`i`): version and project page.
fn draw_info(frame: &mut Frame, area: Rect, app: &App) {
    if !app.info {
        return;
    }
    let label = |s: &'static str| Span::styled(format!("{s:<10}"), Style::new().fg(ACCENT).bold());
    let lines = vec![
        Line::from(vec![
            Span::styled("timew-tui ", Style::new().bold()),
            Span::styled(env!("CARGO_PKG_VERSION"), Style::new().fg(Color::Yellow).bold()),
        ]),
        Line::styled(env!("CARGO_PKG_DESCRIPTION"), Style::new().dim()),
        Line::raw(""),
        Line::from(vec![label("Version"), Span::raw(env!("CARGO_PKG_VERSION"))]),
        Line::from(vec![label("Library"), Span::raw(format!("timew-core {}", timew_core::VERSION))]),
        Line::from(vec![label("Project"), Span::styled(env!("CARGO_PKG_REPOSITORY"), Style::new().fg(ACCENT).underlined())]),
    ];
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

fn draw_legend(frame: &mut Frame, area: Rect, app: &mut App) {
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

/// Local calendar date of an epoch, as `YYYY-MM-DD`.
fn local_date(epoch: i64) -> String {
    jiff::Timestamp::from_second(epoch).unwrap().to_zoned(jiff::tz::TimeZone::system()).date().to_string()
}

/// Local `YYYY-MM-DD`, plus ` HH:MM` when the epoch is not at midnight.
fn local_moment(epoch: i64) -> String {
    let z = jiff::Timestamp::from_second(epoch).unwrap().to_zoned(jiff::tz::TimeZone::system());
    if (z.hour(), z.minute(), z.second()) == (0, 0, 0) {
        z.date().to_string()
    } else {
        z.strftime("%Y-%m-%d %H:%M").to_string()
    }
}

/// "2026-11-01 – 2026-11-30" (end is exclusive internally, shown inclusive
/// for whole days), "since 2026-11-01" for an open-ended range.
fn range_label(range: &Range) -> String {
    if !range.is_ended() {
        return format!("since {}", local_moment(range.start));
    }
    let start = local_moment(range.start);
    let end_is_midnight = local_moment(range.end).len() == 10;
    let end = if end_is_midnight && start.len() == 10 { local_date(range.end - 1) } else { local_moment(range.end) };
    if start == end { start } else { format!("{start} – {end}") }
}

// ---------------------------------------------------------------------------
// Main loop
// ---------------------------------------------------------------------------

fn run(terminal: &mut DefaultTerminal, mut app: App) -> io::Result<()> {
    while !app.quit {
        terminal.draw(|f| draw(f, &mut app))?;
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => app.on_key(key),
            Event::Mouse(mouse) => app.on_mouse(mouse),
            _ => {}
        }
    }
    Ok(())
}

fn main() -> std::process::ExitCode {
    let db_root = match resolve_db_root() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    if !db_root.join("data").is_dir() {
        eprintln!("No Timewarrior database found at {} (missing data/ directory).", db_root.display());
        return std::process::ExitCode::FAILURE;
    }

    let app = App::new(db_root);
    let mut terminal = ratatui::init();
    let _ = execute!(io::stdout(), EnableMouseCapture);
    // lets terminals that support it report Shift+Enter (kitty keyboard protocol)
    let enhanced = ratatui::crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false)
        && execute!(io::stdout(), PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)).is_ok();
    let result = run(&mut terminal, app);
    if enhanced {
        let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
    }
    let _ = execute!(io::stdout(), DisableMouseCapture);
    ratatui::restore();

    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::KeyEventState;
    use std::fs;

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

    #[test]
    fn filters_switch_the_summary() {
        let root = sample_db("filters");
        let mut app = App::new(root.clone());
        assert_eq!(app.filter, Filter::Month, "month is the default filter");
        let month = render(&mut app);
        println!("{month}");
        assert!(month.contains("Summary · Month"));
        assert!(!month.contains("Annotation") && !month.contains("@1"), "ID/Annotation are hidden");

        press(&mut app, '1');
        let day = render(&mut app);
        println!("{day}");
        assert!(day.contains("meetings") && day.contains("email") && !day.contains("planning"));
        assert!(day.contains("Total 3.00 h"));

        press(&mut app, '2');
        let yday = render(&mut app);
        assert!(yday.contains("planning") && !yday.contains("meetings"));
        assert!(yday.contains("Total 4.00 h"));

        press(&mut app, '4');
        assert!(render(&mut app).contains("Total 7.00 h"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn mouse_click_on_button_selects_filter() {
        let root = sample_db("mouse");
        let mut app = App::new(root.clone());
        render(&mut app);
        let (rect, f) = app.buttons[0].clone();
        assert_eq!(f, Button::Range(Filter::Day));
        app.on_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rect.x + 2,
            row: rect.y + 1,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.filter, Filter::Day);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn scrolling_is_clamped() {
        let root = sample_db("scroll");
        let mut app = App::new(root.clone());
        press(&mut app, '4');
        press(&mut app, 'G');
        assert_eq!(app.table.selected(), Some(2));
        press(&mut app, 'j');
        assert_eq!(app.table.selected(), Some(2));
        press(&mut app, 'g');
        assert_eq!(app.table.selected(), Some(0));
        let _ = fs::remove_dir_all(&root);
    }

    fn key(app: &mut App, code: KeyCode) {
        app.on_key(KeyEvent { code, modifiers: KeyModifiers::NONE, kind: KeyEventKind::Press, state: KeyEventState::NONE });
    }

    #[test]
    fn details_popup_follows_selection() {
        let root = sample_db("details");
        let mut app = App::new(root.clone());
        press(&mut app, '4');
        key(&mut app, KeyCode::Enter);
        assert!(app.details);
        let first = render(&mut app);
        println!("{first}");
        assert!(first.contains("Entry details") && first.contains("planning") && first.contains("Duration   4:00"));
        assert!(first.contains("Start      ") && !first.contains(":00:00"), "no seconds anywhere: {first}");
        assert!(first.contains("@3"));
        let start = jiff::Zoned::now().date().yesterday().unwrap();
        assert!(first.contains(&start.strftime("%A").to_string()), "weekday shown");
        assert!(first.contains(&format!("W{:02} {}", start.iso_week_date().week(), start.iso_week_date().year())), "week shown");

        press(&mut app, 'j');
        let second = render(&mut app);
        assert!(second.contains("@2") && second.contains("Duration   2:00"));

        key(&mut app, KeyCode::Esc);
        assert!(!app.details && !app.quit, "Esc closes the details first");
        key(&mut app, KeyCode::Esc);
        assert!(!app.quit, "then it resets the filters, it never quits");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn wide_window_shows_details_on_the_right_automatically() {
        let root = sample_db("wide");
        let mut app = App::new(root.clone());
        press(&mut app, '4');
        let screen = render_at(&mut app, 140, 24);
        println!("{screen}");
        assert!(app.wide);
        assert_eq!(app.table.selected(), Some(0), "top entry is selected");
        // details panel sits on the same line as the table header, to its right
        let header = screen.lines().find(|l| l.contains("Summary · Year")).unwrap();
        assert!(header.contains("Entry details"));
        assert!(screen.contains("@3") && !screen.contains("Enter details"));

        // Enter does nothing in the wide layout; q quits
        key(&mut app, KeyCode::Enter);
        assert!(render_at(&mut app, 140, 24).contains("Entry details"));
        press(&mut app, 'q');
        assert!(app.quit);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn narrow_window_needs_enter_for_details() {
        let root = sample_db("narrow");
        let mut app = App::new(root.clone());
        press(&mut app, '4');
        let screen = render(&mut app);
        assert!(!app.wide);
        assert!(!screen.contains("Entry details") && screen.contains("Enter details"));
        key(&mut app, KeyCode::Enter);
        let screen = render(&mut app);
        let header = screen.lines().find(|l| l.contains("Summary · Year")).unwrap();
        assert!(!header.contains("Entry details"), "narrow details go below the table");
        assert!(screen.contains("Entry details"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn clicking_selected_row_opens_details() {
        let root = sample_db("click-row");
        let mut app = App::new(root.clone());
        press(&mut app, '4');
        render(&mut app);
        let a = app.table_area;
        let click = |app: &mut App, y| {
            app.on_mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: a.x + 5,
                row: y,
                modifiers: KeyModifiers::NONE,
            })
        };
        let second_row = a.y + 1 + PAD.top + 1 + 1; // border, padding, header, first row
        click(&mut app, second_row);
        assert_eq!(app.table.selected(), Some(1));
        assert!(!app.details);
        click(&mut app, second_row);
        assert!(app.details);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn tag_picker_filters_by_tag() {
        let root = sample_db("tag-picker");
        let mut app = App::new(root.clone());
        press(&mut app, '4');
        assert_eq!(
            app.all_tags,
            vec![("email".into(), 1), ("meetings".into(), 1), ("planning".into(), 1)]
        );

        press(&mut app, 't');
        let screen = render(&mut app);
        println!("{screen}");
        assert!(screen.contains("Filter by tag") && screen.contains("All tags"));
        assert!(screen.contains("│ 5 Range │") && screen.contains("│ 6 Tag │"), "no value shown when nothing is selected");

        // entry 0 = All tags, 1 = email, 2 = meetings
        press(&mut app, 'j');
        press(&mut app, 'j');
        key(&mut app, KeyCode::Enter);
        assert!(app.picker.is_none());
        assert_eq!(app.tag.as_deref(), Some("meetings"));
        let screen = render(&mut app);
        println!("{screen}");
        assert!(screen.contains("6 meetings ▾") && screen.contains("tag meetings"));
        assert!(screen.contains("Total 2.00 h") && !screen.contains("planning  "));

        // the tag filter combines with the range filter
        press(&mut app, '2');
        assert!(render(&mut app).contains("No data found in the range") );
        assert!(render(&mut app).contains("tagged with meetings"));

        // Backspace outside the picker does nothing
        key(&mut app, KeyCode::Backspace);
        assert_eq!(app.tag.as_deref(), Some("meetings"));

        // Backspace inside the picker clears the tag filter and closes it
        press(&mut app, 't');
        key(&mut app, KeyCode::Backspace);
        assert!(app.picker.is_none());
        assert_eq!(app.tag, None);
        assert!(render(&mut app).contains("Total 4.00 h"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn tag_picker_by_mouse_and_esc() {
        let root = sample_db("tag-mouse");
        let mut app = App::new(root.clone());
        press(&mut app, '4');
        render(&mut app);
        let (rect, _) = app.buttons.iter().find(|(_, b)| *b == Button::Tag).unwrap().clone();
        let click = |app: &mut App, column, row| {
            app.on_mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column,
                row,
                modifiers: KeyModifiers::NONE,
            })
        };
        click(&mut app, rect.x + 2, rect.y + 1);
        assert!(app.picker.is_some());
        render(&mut app);
        let a = app.picker_area;
        click(&mut app, a.x + 1, a.y + 3); // "planning"
        assert_eq!(app.tag.as_deref(), Some("planning"));

        press(&mut app, 't');
        key(&mut app, KeyCode::Esc);
        assert!(app.picker.is_none() && !app.quit, "Esc only closes the picker");
        assert_eq!(app.tag.as_deref(), Some("planning"));
        let _ = fs::remove_dir_all(&root);
    }

    fn local(y: i16, mo: i8, d: i8, h: i8, mi: i8) -> i64 {
        jiff::civil::date(y, mo, d).at(h, mi, 0, 0).to_zoned(jiff::tz::TimeZone::system()).unwrap().timestamp().as_second()
    }

    #[test]
    fn range_input_parsing() {
        // bare dates: To is inclusive
        let r = parse_range_input("2025-03-01", "2025-03-31").unwrap();
        assert_eq!((r.start, r.end), (local(2025, 3, 1, 0, 0), local(2025, 4, 1, 0, 0)));
        assert_eq!(range_label(&r), "2025-03-01 – 2025-03-31");
        // a single day
        let r = parse_range_input("2025-03-05", "2025-03-05").unwrap();
        assert_eq!(range_label(&r), "2025-03-05");
        // times, with a space or a T, with or without seconds
        let r = parse_range_input("2025-03-05 08:30", "2025-03-05T12:00:00").unwrap();
        assert_eq!((r.start, r.end), (local(2025, 3, 5, 8, 30), local(2025, 3, 5, 12, 0)));
        assert_eq!(range_label(&r), "2025-03-05 08:30 – 2025-03-05 12:00");
        // open end
        let r = parse_range_input("2025-03-01", "").unwrap();
        assert!(!r.is_ended());
        assert_eq!(range_label(&r), "since 2025-03-01");
        // hints
        assert_eq!(parse_range_input(":lastyear", "").unwrap(), expand_range_hint("lastyear").unwrap());
        // errors
        assert!(parse_range_input("", "").unwrap_err().contains("start date"));
        assert!(parse_range_input("2025-03-05", "2025-03-01").unwrap_err().contains("after"));
        assert!(parse_range_input("yesterday-ish", "").is_err());
        assert!(parse_range_input(":nope", "").unwrap_err().contains("Unknown"));
        assert!(parse_range_input(":week", "2025-01-01").is_err());
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

    #[test]
    fn custom_range_form() {
        let root = sample_db("custom-range");
        let mut app = App::new(root.clone());
        let today = jiff::Zoned::now().date();
        let yesterday = today.yesterday().unwrap();

        // prefilled with the current (month) range
        press(&mut app, 'c');
        let input = app.range_input.clone().unwrap();
        assert_eq!(input.from, local_date(app.range.start));
        assert_eq!(input.to, local_date(app.range.end - 1));
        let screen = render(&mut app);
        println!("{screen}");
        assert!(screen.contains("Date range") && screen.contains("From") && screen.contains("To"));

        // invalid input shows an error and keeps the form open; typing 'q' doesn't quit
        clear_field(&mut app);
        type_text(&mut app, "qqq");
        key(&mut app, KeyCode::Enter);
        assert!(!app.quit && app.range_input.as_ref().unwrap().error.is_some());
        assert!(render(&mut app).contains("not a valid date"));

        // yesterday only
        clear_field(&mut app);
        type_text(&mut app, &yesterday.to_string());
        key(&mut app, KeyCode::Tab);
        clear_field(&mut app);
        type_text(&mut app, &yesterday.to_string());
        key(&mut app, KeyCode::Enter);
        assert!(app.range_input.is_none());
        assert_eq!(app.filter, Filter::Custom);
        let screen = render(&mut app);
        println!("{screen}");
        assert!(screen.contains(&format!("5 {yesterday} ▾")));
        assert!(screen.contains("Summary · Range") && screen.contains("Total 4.00 h"));

        // reopening shows what was typed; Esc cancels without changes
        press(&mut app, 'c');
        assert_eq!(app.range_input.as_ref().unwrap().from, yesterday.to_string());
        key(&mut app, KeyCode::Esc);
        assert!(app.range_input.is_none() && !app.quit);
        assert_eq!(app.filter, Filter::Custom);

        // custom range combines with the tag filter, presets still work
        press(&mut app, '1');
        assert_eq!(app.filter, Filter::Day);
        assert!(render(&mut app).contains("Total 3.00 h"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn custom_range_button_opens_form() {
        let root = sample_db("custom-button");
        let mut app = App::new(root.clone());
        render(&mut app);
        let (rect, _) = app.buttons.iter().find(|(_, b)| *b == Button::CustomRange).unwrap().clone();
        app.on_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: rect.x + 2,
            row: rect.y + 1,
            modifiers: KeyModifiers::NONE,
        });
        assert!(app.range_input.is_some());
        render(&mut app);
        let to = app.range_fields[1];
        app.on_mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: to.x + 1, row: to.y, modifiers: KeyModifiers::NONE });
        assert_eq!(app.range_input.as_ref().unwrap().field, 1);
        app.on_mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: 0, row: 0, modifiers: KeyModifiers::NONE });
        assert!(app.range_input.is_none(), "click outside closes the form");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn week_colors_alternate_per_iso_week() {
        let row = |date: Option<&str>| summary::Row {
            week: None,
            date: date.map(str::to_string),
            day_name: None,
            id: String::new(),
            tags: String::new(),
            annotation: String::new(),
            start: String::new(),
            end: String::new(),
            time: String::new(),
            total: String::new(),
            source: 0,
        };
        let rows = [
            row(Some("2026-11-06")), // Fri W45
            row(None),               // same day
            row(Some("2026-11-09")), // Mon W46
            row(Some("2026-11-13")), // Fri W46
            row(Some("2026-11-16")), // Mon W47
            row(None),
            row(Some("2025-11-17")), // W47 of another year -> still a new week
        ];
        assert_eq!(week_parity(&rows), vec![0, 0, 1, 1, 0, 0, 1]);
    }

    #[test]
    fn shift_3_opens_month_list_of_shown_year() {
        let root = sample_db("months");
        let mut app = App::new(root.clone());
        let now = jiff::Zoned::now();
        let yesterday = now.date().yesterday().unwrap();

        for c in ['§', '#'] {
            press(&mut app, c);
            assert!(app.month_picker.is_some(), "{c} opens the month list");
            key(&mut app, KeyCode::Esc);
            assert!(app.month_picker.is_none() && !app.quit);
        }
        app.on_key(KeyEvent {
            code: KeyCode::Char('3'),
            modifiers: KeyModifiers::SHIFT,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        });
        assert!(app.month_picker.is_some(), "Shift+3 reported with modifier");

        let p = app.month_picker.as_ref().unwrap();
        assert_eq!(p.year, now.year());
        assert_eq!(p.state.selected(), Some(now.month() as usize - 1));
        let screen = render(&mut app);
        println!("{screen}");
        assert!(MONTH_NAMES.iter().all(|m| screen.contains(m)));

        // previous year: pick January there
        key(&mut app, KeyCode::Left);
        assert_eq!(app.month_picker.as_ref().unwrap().year, now.year() - 1);
        press(&mut app, 'g');
        key(&mut app, KeyCode::Enter);
        assert!(app.month_picker.is_none());
        assert_eq!((app.filter, app.month), (Filter::Month, Some((now.year() - 1, 1))));
        assert_eq!(app.range, month_range(now.year() - 1, 1));
        let screen = render(&mut app);
        assert!(screen.contains(&format!("3 {}-01 ▾", now.year() - 1)), "button shows the month number");
        assert!(screen.contains("No data found"));

        // opening again shows that year; plain 3 goes back to the current month
        press(&mut app, '#');
        assert_eq!(app.month_picker.as_ref().unwrap().year, now.year() - 1);
        key(&mut app, KeyCode::Esc);
        press(&mut app, '3');
        assert_eq!(app.month, None);
        assert_eq!(app.range, expand_range_hint("month").unwrap());

        // Backspace in the list behaves like pressing 3
        press(&mut app, '#');
        key(&mut app, KeyCode::Left);
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.month, Some((now.year() - 1, now.month())));
        press(&mut app, '#');
        key(&mut app, KeyCode::Backspace);
        assert!(app.month_picker.is_none());
        assert_eq!((app.filter, app.month), (Filter::Month, None));
        assert_eq!(app.range, expand_range_hint("month").unwrap());
        press(&mut app, '4');
        press(&mut app, '#');
        key(&mut app, KeyCode::Backspace);
        assert_eq!((app.filter, app.month), (Filter::Month, None), "also from another filter");

        // pick yesterday's month by mouse
        press(&mut app, '#');
        if yesterday.year() != now.year() {
            key(&mut app, KeyCode::Left);
        }
        render(&mut app);
        let a = app.month_picker_area;
        app.on_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: a.x + 2,
            row: a.y + yesterday.month() as u16 - 1,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.month, Some((yesterday.year(), yesterday.month())));
        assert!(render(&mut app).contains("planning"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn shift_4_opens_list_of_last_five_years() {
        let root = sample_db("years");
        let mut app = App::new(root.clone());
        let this_year = jiff::Zoned::now().year();

        press(&mut app, '$');
        let p = app.year_picker.as_ref().expect("$ opens the year list");
        assert_eq!(p.years, (0..5).map(|i| this_year - i).collect::<Vec<_>>());
        assert_eq!(p.state.selected(), Some(0), "the shown (current) year is preselected");
        let screen = render(&mut app);
        println!("{screen}");
        assert!(screen.contains(" Year ") && screen.contains(&(this_year - 4).to_string()));
        key(&mut app, KeyCode::Esc);
        assert!(app.year_picker.is_none() && !app.quit);

        app.on_key(KeyEvent {
            code: KeyCode::Char('4'),
            modifiers: KeyModifiers::SHIFT,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        });
        assert!(app.year_picker.is_some(), "Shift+4 reported with modifier");

        // pick last year
        press(&mut app, 'j');
        key(&mut app, KeyCode::Enter);
        assert_eq!((app.filter, app.year), (Filter::Year, Some(this_year - 1)));
        assert_eq!(app.range, year_range(this_year - 1));
        assert!(render(&mut app).contains(&format!("4 {} ▾", this_year - 1)));

        // the month list now opens on that year
        press(&mut app, '#');
        assert_eq!(app.month_picker.as_ref().unwrap().year, this_year - 1);
        key(&mut app, KeyCode::Esc);

        // reopening preselects the shown year; Backspace = pressing 4
        press(&mut app, '$');
        assert_eq!(app.year_picker.as_ref().unwrap().state.selected(), Some(1));
        key(&mut app, KeyCode::Backspace);
        assert!(app.year_picker.is_none());
        assert_eq!((app.filter, app.year), (Filter::Year, None));
        assert_eq!(app.range, expand_range_hint("year").unwrap());

        // plain 4 also resets a picked year; mouse picking works
        press(&mut app, '$');
        render(&mut app);
        let a = app.year_picker_area;
        app.on_mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: a.x + 2, row: a.y + 2, modifiers: KeyModifiers::NONE });
        assert_eq!(app.year, Some(this_year - 2));
        press(&mut app, '4');
        assert_eq!(app.year, None);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn seconds_are_not_shown() {
        assert_eq!(without_seconds("8:50:00"), "8:50");
        assert_eq!(without_seconds("10:20:59"), "10:20");
        assert_eq!(without_seconds("2086:35:00"), "2086:35");
        assert_eq!(without_seconds("-"), "-");
        assert_eq!(without_seconds(""), "");
        assert_eq!(format_hm(0), "0:00");
        assert_eq!(format_hm(7 * 3600 + 59 * 60 + 2), "7:59");
        assert_eq!(format_hm(2086 * 3600 + 35 * 60), "2086:35");
    }

    #[test]
    fn totals_are_decimal_hours() {
        assert_eq!(parse_hms("7:30:00"), Some(7 * 3600 + 30 * 60));
        assert_eq!(parse_hms("2086:35:00"), Some(2086 * 3600 + 35 * 60));
        assert_eq!(parse_hms(""), None);
        assert_eq!(parse_hms("-"), None);
        assert_eq!(decimal_hours(7 * 3600 + 30 * 60), "7.50");
        assert_eq!(decimal_hours(8 * 3600 + 25 * 60), "8.42");
        assert_eq!(decimal_hours(2086 * 3600 + 35 * 60), "2086.58");
        assert_eq!(decimal_hours(0), "0.00");
    }

    #[test]
    fn bar_chart_of_hours_per_tag() {
        let root = sample_db("chart");
        let mut app = App::new(root.clone());
        press(&mut app, '4');
        assert_eq!(
            simple(&tag_totals(&app)),
            vec![("planning".into(), 4 * 3600, 0), ("meetings".into(), 2 * 3600, 0), ("email".into(), 3600, 0)]
        );

        // wide: the chart is always shown, below the details on the right
        let screen = render_at(&mut app, 140, 34);
        println!("{screen}");
        let title = screen.lines().find(|l| l.contains("Summary · Year")).unwrap();
        assert!(title.contains("Entry details"));
        let lines: Vec<&str> = screen.lines().collect();
        let row = |s: &str| lines.iter().position(|l| l.contains(s)).unwrap();
        assert!(row("Hours per tag ─") > row("Duration"), "chart below the details");
        assert!(!screen.contains("Hours per tag ·"), "all tags shown: no count");
        // b and Esc don't hide it in the wide layout; Esc quits
        press(&mut app, 'b');
        assert!(render_at(&mut app, 140, 34).contains("Hours per tag"));
        let planning = chart_line(&screen, "planning");
        assert!(planning.contains("4.00 h") && planning.contains("57 %"));
        assert!(screen.contains("Total 7.00 h"));
        // the longest bar belongs to the largest tag
        let bar_len = |tag: &str| chart_line(&screen, tag).matches('█').count();
        assert!(bar_len("planning") > bar_len("meetings") && bar_len("meetings") > bar_len("email"));

        // the chart follows the filters
        press(&mut app, '1');
        assert_eq!(simple(&tag_totals(&app)), vec![("meetings".into(), 2 * 3600, 0), ("email".into(), 3600, 0)]);
        assert!(chart_line(&render_at(&mut app, 140, 34), "email").contains('█'));

        // narrow: b shows the chart below the table; Esc closes it, Enter switches to details
        render(&mut app); // the window is now narrow
        assert!(!app.wide && !render(&mut app).contains("Hours per tag"));
        press(&mut app, 'b');
        let screen = render(&mut app);
        println!("{screen}");
        let title = screen.lines().find(|l| l.contains("Summary · Day")).unwrap();
        assert!(!title.contains("Hours per tag") && screen.contains("Hours per tag ─"));
        key(&mut app, KeyCode::Esc);
        assert!(!app.chart && !app.quit);
        press(&mut app, 'b');
        key(&mut app, KeyCode::Enter);
        assert!(!app.chart && app.details);
        assert!(render(&mut app).contains("Entry details"));
        let _ = fs::remove_dir_all(&root);
    }

    /// The chart's line for `tag` (the part right of the summary table, if any).
    fn chart_line(screen: &str, tag: &str) -> String {
        screen
            .lines()
            .map(|l| l.rsplit("││").next().unwrap_or(l))
            .find(|l| l.contains(tag) && l.contains('█'))
            .unwrap_or_else(|| panic!("no chart bar for {tag}:\n{screen}"))
            .to_string()
    }

    #[test]
    fn neighboring_bars_have_different_colors() {
        let total = |tag: &str| TagTotal { tag: tag.to_string(), share: 1.0, shared: 0.0 };
        // more bars than colors, with "(no tag)" in between
        let mut totals: Vec<TagTotal> = (0..20).map(|i| total(&format!("t{i}"))).collect();
        totals.insert(5, total(NO_TAG));
        let colors = bar_colors(&totals);
        assert_eq!(colors[0], TAG_COLORS[0]);
        assert_eq!(colors[5], GRAY_COLORS, "no tag is gray");
        assert_eq!(colors[6], TAG_COLORS[5], "gray doesn't use up a color");
        for pair in colors.windows(2) {
            assert_ne!(pair[0].0, pair[1].0, "neighbors differ: {pair:?}");
        }
        // the palette itself: no color twice, and it wraps around to a different one
        for (i, c) in TAG_COLORS.iter().enumerate() {
            assert!(TAG_COLORS.iter().skip(i + 1).all(|d| d != c));
        }

        // drawn: the bars top to bottom take the palette in order
        let root = sample_db("colors");
        let mut app = App::new(root.clone());
        press(&mut app, '1');
        let mut terminal = Terminal::new(TestBackend::new(140, 34)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let bar_color = |tag: &str| {
            let line = |y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>();
            let y = (0..buf.area.height).find(|&y| line(y).contains('█') && line(y).contains(tag)).unwrap();
            let x = (0..buf.area.width).find(|&x| buf[(x, y)].symbol() == "█").unwrap();
            buf[(x, y)].fg
        };
        // today: meetings 2 h (first bar), email 1 h (second)
        assert_eq!(bar_color("meetings"), TAG_COLORS[0].0);
        assert_eq!(bar_color("email"), TAG_COLORS[1].0);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn wide_chart_shrinks_in_low_windows() {
        let root = sample_db("low");
        let mut app = App::new(root.clone());
        press(&mut app, '4');
        // room for everything
        let screen = render_at(&mut app, 140, 27);
        assert!(chart_line(&screen, "email").contains('█') && screen.contains("Total 7.00 h  "));
        // enough for the three bars, not for the total line
        let screen = render_at(&mut app, 140, 25);
        println!("{screen}");
        assert!(chart_line(&screen, "email").contains('█'));
        assert!(!screen.lines().any(|l| l.contains("││  Total")));
        // one bar and a "more" line
        let screen = render_at(&mut app, 140, 24);
        println!("{screen}");
        assert!(chart_line(&screen, "planning").contains('█'));
        assert!(screen.contains("… 2 more tags, 3.00 h"));
        assert!(screen.contains("Hours per tag · 3"), "count shown when tags are cut off");
        // too low for the chart: details only
        let screen = render_at(&mut app, 140, 22);
        assert!(screen.contains("Entry details") && !screen.contains("Hours per tag"));
        let _ = fs::remove_dir_all(&root);
    }

    /// `TagTotal`s as (tag, share, shared) in whole seconds, for comparing.
    fn simple(totals: &[TagTotal]) -> Vec<(String, i64, i64)> {
        totals.iter().map(|t| (t.tag.clone(), t.share.round() as i64, t.shared.round() as i64)).collect()
    }

    #[test]
    fn tag_totals_split_time_evenly() {
        let root = env::temp_dir().join(format!("timew-tui-test-multitag-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("data")).unwrap();
        let tz = jiff::tz::TimeZone::system();
        let today = jiff::Zoned::now().date();
        let at = |h: i8| timew_core::interval::format_iso(today.at(h, 0, 0, 0).to_zoned(tz.clone()).unwrap().timestamp().as_second());
        // 8-10 "a b", 10-11 untagged, 11-14 "a b c", 14-15 "c"
        let lines = format!(
            "inc {} - {} # a b\ninc {} - {}\ninc {} - {} # a b c\ninc {} - {} # c\n",
            at(8), at(10), at(10), at(11), at(11), at(14), at(14), at(15)
        );
        fs::write(root.join("data").join(format!("{:04}-{:02}.data", today.year(), today.month())), lines).unwrap();
        let mut app = App::new(root.clone());
        press(&mut app, '1');
        let totals = tag_totals(&app);
        assert_eq!(
            simple(&totals),
            vec![
                // a and b: 2h/2 + 3h/3 = 2h own share; shared 1h + 2h = 3h
                ("a".into(), 7200, 3 * 3600),
                ("b".into(), 7200, 3 * 3600),
                // c: 3h/3 + 1h = 2h own share; shared 2h (same share, less shared: after a and b)
                ("c".into(), 7200, 7200),
                ("(no tag)".into(), 3600, 0),
            ]
        );
        // the own shares add up exactly to the total (7 h)
        let shares: f64 = totals.iter().map(|t| t.share).sum();
        assert!((shares - app.summary.grand_total as f64).abs() < 1e-6);
        assert_eq!(app.summary.grand_total, 7 * 3600);
        // own share + shared = the full time of the entries with that tag
        assert_eq!(totals[0].share + totals[0].shared, 5.0 * 3600.0);

        // the chart shows the share in yellow, the shared time as "+", and a legend
        let screen = render_at(&mut app, 140, 34);
        println!("{screen}");
        let a = chart_line(&screen, "a ");
        assert!(a.contains(" 2.00 h") && a.contains("+3.00 h") && a.contains("29 %"), "{a}");
        let none = chart_line(&screen, "(no tag)");
        assert!(!none.contains('+'), "{none}");
        assert!(screen.contains("own share") && screen.contains("shared (+)"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn stacked_bars_use_eighth_blocks_in_two_shades() {
        let (s, l) = (Color::Indexed(1), Color::Indexed(2));
        let parts = |strong, light| -> Vec<(String, Style)> {
            stacked_bar(strong, light, 4, s, l).into_iter().map(|sp| (sp.content.to_string(), sp.style)).collect()
        };
        let fg = |c| Style::new().fg(c);
        assert_eq!(parts(1.0, 0.0), vec![("████".into(), fg(s))]);
        assert_eq!(parts(0.5, 0.0), vec![("██".into(), fg(s)), ("  ".into(), Style::new())]);
        assert_eq!(parts(0.0, 0.0), vec![("    ".into(), Style::new())]);
        assert_eq!(parts(1.0 / 32.0, 0.0), vec![("▏".into(), fg(s)), ("   ".into(), Style::new())]);
        // 2.25 cells strong
        assert_eq!(parts(0.5625, 0.0), vec![("██▎".into(), fg(s)), (" ".into(), Style::new())]);
        // half strong, a quarter light
        assert_eq!(parts(0.5, 0.25), vec![("██".into(), fg(s)), ("█".into(), fg(l)), (" ".into(), Style::new())]);
        // the strong part ends inside a cell that the light part continues
        assert_eq!(
            parts(0.5625, 0.4375),
            vec![("██".into(), fg(s)), ("▎".into(), Style::new().fg(s).bg(l)), ("█".into(), fg(l))]
        );
        // always exactly `width` cells
        for (a, b) in [(0.0, 0.0), (0.3, 0.3), (0.7, 0.3), (1.0, 0.0), (0.1, 0.05)] {
            let w: usize = parts(a, b).iter().map(|(t, _)| t.chars().count()).sum();
            assert_eq!(w, 4, "{a} {b}");
        }
    }

    #[test]
    fn help_line_always_ends_with_quit() {
        let root = sample_db("help");
        let mut app = App::new(root.clone());
        for w in [40u16, 60, 90, 140] {
            let screen = render_at(&mut app, w, 24);
            let help = screen.lines().last().unwrap();
            assert!(help.ends_with("q quit"), "width {w}: {help}");
            assert!(help.chars().count() <= w as usize);
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn question_mark_opens_the_key_legend() {
        let root = sample_db("legend");
        let mut app = App::new(root.clone());

        press(&mut app, '?');
        assert_eq!(app.legend, Some(0));
        let screen = render_at(&mut app, 100, 60);
        println!("{screen}");
        assert!(screen.contains(" Keys ") && screen.contains("Periods") && screen.contains("hours-per-tag chart"));
        // every line fits at this height: no scrolling
        press(&mut app, 'j');
        render_at(&mut app, 100, 60);
        assert_eq!(app.legend, Some(0));

        // the legend takes all keys: these don't change the filter or quit
        press(&mut app, '1');
        assert_eq!(app.filter, Filter::Month);
        key(&mut app, KeyCode::Esc);
        assert!(app.legend.is_none() && !app.quit);

        // in a small window it scrolls, and the offset is clamped
        press(&mut app, '?');
        let screen = render_at(&mut app, 60, 20);
        println!("{screen}");
        assert!(screen.contains("Moving") && !screen.contains("Ctrl+C"));
        press(&mut app, 'G');
        let screen = render_at(&mut app, 60, 20);
        println!("{screen}");
        assert!(screen.contains("Ctrl+C") && !screen.contains("Moving"));
        press(&mut app, 'g');
        assert_eq!(app.legend, Some(0));
        press(&mut app, '?');
        assert!(app.legend.is_none());

        // a click anywhere closes it
        press(&mut app, '?');
        app.on_mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: 0, row: 0, modifiers: KeyModifiers::NONE });
        assert!(app.legend.is_none());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn footer_offers_help_only_when_cut_off() {
        let root = sample_db("footer-help");
        let mut app = App::new(root.clone());
        let footer = |app: &mut App, w| render_at(app, w, 34).lines().last().unwrap().to_string();
        let wide = footer(&mut app, 140);
        assert!(wide.contains("r reload") && !wide.contains("? help"), "{wide}");
        for w in [20u16, 40, 60, 90] {
            let narrow = footer(&mut app, w);
            assert!(narrow.starts_with(" ? help  ") && narrow.ends_with("q quit"), "width {w}: {narrow}");
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn empty_database_shows_message() {
        let root = env::temp_dir().join(format!("timew-tui-test-empty-{}", std::process::id()));
        fs::create_dir_all(root.join("data")).unwrap();
        let mut app = App::new(root.clone());
        assert!(render(&mut app).contains("No data found in the range"));
        let _ = fs::remove_dir_all(&root);
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

    #[test]
    fn edit_changes_start_and_end() {
        let root = sample_db("edit-save");
        let mut app = App::new(root.clone());
        app.config_dir = root.clone();
        press(&mut app, '1');
        render_at(&mut app, 140, 24);
        assert_eq!(app.selected().unwrap().interval.tags, vec!["meetings".to_string()]);

        press(&mut app, 'e');
        let screen = render_at(&mut app, 140, 24);
        println!("{screen}");
        assert!(screen.contains("Edit entry") && !screen.contains("Entry details"));
        let today = local_date(jiff::Timestamp::now().as_second());
        assert!(screen.lines().any(|l| l.contains("Start ") && l.contains(&today) && l.contains("08:00")));
        assert!(screen.lines().any(|l| l.contains("End ") && l.contains(&today) && l.contains("10:00")));

        // start 07:30, end 09:45: typing overwrites the digits
        key(&mut app, KeyCode::Tab);
        type_text(&mut app, "0730");
        key(&mut app, KeyCode::Tab);
        key(&mut app, KeyCode::Tab);
        type_text(&mut app, "0945");
        assert_eq!(app.edit.as_ref().unwrap().fields[1], "07:30");
        assert_eq!(app.edit.as_ref().unwrap().fields[3], "09:45");
        assert!(render_at(&mut app, 140, 24).contains("2:15"), "duration preview");
        key(&mut app, KeyCode::Enter);

        let screen = render_at(&mut app, 140, 24);
        assert!(app.edit.is_none());
        assert!(screen.contains("Saved."), "{screen}");
        let iv = &app.selected().unwrap().interval;
        assert_eq!(iv.tags, vec!["meetings".to_string()], "the edited entry stays selected");
        let data = today_file(&root);
        assert!(data.contains(&format!("inc {} - {} # meetings", today_at(7, 30), today_at(9, 45))), "{data}");
        assert!(!root.join("data/undo.data").exists(), "no undo journal is written");
        // the notice goes away with the next key
        press(&mut app, 'j');
        assert!(!render_at(&mut app, 140, 24).contains("Saved."));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn edit_refuses_overlaps_and_bad_input() {
        let root = sample_db("edit-refuse");
        let mut app = App::new(root.clone());
        app.config_dir = root.clone();
        press(&mut app, '1');
        render(&mut app);
        let before = today_file(&root);

        // narrow window: the form opens below the table
        press(&mut app, 'e');
        let screen = render(&mut app);
        assert!(!app.wide && screen.contains("Edit entry"));
        let header = screen.lines().find(|l| l.contains("Summary · Day")).unwrap();
        assert!(!header.contains("Edit entry"));

        key(&mut app, KeyCode::BackTab); // annotation
        key(&mut app, KeyCode::BackTab); // tags
        key(&mut app, KeyCode::BackTab); // end time
        ctrl_u(&mut app);
        type_text(&mut app, "25:00");
        key(&mut app, KeyCode::Enter);
        assert!(render(&mut app).contains("End time: use HH:MM."));

        ctrl_u(&mut app);
        type_text(&mut app, "10:30"); // runs into the email entry at 10:00
        key(&mut app, KeyCode::Enter);
        let screen = render(&mut app);
        assert!(screen.contains("Overlaps another entry. Shift+Enter adjusts it."), "{screen}");
        assert!(app.edit.is_some(), "the form stays open");

        ctrl_u(&mut app);
        type_text(&mut app, "07:00"); // before the start
        key(&mut app, KeyCode::Enter);
        assert!(render(&mut app).contains("The end must be after the start."));

        key(&mut app, KeyCode::Esc);
        assert!(app.edit.is_none() && !app.quit);
        assert_eq!(today_file(&root), before, "nothing was written");
        assert!(!root.join("data/undo.data").exists());
        let _ = fs::remove_dir_all(&root);
    }


    #[test]
    fn edit_fields_overwrite_and_move_the_cursor() {
        let root = sample_db("edit-cursor");
        let mut app = App::new(root.clone());
        press(&mut app, '1');
        render_at(&mut app, 140, 24);
        press(&mut app, 'e');
        // the date field: retype it digit by digit
        type_text(&mut app, "20251231");
        let form = app.edit.as_ref().unwrap();
        assert_eq!((form.fields[0].as_str(), form.field, form.cursor), ("2025-12-31", 0, 10));
        // → at the end goes to the time field, ← at its start back again
        key(&mut app, KeyCode::Right);
        assert_eq!((app.edit.as_ref().unwrap().field, app.edit.as_ref().unwrap().cursor), (1, 0));
        key(&mut app, KeyCode::Left);
        assert_eq!((app.edit.as_ref().unwrap().field, app.edit.as_ref().unwrap().cursor), (0, 10));
        // backspace deletes before the cursor, typing then appends
        key(&mut app, KeyCode::Backspace);
        key(&mut app, KeyCode::Backspace);
        type_text(&mut app, "30");
        assert_eq!(app.edit.as_ref().unwrap().fields[0], "2025-12-30");
        key(&mut app, KeyCode::Home);
        key(&mut app, KeyCode::Delete);
        assert_eq!(app.edit.as_ref().unwrap().fields[0], "025-12-30");
        // the terminal cursor follows
        key(&mut app, KeyCode::End);
        let mut terminal = Terminal::new(TestBackend::new(140, 24)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let field = app.edit_fields[0];
        assert_eq!(terminal.get_cursor_position().unwrap(), Position::new(field.x + 1 + 9, field.y));
        let _ = fs::remove_dir_all(&root);
    }


    fn key_with(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
        app.on_key(KeyEvent { code, modifiers, kind: KeyEventKind::Press, state: KeyEventState::NONE });
    }

    #[test]
    fn shift_enter_adjusts_overlapping_entries() {
        for (name, modifier) in [("shift", KeyModifiers::SHIFT), ("alt", KeyModifiers::ALT)] {
            let root = sample_db(&format!("edit-adjust-{name}"));
            let mut app = App::new(root.clone());
            app.config_dir = root.clone();
            press(&mut app, '1');
            render_at(&mut app, 140, 24);
            // meetings 08-10 now ends 10:30, into email 10-11
            press(&mut app, 'e');
            key(&mut app, KeyCode::Up); // annotation
            key(&mut app, KeyCode::Up); // tags
            key(&mut app, KeyCode::Up); // end time
            type_text(&mut app, "1030");
            key(&mut app, KeyCode::Enter);
            assert!(app.edit.as_ref().unwrap().error.is_some(), "plain Enter refuses the overlap");
            key_with(&mut app, KeyCode::Enter, modifier);
            assert!(app.edit.is_none());
            let screen = render_at(&mut app, 140, 24);
            assert!(screen.contains("Saved. 1 overlapping entry adjusted."), "{screen}");
            let data = today_file(&root);
            assert!(data.contains(&format!("inc {} - {} # meetings", today_at(8, 0), today_at(10, 30))), "{data}");
            assert!(data.contains(&format!("inc {} - {} # email", today_at(10, 30), today_at(11, 0))), "{data}");
            let _ = fs::remove_dir_all(&root);
        }
    }


    #[test]
    fn focused_view_has_a_yellow_border() {
        let root = sample_db("focus");
        let mut app = App::new(root.clone());
        press(&mut app, '1');
        // color of the summary's top-left corner
        let corner = |app: &mut App| {
            let mut terminal = Terminal::new(TestBackend::new(140, 24)).unwrap();
            terminal.draw(|f| draw(f, app)).unwrap();
            let buf = terminal.backend().buffer().clone();
            let y = (0..buf.area.height).find(|&y| (0..20).any(|x| buf[(x, y)].symbol() == "S" && buf[(x + 1, y)].symbol() == "u")).unwrap();
            assert_eq!(buf[(0, y)].symbol(), "╭");
            buf[(0, y)].fg
        };
        assert_eq!(corner(&mut app), FOCUS, "the table has the focus");
        press(&mut app, 'e');
        assert_ne!(corner(&mut app), FOCUS, "the edit form has it");
        key(&mut app, KeyCode::Esc);
        assert_eq!(corner(&mut app), FOCUS);
        press(&mut app, 't');
        assert_ne!(corner(&mut app), FOCUS, "the tag list has it");
        key(&mut app, KeyCode::Esc);
        press(&mut app, '?');
        assert_ne!(corner(&mut app), FOCUS, "the legend has it");
        let _ = fs::remove_dir_all(&root);
    }


    #[test]
    fn tags_as_text() {
        let tags = vec!["client a".to_string(), "meetings".to_string(), "say \"hi\"".to_string()];
        let text = tags_to_text(&tags);
        assert_eq!(text, "\"client a\" meetings \"say \\\"hi\\\"\"");
        assert_eq!(parse_tags(&text).unwrap(), tags);
        assert_eq!(parse_tags("  a   b-c  ").unwrap(), vec!["a", "b-c"]);
        assert_eq!(parse_tags("").unwrap(), Vec::<String>::new());
        assert!(parse_tags("a \"b c").is_err());
    }

    #[test]
    fn edit_changes_tags() {
        let root = sample_db("edit-tags");
        let mut app = App::new(root.clone());
        app.config_dir = root.clone();
        press(&mut app, '1');
        render_at(&mut app, 140, 24);
        press(&mut app, 'e');
        key(&mut app, KeyCode::Up); // from the start date up to the annotation
        key(&mut app, KeyCode::Up); // and the tags
        assert_eq!(app.edit.as_ref().unwrap().field, TAGS);
        let screen = render_at(&mut app, 140, 24);
        assert!(screen.contains("separated by spaces"), "{screen}");
        key(&mut app, KeyCode::End);
        type_text(&mut app, " \"client a");
        key(&mut app, KeyCode::Enter);
        assert!(render_at(&mut app, 140, 24).contains("Tags: a quote is not closed."));
        type_text(&mut app, "\" planning");
        key(&mut app, KeyCode::Enter);
        assert!(app.edit.is_none());
        assert_eq!(app.selected().unwrap().interval.tags, vec!["client a", "meetings", "planning"]);
        let data = today_file(&root);
        assert!(data.contains(&format!("inc {} - {} # \"client a\" meetings planning", today_at(8, 0), today_at(10, 0))), "{data}");
        let tags_db = fs::read_to_string(root.join("data/tags.data")).unwrap();
        assert!(tags_db.contains("\"client a\":{\"count\":1}") && tags_db.contains("\"planning\":{\"count\":2}"), "{tags_db}");

        // remove all tags
        press(&mut app, 'e');
        key(&mut app, KeyCode::BackTab);
        key(&mut app, KeyCode::BackTab);
        ctrl_u(&mut app);
        key(&mut app, KeyCode::Enter);
        assert!(app.selected().unwrap().interval.tags.is_empty());
        assert!(today_file(&root).contains(&format!("inc {} - {}\n", today_at(8, 0), today_at(10, 0))));
        let _ = fs::remove_dir_all(&root);
    }



    #[test]
    fn edit_changes_the_annotation() {
        let root = sample_db("edit-annotation");
        let mut app = App::new(root.clone());
        app.config_dir = root.clone();
        press(&mut app, '1');
        render_at(&mut app, 140, 24);
        press(&mut app, 'e');
        key(&mut app, KeyCode::Up); // the annotation is just above the start
        assert_eq!(app.edit.as_ref().unwrap().field, ANNOTATION);
        assert!(render_at(&mut app, 140, 24).contains("any text; empty removes the annotation"));
        type_text(&mut app, "weekly sync, \"room 2\"");
        key(&mut app, KeyCode::Enter);
        assert!(app.edit.is_none());
        let iv = &app.selected().unwrap().interval;
        assert_eq!(iv.annotation.as_deref(), Some("weekly sync, \"room 2\""));
        let data = today_file(&root);
        assert!(data.contains(&format!("inc {} - {} # meetings # \"weekly sync, \\\"room 2\\\"\"", today_at(8, 0), today_at(10, 0))), "{data}");
        // the details panel shows it
        assert!(render_at(&mut app, 140, 24).contains("weekly sync"));

        // the form shows the current annotation; clearing removes it
        press(&mut app, 'e');
        assert_eq!(app.edit.as_ref().unwrap().fields[ANNOTATION], "weekly sync, \"room 2\"");
        key(&mut app, KeyCode::Up);
        ctrl_u(&mut app);
        type_text(&mut app, "   ");
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.selected().unwrap().interval.annotation, None);
        assert!(today_file(&root).contains(&format!("inc {} - {} # meetings\n", today_at(8, 0), today_at(10, 0))));
        let _ = fs::remove_dir_all(&root);
    }


    fn footer(app: &mut App) -> String {
        render_at(app, 140, 24).lines().last().unwrap().to_string()
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

    #[test]
    fn u_undoes_the_last_saved_edit() {
        let root = sample_db("edit-undo");
        let mut app = App::new(root.clone());
        app.config_dir = root.clone();
        press(&mut app, '1');
        let original = today_file(&root);
        assert!(!footer(&mut app).contains("u undo"), "nothing to undo yet");
        press(&mut app, 'u'); // does nothing
        assert_eq!(today_file(&root), original);

        // two edits: meetings ends 09:30, then it ends 10:30 adjusting email
        edit_end(&mut app, "0930", false);
        let first = today_file(&root);
        assert!(footer(&mut app).starts_with(" u undo  "), "{}", footer(&mut app));
        edit_end(&mut app, "1030", true);
        assert!(today_file(&root).contains(&format!("inc {} - {} # email", today_at(10, 30), today_at(11, 0))));

        // u reverts the last edit only, the adjusted email entry included
        press(&mut app, 'u');
        assert!(render_at(&mut app, 140, 24).contains("Undone."));
        assert_eq!(today_file(&root), first);
        assert_eq!(app.selected().unwrap().interval.tags, vec!["meetings".to_string()]);
        assert!(!footer(&mut app).contains("u undo"), "only one level of undo");
        press(&mut app, 'u');
        assert_eq!(today_file(&root), first);
        assert!(!root.join("data/undo.data").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn u_refuses_when_the_entry_changed_since() {
        let root = sample_db("edit-undo-stale");
        let mut app = App::new(root.clone());
        app.config_dir = root.clone();
        press(&mut app, '1');
        edit_end(&mut app, "0930", false);

        // another program changes the edited entry afterwards
        let mut paths = Paths::from_env();
        paths.db_dir = root.clone();
        paths.config_dir = root.clone();
        let mut db = Database::open(root.join("data")).unwrap();
        let meetings = db.all_tracked().unwrap().remove(1).interval;
        assert_eq!(meetings.tags, vec!["meetings".to_string()]);
        let mut changed = meetings.clone();
        changed.add_tag("x");
        timew_core::modify_interval(&paths, &meetings, &changed, false).unwrap();
        let after_other = today_file(&root);

        press(&mut app, 'u');
        let screen = render_at(&mut app, 140, 24);
        assert!(screen.contains("Not undone: The entry was changed by another program since."), "{screen}");
        assert_eq!(today_file(&root), after_other, "nothing was reverted");
        assert!(!footer(&mut app).contains("u undo"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn u_refuses_to_restore_into_a_new_entry() {
        let root = sample_db("edit-undo-overlap");
        let mut app = App::new(root.clone());
        app.config_dir = root.clone();
        press(&mut app, '1');
        edit_end(&mut app, "0930", false); // meetings 08:00-09:30, free until 10:00

        // another program records 09:40-09:50
        let mut paths = Paths::from_env();
        paths.db_dir = root.clone();
        paths.config_dir = root.clone();
        let mut db = Database::open(root.join("data")).unwrap();
        let mut new = db.all_tracked().unwrap().remove(1).interval;
        let start = new.range.end + 600;
        new.range = Range::new(start, start + 600);
        new.tags = vec!["new".into()];
        db.add_interval(&new).unwrap();
        db.commit().unwrap();
        let before = today_file(&root);

        press(&mut app, 'u');
        assert!(render_at(&mut app, 140, 24).contains("Not undone: It would overlap an entry added since."));
        assert_eq!(today_file(&root), before);
        let _ = fs::remove_dir_all(&root);
    }


    #[test]
    fn i_shows_version_and_project_page() {
        let root = sample_db("info");
        let mut app = App::new(root.clone());
        press(&mut app, '1');
        assert!(footer(&mut app).contains("i info"), "{}", footer(&mut app));
        press(&mut app, 'i');
        let screen = render_at(&mut app, 140, 24);
        println!("{screen}");
        assert!(screen.contains(" Info "));
        assert!(screen.contains(&format!("timew-tui {}", env!("CARGO_PKG_VERSION"))));
        assert!(screen.contains(&format!("timew-core {}", timew_core::VERSION)));
        assert!(screen.contains("https://github.com/chnoack/timew-rs"));
        // it takes the keys: q closes it instead of quitting
        press(&mut app, 'q');
        assert!(!app.info && !app.quit);
        assert!(!render_at(&mut app, 140, 24).contains(" Info "));
        // a click closes it too
        press(&mut app, 'i');
        app.on_mouse(MouseEvent { kind: MouseEventKind::Down(MouseButton::Left), column: 0, row: 0, modifiers: KeyModifiers::NONE });
        assert!(!app.info);
        let _ = fs::remove_dir_all(&root);
    }


    #[test]
    fn esc_resets_all_filters() {
        let root = sample_db("esc-reset");
        let mut app = App::new(root.clone());
        let default_range = app.range;
        // year, a tag, then Esc: back to the current month without a tag
        press(&mut app, '4');
        press(&mut app, 't');
        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Enter);
        assert!(app.tag.is_some() && app.filter == Filter::Year);
        key(&mut app, KeyCode::Esc);
        assert!(!app.quit);
        assert_eq!((app.filter, app.month, app.year, app.tag.clone()), (Filter::Month, None, None, None));
        assert_eq!(app.range, default_range);
        assert!(render(&mut app).contains("All filters reset"));
        // a picked month and a custom range are reset too
        app.month = Some((2020, 5));
        app.load();
        key(&mut app, KeyCode::Esc);
        assert_eq!((app.filter, app.month), (Filter::Month, None));
        app.filter = Filter::Custom;
        app.load();
        key(&mut app, KeyCode::Esc);
        assert_eq!(app.filter, Filter::Month);
        // nothing to reset: nothing happens (and no quitting)
        key(&mut app, KeyCode::Esc);
        assert!(!app.quit && app.notice.is_none());
        let _ = fs::remove_dir_all(&root);
    }

}
