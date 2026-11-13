//! `timew track <range> [<tag> ...]`: record a closed interval. Excluded
//! time inside the range is left out (the interval is split around it).
//! Without an end it works like `start`.

use crate::interval::Interval;
use crate::range::Range;
use crate::session::{flatten, Session};
use crate::validate::{auto_adjust, fill};

pub fn run(s: &mut Session) -> Result<i32, String> {
    if !s.cli.ids().is_empty() {
        return Err("The track command does not accept ids. Perhaps you want the continue command?".to_string());
    }
    let range = s.cli.range(Range::empty(), s.now)?;
    if !range.is_started() || !range.is_ended() {
        return super::start::run(s);
    }
    let mut interval = Interval::new(range);
    for tag in s.cli.tags() {
        interval.add_tag(tag);
    }
    s.journal.start();
    if s.cli.has_hint("fill") {
        fill(s, &mut interval, 0)?;
    }
    auto_adjust(s, s.cli.has_hint("adjust"), &interval)?;
    let exclusions = s.exclusions(&range)?;
    for piece in flatten(&interval, &exclusions, s.now) {
        s.add(&piece)?;
        if s.verbose() {
            let text = super::summarize(&s.rules, &piece, s.now);
            s.out.push_str(&text);
        }
    }
    s.journal.end()?;
    Ok(0)
}
