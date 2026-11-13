//! `timew continue [@<id> | <tag> ...] [<date> | <range>]`: track again
//! with the tags (and annotation) of an earlier interval: the most recent
//! one, the one with the ID, or the most recent one with all the tags.

use crate::range::Range;
use crate::session::Session;
use crate::validate::auto_adjust;

pub fn run(s: &mut Session) -> Result<i32, String> {
    let range = s.cli.range(Range::new(s.now, 0), s.now)?;
    if range.start > s.now {
        return Err("Time tracking cannot be set in the future.".to_string());
    }
    let tags = s.cli.tags();
    let ids = s.cli.ids();
    if !ids.is_empty() && !tags.is_empty() {
        return Err("You cannot specify both id and tags to continue an interval.".to_string());
    }
    if ids.len() > 1 {
        return Err("You can only specify one ID to continue.".to_string());
    }

    let mut interval = if !ids.is_empty() {
        s.tracked_ids(&ids)?.remove(0).interval
    } else if !tags.is_empty() {
        let found = s.tracked()?.into_iter().find(|t| tags.iter().all(|tag| t.interval.has_tag(tag)));
        match found {
            Some(t) => t.interval,
            None => {
                return Err(format!(
                    "Tags '{}' do not correspond to any tracking.",
                    crate::text::join_quoted(", ", &tags)
                ));
            }
        }
    } else {
        match s.first()? {
            None => return Err("There is no previous tracking to continue.".to_string()),
            Some(t) if t.interval.is_open() => return Err("There is already active tracking.".to_string()),
            Some(t) => t.interval,
        }
    };

    interval.range = if range.is_started() { range } else { Range::new(s.now, 0) };
    if auto_adjust(s, s.cli.has_hint("adjust"), &interval)? {
        s.add(&interval)?;
    }
    if s.verbose() {
        let text = super::summarize(&s.rules, &interval, s.now);
        s.out.push_str(&text);
    }
    Ok(0)
}
