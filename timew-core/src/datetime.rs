//! Dates and times: parsing everything Timewarrior accepts on the command
//! line, plus the local-calendar arithmetic the reports need.
//!
//! All instants are seconds since the Unix epoch ([`Epoch`]). Calendar
//! questions (which day, which week, midnight) are answered in the system's
//! local time zone via `jiff`, so daylight saving time is handled.
//!
//! Accepted inputs:
//!
//! - ISO 8601, extended or compact: `2025-03-01`, `2025-03-01T08:00`,
//!   `2025-03-01T08:00:00Z`, `20250301T080000Z`, `2025-03`, `2025-W10`,
//!   `2025-W10-2`, `2025-060`, times `08:00`, `08:00:00`, `0800`, with an
//!   optional `Z` or `±hh[:mm]` offset.
//! - Epoch seconds (from 1980 on).
//! - Informal times: `8am`, `8:30pm`, `8:30`, `12p`.
//! - Names: `now`, `today`, `yesterday`, `tomorrow`, day names (`mon`,
//!   `monday`), month names (`mar`), ordinals (`15th`), `sod`/`eod`,
//!   `sow`/`eow`, `som`/`eom`, `soq`/`eoq`, `soy`/`eoy` with their
//!   `p`(revious) and `n`(ext) variants (`sopw`, `eonm`, ...), work weeks
//!   (`soww`, `eoww`, ...), `later`/`someday`, `easter`, `eastermonday`,
//!   `ascension`, `pentecost`, `goodfriday`, `midsommar`,
//!   `midsommarafton`/`juhannus`.
//!
//! Times without a date are today; day and month names refer to the most
//! recent occurrence (today and this month included), never the future.
//! The `eo*` names are the exclusive end, i.e. midnight after the period.

use jiff::civil::{Date, DateTime, Weekday};
use jiff::tz::TimeZone;
use jiff::{Timestamp, Zoned};

use crate::range::{Epoch, Range};

/// Smallest/largest number accepted as epoch seconds (1980-01-01 .. 9999).
const EPOCH_MIN: i64 = 315_532_800;
const EPOCH_MAX: i64 = 253_402_293_599;

pub fn now() -> Epoch {
    Timestamp::now().as_second()
}

/// The system time zone, looked up once per run (looking it up is much
/// slower than using it, and reports convert thousands of times).
fn tz() -> TimeZone {
    static TZ: std::sync::OnceLock<TimeZone> = std::sync::OnceLock::new();
    TZ.get_or_init(system_time_zone).clone()
}

/// The system time zone, read straight from its zone file: `$TZ` naming a
/// zone (`Europe/Berlin`, `:Europe/Berlin` or a path), else
/// `/etc/localtime`. This is the same data `jiff` would use, but much
/// faster than `TimeZone::system()`, which first searches the time zone
/// database. Anything else (such as a POSIX rule in `$TZ`) goes to `jiff`.
fn system_time_zone() -> TimeZone {
    use std::path::{Path, PathBuf};
    let zoneinfo = std::env::var_os("TZDIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/usr/share/zoneinfo"));
    let load = |name: &str, path: &Path| std::fs::read(path).ok().and_then(|bytes| TimeZone::tzif(name, &bytes).ok());
    let found = match std::env::var("TZ") {
        Ok(tz) if !tz.is_empty() => {
            let name = tz.strip_prefix(':').unwrap_or(&tz);
            let is_name = name.split('/').all(|part| !part.is_empty() && part != ".." && part.chars().all(|c| c.is_ascii_alphanumeric() || "_-+".contains(c)));
            if name.starts_with('/') {
                load(name.rsplit_once("zoneinfo/").map_or(name, |(_, n)| n), Path::new(name))
            } else if is_name {
                load(name, &zoneinfo.join(name))
            } else {
                None
            }
        }
        Err(std::env::VarError::NotPresent) => {
            let link = std::fs::read_link("/etc/localtime").ok();
            let name = link
                .as_ref()
                .and_then(|p| p.to_str())
                .and_then(|p| p.rsplit_once("zoneinfo/"))
                .map_or("Local".to_string(), |(_, n)| n.to_string());
            load(&name, Path::new("/etc/localtime"))
        }
        _ => None,
    };
    found.unwrap_or_else(TimeZone::system)
}

/// The local date and time of an instant.
pub fn local(epoch: Epoch) -> Zoned {
    Timestamp::from_second(epoch).unwrap_or(Timestamp::UNIX_EPOCH).to_zoned(tz())
}

/// The instant of a local civil time. Out-of-range fields are normalized
/// the way `mktime` does: month 13 is January of the next year, day 0 is
/// the last day of the previous month, hour 24 is midnight of the next day.
pub fn make_local(year: i64, month: i64, day: i64, hour: i64, minute: i64, second: i64) -> Epoch {
    let dt = normalized_civil(year, month, day, hour, minute, second);
    dt.to_zoned(tz()).map(|z| z.timestamp().as_second()).unwrap_or(0)
}

