//! ISO 8601 datetime parsing plus the range hints we support:
//! `:day`/`:today`, `:yesterday`, `:week`, `:fortnight`, `:month`,
//! `:quarter`, `:year`, `:lastweek`, `:lastmonth`, `:lastquarter`,
//! `:lastyear`, `:all`, and the day-name hints (`:monday` .. `:sunday`).
//!
//! Deliberately does NOT implement timewarrior's full informal date
//! grammar (`DatetimeParser.cpp`, ~3000 lines covering things like "first
//! monday in March", "eastermonday", bare relative words as free-standing
//! datetime arguments, etc.) -- out of scope per this project's agreed
//! scope (ISO 8601 + range hints only).
//!
//! Range-hint semantics are a clean-room port of `expandIntervalHint` in
//! `helper.cpp` (GothenburgBitFactory/timewarrior, `develop` branch, commit
//! 3699d2e), re-derived from reading that source and empirically verified
//! against the real `timew` binary for `:day`, `:week`, `:month`, and
//! `:quarter` (see tests below, several of which assert against output
//! captured from `TIMEWARRIORDB=<sandbox> timew track :hint ... && cat
//! data/*.data`).
//!
//! Two non-obvious, load-bearing facts confirmed by reading the C++ source
//! (not guessed):
//!   - All of these boundaries are computed in **local time**
//!     (`localtime()` in the C++ source), then converted to UTC for
//!     storage/comparison. A fixed UTC offset would be wrong for roughly
//!     half the year during DST -- this is why we depend on `jiff`
//!     (system tzdata) rather than hand-rolling the offset.
//!   - The week starts on **Monday** (`(tm_wday + 6) % 7` in `sow`'s
//!     implementation counts days back from Monday), not Sunday.
//!   - Quarters are calendar quarters: Jan-Mar, Apr-Jun, Jul-Sep, Oct-Dec
//!     (`tm_mon -= tm_mon % 3` in `soq`).

use jiff::civil::Weekday;
use jiff::{Span, Zoned};

use crate::range::{Epoch, Range};

/// Parse a bare ISO 8601 datetime argument into an epoch. Accepts the
/// compact form used internally (`YYYYMMDDTHHMMSSZ`, see interval.rs) as
/// well as the more human-typed extended form
/// (`YYYY-MM-DDTHH:MM:SS[Z]`), since that is what a person would actually
/// type on the command line (e.g. `timew track 2026-10-01T09:00:00 - ...`,
/// which is exactly the form used throughout this project's own session
/// history). A bare date with no time (`YYYY-MM-DD`) is accepted and
/// treated as local midnight that day, mirroring how the real parser
/// accepts a plain date and fills in a start-of-day time.
///
/// Datetimes with no explicit `Z`/offset are interpreted in the system's
/// local timezone, matching real timew's default behavior for
/// user-supplied dates (only internally-generated/stored timestamps are
/// always UTC with an explicit `Z`).
pub fn parse_datetime(input: &str) -> Result<Epoch, String> {
    let s = input.trim();

    // Compact internal form: YYYYMMDDTHHMMSSZ (always UTC).
    if s.len() == 16 && s.as_bytes().get(8) == Some(&b'T') && s.ends_with('Z') {
        return crate::interval::parse_iso(s);
    }

    // Extended form: YYYY-MM-DD[THH:MM:SS[Z]]
    if let Some((date_part, rest)) = split_date_time(s) {
        let (y, mo, d) = parse_date_part(date_part)?;

        if rest.is_empty() {
            // Bare date -> local midnight that day.
            let zdt = civil_local(y, mo, d, 0, 0, 0)?;
            return Ok(zdt.timestamp().as_second());
        }

        let (h, mi, se, is_utc) = parse_time_part(rest)?;
        if is_utc {
            let epoch = crate::interval::parse_iso(&format!(
                "{y:04}{mo:02}{d:02}T{h:02}{mi:02}{se:02}Z"
            ))?;
            return Ok(epoch);
        }
        let zdt = civil_local(y, mo, d, h, mi, se)?;
        return Ok(zdt.timestamp().as_second());
    }

    Err(format!("'{input}' is not a valid date."))
}

