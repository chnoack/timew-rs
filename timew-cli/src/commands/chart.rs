//! `timew day|week|month [<range>] [<tag> ...]`: a chart with one line per
//! day and one cell per hour, showing tracked intervals as colored blocks
//! labeled with their tags, and excluded time in gray.
//!
//! Settings (`reports.<day|week|month>.*`): `range`, `hours` (`all` or
//! `auto`), `cell` (minutes per character), `lines` (lines per day),
//! `spacing`, `axis` (`internal` puts the hour labels into the chart),
//! `month`, `week`, `weekday`, `day` (labels), `totals`, `summary`,
//! `holidays`. Hints: `:ids` shows IDs in the labels, `:blank` hides the
//! intervals.

use std::collections::BTreeMap;

use crate::color::{Color, Palette};
use crate::composite::Composite;
use crate::database::Tracked;
use crate::datetime::{self, add_days, local, make_local, same_day, start_of_day};
use crate::duration::format_hours;
use crate::exclusion;
use crate::range::{Epoch, Range};
use crate::rules::Rules;
use crate::session::Session;
use crate::text::{display_width, left_justify, wrap_text};

struct Chart {
    now: Epoch,
    month: bool,
    week: bool,
    weekday: bool,
    day: bool,
    ids: bool,
    summary: bool,
    holidays: bool,
    totals: bool,
    internal_axis: bool,
    blank: bool,
    auto_hours: bool,
    minutes_per_char: i64,
    spacing: i64,
    lines: usize,
    color: bool,
    color_today: Color,
    color_holiday: Color,
    color_label: Color,
    color_exclusion: Color,
    tag_colors: BTreeMap<String, Color>,
    cell_width: i64,
}

pub fn run(s: &mut Session, kind: &str) -> Result<i32, String> {
    let default = super::default_report_range(s, kind, kind);
    let ids = s.cli.ids();
    let tags = s.cli.tags();
    if !ids.is_empty() && !tags.is_empty() {
        return Err("You cannot filter intervals by both, ids and tags.".to_string());
    }
    let range = s.cli.range(default, s.now)?;
    let tracked = if ids.is_empty() { s.tracked_in(&range, &tags)? } else { s.tracked_ids(&ids)? };
    if tracked.is_empty() {
        if s.verbose() {
            s.out.push_str(&super::no_data_message(&range, &tags));
        }
        return Ok(0);
    }

    let key = |name: &str| format!("reports.{kind}.{name}");
    let rules = &s.rules;
    let minutes_per_char = rules.get_int(&key("cell"), 15)?;
    if minutes_per_char < 1 {
        return Err(format!("The value for 'reports.{kind}.cell' must be at least 1."));
    }
    let lines = rules.get_int(&key("lines"), 1)?;
    if lines < 1 {
        return Err(format!("Invalid value for 'reports.{kind}.lines': '{lines}'"));
    }
    let spacing = rules.get_int(&key("spacing"), 1)?;
    let color = s.color();
    let theme = |name: &str| {
        if color { Color::parse_or_none(rules.get(&format!("theme.colors.{name}")).unwrap_or("")) } else { Color::none() }
    };
    let chart = Chart {
        now: s.now,
        month: rules.get_bool(&key("month"), false),
        week: rules.get_bool(&key("week"), false),
        weekday: rules.get_bool(&key("weekday"), false),
        day: rules.get_bool(&key("day"), false),
        ids: s.cli.has_hint("ids"),
        summary: rules.get_bool(&key("summary"), false),
        holidays: rules.get_bool(&key("holidays"), false),
        totals: rules.get_bool(&key("totals"), false),
        internal_axis: rules.get_or(&key("axis"), "") == "internal",
        blank: s.cli.has_hint("blank"),
        auto_hours: rules.get_or(&key("hours"), "") == "auto",
        minutes_per_char,
        spacing,
        lines: lines as usize,
        color,
        color_today: theme("today"),
        color_holiday: theme("holiday"),
        color_label: theme("label"),
        color_exclusion: theme("exclusion"),
        tag_colors: tag_colors(rules, color, &tracked),
        cell_width: 60 / minutes_per_char + spacing,
    };
    let exclusions = s.exclusions(&range)?;
    let holiday_text = exclusion::render_holidays(rules, &range);
    let holiday_days: Vec<Epoch> = exclusion::holidays(rules).iter().map(|h| h.day).collect();
    s.out.push_str(&chart.render(&range, &tracked, &exclusions, &holiday_days, &holiday_text));
    Ok(0)
}