/// Like [`make_local`], but in UTC.
pub fn make_utc(year: i64, month: i64, day: i64, hour: i64, minute: i64, second: i64) -> Epoch {
    let dt = normalized_civil(year, month, day, hour, minute, second);
    dt.to_zoned(TimeZone::UTC).map(|z| z.timestamp().as_second()).unwrap_or(0)
}

fn normalized_civil(year: i64, month: i64, day: i64, hour: i64, minute: i64, second: i64) -> DateTime {
    let m0 = month - 1;
    let y = (year + m0.div_euclid(12)).clamp(-9999, 9999) as i16;
    let m = (m0.rem_euclid(12) + 1) as i8;
    let first = Date::new(y, m, 1).unwrap_or(Date::constant(1970, 1, 1));
    let seconds = (day - 1) * 86400 + hour * 3600 + minute * 60 + second;
    first
        .to_datetime(jiff::civil::Time::midnight())
        .checked_add(jiff::Span::new().seconds(seconds))
        .unwrap_or(first.to_datetime(jiff::civil::Time::midnight()))
}

/// Local midnight at the start of the day containing `epoch`.
pub fn start_of_day(epoch: Epoch) -> Epoch {
    let z = local(epoch);
    make_local(z.year() as i64, z.month() as i64, z.day() as i64, 0, 0, 0)
}

/// Local midnight `days` days after the day containing `epoch`.
pub fn add_days(epoch: Epoch, days: i64) -> Epoch {
    let z = local(epoch);
    make_local(z.year() as i64, z.month() as i64, z.day() as i64 + days, 0, 0, 0)
}

/// The whole local day containing `epoch`.
pub fn full_day(epoch: Epoch) -> Range {
    Range::new(start_of_day(epoch), add_days(epoch, 1))
}

pub fn days_in_month(year: i64, month: i64) -> i64 {
    Date::new(year as i16, month as i8, 1).map(|d| d.days_in_month() as i64).unwrap_or(30)
}

pub fn days_in_year(year: i64) -> i64 {
    if Date::new(year as i16, 1, 1).map(|d| d.in_leap_year()).unwrap_or(false) { 366 } else { 365 }
}

/// 0 = Sunday .. 6 = Saturday.
pub fn weekday_sunday0(w: Weekday) -> i64 {
    w.to_sunday_zero_offset() as i64
}

/// ISO 8601 week number of the local day containing `epoch`.
pub fn iso_week(epoch: Epoch) -> i64 {
    local(epoch).date().iso_week_date().week() as i64
}

pub fn same_day(a: Epoch, b: Epoch) -> bool {
    local(a).date() == local(b).date()
}

pub const DAY_NAMES: [&str; 7] = ["sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday"];
pub const MONTH_NAMES: [&str; 12] = [
    "january", "february", "march", "april", "may", "june", "july", "august", "september", "october",
    "november", "december",
];

/// "Mon", "Tue", ...
pub fn day_name_short(epoch: Epoch) -> String {
    let name = DAY_NAMES[weekday_sunday0(local(epoch).weekday()) as usize];
    capitalize(&name[..3])
}

/// "Jan", "Feb", ...
pub fn month_name_short(month: i64) -> String {
    capitalize(&MONTH_NAMES[(month - 1).clamp(0, 11) as usize][..3])
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_ascii_uppercase().to_string() + c.as_str(),
        None => String::new(),
    }
}

/// Day-of-week index from a name (`monday` .. `sunday`, or a 3+ letter
/// abbreviation): 0 = Sunday .. 6 = Saturday.
pub fn day_of_week(name: &str) -> Option<usize> {
    let name = name.to_ascii_lowercase();
    if name.len() < 3 {
        return None;
    }
    DAY_NAMES.iter().position(|d| d.starts_with(&name))
}

/// `2025-03-01T08:00:00` in local time.
pub fn format_local_iso(epoch: Epoch) -> String {
    let z = local(epoch);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        z.year(),
        z.month(),
        z.day(),
        z.hour(),
        z.minute(),
        z.second()
    )
}

/// `2025-03-01` in local time.
pub fn format_ymd(epoch: Epoch) -> String {
    let z = local(epoch);
    format!("{:04}-{:02}-{:02}", z.year(), z.month(), z.day())
}

/// `8:05:00` (hour without padding) in local time.
pub fn format_time(epoch: Epoch) -> String {
    let z = local(epoch);
    format!("{}:{:02}:{:02}", z.hour(), z.minute(), z.second())
}

// ---------------------------------------------------------------------------
// Parsing

/// What kind of period a single date names, for use as a range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Extent {
    /// An instant (a range starting there, open-ended).
    Point,
    Day,
    Week,
    Month,
}

/// Parse a complete string as an instant. `None` if it is not a date.
pub fn parse_point(input: &str) -> Option<Epoch> {
    parse(input, now()).map(|(epoch, _)| epoch)
}

