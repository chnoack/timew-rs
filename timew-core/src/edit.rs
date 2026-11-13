//! Changing one entry, and undoing that: the editing functions of
//! `timew-tui`. They open the configuration and the database like the
//! command line program does and save right away.

use crate::session::{Change, Session};
use crate::{database, datetime, interval, paths, range, rules, validate};

/// Replace the stored interval `from` with `to`, the way `timew modify`
/// does: an overlap with another interval is refused, or with `adjust`
/// resolved like `:adjust` (the other intervals are shortened, split or
/// deleted). The tag counts are kept up to date.
///
/// Returns how many other intervals were adjusted, and every change made
/// (including those to adjusted intervals), which [`undo_change`] can
/// revert. Used by `timew-tui` to edit entries.
pub fn modify_interval(
    paths: &paths::Paths,
    from: &interval::Interval,
    to: &interval::Interval,
    adjust: bool,
) -> Result<(usize, Vec<Change>), String> {
    if from == to {
        return Ok((0, Vec::new()));
    }
    let now = datetime::now();
    if to.range.is_ended() && to.range.start >= to.range.end {
        return Err("The end must be after the start.".to_string());
    }
    if to.range.is_open() && to.range.start > now {
        return Err("A running entry cannot start in the future.".to_string());
    }
    let mut s = open_session(paths, now)?;
    if !s.db.intervals()?.contains(from) {
        return Err("The entry was changed by another program. Press r to reload.".to_string());
    }
    s.delete(from)?;
    let overlapping = s.tracked_in(&to.range, &[])?.len();
    validate::auto_adjust(&mut s, adjust, to)?;
    s.add(to)?;
    s.db.commit()?;
    Ok((overlapping, s.changes))
}

/// Revert the changes made by [`modify_interval`]. Only if the intervals
/// they left behind are still stored unchanged, and the restored intervals
/// don't overlap anything added since; otherwise nothing is changed.
pub fn undo_change(paths: &paths::Paths, changes: &[Change]) -> Result<(), String> {
    let mut s = open_session(paths, datetime::now())?;
    for change in changes.iter().rev() {
        s.db.modify_interval(&change.after, &change.before)
            .map_err(|_| "The entry was changed by another program since.".to_string())?;
    }
    let stored = s.db.intervals()?;
    for restored in changes.iter().map(|c| &c.before).filter(|b| !b.is_empty() && stored.contains(b)) {
        if stored.iter().any(|other| other != restored && other.range.intersects(&restored.range)) {
            return Err("It would overlap an entry added since.".to_string());
        }
    }
    s.db.commit()
}

fn open_session(paths: &paths::Paths, now: range::Epoch) -> Result<Session, String> {
    let config = paths.config_file();
    let mut rules = if config.exists() { rules::Rules::load(&config)? } else { rules::Rules::default() };
    rules.set("verbose", "off");
    let (db, _, _) = database::Database::open_and_repair(paths.data_dir())?;
    Ok(Session::new(rules, db, paths.clone(), now))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datetime::make_local;
    use crate::interval::{format_iso, Interval};
    use crate::range::Range;

    #[test]
    fn edits_an_entry_written_differently_in_the_wrong_file() {
        let root = std::env::temp_dir().join(format!("timew-core-edit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("data")).unwrap();
        let (start, end) = (make_local(2026, 11, 2, 9, 0, 0), make_local(2026, 11, 2, 10, 0, 0));
        // November entry, in the October file, tags unsorted and unquoted
        let line = format!("inc {}  -  {} # zeta client-a\n", format_iso(start), format_iso(end));
        std::fs::write(root.join("data/2026-10.data"), &line).unwrap();
        let paths = paths::Paths { config_dir: root.clone(), db_dir: root.clone() };

        let mut from = Interval::new(Range::new(start, end));
        from.add_tag("zeta");
        from.add_tag("client-a");
        let mut to = from.clone();
        to.range.end = end + 1800;
        let (_, changes) = modify_interval(&paths, &from, &to, false).unwrap();
        assert!(!root.join("data/2026-10.data").exists(), "the old file had only this entry");
        let nov = std::fs::read_to_string(root.join("data/2026-11.data")).unwrap();
        assert_eq!(nov, format!("inc {} - {} # \"client-a\" zeta\n", format_iso(start), format_iso(end + 1800)));

        undo_change(&paths, &changes).unwrap();
        let nov = std::fs::read_to_string(root.join("data/2026-11.data")).unwrap();
        assert_eq!(nov, format!("inc {} - {} # \"client-a\" zeta\n", format_iso(start), format_iso(end)));
        let _ = std::fs::remove_dir_all(&root);
    }
}
