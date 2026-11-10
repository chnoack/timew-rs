//! Read/write a single `YYYY-MM.data` file: one serialized `Interval` line
//! per entry.
//!
//! Clean-room port of `Datafile.cpp` (GothenburgBitFactory/timewarrior,
//! `develop` branch, commit 3699d2e). Key behaviors confirmed by reading
//! that source (not guessed):
//!
//!   - Deletion matches by exact serialized string, not position/index.
//!     Two intervals that happen to serialize identically are
//!     indistinguishable; deleting one removes the first match.
//!   - On commit, lines are re-sorted by plain string order before writing.
//!     Since the ISO timestamp prefix (`YYYYMMDDTHHMMSSZ`) is a
//!     lexicographically-sortable fixed-width format, string-sort equals
//!     chronological-sort -- this is relied upon, not incidental.
//!   - If all lines are deleted, the file is removed entirely rather than
//!     written out empty (`AtomicFile::remove()` in `commit()`).
//!   - Writes are atomic: the real implementation writes a temp file and
//!     renames it over the original (`AtomicFile`). We replicate that here.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::interval::Interval;

pub struct Datafile {
    path: PathBuf,
    lines: Vec<String>,
    loaded: bool,
    dirty: bool,
}

impl Datafile {
    /// `path` must be of the form `.../YYYY-MM.data` (the year/month are not
    /// otherwise validated here -- `Database` is responsible for naming
    /// files correctly; this type just reads/writes whatever path it's
    /// given).
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Datafile { path: path.into(), lines: Vec::new(), loaded: false, dirty: false }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn ensure_loaded(&mut self) -> io::Result<()> {
        if self.loaded {
            return Ok(());
        }
        match fs::read_to_string(&self.path) {
            Ok(contents) => {
                self.lines = contents.lines().map(str::to_string).collect();
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                self.lines = Vec::new();
            }
            Err(e) => return Err(e),
        }
        self.loaded = true;
        Ok(())
    }

    /// All lines, newest-last (on-disk order is ascending-by-sort after a
    /// commit, but before a commit newly-added lines are simply appended --
    /// callers needing a guaranteed chronological view should sort).
    pub fn all_lines(&mut self) -> io::Result<&[String]> {
        self.ensure_loaded()?;
        Ok(&self.lines)
    }

    /// Append a new interval's serialized line. Mirrors
    /// `Datafile::addInterval`'s encode/decode round-trip safety check: we
    /// refuse to add a line that doesn't parse back to an equal interval,
    /// which would indicate a serialization bug rather than bad input.
    pub fn add_interval(&mut self, interval: &Interval) -> Result<(), String> {
        self.ensure_loaded().map_err(|e| e.to_string())?;

        let serialized = interval.serialize();
        match Interval::parse(&serialized) {
            Ok(Some(round_tripped)) if &round_tripped == interval => {}
            Ok(Some(round_tripped)) => {
                return Err(format!(
                    "Internal error. Failed encode / decode check.\n  {:?}\nis not equal to:\n  {:?}",
                    interval, round_tripped
                ));
            }
            Ok(None) => return Err("Internal error. Serialized interval parsed as blank.".to_string()),
            Err(e) => return Err(format!("Internal error. Failed encode / decode check: {e}")),
        }

        self.lines.push(serialized);
        self.dirty = true;
        Ok(())
    }

    /// Remove the first line equal to `interval`'s serialization. Errors if
    /// no matching line is found (mirrors the C++ source throwing in this
    /// case -- callers are expected to only delete intervals they know
    /// exist).
    pub fn delete_interval(&mut self, interval: &Interval) -> Result<(), String> {
        self.ensure_loaded().map_err(|e| e.to_string())?;

        let serialized = interval.serialize();
        let pos = self.lines.iter().position(|l| l == &serialized).ok_or_else(|| {
            format!("Datafile::delete_interval failed to find '{serialized}'")
        })?;
        self.lines.remove(pos);
        self.dirty = true;
        Ok(())
    }

