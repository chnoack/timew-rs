//! `timew cancel`: throw away the running interval.

use crate::session::Session;

pub fn run(s: &mut Session) -> Result<i32, String> {
    let latest = match s.latest()? {
        Some(l) if l.is_open() => l,
        _ => {
            if s.verbose() {
                s.out.push_str("There is no active time tracking.\n");
            }
            return Ok(0);
        }
    };
    s.journal.start();
    s.delete(&latest)?;
    s.journal.end()?;
    if s.verbose() {
        s.out.push_str("Canceled active time tracking.\n");
    }
    Ok(0)
}
