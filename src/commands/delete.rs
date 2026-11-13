//! `timew delete @<id> ...`: delete intervals.

use crate::session::Session;

pub fn run(s: &mut Session) -> Result<i32, String> {
    let ids = s.cli.ids();
    if ids.is_empty() {
        return Err("IDs must be specified. See 'timew help delete'.".to_string());
    }
    s.journal.start();
    s.flatten_latest()?;
    for t in s.tracked_ids(&ids)? {
        s.delete(&t.interval)?;
        if s.verbose() {
            s.out.push_str(&format!("Deleted @{}\n", t.id));
        }
    }
    s.journal.end()?;
    Ok(0)
}
