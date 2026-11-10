//! `timew get <dom-reference> [<dom-reference> ...]`
//!
//! Clean-room port of `CmdGet.cpp` + the DOM-resolution logic in `dom.cpp`
//! (GothenburgBitFactory/timewarrior, `develop` branch, commit 3699d2e),
//! re-derived from reading that source and empirically verified against
//! the real binary for every reference form implemented here (see tests,
//! several of which assert against literal output captured via the
//! sandboxed `TIMEWARRIORDB`).
//!
//! Scope: `dom.active[...]`, `dom.tracked.<N|count|ids|tags...>`,
//! `dom.tag.*` / `dom.tags[...]` (global tag listing). Deliberately NOT
//! implemented: `dom.rc.<name>` (reads the `Rules`/config system, which
//! this project has scoped out entirely).
//!
//! Non-obvious, empirically-confirmed facts:
//!   - A token is a DOM reference purely by prefix: anything starting with
//!     literal `"dom."` (`CLI.cpp`'s `raw.rfind("dom.", 0) == 0` check).
//!   - `dom.tracked.<N>` is 1-based and counts from the END of the
//!     (chronologically oldest-first) `tracked` vector, i.e. `<N>=1` is
//!     the MOST RECENT interval -- `tracked[count - n]` in the source.
//!     This matches id=1 also being the most recent interval, but is a
//!     numerically separate piece of indexing logic re-derived from the
//!     source, not assumed to just equal `id - 1`.
//!   - Tag listings (`dom.tags`, `dom.tracked.tags`, `dom.tracked.<N>.tags`,
//!     `dom.active.tags`) are alphabetically ordered (`std::set` iteration)
//!     and 1-based-indexed via `.tags.<M>`.
//!   - `dom.*.start`/`.end` render in **local time**, extended ISO form
//!     (`YYYY-MM-DDTHH:MM:SS`, no `Z`) -- confirmed empirically: local
//!     09:00 CEST in October renders as `2026-10-01T09:00:00`, not the
//!     UTC-with-`Z` form used internally for storage.
//!   - `.duration` renders as an ISO 8601 duration (`PnDTnHnMnS`), with
//!     each of D/H/M/S independently omitted when zero, EXCEPT the
//!     all-zero case which is the special-cased literal `PT0S` (confirmed
//!     empirically for 0s, 90s, exactly 1 day, 2 days + 3 hours, and
//!     1h0m5s).
//!   - Multiple DOM references in one `get` invocation are resolved
//!     independently and joined with a single space on one output line
//!     (plus a trailing newline), matching `join(" ", results)`.
//!   - A reference that doesn't resolve to anything is an error naming
//!     that exact reference string, not a blank/empty result.

use crate::database::{Database, Tracked};
use crate::interval::Interval;

pub fn run(database: &mut Database, references: &[String]) -> Result<String, String> {
    // `all_tracked` returns newest-first; dom.cpp's `tracked` (from
    // `getTracked`) is oldest-first, and BOTH the `<N>` indexing
    // (`tracked[count - n]`) AND the plain iteration order (`dom.tracked.ids`
    // walks `tracked` front-to-back) depend on that ordering. We reverse
    // here to match exactly, rather than relying on an index-translation
    // trick that would only fix `<N>` lookups and silently leave
    // `dom.tracked.ids` printing in the wrong order (caught via
    // differential testing against the real binary).
    let mut tracked = database.all_tracked()?;
    tracked.reverse();

    let mut results = Vec::with_capacity(references.len());
    for reference in references {
        let value = resolve(database, &tracked, reference)
            .ok_or_else(|| format!("DOM reference '{reference}' is not valid."))?;
        results.push(value);
    }

    Ok(format!("{}\n", results.join(" ")))
}

fn resolve(database: &mut Database, tracked: &[Tracked], reference: &str) -> Option<String> {
    let rest = reference.strip_prefix("dom.")?;

    if let Some(rest) = rest.strip_prefix("active") {
        return resolve_active(tracked, rest);
    }

    if let Some(rest) = rest.strip_prefix("tracked.") {
        return resolve_tracked(tracked, rest);
    }

    if let Some(rest) = rest.strip_prefix("tag.") {
        // Deprecated alias for dom.tags.* -- same data, no warning emitted
        // (warning-to-stderr behavior is cosmetic and out of scope).
        return resolve_tag_list(database, rest);
    }

    if let Some(rest) = rest.strip_prefix("tags") {
        return resolve_tags_root(database, rest);
    }

    None
}

