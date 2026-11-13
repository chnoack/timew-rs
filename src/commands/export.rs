//! `timew export [<range>] [<tag> ...] | [@<id> ...]`: the intervals as a
//! JSON array, one object per line, oldest first.

use crate::database::Tracked;
use crate::range::Range;
use crate::session::Session;

pub fn run(s: &mut Session) -> Result<i32, String> {
    let ids = s.cli.ids();
    let tags = s.cli.tags();
    if !ids.is_empty() && !tags.is_empty() {
        return Err("You cannot specify both id and tags/range to export intervals.".to_string());
    }
    let tracked = if ids.is_empty() {
        let range = s.cli.range(Range::empty(), s.now)?;
        s.tracked_in(&range, &tags)?
    } else {
        s.tracked_ids(&ids)?
    };
    s.out.push_str(&json_array(&tracked));
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
