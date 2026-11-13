//! Untrackable time from the configuration: holidays and exclusions.
//!
//! ```text
//! holidays.en-US.2025_12_25 = Christmas      a holiday (whole day)
//! exclusions.monday = <8:00 12:00-12:45 >17:30
//! exclusions.days.2025_12_24 = off           an extra day off
//! exclusions.days.2025_12_27 = on            a working day despite a holiday
//! ```
//!
//! A time block is `<HH:MM` (from midnight), `>HH:MM` (until midnight) or
//! `HH:MM-HH:MM`.

use crate::datetime::{self, day_of_week, local, make_local, weekday_sunday0};
use crate::range::{merge, subtract_all, Epoch, Range};
use crate::rules::Rules;

/// A configured holiday.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holiday {
    /// Local midnight at the start of the day.
    pub day: Epoch,
    /// The locale part of the key, e.g. `en-US`.
    pub locale: String,
    pub name: String,
}

/// Parse `YYYY_MM_DD` (or with `-`) as local midnight.
fn parse_day(s: &str) -> Option<Epoch> {
    let parts: Vec<&str> = s.split(['_', '-']).collect();
    if parts.len() != 3 {
        return None;
    }
    let y: i64 = parts[0].parse().ok()?;
    let m: i64 = parts[1].parse().ok()?;
    let d: i64 = parts[2].parse().ok()?;
    if !(1..=12).contains(&m) || d < 1 || d > datetime::days_in_month(y, m) {
        return None;
    }
    Some(make_local(y, m, d, 0, 0, 0))
}

fn whole_day(day: Epoch) -> Range {
    Range::new(day, datetime::add_days(day, 1))
}

/// All holidays in the configuration, sorted by day.
pub fn holidays(rules: &Rules) -> Vec<Holiday> {
    let mut out: Vec<Holiday> = rules
        .keys("holidays.")
        .into_iter()
        .filter_map(|key| {
            let rest = &key["holidays.".len()..];
            let (locale, date) = rest.rsplit_once('.').unwrap_or(("", rest));
            Some(Holiday { day: parse_day(date)?, locale: locale.to_string(), name: rules.get(&key)?.to_string() })
        })
        .collect();
    out.sort_by(|a, b| a.day.cmp(&b.day).then(a.locale.cmp(&b.locale)));
    out
}

/// Holidays falling within `range` (inclusive of the end day), one line
/// each, as shown below the summary and charts.
pub fn render_holidays(rules: &Rules, range: &Range) -> String {
    let mut out = String::new();
    let mut seen = std::collections::BTreeMap::new();
    for h in holidays(rules) {
        if h.day >= range.start && (!range.is_ended() || h.day <= range.end) {
            seen.insert(h.day, format!(" [{}] {}", h.locale, h.name));
        }
    }
    for (day, text) in seen {
        out.push_str(&format!("{} {}\n", datetime::format_ymd(day), text));
    }
    out
}

/// A time block of a weekday exclusion, on the day starting at `day`.
fn time_block(block: &str, day: Epoch) -> Result<Range, String> {
    let malformed = || format!("Malformed time block '{block}'.");
    let hms = |s: &str| -> Option<(i64, i64, i64)> {
        let mut parts = s.split(':');
        let h = parts.next()?;
        let m = parts.next()?;
        let sec = parts.next().unwrap_or("0");
        if parts.next().is_some() || h.is_empty() || h.len() > 2 || m.len() != 2 {
            return None;
        }
        let (h, m, sec): (i64, i64, i64) = (h.parse().ok()?, m.parse().ok()?, sec.parse().ok()?);
        (h <= 24 && m < 60 && sec < 60).then_some((h, m, sec))
    };
    let z = local(day);
    let (y, mo, d) = (z.year() as i64, z.month() as i64, z.day() as i64);
    let at = |(h, m, s): (i64, i64, i64)| make_local(y, mo, d, h, m, s);
    if let Some(t) = block.strip_prefix('<') {
        return Ok(Range::new(day, at(hms(t).ok_or_else(malformed)?)));
    }
    if let Some(t) = block.strip_prefix('>') {
        return Ok(Range::new(at(hms(t).ok_or_else(malformed)?), datetime::add_days(day, 1)));
    }
    let (from, to) = block.split_once('-').ok_or_else(malformed)?;
    Ok(Range::new(at(hms(from).ok_or_else(malformed)?), at(hms(to).ok_or_else(malformed)?)))
}