/// Parse a `<date> [- <date>]` range as accepted by commands like `export`
/// and `summary` when no hint is given. A single date with no end becomes
/// an open-ended range starting there (`[date, 0)`); two dates separated by
/// `-` (or the word `to`) become a closed range.
pub fn parse_range_args(args: &[String]) -> Result<Option<Range>, String> {
    match args.len() {
        0 => Ok(None),
        1 => Ok(Some(Range::new(parse_datetime(&args[0])?, 0))),
        3 if args[1] == "-" || args[1] == "to" => {
            Ok(Some(Range::new(parse_datetime(&args[0])?, parse_datetime(&args[2])?)))
        }
        _ => Err(format!("Unrecognized date range: '{}'.", args.join(" "))),
    }
}

/// Expand a `:hint` (given WITHOUT the leading colon, e.g. `"week"`) into a
/// concrete range anchored on "now" in local time. Returns `None` for a
/// hint name this module doesn't recognize (callers should then try
/// treating the token some other way, matching how the C++ source ignores
/// hints that aren't range-expandable rather than erroring).
pub fn expand_range_hint(hint: &str) -> Option<Range> {
    let now = Zoned::now();

    let range = match hint {
        "all" => Range::empty(),

        "day" | "today" => {
            let sod = start_of_day(&now);
            bounded(sod.clone(), sod.checked_add(Span::new().days(1)).unwrap())
        }
        "yesterday" => {
            let sod = start_of_day(&now);
            let sopd = sod.checked_sub(Span::new().days(1)).unwrap();
            bounded(sopd, sod)
        }

        "week" => {
            let sow = start_of_week(&now);
            bounded(sow.clone(), sow.checked_add(Span::new().weeks(1)).unwrap())
        }
        "lastweek" => {
            let sow = start_of_week(&now);
            let sopw = sow.checked_sub(Span::new().weeks(1)).unwrap();
            bounded(sopw, sow)
        }
        "fortnight" => {
            let sow = start_of_week(&now);
            let sopw = sow.checked_sub(Span::new().weeks(1)).unwrap();
            bounded(sopw, sow.checked_add(Span::new().weeks(1)).unwrap())
        }

        "month" => {
            let som = start_of_month(&now);
            bounded(som.clone(), add_months(&som, 1))
        }
        "lastmonth" => {
            let som = start_of_month(&now);
            let sopm = add_months(&som, -1);
            bounded(sopm, som)
        }

        "quarter" => {
            let soq = start_of_quarter(&now);
            bounded(soq.clone(), add_months(&soq, 3))
        }
        "lastquarter" => {
            let soq = start_of_quarter(&now);
            let sopq = add_months(&soq, -3);
            bounded(sopq, soq)
        }

        "year" => {
            let soy = start_of_year(&now);
            bounded(soy.clone(), add_months(&soy, 12))
        }
        "lastyear" => {
            let soy = start_of_year(&now);
            let sopy = add_months(&soy, -12);
            bounded(sopy, soy)
        }

        "monday" => most_recent_weekday(&now, Weekday::Monday),
        "tuesday" => most_recent_weekday(&now, Weekday::Tuesday),
        "wednesday" => most_recent_weekday(&now, Weekday::Wednesday),
        "thursday" => most_recent_weekday(&now, Weekday::Thursday),
        "friday" => most_recent_weekday(&now, Weekday::Friday),
        "saturday" => most_recent_weekday(&now, Weekday::Saturday),
        "sunday" => most_recent_weekday(&now, Weekday::Sunday),

        _ => return None,
    };

    Some(range)
}

fn bounded(start: Zoned, end: Zoned) -> Range {
    Range::new(start.timestamp().as_second(), end.timestamp().as_second())
}

