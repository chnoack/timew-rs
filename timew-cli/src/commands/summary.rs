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
//! The rows themselves come from `timew_core::summary`, which `timew-tui`
//! uses too.

use crate::color::Color;
use crate::duration::format_hours;
use crate::session::Session;
use crate::summary::{build, Summary};
use crate::table::{terminal_width, Table};

pub fn run(s: &mut Session) -> Result<i32, String> {
    let default = super::default_report_range(s, "summary", "day");
    let (range, tracked) = super::select_for_report(s, default)?;
    if tracked.is_empty() {
        if s.verbose() {
            let text = super::no_data_message(&range, &s.cli.tags());
            s.out.push_str(&text);
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
        let text = crate::exclusion::render_holidays(&s.rules, &range);
        s.out.push_str(&text);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Tracked;
    use crate::datetime::make_local;
    use crate::range::{Epoch, Range};
    use crate::summary::build_at;
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
}
