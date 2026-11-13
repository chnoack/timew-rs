//! `timew join @<id> @<id>`: merge two intervals into one, from the start
//! of the earlier to the end of the later, with the tags of both.

use crate::session::Session;
use crate::validate::auto_adjust;

pub fn run(s: &mut Session) -> Result<i32, String> {
    let ids = s.cli.ids();
    if ids.len() != 2 {
        return Err("Two IDs must be specified. See 'timew help join'.".to_string());
    }
    s.flatten_latest()?;
    let found = s.tracked_ids(&ids)?;
    let (earlier, later) = (&found[0], &found[1]);
    let mut combined = earlier.interval.clone();
    combined.range.end = later.interval.range.end;
    for tag in &later.interval.tags {
        combined.add_tag(tag.clone());
    }
    if combined.annotation.is_none() {
        combined.annotation = later.interval.annotation.clone();
    }
    s.delete(&later.interval)?;
    s.delete(&earlier.interval)?;
    auto_adjust(&mut s.core, s.cli.has_hint("adjust"), &combined)?;
    s.add(&combined)?;
    if s.verbose() {
        s.out.push_str(&format!("Joined @{} and @{}\n", later.id, earlier.id));
    }
    Ok(0)
}
