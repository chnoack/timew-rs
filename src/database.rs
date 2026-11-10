//! Aggregates all `YYYY-MM.data` files under `data/`, and assigns ephemeral
//! interval IDs by counting backward from the most recent interval.
//!
//! **This is the single most important, least obvious piece of behavior in
//! the whole system.** IDs are NEVER stored on disk -- `@1` always means
//! "the most recent interval" (the open one, if any is open), `@2` the one
//! before it, and so on. The ID an interval has is entirely a function of
//! its position when sorted newest-first, recomputed fresh on every
//! invocation. We were bitten by exactly this earlier in this project: a
//! `timew modify` with a bare time (no date) silently applied to *today*
//! instead of the target historical date, and that is possible precisely
//! because `@141` is not a stable identity -- it's "whichever interval is
//! currently 141st-from-the-end."
//!
//! Clean-room port of `Database.cpp` + the ID-assignment logic in
//! `data.cpp::getTracked` (GothenburgBitFactory/timewarrior, `develop`
//! branch, commit 3699d2e), re-derived from reading that source, scoped
//! down by deliberately omitting the `Rules`/holiday/exclusion-driven
//! "synthetic interval" splitting (`expandLatest`'s flatten-on-exclusions
//! path) -- confirmed by reading `expandLatest` that without exclusions
//! configured, it always reduces to the simple "latest interval gets id 1"
//! case, which is what this scoped project needs.
//!
//! Key facts about file/line ordering, confirmed by reading the source
//! (not guessed):
//!   - On startup, `.data` files are discovered, sorted *lexicographically*
//!     by filename. Since filenames are `YYYY-MM.data`, this is also
//!     chronological order (oldest to newest).
//!   - `Database::begin()` iterates files in *reverse* (newest file first),
//!     and within each file, iterates lines in *reverse* too. Since
//!     `Datafile::commit()` sorts lines ascending-by-string before writing
//!     (and the ISO prefix sorts chronologically), reverse-iterating a
//!     file's lines gives newest-line-first.
//!   - Net effect: `Database::begin()` yields intervals newest-to-oldest
//!     across the whole database, which is exactly the order needed to
//!     assign id=1 to the most recent interval, id=2 to the next, etc.
//!   - An interval is always stored in the datafile keyed by its **start**
//!     date's year/month (`Database::addInterval` / `getDatafile`), even if
//!     it ends in a different month.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::datafile::Datafile;
use crate::interval::Interval;
use crate::range::Range;

pub struct Database {
    data_dir: PathBuf,
    /// Loaded datafiles, kept in ascending filename order (oldest month
    /// first), mirroring `Database::_files` after `initializeDatafiles()`.
    files: Vec<Datafile>,
}

/// An interval together with the ephemeral id it was assigned for this
/// query. Never persisted -- recomputed every time `Database::tracked` (or
/// similar) is called.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tracked {
    pub id: u32,
    pub interval: Interval,
}