    /// Write pending changes to disk, atomically (write to a temp file in
    /// the same directory, then rename over the original). If deletions
    /// have emptied the file, the file is removed instead of written empty.
    pub fn commit(&mut self) -> io::Result<()> {
        if !self.dirty {
            return Ok(());
        }

        if self.lines.is_empty() {
            match fs::remove_file(&self.path) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
            self.dirty = false;
            return Ok(());
        }

        // String-sort == chronological-sort, since lines begin with the
        // fixed-width `YYYYMMDDTHHMMSSZ` timestamp.
        self.lines.sort();

        let mut contents = String::new();
        for line in &self.lines {
            contents.push_str(line);
            contents.push('\n');
        }

        atomic_write(&self.path, &contents)?;
        self.dirty = false;
        Ok(())
    }
}

/// Write `contents` to `path` atomically: write to a sibling temp file, then
/// rename it into place. A rename within the same directory/filesystem is
/// atomic on POSIX systems, matching the durability intent of `AtomicFile`
/// in the C++ source.
fn atomic_write(path: &Path, contents: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let tmp_path = path.with_extension(format!(
        "{}.tmp",
        path.extension().and_then(|e| e.to_str()).unwrap_or("data")
    ));

    fs::write(&tmp_path, contents)?;
    fs::rename(&tmp_path, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::range::Range;
    use std::fs;

    fn tmp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("timew-rs-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn mk_interval(start_iso: &str, end_iso: &str, tag: &str) -> Interval {
        let mut iv = Interval::new(Range::new(
            crate::interval::parse_iso(start_iso).unwrap(),
            crate::interval::parse_iso(end_iso).unwrap(),
        ));
        iv.add_tag(tag);
        iv
    }

    #[test]
    fn add_then_commit_writes_sorted_lines() {
        let dir = tmp_dir("add-commit");
        let path = dir.join("2026-10.data");
        let mut df = Datafile::new(&path);

        // Add out of chronological order to verify commit() sorts.
        df.add_interval(&mk_interval("20261002T110000Z", "20261002T130000Z", "second")).unwrap();
        df.add_interval(&mk_interval("20261001T070000Z", "20261001T080000Z", "first")).unwrap();
        df.commit().unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("first"));
        assert!(lines[1].contains("second"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_removes_matching_line() {
        let dir = tmp_dir("delete");
        let path = dir.join("2026-10.data");
        let mut df = Datafile::new(&path);

        let a = mk_interval("20261001T070000Z", "20261001T080000Z", "keep");
        let b = mk_interval("20261002T070000Z", "20261002T080000Z", "remove");
        df.add_interval(&a).unwrap();
        df.add_interval(&b).unwrap();
        df.commit().unwrap();

        let mut df2 = Datafile::new(&path);
        df2.delete_interval(&b).unwrap();
        df2.commit().unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("keep"));
        assert!(!contents.contains("remove"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn deleting_last_line_removes_file() {
        let dir = tmp_dir("delete-last");
        let path = dir.join("2026-10.data");
        let mut df = Datafile::new(&path);

        let a = mk_interval("20261001T070000Z", "20261001T080000Z", "only");
        df.add_interval(&a).unwrap();
        df.commit().unwrap();
        assert!(path.exists());

        let mut df2 = Datafile::new(&path);
        df2.delete_interval(&a).unwrap();
        df2.commit().unwrap();
        assert!(!path.exists(), "file should be removed once empty, not written out blank");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_nonexistent_interval_errors() {
        let dir = tmp_dir("delete-missing");
        let path = dir.join("2026-10.data");
        let mut df = Datafile::new(&path);

        let a = mk_interval("20261001T070000Z", "20261001T080000Z", "ghost");
        assert!(df.delete_interval(&a).is_err());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn reading_nonexistent_file_yields_empty_lines() {
        let dir = tmp_dir("read-missing");
        let path = dir.join("2026-10.data");
        let mut df = Datafile::new(&path);
        assert_eq!(df.all_lines().unwrap(), &[] as &[String]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn round_trip_through_real_file_layout() {
        // Mirrors a line captured from the real timew data file.
        let dir = tmp_dir("round-trip");
        let path = dir.join("2026-10.data");
        fs::write(&path, "inc 20261002T110000Z - 20261002T133000Z # MISC\n").unwrap();

        let mut df = Datafile::new(&path);
        let lines = df.all_lines().unwrap().to_vec();
        assert_eq!(lines, vec!["inc 20261002T110000Z - 20261002T133000Z # MISC".to_string()]);

        let _ = fs::remove_dir_all(&dir);
    }
}
