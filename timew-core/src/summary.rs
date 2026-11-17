//! The rows of a summary: one row per interval and day, with daily and
//! grand totals, every cell already formatted as text. Intervals with a tag
//! starting with one underscore (`_break`) get rows but don't count
//! towards the totals. `timew-cli summary`
//! lays them out as a table; `timew-tui` shows them in its list.

use jiff::civil::Date;

use crate::database::Tracked;
use crate::datetime::{self, local, make_local};
use crate::range::{Epoch, Range};

/// One summary row, with every cell already formatted as text.
/// `week`/`date`/`day_name` are only set on the first row of a day, and
/// `total` only on the last one (empty otherwise).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub week: Option<String>,
    pub date: Option<String>,
    pub day_name: Option<String>,
    pub id: String,
    pub tags: String,
    pub annotation: String,
    pub start: String,
    pub end: String,
    pub time: String,
    pub total: String,
    /// Whether the row's time counts towards the daily and grand totals
    /// (not for an interval with an `_tag`, see
    /// [`Interval::counts_in_totals`](crate::interval::Interval::counts_in_totals)).
    pub counted: bool,
    /// Index into the `tracked` slice passed to `build` of the interval
    /// this row was derived from (a multi-day interval yields several rows
    /// with the same index).
    pub source: usize,
}

/// The rows of a summary plus the grand total in seconds (of the counted
/// rows only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub rows: Vec<Row>,
    pub grand_total: i64,
}

/// Build the summary rows for `tracked` (oldest first) over `range`. An
/// interval crossing midnight gets one row per day; a running one counts
/// up to now.
pub fn build(range: &Range, tracked: &[Tracked]) -> Summary {
    build_at(range, tracked, datetime::now())
}

pub fn build_at(range: &Range, tracked: &[Tracked], now: Epoch) -> Summary {
    if tracked.is_empty() && !(range.is_started() && range.is_ended()) {
        return Summary { rows: Vec::new(), grand_total: 0 };
    }
    let first_start = if range.is_started() { range.start } else { tracked.first().unwrap().interval.range.start };
    let last_end = if range.is_ended() { range.end } else { tracked.last().unwrap().interval.range.end };
    let last_end = if last_end == 0 { now } else { last_end };

    let mut rows: Vec<Row> = Vec::with_capacity(tracked.len() + 8);
    let mut grand_total = 0;
    let mut day: Date = local(first_start).date();
    // The intervals are normally sorted by start; then each day only needs
    // to look at the ones from `first` on, up to the first one starting
    // after the day. Otherwise every interval is checked on every day.
    let sorted = tracked.windows(2).all(|w| w[0].interval.range.start <= w[1].interval.range.start);
    let mut first = 0;
    loop {
        let day_start = midnight(day);
        if day_start >= last_end {
            break;
        }
        let next = day.tomorrow().unwrap();
        let day_range = Range::new(day_start, midnight(next));
        let first_row = rows.len();
        let mut daily_total = 0;

        if sorted {
            // skip what ended before this day (it can't touch later days either)
            while let Some(t) = tracked.get(first) {
                let r = &t.interval.range;
                if r.is_ended() && r.end <= day_start && r.start < day_start {
                    first += 1;
                } else {
                    break;
                }
            }
        }
        let candidates = tracked.iter().enumerate().skip(if sorted { first } else { 0 });
        let candidates = candidates.take_while(|(_, t)| !sorted || t.interval.range.start < day_range.end);
        for (source, t) in candidates.filter(|(_, t)| day_range.intersects(&t.interval.range)) {
            let iv = &t.interval;
            if iv.is_open() && day_start > now {
                continue;
            }
            let mut part = day_range.intersect(&iv.range);
            if iv.is_open() && iv.range.start > now {
                part.end = iv.range.start;
            } else if iv.is_open() && day_start <= now && part.end > now {
                part.end = now;
            }
            if part.start > part.end {
                part.start = part.end;
            }
            let total = part.end - part.start;
            let first = rows.len() == first_row;
            rows.push(Row {
                week: first.then(|| format!("W{}", day.iso_week_date().week())),
                date: first.then(|| format!("{:04}-{:02}-{:02}", day.year(), day.month(), day.day())),
                day_name: first.then(|| datetime::day_name_short(day_start)),
                id: format!("@{}", t.id),
                tags: iv.tags.join(", "),
                annotation: iv.annotation.clone().unwrap_or_default(),
                start: time_of_day(part.start, &day_range),
                end: if iv.is_open() { "-".to_string() } else { time_of_day(part.end, &day_range) },
                time: format_hours(total),
                total: String::new(),
                counted: iv.counts_in_totals(),
                source,
            });
            if iv.counts_in_totals() {
                daily_total += total;
            }
        }
        if rows.len() > first_row {
            rows.last_mut().unwrap().total = format_hours(daily_total);
        }
        grand_total += daily_total;
        day = next;
    }
    Summary { rows, grand_total }
}