fn resolve_active(tracked: &[Tracked], rest: &str) -> Option<String> {
    // `tracked` here is oldest-first (see `run`), so the most recent
    // interval -- the only one that can possibly be open -- is the LAST
    // element, matching the source's `intervals.at(0)` only because ITS
    // `intervals` (from a dedicated `IntervalFilterFirstOf` scan, not this
    // `tracked` vector) is newest-first. We derive the same "most recent"
    // interval from our oldest-first `tracked` by taking the last element.
    let latest = tracked.last();

    if rest.is_empty() {
        let is_open = latest.map(|t| t.interval.is_open()).unwrap_or(false);
        return Some(if is_open { "1" } else { "0" }.to_string());
    }

    let rest = rest.strip_prefix('.')?;
    let latest = latest.filter(|t| t.interval.is_open())?;
    resolve_interval_field(&latest.interval, Some(latest.id), rest)
}

fn resolve_tracked(tracked: &[Tracked], rest: &str) -> Option<String> {
    let count = tracked.len();

    if let Some(rest) = rest.strip_prefix("tags") {
        let mut tags: Vec<String> =
            tracked.iter().flat_map(|t| t.interval.tags.iter().cloned()).collect();
        tags.sort();
        tags.dedup();
        return resolve_tag_field(&tags, rest);
    }

    if rest == "ids" {
        let mut s = String::new();
        for t in tracked {
            s.push_str(&format!("@{} ", t.id));
        }
        return Some(s);
    }

    if rest == "count" {
        return Some(count.to_string());
    }

    // dom.tracked.<N>.<...> -- N is 1-based, 1 = most recent (id 1).
    // `tracked` here is oldest-first, so matching the source's
    // `tracked[count - n]` indexing (where `tracked` is also oldest-first)
    // directly, rather than re-deriving an equivalent front-indexed scheme,
    // is what keeps this in lockstep with `dom.tracked.ids`'s plain
    // iteration order above.
    let dot = rest.find('.')?;
    let n: usize = rest[..dot].parse().ok()?;
    if n == 0 || n > count {
        return None;
    }
    let t = &tracked[count - n];
    resolve_interval_field(&t.interval, Some(t.id), &rest[dot + 1..])
}

fn resolve_interval_field(interval: &Interval, id: Option<u32>, field: &str) -> Option<String> {
    match field {
        "start" => Some(local_iso_extended(interval.range.start)),
        "end" => {
            if interval.is_open() {
                Some(String::new())
            } else {
                Some(local_iso_extended(interval.range.end))
            }
        }
        "duration" => Some(format_iso_duration(interval_total_seconds(interval))),
        "json" => Some(interval.to_json(id.unwrap_or(0))),
        _ => {
            if let Some(rest) = field.strip_prefix("tag.") {
                return resolve_tag_field(&interval.tags, rest);
            }
            if let Some(rest) = field.strip_prefix("tags") {
                return resolve_tag_field(&interval.tags, rest);
            }
            None
        }
    }
}

/// `tags` must already be alphabetically sorted (true for `Interval::tags`
/// by construction, and for the aggregated lists built above). `rest` is
/// what follows `.tags`/`.tag.` in the reference: empty (list all),
/// `.count`, or `.<N>` / `<N>` (1-based index).
fn resolve_tag_field(tags: &[String], rest: &str) -> Option<String> {
    if rest.is_empty() {
        return Some(join_quoted_if_needed(tags));
    }
    let rest = rest.strip_prefix('.').unwrap_or(rest);
    if rest == "count" {
        return Some(tags.len().to_string());
    }
    let n: usize = rest.parse().ok()?;
    if n == 0 || n > tags.len() {
        return None;
    }
    Some(tags[n - 1].clone())
}

