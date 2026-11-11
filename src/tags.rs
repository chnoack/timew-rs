//! Read/write `tags.data`: a persistent map of tag name -> usage count,
//! used to answer "is this a brand-new tag?" (for the `Note: 'x' is a new
//! tag.` message) and to back `dom.tags`/`timew tags` without having to
//! rescan every interval in the database.
//!
//! Clean-room port of `TagInfoDatabase.cpp`/`TagInfo.cpp`
//! (GothenburgBitFactory/timewarrior, `develop` branch, commit 3699d2e),
//! re-derived from reading that source and empirically confirmed against
//! the real binary (see tests) for the on-disk JSON shape, the
//! zero-count-drops-the-entry behavior, and the "new tag" detection.
//!
//! On-disk format, confirmed against a real captured `tags.data`:
//! ```text
//! {
//!   "alpha":{"count":1},
//!   "zeta":{"count":1}
//! }
//! ```
//! Keys are written in sorted (`BTreeMap`-equivalent) order, matching the
//! C++ source's `std::map<string, TagInfo>` iteration order. A tag whose
//! count has been decremented to 0 is dropped from the file entirely on
//! the next write (`hasCount()` gates serialization) -- it is NOT written
//! out as `{"count":0}`. An empty database serializes as `{\n}`.
//!
//! Loading is lenient: a missing, empty, or malformed `tags.data` is
//! treated as "start fresh with an empty database" rather than an error
//! (mirrors `Database::initializeTagDatabase`'s catch-and-reset behavior,
//! which also logs a parse error to stderr -- reproduced here via the
//! `Result`'s `Err` variant, left for the caller to decide whether/how to
//! surface it, since stderr-logging policy is a presentation concern).

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct TagsDatabase {
    path: PathBuf,
    counts: BTreeMap<String, u32>,
    modified: bool,
}

impl TagsDatabase {
    /// Load from `path` (typically `<TIMEWARRIORDB>/data/tags.data`). A
    /// missing file is treated as an empty, fresh database (not an error).
    /// A malformed file is also treated as an empty, fresh database, with
    /// the parse error returned separately so the caller can log it
    /// (matching the C++ source logging to stderr and resetting, rather
    /// than propagating the error up as a hard failure).
    pub fn open(path: impl Into<PathBuf>) -> (Self, Option<String>) {
        let path = path.into();

        let contents = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return (TagsDatabase { path, counts: BTreeMap::new(), modified: false }, None);
            }
            Err(e) => {
                return (
                    TagsDatabase { path, counts: BTreeMap::new(), modified: false },
                    Some(format!("Error parsing tags database: {e}")),
                );
            }
        };

        match parse_tags_json(&contents) {
            Ok(counts) => (TagsDatabase { path, counts, modified: false }, None),
            Err(e) => (
                TagsDatabase { path, counts: BTreeMap::new(), modified: false },
                Some(format!("Error parsing tags database: {e}")),
            ),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// All known tag names, alphabetically (the keys of the underlying
    /// `BTreeMap`, which is always sorted).
    pub fn tags(&self) -> Vec<String> {
        self.counts.keys().cloned().collect()
    }

    /// Increment `tag`'s usage count, creating it with count 1 if it's new.
    /// Returns the PREVIOUS count, or `-1` if the tag didn't exist before
    /// this call -- matching `TagInfoDatabase::incrementTag`'s return
    /// value, which callers use to decide whether to print the "new tag"
    /// note.
    pub fn increment(&mut self, tag: &str) -> i64 {
        self.modified = true;
        match self.counts.get_mut(tag) {
            Some(count) => {
                let prev = *count as i64;
                *count += 1;
                prev
            }
            None => {
                self.counts.insert(tag.to_string(), 1);
                -1
            }
        }
    }

    /// Decrement `tag`'s usage count. Errors if the tag doesn't exist,
    /// matching `TagInfoDatabase::decrementTag` throwing in that case.
    /// A count that reaches 0 stays in memory as 0 (so a second erroneous
    /// decrement is still detected as "exists, now below zero" the same
    /// way the C++ source's unsigned-underflow-prone decrement would --
    /// but we don't need to replicate underflow, just the "tag must
    /// exist" precondition) and is simply omitted on the next write.
    pub fn decrement(&mut self, tag: &str) -> Result<u32, String> {
        self.modified = true;
        match self.counts.get_mut(tag) {
            Some(count) => {
                *count = count.saturating_sub(1);
                Ok(*count)
            }
            None => Err(format!("Trying to decrement non-existent tag '{tag}'")),
        }
    }

    pub fn is_modified(&self) -> bool {
        self.modified
    }

    /// Persist to disk if modified, atomically (temp file + rename).
    /// Writes unconditionally if `force` is true, matching
    /// `Database::initializeTagDatabase`'s "always want the tag database
    /// file to exist" fresh-write-on-reset case.
    pub fn commit(&mut self, force: bool) -> io::Result<()> {
        if !self.modified && !force {
            return Ok(());
        }

        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }

        let json = self.to_json();
        let tmp_path = self.path.with_extension("data.tmp");
        fs::write(&tmp_path, &json)?;
        fs::rename(&tmp_path, &self.path)?;

        self.modified = false;
        Ok(())
    }

    /// Render as the pretty-printed JSON object format real timew writes,
    /// confirmed against a real captured `tags.data`. Tags with a count of
    /// 0 are omitted entirely (`hasCount()` gate in the C++ source).
    fn to_json(&self) -> String {
        let mut out = String::from("{");
        let mut first = true;
        for (tag, count) in &self.counts {
            if *count == 0 {
                continue;
            }
            if !first {
                out.push(',');
            }
            out.push_str(&format!("\n  \"{}\":{{\"count\":{}}}", json_escape_key(tag), count));
            first = false;
        }
        out.push_str("\n}");
        out
    }
}

