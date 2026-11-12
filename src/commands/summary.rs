//! `timew summary [<range>] [<tags>] [@id ...]`
//!
//! Clean-room port of `CmdSummary.cpp` + the row-building logic in
//! `SummaryTable::Builder::build()` (GothenburgBitFactory/timewarrior,
//! `develop` branch, commit 3699d2e), re-derived from reading that source
//! and empirically confirmed against the real binary for every layout rule
//! implemented here (see tests, several of which assert against literal
//! output captured via the sandboxed `TIMEWARRIORDB`).
//!
//! Scope decisions:
//!   - Columns shown: Wk, Date, Day, ID, Tags, Annotation, Start, End,
//!     Time, Total. Real timew's ID/Annotation columns are driven by
//!     `reports.summary.ids`/`.annotations` config keys that default to
//!     OFF in the compiled-in `Rules`, BUT the installer's
//!     auto-generated `timewarrior.cfg` always sets both to `yes` --
//!     confirmed by reading `paths.cpp::initializeDirs`'s `defaultConfig`
//!     vector and reproducing it in a throwaway sandbox with NO prior
//!     config file. Since every real-world install therefore shows these
//!     columns, and this project has no config system to make them
//!     optional anyway, they are always shown here.
//!   - Holidays and color output are out of scope entirely (no config/
//!     Rules system, no terminal color support planned).
//!   - Terminal-width-based column truncation (`Table::width`) is not
//!     reproduced; columns always size to their widest content, which
//!     matches real output whenever the terminal is wide enough (true for
//!     the realistic row counts this project's test data uses).
//!
//! Non-obvious, empirically-confirmed layout rules:
//!   - Default range (no range/hint given) is `:day` (today).
//!   - ids and tags/range are mutually exclusive (error if both given),
//!     matching export's same rule but with a summary-specific message.
//!   - Each column's width is independently content-driven: the max of
//!     its header length and the longest value that will appear in it
//!     (confirmed by widening ID from "ID" to "@12" when a double-digit id
//!     appears, and Tags/Total similarly).
//!   - Wk/Date/Day are only printed on the FIRST row of a given calendar
//!     day; subsequent same-day rows leave those columns blank.
//!   - Start/End/Time are right-aligned; Wk/Date/Day/ID/Tags/Annotation
//!     are left-aligned.
//!   - An interval's displayed Start/End/Time for a given day are the
//!     intersection of the interval with that calendar day -- a
//!     multi-day-spanning interval (including an open one) gets ONE ROW
//!     PER DAY it touches, confirmed empirically: a still-open interval
//!     started yesterday shows a row for yesterday (Start=actual start,
//!     End="-", Time=hours remaining in that day) AND a row for today
//!     (Start=0:00:00, End="-", Time=elapsed-today).
//!   - An open interval's End is the literal string `"-"`; its Time for
//!     "today" is computed against the current moment (clipped to now),
//!     not midnight.
//!   - The Total column is filled in only on the LAST row of each day
//!     (the running sum for that day), and a final two-row footer prints
//!     the grand total (an empty separator row, then the total itself).
//!   - Week numbers are ISO 8601 week-numbering (`jiff`'s
//!     `iso_week_date()`), confirmed via Dec 31 2026 rendering as `W53`.

use crate::cli::ParsedArgs;
use crate::database::{Database, Tracked};
use crate::range::{Epoch, Range};
use jiff::civil::Date;
use jiff::Zoned;