/// Every excluded (untrackable) range overlapping `range`: holidays, days
/// off, and the weekday time blocks, merged.
pub fn all_exclusions(rules: &Rules, range: &Range) -> Result<Vec<Range>, String> {
    let mut days_off: Vec<Range> = holidays(rules).iter().map(|h| whole_day(h.day)).collect();
    let mut days_on = Vec::new();
    let mut weekly: Vec<(usize, Vec<String>)> = Vec::new();

    for key in rules.keys("exclusions.") {
        let value = rules.get(&key).unwrap_or("");
        let name = key.to_ascii_lowercase();
        let parts: Vec<&str> = name.split('.').collect();
        let bad = || format!("Unrecognized exclusion syntax: '{key}' '{value}'.");
        if parts.len() == 3 && parts[1] == "days" {
            let day = parse_day(parts[2]).ok_or_else(bad)?;
            match value.trim() {
                "on" => days_on.push(whole_day(day)),
                "off" => days_off.push(whole_day(day)),
                _ => return Err(bad()),
            }
        } else if parts.len() == 2
            && let Some(wd) = day_of_week(parts[1])
        {
            weekly.push((wd, value.split_whitespace().map(str::to_string).collect()));
        } else {
            return Err(bad());
        }
    }

    days_off.retain(|r| range.overlaps(r));
    let mut result = subtract_all(&days_off, &days_on);

    if range.is_started() {
        let end = if range.is_ended() { range.end } else { datetime::now() };
        let mut day = datetime::start_of_day(range.start);
        while day <= end {
            let wd = weekday_sunday0(local(day).weekday()) as usize;
            for (weekday, blocks) in &weekly {
                if *weekday == wd {
                    for block in blocks {
                        let r = time_block(block, day)?;
                        let limit = Range::new(range.start, end);
                        if !r.is_empty() && limit.overlaps(&r) {
                            result.push(r);
                        }
                    }
                }
            }
            day = datetime::add_days(day, 1);
        }
    }
    Ok(merge(&result))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(d: i64, h: i64, m: i64) -> Epoch {
        make_local(2025, 3, d, h, m, 0)
    }

    #[test]
    fn weekday_blocks() {
        let mut rules = Rules::default();
        rules.set("exclusions.monday", "<8:00 12:00-12:45 >17:30");
        // 2025-03-03 is a Monday.
        let r = all_exclusions(&rules, &Range::new(at(3, 0, 0), at(4, 0, 0))).unwrap();
        assert_eq!(r, vec![Range::new(at(3, 0, 0), at(3, 8, 0)), Range::new(at(3, 12, 0), at(3, 12, 45)), Range::new(at(3, 17, 30), at(4, 0, 0))]);
        let none = all_exclusions(&rules, &Range::new(at(4, 0, 0), at(5, 0, 0))).unwrap();
        assert!(none.is_empty());
    }

    #[test]
    fn holidays_and_days() {
        let mut rules = Rules::default();
        rules.set("holidays.eo.2025_03_04", "Testtag");
        rules.set("holidays.eo.2025_03_05", "Workday");
        rules.set("exclusions.days.2025_03_05", "on");
        rules.set("exclusions.days.2025_03_06", "off");
        let r = all_exclusions(&rules, &Range::new(at(1, 0, 0), at(10, 0, 0))).unwrap();
        assert_eq!(r, vec![Range::new(at(4, 0, 0), at(5, 0, 0)), Range::new(at(6, 0, 0), at(7, 0, 0))]);
        rules.set("exclusions.bogus", "x");
        assert!(all_exclusions(&rules, &Range::new(at(1, 0, 0), at(10, 0, 0))).is_err());
    }
}