/// A color per tag: `tags.<tag>.color`, else the next palette color
/// (`theme.palette.color*`, or the built-in palette). The empty tag stands
/// for intervals without tags.
fn tag_colors(rules: &Rules, enabled: bool, tracked: &[Tracked]) -> BTreeMap<String, Color> {
    let custom: Vec<Color> = rules.keys("theme.palette.color").iter().filter_map(|k| rules.get(k)).map(Color::parse_or_none).collect();
    let mut palette = if custom.is_empty() { Palette::new(enabled) } else { Palette::with_colors(custom, enabled) };
    let mut map = BTreeMap::new();
    map.insert(String::new(), palette.next_color());
    for t in tracked {
        for tag in &t.interval.tags {
            if let Some(c) = rules.get(&format!("tags.{tag}.color")) {
                map.insert(tag.clone(), if enabled { Color::parse_or_none(c) } else { Color::none() });
            } else if !map.contains_key(tag) {
                map.insert(tag.clone(), palette.next_color());
            }
        }
    }
    map
}

/// Round minutes to the nearest multiple of `n`.
fn quantize(minutes: i64, n: i64) -> i64 {
    let deviation = minutes.rem_euclid(n);
    if deviation == 0 {
        minutes
    } else if deviation < n / 2 {
        minutes - deviation
    } else {
        minutes + n - deviation
    }
}

/// `interval` limited to `range`.
fn clip(interval: Range, range: &Range) -> Range {
    if !range.is_started() || range.is_empty() {
        return interval;
    }
    let mut r = interval;
    if r.start != 0 && r.start < range.start {
        r.start = range.start;
    }
    if r.end != 0 && range.is_ended() && r.end > range.end {
        r.end = range.end;
    }
    r
}

impl Chart {
    fn indent(&self) -> usize {
        (if self.month { 4 } else { 0 })
            + (if self.week { 4 } else { 0 })
            + (if self.day { 3 } else { 0 })
            + (if self.weekday { 4 } else { 0 })
    }

    fn render(&self, range: &Range, tracked: &[Tracked], exclusions: &[Range], holidays: &[Epoch], holiday_text: &str) -> String {
        let (first_hour, last_hour) = if self.auto_hours { self.hour_range(range, tracked) } else { (0, 23) };
        let indent = " ".repeat(self.indent());
        let total_width = (last_hour - first_hour + 1) * self.cell_width;

        let mut out = String::from("\n");
        if !self.internal_axis {
            out.push_str(&indent);
            out.push_str(&self.axis(first_hour, last_hour));
        }

        let range_start = if range.is_started() { range.start } else { tracked.first().unwrap().interval.range.start };
        let mut range_end = if range.is_ended() { range.end } else { tracked.last().unwrap().interval.range.end };
        if range_end == 0 {
            range_end = self.now;
        }

        let mut total_work = 0;
        let mut previous: Option<Epoch> = None;
        let mut day = start_of_day(range_start);
        while day < range_end {
            let mut lines: Vec<Composite> = (0..self.lines).map(|_| Composite::default()).collect();
            for line in &mut lines {
                line.add(" ".repeat(total_width as usize), 0, Color::none());
            }
            self.exclusion_blocks(&mut lines, day, first_hour, last_hour, exclusions);
            let mut work = 0;
            if !self.blank {
                for t in tracked {
                    let seconds = self.interval(&mut lines, day, t, first_hour);
                    // drawn, but an `_tag` entry doesn't count towards the totals
                    if t.interval.counts_in_totals() {
                        work += seconds;
                    }
                }
            }

            let day_color = if same_day(day, self.now) {
                self.color_today
            } else if holidays.iter().any(|h| same_day(*h, day)) {
                self.color_holiday
            } else {
                Color::none()
            };
            let z = local(day);
            if self.month {
                let show = previous.is_none_or(|p| local(p).month() != z.month());
                out.push_str(&if show { datetime::month_name_short(z.month() as i64) } else { "   ".to_string() });
                out.push(' ');
            }
            if self.week {
                let show = previous.is_none_or(|p| datetime::iso_week(p) != datetime::iso_week(day));
                out.push_str(&if show { left_justify(&format!("W{}", datetime::iso_week(day)), 3) } else { "   ".to_string() });
                out.push(' ');
            }
            if self.weekday {
                out.push_str(&day_color.colorize(&datetime::day_name_short(day)));
                out.push(' ');
            }
            if self.day {
                out.push_str(&day_color.colorize(&format!("{:>2}", z.day())));
                out.push(' ');
            }
            for (i, line) in lines.iter().enumerate() {
                if i > 0 {
                    out.push('\n');
                    out.push_str(&indent);
                }
                out.push_str(&line.render());
            }
            if self.totals && work > 0 {
                out.push_str(&format!(" {:>3}:{:02}", work / 3600, (work % 3600) / 60));
            }
            out.push('\n');
            previous = Some(day);
            total_work += work;
            day = add_days(day, 1);
        }

        if self.totals {
            let padding = " ".repeat(self.indent() + total_width as usize + 1);
            let underline = if self.color { Color::underline() } else { Color::none() };
            out.push_str(&format!(
                "{padding}{}\n{padding}{:>3}:{:02}\n",
                underline.colorize("      "),
                total_work / 3600,
                (total_work % 3600) / 60
            ));
        }
        if self.holidays {
            out.push_str(holiday_text);
        }
        if self.summary {
            out.push_str(&self.summary_text(&indent, &Range::new(range_start, range_end), exclusions, tracked));
        }
        out
    }

