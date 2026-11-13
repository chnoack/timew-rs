//! Changing entries, and undoing that: the editing functions of
//! `timew-tui` (start, stop, modify). They open the configuration and the
//! database like the command line program does and save right away. Each
//! returns the changes it made, which [`undo_change`] can revert.

use crate::session::{flatten, Change, Session};
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

/// Start tracking now, with `tags` and `annotation`, like `timew start`.
/// Refused if an entry is already running. Returns the new entry.
pub fn start_interval(
    paths: &paths::Paths,
    tags: &[String],
    annotation: Option<&str>,
) -> Result<(interval::Interval, Vec<Change>), String> {
    let now = datetime::now();
    let mut s = open_session(paths, now)?;
    if s.db.latest()?.is_some_and(|l| l.is_open()) {
        return Err("An entry is already running.".to_string());
    }
    let mut interval = interval::Interval::new(range::Range::new(now, 0));
    for tag in tags {
        interval.add_tag(tag.clone());
    }
    interval.annotation = annotation.map(str::trim).filter(|a| !a.is_empty()).map(str::to_string);
    // A running entry has no end, so it would overlap everything stored
    // after now (entries entered in advance, or still going on).
    let later = s.tracked_in(&interval.range, &[])?;
    if let Some(first) = later.first() {
        let when = datetime::format_local_iso(first.interval.range.start).replace('T', " ");
        let when = &when[..16];
        let tags = if first.interval.tags.is_empty() { String::new() } else { format!(" ({})", first.interval.tags.join(", ")) };
        let what = match later.len() {
            1 => format!("an entry after now: {when}{tags}"),
            n => format!("{n} entries after now, the first {when}{tags}"),
        };
        return Err(format!("A running entry would overlap {what}."));
    }
    validate::auto_adjust(&mut s, false, &interval)?;
    s.add(&interval)?;
    s.db.commit()?;
    Ok((interval, s.changes))
}

/// Stop the running entry now, like `timew stop`: excluded time inside it
/// (configured exclusions) is cut out. Returns the entry as it was before.
pub fn stop_interval(paths: &paths::Paths) -> Result<(interval::Interval, Vec<Change>), String> {
    let now = datetime::now();
    let mut s = open_session(paths, now)?;
    let running = match s.db.latest()? {
        Some(l) if l.is_open() => l,
        _ => return Err("No entry is running.".to_string()),
    };
    if running.range.start >= now {
        return Err("The running entry starts in the future; it can't be stopped yet.".to_string());
    }
    let mut stopped = running.clone();
    stopped.range.end = now;
    s.delete(&running)?;
    validate::auto_adjust(&mut s, false, &stopped)?;
    let exclusions = s.exclusions(&stopped.range)?;
    for piece in flatten(&stopped, &exclusions, now) {
        s.add(&piece)?;
    }
    s.db.commit()?;
    Ok((running, s.changes))
}

/// Revert the changes made by [`modify_interval`], [`start_interval`] or
/// [`stop_interval`]. Only if the intervals
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

    #[test]
    fn start_and_stop_with_undo() {
        let root = std::env::temp_dir().join(format!("timew-core-startstop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("data")).unwrap();
        let paths = paths::Paths { config_dir: root.clone(), db_dir: root.clone() };
        let stored = || {
            let mut db = database::Database::open(root.join("data")).unwrap();
            db.intervals().unwrap().to_vec()
        };

        assert!(stop_interval(&paths).unwrap_err().contains("No entry is running"));
        let (started, start_changes) =
            start_interval(&paths, &["b".into(), "a".into()], Some("  first  ")).unwrap();
        assert!(started.is_open());
        assert_eq!(started.tags, vec!["a", "b"]);
        assert_eq!(started.annotation.as_deref(), Some("first"));
        assert_eq!(stored(), vec![started.clone()]);
        assert!(start_interval(&paths, &[], None).unwrap_err().contains("already running"));

        std::thread::sleep(std::time::Duration::from_millis(1100));
        let (before, stop_changes) = stop_interval(&paths).unwrap();
        assert_eq!(before, started);
        let now = stored();
        assert_eq!(now.len(), 1);
        assert!(now[0].is_ended() && now[0].range.start == started.range.start);

        undo_change(&paths, &stop_changes).unwrap();
        assert_eq!(stored(), vec![started.clone()], "running again");
        undo_change(&paths, &start_changes).unwrap();
        assert!(stored().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }


    #[test]
    fn start_names_the_entries_in_the_way() {
        let root = std::env::temp_dir().join(format!("timew-core-start-later-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("data")).unwrap();
        let paths = paths::Paths { config_dir: root.clone(), db_dir: root.clone() };
        let soon = datetime::now() + 3 * 86400;
        let line = |start: i64, tag: &str| format!("inc {} - {} # {tag}\n", format_iso(start), format_iso(start + 3600));
        let file = |e: i64| {
            let (y, m) = crate::interval::year_month_of(e);
            root.join(format!("data/{y:04}-{m:02}.data"))
        };
        std::fs::write(file(soon), line(soon, "planned")).unwrap();
        let err = start_interval(&paths, &[], None).unwrap_err();
        let when = &datetime::format_local_iso(soon).replace('T', " ")[..16];
        assert_eq!(err, format!("A running entry would overlap an entry after now: {when} (planned)."));
        let _ = std::fs::remove_dir_all(&root);
    }

}
