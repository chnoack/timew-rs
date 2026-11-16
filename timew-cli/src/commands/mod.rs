//! The commands. Each takes the [`Session`] and returns the exit status;
//! output goes to `Session::out`.

pub mod annotate;
pub mod cancel;
pub mod chart;
pub mod config;
pub mod continue_;
pub mod default;
pub mod delete;
pub mod diagnostics;
pub mod export;
pub mod extensions;
pub mod fill;
pub mod gaps;
pub mod get;
pub mod help;
pub mod import;
pub mod info;
pub mod join;
pub mod lengthen;
pub mod modify;
pub mod move_;
pub mod report;
pub mod split;
pub mod start;
pub mod stop;
pub mod summary;
pub mod tag;
pub mod tags;
pub mod track;

use crate::color::Color;
use crate::datetime::{self, format_local_iso, local};
use crate::duration::format_hours;
use crate::interval::Interval;
use crate::range::{Epoch, Range};
use crate::rules::Rules;
use crate::session::Session;
use crate::text::quote_if_needed;

/// Run the command named on the command line.
pub fn dispatch(s: &mut Session) -> Result<i32, String> {
    let Some(command) = s.cli.command().map(str::to_string) else {
        if let Some(word) = s.cli.words().first() {
            return Err(format!("'{word}' is not a timew command. See 'timew help'."));
        }
        return default::run(s);
    };
    match command.as_str() {
        "annotate" => annotate::run(s),
        "cancel" => cancel::run(s),
        "config" => config::run(s),
        "continue" => continue_::run(s),
        "day" | "week" | "month" => chart::run(s, &command),
        "delete" => delete::run(s),
        "diagnostics" => diagnostics::run(s),
        "export" => export::run(s),
        "extensions" => extensions::run(s),
        "fill" => fill::run(s),
        "gaps" => gaps::run(s),
        "get" => get::run(s),
        "help" | "--help" | "-h" => help::run(s),
        "import" => import::run(s),
        "info" => info::run(s),
        "join" => join::run(s),
        "lengthen" => lengthen::run(s, lengthen::Change::Lengthen),
        "shorten" => lengthen::run(s, lengthen::Change::Shorten),
        "resize" => lengthen::run(s, lengthen::Change::Resize),
        "modify" => modify::run(s),
        "move" => move_::run(s),
        "report" => report::run(s),
        "retag" => tag::run(s, tag::Change::Retag),
        "show" => config::show(s),
        "split" => split::run(s),
        "start" => start::run(s),
        "stop" => stop::run(s),
        "summary" => summary::run(s),
        "tag" => tag::run(s, tag::Change::Tag),
        "tags" => tags::run(s),
        "track" => track::run(s),
        "untag" => tag::run(s, tag::Change::Untag),
        _ => report::run(s),
    }
}

/// Color of `tags.<tag>.color`, if colors are on.
pub fn tag_color(rules: &Rules, tag: &str) -> Color {
    if !rules.get_bool("color", false) {
        return Color::none();
    }
    rules.get(&format!("tags.{tag}.color")).map(Color::parse_or_none).unwrap_or_default()
}

/// The blend of the tag colors of an interval.
pub fn tags_color(rules: &Rules, tags: &[String]) -> Color {
    let mut c = Color::none();
    for tag in tags {
        c.blend(&tag_color(rules, tag));
    }
    c
}

/// `right` in local ISO form, with the leading parts it shares with
/// `left` (same year, month, day, hour, minute) blanked out.
pub fn minimal_delta(left: Epoch, right: Epoch) -> String {
    let mut s: Vec<char> = format_local_iso(right).chars().collect();
    let (l, r) = (local(left), local(right));
    let mut blank = |from: usize, len: usize| {
        for c in s.iter_mut().skip(from).take(len) {
            *c = ' ';
        }
    };
    if l.year() == r.year() {
        blank(0, 5);
        if l.month() == r.month() {
            blank(5, 3);
            if l.day() == r.day() {
                blank(8, 3);
                if l.hour() == r.hour() {
                    blank(11, 3);
                    if l.minute() == r.minute() {
                        blank(14, 3);
                    }
                }
            }
        }
    }
    s.into_iter().collect()
}