/// Parse a complete string as an instant, with an error message if it is
/// not a date.
pub fn parse_datetime(input: &str) -> Result<Epoch, String> {
    parse_point(input.trim()).ok_or_else(|| format!("'{input}' is not a valid date."))
}

/// Parse a single date used as a whole range: a date names its day, week or
/// month (`2025-03-01`, `2025-W10`, `2025-03`, `yesterday`, `monday`,
/// `march`); a date with a time or a named instant starts an open range.
pub fn parse_range(input: &str) -> Result<Range, String> {
    let (start, extent) = parse(input, now()).ok_or_else(|| format!("'{input}' is not a valid range."))?;
    let z = local(start);
    let (y, m, d) = (z.year() as i64, z.month() as i64, z.day() as i64);
    Ok(match extent {
        Extent::Point => Range::new(start, 0),
        Extent::Day => Range::new(start, make_local(y, m, d + 1, 0, 0, 0)),
        Extent::Week => Range::new(start, make_local(y, m, d + 7, 0, 0, 0)),
        Extent::Month => Range::new(start, make_local(y, m + 1, 1, 0, 0, 0)),
    })
}

/// Parse a complete string as a date, returning the instant and the kind
/// of period it names.
fn parse(input: &str, now: Epoch) -> Option<(Epoch, Extent)> {
    if input.is_empty() || !input.is_ascii() {
        return None;
    }
    if input.bytes().all(|b| b.is_ascii_digit())
        && let Ok(e) = input.parse::<i64>()
            && (EPOCH_MIN..EPOCH_MAX).contains(&e)
        {
            return Some((e, Extent::Point));
        }
    if let Some(fields) = Iso::parse(input) {
        if fields.valid() {
            return Some((fields.resolve(now), fields.extent()));
        }
        return None;
    }
    named(input, now)
}

/// Fields collected from an ISO 8601 date/time.
#[derive(Debug, Default, Clone, Copy)]
struct Iso {
    year: i64,
    month: i64,
    week: i64,
    weekday: i64,
    julian: i64,
    day: i64,
    seconds: i64,
    has_time: bool,
    offset: i64,
    utc: bool,
}

