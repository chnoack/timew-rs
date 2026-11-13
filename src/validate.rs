//! Keeping intervals from overlapping, and the `:fill` hint.
//!
//! Before an interval is added, [`auto_adjust`] checks it against the
//! stored ones. Without `:adjust` any overlap is an error. With it, the new
//! interval wins: overlapped intervals are shortened, split or deleted.
//!
//! A new running interval that starts inside the current running one is
//! not an overlap: the current one is stopped where the new one starts
//! (unless both have the same tags, in which case nothing changes).

use crate::interval::Interval;
use crate::session::{flatten, Session};

/// Move `interval`'s start back to the end of the closest earlier
/// interval.
pub fn fill_start(s: &mut Session, interval: &mut Interval, id: u32) -> Result<(), String> {
    for earlier in s.tracked()? {
        if !earlier.interval.is_open() && earlier.interval.range.end <= interval.range.start {
            interval.range.start = earlier.interval.range.end;
            if s.verbose() {
                s.out.push_str(&format!(
                    "Backfilled {}to {}\n",
                    if id > 0 { format!("@{id} ") } else { String::new() },
                    crate::datetime::format_local_iso(interval.range.start)
                ));
            }
            break;
        }
    }
    Ok(())
}

/// Move a closed `interval`'s end forward to the start of the closest
/// later interval.
pub fn fill_end(s: &mut Session, interval: &mut Interval, id: u32) -> Result<(), String> {
    if interval.is_open() {
        return Ok(());
    }
    for later in s.tracked()?.into_iter().rev() {
        if interval.range.end <= later.interval.range.start {
            interval.range.end = later.interval.range.start;
            if s.verbose() {
                s.out.push_str(&format!(
                    "Filled {}to {}\n",
                    if id > 0 { format!("@{id} ") } else { String::new() },
                    crate::datetime::format_local_iso(interval.range.end)
                ));
            }
            break;
        }
    }
    Ok(())
}

pub fn fill(s: &mut Session, interval: &mut Interval, id: u32) -> Result<(), String> {
    fill_start(s, interval, id)?;
    fill_end(s, interval, id)
}

/// Make room for `interval`. Returns `false` if nothing needs to be added
/// (a running interval with the same tags is already being tracked).
pub fn auto_adjust(s: &mut Session, adjust: bool, interval: &Interval) -> Result<bool, String> {
    if interval.is_open()
        && let Some(mut latest) = s.latest()?
        && latest.range.encloses(&interval.range)
    {
        if latest.tags == interval.tags {
            return Ok(false);
        }
        s.delete(&latest)?;
        latest.range.end = interval.range.start;
        let exclusions = s.exclusions(&latest.range)?;
        for piece in flatten(&latest, &exclusions, s.now) {
            if piece.range.is_empty() {
                continue;
            }
            s.add(&piece)?;
            if s.verbose() {
                let text = crate::commands::summarize(&s.rules, &piece, s.now);
                s.out.push_str(&text);
            }
        }
    }

    let overlaps: Vec<Interval> = s.tracked_in(&interval.range, &[])?.into_iter().map(|t| t.interval).collect();
    if overlaps.is_empty() {
        return Ok(true);
    }
    if !adjust {
        return Err("You cannot overlap intervals. Correct the start/end time, or specify the :adjust hint.".to_string());
    }

    for overlap in overlaps {
        let starts_within = interval.range.starts_within(&overlap.range);
        let ends_within = interval.range.ends_within(&overlap.range);
        match (starts_within, ends_within) {
            (true, false) => {
                // The new interval starts inside: cut the old one's end.
                let mut modified = overlap.clone();
                modified.range.end = interval.range.start;
                if modified.range.is_empty() { s.delete(&overlap)? } else { s.modify(&overlap, &modified)? }
            }
            (false, true) => {
                // The new interval ends inside: cut the old one's start.
                let mut modified = overlap.clone();
                modified.range.start = interval.range.end;
                if modified.range.is_empty() { s.delete(&overlap)? } else { s.modify(&overlap, &modified)? }
            }
            (false, false) => s.delete(&overlap)?,
            (true, true) => {
                // The new interval lies inside: split the old one around it.
                let mut before = overlap.clone();
                let mut after = overlap.clone();
                before.range.end = interval.range.start;
                after.range.start = interval.range.end;
                if before.range.is_empty() { s.delete(&overlap)? } else { s.modify(&overlap, &before)? }
                if !after.range.is_empty() {
                    s.add(&after)?;
                }
            }
        }
    }
    Ok(true)
}
