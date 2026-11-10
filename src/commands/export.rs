//! `timew export [<range>] [<tags>] [@id ...]`
//!
//! Clean-room port of `CmdExport.cpp` (GothenburgBitFactory/timewarrior,
//! `develop` branch, commit 3699d2e), empirically confirmed against the
//! real binary (see tests):
//!   - `timew export` with no arguments returns every tracked interval.
//!   - ids and tags/range are mutually exclusive; giving both is an error.
//!   - When filtering by id, any id that doesn't currently exist is an
//!     error naming that id specifically.
//!   - When filtering by range/tags, a tag filter requires ALL given tags
//!     to be present on an interval (AND semantics, from
//!     `IntervalFilterAllWithTags`).
//!   - Output is `[\n<entry>,\n<entry>\n]\n` (or `[\n]\n` if empty), one
//!     JSON object per line, intervals in chronological (oldest-first)
//!     order -- this ordering comes from `Database::tracked_in_range`
//!     already matching `getTracked`'s documented "sorted by date"
//!     contract.

use crate::cli::ParsedArgs;
use crate::database::Database;
use crate::range::Range;

pub fn run(database: &mut Database, mut args: ParsedArgs) -> Result<String, String> {
    let ids = args.ids.clone();
    let range = args.take_range()?;
    let tags: Vec<String> = args.tags().to_vec();

    if !ids.is_empty() && !tags.is_empty() {
        return Err("You cannot specify both id and tags/range to export intervals.".to_string());
    }

    let tracked = if !ids.is_empty() {
        let found = database.tracked_by_ids(&ids)?;
        for (id, result) in ids.iter().zip(found.iter()) {
            if result.is_none() {
                return Err(format!("ID '@{id}' does not correspond to any tracking."));
            }
        }
        found.into_iter().flatten().collect::<Vec<_>>()
    } else {
        let range = range.unwrap_or_else(Range::empty);
        database
            .tracked_in_range(&range)?
            .into_iter()
            .filter(|t| tags.iter().all(|tag| t.interval.has_tag(tag)))
            .collect::<Vec<_>>()
    };

    Ok(json_from_tracked(&tracked))
}

fn json_from_tracked(tracked: &[crate::database::Tracked]) -> String {
    let mut out = String::from("[\n");
    for (i, t) in tracked.iter().enumerate() {
        if i > 0 {
            out.push_str(",\n");
        }
        out.push_str(&t.interval.to_json(t.id));
    }
    if !tracked.is_empty() {
        out.push('\n');
    }
    out.push_str("]\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli;
    use crate::interval::{parse_iso, Interval};
    use std::fs;
    use std::path::PathBuf;

    fn tmp_db(name: &str) -> (PathBuf, Database) {
        let dir = std::env::temp_dir().join(format!("timew-rs-export-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let db = Database::open(&dir).unwrap();
        (dir, db)
    }

    fn mk_interval(start: &str, end: &str, tag: &str) -> Interval {
        let mut iv = Interval::new(crate::range::Range::new(parse_iso(start).unwrap(), parse_iso(end).unwrap()));
        iv.add_tag(tag);
        iv
    }

    fn parsed(words: &[&str]) -> ParsedArgs {
        let strs: Vec<String> = words.iter().map(|s| s.to_string()).collect();
        cli::parse(&strs).unwrap()
    }

    #[test]
    fn export_with_no_args_returns_everything() {
        let (dir, mut db) = tmp_db("no-args");
        db.add_interval(&mk_interval("20261001T070000Z", "20261001T080000Z", "AAA")).unwrap();
        db.commit().unwrap();

        let out = run(&mut db, parsed(&["export"])).unwrap();
        assert_eq!(
            out,
            "[\n{\"id\":1,\"start\":\"20261001T070000Z\",\"end\":\"20261001T080000Z\",\"tags\":[\"AAA\"]}\n]\n"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_empty_database_matches_real_timew_output_shape() {
        let (dir, mut db) = tmp_db("empty");
        let out = run(&mut db, parsed(&["export"])).unwrap();
        // Confirmed empirically: real `timew export` on an empty db prints "[\n]\n".
        assert_eq!(out, "[\n]\n");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_filters_by_range() {
        let (dir, mut db) = tmp_db("range-filter");
        db.add_interval(&mk_interval("20261001T070000Z", "20261001T080000Z", "before")).unwrap();
        db.add_interval(&mk_interval("20261005T070000Z", "20261005T080000Z", "inside")).unwrap();
        db.add_interval(&mk_interval("20261010T070000Z", "20261010T080000Z", "after")).unwrap();
        db.commit().unwrap();

        let out = run(
            &mut db,
            parsed(&["export", "2026-10-03T00:00:00Z", "-", "2026-10-08T00:00:00Z"]),
        )
        .unwrap();
        assert!(out.contains("inside"));
        assert!(!out.contains("before"));
        assert!(!out.contains("after"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_filters_by_tag_with_and_semantics() {
        let (dir, mut db) = tmp_db("tag-filter");
        let mut both = mk_interval("20261001T070000Z", "20261001T080000Z", "alpha");
        both.add_tag("beta");
        db.add_interval(&both).unwrap();
        db.add_interval(&mk_interval("20261002T070000Z", "20261002T080000Z", "alpha")).unwrap();
        db.commit().unwrap();

        let out = run(&mut db, parsed(&["export", "alpha", "beta"])).unwrap();
        // Only the interval with BOTH tags should match.
        let count = out.matches("\"id\"").count();
        assert_eq!(count, 1);
        assert!(out.contains("alpha"));
        assert!(out.contains("beta"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_by_id() {
        let (dir, mut db) = tmp_db("by-id");
        db.add_interval(&mk_interval("20261001T070000Z", "20261001T080000Z", "a")).unwrap();
        db.add_interval(&mk_interval("20261002T070000Z", "20261002T080000Z", "b")).unwrap();
        db.commit().unwrap();

        // id 1 = newest = "b"
        let out = run(&mut db, parsed(&["export", "@1"])).unwrap();
        assert!(out.contains("\"b\""));
        assert!(!out.contains("\"a\""));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_nonexistent_id_errors_with_the_offending_id() {
        let (dir, mut db) = tmp_db("missing-id");
        db.add_interval(&mk_interval("20261001T070000Z", "20261001T080000Z", "a")).unwrap();
        db.commit().unwrap();

        let err = run(&mut db, parsed(&["export", "@99"])).unwrap_err();
        assert!(err.contains("@99"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_rejects_both_id_and_tags() {
        let (dir, mut db) = tmp_db("id-and-tags");
        db.add_interval(&mk_interval("20261001T070000Z", "20261001T080000Z", "a")).unwrap();
        db.commit().unwrap();

        let err = run(&mut db, parsed(&["export", "@1", "a"])).unwrap_err();
        assert!(err.contains("cannot specify both"));

        let _ = fs::remove_dir_all(&dir);
    }
}