struct Cursor<'a> {
    s: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn peek(&self) -> u8 {
        *self.s.get(self.pos).unwrap_or(&0)
    }
    fn peek_digit(&self) -> bool {
        self.peek().is_ascii_digit()
    }
    fn skip(&mut self, c: u8) -> bool {
        if self.peek() == c {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn digits(&mut self, n: usize) -> Option<i64> {
        let end = self.pos + n;
        let slice = self.s.get(self.pos..end)?;
        if !slice.iter().all(u8::is_ascii_digit) {
            return None;
        }
        self.pos = end;
        std::str::from_utf8(slice).ok()?.parse().ok()
    }
    fn number(&mut self, n: usize, range: std::ops::RangeInclusive<i64>) -> Option<i64> {
        let save = self.pos;
        match self.digits(n) {
            Some(v) if range.contains(&v) => Some(v),
            _ => {
                self.pos = save;
                None
            }
        }
    }
    fn eos(&self) -> bool {
        self.pos >= self.s.len()
    }
}

impl Iso {
    /// Try the ISO forms, strictest first. The whole input must be used.
    fn parse(input: &str) -> Option<Iso> {
        let forms: [fn(&mut Cursor, &mut Iso) -> bool; 10] = [
            |c, f| f.date_ext(c) && c.skip(b'T') && (f.time_utc_ext(c) || f.time_off_ext(c) || f.time_ext(c, true)),
            |c, f| f.date(c) && c.skip(b'T') && (f.time_utc(c) || f.time_off(c) || f.time(c, true)),
            |c, f| f.date_ext(c),
            |c, f| f.date(c),
            |c, f| f.time_utc_ext(c),
            |c, f| f.time_utc(c),
            |c, f| f.time_off_ext(c),
            |c, f| f.time_ext(c, true),
            |c, f| f.time(c, true),
            |c, f| f.time_off(c),
        ];
        for form in forms {
            let mut cursor = Cursor { s: input.as_bytes(), pos: 0 };
            let mut fields = Iso { weekday: 1, ..Iso::default() };
            if form(&mut cursor, &mut fields) {
                // The first form that matches decides; it must use it all.
                return cursor.eos().then_some(fields);
            }
        }
        None
    }

    fn year(c: &mut Cursor) -> Option<i64> {
        c.number(4, 1970..=9999)
    }

    /// `YYYY-MM-DD`, `YYYY-MM`, `YYYY-DDD`, `YYYY-Www[-D]`
    fn date_ext(&mut self, c: &mut Cursor) -> bool {
        let start = c.pos;
        let Some(year) = Self::year(c) else { return false };
        if !c.skip(b'-') {
            c.pos = start;
            return false;
        }
        let after_year = c.pos;
        if c.skip(b'W')
            && let Some(week) = c.number(2, 1..=53)
        {
            let save = c.pos;
            let weekday = if c.skip(b'-')
                && let Some(d) = c.number(1, 1..=7)
            {
                d
            } else {
                c.pos = save;
                1
            };
            if !c.peek_digit() {
                (self.year, self.week, self.weekday) = (year, week, weekday);
                return true;
            }
        }
        c.pos = after_year;
        if let Some(month) = c.number(2, 1..=12)
            && c.skip(b'-')
            && let Some(day) = c.number(2, 1..=31)
            && !c.peek_digit()
        {
            (self.year, self.month, self.day) = (year, month, day);
            return true;
        }
        c.pos = after_year;
        if let Some(julian) = c.number(3, 1..=366)
            && !c.peek_digit()
        {
            (self.year, self.julian) = (year, julian);
            return true;
        }
        c.pos = after_year;
        if let Some(month) = c.number(2, 1..=12)
            && c.peek() != b'-'
            && !c.peek_digit()
        {
            (self.year, self.month) = (year, month);
            return true;
        }
        c.pos = start;
        false
    }

    /// `YYYYMMDD`, `YYYYMM`, `YYYYDDD`, `YYYYWww[D]`
    fn date(&mut self, c: &mut Cursor) -> bool {
        let start = c.pos;
        let Some(year) = Self::year(c) else { return false };
        let after_year = c.pos;
        if c.skip(b'W')
            && let Some(week) = c.number(2, 1..=53)
        {
            let weekday = c.number(1, 1..=7).unwrap_or(1);
            if !c.peek_digit() {
                (self.year, self.week, self.weekday) = (year, week, weekday);
                return true;
            }
        }
        c.pos = after_year;
        if let Some(julian) = c.number(3, 1..=366)
            && !c.peek_digit()
        {
            (self.year, self.julian) = (year, julian);
            return true;
        }
        c.pos = after_year;
        if let Some(month) = c.number(2, 1..=12) {
            let day = c.number(2, 1..=31);
            if !c.peek_digit() {
                (self.year, self.month, self.day) = (year, month, day.unwrap_or(0));
                return true;
            }
        }
        c.pos = start;
        false
    }

    fn set_time(&mut self, h: i64, m: i64, s: i64) {
        self.seconds = h * 3600 + m * 60 + s;
        self.has_time = true;
    }

    /// `hh:mm[:ss]`
    fn time_ext(&mut self, c: &mut Cursor, terminated: bool) -> bool {
        let start = c.pos;
        if let Some(h) = c.number(2, 0..=23)
            && c.skip(b':')
            && let Some(m) = c.number(2, 0..=59)
        {
            if c.skip(b':') {
                if let Some(s) = c.number(2, 0..=59)
                    && !c.peek_digit()
                    && (!terminated || (c.peek() != b'-' && c.peek() != b'+'))
                {
                    self.set_time(h, m, s);
                    return true;
                }
                c.pos = start;
                return false;
            }
            let next = c.peek();
            if !next.is_ascii_digit()
                && (!terminated || (next != b'+' && next != b'-'))
                && !matches!(next, b'A' | b'a' | b'P' | b'p')
            {
                self.set_time(h, m, 0);
                return true;
            }
        }
        c.pos = start;
        false
    }

    fn time_utc_ext(&mut self, c: &mut Cursor) -> bool {
        let start = c.pos;
        if self.time_ext(c, false) && c.skip(b'Z') && !c.peek_digit() {
            self.utc = true;
            return true;
        }
        c.pos = start;
        false
    }

    fn time_off_ext(&mut self, c: &mut Cursor) -> bool {
        let start = c.pos;
        if self.time_ext(c, false) && self.offset_ext(c) {
            return true;
        }
        c.pos = start;
        false
    }

    /// `±hh[:mm]`
    fn offset_ext(&mut self, c: &mut Cursor) -> bool {
        let start = c.pos;
        let sign = c.peek();
        if sign == b'+' || sign == b'-' {
            c.pos += 1;
            if let Some(h) = c.number(2, 0..=12) {
                let mut m = 0;
                if c.skip(b':') {
                    match c.number(2, 0..=59) {
                        Some(v) => m = v,
                        None => {
                            c.pos = start;
                            return false;
                        }
                    }
                }
                if !c.peek_digit() {
                    self.offset = (h * 3600 + m * 60) * if sign == b'-' { -1 } else { 1 };
                    return true;
                }
            }
        }
        c.pos = start;
        false
    }

    /// `hhmm[ss]`
    fn time(&mut self, c: &mut Cursor, terminated: bool) -> bool {
        let start = c.pos;
        if let Some(h) = c.number(2, 0..=23)
            && let Some(m) = c.number(2, 0..=59)
        {
            let s = c.number(2, 0..=59).unwrap_or(0);
            let next = c.peek();
            if !terminated || (!next.is_ascii_digit() && next != b'-' && next != b'+') {
                self.set_time(h, m, s);
                return true;
            }
        }
        c.pos = start;
        false
    }

    fn time_utc(&mut self, c: &mut Cursor) -> bool {
        let start = c.pos;
        if self.time(c, false) && c.skip(b'Z') && !c.peek_digit() {
            self.utc = true;
            return true;
        }
        c.pos = start;
        false
    }

    fn time_off(&mut self, c: &mut Cursor) -> bool {
        let start = c.pos;
        if self.time(c, false) && self.offset(c) && c.peek() != b'-' && !c.peek_digit() {
            return true;
        }
        c.pos = start;
        false
    }

    /// `±hh[mm]`
    fn offset(&mut self, c: &mut Cursor) -> bool {
        let start = c.pos;
        let sign = c.peek();
        if sign == b'+' || sign == b'-' {
            c.pos += 1;
            if let Some(h) = c.number(2, 0..=12) {
                let m = c.number(2, 0..=59).unwrap_or(0);
                if !c.peek_digit() {
                    self.offset = (h * 3600 + m * 60) * if sign == b'-' { -1 } else { 1 };
                    return true;
                }
            }
        }
        c.pos = start;
        false
    }

    fn valid(&self) -> bool {
        let bad = (self.year != 0 && !(1900..=9999).contains(&self.year))
            || (self.month != 0 && !(1..=12).contains(&self.month))
            || (self.week != 0 && !(1..=53).contains(&self.week))
            || !(1..=7).contains(&self.weekday)
            || (self.julian != 0 && !(1..=days_in_year(self.year)).contains(&self.julian))
            || (self.day != 0 && !(1..=days_in_month(self.year, self.month.max(1))).contains(&self.day))
            || !(-86400..=86400).contains(&self.offset);
        !bad
    }

    fn extent(&self) -> Extent {
        if self.has_time {
            Extent::Point
        } else if self.week != 0 {
            Extent::Week
        } else if self.month != 0 && self.day == 0 {
            Extent::Month
        } else {
            Extent::Day
        }
    }

    fn resolve(&self, now: Epoch) -> Epoch {
        let mut seconds = self.seconds;
        let mut utc = self.utc;
        let mut reference = now;
        if self.offset != 0 {
            seconds -= self.offset;
            reference += self.offset;
            utc = true;
        }
        let (mut year, mut month, mut day) = (self.year, self.month, self.day);
        if self.week != 0 {
            // ISO week date to day of year, possibly in a neighboring year.
            let jan4 = Date::new(year as i16, 1, 4).map(|d| d.weekday().to_monday_one_offset() as i64).unwrap_or(1);
            let mut julian = self.week * 7 + self.weekday - (jan4 + 3);
            if julian < 1 {
                year -= 1;
                julian += days_in_year(year);
            } else if julian > days_in_year(year) {
                julian -= days_in_year(year);
                year += 1;
            }
            month = 1;
            day = julian;
        } else if self.julian != 0 {
            month = 1;
            day = self.julian;
        } else if year == 0 {
            let today = if utc {
                Timestamp::from_second(reference).unwrap().to_zoned(TimeZone::UTC)
            } else {
                local(reference)
            };
            (year, month, day) = (today.year() as i64, today.month() as i64, today.day() as i64);
        } else if month == 0 {
            (month, day) = (1, 1);
        } else if day == 0 {
            day = 1;
        }
        if utc {
            make_utc(year, month, day, 0, 0, seconds)
        } else {
            make_local(year, month, day, 0, 0, seconds)
        }
    }
}

/// Named dates and informal times, relative to `now`.
fn named(input: &str, now: Epoch) -> Option<(Epoch, Extent)> {
    let z = local(now);
    let (y, m, d) = (z.year() as i64, z.month() as i64, z.day() as i64);
    let wday = weekday_sunday0(z.weekday());
    let monday_offset = (wday + 6) % 7;
    let midnight = |yy: i64, mm: i64, dd: i64| make_local(yy, mm, dd, 0, 0, 0);
    let quarter_start = m - (m - 1) % 3;

    // Names that may be abbreviated to 3 or more characters.
    let abbreviates = |full: &str| input.len() >= 3 && full.starts_with(input);
    let lower = input.to_ascii_lowercase();
    let abbreviates_ci = |full: &str| lower.len() >= 3 && full.starts_with(&lower);

    if input == "now" {
        return Some((now, Extent::Point));
    }
    if abbreviates("yesterday") {
        return Some((midnight(y, m, d - 1), Extent::Day));
    }
    if abbreviates("today") {
        return Some((midnight(y, m, d), Extent::Day));
    }
    if abbreviates("tomorrow") {
        return Some((midnight(y, m, d + 1), Extent::Day));
    }
    if let Some(n) = ordinal(input) {
        // The most recent such day: this month, or last month if it is
        // still to come this month.
        let month = if d < n && n <= days_in_month(y, m) { m - 1 } else { m };
        return Some((midnight(y, month, n), Extent::Day));
    }
    if let Some(day) = DAY_NAMES.iter().position(|n| abbreviates_ci(n)) {
        let day = day as i64;
        let delta = if wday >= day { day - wday } else { day - wday - 7 };
        return Some((midnight(y, m, d + delta), Extent::Day));
    }
    if let Some(month) = MONTH_NAMES.iter().position(|n| abbreviates_ci(n)) {
        return Some((midnight(y, month as i64 + 1, 1), Extent::Month));
    }
    if abbreviates("later") || (input.len() >= 4 && "someday".starts_with(input)) {
        return Some((midnight(9999, 12, 30), Extent::Point));
    }

    let point = |e: Epoch| Some((e, Extent::Point));
    match input {
        "sopd" => return point(midnight(y, m, d - 1)),
        "sod" => return point(midnight(y, m, d)),
        "sond" => return point(midnight(y, m, d + 1)),
        "eopd" => return point(midnight(y, m, d)),
        "eod" => return point(midnight(y, m, d + 1)),
        "eond" => return point(midnight(y, m, d + 2)),
        "sopw" => return point(midnight(y, m, d - monday_offset - 7)),
        "sow" => return point(midnight(y, m, d - monday_offset)),
        "sonw" => return point(midnight(y, m, d - monday_offset + 7)),
        "eopw" => return point(midnight(y, m, d - monday_offset)),
        "eow" => return point(midnight(y, m, d - monday_offset + 7)),
        "eonw" => return point(midnight(y, m, d - monday_offset + 14)),
        // Work weeks: Monday to Friday.
        "sopww" => return point(midnight(y, m, d - monday_offset - 7)),
        "soww" => return point(midnight(y, m, d - monday_offset)),
        "sonww" => return point(midnight(y, m, d - monday_offset + 7)),
        "eopww" => return point(midnight(y, m, d - monday_offset - 2)),
        "eoww" => return point(midnight(y, m, d - monday_offset + 5)),
        "eonww" => return point(midnight(y, m, d - monday_offset + 12)),
        "sopm" => return point(midnight(y, m - 1, 1)),
        "som" => return point(midnight(y, m, 1)),
        "sonm" => return point(midnight(y, m + 1, 1)),
        "eopm" => return point(midnight(y, m, 1)),
        "eom" => return point(midnight(y, m + 1, 1)),
        "eonm" => return point(midnight(y, m + 2, 1)),
        "sopq" => return point(midnight(y, quarter_start - 3, 1)),
        "soq" => return point(midnight(y, quarter_start, 1)),
        "sonq" => return point(midnight(y, quarter_start + 3, 1)),
        "eopq" => return point(midnight(y, quarter_start, 1)),
        "eoq" => return point(midnight(y, quarter_start + 3, 1)),
        "eonq" => return point(midnight(y, quarter_start + 6, 1)),
        "sopy" => return point(midnight(y - 1, 1, 1)),
        "soy" => return point(midnight(y, 1, 1)),
        "sony" => return point(midnight(y + 1, 1, 1)),
        "eopy" => return point(midnight(y, 1, 1)),
        "eoy" => return point(midnight(y + 1, 1, 1)),
        "eony" => return point(midnight(y + 2, 1, 1)),
        _ => {}
    }

    // Holidays: the next occurrence (today included).
    let next_holiday = |of_year: &dyn Fn(i64) -> (i64, i64, i64)| {
        let at = |year| {
            let (yy, mm, dd) = of_year(year);
            midnight(yy, mm, dd)
        };
        let this_year = at(y);
        if this_year < midnight(y, m, d) { at(y + 1) } else { this_year }
    };
    let easter_offset = match input {
        "easter" => Some(0),
        "eastermonday" => Some(1),
        "ascension" => Some(39),
        "pentecost" => Some(49),
        "goodfriday" => Some(-2),
        _ => None,
    };
    if let Some(offset) = easter_offset {
        let day = next_holiday(&|year| {
            let (mm, dd) = easter(year);
            (year, mm, dd + offset)
        });
        return Some((day, Extent::Day));
    }
    let midsummer = |first: i64, weekday: i64| {
        move |year: i64| {
            let wd = weekday_sunday0(Date::new(year as i16, 6, first as i8).unwrap().weekday());
            (year, 6, first + (weekday - wd).rem_euclid(7))
        }
    };
    match input {
        "midsommar" => return Some((next_holiday(&midsummer(20, 6)), Extent::Day)),
        "midsommarafton" | "juhannus" => return Some((next_holiday(&midsummer(19, 5)), Extent::Day)),
        _ => {}
    }

    informal_time(input).map(|secs| (make_local(y, m, d, 0, 0, secs), Extent::Point))
}

/// `1st`, `2nd`, `3rd`, `4th`, ... `31st`.
fn ordinal(input: &str) -> Option<i64> {
    let digits = input.find(|c: char| !c.is_ascii_digit())?;
    if digits == 0 {
        return None;
    }
    let n: i64 = input[..digits].parse().ok()?;
    if !(1..=31).contains(&n) {
        return None;
    }
    let suffix = match (n % 100, n % 10) {
        (11..=13, _) => "th",
        (_, 1) => "st",
        (_, 2) => "nd",
        (_, 3) => "rd",
        _ => "th",
    };
    (&input[digits..] == suffix).then_some(n)
}

/// `8am`, `8a`, `8:30pm`, `8:30`, `8:30:15` as seconds after midnight.
fn informal_time(input: &str) -> Option<i64> {
    let mut c = Cursor { s: input.as_bytes(), pos: 0 };
    let mut hours = c.digits(1)?;
    if let Some(d) = c.digits(1) {
        hours = hours * 10 + d;
    }
    let mut minutes = 0;
    let mut seconds = 0;
    let mut need_designator = true;
    if c.skip(b':') {
        minutes = c.digits(2)?;
        if c.skip(b':') {
            let start = c.pos;
            while c.peek_digit() {
                c.pos += 1;
            }
            seconds = std::str::from_utf8(&c.s[start..c.pos]).ok()?.parse().ok()?;
        }
        need_designator = false;
    }
    let rest = &input[c.pos..];
    let designator = match rest {
        "am" | "a" => {
            if hours == 12 {
                hours = 0;
            }
            true
        }
        "pm" | "p" => {
            if hours != 12 {
                hours += 12;
            }
            true
        }
        "" => false,
        _ => return None,
    };
    if need_designator && !designator {
        return None;
    }
    if hours < 24 && minutes < 60 && seconds < 60 {
        Some(hours * 3600 + minutes * 60 + seconds)
    } else {
        None
    }
}

/// Month and day of Easter Sunday (Gregorian calendar).
fn easter(year: i64) -> (i64, i64) {
    let a = year % 19;
    let b = year / 100;
    let c = year % 100;
    let d = b / 4;
    let e = b % 4;
    let f = (b + 8) / 25;
    let g = (b - f + 1) / 3;
    let h = (19 * a + b - d - g + 15) % 30;
    let i = c / 4;
    let k = c % 4;
    let l = (32 + 2 * e + 2 * i - h - k) % 7;
    let m = (a + 11 * h + 22 * l) / 451;
    let month = (h + l - 7 * m + 114) / 31;
    let day = ((h + l - 7 * m + 114) % 31) + 1;
    (month, day)
}

// ---------------------------------------------------------------------------
// Range hints

/// Expand a range hint (without the leading `:`): `day`/`today`,
/// `yesterday`, `week`, `lastweek`, `fortnight`, `month`, `lastmonth`,
/// `quarter`, `lastquarter`, `year`, `lastyear`, `monday` .. `sunday`, `all`.
pub fn expand_range_hint(hint: &str) -> Option<Range> {
    expand_range_hint_at(hint, now())
}

pub fn expand_range_hint_at(hint: &str, now: Epoch) -> Option<Range> {
    let point = |name: &str| named(name, now).map(|(e, _)| e).unwrap();
    let z = local(now);
    let (y, m, d) = (z.year() as i64, z.month() as i64, z.day() as i64);
    let monday_offset = (weekday_sunday0(z.weekday()) + 6) % 7;
    let quarter_start = m - (m - 1) % 3;

    let range = match hint {
        "all" => Range::empty(),
        "day" | "today" => Range::new(point("sod"), point("sond")),
        "yesterday" => Range::new(point("sopd"), point("sod")),
        "week" => Range::new(point("sow"), point("sonw")),
        "lastweek" => Range::new(
            make_local(y, m, d - monday_offset - 7, 0, 0, 0),
            make_local(y, m, d - monday_offset, 0, 0, 0),
        ),
        "fortnight" => Range::new(point("sopw"), point("sonw")),
        "month" => Range::new(point("som"), point("sonm")),
        "lastmonth" => Range::new(make_local(y, m - 1, 1, 0, 0, 0), make_local(y, m, 1, 0, 0, 0)),
        "quarter" => Range::new(point("soq"), point("sonq")),
        "lastquarter" => {
            Range::new(make_local(y, quarter_start - 3, 1, 0, 0, 0), make_local(y, quarter_start, 1, 0, 0, 0))
        }
        "year" => Range::new(point("soy"), point("sony")),
        "lastyear" => Range::new(make_local(y - 1, 1, 1, 0, 0, 0), make_local(y, 1, 1, 0, 0, 0)),
        day if DAY_NAMES.contains(&day) => {
            let (start, _) = named(day, now)?;
            Range::new(start, add_days(start, 1))
        }
        _ => return None,
    };
    Some(range)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(y: i64, m: i64, d: i64, h: i64, mi: i64) -> Epoch {
        make_local(y, m, d, h, mi, 0)
    }

    // 2025-03-05 is a Wednesday.
    fn now() -> Epoch {
        at(2025, 3, 5, 12, 34)
    }

    fn p(s: &str) -> Option<Epoch> {
        parse(s, now()).map(|(e, _)| e)
    }

    #[test]
    fn iso_forms() {
        assert_eq!(p("2025-03-01"), Some(at(2025, 3, 1, 0, 0)));
        assert_eq!(p("2025-03-01T08:30"), Some(at(2025, 3, 1, 8, 30)));
        assert_eq!(p("2025-03-01T08:30:15"), Some(at(2025, 3, 1, 8, 30) + 15));
        assert_eq!(p("20250301T083000Z"), Some(1740817800));
        assert_eq!(p("2025-03-01T08:30:00Z"), Some(1740817800));
        assert_eq!(p("2025-03-01T09:30:00+01:00"), Some(1740817800));
        assert_eq!(p("2025-03"), Some(at(2025, 3, 1, 0, 0)));
        assert_eq!(p("2025-W10"), Some(at(2025, 3, 3, 0, 0)));
        assert_eq!(p("2025-W10-3"), Some(at(2025, 3, 5, 0, 0)));
        assert_eq!(p("2025-060"), Some(at(2025, 3, 1, 0, 0)));
        assert_eq!(p("08:15"), Some(at(2025, 3, 5, 8, 15)));
        assert_eq!(p("0815"), Some(at(2025, 3, 5, 8, 15)));
        assert_eq!(p("2025-02-30"), None);
        assert_eq!(p("2025-03-01-x"), None);
        assert_eq!(p("1740817800"), Some(1740817800));
    }

    #[test]
    fn informal_and_named() {
        assert_eq!(p("8am"), Some(at(2025, 3, 5, 8, 0)));
        assert_eq!(p("8:30pm"), Some(at(2025, 3, 5, 20, 30)));
        assert_eq!(p("12am"), Some(at(2025, 3, 5, 0, 0)));
        assert_eq!(p("12pm"), Some(at(2025, 3, 5, 12, 0)));
        assert_eq!(p("9:15"), Some(at(2025, 3, 5, 9, 15)));
        assert_eq!(p("8"), None);
        assert_eq!(p("today"), Some(at(2025, 3, 5, 0, 0)));
        assert_eq!(p("yest"), Some(at(2025, 3, 4, 0, 0)));
        assert_eq!(p("ye"), None);
        assert_eq!(p("monday"), Some(at(2025, 3, 3, 0, 0)));
        assert_eq!(p("wed"), Some(at(2025, 3, 5, 0, 0)));
        assert_eq!(p("thursday"), Some(at(2025, 2, 27, 0, 0)));
        assert_eq!(p("jan"), Some(at(2025, 1, 1, 0, 0)));
        assert_eq!(p("3rd"), Some(at(2025, 3, 3, 0, 0)));
        assert_eq!(p("20th"), Some(at(2025, 2, 20, 0, 0)));
        assert_eq!(p("eod"), Some(at(2025, 3, 6, 0, 0)));
        assert_eq!(p("sow"), Some(at(2025, 3, 3, 0, 0)));
        assert_eq!(p("eom"), Some(at(2025, 4, 1, 0, 0)));
        assert_eq!(p("soq"), Some(at(2025, 1, 1, 0, 0)));
        assert_eq!(p("easter"), Some(at(2025, 4, 20, 0, 0)));
        assert_eq!(p("midsommar"), Some(at(2025, 6, 21, 0, 0)));
        assert_eq!(p("meeting"), None);
        assert_eq!(p("2016"), Some(at(2025, 3, 5, 20, 16)));
    }

    #[test]
    fn ranges() {
        let r = |s| {
            let (start, extent) = parse(s, now()).unwrap();
            (start, extent)
        };
        assert_eq!(r("2025-03-01").1, Extent::Day);
        assert_eq!(r("2025-03").1, Extent::Month);
        assert_eq!(r("2025-W10").1, Extent::Week);
        assert_eq!(r("2025-03-01T10:00").1, Extent::Point);
        assert_eq!(r("march").1, Extent::Month);
        assert_eq!(r("monday").1, Extent::Day);
        assert_eq!(r("9am").1, Extent::Point);
    }

    #[test]
    fn hints() {
        let r = expand_range_hint_at("week", now()).unwrap();
        assert_eq!(r, Range::new(at(2025, 3, 3, 0, 0), at(2025, 3, 10, 0, 0)));
        let r = expand_range_hint_at("lastmonth", now()).unwrap();
        assert_eq!(r, Range::new(at(2025, 2, 1, 0, 0), at(2025, 3, 1, 0, 0)));
        let r = expand_range_hint_at("lastquarter", now()).unwrap();
        assert_eq!(r, Range::new(at(2024, 10, 1, 0, 0), at(2025, 1, 1, 0, 0)));
        let r = expand_range_hint_at("friday", now()).unwrap();
        assert_eq!(r, Range::new(at(2025, 2, 28, 0, 0), at(2025, 3, 1, 0, 0)));
        assert_eq!(expand_range_hint_at("all", now()), Some(Range::empty()));
        assert_eq!(expand_range_hint_at("bogus", now()), None);
    }
}
