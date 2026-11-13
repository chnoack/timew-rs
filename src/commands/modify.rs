//! `timew modify start|end @<id> <date>` and
//! `timew modify range @<id> <range>`: change when an interval starts or
//! ends. With `:fill`, the new time is the end of the previous (or start of
//! the next) interval.

use crate::range::Range;
use crate::session::Session;
use crate::validate::{auto_adjust, fill, fill_end, fill_start};

pub fn run(s: &mut Session) -> Result<i32, String> {
    let Some(what) = s.cli.take_subcommand(&["start", "end", "range"]) else {
        return Err("Must specify start|end|range command to modify. See 'timew help modify'.".to_string());
    };
    let ids = s.cli.ids();
    if ids.is_empty() {
        return Err("ID must be specified. See 'timew help modify'.".to_string());
    }
    if ids.len() > 1 {
        return Err("Only one ID may be specified. See 'timew help modify'.".to_string());
    }
    let id = *ids.iter().next().unwrap();
    let range = s.cli.range(Range::empty(), s.now)?;
    let do_fill = s.cli.has_hint("fill");

    s.journal.start();
    s.flatten_latest()?;
    let interval = s.tracked_ids(&ids)?.remove(0).interval;
    if !range.is_started() && !do_fill {
        return Err("No updated time specified. See 'timew help modify'.".to_string());
    }
    let mut modified = interval.clone();
    match what.as_str() {
        "start" => {
            if do_fill {
                fill_start(s, &mut modified, id)?;
            } else {
                modified.range.start = range.start;
            }
        }
        "end" => {
            if interval.is_open() {
                return Err(format!("Cannot modify end of open interval @{id}."));
            }
            if do_fill {
                fill_end(s, &mut modified, id)?;
            } else {
                modified.range.end = range.start;
            }
        }
        _ => {
            if do_fill {
                fill(s, &mut modified, id)?;
            } else {
                modified.range = range;
            }
        }
    }
    if !modified.is_open() && modified.range.start > modified.range.end {
        return Err(format!("Cannot modify interval @{id} where start is after end."));
    }
    s.delete(&interval)?;
    auto_adjust(s, s.cli.has_hint("adjust"), &modified)?;
    s.add(&modified)?;
    s.journal.end()?;
    Ok(0)
}