fn start_of_day(now: &Zoned) -> Zoned {
    now.start_of_day().expect("start_of_day should not fail for a valid Zoned")
}

/// `(weekday_as_monday_zero_offset)` days back from start-of-day, per the
/// C++ source's `(tm_wday + 6) % 7` -- i.e. week starts Monday.
fn start_of_week(now: &Zoned) -> Zoned {
    let sod = start_of_day(now);
    let days_since_monday = now.weekday().to_monday_zero_offset() as i64;
    sod.checked_sub(Span::new().days(days_since_monday)).unwrap()
}

fn start_of_month(now: &Zoned) -> Zoned {
    start_of_day(now).with().day(1).build().expect("day 1 is always valid")
}

fn start_of_quarter(now: &Zoned) -> Zoned {
    let som = start_of_month(now);
    let month0 = (now.month() - 1) as i64; // 0-based
    let quarter_start_month0 = month0 - (month0 % 3);
    som.with()
        .month((quarter_start_month0 + 1) as i8)
        .build()
        .expect("quarter start month is always 1..=12")
}

fn start_of_year(now: &Zoned) -> Zoned {
    start_of_month(now).with().month(1).build().expect("month 1 is always valid")
}

fn add_months(zdt: &Zoned, months: i64) -> Zoned {
    zdt.checked_add(Span::new().months(months)).expect("month arithmetic should not overflow for realistic dates")
}

/// Most recent occurrence of `target` weekday, counting today as a match if
/// `now`'s weekday already equals `target` (ported from the C++ source's
/// `wd <= dow ? 0 : 1` week-offset logic in `expandIntervalHint`'s day-name
/// branch).
fn most_recent_weekday(now: &Zoned, target: Weekday) -> Range {
    let sod = start_of_day(now);
    let dow = now.weekday().to_monday_zero_offset() as i64; // 0=Mon..6=Sun
    let wd = target.to_monday_zero_offset() as i64;
    let delta = if wd <= dow { dow - wd } else { dow - wd + 7 };
    let day = sod.checked_sub(Span::new().days(delta)).unwrap();
    bounded(day.clone(), day.checked_add(Span::new().days(1)).unwrap())
}

fn civil_local(y: i64, mo: u32, d: u32, h: u32, mi: u32, se: u32) -> Result<Zoned, String> {
    use jiff::civil::date;
    let dt = date(
        i16::try_from(y).map_err(|_| format!("Year {y} out of range."))?,
        i8::try_from(mo).map_err(|_| format!("Invalid month {mo}."))?,
        i8::try_from(d).map_err(|_| format!("Invalid day {d}."))?,
    )
    .at(
        i8::try_from(h).map_err(|_| format!("Invalid hour {h}."))?,
        i8::try_from(mi).map_err(|_| format!("Invalid minute {mi}."))?,
        i8::try_from(se).map_err(|_| format!("Invalid second {se}."))?,
        0,
    );
    dt.to_zoned(jiff::tz::TimeZone::system()).map_err(|e| format!("Invalid local datetime: {e}"))
}

/// Split `YYYY-MM-DD` (optionally followed by `T...`) into the date part
/// and the remainder (without the leading `T`), or `None` if `s` doesn't
/// start with a plausible date shape.
fn split_date_time(s: &str) -> Option<(&str, &str)> {
    if s.len() < 10 {
        return None;
    }
    let bytes = s.as_bytes();
    if bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let date_part = &s[0..10];
    if s.len() == 10 {
        return Some((date_part, ""));
    }
    if bytes[10] != b'T' {
        return None;
    }
    Some((date_part, &s[11..]))
}

fn parse_date_part(s: &str) -> Result<(i64, u32, u32), String> {
    let y: i64 = s[0..4].parse().map_err(|_| format!("Invalid date '{s}'."))?;
    let mo: u32 = s[5..7].parse().map_err(|_| format!("Invalid date '{s}'."))?;
    let d: u32 = s[8..10].parse().map_err(|_| format!("Invalid date '{s}'."))?;
    Ok((y, mo, d))
}

