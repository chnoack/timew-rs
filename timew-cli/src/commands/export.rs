//! `timew export [<range>] [<tag> ...] | [@<id> ...]`: the intervals as a
//! JSON array, one object per line, oldest first. With `:csv`, a table
//! as CSV instead: the columns of the `timew-tui` table plus the
//! annotation, one row per interval and day, separated by
//! `export.csv.separator` (a comma by default; see
//! [`timew_core::summary::to_csv`]).

use crate::database::Tracked;
use crate::range::Range;
use crate::session::Session;

pub fn run(s: &mut Session) -> Result<i32, String> {
    let ids = s.cli.ids();
    let tags = s.cli.tags();
    if !ids.is_empty() && !tags.is_empty() {
        return Err("You cannot specify both id and tags/range to export intervals.".to_string());
    }
    // Without a range (or with IDs) the CSV rows span the intervals found.
    let (range, tracked) = if ids.is_empty() {
        let range = s.cli.range(Range::empty(), s.now)?;
        let tracked = s.tracked_in(&range, &tags)?;
        (range, tracked)
    } else {
        (Range::empty(), s.tracked_ids(&ids)?)
    };
    if s.cli.has_hint("csv") {
        use crate::summary::{CSV_SEPARATOR_SETTING, build, csv_separator, to_csv};
        let separator = csv_separator(s.rules.get(CSV_SEPARATOR_SETTING))?;
        s.out.push_str(&to_csv(&build(&range, &tracked), separator));
    } else {
        s.out.push_str(&json_array(&tracked));
    }
    Ok(0)
}

pub fn json_array(tracked: &[Tracked]) -> String {
    let items: Vec<String> = tracked.iter().map(|t| t.interval.to_json(t.id)).collect();
    if items.is_empty() {
        "[\n]\n".to_string()
    } else {
        format!("[\n{}\n]\n", items.join(",\n"))
    }
}