impl Database {
    /// `data_dir` is the `data/` subdirectory of a `TIMEWARRIORDB` root
    /// (i.e. `<TIMEWARRIORDB>/data`), matching `paths::dbDataDir()`.
    pub fn open(data_dir: impl Into<PathBuf>) -> io::Result<Self> {
        let data_dir = data_dir.into();
        let files = discover_datafiles(&data_dir)?;
        Ok(Database { data_dir, files })
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// Every stored interval, newest-first, with ids assigned 1, 2, 3, ...
    /// in that order. This is the core traversal every other query is built
    /// on (mirrors `Database::begin()`/`end()` plus the id-assignment loop
    /// in `getTracked`, minus the exclusion-driven synthetic-interval
    /// splitting -- see module docs).
    pub fn all_tracked(&mut self) -> Result<Vec<Tracked>, String> {
        let mut result = Vec::new();
        let mut next_id: u32 = 1;

        // Newest file first.
        for file in self.files.iter_mut().rev() {
            let lines = file.all_lines().map_err(|e| e.to_string())?;
            // Lines are stored ascending-by-string (chronological) once
            // committed, but may be append-order before a commit. We sort a
            // local copy to guarantee newest-line-first within this file,
            // matching the guarantee `Datafile::commit()` provides on disk
            // (string-sort == chronological-sort for the fixed-width ISO
            // prefix).
            let mut sorted_lines: Vec<&String> = lines.iter().collect();
            sorted_lines.sort();

            for line in sorted_lines.into_iter().rev() {
                if let Some(interval) = Interval::parse(line)? {
                    result.push(Tracked { id: next_id, interval });
                    next_id += 1;
                }
            }
        }

        Ok(result)
    }

    /// All tracked intervals whose range intersects `range`. `Range::empty()`
    /// (unset start and end) matches everything, mirroring
    /// `IntervalFilterAllInRange({0,0})` usage for unbounded scans (e.g.
    /// fill-forward/fill-backward in `validate.rs`).
    ///
    /// Results are returned oldest-first (ids ascending-by-recency get
    /// reversed at the end), matching `getTracked`'s documented contract:
    /// "sorted by date" even though ids are assigned while walking
    /// newest-first. This matters for callers like `summary` that print
    /// chronologically.
    pub fn tracked_in_range(&mut self, range: &Range) -> Result<Vec<Tracked>, String> {
        let unbounded = !range.is_started() && !range.is_ended();
        let mut out = Vec::new();

        for t in self.all_tracked()? {
            if unbounded || t.interval.range.intersects(range) {
                out.push(t);
            } else if t.interval.range.end != 0 && t.interval.range.end <= range.start {
                // We're walking newest-first; once we see an interval that
                // ends at/before the query range's start, every earlier
                // interval is also before it (ids/lines are chronologically
                // ordered), so we can stop early. Mirrors the `set_done`
                // early-exit optimization in `IntervalFilterAllInRange`.
                break;
            }
        }

        out.reverse();
        Ok(out)
    }

    /// Fetch tracked intervals by their ephemeral id (as currently assigned
    /// across the whole database). Returns them in the order the ids were
    /// given, with `None` for any id that doesn't currently exist.
    pub fn tracked_by_ids(&mut self, ids: &[u32]) -> Result<Vec<Option<Tracked>>, String> {
        let all = self.all_tracked()?;
        Ok(ids
            .iter()
            .map(|id| all.iter().find(|t| t.id == *id).cloned())
            .collect())
    }

    /// The single most recent interval (id 1), if any exists.
    pub fn latest(&mut self) -> Result<Option<Tracked>, String> {
        // We only need the very first (newest file, newest line) entry, but
        // reuse all_tracked for correctness/simplicity at this project's
        // scale; a real large-database implementation would short-circuit
        // after the first match the way the C++ iterator-based code does.
        Ok(self.all_tracked()?.into_iter().next())
    }

    /// Add a new interval, keyed into the datafile matching its **start**
    /// date's year/month (matches `Database::addInterval`/`getDatafile`).
    /// Does not touch `tags.data` -- callers that care about tag usage
    /// counts (e.g. the `tag`/`track`/`start` commands) update `tags.rs`
    /// separately, mirroring how `Database::addInterval` and
    /// `TagInfoDatabase` are distinct collaborators in the C++ source.
    pub fn add_interval(&mut self, interval: &Interval) -> Result<(), String> {
        if interval.range.is_ended() && interval.range.start > interval.range.end {
            return Err("Cannot add an interval that ends before it starts.".to_string());
        }
        let idx = self.datafile_index_for(interval.range.start);
        self.files[idx].add_interval(interval)
    }

    /// Remove an interval, keyed by its start date the same way
    /// `add_interval` is (matches `Database::deleteInterval`).
    pub fn delete_interval(&mut self, interval: &Interval) -> Result<(), String> {
        let idx = self.datafile_index_for(interval.range.start);
        self.files[idx].delete_interval(interval)
    }

    /// Replace `from` with `to`: delete the old serialization, add the new
    /// one. This two-step dance (rather than an in-place edit) is
    /// deliberate and matches `Database::modifyInterval` exactly, because a
    /// modification can change the start date enough to move the interval
    /// into a different month's datafile.
    pub fn modify_interval(&mut self, from: &Interval, to: &Interval) -> Result<(), String> {
        if !from.is_empty() {
            self.delete_interval(from)?;
        }
        if !to.is_empty() {
            self.add_interval(to)?;
        }
        Ok(())
    }

    /// Persist all pending changes across every loaded datafile.
    pub fn commit(&mut self) -> io::Result<()> {
        for file in &mut self.files {
            file.commit()?;
        }
        Ok(())
    }

    /// Find (creating on demand, in-memory only -- not written until
    /// `commit`) the index into `self.files` for the datafile that should
    /// hold an interval starting at `start_epoch`.
    fn datafile_index_for(&mut self, start_epoch: crate::range::Epoch) -> usize {
        let (year, month) = crate::interval::year_month_of(start_epoch);
        let name = format!("{year:04}-{month:02}.data");

        if let Some(idx) = self.files.iter().position(|f| {
            f.path().file_name().and_then(|n| n.to_str()) == Some(name.as_str())
        }) {
            return idx;
        }

        let path = self.data_dir.join(&name);
        self.files.push(Datafile::new(path));
        self.files.len() - 1
    }
}

/// Scan `data_dir` for files matching `YYYY-MM.data`, returning `Datafile`
/// handles sorted ascending by filename (oldest month first), matching
/// `Database::initializeDatafiles`. A missing `data_dir` is treated as "no
/// files yet" rather than an error (a brand new TIMEWARRIORDB has no data/
/// directory until the first write).
fn discover_datafiles(data_dir: &Path) -> io::Result<Vec<Datafile>> {
    let entries = match fs::read_dir(data_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let mut names: Vec<String> = Vec::new();
    for entry in entries {
        let entry = entry?;
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else { continue };
        if is_month_datafile_name(name) {
            names.push(name.to_string());
        }
    }
    names.sort();

    Ok(names.into_iter().map(|name| Datafile::new(data_dir.join(name))).collect())
}

/// Matches `Database::initializeDatafiles`'s check: 8th-from-last char is
/// `-` and the name ends in `.data`, e.g. `2026-10.data`. We additionally
/// require the two numeric groups to actually be digits, which the C++
/// source does not strictly validate at this stage (it relies on `strtol`
/// silently returning 0 for garbage) -- being stricter here only narrows
/// what we accept, it never accepts something the original would reject.
fn is_month_datafile_name(name: &str) -> bool {
    if !name.ends_with(".data") || name.len() < 8 {
        return false;
    }
    let bytes = name.as_bytes();
    if bytes[bytes.len() - 8] != b'-' {
        return false;
    }
    let year = &name[0..4];
    let month = &name[5..7];
    year.chars().all(|c| c.is_ascii_digit()) && month.chars().all(|c| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interval::parse_iso;

    fn tmp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("timew-rs-db-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_month_file(dir: &Path, name: &str, lines: &[&str]) {
        fs::write(dir.join(name), lines.join("\n") + "\n").unwrap();
    }

    #[test]
    fn discovers_only_month_shaped_data_files() {
        let dir = tmp_dir("discover");
        write_month_file(&dir, "2026-10.data", &["inc 20261001T070000Z - 20261001T080000Z # a"]);
        fs::write(dir.join("tags.data"), "{}").unwrap();
        fs::write(dir.join("undo.data"), "").unwrap();
        fs::write(dir.join("notes.txt"), "").unwrap();

        let files = discover_datafiles(&dir).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path().file_name().unwrap(), "2026-10.data");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_data_dir_yields_empty_database() {
        let dir = tmp_dir("missing").join("does-not-exist");
        let mut db = Database::open(&dir).unwrap();
        assert_eq!(db.all_tracked().unwrap(), Vec::new());
    }

    #[test]
    fn ids_assigned_newest_first_across_single_file() {
        let dir = tmp_dir("ids-single-file");
        write_month_file(
            &dir,
            "2026-10.data",
            &[
                "inc 20261001T070000Z - 20261001T080000Z # first",
                "inc 20261002T070000Z - 20261002T080000Z # second",
                "inc 20261003T070000Z - 20261003T080000Z # third",
            ],
        );

        let mut db = Database::open(&dir).unwrap();
        let tracked = db.all_tracked().unwrap();
        assert_eq!(tracked.len(), 3);
        // Newest (third) is id 1, oldest (first) is id 3.
        assert_eq!(tracked[0].id, 1);
        assert!(tracked[0].interval.has_tag("third"));
        assert_eq!(tracked[1].id, 2);
        assert!(tracked[1].interval.has_tag("second"));
        assert_eq!(tracked[2].id, 3);
        assert!(tracked[2].interval.has_tag("first"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn ids_assigned_newest_first_across_multiple_files() {
        // This is the critical case: id assignment must span file
        // boundaries correctly, with the NEWER MONTH'S intervals getting
        // lower ids regardless of file discovery order.
        let dir = tmp_dir("ids-multi-file");
        write_month_file(
            &dir,
            "2026-09.data",
            &["inc 20260930T070000Z - 20260930T080000Z # september"],
        );
        write_month_file(
            &dir,
            "2026-10.data",
            &["inc 20261001T070000Z - 20261001T080000Z # october"],
        );
        write_month_file(
            &dir,
            "2026-11.data",
            &["inc 20261101T070000Z - 20261101T080000Z # november"],
        );

        let mut db = Database::open(&dir).unwrap();
        let tracked = db.all_tracked().unwrap();
        assert_eq!(tracked.len(), 3);
        assert_eq!(tracked[0].id, 1);
        assert!(tracked[0].interval.has_tag("november"));
        assert_eq!(tracked[1].id, 2);
        assert!(tracked[1].interval.has_tag("october"));
        assert_eq!(tracked[2].id, 3);
        assert!(tracked[2].interval.has_tag("september"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_interval_is_always_id_one() {
        let dir = tmp_dir("open-is-one");
        write_month_file(
            &dir,
            "2026-10.data",
            &[
                "inc 20261001T070000Z - 20261001T080000Z # closed",
                "inc 20261002T070000Z # open",
            ],
        );

        let mut db = Database::open(&dir).unwrap();
        let latest = db.latest().unwrap().unwrap();
        assert_eq!(latest.id, 1);
        assert!(latest.interval.is_open());
        assert!(latest.interval.has_tag("open"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn add_interval_creates_datafile_keyed_by_start_month() {
        let dir = tmp_dir("add-keyed");
        let mut db = Database::open(&dir).unwrap();

        let iv = Interval::new(Range::new(
            parse_iso("20261015T090000Z").unwrap(),
            parse_iso("20261015T100000Z").unwrap(),
        ));
        db.add_interval(&iv).unwrap();
        db.commit().unwrap();

        assert!(dir.join("2026-10.data").exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn modify_interval_moves_across_month_boundary() {
        // An interval starting in October, modified to start in November,
        // must be removed from 2026-10.data and added to 2026-11.data.
        let dir = tmp_dir("modify-cross-month");
        let mut db = Database::open(&dir).unwrap();

        let original = Interval::new(Range::new(
            parse_iso("20261031T230000Z").unwrap(),
            parse_iso("20261031T235900Z").unwrap(),
        ));
        db.add_interval(&original).unwrap();
        db.commit().unwrap();
        assert!(dir.join("2026-10.data").exists());

        let modified = Interval::new(Range::new(
            parse_iso("20261101T000000Z").unwrap(),
            parse_iso("20261101T005900Z").unwrap(),
        ));
        db.modify_interval(&original, &modified).unwrap();
        db.commit().unwrap();

        let oct_contents = fs::read_to_string(dir.join("2026-10.data")).unwrap_or_default();
        assert!(!oct_contents.contains("20261031T230000Z"), "old interval should be gone from October file");
        let nov_contents = fs::read_to_string(dir.join("2026-11.data")).unwrap();
        assert!(nov_contents.contains("20261101T000000Z"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn tracked_in_range_filters_and_returns_oldest_first() {
        let dir = tmp_dir("range-filter");
        write_month_file(
            &dir,
            "2026-10.data",
            &[
                "inc 20261001T070000Z - 20261001T080000Z # before",
                "inc 20261005T070000Z - 20261005T080000Z # inside1",
                "inc 20261006T070000Z - 20261006T080000Z # inside2",
                "inc 20261010T070000Z - 20261010T080000Z # after",
            ],
        );

        let mut db = Database::open(&dir).unwrap();
        let range = Range::new(
            parse_iso("20261003T000000Z").unwrap(),
            parse_iso("20261008T000000Z").unwrap(),
        );
        let tracked = db.tracked_in_range(&range).unwrap();
        let tags: Vec<&str> = tracked.iter().map(|t| t.interval.tags[0].as_str()).collect();
        assert_eq!(tags, vec!["inside1", "inside2"], "expected chronological (oldest-first) order");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn tracked_by_ids_preserves_requested_order_and_handles_missing() {
        let dir = tmp_dir("by-ids");
        write_month_file(
            &dir,
            "2026-10.data",
            &[
                "inc 20261001T070000Z - 20261001T080000Z # a",
                "inc 20261002T070000Z - 20261002T080000Z # b",
            ],
        );

        let mut db = Database::open(&dir).unwrap();
        // id 1 = newest = "b", id 2 = "a", id 99 doesn't exist.
        let results = db.tracked_by_ids(&[2, 99, 1]).unwrap();
        assert_eq!(results.len(), 3);
        assert!(results[0].as_ref().unwrap().interval.has_tag("a"));
        assert!(results[1].is_none());
        assert!(results[2].as_ref().unwrap().interval.has_tag("b"));

        let _ = fs::remove_dir_all(&dir);
    }
}