pub fn run(database: &mut Database, mut args: ParsedArgs) -> Result<String, String> {
    let ids = args.ids.clone();
    let range_arg = args.take_range()?;
    let tags: Vec<String> = args.tags().to_vec();

    if !ids.is_empty() && !tags.is_empty() {
        return Err("You cannot filter intervals by both, ids and tags.".to_string());
    }

    let (range, tracked): (Range, Vec<Tracked>) = if !ids.is_empty() {
        let found = database.tracked_by_ids(&ids)?;
        for (id, result) in ids.iter().zip(found.iter()) {
            if result.is_none() {
                return Err(format!("ID '@{id}' does not correspond to any tracking."));
            }
        }
        let tracked: Vec<Tracked> = found.into_iter().flatten().collect();
        // Real timew derives the display range from the first/last matched
        // interval's start/end when filtering by id (CmdSummary.cpp:
        // `Range{tracked.front().start, tracked.back().end}`) -- note
        // `tracked` there is oldest-first, so front=earliest start,
        // back=latest end.
        let mut sorted = tracked.clone();
        sorted.sort_by_key(|t| t.interval.range.start);
        let range = Range::new(
            sorted.first().map(|t| t.interval.range.start).unwrap_or(0),
            sorted.last().map(|t| t.interval.range.end).unwrap_or(0),
        );
        (range, sorted)
    } else {
        let range = range_arg.unwrap_or_else(|| crate::datetime::expand_range_hint("day").unwrap());
        let tracked = database
            .tracked_in_range(&range)?
            .into_iter()
            .filter(|t| tags.iter().all(|tag| t.interval.has_tag(tag)))
            .collect::<Vec<_>>();
        (range, tracked)
    };

    if tracked.is_empty() {
        let mut msg = String::from("No filtered data found");
        if range.is_started() {
            msg.push_str(&format!(" in the range {}", local_iso_extended(range.start)));
            if range.is_ended() {
                msg.push_str(&format!(" - {}", local_iso_extended(range.end)));
            }
        }
        if !tags.is_empty() {
            msg.push_str(&format!(" tagged with {}", join_quoted_if_needed(&tags, ", ")));
        }
        msg.push_str(".\n");
        return Ok(msg);
    }

    Ok(render_table(&range, &tracked))
}

/// One rendered summary row, with every cell already formatted as text.
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

/// The structured result of a summary: its rows plus the grand total in
/// seconds. Shared by the text renderer and `timew-tui`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub rows: Vec<Row>,
    pub grand_total: i64,
}

fn render_table(range: &Range, tracked: &[Tracked]) -> String {
    let summary = build(range, tracked);
    format_table(&summary.rows, summary.grand_total)
}

/// Build the summary rows for `tracked` (as returned by
/// `Database::tracked_in_range(range)`) over `range`. `tracked` must be
/// non-empty when `range` is unbounded on either side.
pub fn build(range: &Range, tracked: &[Tracked]) -> Summary {
    if tracked.is_empty() && !(range.is_started() && range.is_ended()) {
        return Summary { rows: Vec::new(), grand_total: 0 };
    }
    let now = jiff::Timestamp::now().as_second();

    let days_start = if range.is_started() { range.start } else { tracked.first().unwrap().interval.range.start };
    let days_end_raw = if range.is_ended() {
        range.end
    } else {
        tracked.last().unwrap().interval.range.end
    };
    let days_end = if days_end_raw == 0 { now } else { days_end_raw };

    let mut rows: Vec<Row> = Vec::new();
    let mut grand_total: i64 = 0;

    let mut day = civil_date_of(days_start);
    // Iterate while `day`'s start-of-day epoch is < days_end (mirrors the
    // C++ `for (Datetime day = days_start.startOfDay(); day < days_end; ++day)`).
    loop {
        let day_start_epoch = epoch_of_civil_midnight(day);
        if day_start_epoch >= days_end {
            break;
        }

        let day_range = Range::new(day_start_epoch, epoch_of_civil_midnight(day.tomorrow().unwrap()));

        let mut daily_total: i64 = 0;
        let rows_before_this_day = rows.len();

        for (source, t) in tracked.iter().enumerate().filter(|(_, t)| day_range.intersects(&t.interval.range)) {
            // Skip a still-open interval on a day that hasn't happened yet
            // relative to "now" (mirrors `if (track.is_open() && day > now) continue;`).
            if t.interval.is_open() && day_start_epoch > now {
                continue;
            }

            let mut today_start = t.interval.range.start.max(day_range.start);
            let mut today_end = if t.interval.is_ended() {
                t.interval.range.end.min(day_range.end)
            } else {
                day_range.end
            };

            if t.interval.is_open() && t.interval.range.start > now {
                today_end = t.interval.range.start;
            } else if t.interval.is_open() && day_start_epoch <= now && today_end > now {
                today_end = now;
            }
            if today_start > today_end {
                today_start = today_end;
            }

            let total = today_end - today_start;
            let is_first_row_this_day = rows.len() == rows_before_this_day;

            rows.push(Row {
                week: is_first_row_this_day.then(|| format!("W{}", iso_week_of(day))),
                date: is_first_row_this_day.then(|| civil_date_string(day)),
                day_name: is_first_row_this_day.then(|| short_weekday(day)),
                id: format!("@{}", t.id),
                tags: t.interval.tags.join(", "),
                annotation: t.interval.annotation.clone().unwrap_or_default(),
                start: time_of_day_string(today_start),
                end: if t.interval.is_open() { "-".to_string() } else { time_of_day_string(today_end) },
                time: format_hours(total),
                total: String::new(),
                source,
            });

            daily_total += total;
        }

        // The Total column is only filled in on the last row added for
        // this day (matches the C++ source only setting it once, after
        // the inner loop, using whatever `row` index it last wrote).
        if rows.len() > rows_before_this_day {
            rows.last_mut().unwrap().total = format_hours(daily_total);
        }

        grand_total += daily_total;
        day = day.tomorrow().unwrap();
    }

    Summary { rows, grand_total }
}