/// `datetime::format_time(t)` for a `t` within `day`, without a time zone
/// lookup on days that have 24 hours (no daylight saving change).
fn time_of_day(t: Epoch, day: &Range) -> String {
    if day.end - day.start != 86400 {
        return datetime::format_time(t);
    }
    let secs = (t - day.start).rem_euclid(86400);
    format!("{}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60)
}

fn midnight(day: Date) -> Epoch {
    make_local(day.year() as i64, day.month() as i64, day.day() as i64, 0, 0, 0)
}

/// `H:MM:SS`, e.g. `1:30:00`, `39:00:00`.
pub fn format_hours(seconds: i64) -> String {
    crate::duration::format_hours(seconds)
}

/// `2025-03-01T08:00:00` in local time.
pub fn local_iso_extended(epoch: Epoch) -> String {
    datetime::format_local_iso(epoch)
}

/// The columns of [`to_csv`], in order: those of the `timew-tui` table
/// plus the annotation.
pub const CSV_HEADERS: [&str; 9] = ["Wk", "Date", "Day", "Tags", "Start", "End", "Time", "Total", "Annotation"];

/// The setting for the field separator of [`to_csv`].
pub const CSV_SEPARATOR_SETTING: &str = "export.csv.separator";

/// The field separator from the value of `export.csv.separator`: one
/// character, or `tab`; a comma when unset or empty.
pub fn csv_separator(value: Option<&str>) -> Result<char, String> {
    let value = value.unwrap_or("").trim();
    let mut chars = value.chars();
    match (chars.next(), chars.next()) {
        (None, _) => Ok(','),
        (Some(c), None) if c != '"' && c != '\n' && c != '\r' => Ok(c),
        _ if value.eq_ignore_ascii_case("tab") || value == "\\t" => Ok('\t'),
        _ => Err(format!(
            "{CSV_SEPARATOR_SETTING} must be one character (not a quote) or 'tab', not '{value}'."
        )),
    }
}

/// The rows as CSV (as in RFC 4180, but with `\n` line ends and any
/// `separator`: fields with the separator, a quote or a line break in
/// double quotes), with a heading line.
/// The cells are those of the `timew-tui` table: Wk, Date and Day on every
/// row (not only the first of a day), times as `H:MM`, the daily total in
/// decimal hours on the last row of a day.
pub fn to_csv(summary: &Summary, separator: char) -> String {
    let mut out = String::new();
    let mut line = |cells: &[&str]| {
        let fields: Vec<String> = cells.iter().map(|c| csv_field(c, separator)).collect();
        out.push_str(&fields.join(separator.encode_utf8(&mut [0; 4])));
        out.push('\n');
    };
    line(&CSV_HEADERS);
    let (mut week, mut date, mut day) = (String::new(), String::new(), String::new());
    for r in &summary.rows {
        if let (Some(w), Some(d), Some(n)) = (&r.week, &r.date, &r.day_name) {
            (week, date, day) = (w.clone(), d.clone(), n.clone());
        }
        let total = parse_hms(&r.total).map(|s| format!("{:.2}", s as f64 / 3600.0)).unwrap_or_default();
        line(&[
            &week,
            &date,
            &day,
            &r.tags,
            &without_seconds(&r.start),
            &without_seconds(&r.end),
            &without_seconds(&r.time),
            &total,
            &r.annotation,
        ]);
    }
    out
}

fn csv_field(s: &str, separator: char) -> String {
    if s.contains([separator, '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
}

/// `H:MM:SS` -> `H:MM` (seconds cut off); anything else unchanged.
fn without_seconds(s: &str) -> String {
    match parse_hms(s) {
        Some(_) => s[..s.len() - 3].to_string(),
        None => s.to_string(),
    }
}

/// Seconds of an `H:MM:SS` duration or time of day.
fn parse_hms(s: &str) -> Option<i64> {
    let mut parts = s.split(':');
    let (h, m, sec) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || m.len() != 2 || sec.len() != 2 {
        return None;
    }
    Some(h.parse::<i64>().ok()? * 3600 + m.parse::<i64>().ok()? * 60 + sec.parse::<i64>().ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interval::Interval;

    fn tracked(id: u32, start: Epoch, end: Epoch, tags: &[&str], annotation: Option<&str>) -> Tracked {
        let mut interval = Interval::new(Range::new(start, end));
        for t in tags {
            interval.add_tag(*t);
        }
        interval.annotation = annotation.map(str::to_string);
        Tracked { id, interval, synthetic: false }
    }

    #[test]
    fn underscore_tags_are_left_out_of_the_totals() {
        let day = make_local(2025, 3, 3, 0, 0, 0);
        let h = 3600;
        let t = vec![
            tracked(3, day + 8 * h, day + 10 * h, &["meetings"], None),
            tracked(2, day + 10 * h, day + 11 * h, &["_break", "meetings"], None),
            tracked(1, day + 11 * h, day + 12 * h, &["__double"], None),
        ];
        let summary = build_at(&Range::new(day, day + 24 * h), &t, day + 30 * h);
        assert_eq!(summary.rows.iter().map(|r| r.counted).collect::<Vec<_>>(), [true, false, true]);
        assert_eq!(summary.rows[1].time, "1:00:00", "the row keeps its own time");
        assert_eq!(summary.rows[2].total, "3:00:00", "the daily total leaves out the _break hour");
        assert_eq!(summary.grand_total, 3 * h);
        assert!(crate::interval::is_uncounted_tag("_"));
        assert!(!crate::interval::is_uncounted_tag("a_b"));
    }

    fn at(d: i64, h: i64, m: i64, s: i64) -> Epoch {
        make_local(2026, 10, d, h, m, s)
    }

    #[test]
    fn csv_has_a_heading_and_every_column_on_every_row() {
        let t = vec![
            tracked(1, at(1, 8, 0, 0), at(1, 9, 30, 15), &["a", "b"], Some("say \"hi\", then go")),
            tracked(2, at(1, 10, 0, 0), at(1, 10, 30, 0), &["c"], None),
        ];
        let summary = build_at(&Range::new(at(1, 0, 0, 0), at(2, 0, 0, 0)), &t, at(5, 0, 0, 0));
        let csv = to_csv(&summary, ',');
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], "Wk,Date,Day,Tags,Start,End,Time,Total,Annotation");
        assert_eq!(lines[1], "W40,2026-10-01,Thu,\"a, b\",8:00,9:30,1:30,,\"say \"\"hi\"\", then go\"");
        assert_eq!(lines[2], "W40,2026-10-01,Thu,c,10:00,10:30,0:30,2.00,");
        assert_eq!(lines.len(), 3);
        assert!(csv.ends_with('\n'));

        let csv = to_csv(&summary, ';');
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], "Wk;Date;Day;Tags;Start;End;Time;Total;Annotation");
        // the comma needs no quotes any more, the quote still does
        assert_eq!(lines[1], "W40;2026-10-01;Thu;a, b;8:00;9:30;1:30;;\"say \"\"hi\"\", then go\"");
        assert_eq!(lines[2], "W40;2026-10-01;Thu;c;10:00;10:30;0:30;2.00;");
        assert!(to_csv(&summary, '\t').starts_with("Wk\tDate\t"));
    }

    #[test]
    fn csv_separator_setting() {
        assert_eq!(csv_separator(None), Ok(','));
        assert_eq!(csv_separator(Some("")), Ok(','));
        assert_eq!(csv_separator(Some(";")), Ok(';'));
        assert_eq!(csv_separator(Some(" | ")), Ok('|'));
        assert_eq!(csv_separator(Some("tab")), Ok('\t'));
        assert_eq!(csv_separator(Some("TAB")), Ok('\t'));
        assert_eq!(csv_separator(Some("\\t")), Ok('\t'));
        assert!(csv_separator(Some(";;")).is_err());
        assert!(csv_separator(Some("\"")).is_err());
    }

    #[test]
    fn interval_across_midnight_gets_a_row_per_day() {
        let t = vec![tracked(1, at(1, 22, 0, 0), at(2, 2, 0, 0), &["late"], None)];
        let summary = build_at(&Range::new(at(1, 0, 0, 0), at(3, 0, 0, 0)), &t, at(5, 0, 0, 0));
        assert_eq!(summary.rows.len(), 2);
        assert_eq!(summary.rows[0].time, "2:00:00");
        assert_eq!(summary.rows[1].start, "0:00:00");
        assert_eq!(summary.grand_total, 4 * 3600);
    }

    #[test]
    fn open_interval_counts_to_now() {
        let t = vec![tracked(1, at(1, 9, 0, 0), 0, &["x"], None)];
        let summary = build_at(&Range::new(at(1, 0, 0, 0), at(2, 0, 0, 0)), &t, at(1, 10, 0, 0));
        assert_eq!(summary.rows[0].end, "-");
        assert_eq!(summary.rows[0].time, "1:00:00");
    }

    #[test]
    fn iso_week_numbers() {
        let start = make_local(2026, 12, 31, 8, 0, 0);
        let t = vec![tracked(1, start, start + 3600, &["x"], None)];
        let summary = build_at(&Range::new(start - 8 * 3600, start + 16 * 3600), &t, start + 86400);
        assert_eq!(summary.rows[0].week.as_deref(), Some("W53"));
    }
}