    /// From the hour before the earliest tracked time to the hour of the
    /// latest.
    fn hour_range(&self, range: &Range, tracked: &[Tracked]) -> (i64, i64) {
        let (mut first, mut last) = (23, 0);
        if range.is_started() && range.is_ended() {
            let mut day = start_of_day(range.start);
            while day < range.end {
                let day_range = Range::new(day, add_days(day, 1));
                for t in tracked {
                    let mut r = t.interval.range;
                    if r.end == 0 {
                        r.end = self.now;
                    }
                    if day_range.overlaps(&r) {
                        let c = clip(r, &day_range);
                        first = first.min(local(c.start).hour() as i64);
                        if c.end != 0 {
                            last = last.max(local(c.end).hour() as i64);
                        }
                    }
                }
                day = add_days(day, 1);
            }
        }
        if first == 23 && last == 0 {
            let h = local(self.now).hour() as i64;
            (h, (h + 1).min(23))
        } else {
            ((first - 1).max(0), last.min(23))
        }
    }

    fn axis(&self, first: i64, last: i64) -> String {
        let now_hour = local(self.now).hour() as i64;
        let mut out = String::new();
        for hour in first..=last {
            let color = if hour == now_hour { self.color_today } else { self.color_label };
            out.push_str(&color.colorize(&left_justify(&hour.to_string(), self.cell_width as usize)));
        }
        if self.totals {
            out.push_str("  ");
            out.push_str(&self.color_label.colorize("Total"));
        }
        out.push('\n');
        out
    }

    fn exclusion_blocks(&self, lines: &mut [Composite], day: Epoch, first: i64, last: i64, exclusions: &[Range]) {
        let z = local(day);
        let (y, m, d) = (z.year() as i64, z.month() as i64, z.day() as i64);
        for hour in first..=last {
            let hour_range = Range::new(make_local(y, m, d, hour, 0, 0), make_local(y, m, d, hour + 1, 0, 0));
            let label = hour.to_string();
            let offset = ((hour - first) * self.cell_width) as usize;
            if self.internal_axis {
                lines[0].add(label.clone(), offset, self.color_label);
            }
            for e in exclusions.iter().filter(|e| e.overlaps(&hour_range)) {
                let part = e.intersect(&hour_range);
                let minute = |t: Epoch| local(t).minute() as i64;
                let end_minute = if part.end >= hour_range.end || minute(part.end) == 0 { 60 } else { minute(part.end) };
                let start_block = quantize(minute(part.start), self.minutes_per_char) / self.minutes_per_char;
                let end_block = quantize(end_minute, self.minutes_per_char) / self.minutes_per_char;
                let width = (end_block - start_block).max(0) as usize;
                let at = offset + start_block as usize;
                for line in lines.iter_mut() {
                    line.add(" ".repeat(width), at, self.color_exclusion);
                }
                if self.internal_axis && start_block == 0 && width >= label.len() {
                    lines[0].add(label.clone(), at, self.color_exclusion);
                }
            }
        }
    }

