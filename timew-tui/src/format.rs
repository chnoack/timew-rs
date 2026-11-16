//! Formatting times, dates, durations and ranges for display.

use timew_core::range::Range;

/// Decimal hours with two places from (possibly fractional) seconds.
pub(crate) fn decimal_hours_f(seconds: f64) -> String {
    format!("{:.2}", seconds / 3600.0)
}

/// The TUI shows no seconds: turn `H:MM:SS` into `H:MM` (seconds are cut
/// off, not rounded). Anything else, such as the `-` end of a running
/// entry or an empty cell, is returned unchanged.
pub(crate) fn without_seconds(s: &str) -> String {
    let b = s.as_bytes();
    let n = b.len();
    let is_hms = n >= 7
        && b[n - 3] == b':'
        && b[n - 6] == b':'
        && [n - 1, n - 2, n - 4, n - 5].iter().all(|&i| b[i].is_ascii_digit());
    if is_hms { s[..n - 3].to_string() } else { s.to_string() }
}

/// Seconds of an `H:MM:SS` duration string (None for anything else).
pub(crate) fn parse_hms(s: &str) -> Option<i64> {
    let mut parts = s.split(':');
    let (h, m, sec) = (parts.next()?.parse::<i64>().ok()?, parts.next()?.parse::<i64>().ok()?, parts.next()?.parse::<i64>().ok()?);
    parts.next().is_none().then_some(h * 3600 + m * 60 + sec)
}

/// A duration as decimal hours with two places, e.g. 7:30 -> `7.50`.
pub(crate) fn decimal_hours(seconds: i64) -> String {
    format!("{:.2}", seconds as f64 / 3600.0)
}

/// A duration in seconds as `H:MM` (hours unbounded, seconds cut off).
pub(crate) fn format_hm(seconds: i64) -> String {
    format!("{}:{:02}", seconds / 3600, (seconds % 3600) / 60)
}

/// "2026-11-12 Thu 08:00" in local time.
pub(crate) fn local_datetime(epoch: i64) -> String {
    let z = jiff::Timestamp::from_second(epoch).unwrap().to_zoned(jiff::tz::TimeZone::system());
    z.strftime("%Y-%m-%d %a %H:%M").to_string()
}

/// Local calendar date of an epoch, as `YYYY-MM-DD`.
pub(crate) fn local_date(epoch: i64) -> String {
    jiff::Timestamp::from_second(epoch).unwrap().to_zoned(jiff::tz::TimeZone::system()).date().to_string()
}

/// Local `YYYY-MM-DD`, plus ` HH:MM` when the epoch is not at midnight.
pub(crate) fn local_moment(epoch: i64) -> String {
    let z = jiff::Timestamp::from_second(epoch).unwrap().to_zoned(jiff::tz::TimeZone::system());
    if (z.hour(), z.minute(), z.second()) == (0, 0, 0) {
        z.date().to_string()
    } else {
        z.strftime("%Y-%m-%d %H:%M").to_string()
    }
}

/// "2026-11-01 – 2026-11-30" (end is exclusive internally, shown inclusive
/// for whole days), "since 2026-11-01" for an open-ended range.
pub(crate) fn range_label(range: &Range) -> String {
    if !range.is_ended() {
        return format!("since {}", local_moment(range.start));
    }
    let start = local_moment(range.start);
    let end_is_midnight = local_moment(range.end).len() == 10;
    let end = if end_is_midnight && start.len() == 10 { local_date(range.end - 1) } else { local_moment(range.end) };
    if start == end { start } else { format!("{start} – {end}") }
}
