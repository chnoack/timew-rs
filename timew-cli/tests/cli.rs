//! End-to-end tests of the `timew-cli` binary in throwaway databases. The
//! expected outputs were captured from the original `timew` (1.10.0-dev)
//! with `TZ=Europe/Berlin` and an 80 column terminal. (`timew-cli` has no
//! `undo` and writes no `undo.data`.)

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Db {
    dir: PathBuf,
}

static COUNTER: AtomicUsize = AtomicUsize::new(0);

impl Db {
    fn new() -> Db {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("timew-cli-cli-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Db { dir }
    }

    /// Run with the given arguments; returns (status, stdout, stderr).
    fn run(&self, args: &[&str]) -> (i32, String, String) {
        let out = Command::new(env!("CARGO_BIN_EXE_timew-cli"))
            .args(args)
            .env("TIMEWARRIORDB", &self.dir)
            .env("TZ", "Europe/Berlin")
            .env("COLUMNS", "80")
            .output()
            .unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }

    /// Run and expect success; returns stdout.
    fn ok(&self, args: &[&str]) -> String {
        let (status, out, err) = self.run(args);
        assert_eq!(status, 0, "timew {args:?} failed: {err}");
        out
    }

    /// Run and expect failure; returns stderr.
    fn fail(&self, args: &[&str]) -> String {
        let (status, _, err) = self.run(args);
        assert_ne!(status, 0, "timew {args:?} should have failed");
        err
    }

    fn file(&self, name: &str) -> String {
        fs::read_to_string(self.dir.join(name)).unwrap_or_default()
    }

    fn path(&self) -> &Path {
        &self.dir
    }
}

impl Drop for Db {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn new_database_is_set_up() {
    let db = Db::new();
    let (status, out, _) = db.run(&[]);
    assert_eq!(status, 1);
    assert_eq!(out, "There is no active time tracking.\n");
    assert_eq!(db.file("timewarrior.cfg"), "reports.summary.ids = yes\nreports.summary.annotations = yes\n");
    assert!(db.path().join("data").is_dir());
    assert!(db.path().join("extensions").is_dir());
    assert_eq!(db.file("data/tags.data"), "{\n}");
}

#[test]
fn track_summary_and_files() {
    let db = Db::new();
    assert_eq!(
        db.ok(&["track", "2025-03-03T08:00", "-", "2025-03-03T09:30", "foo", "bar"]),
        "Note: 'bar' is a new tag.\nNote: 'foo' is a new tag.\nRecorded bar foo\n  Started 2025-03-03T08:00:00\n  Ended              09:30:00\n  Total               1:30:00\n"
    );
    assert_eq!(
        db.ok(&["track", "2025-03-03T10:00", "-", "2025-03-03T11:00", "client a"]),
        "Note: '\"client a\"' is a new tag.\nRecorded \"client a\"\n  Started 2025-03-03T10:00:00\n  Ended              11:00:00\n  Total               1:00:00\n"
    );
    assert_eq!(
        db.ok(&["summary", "2025-03-03"]),
        concat!(
            "\n",
            "Wk  Date       Day ID Tags     Annotation    Start      End    Time   Total\n",
            "--- ---------- --- -- -------- ---------- -------- -------- ------- -------\n",
            "W10 2025-03-03 Mon @2 bar, foo             8:00:00  9:30:00 1:30:00\n",
            "                   @1 client a            10:00:00 11:00:00 1:00:00 2:30:00\n",
            "\n",
            "                                                                    2:30:00\n",
            "\n",
        )
    );
    assert_eq!(
        db.ok(&["day", "2025-03-03"]),
        concat!(
            "\n",
            "Mon  3 0    1    2    3    4    5    6    7    bar foo   clien11   12   13   14   15   16   17   18   19   20   21   22   23   \n",
            "                                                         t a                                                                   \n",
            "\n",
            "       Tracked         2:30:00\n",
            "       Available      21:30:00\n",
            "       Total          24:00:00\n",
            "\n",
        )
    );
}

#[test]
fn overlaps_and_adjust() {
    let db = Db::new();
    db.ok(&["track", "2025-03-03T08:00", "-", "2025-03-03T09:30", "foo", "bar"]);
    db.ok(&["track", "2025-03-03T10:00", "-", "2025-03-03T11:00", "client a"]);
    assert_eq!(
        db.fail(&["track", "2025-03-03T09:00", "-", "2025-03-03T10:30", "baz"]),
        "You cannot overlap intervals. Correct the start/end time, or specify the :adjust hint.\n"
    );
    db.ok(&["track", "2025-03-03T09:00", "-", "2025-03-03T10:30", "baz", ":adjust"]);
    assert_eq!(
        db.ok(&["export"]),
        concat!(
            "[\n",
            "{\"id\":3,\"start\":\"20250303T070000Z\",\"end\":\"20250303T080000Z\",\"tags\":[\"bar\",\"foo\"]},\n",
            "{\"id\":2,\"start\":\"20250303T080000Z\",\"end\":\"20250303T093000Z\",\"tags\":[\"baz\"]},\n",
            "{\"id\":1,\"start\":\"20250303T093000Z\",\"end\":\"20250303T100000Z\",\"tags\":[\"client a\"]}\n",
            "]\n",
        )
    );
    assert_eq!(
        db.file("data/2025-03.data"),
        concat!(
            "inc 20250303T070000Z - 20250303T080000Z # bar foo\n",
            "inc 20250303T080000Z - 20250303T093000Z # baz\n",
            "inc 20250303T093000Z - 20250303T100000Z # \"client a\"\n",
        )
    );
    assert_eq!(
        db.file("data/tags.data"),
        "{\n  \"bar\":{\"count\":1},\n  \"baz\":{\"count\":1},\n  \"client a\":{\"count\":1},\n  \"foo\":{\"count\":1}\n}"
    );
}

#[test]
fn stopping_some_tags() {
    let db = Db::new();
    db.ok(&["start", "2025-03-03T12:00", "x", "y"]);
    let out = db.ok(&["stop", "2025-03-03T12:30", "x"]);
    assert!(out.starts_with("Recorded x y\n  Started 2025-03-03T12:00:00\n  Ended                 30:00\n  Total               0:30:00\n\nTracking y\n"));
    assert_eq!(
        db.ok(&["export", "2025-03-03T12:00", "-", "2025-03-03T14:00"]),
        concat!(
            "[\n",
            "{\"id\":2,\"start\":\"20250303T110000Z\",\"end\":\"20250303T113000Z\",\"tags\":[\"x\",\"y\"]},\n",
            "{\"id\":1,\"start\":\"20250303T113000Z\",\"tags\":[\"y\"]}\n",
            "]\n",
        )
    );
    assert_eq!(db.fail(&["stop", "2025-03-03T13:00", "z"]), "The current interval does not have the 'z' tag.\n");
    db.ok(&["stop", "2025-03-03T13:00"]);
    assert_eq!(db.fail(&["stop"]), "There is no active time tracking.\n");
}

#[test]
fn no_undo_and_no_journal() {
    let db = Db::new();
    db.ok(&["track", "2025-03-03T08:00", "-", "2025-03-03T09:00", "foo"]);
    db.ok(&["config", "name", "value", ":yes"]);
    assert!(!db.path().join("data/undo.data").exists());
    assert_eq!(db.fail(&["undo"]), "'undo' is not a timew command. See 'timew help'.\n");
}

#[test]
fn editing_commands() {
    let db = Db::new();
    db.ok(&["track", "2025-03-03T08:00", "-", "2025-03-03T09:00", "foo"]);
    db.ok(&["track", "2025-03-03T10:00", "-", "2025-03-03T11:00", "bar"]);
    assert_eq!(db.ok(&["tag", "@1", "@2", "x"]), "Note: 'x' is a new tag.\nAdded x to @2\nAdded x to @1\n");
    assert_eq!(db.ok(&["untag", "@2", "foo"]), "Removed foo from @2\n");
    assert_eq!(db.ok(&["retag", "@1", "z"]), "Note: 'z' is a new tag.\nRetagged @1 as z\n");
    assert_eq!(db.ok(&["annotate", "@2", "a note"]), "Annotated @2 with \"a note\"\n");
    assert_eq!(db.ok(&["lengthen", "@1", "30min"]), "Lengthened @1 by 0:30:00\n");
    assert_eq!(db.ok(&["shorten", "@1", "10min"]), "Shortened @1 by 0:10:00\n");
    assert_eq!(db.ok(&["move", "@1", "2025-03-03T12:00"]), "Moved @1 to 2025-03-03T12:00:00\n");
    assert_eq!(db.ok(&["resize", "@1", "2h"]), "Resized @1 to 2:00:00\n");
    assert_eq!(db.ok(&["split", "@1"]), "Split @1\n");
    assert_eq!(db.ok(&["join", "@1", "@2"]), "Joined @1 and @2\n");
    db.ok(&["modify", "end", "@1", "2025-03-03T15:00"]);
    db.ok(&["modify", "start", "@2", "2025-03-03T07:30"]);
    assert_eq!(
        db.ok(&["export"]),
        concat!(
            "[\n",
            "{\"id\":2,\"start\":\"20250303T063000Z\",\"end\":\"20250303T080000Z\",\"tags\":[\"x\"],\"annotation\":\"a note\"},\n",
            "{\"id\":1,\"start\":\"20250303T110000Z\",\"end\":\"20250303T140000Z\",\"tags\":[\"z\"]}\n",
            "]\n",
        )
    );
    assert_eq!(db.ok(&["delete", "@1", "@2"]), "Deleted @2\nDeleted @1\n");
    assert_eq!(db.fail(&["delete", "@1"]), "ID '@1' does not correspond to any tracking.\n");
    assert_eq!(db.fail(&["delete"]), "IDs must be specified. See 'timew help delete'.\n");
}

#[test]
fn exclusions_split_tracked_time() {
    let db = Db::new();
    db.ok(&["config", "exclusions.monday", "<8:00 12:00-13:00 >17:00", ":yes"]);
    db.ok(&["track", "2025-03-03T07:00", "-", "2025-03-03T18:00", "work"]);
    assert_eq!(
        db.ok(&["export"]),
        concat!(
            "[\n",
            // Only the lunch break lies inside the interval, so only it is cut out.
            "{\"id\":2,\"start\":\"20250303T060000Z\",\"end\":\"20250303T110000Z\",\"tags\":[\"work\"]},\n",
            "{\"id\":1,\"start\":\"20250303T120000Z\",\"end\":\"20250303T170000Z\",\"tags\":[\"work\"]}\n",
            "]\n",
        )
    );
    let gaps = db.ok(&["gaps", "2025-03-03", ":blank"]);
    assert!(gaps.contains(" 8:00:00 12:00:00"), "{gaps}");
}

#[test]
fn configuration() {
    let db = Db::new();
    let out = db.ok(&["config", "name", "value", ":yes"]);
    assert!(out.starts_with("Config file '") && out.ends_with("' modified.\n"));
    assert!(db.ok(&["config"]).contains("\nname = value\n"));
    let (status, _, _) = db.run(&["config", "name", "value", ":yes"]);
    assert_eq!(status, 1);
    assert_eq!(db.fail(&["config", "debug", ":yes"]), "No entry named 'debug' found.\n");
    db.ok(&["config", "tags.foo.description", "A foo", ":yes"]);
    let show = db.ok(&["show"]);
    assert!(show.contains("define tags:\n  foo:\n    description = A foo\n"), "{show}");
    assert_eq!(db.ok(&["get", "dom.rc.tags.foo.description", "dom.rc.nothing"]), "A foo \n");
    db.ok(&["config", "name", ":yes"]);
    assert!(!db.file("timewarrior.cfg").contains("name"));
    assert_eq!(db.ok(&["rc.verbose=off", "track", "2025-03-03T08:00", "-", "2025-03-03T09:00", "foo"]), "");
}

#[test]
fn ranges_and_hints() {
    let db = Db::new();
    db.ok(&["track", "2025-03-03T08:00", "-", "2025-03-03T09:00", "foo"]);
    db.ok(&["track", "2025-03-04T08:00", "-", "2025-03-04T09:00", "bar"]);
    let count = |args: &[&str]| {
        let mut a = vec!["get", "dom.tracked.count"];
        a.extend_from_slice(args);
        db.ok(&a)
    };
    assert_eq!(count(&[]), "2\n");
    assert_eq!(count(&["2025-03-03"]), "1\n");
    assert_eq!(count(&["2025-03"]), "2\n");
    assert_eq!(count(&["2025-W10"]), "2\n");
    assert_eq!(count(&["from", "2025-03-04"]), "1\n");
    assert_eq!(count(&["2025-03-03T00:00", "for", "1d"]), "1\n");
    assert_eq!(count(&["foo"]), "1\n");
    assert_eq!(
        db.fail(&["summary", "2025-03-05", "-", "2025-03-01"]),
        "The end of a date range must be after the start.\n"
    );
    assert_eq!(db.fail(&["summary", ":week", "2025-03-01"]), "Unrecognized date range: '<date> - <date> <date>'.\n");
    assert_eq!(db.fail(&["frobnicate"]), "'frobnicate' is not a timew command. See 'timew help'.\n");
}

#[test]
fn repairs_tags_database() {
    let db = Db::new();
    db.ok(&["track", "2025-03-03T08:00", "-", "2025-03-03T09:00", "foo"]);
    fs::remove_file(db.path().join("data/tags.data")).unwrap();
    let out = db.ok(&["export"]);
    assert!(out.starts_with("Tags database does not exist. Recreating from interval data...\n["));
    assert_eq!(db.file("data/tags.data"), "{\n  \"foo\":{\"count\":1}\n}");
}

#[test]
fn import_export_round_trip() {
    let db = Db::new();
    db.ok(&["track", "2025-03-03T08:00", "-", "2025-03-03T09:00", "foo", "with space"]);
    db.ok(&["annotate", "@1", "quote \" inside"]);
    let exported = db.ok(&["export"]);
    let file = db.path().join("export.json");
    fs::write(&file, &exported).unwrap();

    let other = Db::new();
    assert_eq!(
        other.ok(&["import", file.to_str().unwrap()]),
        format!(
            "Note: 'foo' is a new tag.\nNote: '\"with space\"' is a new tag.\nImported 1 interval(s) from '{}'.\n",
            file.display()
        )
    );
    assert_eq!(other.ok(&["export"]), exported);
    assert_eq!(other.file("data/2025-03.data"), db.file("data/2025-03.data"));
}

#[test]
fn extension_reports() {
    let db = Db::new();
    db.run(&[]);
    let script = db.path().join("extensions/echo.sh");
    fs::write(&script, "#!/bin/sh\ncat\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    }
    db.ok(&["track", "2025-03-03T08:00", "-", "2025-03-03T09:00", "foo"]);
    let out = db.ok(&["echo", ":all"]);
    assert!(out.contains("\ntemp.report.start: \n"), "{out}");
    assert!(out.ends_with("\n\n[\n{\"id\":1,\"start\":\"20250303T070000Z\",\"end\":\"20250303T080000Z\",\"tags\":[\"foo\"]}\n]\n"));
    assert_eq!(db.ok(&["report", "echo", "2025-03-04"]).lines().last(), Some("]"));
    assert!(db.ok(&["extensions"]).contains("echo.sh   Active"));
}

/// Not in Timewarrior: entries with a tag starting with one underscore are
/// listed but left out of the totals.
#[test]
fn underscore_tags_are_left_out_of_totals() {
    let db = Db::new();
    db.ok(&[":yes", "track", "2025-03-03T08:00", "-", "2025-03-03T10:00", "meetings"]);
    db.ok(&["track", "2025-03-03T10:00", "-", "2025-03-03T11:00", "_break", "meetings"]);
    db.ok(&["track", "2025-03-03T11:00", "-", "2025-03-03T11:30", "__double"]);
    let summary = db.ok(&["summary", "2025-03-03", "rc.color=off"]);
    assert!(summary.contains("_break,") && summary.contains("1:00:00"), "{summary}");
    // the day total and the grand total: 2:00 + 0:30, without the hour of _break
    assert!(summary.contains("0:30:00 2:30:00\n"), "{summary}");
    assert!(summary.trim_end().ends_with("2:30:00"), "{summary}");
    let day = db.ok(&["day", "2025-03-03", "rc.color=off", "rc.reports.day.totals=on", "rc.reports.day.summary=on"]);
    assert!(day.contains("2:30") && !day.contains("3:30"), "{day}");
    assert!(day.contains("Tracked") && day.contains("2:30:00"), "{day}");
}

/// Not in Timewarrior: `export :csv`.
#[test]
fn export_as_csv() {
    let db = Db::new();
    db.ok(&[":yes", "track", "2025-03-03T08:00", "-", "2025-03-03T10:00", "meetings", "team"]);
    db.ok(&["track", "2025-03-03T10:00", "-", "2025-03-03T10:30", "email"]);
    db.ok(&["annotate", "@1", "inbox, zero"]);
    db.ok(&["track", "2025-03-04T09:00", "-", "2025-03-04T09:15", "email"]);
    let csv = db.ok(&["export", ":csv"]);
    assert_eq!(
        csv,
        "Wk,Date,Day,Tags,Start,End,Time,Total,Annotation\n\
         W10,2025-03-03,Mon,\"meetings, team\",8:00,10:00,2:00,,\n\
         W10,2025-03-03,Mon,email,10:00,10:30,0:30,2.50,\"inbox, zero\"\n\
         W10,2025-03-04,Tue,email,9:00,9:15,0:15,0.25,\n"
    );
    let one_day = db.ok(&["export", ":csv", "2025-03-04", "email"]);
    assert_eq!(one_day.lines().count(), 2, "{one_day}");
    let semi = db.ok(&["export", ":csv", "2025-03-03", "rc.export.csv.separator=;"]);
    assert!(semi.starts_with("Wk;Date;Day;") && semi.contains(";meetings, team;8:00;"), "{semi}");
    let tab = db.ok(&["export", ":csv", "2025-03-04", "rc.export.csv.separator=tab"]);
    assert!(tab.contains("\tTue\temail\t9:00\t"), "{tab}");
    let bad = db.fail(&["export", ":csv", "rc.export.csv.separator=;;"]);
    assert!(bad.contains("export.csv.separator"), "{bad}");
    fs::write(db.path().join("timewarrior.cfg"), "export.csv.separator = |\n").unwrap();
    assert!(db.ok(&["export", ":csv", "2025-03-04"]).contains("|Tue|email|"));
    let by_id = db.ok(&["export", ":csv", "@3"]);
    assert!(by_id.contains("8:00|10:00") && by_id.lines().count() == 2, "{by_id}");
}