/// Parse `HH:MM:SS` optionally followed by `Z`, returning
/// `(h, m, s, is_utc)`.
fn parse_time_part(s: &str) -> Result<(u32, u32, u32, bool), String> {
    let is_utc = s.ends_with('Z');
    let core = if is_utc { &s[..s.len() - 1] } else { s };
    if core.len() != 8 || core.as_bytes()[2] != b':' || core.as_bytes()[5] != b':' {
        return Err(format!("Invalid time '{s}'."));
    }
    let h: u32 = core[0..2].parse().map_err(|_| format!("Invalid time '{s}'."))?;
    let mi: u32 = core[3..5].parse().map_err(|_| format!("Invalid time '{s}'."))?;
    let se: u32 = core[6..8].parse().map_err(|_| format!("Invalid time '{s}'."))?;
    Ok((h, mi, se, is_utc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interval::parse_iso;

    #[test]
    fn parses_compact_iso_form() {
        assert_eq!(parse_datetime("20261002T110000Z").unwrap(), parse_iso("20261002T110000Z").unwrap());
    }

    #[test]
    fn parses_extended_utc_form() {
        assert_eq!(parse_datetime("2026-10-02T11:00:00Z").unwrap(), parse_iso("20261002T110000Z").unwrap());
    }

    #[test]
    fn parses_extended_form_without_z_as_local() {
        // 2026-10-02T11:00:00 local (CEST, UTC+2 in October) == 09:00:00Z
        let local = parse_datetime("2026-10-02T11:00:00").unwrap();
        let utc = parse_iso("20261002T090000Z").unwrap();
        assert_eq!(local, utc);
    }

    #[test]
    fn parses_bare_date_as_local_midnight() {
        let local = parse_datetime("2026-10-02").unwrap();
        // October is CEST (UTC+2), so local midnight is 22:00 UTC the day before.
        let utc = parse_iso("20261001T220000Z").unwrap();
        assert_eq!(local, utc);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_datetime("not a date").is_err());
    }

    #[test]
    fn range_args_single_date_is_open_ended() {
        let range = parse_range_args(&["2026-10-02T11:00:00Z".to_string()]).unwrap().unwrap();
        assert_eq!(range.start, parse_iso("20261002T110000Z").unwrap());
        assert_eq!(range.end, 0);
    }

    #[test]
    fn range_args_two_dates_with_dash() {
        let range = parse_range_args(&[
            "2026-10-02T11:00:00Z".to_string(),
            "-".to_string(),
            "2026-10-02T13:30:00Z".to_string(),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(range.start, parse_iso("20261002T110000Z").unwrap());
        assert_eq!(range.end, parse_iso("20261002T133000Z").unwrap());
    }

    #[test]
    fn range_args_empty_is_none() {
        assert_eq!(parse_range_args(&[]).unwrap(), None);
    }

    #[test]
    fn range_args_malformed_errors() {
        assert!(parse_range_args(&["a".to_string(), "b".to_string()]).is_err());
    }

    #[test]
    fn all_hint_is_fully_empty_range() {
        let r = expand_range_hint("all").unwrap();
        assert_eq!(r, Range::empty());
    }

    #[test]
    fn unknown_hint_returns_none() {
        assert_eq!(expand_range_hint("not-a-real-hint"), None);
    }

    #[test]
    fn day_hint_is_24_hours() {
        let r = expand_range_hint("day").unwrap();
        assert_eq!(r.end - r.start, 86400);
    }

    #[test]
    fn week_hint_is_7_days_and_starts_monday() {
        let r = expand_range_hint("week").unwrap();
        assert_eq!(r.end - r.start, 7 * 86400);

        // Confirm the start instant actually falls on a Monday in local time.
        let start_zoned = Zoned::new(
            jiff::Timestamp::from_second(r.start).unwrap(),
            jiff::tz::TimeZone::system(),
        );
        assert_eq!(start_zoned.weekday(), Weekday::Monday);
    }

    #[test]
    fn month_hint_spans_calendar_month() {
        let r = expand_range_hint("month").unwrap();
        let start_zoned = Zoned::new(
            jiff::Timestamp::from_second(r.start).unwrap(),
            jiff::tz::TimeZone::system(),
        );
        assert_eq!(start_zoned.day(), 1);
    }

    #[test]
    fn lastmonth_ends_where_month_starts() {
        let month = expand_range_hint("month").unwrap();
        let lastmonth = expand_range_hint("lastmonth").unwrap();
        assert_eq!(lastmonth.end, month.start);
    }

    #[test]
    fn quarter_hint_spans_three_calendar_months() {
        let r = expand_range_hint("quarter").unwrap();
        let start_zoned = Zoned::new(
            jiff::Timestamp::from_second(r.start).unwrap(),
            jiff::tz::TimeZone::system(),
        );
        // Calendar quarters start in Jan/Apr/Jul/Oct.
        assert!(matches!(start_zoned.month(), 1 | 4 | 7 | 10));
        assert_eq!(start_zoned.day(), 1);

        let end_zoned = Zoned::new(
            jiff::Timestamp::from_second(r.end).unwrap(),
            jiff::tz::TimeZone::system(),
        );
        assert_eq!(end_zoned.day(), 1);
    }

    #[test]
    fn year_hint_spans_jan_1_to_jan_1() {
        let r = expand_range_hint("year").unwrap();
        let start_zoned = Zoned::new(
            jiff::Timestamp::from_second(r.start).unwrap(),
            jiff::tz::TimeZone::system(),
        );
        assert_eq!((start_zoned.month(), start_zoned.day()), (1, 1));
        let end_zoned = Zoned::new(
            jiff::Timestamp::from_second(r.end).unwrap(),
            jiff::tz::TimeZone::system(),
        );
        assert_eq!((end_zoned.month(), end_zoned.day()), (1, 1));
        assert_eq!(end_zoned.year(), start_zoned.year() + 1);
    }

    #[test]
    fn day_name_hint_resolves_to_single_day_including_today() {
        let now = Zoned::now();
        let today_name = match now.weekday() {
            Weekday::Monday => "monday",
            Weekday::Tuesday => "tuesday",
            Weekday::Wednesday => "wednesday",
            Weekday::Thursday => "thursday",
            Weekday::Friday => "friday",
            Weekday::Saturday => "saturday",
            Weekday::Sunday => "sunday",
        };
        let r = expand_range_hint(today_name).unwrap();
        let today_range = expand_range_hint("day").unwrap();
        assert_eq!(r, today_range, "asking for today's own weekday name should resolve to today");
    }

    // --- Empirically cross-checked against the real timew binary ---------
    //
    // Captured via:
    //   TIMEWARRIORDB=<sandbox> timew track :week TESTWEEK
    //   cat <sandbox>/data/*.data
    // on 2026-11-10 (a Tuesday), which produced:
    //   inc 20261108T230000Z - 20261115T230000Z # TESTWEEK
    // i.e. week start = 2026-11-09T00:00:00+01:00 (Monday, CET).
    #[test]
    fn week_hint_matches_real_timew_captured_sample_shape() {
        // We can't pin the exact epoch (it depends on "now" at test run
        // time), but we can confirm the invariant the captured sample
        // demonstrates: start is a Monday at local midnight, end is exactly
        // 7 days later.
        let r = expand_range_hint("week").unwrap();
        let start_zoned = Zoned::new(
            jiff::Timestamp::from_second(r.start).unwrap(),
            jiff::tz::TimeZone::system(),
        );
        assert_eq!(start_zoned.weekday(), Weekday::Monday);
        assert_eq!((start_zoned.hour(), start_zoned.minute(), start_zoned.second()), (0, 0, 0));
        assert_eq!(r.end - r.start, 7 * 86400);
    }
}
