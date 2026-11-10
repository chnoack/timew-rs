//! Minimal argument parser: classifies `argv[1..]` into a command word,
//! `@N` ids, `:hint` tokens, a datetime range, and plain tags.
//!
//! This is a deliberately narrow, purpose-built parser -- NOT a port of
//! `CLI.cpp`'s general lexer (which also handles DOM references,
//! durations, config overrides, and quote/escape re-tokenizing of a single
//! shell string for contexts outside of normal argv). Those are out of
//! scope for this project. What IS in scope and ported faithfully:
//!
//!   - `@<positive-integer>` token recognition, confirmed against
//!     `CLI::identifyIds` in the C++ source: the ENTIRE token must match
//!     `@` followed by digits (not just a prefix), and `@0` or `@-1` are
//!     rejected as invalid ids, not silently treated as tags.
//!   - `:name` tokens are hints, matched by `CLI::findHint`'s literal
//!     `":" + hint` comparison.
//!   - Because `argv` is already shell-split by the OS before our process
//!     starts, each element is one token -- we don't need to re-lex a
//!     joined string the way the C++ source sometimes does for embedded/
//!     extension contexts.
//!
//! Remaining (non-id, non-hint) tokens are split into "range-shaped" ones
//! (dates, `-`, `to`) and everything else, which become tags. This mirrors
//! the effect of `CLI::getRange` picking date-shaped tokens out of the
//! token stream while `CLI::getTags` picks up everything tagged `TAG`
//! (i.e. not recognized as a date, duration, or keyword) -- re-derived
//! from that behavior, not transcribed from the lexer internals.

use crate::datetime::{expand_range_hint, parse_datetime};
use crate::range::Range;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedArgs {
    pub command: Option<String>,
    pub ids: Vec<u32>,
    pub hints: Vec<String>,
    /// Remaining tokens not consumed as the command/an id/a hint, in
    /// original order. Commands that need date-range parsing consume a
    /// prefix of these (see `take_range`); whatever is left after that is
    /// the tag list.
    pub rest: Vec<String>,
}

impl ParsedArgs {
    pub fn hint(&self, name: &str) -> bool {
        self.hints.iter().any(|h| h == name)
    }

    /// Consume a leading date-range from `rest` (one of: nothing, a single
    /// `:hint`, a single date, or `<date> - <date>` / `<date> to <date>`),
    /// returning the resolved `Range` and leaving whatever tokens remain
    /// (which become tags) in `self.rest`.
    ///
    /// Order of precedence matches the C++ source's `getRange`: a range
    /// hint, if present among the hints already extracted, takes priority
    /// over any date tokens (a command like `export :week` doesn't also
    /// need date args, but if both were somehow given, the hint wins,
    /// matching `expandIntervalHint` being checked first in `getRange`'s
    /// token classification loop).
    pub fn take_range(&mut self) -> Result<Option<Range>, String> {
        for hint in &self.hints {
            if let Some(range) = expand_range_hint(hint) {
                return Ok(Some(range));
            }
        }

        if self.rest.is_empty() {
            return Ok(None);
        }

        // Try <date> - <date> / <date> to <date> first (greedy), then a
        // single <date>, else leave `rest` untouched (not a range at all --
        // e.g. a bare `timew tag @1 sometag` where "sometag" isn't a date).
        if self.rest.len() >= 3
            && is_datetime_token(&self.rest[0])
            && (self.rest[1] == "-" || self.rest[1] == "to")
            && is_datetime_token(&self.rest[2])
        {
            let start = parse_datetime(&self.rest[0])?;
            let end = parse_datetime(&self.rest[2])?;
            self.rest.drain(0..3);
            return Ok(Some(Range::new(start, end)));
        }

        if is_datetime_token(&self.rest[0]) {
            let start = parse_datetime(&self.rest[0])?;
            self.rest.remove(0);
            return Ok(Some(Range::new(start, 0)));
        }

        Ok(None)
    }

    /// Everything left in `rest` after `take_range` (or all of `rest`, if
    /// `take_range` was never called) is tag text, mirroring `getTags()`.
    pub fn tags(&self) -> &[String] {
        &self.rest
    }
}

/// A token "looks like" a datetime argument if it's the compact ISO form,
/// or starts with `YYYY-MM-DD` (the extended form `parse_datetime` also
/// accepts). We deliberately check shape here (not full validity) so that a
/// malformed-but-date-shaped token still gets routed to `parse_datetime`
/// and produces a proper parse error, rather than silently being treated as
/// a tag.
fn is_datetime_token(s: &str) -> bool {
    if s.len() == 16 && s.as_bytes().get(8) == Some(&b'T') && s.ends_with('Z') {
        return true;
    }
    if s.len() >= 10 {
        let bytes = s.as_bytes();
        return bytes[0..4].iter().all(u8::is_ascii_digit)
            && bytes[4] == b'-'
            && bytes[5..7].iter().all(u8::is_ascii_digit)
            && bytes[7] == b'-'
            && bytes[8..10].iter().all(u8::is_ascii_digit);
    }
    false
}

/// Parse `argv[1..]` (i.e. excluding the program name itself) into a
/// `ParsedArgs`. The first non-id, non-hint token is taken as the command.
pub fn parse(args: &[String]) -> Result<ParsedArgs, String> {
    let mut parsed = ParsedArgs::default();

    for arg in args {
        if let Some(id) = parse_id_token(arg)? {
            parsed.ids.push(id);
            continue;
        }

        if let Some(hint) = arg.strip_prefix(':')
            && !hint.is_empty() {
                parsed.hints.push(hint.to_string());
                continue;
            }

        if parsed.command.is_none() {
            parsed.command = Some(arg.clone());
        } else {
            parsed.rest.push(arg.clone());
        }
    }

    Ok(parsed)
}