/// The feedback after starting, stopping or recording an interval.
pub fn summarize(rules: &Rules, interval: &Interval, now: Epoch) -> String {
    if !interval.is_started() {
        return String::new();
    }
    let tags: Vec<String> =
        interval.tags.iter().map(|t| tag_color(rules, t).colorize(&quote_if_needed(t))).collect();
    let tags = tags.join(" ");
    let start = format_local_iso(interval.range.start);
    let total = format!("{:>19}", format_hours(interval.range.total(now)));
    if interval.is_open() {
        if interval.range.start <= now {
            format!(
                "Tracking {tags}\n  Started {start}\n  Current {}\n  Total   {total}\n",
                minimal_delta(interval.range.start, now)
            )
        } else {
            format!("Tracking {tags}\n  Starting {start}\n")
        }
    } else {
        format!(
            "Recorded {tags}\n  Started {start}\n  Ended   {}\n  Total   {total}\n",
            minimal_delta(interval.range.start, interval.range.end)
        )
    }
}

/// "No filtered data found in the range … tagged with …."
pub fn no_data_message(range: &Range, tags: &[String]) -> String {
    let mut msg = String::from("No filtered data found");
    if range.is_started() {
        msg.push_str(&format!(" in the range {}", format_local_iso(range.start)));
        if range.is_ended() {
            msg.push_str(&format!(" - {}", format_local_iso(range.end)));
        }
    }
    if !tags.is_empty() {
        msg.push_str(&format!(" tagged with {}", crate::text::join_quoted(", ", tags)));
    }
    msg.push_str(".\n");
    msg
}

/// The default range of a report: `reports.<name>.range`, else
/// `reports.range`, else `fallback` (a hint name without the colon).
pub fn default_report_range(s: &Session, name: &str, fallback: &str) -> Range {
    let general = s.rules.get_or("reports.range", fallback);
    let hint = s.rules.get_or(&format!("reports.{name}.range"), &general);
    datetime::expand_range_hint_at(&hint, s.now).unwrap_or_default()
}

/// The intervals selected by IDs, or by range and tags, for a report.
/// Returns the range and the intervals, oldest first.
pub fn select_for_report(
    s: &mut Session,
    default_range: Range,
) -> Result<(Range, Vec<crate::database::Tracked>), String> {
    let ids = s.cli.ids();
    let tags = s.cli.tags();
    if !ids.is_empty() && !tags.is_empty() {
        return Err("You cannot filter intervals by both, ids and tags.".to_string());
    }
    if !ids.is_empty() {
        let tracked = s.tracked_ids(&ids)?;
        let range = Range::new(
            tracked.first().map(|t| t.interval.range.start).unwrap_or(0),
            tracked.last().map(|t| t.interval.range.end).unwrap_or(0),
        );
        return Ok((range, tracked));
    }
    let range = s.cli.range(default_range, s.now)?;
    let tracked = s.tracked_in(&range, &tags)?;
    Ok((range, tracked))
}

/// The intervals named by ID, or the running one if no IDs are given.
/// `what` is the command name, for the error message.
pub fn ids_or_active(s: &mut Session, what: &str) -> Result<Vec<crate::database::Tracked>, String> {
    let ids = s.cli.ids();
    if !ids.is_empty() {
        return s.tracked_ids(&ids);
    }
    match s.first()? {
        None => Err("There is no active time tracking.".to_string()),
        Some(t) if !t.interval.is_open() => Err(format!("At least one ID must be specified. See 'timew help {what}'.")),
        Some(t) => Ok(vec![t]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datetime::make_local;

    #[test]
    fn minimal_delta_blanks_shared_parts() {
        let a = make_local(2025, 3, 5, 9, 0, 0);
        assert_eq!(minimal_delta(a, make_local(2025, 3, 5, 10, 30, 0)), "           10:30:00");
        assert_eq!(minimal_delta(a, make_local(2025, 3, 5, 9, 0, 30)), "                 30");
        assert_eq!(minimal_delta(a, make_local(2025, 4, 1, 9, 0, 0)), "     04-01T09:00:00");
        assert_eq!(minimal_delta(a, make_local(2026, 3, 5, 9, 0, 0)), "2026-03-05T09:00:00");
    }
}