fn format_table(rows: &[Row], grand_total: i64) -> String {
    const HEADERS: [&str; 10] =
        ["Wk", "Date", "Day", "ID", "Tags", "Annotation", "Start", "End", "Time", "Total"];

    let grand_total_str = format_hours(grand_total);

    let width_of = |get: fn(&Row) -> &str, header: &str, extra: &str| -> usize {
        rows.iter()
            .map(|r| get(r).len())
            .chain(std::iter::once(extra.len()))
            .max()
            .unwrap_or(0)
            .max(header.len())
    };

    let w_wk = width_of(|r| r.week.as_deref().unwrap_or(""), HEADERS[0], "");
    let w_date = width_of(|r| r.date.as_deref().unwrap_or(""), HEADERS[1], "");
    let w_day = width_of(|r| r.day_name.as_deref().unwrap_or(""), HEADERS[2], "");
    let w_id = width_of(|r| r.id.as_str(), HEADERS[3], "");
    let w_tags = width_of(|r| r.tags.as_str(), HEADERS[4], "");
    let w_ann = width_of(|r| r.annotation.as_str(), HEADERS[5], "");
    let w_start = width_of(|r| r.start.as_str(), HEADERS[6], "");
    let w_end = width_of(|r| r.end.as_str(), HEADERS[7], "");
    let w_time = width_of(|r| r.time.as_str(), HEADERS[8], "");
    let w_total = width_of(|r| r.total.as_str(), HEADERS[9], &grand_total_str);

    let mut out = String::from("\n");

    out.push_str(&format!(
        "{:<w_wk$} {:<w_date$} {:<w_day$} {:<w_id$} {:<w_tags$} {:<w_ann$} {:>w_start$} {:>w_end$} {:>w_time$} {:>w_total$}\n",
        HEADERS[0], HEADERS[1], HEADERS[2], HEADERS[3], HEADERS[4], HEADERS[5], HEADERS[6], HEADERS[7], HEADERS[8], HEADERS[9],
    ));
    out.push_str(&format!(
        "{} {} {} {} {} {} {} {} {} {}\n",
        "-".repeat(w_wk),
        "-".repeat(w_date),
        "-".repeat(w_day),
        "-".repeat(w_id),
        "-".repeat(w_tags),
        "-".repeat(w_ann),
        "-".repeat(w_start),
        "-".repeat(w_end),
        "-".repeat(w_time),
        "-".repeat(w_total),
    ));

    for r in rows {
        out.push_str(&format!(
            "{:<w_wk$} {:<w_date$} {:<w_day$} {:<w_id$} {:<w_tags$} {:<w_ann$} {:>w_start$} {:>w_end$} {:>w_time$} {:>w_total$}\n",
            r.week.as_deref().unwrap_or(""),
            r.date.as_deref().unwrap_or(""),
            r.day_name.as_deref().unwrap_or(""),
            r.id,
            r.tags,
            r.annotation,
            r.start,
            r.end,
            r.time,
            r.total,
        ));
    }

    out.push('\n');
    out.push_str(&format!(
        "{:<w_wk$} {:<w_date$} {:<w_day$} {:<w_id$} {:<w_tags$} {:<w_ann$} {:>w_start$} {:>w_end$} {:>w_time$} {:>w_total$}\n",
        "", "", "", "", "", "", "", "", "", grand_total_str,
    ));

    // Trim trailing-space-only artifacts from blank cells on right-aligned
    // empty strings by stripping trailing whitespace per line, matching
    // real timew's table renderer which does not pad trailing blank cells
    // (confirmed for the tags command; applied consistently here too).
    trim_trailing_spaces(&out)
}

fn trim_trailing_spaces(s: &str) -> String {
    s.lines().map(|l| l.trim_end()).collect::<Vec<_>>().join("\n") + "\n"
}

fn civil_date_of(epoch: Epoch) -> Date {
    jiff::Zoned::new(jiff::Timestamp::from_second(epoch).unwrap(), jiff::tz::TimeZone::system()).date()
}

