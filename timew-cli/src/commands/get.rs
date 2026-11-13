//! `timew get <dom> ...`: print values from the data, space-separated.
//!
//! | Reference | Value |
//! | --- | --- |
//! | `dom.active` | `1` if tracking, else `0` |
//! | `dom.active.start`, `.duration`, `.json` | the running interval |
//! | `dom.active.tags`, `.tags.count`, `.tags.<N>` | its tags |
//! | `dom.tracked.count`, `dom.tracked.ids` | intervals in the range given |
//! | `dom.tracked.tags`, `.tags.count`, `.tags.<N>` | their tags |
//! | `dom.tracked.<N>.start`, `.end`, `.duration`, `.json`, `.tags…` | the N-th most recent one |
//! | `dom.tags`, `dom.tags.count`, `dom.tags.<N>` | all known tags (`tags.data`) |
//! | `dom.rc.<name>` | a configuration value |
//!
//! `.tag.` works in place of `.tags.` but is deprecated.

use crate::datetime::format_local_iso;
use crate::duration::format_iso;
use crate::interval::Interval;
use crate::range::Range;
use crate::session::Session;
use crate::text::join_quoted;

pub fn run(s: &mut Session) -> Result<i32, String> {
    let range = s.cli.range(Range::empty(), s.now)?;
    let tags = s.cli.tags();
    let mut values = Vec::new();
    for reference in s.cli.dom_refs() {
        let value = resolve(s, &range, &tags, &reference)?
            .ok_or_else(|| format!("DOM reference '{reference}' is not valid."))?;
        values.push(value);
    }
    s.out.push_str(&values.join(" "));
    s.out.push('\n');
    Ok(0)
}

const DEPRECATED: &str =
    "DOM reference '.tag.' is deprecated and will be removed in a future version of Timewarrior!\nUse reference '.tags.' instead.";

fn resolve(s: &mut Session, range: &Range, tags: &[String], reference: &str) -> Result<Option<String>, String> {
    let Some(rest) = reference.strip_prefix("dom.") else { return Ok(None) };

    if let Some(rest) = rest.strip_prefix("active") {
        let active = s.active()?;
        if rest.is_empty() {
            return Ok(Some(if active.is_some() { "1" } else { "0" }.to_string()));
        }
        let Some(active) = active else { return Ok(None) };
        let Some(field) = rest.strip_prefix('.') else { return Ok(None) };
        return Ok(field_of(&active.interval, active.id, field, s.now, false));
    }

    if let Some(rest) = rest.strip_prefix("tracked.") {
        let tracked = s.tracked_in(range, tags)?;
        if let Some(t) = rest.strip_prefix("tags") {
            let mut all: Vec<String> = tracked.iter().flat_map(|t| t.interval.tags.clone()).collect();
            all.sort();
            all.dedup();
            return Ok(tag_value(&all, t));
        }
        if rest == "ids" {
            return Ok(Some(tracked.iter().map(|t| format!("@{} ", t.id)).collect()));
        }
        if rest == "count" {
            return Ok(Some(tracked.len().to_string()));
        }
        let Some((n, field)) = rest.split_once('.') else { return Ok(None) };
        let Ok(n) = n.parse::<usize>() else { return Ok(None) };
        if n == 0 || n > tracked.len() {
            return Ok(None);
        }
        let t = &tracked[tracked.len() - n];
        return Ok(field_of(&t.interval, t.id, field, s.now, true));
    }

    if let Some(rest) = rest.strip_prefix("tag.") {
        eprintln!("{DEPRECATED}");
        return Ok(tag_value(&s.db.tag_names(), &format!(".{rest}")));
    }
    if let Some(rest) = rest.strip_prefix("tags") {
        return Ok(tag_value(&s.db.tag_names(), rest));
    }
    if let Some(name) = rest.strip_prefix("rc.") {
        return Ok(Some(s.rules.get(name).unwrap_or("").to_string()));
    }
    Ok(None)
}

fn field_of(interval: &Interval, id: u32, field: &str, now: i64, with_end: bool) -> Option<String> {
    match field {
        "start" => Some(format_local_iso(interval.range.start)),
        "end" if with_end => Some(if interval.is_ended() { format_local_iso(interval.range.end) } else { String::new() }),
        "duration" => Some(format_iso(interval.range.total(now))),
        "json" => Some(interval.to_json(id)),
        _ => {
            if let Some(rest) = field.strip_prefix("tag.") {
                eprintln!("{DEPRECATED}");
                return tag_value(&interval.tags, &format!(".{rest}"));
            }
            tag_value(&interval.tags, field.strip_prefix("tags")?)
        }
    }
}

/// `""` → all tags, `.count`, `.<N>` (1-based).
fn tag_value(tags: &[String], rest: &str) -> Option<String> {
    if rest.is_empty() {
        return Some(join_quoted(" ", tags));
    }
    let rest = rest.strip_prefix('.')?;
    if rest == "count" {
        return Some(tags.len().to_string());
    }
    let n: usize = rest.parse().ok()?;
    (1..=tags.len()).contains(&n).then(|| tags[n - 1].clone())
}
