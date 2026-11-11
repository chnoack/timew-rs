//! `timew tags [<range>] [<tags>]`
//!
//! Clean-room port of `CmdTags.cpp` (GothenburgBitFactory/timewarrior,
//! `develop` branch, commit 3699d2e), empirically confirmed against the
//! real binary (see tests).
//!
//! Scope note: the real command looks up a per-tag `tags.<name>.description`
//! config value and a configurable color via `Rules`, both of which this
//! project has scoped out entirely (no config system). Since no
//! description is ever configured in that scope, every row's Description
//! column is always `"-"` here -- which is also exactly what the real
//! binary prints for any tag that has no configured description, so this
//! degrades to matching real output for the common (no description
//! configured) case, confirmed empirically.
//!
//! Output shape, confirmed against the real binary:
//!   - Empty result set with `verbose` on (the default, i.e. no `:quiet`
//!     hint): prints `"No data found.\n"`. With `:quiet`, prints nothing.
//!   - Non-empty: a blank line, then a two-column table (`Tag`,
//!     `Description`) with a dashed separator row sized to each column's
//!     widest entry (`Tag` column width = max tag name length, at least
//!     `len("Tag")`; `Description` column is always `len("Description")`
//!     wide here since every value is the single character `-`), then a
//!     trailing blank line.
//!   - Rows are alphabetically sorted by tag name (the union of tags
//!     across all matching intervals, deduplicated -- a `std::set` in the
//!     source).

use crate::cli::ParsedArgs;
use crate::database::Database;
use crate::range::Range;

pub fn run(database: &mut Database, mut args: ParsedArgs, verbose: bool) -> Result<String, String> {
    let range = args.take_range()?.unwrap_or_else(Range::empty);
    let required_tags: Vec<String> = args.tags().to_vec();

    let mut tags: Vec<String> = database
        .tracked_in_range(&range)?
        .into_iter()
        .filter(|t| required_tags.iter().all(|rt| t.interval.has_tag(rt)))
        .flat_map(|t| t.interval.tags)
        .collect();
    tags.sort();
    tags.dedup();

    if tags.is_empty() {
        return Ok(if verbose { "No data found.\n".to_string() } else { String::new() });
    }

    Ok(render_table(&tags))
}

fn render_table(tags: &[String]) -> String {
    const TAG_HEADER: &str = "Tag";
    const DESC_HEADER: &str = "Description";
    const DESC_VALUE: &str = "-";

    let tag_width = tags.iter().map(String::len).max().unwrap_or(0).max(TAG_HEADER.len());
    let desc_width = DESC_HEADER.len().max(DESC_VALUE.len());

    // The last column is never padded with trailing spaces (confirmed
    // empirically: real timew's table renderer left-aligns but doesn't pad
    // the rightmost column), even though its width still determines the
    // separator row's dash count.
    let mut out = String::from("\n");
    out.push_str(&format!("{TAG_HEADER:tag_width$} {DESC_HEADER}\n"));
    out.push_str(&format!("{} {}\n", "-".repeat(tag_width), "-".repeat(desc_width)));
    for tag in tags {
        out.push_str(&format!("{tag:tag_width$} {DESC_VALUE}\n"));
    }
    out.push('\n');
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
        let dir = std::env::temp_dir().join(format!("timew-rs-tags-cmd-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let db = Database::open(&dir).unwrap();
        (dir, db)
    }

    fn mk_interval(start: &str, end: &str, tags: &[&str]) -> Interval {
        let mut iv = Interval::new(crate::range::Range::new(parse_iso(start).unwrap(), parse_iso(end).unwrap()));
        for t in tags {
            iv.add_tag(*t);
        }
        iv
    }

    fn parsed(words: &[&str]) -> ParsedArgs {
        let strs: Vec<String> = words.iter().map(|s| s.to_string()).collect();
        cli::parse(&strs).unwrap()
    }

    #[test]
    fn empty_database_verbose_prints_no_data_found() {
        let (dir, mut db) = tmp_db("empty-verbose");
        let out = run(&mut db, parsed(&["tags"]), true).unwrap();
        assert_eq!(out, "No data found.\n");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_database_quiet_prints_nothing() {
        let (dir, mut db) = tmp_db("empty-quiet");
        let out = run(&mut db, parsed(&["tags"]), false).unwrap();
        assert_eq!(out, "");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn table_matches_real_timew_output_for_short_tags() {
        let (dir, mut db) = tmp_db("short-table");
        db.add_interval(&mk_interval("20261001T070000Z", "20261001T080000Z", &["zeta", "alpha"])).unwrap();
        db.commit().unwrap();

        let out = run(&mut db, parsed(&["tags"]), true).unwrap();
        // Captured verbatim from the real binary.
        assert_eq!(out, "\nTag   Description\n----- -----------\nalpha -\nzeta  -\n\n");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn table_widens_tag_column_for_long_names() {
        let (dir, mut db) = tmp_db("long-table");
        db.add_interval(&mk_interval("20261001T070000Z", "20261001T080000Z", &["a-pretty-long-tag-name", "x"]))
            .unwrap();
        db.commit().unwrap();

        let out = run(&mut db, parsed(&["tags"]), true).unwrap();
        // Captured verbatim from the real binary.
        assert_eq!(
            out,
            "\nTag                    Description\n---------------------- -----------\na-pretty-long-tag-name -\nx                      -\n\n"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn filters_by_range() {
        let (dir, mut db) = tmp_db("range-filter");
        db.add_interval(&mk_interval("20261001T070000Z", "20261001T080000Z", &["before"])).unwrap();
        db.add_interval(&mk_interval("20261005T070000Z", "20261005T080000Z", &["inside"])).unwrap();
        db.commit().unwrap();

        let out = run(
            &mut db,
            parsed(&["tags", "2026-10-03T00:00:00Z", "-", "2026-10-08T00:00:00Z"]),
            true,
        )
        .unwrap();
        assert!(out.contains("inside"));
        assert!(!out.contains("before"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn filters_by_required_tags_with_and_semantics() {
        let (dir, mut db) = tmp_db("tag-filter");
        db.add_interval(&mk_interval("20261001T070000Z", "20261001T080000Z", &["alpha", "beta"])).unwrap();
        db.add_interval(&mk_interval("20261002T070000Z", "20261002T080000Z", &["alpha"])).unwrap();
        db.commit().unwrap();

        let out = run(&mut db, parsed(&["tags", "beta"]), true).unwrap();
        assert!(out.contains("alpha"));
        assert!(out.contains("beta"));
        // Only one matching interval, but its tags are both shown.
        let lines: Vec<&str> = out.lines().filter(|l| !l.is_empty() && !l.starts_with('-') && !l.starts_with("Tag")).collect();
        assert_eq!(lines.len(), 2);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn deduplicates_tags_across_intervals() {
        let (dir, mut db) = tmp_db("dedup");
        db.add_interval(&mk_interval("20261001T070000Z", "20261001T080000Z", &["shared"])).unwrap();
        db.add_interval(&mk_interval("20261002T070000Z", "20261002T080000Z", &["shared"])).unwrap();
        db.commit().unwrap();

        let out = run(&mut db, parsed(&["tags"]), true).unwrap();
        assert_eq!(out.matches("shared").count(), 1);

        let _ = fs::remove_dir_all(&dir);
    }
}