fn epoch_of_civil_midnight(date: Date) -> Epoch {
    date.to_zoned(jiff::tz::TimeZone::system()).unwrap().timestamp().as_second()
}

fn iso_week_of(date: Date) -> i8 {
    date.iso_week_date().week()
}

fn civil_date_string(date: Date) -> String {
    format!("{:04}-{:02}-{:02}", date.year(), date.month(), date.day())
}

fn short_weekday(date: Date) -> String {
    use jiff::civil::Weekday::*;
    match date.weekday() {
        Monday => "Mon",
        Tuesday => "Tue",
        Wednesday => "Wed",
        Thursday => "Thu",
        Friday => "Fri",
        Saturday => "Sat",
        Sunday => "Sun",
    }
    .to_string()
}

fn time_of_day_string(epoch: Epoch) -> String {
    let zoned = Zoned::new(jiff::Timestamp::from_second(epoch).unwrap(), jiff::tz::TimeZone::system());
    format!("{}:{:02}:{:02}", zoned.hour(), zoned.minute(), zoned.second())
}

/// Format a duration in seconds as `H:MM:SS` (hours unbounded/unpadded,
/// minutes and seconds zero-padded to 2 digits), matching
/// `Duration::formatHours()`'s output confirmed against real captured
/// samples (e.g. "1:30:00", "24:00:00", "39:00:00").
pub fn format_hours(total_seconds: i64) -> String {
    let h = total_seconds / 3600;
    let m = (total_seconds % 3600) / 60;
    let s = total_seconds % 60;
    format!("{h}:{m:02}:{s:02}")
}

pub fn local_iso_extended(epoch: Epoch) -> String {
    let zoned = Zoned::new(jiff::Timestamp::from_second(epoch).unwrap(), jiff::tz::TimeZone::system());
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        zoned.year(),
        zoned.month(),
        zoned.day(),
        zoned.hour(),
        zoned.minute(),
        zoned.second()
    )
}

