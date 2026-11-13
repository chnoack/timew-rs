//! `timew undo`: revert the most recent change.

use crate::interval::Interval;
use crate::journal::ActionKind;
use crate::rules::{revert_config_change, ConfigChange};
use crate::session::Session;

pub fn run(s: &mut Session) -> Result<i32, String> {
    let Some(actions) = s.journal.pop()? else {
        if s.verbose() {
            s.out.push_str("Nothing to undo.\n");
        }
        return Ok(0);
    };
    // Revert in reverse order, so that later actions are undone first.
    for action in actions.iter().rev() {
        match action.kind {
            ActionKind::Interval => {
                let (before, _) = Interval::from_json(&action.before)?;
                let (after, _) = Interval::from_json(&action.after)?;
                s.db.modify_interval(&after, &before)?;
            }
            ActionKind::Config => {
                let change = ConfigChange { before: action.before.clone(), after: action.after.clone() };
                revert_config_change(&s.rules, &change)?;
            }
        }
    }
    if s.verbose() {
        s.out.push_str("Undo\n");
    }
    Ok(0)
}