fn resolve_tag_list(database: &mut Database, rest: &str) -> Option<String> {
    let tags = all_known_tags(database);
    if rest == "count" {
        return Some(tags.len().to_string());
    }
    let n: usize = rest.parse().ok()?;
    if n == 0 || n > tags.len() {
        return None;
    }
    Some(tags[n - 1].clone())
}

fn resolve_tags_root(database: &mut Database, rest: &str) -> Option<String> {
    let tags = all_known_tags(database);
    if rest.is_empty() {
        return Some(join_quoted_if_needed(&tags));
    }
    let rest = rest.strip_prefix('.')?;
    if rest == "count" {
        return Some(tags.len().to_string());
    }
    let n: usize = rest.parse().ok()?;
    if n == 0 || n > tags.len() {
        return None;
    }
    Some(tags[n - 1].clone())
}

/// All tags ever used across the whole database (not just currently
/// tracked ones), matching `Database::tags()` / `TagInfoDatabase`. We don't
/// have a persisted tag-usage index yet (see tags.rs, not yet wired to
/// Database), so for now this derives the set from every stored interval --
/// behaviorally equivalent to the real tags.data index as long as that
/// index hasn't drifted from the actual interval data.
fn all_known_tags(database: &mut Database) -> Vec<String> {
    let mut tags: Vec<String> = database
        .all_tracked()
        .unwrap_or_default()
        .into_iter()
        .flat_map(|t| t.interval.tags)
        .collect();
    tags.sort();
    tags.dedup();
    tags
}

fn join_quoted_if_needed(tags: &[String]) -> String {
    tags.iter()
        .map(|t| crate::interval::quote_if_needed(t))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Render as local-time extended ISO (`YYYY-MM-DDTHH:MM:SS`, no `Z`),
/// matching `Datetime::toISOLocalExtended()`.
fn local_iso_extended(epoch: crate::range::Epoch) -> String {
    let zoned = jiff::Zoned::new(
        jiff::Timestamp::from_second(epoch).expect("valid epoch"),
        jiff::tz::TimeZone::system(),
    );
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        zoned.year(),
        zoned.month(),
        zoned.day(),
        zoned.hour(),
        zoned.minute(),
        zoned.second()
    )
}

fn interval_total_seconds(interval: &Interval) -> i64 {
    let now = jiff::Timestamp::now().as_second();
    interval.range.total(now)
}