fn join_quoted_if_needed(tags: &[String], glue: &str) -> String {
    tags.iter().map(|t| crate::interval::quote_if_needed(t)).collect::<Vec<_>>().join(glue)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli;
    use crate::interval::{parse_iso, Interval};
    use std::fs;
    use std::path::PathBuf;

    fn tmp_db(name: &str) -> (PathBuf, Database) {
        let dir = std::env::temp_dir().join(format!("timew-rs-summary-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let db = Database::open(&dir).unwrap();
        (dir, db)
    }

    fn mk_closed(start: &str, end: &str, tags: &[&str]) -> Interval {
        let mut iv = Interval::new(crate::range::Range::new(parse_iso(start).unwrap(), parse_iso(end).unwrap()));
        for t in tags {
            iv.add_tag(*t);
        }
        iv
    }

    fn mk_open(start: &str, tags: &[&str]) -> Interval {
        let mut iv = Interval::new(crate::range::Range::new(parse_iso(start).unwrap(), 0));
        for t in tags {
            iv.add_tag(*t);
        }
        iv
    }

    fn parsed(words: &[&str]) -> ParsedArgs {
        let strs: Vec<String> = words.iter().map(|s| s.to_string()).collect();
        cli::parse(&strs).unwrap()
    }

    #[test]
    fn format_hours_matches_real_samples() {
        assert_eq!(format_hours(90 * 60), "1:30:00");
        assert_eq!(format_hours(24 * 3600), "24:00:00");
        assert_eq!(format_hours(39 * 3600), "39:00:00");
        assert_eq!(format_hours(0), "0:00:00");
    }

    #[test]
    fn empty_result_message_with_range_only() {
        let (dir, mut db) = tmp_db("empty-range");
        let out = run(
            &mut db,
            parsed(&["summary", "2026-10-01T00:00:00Z", "-", "2026-10-02T00:00:00Z"]),
        )
        .unwrap();
        assert!(out.starts_with("No filtered data found in the range"));
        assert!(out.ends_with(".\n"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_result_message_with_tags() {
        let (dir, mut db) = tmp_db("empty-tags");
        let out = run(&mut db, parsed(&["summary", "sometag"])).unwrap();
        assert!(out.contains("tagged with sometag"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn single_day_two_intervals_matches_real_captured_sample() {
        let (dir, mut db) = tmp_db("captured-sample");
        db.add_interval(&mk_closed("20261001T070000Z", "20261001T083000Z", &["alpha"])).unwrap();
        db.add_interval(&mk_closed("20261001T110000Z", "20261001T121530Z", &["beta"])).unwrap();
        db.commit().unwrap();

        let out = run(
            &mut db,
            parsed(&["summary", "2026-10-01T00:00:00Z", "-", "2026-10-03T00:00:00Z"]),
        )
        .unwrap();

        // Captured verbatim from the real binary (CET/UTC+2 in October):
        // local times 9:00-10:30 and 13:00-14:15:30. Built with explicit
        // \n (not a `\`-continued literal) so leading spaces on the
        // continuation row are preserved exactly.
        let expected = concat!(
            "\n",
            "Wk  Date       Day ID Tags  Annotation    Start      End    Time   Total\n",
            "--- ---------- --- -- ----- ---------- -------- -------- ------- -------\n",
            "W40 2026-10-01 Thu @2 alpha             9:00:00 10:30:00 1:30:00\n",
            "                   @1 beta             13:00:00 14:15:30 1:15:30 2:45:30\n",
            "\n",
            "                                                                 2:45:30\n",
        );
        assert_eq!(out, expected);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn multi_tag_and_annotation_matches_real_captured_sample() {
        // Matches the real session this was captured from: "alpha" was
        // tracked first (older, so it becomes id @2), "beta, gamma" second
        // (newer, id @1), and the annotation was applied to @1 --
        // i.e. the "beta, gamma" interval, not "alpha".
        let (dir, mut db) = tmp_db("multitag-annotation");
        db.add_interval(&mk_closed("20261001T070000Z", "20261001T083000Z", &["alpha"])).unwrap();
        let mut b = mk_closed("20261001T110000Z", "20261001T121530Z", &["beta", "gamma"]);
        b.annotation = Some("a note".to_string());
        db.add_interval(&b).unwrap();
        db.commit().unwrap();

        let out = run(
            &mut db,
            parsed(&["summary", "2026-10-01T00:00:00Z", "-", "2026-10-02T00:00:00Z"]),
        )
        .unwrap();

        let expected = concat!(
            "\n",
            "Wk  Date       Day ID Tags        Annotation    Start      End    Time   Total\n",
            "--- ---------- --- -- ----------- ---------- -------- -------- ------- -------\n",
            "W40 2026-10-01 Thu @2 alpha                   9:00:00 10:30:00 1:30:00\n",
            "                   @1 beta, gamma a note     13:00:00 14:15:30 1:15:30 2:45:30\n",
            "\n",
            "                                                                       2:45:30\n",
        );
        assert_eq!(out, expected);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn week_number_is_iso_week() {
        let (dir, mut db) = tmp_db("iso-week");
        db.add_interval(&mk_closed("20261231T080000Z", "20261231T090000Z", &["x"])).unwrap();
        db.commit().unwrap();

        let out = run(
            &mut db,
            parsed(&["summary", "2026-12-31T00:00:00Z", "-", "2027-01-01T00:00:00Z"]),
        )
        .unwrap();
        // Confirmed empirically: Dec 31 2026 is ISO week 53.
        assert!(out.contains("W53"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn mutually_exclusive_id_and_tags_is_an_error() {
        let (dir, mut db) = tmp_db("id-and-tags");
        db.add_interval(&mk_closed("20261001T070000Z", "20261001T080000Z", &["a"])).unwrap();
        db.commit().unwrap();

        let err = run(&mut db, parsed(&["summary", "@1", "a"])).unwrap_err();
        assert!(err.contains("cannot filter"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn nonexistent_id_errors_naming_it() {
        let (dir, mut db) = tmp_db("missing-id");
        db.add_interval(&mk_closed("20261001T070000Z", "20261001T080000Z", &["a"])).unwrap();
        db.commit().unwrap();

        let err = run(&mut db, parsed(&["summary", "@99"])).unwrap_err();
        assert!(err.contains("@99"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_interval_shows_dash_for_end() {
        let (dir, mut db) = tmp_db("open-dash");
        // Start it "now" so the row is included and End renders as "-".
        let now = jiff::Timestamp::now().as_second();
        let start_epoch = now - 3600; // an hour ago
        let iso = crate::interval::format_iso(start_epoch);
        db.add_interval(&mk_open(&iso, &["working"])).unwrap();
        db.commit().unwrap();

        let out = run(&mut db, parsed(&["summary", ":day"])).unwrap();
        assert!(out.contains(" - "), "expected a dash End column, got: {out}");

        let _ = fs::remove_dir_all(&dir);
    }
}
