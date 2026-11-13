//! `timew summary [<range>] [<tag> ...] | [@<id> ...]`: a table of the
//! intervals, one row per interval and day, with daily and grand totals.
//!
//! The default range is `:day` (`reports.summary.range`, else
//! `reports.range`). Columns can be switched with configuration and hints:
//!
//! | Column | Setting | Hint |
//! | --- | --- | --- |
//! | Wk | `reports.summary.weeks` (on) | |
//! | Day | `reports.summary.weekdays` (on) | |
//! | ID | `reports.summary.ids` (off) | `:ids` / `:no-ids` |
//! | Tags | `reports.summary.tags` (on) | `:tags` / `:no-tags` |
//! | Annotation | `reports.summary.annotations` (off) | `:annotations` / `:no-annotations` |
//!
//! A new configuration file turns IDs and annotations on. Holidays in the
//! range are listed below the table (`reports.summary.holidays`,
//! `:holidays` / `:no-holidays`).
//!
//! [`build`] produces the rows as data; `timew-tui` uses it directly.

use jiff::civil::Date;

use crate::color::Color;
use crate::database::Tracked;
use crate::datetime::{self, local, make_local};
use crate::range::{Epoch, Range};
use crate::session::Session;
use crate::table::{terminal_width, Table};

pub fn run(s: &mut Session) -> Result<i32, String> {
    let default = super::default_report_range(s, "summary", "day");
    let (range, tracked) = super::select_for_report(s, default)?;
    if tracked.is_empty() {
        if s.verbose() {
            s.out.push_str(&super::no_data_message(&range, &s.cli.tags()));
        }
        return Ok(0);
    }

    let color = s.color();
    let options = Options {
        weeks: s.rules.get_bool("reports.summary.weeks", true),
        weekdays: s.rules.get_bool("reports.summary.weekdays", true),
        ids: s.cli.hint_or("ids", s.rules.get_bool("reports.summary.ids", false)),
        tags: s.cli.hint_or("tags", s.rules.get_bool("reports.summary.tags", true)),
        annotations: s.cli.hint_or("annotations", s.rules.get_bool("reports.summary.annotations", false)),
        color,
        id_color: if color { Color::parse_or_none(s.rules.get("theme.colors.ids").unwrap_or("")) } else { Color::none() },
        width: terminal_width(),
    };
    let summary = build(&range, &tracked);
    let tag_colors: Vec<Color> = tracked.iter().map(|t| super::tags_color(&s.rules, &t.interval.tags)).collect();
    s.out.push('\n');
    s.out.push_str(&render(&summary, &options, &tag_colors));
    if s.cli.hint_or("holidays", s.rules.get_bool("reports.summary.holidays", true)) {
        s.out.push_str(&crate::exclusion::render_holidays(&s.rules, &range));
    }
    s.out.push('\n');
    Ok(0)
}

/// Which columns to show, and how.
pub struct Options {
    pub weeks: bool,
    pub weekdays: bool,
    pub ids: bool,
    pub tags: bool,
    pub annotations: bool,
    pub color: bool,
    pub id_color: Color,
    pub width: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            weeks: true,
            weekdays: true,
            ids: true,
            tags: true,
            annotations: true,
            color: false,
            id_color: Color::none(),
            width: 1000,
        }
    }
}

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
    /// Index into the `tracked` slice passed to `build` of the interval
    /// this row was derived from (a multi-day interval yields several rows
    /// with the same index).
    pub source: usize,
}

/// The rows of a summary plus the grand total in seconds.
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

    let mut rows: Vec<Row> = Vec::new();
    let mut grand_total = 0;
    let mut day: Date = local(first_start).date();
    loop {
        let day_start = midnight(day);
        if day_start >= last_end {
            break;
        }
        let next = day.tomorrow().unwrap();
        let day_range = Range::new(day_start, midnight(next));
        let first_row = rows.len();
        let mut daily_total = 0;

        for (source, t) in tracked.iter().enumerate().filter(|(_, t)| day_range.intersects(&t.interval.range)) {
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
                start: datetime::format_time(part.start),
                end: if iv.is_open() { "-".to_string() } else { datetime::format_time(part.end) },
                time: format_hours(total),
                total: String::new(),
                source,
            });
            daily_total += total;
        }
        if rows.len() > first_row {
            rows.last_mut().unwrap().total = format_hours(daily_total);
        }
        grand_total += daily_total;
        day = next;
    }
    Summary { rows, grand_total }
}

