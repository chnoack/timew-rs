//! The database: all `YYYY-MM.data` files of a data directory, plus the
//! tag counts in `tags.data`.
//!
//! IDs are never stored. Each time the data is read, intervals are numbered
//! newest first: `@1` is the most recent one. Files are named by the
//! (local) month an interval starts in, so reading the files in reverse
//! name order, and each file's lines in reverse, gives newest first.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::datafile::Datafile;
use crate::interval::Interval;
use crate::range::{Epoch, Range};
use crate::tags::TagsDatabase;

pub struct Database {
    data_dir: PathBuf,
    files: Vec<Datafile>,
    tags: TagsDatabase,
    /// Parsed intervals, newest first; dropped on every change.
    cache: Option<Vec<Interval>>,
}

/// An interval together with its current ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tracked {
    pub id: u32,
    pub interval: Interval,
    /// A piece of a running interval split by exclusions. It is not stored
    /// as such; see `session::Session::flatten_latest`.
    pub synthetic: bool,
}

impl Database {
    /// Open a data directory for reading. A missing directory is an empty
    /// database. `tags.data` is read but never repaired.
    pub fn open(data_dir: impl Into<PathBuf>) -> io::Result<Self> {
        let data_dir = data_dir.into();
        let files = discover_datafiles(&data_dir)?;
        let (tags, _) = TagsDatabase::open(data_dir.join("tags.data"));
        Ok(Database { data_dir, files, tags, cache: None })
    }

