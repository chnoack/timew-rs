//! `timew split @<id> ...`: split intervals in two equal halves (a running
//! one at the middle between its start and now).

use crate::session::Session;
use crate::validate::auto_adjust;

pub fn run(s: &mut Session) -> Result<i32, String> {
    let ids = s.cli.ids();
    if ids.is_empty() {
        return Err("IDs must be specified. See 'timew help split'.".to_string());
    }
    let adjust = s.cli.has_hint("adjust");
    s.flatten_latest()?;
    for t in s.tracked_ids(&ids)? {
        let end = if t.interval.is_open() { s.now } else { t.interval.range.end };
        let middle = t.interval.range.start + (end - t.interval.range.start) / 2;
        let mut first = t.interval.clone();
        let mut second = t.interval.clone();
        first.range.end = middle;
        second.range.start = middle;
        s.delete(&t.interval)?;
        auto_adjust(s, adjust, &first)?;
        s.add(&first)?;
        auto_adjust(s, adjust, &second)?;
        s.add(&second)?;
        if s.verbose() {
            s.out.push_str(&format!("Split @{}\n", t.id));
        }
    }
    Ok(0)
}