fn midnight(day: Date) -> Epoch {
    make_local(day.year() as i64, day.month() as i64, day.day() as i64, 0, 0, 0)
}

/// The summary as a text table. `tag_colors` has the color for each
/// tracked interval (indexed like `Row::source`).
pub fn render(summary: &Summary, o: &Options, tag_colors: &[Color]) -> String {
    let mut table = Table::new(o.width, o.color);
    let mut columns = Vec::new();
    let mut column = |table: &mut Table, name: &'static str, show: bool, left: bool| {
        if show {
            table.add_column(name, left);
            columns.push(name);
        }
    };
    column(&mut table, "Wk", o.weeks, true);
    column(&mut table, "Date", true, true);
    column(&mut table, "Day", o.weekdays, true);
    column(&mut table, "ID", o.ids, true);
    column(&mut table, "Tags", o.tags, true);
    column(&mut table, "Annotation", o.annotations, true);
    for name in ["Start", "End", "Time", "Total"] {
        column(&mut table, name, true, false);
    }
    let col = |name: &str| columns.iter().position(|c| *c == name);
    let total_col = col("Total").unwrap();

    for r in &summary.rows {
        let row = table.add_row();
        let cells: [(&str, &str, Color); 10] = [
            ("Wk", r.week.as_deref().unwrap_or(""), Color::none()),
            ("Date", r.date.as_deref().unwrap_or(""), Color::none()),
            ("Day", r.day_name.as_deref().unwrap_or(""), Color::none()),
            ("ID", &r.id, o.id_color),
            ("Tags", &r.tags, tag_colors.get(r.source).copied().unwrap_or_default()),
            ("Annotation", &r.annotation, Color::none()),
            ("Start", &r.start, Color::none()),
            ("End", &r.end, Color::none()),
            ("Time", &r.time, Color::none()),
            ("Total", &r.total, Color::none()),
        ];
        for (name, value, color) in cells {
            if let Some(c) = col(name) {
                table.set_colored(row, c, value, color);
            }
        }
    }
    let row = table.add_row();
    table.set_colored(row, total_col, " ", Color::underline());
    let row = table.add_row();
    table.set(row, total_col, format_hours(summary.grand_total));
    table.render()
}

/// `H:MM:SS`, e.g. `1:30:00`, `39:00:00`.
pub fn format_hours(seconds: i64) -> String {
    crate::duration::format_hours(seconds)
}

/// `2025-03-01T08:00:00` in local time.
pub fn local_iso_extended(epoch: Epoch) -> String {
    datetime::format_local_iso(epoch)
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

    fn at(d: i64, h: i64, m: i64, s: i64) -> Epoch {
        make_local(2026, 10, d, h, m, s)
    }

    #[test]
    fn two_intervals_one_day() {
        let t = vec![
            tracked(2, at(1, 9, 0, 0), at(1, 10, 30, 0), &["alpha"], None),
            tracked(1, at(1, 13, 0, 0), at(1, 14, 15, 30), &["beta", "gamma"], Some("a note")),
        ];
        let summary = build_at(&Range::new(at(1, 0, 0, 0), at(3, 0, 0, 0)), &t, at(5, 0, 0, 0));
        let expected = concat!(
            "Wk  Date       Day ID Tags        Annotation    Start      End    Time   Total\n",
            "--- ---------- --- -- ----------- ---------- -------- -------- ------- -------\n",
            "W40 2026-10-01 Thu @2 alpha                   9:00:00 10:30:00 1:30:00\n",
            "                   @1 beta, gamma a note     13:00:00 14:15:30 1:15:30 2:45:30\n",
            "\n",
            "                                                                       2:45:30\n",
        );
        assert_eq!(render(&summary, &Options::default(), &[]), expected);

        let options = Options { ids: false, annotations: false, ..Options::default() };
        let expected = concat!(
            "Wk  Date       Day Tags           Start      End    Time   Total\n",
            "--- ---------- --- ----------- -------- -------- ------- -------\n",
            "W40 2026-10-01 Thu alpha        9:00:00 10:30:00 1:30:00\n",
            "                   beta, gamma 13:00:00 14:15:30 1:15:30 2:45:30\n",
            "\n",
            "                                                         2:45:30\n",
        );
        assert_eq!(render(&summary, &options, &[]), expected);
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