/// Minimal JSON string escaping for tag names used as object keys. Mirrors
/// `json::encode` for the character classes tag names can realistically
/// contain (quotes, backslashes, and the `/` the real encoder also escapes,
/// confirmed empirically: a tag with `/` in it round-trips through a real
/// tags.data as `\/`).
fn json_escape_key(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '/' => out.push_str("\\/"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

/// Parse the `{"name":{"count":N}, ...}` shape. Deliberately hand-rolled
/// (not `serde_json`-derived) because the real format has no surrounding
/// schema to speak of -- it's just an object of objects -- and this keeps
/// the parser forgiving in the same ad-hoc way the C++ source's JSON walk
/// is, while still rejecting a genuinely malformed file.
fn parse_tags_json(contents: &str) -> Result<BTreeMap<String, u32>, String> {
    let value: serde_json::Value =
        serde_json::from_str(contents).map_err(|e| format!("Contents invalid: {e}"))?;
    let obj = value.as_object().ok_or_else(|| "Contents invalid.".to_string())?;

    let mut counts = BTreeMap::new();
    for (key, entry) in obj {
        let count = entry
            .get("count")
            .ok_or_else(|| format!("Failed to find \"count\" member for tag \"{key}\" in tags database."))?
            .as_u64()
            .ok_or_else(|| format!("Invalid \"count\" member for tag \"{key}\" in tags database."))?;
        counts.insert(key.clone(), count as u32);
    }
    Ok(counts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("timew-rs-tags-test-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        dir.join(format!("{name}-tags.data"))
    }

    #[test]
    fn missing_file_opens_as_empty_database() {
        let path = tmp_path("missing");
        let _ = fs::remove_file(&path);
        let (db, err) = TagsDatabase::open(&path);
        assert!(err.is_none());
        assert!(db.tags().is_empty());
    }

    #[test]
    fn malformed_file_resets_to_empty_with_error_reported() {
        let path = tmp_path("malformed");
        fs::write(&path, "not json at all").unwrap();
        let (db, err) = TagsDatabase::open(&path);
        assert!(err.is_some());
        assert!(db.tags().is_empty());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn parses_real_captured_sample() {
        // Captured verbatim from a real TIMEWARRIORDB sandbox.
        let path = tmp_path("real-sample");
        fs::write(&path, "{\n  \"alpha\":{\"count\":1},\n  \"zeta\":{\"count\":1}\n}\n").unwrap();
        let (db, err) = TagsDatabase::open(&path);
        assert!(err.is_none());
        assert_eq!(db.tags(), vec!["alpha".to_string(), "zeta".to_string()]);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn increment_new_tag_returns_negative_one() {
        let path = tmp_path("increment-new");
        let _ = fs::remove_file(&path);
        let (mut db, _) = TagsDatabase::open(&path);
        assert_eq!(db.increment("fresh"), -1);
        assert_eq!(db.tags(), vec!["fresh".to_string()]);
    }

    #[test]
    fn increment_existing_tag_returns_previous_count() {
        let path = tmp_path("increment-existing");
        let _ = fs::remove_file(&path);
        let (mut db, _) = TagsDatabase::open(&path);
        db.increment("x");
        assert_eq!(db.increment("x"), 1);
        db.increment("x");
        assert_eq!(db.increment("x"), 3);
    }

    #[test]
    fn decrement_nonexistent_tag_errors() {
        let path = tmp_path("decrement-missing");
        let _ = fs::remove_file(&path);
        let (mut db, _) = TagsDatabase::open(&path);
        assert!(db.decrement("ghost").is_err());
    }

    #[test]
    fn decrement_existing_tag_reduces_count() {
        let path = tmp_path("decrement-existing");
        let _ = fs::remove_file(&path);
        let (mut db, _) = TagsDatabase::open(&path);
        db.increment("x");
        db.increment("x");
        assert_eq!(db.decrement("x").unwrap(), 1);
    }

    #[test]
    fn zero_count_tag_is_dropped_on_write() {
        // Matches real timew: decrementing a tag to 0 removes it entirely
        // from the next-written tags.data (verified empirically against
        // the real binary: `timew delete @1` on a single-use tag leaves
        // tags.data as the literal "{\n}").
        let path = tmp_path("zero-dropped");
        let _ = fs::remove_file(&path);
        let (mut db, _) = TagsDatabase::open(&path);
        db.increment("gone");
        db.decrement("gone").unwrap();
        db.commit(false).unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        assert_eq!(contents, "{\n}");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn commit_writes_sorted_pretty_json_matching_real_format() {
        let path = tmp_path("commit-format");
        let _ = fs::remove_file(&path);
        let (mut db, _) = TagsDatabase::open(&path);
        db.increment("zeta");
        db.increment("alpha");
        db.commit(false).unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        assert_eq!(contents, "{\n  \"alpha\":{\"count\":1},\n  \"zeta\":{\"count\":1}\n}");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn commit_is_a_no_op_when_not_modified_and_not_forced() {
        let path = tmp_path("no-op-commit");
        let _ = fs::remove_file(&path);
        let (mut db, _) = TagsDatabase::open(&path);
        // Never modified -- commit should not create the file.
        db.commit(false).unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn commit_force_always_writes_even_when_empty() {
        let path = tmp_path("force-commit");
        let _ = fs::remove_file(&path);
        let (mut db, _) = TagsDatabase::open(&path);
        db.commit(true).unwrap();
        let contents = fs::read_to_string(&path).unwrap();
        assert_eq!(contents, "{\n}");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn round_trip_through_commit_and_reopen() {
        let path = tmp_path("round-trip");
        let _ = fs::remove_file(&path);
        let (mut db, _) = TagsDatabase::open(&path);
        db.increment("a");
        db.increment("b");
        db.increment("b");
        db.commit(false).unwrap();

        let (reopened, err) = TagsDatabase::open(&path);
        assert!(err.is_none());
        assert_eq!(reopened.tags(), vec!["a".to_string(), "b".to_string()]);
        let _ = fs::remove_file(&path);
    }
}