    /// Draw `t` on `day`; returns the tracked seconds on that day.
    fn interval(&self, lines: &mut [Composite], day: Epoch, t: &Tracked, first_hour: i64) -> i64 {
        let iv = &t.interval;
        let day_range = Range::new(day, add_days(day, 1));
        if !day_range.overlaps(&iv.range) || (iv.is_open() && day > self.now) {
            return 0;
        }
        let mut clipped = clip(iv.range, &day_range);
        if iv.is_open() {
            clipped.end = if same_day(day, self.now) { self.now } else { day_range.end };
        }
        let minutes = |e: Epoch| {
            if e >= day_range.end {
                (24 - first_hour) * 60
            } else {
                let z = local(e);
                (z.hour() as i64 - first_hour) * 60 + z.minute() as i64
            }
        };
        let start_mins = minutes(clipped.start).max(0);
        let end_mins = minutes(clipped.end).max(0);
        let work = clipped.total(self.now);

        let mpc = self.minutes_per_char;
        let start_block = quantize(start_mins, mpc) / mpc;
        let end_block = quantize(if end_mins == start_mins { start_mins + 60 } else { end_mins }, mpc) / mpc;
        let start_offset = start_block + self.spacing * (start_mins / 60);
        let end_offset = end_block + self.spacing * (end_mins / 60);
        if end_offset > start_offset {
            let color = if iv.tags.is_empty() {
                self.tag_colors.get("").copied().unwrap_or_default()
            } else {
                let mut c = Color::none();
                for tag in &iv.tags {
                    c.blend(&self.tag_colors.get(tag).copied().unwrap_or_default());
                }
                c
            };
            let mut label = if self.ids { format!("@{}", t.id) } else { String::new() };
            for tag in &iv.tags {
                if !label.is_empty() {
                    label.push(' ');
                }
                label.push_str(tag);
            }
            let width = (end_offset - start_offset) as usize;
            let text: Vec<String> = if width >= display_width(&label) {
                vec![label.clone()]
            } else if label.is_ascii() {
                wrap_text(&label, width)
            } else {
                Vec::new()
            };
            let n = lines.len();
            for (i, line) in lines.iter_mut().enumerate() {
                let content = text.get(i).map(|l| left_justify(l, width)).unwrap_or_else(|| " ".repeat(width));
                line.add(content, start_offset as usize, color);
                if iv.is_open() && i == n - 1 {
                    line.add("+", start_offset as usize + width - 1, color);
                }
            }
        }
        work
    }

    fn summary_text(&self, indent: &str, range: &Range, exclusions: &[Range], tracked: &[Tracked]) -> String {
        let unavailable: i64 =
            exclusions.iter().filter(|e| range.overlaps(e)).map(|e| range.intersect(e).total(self.now)).sum();
        let worked: i64 = if self.blank {
            0
        } else {
            tracked
                .iter()
                .filter(|t| range.overlaps(&t.interval.range) && t.interval.counts_in_totals())
                .map(|t| {
                    let mut c = clip(t.interval.range, range);
                    if t.interval.is_open() {
                        c.end = self.now;
                    }
                    c.total(self.now)
                })
                .sum()
        };
        let available = range.total(self.now) - unavailable;
        let remaining = available - worked;
        let mut out = format!("\n{indent}Tracked   {:>13}\n", format_hours(worked));
        if remaining >= 0 {
            out.push_str(&format!("{indent}Available {:>13}\n", format_hours(remaining)));
        }
        out.push_str(&format!("{indent}Total     {:>13}\n\n", format_hours(available)));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantizing() {
        assert_eq!(quantize(0, 15), 0);
        assert_eq!(quantize(7, 15), 7 + 8);
        assert_eq!(quantize(6, 15), 0);
        assert_eq!(quantize(21, 15), 15);
        assert_eq!(quantize(22, 15), 30);
        assert_eq!(quantize(23, 15), 30);
    }
}
