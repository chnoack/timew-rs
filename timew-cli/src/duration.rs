//! Durations: parsing (`1h`, `90min`, `1.5 hours`, `PT1H30M`, `3600`) and
//! the output formats used by the reports.

const MINUTE: f64 = 60.0;
const HOUR: f64 = 3600.0;
const DAY: f64 = 86400.0;

/// Unit names, their length in seconds, and whether the name is a duration
/// on its own (`hour` means one hour, `h` does not).
const UNITS: &[(&str, f64, bool)] = &[
    ("annual", 365.0 * DAY, true),
    ("biannual", 730.0 * DAY, true),
    ("bimonthly", 61.0 * DAY, true),
    ("biweekly", 14.0 * DAY, true),
    ("biyearly", 730.0 * DAY, true),
    ("daily", DAY, true),
    ("days", DAY, false),
    ("day", DAY, true),
    ("d", DAY, false),
    ("fortnight", 14.0 * DAY, true),
    ("hours", HOUR, false),
    ("hour", HOUR, true),
    ("hrs", HOUR, false),
    ("hr", HOUR, true),
    ("h", HOUR, false),
    ("minutes", MINUTE, false),
    ("minute", MINUTE, true),
    ("mins", MINUTE, false),
    ("min", MINUTE, true),
    ("monthly", 30.0 * DAY, true),
    ("months", 30.0 * DAY, false),
    ("month", 30.0 * DAY, true),
    ("mnths", 30.0 * DAY, false),
    ("mths", 30.0 * DAY, false),
    ("mth", 30.0 * DAY, true),
    ("mos", 30.0 * DAY, false),
    ("mo", 30.0 * DAY, true),
    ("m", 30.0 * DAY, false),
    ("quarterly", 91.0 * DAY, true),
    ("quarters", 91.0 * DAY, false),
    ("quarter", 91.0 * DAY, true),
    ("qrtrs", 91.0 * DAY, false),
    ("qrtr", 91.0 * DAY, true),
    ("qtrs", 91.0 * DAY, false),
    ("qtr", 91.0 * DAY, true),
    ("q", 91.0 * DAY, false),
    ("semiannual", 183.0 * DAY, true),
    ("sennight", 14.0 * DAY, false),
    ("seconds", 1.0, false),
    ("second", 1.0, true),
    ("secs", 1.0, false),
    ("sec", 1.0, true),
    ("s", 1.0, false),
    ("weekdays", DAY, true),
    ("weekly", 7.0 * DAY, true),
    ("weeks", 7.0 * DAY, false),
    ("week", 7.0 * DAY, true),
    ("wks", 7.0 * DAY, false),
    ("wk", 7.0 * DAY, true),
    ("w", 7.0 * DAY, false),
    ("yearly", 365.0 * DAY, true),
    ("years", 365.0 * DAY, false),
    ("year", 365.0 * DAY, true),
    ("yrs", 365.0 * DAY, false),
    ("yr", 365.0 * DAY, true),
    ("y", 365.0 * DAY, false),
];

/// Parse a complete string as a duration, returning seconds.
pub fn parse(input: &str) -> Option<i64> {
    let s = input;
    if s.is_empty() {
        return None;
    }

    // A plain number of seconds. Small numbers (1-60) are not durations, so
    // that e.g. a tag or day number is not mistaken for one.
    if s.bytes().all(|b| b.is_ascii_digit()) {
        let n: i64 = s.parse().ok()?;
        return (n == 0 || n > 60).then_some(n);
    }

    if let Some(v) = parse_designated(s) {
        return Some(v);
    }

    // A unit that stands for one of itself: "hour", "week", ...
    if let Some((_, secs, _)) = UNITS.iter().find(|(name, _, standalone)| *standalone && *name == s) {
        return Some(*secs as i64);
    }

    // <number> [whitespace] <unit>
    let number_end = s.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(s.len());
    let (number, unit) = s.split_at(number_end);
    if number.is_empty() || !number.starts_with(|c: char| c.is_ascii_digit()) || number.matches('.').count() > 1 {
        return None;
    }
    let number: f64 = number.parse().ok()?;
    let unit = unit.trim_start();
    let (_, secs, _) = UNITS.iter().find(|(name, _, _)| *name == unit)?;
    if unit == "d" && number > 10000.0 {
        return None;
    }
    Some((number * secs) as i64)
}

