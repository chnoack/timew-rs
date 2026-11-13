//! `timew fill [@<id>]`: extend an interval (by default the running one)
//! back to the end of the previous one and forward to the start of the
//! next one.

use crate::session::Session;
use crate::validate::{auto_adjust, fill};

pub fn run(s: &mut Session) -> Result<i32, String> {
    let ids = s.cli.ids();
    if ids.len() > 1 {
        return Err("Only one ID may be specified. See 'timew help fill'.".to_string());
    }
    let found = if ids.is_empty() {
        match s.first()? {
            None => return Err("There is no active time tracking.".to_string()),
            Some(t) if !t.interval.is_open() => return Err("ID must be specified. See 'timew help fill'.".to_string()),
            Some(t) => t,
        }
    } else {
        s.tracked_ids(&ids)?.remove(0)
    };
    let mut modified = found.interval.clone();
    fill(s, &mut modified, found.id)?;
    s.delete(&found.interval)?;
    auto_adjust(s, s.cli.has_hint("adjust"), &modified)?;
    s.add(&modified)?;
    Ok(0)
}
