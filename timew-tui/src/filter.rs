//! The filters (day, yesterday, month, year, date range, tag) and the
//! state of the date range form and the month and year lists.

use ratatui::widgets::ListState;
use timew_core::datetime::{expand_range_hint, parse_datetime};
use timew_core::range::Range;

use crate::forms::normalize_datetime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Filter {
    Day,
    Yesterday,
    Month,
    Year,
    /// A user-entered date range (`App::custom_range`).
    Custom,
}

impl Filter {
    /// The preset filters (quick buttons 1-4, cycled with ←→).
    pub(crate) const ALL: [Filter; 4] = [Filter::Day, Filter::Yesterday, Filter::Month, Filter::Year];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Filter::Day => "Day",
            Filter::Yesterday => "Yesterday",
            Filter::Month => "Month",
            Filter::Year => "Year",
            Filter::Custom => "Range",
        }
    }

    /// The `:hint` a preset filter expands to (None for `Custom`).
    pub(crate) fn hint(self) -> Option<&'static str> {
        match self {
            Filter::Day => Some("day"),
            Filter::Yesterday => Some("yesterday"),
            Filter::Month => Some("month"),
            Filter::Year => Some("year"),
            Filter::Custom => None,
        }
    }

    pub(crate) fn cycle(self, delta: isize) -> Filter {
        let n = Filter::ALL.len() as isize;
        let Some(i) = Filter::ALL.iter().position(|f| *f == self) else {
            // from a custom range, step onto the first/last preset
            return if delta > 0 { Filter::ALL[0] } else { Filter::ALL[Filter::ALL.len() - 1] };
        };
        Filter::ALL[(i as isize + delta).rem_euclid(n) as usize]
    }
}

/// State of the open "date range" form.
#[derive(Debug, Clone, Default)]
pub(crate) struct RangeInput {
    pub(crate) from: String,
    pub(crate) to: String,
    /// 0 = From, 1 = To.
    pub(crate) field: usize,
    pub(crate) error: Option<String>,
}

impl RangeInput {
    pub(crate) fn active(&mut self) -> &mut String {
        if self.field == 0 { &mut self.from } else { &mut self.to }
    }

    pub(crate) fn active_len(&self) -> usize {
        if self.field == 0 { self.from.chars().count() } else { self.to.chars().count() }
    }
}

/// Parse the form's From/To into a range. `To` given as a bare date is
/// inclusive (the range ends at the following midnight); an empty `To`
/// leaves the range open-ended. `From` may also be a `:hint` such as
/// `:lastyear` or `:week` (then `To` must be empty).
pub(crate) fn parse_range_input(from: &str, to: &str) -> Result<Range, String> {
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

/// State of the open month list (`Shift+3`).
pub(crate) struct MonthPicker {
    pub(crate) year: i16,
    pub(crate) state: ListState,
}

pub(crate) const MONTH_NAMES: [&str; 12] = [
    "January", "February", "March", "April", "May", "June",
    "July", "August", "September", "October", "November", "December",
];

/// State of the open year list (`Shift+4`).
pub(crate) struct YearPicker {
    /// Selectable years, newest first.
    pub(crate) years: Vec<i16>,
    pub(crate) state: ListState,
}

/// Local-time range covering the calendar year `year`.
pub(crate) fn year_range(year: i16) -> Range {
    Range::new(month_range(year, 1).start, month_range(year, 12).end)
}

/// Local-time range covering the day `date`.
pub(crate) fn day_range(date: jiff::civil::Date) -> Range {
    let tz = jiff::tz::TimeZone::system();
    let epoch = |d: jiff::civil::Date| d.to_zoned(tz.clone()).unwrap().timestamp().as_second();
    Range::new(epoch(date), epoch(date.tomorrow().unwrap()))
}

/// Local-time range covering month `m` (1-12) of `year`.
pub(crate) fn month_range(year: i16, m: i8) -> Range {
    let tz = jiff::tz::TimeZone::system();
    let start = jiff::civil::date(year, m, 1);
    let end = start.checked_add(jiff::Span::new().months(1)).unwrap();
    let epoch = |d: jiff::civil::Date| d.to_zoned(tz.clone()).unwrap().timestamp().as_second();
    Range::new(epoch(start), epoch(end))
}