/// ISO 8601 durations: `P[nY][nM][nD][T[nH][nM][nS]]` or `PnW`, optionally
/// negated with a leading `-`.
fn parse_designated(s: &str) -> Option<i64> {
    let (sign, rest) = match s.strip_prefix('-') {
        Some(r) => (-1, r),
        None => (1, s),
    };
    let body = rest.strip_prefix('P')?;
    if body.is_empty() {
        return None;
    }

    if let Some(weeks) = body.strip_suffix('W')
        && sign == 1
        && !weeks.is_empty()
        && weeks.bytes().all(|b| b.is_ascii_digit())
    {
        return Some(weeks.parse::<i64>().ok()? * 7 * 86400);
    }

    let (date_part, time_part) = match body.split_once('T') {
        Some((d, t)) => (d, Some(t)),
        None => (body, None),
    };
    let mut total = 0;
    let mut take = |part: &str, designators: &[(char, i64)]| -> Option<()> {
        let mut rest = part;
        let mut allowed = designators;
        while !rest.is_empty() {
            let digits = rest.find(|c: char| !c.is_ascii_digit())?;
            if digits == 0 {
                return None;
            }
            let value: i64 = rest[..digits].parse().ok()?;
            let designator = rest[digits..].chars().next()?;
            let pos = allowed.iter().position(|(d, _)| *d == designator)?;
            total += value * allowed[pos].1;
            allowed = &allowed[pos + 1..];
            rest = &rest[digits + 1..];
        }
        Some(())
    };
    take(date_part, &[('Y', 365 * 86400), ('M', 30 * 86400), ('D', 86400)])?;
    if let Some(t) = time_part {
        if t.is_empty() {
            return None;
        }
        take(t, &[('H', 3600), ('M', 60), ('S', 1)])?;
    }
    Some(sign * total)
}

/// `H:MM:SS` with unbounded hours, e.g. `0:30:00`, `26:00:00`.
pub fn format_hours(seconds: i64) -> String {
    let sign = if seconds < 0 { "-" } else { "" };
    let t = seconds.abs();
    format!("{sign}{}:{:02}:{:02}", t / 3600, (t % 3600) / 60, t % 60)
}

/// `[Nd ]H:MM:SS`.
pub fn format_days(seconds: i64) -> String {
    let sign = if seconds < 0 { "-" } else { "" };
    let t = seconds.abs();
    let days = t / 86400;
    let rest = t % 86400;
    let day_part = if days > 0 { format!("{days}d ") } else { String::new() };
    format!("{sign}{day_part}{}:{:02}:{:02}", rest / 3600, (rest % 3600) / 60, rest % 60)
}

/// ISO 8601, e.g. `PT1H30M`, `P2DT3H`, `PT0S`.
pub fn format_iso(seconds: i64) -> String {
    if seconds == 0 {
        return "PT0S".to_string();
    }
    let sign = if seconds < 0 { "-" } else { "" };
    let t = seconds.abs();
    let (days, h, m, s) = (t / 86400, (t % 86400) / 3600, (t % 3600) / 60, t % 60);
    let mut out = format!("{sign}P");
    if days > 0 {
        out.push_str(&format!("{days}D"));
    }
    if h > 0 || m > 0 || s > 0 {
        out.push('T');
        if h > 0 {
            out.push_str(&format!("{h}H"));
        }
        if m > 0 {
            out.push_str(&format!("{m}M"));
        }
        if s > 0 {
            out.push_str(&format!("{s}S"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_units() {
        assert_eq!(parse("1h"), Some(3600));
        assert_eq!(parse("30min"), Some(1800));
        assert_eq!(parse("1.5h"), Some(5400));
        assert_eq!(parse("2 hours"), Some(7200));
        assert_eq!(parse("hour"), Some(3600));
        assert_eq!(parse("h"), None);
        assert_eq!(parse("10m"), Some(10 * 30 * 86400));
        assert_eq!(parse("90"), Some(90));
        assert_eq!(parse("12"), None);
        assert_eq!(parse("foo"), None);
        assert_eq!(parse("1hx"), None);
    }

    #[test]
    fn parses_iso() {
        assert_eq!(parse("PT1H30M"), Some(5400));
        assert_eq!(parse("P1D"), Some(86400));
        assert_eq!(parse("P2W"), Some(14 * 86400));
        assert_eq!(parse("P"), None);
        assert_eq!(parse("PT"), None);
    }

    #[test]
    fn formats() {
        assert_eq!(format_hours(0), "0:00:00");
        assert_eq!(format_hours(93784), "26:03:04");
        assert_eq!(format_iso(0), "PT0S");
        assert_eq!(format_iso(5400), "PT1H30M");
        assert_eq!(format_iso(2 * 86400 + 3 * 3600), "P2DT3H");
        assert_eq!(format_days(90061), "1d 1:01:01");
    }
}