/// Format a duration in seconds as an ISO 8601 duration `PnDTnHnMnS`, with
/// each field omitted when zero, except the special case of a totally zero
/// duration which renders as the literal `PT0S` (all empirically confirmed
/// against the real binary -- see module docs).
fn format_iso_duration(total_seconds: i64) -> String {
    if total_seconds == 0 {
        return "PT0S".to_string();
    }

    let days = total_seconds / 86400;
    let rem = total_seconds % 86400;
    let hours = rem / 3600;
    let rem = rem % 3600;
    let minutes = rem / 60;
    let seconds = rem % 60;

    let mut out = String::from("P");
    if days > 0 {
        out.push_str(&format!("{days}D"));
    }
    if hours > 0 || minutes > 0 || seconds > 0 {
        out.push('T');
        if hours > 0 {
            out.push_str(&format!("{hours}H"));
        }
        if minutes > 0 {
            out.push_str(&format!("{minutes}M"));
        }
        if seconds > 0 {
            out.push_str(&format!("{seconds}S"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interval::parse_iso;
    use crate::range::Range;
    use std::fs;
    use std::path::PathBuf;

    fn tmp_db(name: &str) -> (PathBuf, Database) {
        let dir = std::env::temp_dir().join(format!("timew-rs-get-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let db = Database::open(&dir).unwrap();
        (dir, db)
    }

    fn mk_closed(start: &str, end: &str, tags: &[&str]) -> Interval {
        let mut iv = Interval::new(Range::new(parse_iso(start).unwrap(), parse_iso(end).unwrap()));
        for t in tags {
            iv.add_tag(*t);
        }
        iv
    }

    fn mk_open(start: &str, tags: &[&str]) -> Interval {
        let mut iv = Interval::new(Range::new(parse_iso(start).unwrap(), 0));
        for t in tags {
            iv.add_tag(*t);
        }
        iv
    }

    #[test]
    fn duration_formatting_matches_real_timew_samples() {
        assert_eq!(format_iso_duration(0), "PT0S");
        assert_eq!(format_iso_duration(3600), "PT1H");
        assert_eq!(format_iso_duration(90), "PT1M30S");
        assert_eq!(format_iso_duration(86400), "P1D");
        assert_eq!(format_iso_duration(3505546), "P40DT13H45M46S");
        assert_eq!(format_iso_duration(3605), "PT1H5S");
        assert_eq!(format_iso_duration(2 * 86400 + 3 * 3600), "P2DT3H");
    }

    #[test]
    fn tracked_count() {
        let (dir, mut db) = tmp_db("count");
        db.add_interval(&mk_closed("20261001T070000Z", "20261001T080000Z", &["a"])).unwrap();
        db.commit().unwrap();

        let out = run(&mut db, &["dom.tracked.count".to_string()]).unwrap();
        assert_eq!(out, "1\n");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn tracked_n_is_one_based_most_recent_first() {
        let (dir, mut db) = tmp_db("n-indexing");
        db.add_interval(&mk_closed("20261001T070000Z", "20261001T080000Z", &["old"])).unwrap();
        db.add_interval(&mk_closed("20261002T070000Z", "20261002T080000Z", &["new"])).unwrap();
        db.commit().unwrap();

        // N=1 should be the most recent ("new"), matching id=1.
        let out = run(&mut db, &["dom.tracked.1.tags.1".to_string()]).unwrap();
        assert_eq!(out, "new\n");
        let out2 = run(&mut db, &["dom.tracked.2.tags.1".to_string()]).unwrap();
        assert_eq!(out2, "old\n");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn tags_alphabetical_and_one_indexed() {
        let (dir, mut db) = tmp_db("tags-alpha");
        db.add_interval(&mk_closed("20261001T070000Z", "20261001T080000Z", &["zeta", "alpha"])).unwrap();
        db.commit().unwrap();

        assert_eq!(run(&mut db, &["dom.tracked.1.tags".to_string()]).unwrap(), "alpha zeta\n");
        assert_eq!(run(&mut db, &["dom.tracked.1.tags.1".to_string()]).unwrap(), "alpha\n");
        assert_eq!(run(&mut db, &["dom.tracked.1.tags.2".to_string()]).unwrap(), "zeta\n");
        assert_eq!(run(&mut db, &["dom.tracked.1.tags.count".to_string()]).unwrap(), "2\n");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn active_zero_when_nothing_open() {
        let (dir, mut db) = tmp_db("active-zero");
        db.add_interval(&mk_closed("20261001T070000Z", "20261001T080000Z", &["a"])).unwrap();
        db.commit().unwrap();

        assert_eq!(run(&mut db, &["dom.active".to_string()]).unwrap(), "0\n");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn active_one_when_open_interval_exists() {
        let (dir, mut db) = tmp_db("active-one");
        db.add_interval(&mk_open("20261001T070000Z", &["a"])).unwrap();
        db.commit().unwrap();

        assert_eq!(run(&mut db, &["dom.active".to_string()]).unwrap(), "1\n");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn active_start_field_resolves_when_open() {
        // Regression test: this was broken by a leading-dot bug caught via
        // differential testing against the real binary (dom.active.start
        // was being matched against the literal string ".start" instead
        // of "start", so it always fell through to "not valid").
        let (dir, mut db) = tmp_db("active-start-field");
        db.add_interval(&mk_open("20261002T070000Z", &["OPEN_TAG"])).unwrap();
        db.commit().unwrap();

        let out = run(&mut db, &["dom.active.start".to_string()]).unwrap();
        assert_eq!(out, format!("{}\n", local_iso_extended(parse_iso("20261002T070000Z").unwrap())));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn active_duration_and_json_fields_resolve_when_open() {
        let (dir, mut db) = tmp_db("active-other-fields");
        db.add_interval(&mk_open("20261001T070000Z", &["a"])).unwrap();
        db.commit().unwrap();

        assert!(run(&mut db, &["dom.active.duration".to_string()]).unwrap().starts_with('P'));
        let json = run(&mut db, &["dom.active.json".to_string()]).unwrap();
        assert!(json.contains("\"id\":1"));
        assert!(json.contains("\"a\""));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn active_field_errors_when_nothing_active() {
        let (dir, mut db) = tmp_db("active-field-errors");
        db.add_interval(&mk_closed("20261001T070000Z", "20261001T080000Z", &["a"])).unwrap();
        db.commit().unwrap();

        let err = run(&mut db, &["dom.active.start".to_string()]).unwrap_err();
        assert!(err.contains("dom.active.start"));
        assert!(err.contains("is not valid"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn end_of_open_interval_is_empty_string() {
        let (dir, mut db) = tmp_db("open-end-empty");
        db.add_interval(&mk_open("20261001T070000Z", &["a"])).unwrap();
        db.commit().unwrap();

        // dom.tracked.1.end should be an empty field, so the overall
        // output is just a trailing newline (space-joined with nothing
        // else in this single-reference call).
        let out = run(&mut db, &["dom.tracked.1.end".to_string()]).unwrap();
        assert_eq!(out, "\n");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn json_field_matches_interval_to_json() {
        let (dir, mut db) = tmp_db("json-field");
        db.add_interval(&mk_closed("20261001T070000Z", "20261001T080000Z", &["a"])).unwrap();
        db.commit().unwrap();

        let out = run(&mut db, &["dom.tracked.1.json".to_string()]).unwrap();
        assert_eq!(
            out,
            "{\"id\":1,\"start\":\"20261001T070000Z\",\"end\":\"20261001T080000Z\",\"tags\":[\"a\"]}\n"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unresolvable_reference_is_an_error_naming_it() {
        let (dir, mut db) = tmp_db("bogus");
        let err = run(&mut db, &["dom.bogus.thing".to_string()]).unwrap_err();
        assert_eq!(err, "DOM reference 'dom.bogus.thing' is not valid.");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn multiple_references_space_joined_on_one_line() {
        let (dir, mut db) = tmp_db("multi-ref");
        db.add_interval(&mk_closed("20261001T070000Z", "20261001T080000Z", &["X"])).unwrap();
        db.commit().unwrap();

        let out = run(
            &mut db,
            &["dom.tracked.count".to_string(), "dom.tracked.1.tags.1".to_string()],
        )
        .unwrap();
        assert_eq!(out, "1 X\n");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn tracked_ids_lists_oldest_first() {
        // Regression test: caught via differential testing against the real
        // binary. dom.tracked.ids iterates the (oldest-first) `tracked`
        // vector directly -- it must NOT print newest-first just because
        // id=1 happens to be the newest interval.
        let (dir, mut db) = tmp_db("ids-order");
        db.add_interval(&mk_closed("20261001T070000Z", "20261001T080000Z", &["old"])).unwrap();
        db.add_interval(&mk_closed("20261002T070000Z", "20261002T080000Z", &["new"])).unwrap();
        db.commit().unwrap();

        // id 1 = "new" (most recent), id 2 = "old". Real timew prints
        // "@2 @1 " (oldest id last... no: oldest INTERVAL first, which is
        // the higher-numbered id since id counts backward from newest).
        let out = run(&mut db, &["dom.tracked.ids".to_string()]).unwrap();
        assert_eq!(out, "@2 @1 \n");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn global_tags_listing_across_multiple_intervals() {
        let (dir, mut db) = tmp_db("global-tags");
        db.add_interval(&mk_closed("20261001T070000Z", "20261001T080000Z", &["alpha"])).unwrap();
        db.add_interval(&mk_closed("20261002T070000Z", "20261002T080000Z", &["beta"])).unwrap();
        db.commit().unwrap();

        assert_eq!(run(&mut db, &["dom.tags".to_string()]).unwrap(), "alpha beta\n");
        assert_eq!(run(&mut db, &["dom.tags.count".to_string()]).unwrap(), "2\n");
        assert_eq!(run(&mut db, &["dom.tags.1".to_string()]).unwrap(), "alpha\n");

        let _ = fs::remove_dir_all(&dir);
    }
}
