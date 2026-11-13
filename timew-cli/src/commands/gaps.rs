//! `timew gaps [<range>] [<tag> ...]`: the untracked time in the range
//! (default `:day`), leaving out excluded time. With `:blank`, all
//! non-excluded time is shown.

use crate::color::Color;
use crate::datetime::{self, add_days, start_of_day};
use crate::duration::format_hours;
use crate::range::{subtract_all, Range};
use crate::session::Session;
use crate::table::{terminal_width, Table};

pub fn run(s: &mut Session) -> Result<i32, String> {
    let default = super::default_report_range(s, "gaps", "day");
    let range = s.cli.range(default, s.now)?;
    let tags = s.cli.tags();
    let gaps = if s.cli.has_hint("blank") {
        subtract_all(&[range], &s.exclusions(&range)?)
    } else {
        s.untracked(&range, &tags)?
    };
    if gaps.is_empty() {
        if s.verbose() {
            s.out.push_str("No gaps found.\n");
        }
        return Ok(0);
    }

    let mut table = Table::new(terminal_width(), s.color());
    for (name, left) in [("Wk", true), ("Date", true), ("Day", true), ("Start", false), ("End", false), ("Time", false), ("Total", false)] {
        table.add_column(name, left);
    }
    let mut grand_total = 0;
    if range.is_started() && range.is_ended() {
        let mut day = start_of_day(range.start);
        while day < range.end {
            let day_range = Range::new(day, add_days(day, 1));
            let mut daily = 0;
            let mut last_row = None;
            for gap in gaps.iter().filter(|g| day_range.intersects(g)) {
                let row = table.add_row();
                if last_row.is_none() {
                    table.set(row, 0, format!("W{}", datetime::iso_week(day)));
                    table.set(row, 1, datetime::format_ymd(day));
                    table.set(row, 2, datetime::day_name_short(day));
                }
                let mut part = day_range.intersect(gap);
                if gap.is_open() {
                    part.end = s.now;
                }
                let total = part.total(s.now);
                table.set(row, 3, datetime::format_time(part.start));
                table.set(row, 4, if gap.is_open() { "-".to_string() } else { datetime::format_time(part.end) });
                table.set(row, 5, format_hours(total));
                daily += total;
                last_row = Some(row);
            }
            if let Some(row) = last_row {
                table.set(row, 6, format_hours(daily));
            }
            grand_total += daily;
            day = add_days(day, 1);
        }
    }
    let row = table.add_row();
    table.set_colored(row, 6, " ", Color::underline());
    let row = table.add_row();
    table.set(row, 6, format_hours(grand_total));
    s.out.push('\n');
    s.out.push_str(&table.render());
    s.out.push('\n');
    Ok(0)
}
