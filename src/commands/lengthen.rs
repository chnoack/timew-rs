//! `timew lengthen|shorten @<id> ... <duration>`: move the end of closed
//! intervals later or earlier. `timew resize @<id> ... <duration>`: set
//! their length.

use crate::duration::format_hours;
use crate::session::Session;
use crate::validate::auto_adjust;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Lengthen,
    Shorten,
    Resize,
}

pub fn run(s: &mut Session, change: Change) -> Result<i32, String> {
    let name = match change {
        Change::Lengthen => "lengthen",
        Change::Shorten => "shorten",
        Change::Resize => "resize",
    };
    let ids = s.cli.ids();
    if ids.is_empty() {
        return Err(format!("IDs must be specified. See 'timew help {name}'."));
    }
    let Some(duration) = s.cli.duration() else {
        return Err(format!("A duration must be specified. See 'timew help {name}'."));
    };
    let adjust = s.cli.has_hint("adjust");

    s.journal.start();
    s.flatten_latest()?;
    for t in s.tracked_ids(&ids)? {
        if t.interval.is_open() {
            let verb = match change {
                Change::Lengthen => "lengthen",
                Change::Shorten => "shorten",
                Change::Resize => "resize",
            };
            return Err(format!("Cannot {verb} open interval @{}", t.id));
        }
        let length = t.interval.range.end - t.interval.range.start;
        if change == Change::Shorten && duration > length {
            return Err(format!(
                "Cannot shorten interval @{} by {} because it is only {} in length.",
                t.id,
                format_hours(duration),
                format_hours(length)
            ));
        }
        let mut modified = t.interval.clone();
        modified.range.end = match change {
            Change::Lengthen => t.interval.range.end + duration,
            Change::Shorten => t.interval.range.end - duration,
            Change::Resize => t.interval.range.start + duration,
        };
        s.delete(&t.interval)?;
        auto_adjust(s, adjust, &modified)?;
        s.add(&modified)?;
        if s.verbose() {
            let line = match change {
                Change::Lengthen => format!("Lengthened @{} by {}\n", t.id, format_hours(duration)),
                Change::Shorten => format!("Shortened @{} by {}\n", t.id, format_hours(duration)),
                Change::Resize => format!("Resized @{} to {}\n", t.id, format_hours(duration)),
            };
            s.out.push_str(&line);
        }
    }
    s.journal.end()?;
    Ok(0)
}
