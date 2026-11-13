//! `timew move @<id> <date>`: move an interval to start at `<date>`,
//! keeping its length.

use crate::session::Session;
use crate::validate::auto_adjust;

pub fn run(s: &mut Session) -> Result<i32, String> {
    let ids = s.cli.ids();
    if ids.len() > 1 {
        return Err("The 'move' command only supports a single ID.".to_string());
    }
    let Some(&id) = ids.iter().next() else {
        return Err("ID must be specified. See 'timew help move'.".to_string());
    };
    s.journal.start();
    s.flatten_latest()?;
    let original = s.tracked_ids(&ids)?.remove(0).interval;
    let Some(date) = s.cli.dates().pop() else {
        return Err("A new start time must be specified. See 'timew help move'.".to_string());
    };
    let start = crate::datetime::parse_datetime(&date)?;
    let mut moved = original.clone();
    let delta = start - original.range.start;
    moved.range.start = start;
    if !moved.is_open() {
        moved.range.end += delta;
    }
    s.delete(&original)?;
    auto_adjust(s, s.cli.has_hint("adjust"), &moved)?;
    s.add(&moved)?;
    s.journal.end()?;
    if s.verbose() {
        s.out.push_str(&format!("Moved @{id} to {}\n", crate::datetime::format_local_iso(moved.range.start)));
    }
    Ok(0)
}