/// Recognize `@<positive-integer>` tokens. The whole token must match (a
/// trailing non-digit character disqualifies it), and `@0` is rejected as
/// invalid -- both ported from `CLI::identifyIds`'s `pig.eos()` check and
/// its explicit `if (digits <= 0) throw ...`.
fn parse_id_token(token: &str) -> Result<Option<u32>, String> {
    let Some(digits) = token.strip_prefix('@') else { return Ok(None) };
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return Ok(None);
    }
    let value: i64 = digits.parse().map_err(|_| format!("'{token}' is not a valid ID."))?;
    if value <= 0 {
        return Err(format!("'{token}' is not a valid ID."));
    }
    Ok(Some(value as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_bare_command() {
        let p = parse(&args(&["summary"])).unwrap();
        assert_eq!(p.command.as_deref(), Some("summary"));
        assert!(p.ids.is_empty());
        assert!(p.hints.is_empty());
        assert!(p.rest.is_empty());
    }

    #[test]
    fn parses_command_with_id() {
        let p = parse(&args(&["modify", "@141", "range"])).unwrap();
        assert_eq!(p.command.as_deref(), Some("modify"));
        assert_eq!(p.ids, vec![141]);
        assert_eq!(p.rest, vec!["range".to_string()]);
    }

    #[test]
    fn parses_command_with_hint() {
        let p = parse(&args(&["track", ":week", "MISC"])).unwrap();
        assert_eq!(p.command.as_deref(), Some("track"));
        assert_eq!(p.hints, vec!["week".to_string()]);
        assert_eq!(p.rest, vec!["MISC".to_string()]);
    }

    #[test]
    fn id_zero_is_rejected() {
        assert!(parse_id_token("@0").unwrap_err().contains("not a valid ID"));
    }

    #[test]
    fn id_with_trailing_garbage_is_not_an_id() {
        // The whole token must be @<digits> -- "@12x" isn't an id at all,
        // it falls through to being treated as a plain word (and since it
        // isn't recognized as anything else, ends up as a tag/command).
        assert_eq!(parse_id_token("@12x").unwrap(), None);
    }

    #[test]
    fn negative_looking_id_is_not_recognized_as_id_token() {
        // "@-5" does not match `@<digits>` at all (strip_prefix leaves
        // "-5", which is not all-digits), so it's not an id token -- this
        // matches the C++ lexer never matching such a malformed token as
        // an ID pattern in the first place.
        assert_eq!(parse_id_token("@-5").unwrap(), None);
    }

    #[test]
    fn multiple_ids_collected_in_order() {
        let p = parse(&args(&["delete", "@3", "@1", "@2"])).unwrap();
        assert_eq!(p.ids, vec![3, 1, 2]);
    }

    #[test]
    fn hint_lookup_helper() {
        let p = parse(&args(&["export", ":lastmonth"])).unwrap();
        assert!(p.hint("lastmonth"));
        assert!(!p.hint("week"));
    }

    #[test]
    fn take_range_prefers_hint_over_date_tokens() {
        let mut p = parse(&args(&["export", ":day", "SomeTag"])).unwrap();
        let range = p.take_range().unwrap();
        assert!(range.is_some());
        // "SomeTag" should remain as a tag, not be consumed as part of a range.
        assert_eq!(p.rest, vec!["SomeTag".to_string()]);
    }

    #[test]
    fn take_range_parses_closed_date_range_with_dash() {
        let mut p = parse(&args(&[
            "export",
            "2026-10-01T00:00:00Z",
            "-",
            "2026-10-02T00:00:00Z",
        ]))
        .unwrap();
        let range = p.take_range().unwrap().unwrap();
        assert_eq!(range.start, crate::interval::parse_iso("20261001T000000Z").unwrap());
        assert_eq!(range.end, crate::interval::parse_iso("20261002T000000Z").unwrap());
        assert!(p.rest.is_empty());
    }

    #[test]
    fn take_range_parses_closed_date_range_with_to() {
        let mut p = parse(&args(&[
            "export",
            "2026-10-01T00:00:00Z",
            "to",
            "2026-10-02T00:00:00Z",
        ]))
        .unwrap();
        let range = p.take_range().unwrap();
        assert!(range.is_some());
        assert!(p.rest.is_empty());
    }

    #[test]
    fn take_range_parses_single_open_ended_date() {
        let mut p = parse(&args(&["export", "2026-10-01T00:00:00Z"])).unwrap();
        let range = p.take_range().unwrap().unwrap();
        assert_eq!(range.start, crate::interval::parse_iso("20261001T000000Z").unwrap());
        assert_eq!(range.end, 0);
    }

    #[test]
    fn take_range_leaves_non_date_tokens_alone() {
        let mut p = parse(&args(&["tag", "@1", "sometag"])).unwrap();
        let range = p.take_range().unwrap();
        assert_eq!(range, None);
        assert_eq!(p.tags(), &["sometag".to_string()]);
    }

    #[test]
    fn take_range_with_no_remaining_tokens_is_none() {
        let mut p = parse(&args(&["summary"])).unwrap();
        assert_eq!(p.take_range().unwrap(), None);
    }

    #[test]
    fn remaining_tags_after_range_consumed() {
        let mut p = parse(&args(&[
            "track",
            "2026-10-01T09:00:00Z",
            "-",
            "2026-10-01T10:00:00Z",
            "alpha",
            "beta",
        ]))
        .unwrap();
        p.take_range().unwrap();
        assert_eq!(p.tags(), &["alpha".to_string(), "beta".to_string()]);
    }
}
