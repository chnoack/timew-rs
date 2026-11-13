//! `timew stop [<date>] [<tag> ...]`: stop the running interval, now or at
//! `<date>`. With tags, only those tags stop: tracking continues with the
//! remaining ones.

use crate::interval::Interval;
use crate::range::Range;
use crate::session::{flatten, Session};
use crate::validate::auto_adjust;

pub fn run(s: &mut Session) -> Result<i32, String> {
    let latest = match s.latest()? {
        Some(l) if l.is_open() => l,
        _ => return Err("There is no active time tracking.".to_string()),
    };
    if !s.cli.ids().is_empty() {
        return Err("The stop command does not accept ids as it works on the most recent open interval only. \
                    Perhaps you want the modify command?."
            .to_string());
    }
    let range = s.cli.range(Range::new(s.now, 0), s.now)?;
    if !range.is_started() {
        return Err("No datetime specified.".to_string());
    }
    if range.start <= latest.range.start {
        return Err("The end of a date range must be after the start.".to_string());
    }
    let tags = s.cli.tags();
    if let Some(missing) = tags.iter().find(|t| !latest.has_tag(t)) {
        return Err(format!("The current interval does not have the '{missing}' tag."));
    }
    let remaining: Vec<String> = if tags.is_empty() {
        Vec::new()
    } else {
        latest.tags.iter().filter(|t| !tags.contains(t)).cloned().collect()
    };
    let adjust = s.cli.has_hint("adjust");

    s.journal.start();
    if remaining.is_empty() {
        let mut stopped = latest.clone();
        stopped.range.end = range.start;
        s.delete(&latest)?;
        auto_adjust(s, adjust, &stopped)?;
        let exclusions = s.exclusions(&stopped.range)?;
        for piece in flatten(&stopped, &exclusions, s.now) {
            s.add(&piece)?;
            if s.verbose() {
                let text = super::summarize(&s.rules, &piece, s.now);
                s.out.push_str(&text);
            }
        }
    } else {
        // Stopping some of the tags: the running interval ends, and a new
        // one with the remaining tags starts at the same time.
        let mut next = Interval::new(Range::new(range.start, 0));
        for tag in remaining {
            next.add_tag(tag);
        }
        auto_adjust(s, adjust, &next)?;
        s.add(&next)?;
        if s.verbose() {
            let text = super::summarize(&s.rules, &next, s.now);
            s.out.push('\n');
            s.out.push_str(&text);
        }
    }
    s.journal.end()?;
    Ok(0)
}
