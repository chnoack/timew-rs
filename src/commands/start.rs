//! `timew start [<date>] [<tag> ...]`: start tracking now or at `<date>`.
//! A running interval is stopped where the new one starts.

use crate::interval::Interval;
use crate::range::Range;
use crate::session::Session;
use crate::validate::{auto_adjust, fill_start};

pub fn run(s: &mut Session) -> Result<i32, String> {
    let range = s.cli.range(Range::new(s.now, 0), s.now)?;
    if range.start > s.now {
        return Err("Time tracking cannot be set in the future.".to_string());
    }
    if !range.is_started() || range.is_ended() {
        return Err("The start command does not accept ranges but only a single datetime. \
                    Perhaps you want the track command?"
            .to_string());
    }
    if !s.cli.ids().is_empty() {
        return Err("The start command does not accept ids. Perhaps you want the continue command?".to_string());
    }
    start_interval(s, range)
}

/// Shared with `track`, which starts an interval when given no end.
pub fn start_interval(s: &mut Session, range: Range) -> Result<i32, String> {
    let mut interval = Interval::new(range);
    for tag in s.cli.tags() {
        interval.add_tag(tag);
    }
    s.journal.start();
    if s.cli.has_hint("fill") {
        fill_start(s, &mut interval, 0)?;
    }
    if auto_adjust(s, s.cli.has_hint("adjust"), &interval)? {
        s.add(&interval)?;
    }
    s.journal.end()?;
    if s.verbose() {
        let text = super::summarize(&s.rules, &interval, s.now);
        s.out.push_str(&text);
    }
    Ok(0)
}