    /// Open a data directory for the command line: like [`open`], but a
    /// missing or unreadable `tags.data` is rebuilt from the intervals (and
    /// written). Returns the database, a notice for stdout and an error
    /// message for stderr about the repair.
    ///
    /// [`open`]: Database::open
    pub fn open_and_repair(data_dir: impl Into<PathBuf>) -> Result<(Self, String, Option<String>), String> {
        let data_dir = data_dir.into();
        let tags_path = data_dir.join("tags.data");
        let exists = tags_path.exists();
        let mut db = Database::open(&data_dir).map_err(|e| e.to_string())?;
        let (tags, error) = TagsDatabase::open(&tags_path);
        let mut notice = String::new();
        if exists && error.is_none() {
            db.tags = tags;
            return Ok((db, notice, None));
        }
        db.tags = TagsDatabase::empty(&tags_path);
        let intervals = db.intervals()?.to_vec();
        if !intervals.is_empty() {
            if !exists {
                notice.push_str("Tags database does not exist. ");
            }
            notice.push_str("Recreating from interval data...\n");
            for interval in &intervals {
                for tag in &interval.tags {
                    db.tags.increment(tag);
                }
            }
        }
        db.tags.commit(true).map_err(|e| format!("Could not write {}: {e}", tags_path.display()))?;
        Ok((db, notice, if exists { error } else { None }))
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// Names of the datafiles, oldest first.
    pub fn file_names(&self) -> Vec<String> {
        self.files.iter().filter_map(|f| f.path().file_name()?.to_str().map(str::to_string)).collect()
    }

    /// The tags known to `tags.data`.
    pub fn tag_names(&self) -> Vec<String> {
        self.tags.tags()
    }

    /// All intervals, newest first. They are read once and kept until the
    /// next change.
    pub fn intervals(&mut self) -> Result<&[Interval], String> {
        if self.cache.is_none() {
            let mut result = Vec::new();
            for file in self.files.iter_mut().rev() {
                let lines = file.all_lines().map_err(|e| e.to_string())?;
                // Files are written sorted; sort only if one isn't.
                let mut order: Vec<&String> = lines.iter().collect();
                if !lines.is_sorted() {
                    order.sort();
                }
                for line in order.into_iter().rev() {
                    if let Some(interval) = Interval::parse(line)? {
                        result.push(interval);
                    }
                }
            }
            self.cache = Some(result);
        }
        Ok(self.cache.as_deref().unwrap_or_default())
    }

    pub fn is_empty(&mut self) -> Result<bool, String> {
        Ok(self.intervals()?.is_empty())
    }

    /// The most recent interval, if any.
    pub fn latest(&mut self) -> Result<Option<Interval>, String> {
        Ok(self.intervals()?.first().cloned())
    }

    /// All intervals with their IDs, newest first (no exclusion handling).
    pub fn all_tracked(&mut self) -> Result<Vec<Tracked>, String> {
        Ok(self
            .intervals()?
            .iter()
            .enumerate()
            .map(|(i, interval)| Tracked { id: i as u32 + 1, interval: interval.clone(), synthetic: false })
            .collect())
    }

    /// Intervals intersecting `range` (all for an unbounded range), oldest
    /// first.
    pub fn tracked_in_range(&mut self, range: &Range) -> Result<Vec<Tracked>, String> {
        let mut out: Vec<Tracked> = self
            .all_tracked()?
            .into_iter()
            .filter(|t| range.is_unbounded() || t.interval.range.intersects(range))
            .collect();
        out.reverse();
        Ok(out)
    }

    /// The intervals with the given IDs, `None` for unknown ones.
    pub fn tracked_by_ids(&mut self, ids: &[u32]) -> Result<Vec<Option<Tracked>>, String> {
        let all = self.all_tracked()?;
        Ok(ids.iter().map(|id| all.iter().find(|t| t.id == *id).cloned()).collect())
    }

    /// Add an interval. Returns the tags that were not known before.
    pub fn add_interval(&mut self, interval: &Interval) -> Result<Vec<String>, String> {
        if interval.range.is_ended() && interval.range.start > interval.range.end {
            return Err("Cannot add an interval that ends before it starts.".to_string());
        }
        let idx = self.datafile_index_for(interval.range.start);
        self.files[idx].add_interval(interval)?;
        self.cache = None;
        let mut new_tags = Vec::new();
        for tag in &interval.tags {
            if self.tags.increment(tag) == -1 {
                new_tags.push(tag.clone());
            }
        }
        Ok(new_tags)
    }

    /// Delete `interval`. It is looked for in the file of the month it
    /// starts in, and if it isn't there (a file written by another tool,
    /// or by one that used other month boundaries), in all others.
    pub fn delete_interval(&mut self, interval: &Interval) -> Result<(), String> {
        let expected = self.datafile_index_for(interval.range.start);
        let mut idx = None;
        if self.files[expected].contains(interval)? {
            idx = Some(expected);
        } else {
            for i in 0..self.files.len() {
                if i != expected && self.files[i].contains(interval)? {
                    idx = Some(i);
                    break;
                }
            }
        }
        let Some(idx) = idx else {
            return Err(format!("Could not find the entry '{}' in any data file.", interval.serialize()));
        };
        self.files[idx].delete_interval(interval)?;
        self.cache = None;
        for tag in &interval.tags {
            // A tag missing from tags.data is simply not counted.
            let _ = self.tags.decrement(tag);
        }
        Ok(())
    }

    /// Replace `from` with `to` (either may be empty).
    pub fn modify_interval(&mut self, from: &Interval, to: &Interval) -> Result<Vec<String>, String> {
        if !from.is_empty() {
            self.delete_interval(from)?;
        }
        if !to.is_empty() {
            return self.add_interval(to);
        }
        Ok(Vec::new())
    }

    /// Write all changed files.
    pub fn commit(&mut self) -> Result<(), String> {
        for file in &mut self.files {
            file.commit().map_err(|e| format!("Could not write to data file {}: {e}", file.path().display()))?;
        }
        if self.tags.is_modified() {
            self.tags.commit(false).map_err(|e| format!("Could not write tags database: {e}"))?;
        }
        Ok(())
    }

    fn datafile_index_for(&mut self, start: Epoch) -> usize {
        let (year, month) = crate::interval::year_month_of(start);
        let name = format!("{year:04}-{month:02}.data");
        if let Some(idx) = self.files.iter().position(|f| f.path().file_name().and_then(|n| n.to_str()) == Some(&name)) {
            return idx;
        }
        self.files.push(Datafile::new(self.data_dir.join(&name)));
        self.files.sort_by(|a, b| a.path().cmp(b.path()));
        self.files.iter().position(|f| f.path().file_name().and_then(|n| n.to_str()) == Some(&name)).unwrap()
    }
}

fn discover_datafiles(data_dir: &Path) -> io::Result<Vec<Datafile>> {
    let entries = match fs::read_dir(data_dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut names: Vec<String> = Vec::new();
    for entry in entries {
        let entry = entry?;
        if let Some(name) = entry.file_name().to_str()
            && is_month_datafile_name(name)
        {
            names.push(name.to_string());
        }
    }
    names.sort();
    Ok(names.into_iter().map(|name| Datafile::new(data_dir.join(name))).collect())
}

/// `YYYY-MM.data`
fn is_month_datafile_name(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() == 12
        && name.ends_with(".data")
        && b[4] == b'-'
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[5..7].iter().all(u8::is_ascii_digit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datetime::make_local;
    use crate::interval::parse_iso;

    fn tmp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("timew-cli-db-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_month_file(dir: &Path, name: &str, lines: &[&str]) {
        fs::write(dir.join(name), lines.join("\n") + "\n").unwrap();
    }

    fn iv(start: &str, end: &str, tag: &str) -> Interval {
        let mut i = Interval::new(Range::new(parse_iso(start).unwrap(), if end.is_empty() { 0 } else { parse_iso(end).unwrap() }));
        if !tag.is_empty() {
            i.add_tag(tag);
        }
        i
    }

    #[test]
    fn discovers_only_month_shaped_data_files() {
        let dir = tmp_dir("discover");
        write_month_file(&dir, "2026-10.data", &["inc 20261001T070000Z - 20261001T080000Z # a"]);
        fs::write(dir.join("tags.data"), "{}").unwrap();
        fs::write(dir.join("undo.data"), "").unwrap();
        fs::write(dir.join("notes.txt"), "").unwrap();
        fs::write(dir.join("x2026-10.data"), "").unwrap();
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
    fn ids_assigned_newest_first_across_files() {
        let dir = tmp_dir("ids");
        write_month_file(&dir, "2026-09.data", &["inc 20260915T070000Z - 20260915T080000Z # old"]);
        write_month_file(
            &dir,
            "2026-10.data",
            &["inc 20261002T070000Z - 20261002T080000Z # b", "inc 20261001T070000Z - 20261001T080000Z # a"],
        );
        let mut db = Database::open(&dir).unwrap();
        let all = db.all_tracked().unwrap();
        let tags: Vec<(u32, &str)> = all.iter().map(|t| (t.id, t.interval.tags[0].as_str())).collect();
        assert_eq!(tags, vec![(1, "b"), (2, "a"), (3, "old")]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn add_interval_creates_datafile_keyed_by_local_start_month() {
        let dir = tmp_dir("add-keyed");
        let mut db = Database::open(&dir).unwrap();
        let start = make_local(2026, 10, 15, 9, 0, 0);
        let new_tags = db.add_interval(&{
            let mut i = Interval::new(Range::new(start, start + 3600));
            i.add_tag("x");
            i
        });
        assert_eq!(new_tags.unwrap(), vec!["x".to_string()]);
        db.commit().unwrap();
        assert!(dir.join("2026-10.data").exists());
        assert!(fs::read_to_string(dir.join("tags.data")).unwrap().contains("\"x\":{\"count\":1}"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn modify_interval_moves_across_month_boundary() {
        let dir = tmp_dir("modify-cross-month");
        let mut db = Database::open(&dir).unwrap();
        let oct = make_local(2026, 10, 31, 23, 0, 0);
        let nov = make_local(2026, 11, 1, 0, 30, 0);
        let original = Interval::new(Range::new(oct, oct + 600));
        db.add_interval(&original).unwrap();
        db.commit().unwrap();
        let modified = Interval::new(Range::new(nov, nov + 600));
        db.modify_interval(&original, &modified).unwrap();
        db.commit().unwrap();
        assert!(!dir.join("2026-10.data").exists(), "an emptied file is removed");
        assert!(dir.join("2026-11.data").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn tracked_in_range_filters_and_returns_oldest_first() {
        let dir = tmp_dir("in-range");
        write_month_file(
            &dir,
            "2026-10.data",
            &[
                "inc 20261001T070000Z - 20261001T080000Z # a",
                "inc 20261002T070000Z - 20261002T080000Z # b",
                "inc 20261003T070000Z - 20261003T080000Z # c",
            ],
        );
        let mut db = Database::open(&dir).unwrap();
        let range = Range::new(parse_iso("20261002T000000Z").unwrap(), parse_iso("20261004T000000Z").unwrap());
        let got: Vec<String> = db.tracked_in_range(&range).unwrap().into_iter().map(|t| t.interval.tags[0].clone()).collect();
        assert_eq!(got, vec!["b", "c"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn repairs_missing_tags_database() {
        let dir = tmp_dir("repair");
        write_month_file(&dir, "2026-10.data", &["inc 20261001T070000Z - 20261001T080000Z # a b"]);
        let (_, msg, err) = Database::open_and_repair(&dir).unwrap();
        assert_eq!(msg, "Tags database does not exist. Recreating from interval data...\n");
        assert_eq!(err, None);
        let tags = fs::read_to_string(dir.join("tags.data")).unwrap();
        assert_eq!(tags, "{\n  \"a\":{\"count\":1},\n  \"b\":{\"count\":1}\n}");
        let (_, msg, _) = Database::open_and_repair(&dir).unwrap();
        assert_eq!(msg, "");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_and_open_intervals() {
        let dir = tmp_dir("delete");
        let mut db = Database::open(&dir).unwrap();
        let a = iv("20261001T070000Z", "20261001T080000Z", "a");
        let b = iv("20261001T090000Z", "", "b");
        db.add_interval(&a).unwrap();
        db.add_interval(&b).unwrap();
        assert!(db.latest().unwrap().unwrap().is_open());
        db.delete_interval(&b).unwrap();
        assert_eq!(db.latest().unwrap().unwrap(), a);
        assert!(db.delete_interval(&b).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn deletes_lines_written_differently_and_misplaced_entries() {
        let dir = tmp_dir("lenient-delete");
        // tags out of order, a tag that should be quoted, extra spaces, CRLF
        write_month_file(
            &dir,
            "2026-10.data",
            &[
                "inc 20261001T070000Z - 20261001T080000Z # zeta alpha",
                "inc 20261002T070000Z  -  20261002T080000Z  #  client-a\r",
                "inc 20261003T070000Z - 20261003T080000Z # keep",
            ],
        );
        // an entry starting in November, but stored in the October file
        let mut nov = std::fs::read_to_string(dir.join("2026-10.data")).unwrap();
        let nov_start = crate::interval::format_iso(make_local(2026, 11, 2, 9, 0, 0));
        let nov_end = crate::interval::format_iso(make_local(2026, 11, 2, 10, 0, 0));
        nov.push_str(&format!("inc {nov_start} - {nov_end} # misplaced\n"));
        fs::write(dir.join("2026-10.data"), nov).unwrap();

        let mut db = Database::open(&dir).unwrap();
        let all = db.intervals().unwrap().to_vec();
        assert_eq!(all.len(), 4);
        for interval in all.iter().filter(|i| !i.has_tag("keep")) {
            db.delete_interval(interval).unwrap();
        }
        db.commit().unwrap();
        let left = fs::read_to_string(dir.join("2026-10.data")).unwrap();
        assert_eq!(left, "inc 20261003T070000Z - 20261003T080000Z # keep\n", "other lines are kept as they are");
        let ghost = iv("20261005T070000Z", "20261005T080000Z", "ghost");
        assert!(db.delete_interval(&ghost).unwrap_err().contains("Could not find the entry"));
        let _ = fs::remove_dir_all(&dir);
    }

}
