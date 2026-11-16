//! The state of the dialogs that change data: the edit form (`e`), the
//! start form (`s`) and the delete confirmation (`d`), with the parsing of
//! what is typed into them.

use timew_core::database::Tracked;
use timew_core::datetime::make_local;
use timew_core::interval::Interval;

use crate::format::local_date;

/// The delete confirmation (`d`): the entry, its ID, and the entry before
/// it, which `e` would extend over it.
#[derive(Debug, Clone)]
pub(crate) struct DeleteDialog {
    pub(crate) interval: Interval,
    pub(crate) id: u32,
    pub(crate) previous: Option<Interval>,
}

/// State of the start dialog (`s`): tags and annotation of a new entry.
#[derive(Debug, Clone, Default)]
pub(crate) struct StartForm {
    /// Tags, annotation.
    pub(crate) fields: [String; 2],
    pub(crate) field: usize,
    /// Cursor position (in characters) in the active field.
    pub(crate) cursor: usize,
    /// First visible character of the active field (it scrolls sideways).
    pub(crate) scroll: usize,
    pub(crate) error: Option<String>,
}

impl StartForm {
    pub(crate) fn len(&self) -> usize {
        self.fields[self.field].chars().count()
    }

    pub(crate) fn focus(&mut self, i: usize) {
        self.field = i % 2;
        self.cursor = self.len();
    }

    pub(crate) fn insert(&mut self, c: char) {
        let mut chars: Vec<char> = self.fields[self.field].chars().collect();
        if chars.len() < MAX_TEXT_LEN {
            chars.insert(self.cursor.min(chars.len()), c);
            self.cursor += 1;
            self.fields[self.field] = chars.into_iter().collect();
        }
        self.error = None;
    }

    pub(crate) fn remove(&mut self, at: usize) {
        let mut chars: Vec<char> = self.fields[self.field].chars().collect();
        if at < chars.len() {
            chars.remove(at);
            self.fields[self.field] = chars.into_iter().collect();
        }
        self.error = None;
    }
}

/// State of the edit form (`e`), shown in the details panel: the start
/// and end of the selected entry as date and time fields.
#[derive(Debug, Clone)]
pub(crate) struct EditForm {
    /// The entry as it is stored.
    pub(crate) original: Interval,
    pub(crate) id: u32,
    /// Start date, start time, end date, end time, tags, annotation.
    pub(crate) fields: [String; FIELDS],
    /// The fields as first shown; unchanged ones keep their exact value
    /// (times their seconds).
    initial: [String; FIELDS],
    /// Index into `fields` of the field being typed in.
    pub(crate) field: usize,
    /// Cursor position (in characters) in that field.
    pub(crate) cursor: usize,
    /// First visible character of the active text field (tags and
    /// annotation scroll sideways).
    pub(crate) scroll: usize,
    pub(crate) error: Option<String>,
}

/// Index of the tags field; the fields from here on are free text.
pub(crate) const TAGS: usize = 4;

pub(crate) const ANNOTATION: usize = 5;

pub(crate) const FIELDS: usize = 6;

/// Screen widths of the date and time fields: a leading space, the text,
/// room for the cursor. The tags field takes the rest of the line.
pub(crate) const EDIT_FIELD_WIDTHS: [u16; 4] = [12, 10, 12, 10];

const MAX_TEXT_LEN: usize = 500;

/// Tags and annotation are text fields: typing inserts, they scroll.
pub(crate) fn is_text(field: usize) -> bool {
    field >= TAGS
}

/// Tags as one line of text: separated by spaces, a tag with a space or a
/// quote in double quotes.
pub(crate) fn tags_to_text(tags: &[String]) -> String {
    tags.iter()
        .map(|t| if t.contains([' ', '"']) { format!("\"{}\"", t.replace('"', "\\\"")) } else { t.clone() })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The reverse of `tags_to_text`: words, or "quoted words" (with `\"` for
/// a quote inside).
pub(crate) fn parse_tags(text: &str) -> Result<Vec<String>, String> {
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
    pub(crate) fn new(t: &Tracked) -> Self {
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

    pub(crate) fn len(&self) -> usize {
        self.fields[self.field].chars().count()
    }

    /// Go to field `i`, with the cursor at its start (or end).
    pub(crate) fn focus(&mut self, i: usize, at_end: bool) {
        self.field = i % FIELDS;
        self.cursor = if at_end { self.len() } else { 0 };
    }

    /// Type `c` at the cursor. Over existing text it replaces the character
    /// under the cursor, stepping over the `-`/`:` separators, so a date or
    /// time can be retyped digit by digit; at the end it appends.
    /// In the tags and annotation fields, it inserts like an ordinary text
    /// field.
    pub(crate) fn type_char(&mut self, c: char) {
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

    pub(crate) fn remove(&mut self, at: usize) {
        let mut chars: Vec<char> = self.fields[self.field].chars().collect();
        if at < chars.len() {
            chars.remove(at);
            self.fields[self.field] = chars.into_iter().collect();
        }
        self.error = None;
    }

    /// The entry with the start, end, tags and annotation from the fields.
    pub(crate) fn edited(&self) -> Result<Interval, String> {
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
pub(crate) fn normalize_datetime(input: &str) -> String {
    let s = input.trim().replacen(' ', "T", 1);
    match s.split_once('T') {
        Some((date, time)) if time.len() == 5 => format!("{date}T{time}:00"),
        _ => s,
    }
}
