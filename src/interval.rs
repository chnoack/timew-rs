//! Interval: a tracked block of time with optional tags and one optional
//! annotation.
//!
//! Clean-room port of the on-disk/JSON contract defined by `Interval.cpp` /
//! `IntervalFactory.cpp` (GothenburgBitFactory/timewarrior, `develop`
//! branch, commit 3699d2e) -- re-derived from reading that source and
//! empirically verified against the real `timew` binary (see tests below,
//! several of which assert against byte-for-byte output captured from
//! `TIMEWARRIORDB=<sandbox> timew track/export ...`).
//!
//! On-disk serialization grammar (one line per interval in a `YYYY-MM.data`
//! file):
//!
//!   inc <ISOstart> [- <ISOend>] [# tag1 "tag with spaces" ... ["# annotation text"]]
//!
//! Where `<ISO...>` is the fixed-width compact UTC form `YYYYMMDDTHHMMSSZ`
//! (always exactly 16 characters -- this fixed length is literally how the
//! real parser distinguishes a timestamp token from everything else, see
//! `IntervalFactory::fromSerialization`'s `tokens[1].length() == 16` check).
//!
//! Tags are quoted with `"..."` whenever they contain a space, a `"`, or any
//! of `-+/()<^!=~_%` (ported from `quoteIfNeeded` in `util.cpp`); a literal
//! `"` inside a tag is backslash-escaped. An annotation, if present, is a
//! second `#`-introduced, double-quoted, space-joined run of tokens after
//! the tags.
//!
//! IDs are intentionally NOT part of this struct's serialized form -- see
//! `database.rs` for why (ids are assigned at read time, never stored).

use std::fmt::Write as _;

use crate::range::{Epoch, Range};
pub use crate::text::quote_if_needed;
use crate::text::{escape, json_encode};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Interval {
    pub range: Range,
    /// Kept sorted + deduplicated, mirroring `std::set<std::string>` in the
    /// C++ source -- this also fixes the serialization/export tag order to
    /// lexicographic, which is NOT incidental: `timew export`'s JSON tag
    /// array order is observably alphabetical because it iterates a
    /// `std::set`.
    pub tags: Vec<String>,
    pub annotation: Option<String>,
}

impl Interval {
    pub fn new(range: Range) -> Self {
        Interval { range, tags: Vec::new(), annotation: None }
    }

    pub fn is_started(&self) -> bool {
        self.range.is_started()
    }

    pub fn is_ended(&self) -> bool {
        self.range.is_ended()
    }

    pub fn is_open(&self) -> bool {
        self.range.is_open()
    }

    /// `Interval::empty()` in the C++ source: a fully-blank interval with no
    /// start, no end, no tags, no annotation. (Not to be confused with
    /// `Range::is_empty()`, which means "zero-width but has timestamps".)
    pub fn is_empty(&self) -> bool {
        !self.range.is_started()
            && !self.range.is_ended()
            && self.tags.is_empty()
            && self.annotation.is_none()
    }

    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t == tag)
    }

    /// Insert a tag, keeping the vector sorted + deduplicated (set semantics).
    pub fn add_tag(&mut self, tag: impl Into<String>) {
        let tag = tag.into();
        if let Err(pos) = self.tags.binary_search(&tag) {
            self.tags.insert(pos, tag);
        }
    }

    pub fn remove_tag(&mut self, tag: &str) {
        self.tags.retain(|t| t != tag);
    }

    /// Render as a `YYYY-MM.data` line, matching `Interval::serialize()`.
    pub fn serialize(&self) -> String {
        let mut out = String::from("inc");

        if self.range.is_started() {
            write!(out, " {}", format_iso(self.range.start)).unwrap();

            if self.range.is_ended() {
                write!(out, " - {}", format_iso(self.range.end)).unwrap();
            }
        }

        if !self.tags.is_empty() {
            out.push_str(" #");
            for tag in &self.tags {
                out.push(' ');
                out.push_str(&quote_if_needed(tag));
            }
        }

        if let Some(annotation) = &self.annotation {
            if self.tags.is_empty() {
                out.push_str(" #");
            }
            write!(out, " # \"{}\"", escape(annotation, '"')).unwrap();
        }

        out
    }

    /// Render as the JSON object used by `timew export`, given the ephemeral
    /// id this interval was assigned for this invocation (see database.rs).
    /// Field order and omission-when-absent match `Interval::json()` exactly.
    pub fn to_json(&self, id: u32) -> String {
        let mut out = String::from("{");

        if !self.is_empty() {
            write!(out, "\"id\":{}", id).unwrap();

            if self.range.is_started() {
                write!(out, ",\"start\":\"{}\"", format_iso(self.range.start)).unwrap();
            }
            if self.range.is_ended() {
                write!(out, ",\"end\":\"{}\"", format_iso(self.range.end)).unwrap();
            }
            if !self.tags.is_empty() {
                out.push_str(",\"tags\":[");
                for (i, tag) in self.tags.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write!(out, "\"{}\"", json_encode(tag)).unwrap();
                }
                out.push(']');
            }
            if let Some(annotation) = &self.annotation {
                write!(out, ",\"annotation\":\"{}\"", json_encode(annotation)).unwrap();
            }
        }

        out.push('}');
        out
    }

    /// Parse one `YYYY-MM.data` line (the `inc ...` grammar described at
    /// module level). Returns `None` for blank lines; returns `Err` for a
    /// line that doesn't start with `inc` or has a malformed structure.
    ///
    /// Deliberately lenient about quoting on read (accepts both `# BAGE-DPP`
    /// and `# "BAGE-DPP"` for a tag that would be written quoted) -- real
    /// `timew`'s tokenizer uses a general-purpose lexer that also tolerates
    /// unquoted tokens containing characters its own writer would quote;
    /// matching that leniency matters because hand-edited / foreign-written
    /// `.data` files exist in practice (we hit exactly this with imported
    /// data).
    /// Read an interval from its JSON form (as written by `to_json`, by
    /// `export` and in the undo journal). Returns the interval and its id
    /// (0 if absent). An empty string is the empty interval.
    pub fn from_json(json: &str) -> Result<(Interval, u32), String> {
        if json.trim().is_empty() {
            return Ok((Interval::default(), 0));
        }
        let value: serde_json::Value = serde_json::from_str(json).map_err(|e| format!("Invalid JSON: {e}"))?;
        Self::from_json_value(&value)
    }

    pub fn from_json_value(value: &serde_json::Value) -> Result<(Interval, u32), String> {
        let obj = value.as_object().ok_or_else(|| "Expected a JSON object for an interval.".to_string())?;
        let mut interval = Interval::default();
        let time = |key: &str| -> Result<Epoch, String> {
            match obj.get(key).and_then(|v| v.as_str()) {
                Some(s) => crate::datetime::parse_point(s).ok_or_else(|| format!("'{s}' is not a valid date.")),
                None => Ok(0),
            }
        };
        interval.range.start = time("start")?;
        interval.range.end = time("end")?;
        if let Some(tags) = obj.get("tags").and_then(|v| v.as_array()) {
            for tag in tags {
                if let Some(t) = tag.as_str() {
                    interval.add_tag(t);
                }
            }
        }
        interval.annotation = obj.get("annotation").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(str::to_string);
        let id = obj.get("id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        Ok((interval, id))
    }

    pub fn parse(line: &str) -> Result<Option<Interval>, String> {
        let line = line.trim();
        if line.is_empty() {
            return Ok(None);
        }

        let tokens = tokenize(line)?;
        if tokens.first().map(String::as_str) != Some("inc") {
            return Err(format!("Unrecognizable line '{line}'."));
        }

        let mut interval = Interval::default();
        let mut idx = 1;

        if let Some(tok) = tokens.get(idx)
            && is_iso_token(tok) {
                interval.range.start = parse_iso(tok)?;
                idx += 1;

                if tokens.get(idx).map(String::as_str) == Some("-")
                    && let Some(end_tok) = tokens.get(idx + 1)
                        && is_iso_token(end_tok) {
                            interval.range.end = parse_iso(end_tok)?;
                            idx += 2;
                        }
            }

        if tokens.get(idx).map(String::as_str) == Some("#") {
            idx += 1;
            while let Some(tok) = tokens.get(idx) {
                if tok == "#" {
                    break;
                }
                interval.add_tag(tok.clone());
                idx += 1;
            }

            if tokens.get(idx).map(String::as_str) == Some("#") {
                idx += 1;
                let annotation = tokens[idx..].join(" ");
                interval.annotation = Some(annotation);
            }
        }

        Ok(Some(interval))
    }
}

/// The (year, month) a datafile keyed by this epoch's date would use, i.e.
/// the `YYYY`/`MM` in `YYYY-MM.data`. Used by `Database::datafile_index_for`
/// to route an interval to the right file by its **start** timestamp
/// (matches `Database::addInterval`'s use of `interval.start.year()` /
/// `.month()`).
/// Year and month (local time) of the datafile an interval starting at
/// `epoch` belongs in.
pub fn year_month_of(epoch: Epoch) -> (i64, u32) {
    let z = crate::datetime::local(epoch);
    (z.year() as i64, z.month() as u32)
}

pub fn format_iso(epoch: Epoch) -> String {
    let (y, mo, d, h, mi, s) = civil_from_epoch(epoch);
    format!("{y:04}{mo:02}{d:02}T{h:02}{mi:02}{s:02}Z")
}

/// Parse the compact UTC ISO form `YYYYMMDDTHHMMSSZ` back to a Unix epoch.
pub fn parse_iso(s: &str) -> Result<Epoch, String> {
    if s.len() != 16 || s.as_bytes()[8] != b'T' || s.as_bytes()[15] != b'Z' {
        return Err(format!("Invalid ISO datetime '{s}'."));
    }
    let y: i64 = s[0..4].parse().map_err(|_| format!("Invalid ISO datetime '{s}'."))?;
    let mo: u32 = s[4..6].parse().map_err(|_| format!("Invalid ISO datetime '{s}'."))?;
    let d: u32 = s[6..8].parse().map_err(|_| format!("Invalid ISO datetime '{s}'."))?;
    let h: i64 = s[9..11].parse().map_err(|_| format!("Invalid ISO datetime '{s}'."))?;
    let mi: i64 = s[11..13].parse().map_err(|_| format!("Invalid ISO datetime '{s}'."))?;
    let se: i64 = s[13..15].parse().map_err(|_| format!("Invalid ISO datetime '{s}'."))?;

    Ok(epoch_from_civil(y, mo, d) + h * 3600 + mi * 60 + se)
}

fn is_iso_token(s: &str) -> bool {
    s.len() == 16 && s.as_bytes()[8] == b'T' && s.ends_with('Z')
}

/// Days since the Unix epoch for a given proleptic-Gregorian civil date,
/// using Howard Hinnant's well-known `days_from_civil` algorithm (public
/// domain / CC0; not timewarrior-derived -- this is a standard date
/// algorithm widely used in date libraries across many languages).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = (m as i64 + 9) % 12; // [0, 11]
    let doy = (153 * mp + 2) / 5 + d as i64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146097 + doe - 719468
}

/// Inverse of `days_from_civil`.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

fn epoch_from_civil(y: i64, m: u32, d: u32) -> Epoch {
    days_from_civil(y, m, d) * 86400
}

fn civil_from_epoch(epoch: Epoch) -> (i64, u32, u32, u32, u32, u32) {
    let days = epoch.div_euclid(86400);
    let secs_of_day = epoch.rem_euclid(86400);
    let (y, mo, d) = civil_from_days(days);
    let h = (secs_of_day / 3600) as u32;
    let mi = ((secs_of_day % 3600) / 60) as u32;
    let s = (secs_of_day % 60) as u32;
    (y, mo, d, h, mi, s)
}

/// Tokenize a serialized interval line into words, honoring double-quoted
/// strings (with backslash-escaped `"` inside them) as single tokens, and
/// treating `#` and `-` as standalone single-character tokens when they
/// appear outside quotes. This is a narrow, purpose-built tokenizer for the
/// `inc ...` grammar only -- NOT a general command-line lexer (that's
/// cli.rs's job, scoped separately since the full Lexer in the C++ source
/// handles many more token types we don't need here).
fn tokenize(line: &str) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        if c.is_whitespace() {
            i += 1;
            continue;
        }

        if c == '"' {
            let mut tok = String::new();
            i += 1;
            let mut closed = false;
            while i < chars.len() {
                if chars[i] == '\\' && i + 1 < chars.len() && chars[i + 1] == '"' {
                    tok.push('"');
                    i += 2;
                    continue;
                }
                if chars[i] == '"' {
                    closed = true;
                    i += 1;
                    break;
                }
                tok.push(chars[i]);
                i += 1;
            }
            if !closed {
                return Err(format!("Unterminated quoted string in '{line}'."));
            }
            tokens.push(tok);
            continue;
        }

        if c == '#' || c == '-' {
            tokens.push(c.to_string());
            i += 1;
            continue;
        }

        let mut tok = String::new();
        while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '"' {
            // A bare '-' or '#' ends the current unquoted token, since those
            // are structural delimiters in this grammar (matches the real
            // tokenizer treating them as standalone tokens even when not
            // separated by whitespace, e.g. "...# -tag" edge cases are not
            // something real timew data ever produces, so we keep this
            // simple and require delimiters to be whitespace-separated in
            // practice).
            if chars[i] == '#' {
                break;
            }
            tok.push(chars[i]);
            i += 1;
        }
        tokens.push(tok);
    }

    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- ISO datetime round-trip -------------------------------------

    #[test]
    fn iso_format_matches_real_timew_sample() {
        // Captured verbatim from ~/.local/share/timewarrior/data/2026-10.data
        assert_eq!(parse_iso("20261002T110000Z").unwrap(), epoch_from_civil(2026, 10, 2) + 11 * 3600);
        assert_eq!(format_iso(epoch_from_civil(2026, 10, 2) + 11 * 3600), "20261002T110000Z");
    }

    #[test]
    fn iso_round_trip_is_stable_across_many_values() {
        for epoch in [0i64, 86399, 1_700_000_000, 1_900_000_000, -1] {
            let s = format_iso(epoch);
            assert_eq!(parse_iso(&s).unwrap(), epoch, "round-trip failed for {epoch}");
        }
    }

    #[test]
    fn days_from_civil_known_epoch() {
        // 1970-01-01 is epoch day 0.
        assert_eq!(days_from_civil(1970, 1, 1), 0);
    }

    #[test]
    fn epoch_matches_system_date_command() {
        // Cross-checked against `date -u -j -f "%Y-%m-%dT%H:%M:%S" <ts> +%s`
        // on this machine (independent oracle, not timewarrior-derived).
        assert_eq!(epoch_from_civil(1970, 1, 1), 0);
        assert_eq!(epoch_from_civil(2026, 1, 1), 1767225600);
        assert_eq!(epoch_from_civil(2026, 10, 2) + 11 * 3600, 1790938800);
        // Leap day: classic bug magnet for civil-date algorithms.
        assert_eq!(epoch_from_civil(2000, 2, 29) + 12 * 3600, 951825600);
    }

    #[test]
    fn civil_from_epoch_matches_system_date_command() {
        assert_eq!(civil_from_epoch(1790938800), (2026, 10, 2, 11, 0, 0));
        assert_eq!(civil_from_epoch(951825600), (2000, 2, 29, 12, 0, 0));
        assert_eq!(civil_from_epoch(0), (1970, 1, 1, 0, 0, 0));
    }

    // --- serialize() against real captured lines ----------------------

    #[test]
    fn serialize_closed_interval_with_simple_tag() {
        let mut iv = Interval::new(Range::new(
            parse_iso("20261002T110000Z").unwrap(),
            parse_iso("20261002T133000Z").unwrap(),
        ));
        iv.add_tag("MISC");
        assert_eq!(iv.serialize(), "inc 20261002T110000Z - 20261002T133000Z # MISC");
    }

    #[test]
    fn serialize_quotes_tag_with_space() {
        // Captured from real data: `inc ... # "1:1 Josef"`
        let mut iv = Interval::new(Range::new(
            parse_iso("20261007T073000Z").unwrap(),
            parse_iso("20261007T090000Z").unwrap(),
        ));
        iv.add_tag("1:1 Josef");
        assert_eq!(iv.serialize(), "inc 20261007T073000Z - 20261007T090000Z # \"1:1 Josef\"");
    }

    #[test]
    fn serialize_quotes_tag_with_hyphen() {
        // Verified empirically: real `timew track ... "BAGE-DPP"` writes
        // `# "BAGE-DPP"` (hyphen is in the operator-char set), even though
        // some hand-written/imported data in the wild has it unquoted.
        let mut iv = Interval::new(Range::new(
            parse_iso("20261001T070000Z").unwrap(),
            parse_iso("20261001T080000Z").unwrap(),
        ));
        iv.add_tag("BAGE-DPP");
        assert_eq!(iv.serialize(), "inc 20261001T070000Z - 20261001T080000Z # \"BAGE-DPP\"");
    }

    #[test]
    fn serialize_escapes_embedded_quote() {
        // Verified empirically against real timew: tag `a "quoted" tag` ->
        // `# "a \"quoted\" tag"`.
        let mut iv = Interval::new(Range::new(
            parse_iso("20261001T070000Z").unwrap(),
            parse_iso("20261001T080000Z").unwrap(),
        ));
        iv.add_tag("a \"quoted\" tag");
        assert_eq!(iv.serialize(), "inc 20261001T070000Z - 20261001T080000Z # \"a \\\"quoted\\\" tag\"");
    }

    #[test]
    fn serialize_open_interval_has_no_end() {
        let iv = Interval::new(Range::new(parse_iso("20261001T070000Z").unwrap(), 0));
        assert_eq!(iv.serialize(), "inc 20261001T070000Z");
    }

    #[test]
    fn serialize_multiple_tags_sorted() {
        let mut iv = Interval::new(Range::new(
            parse_iso("20261001T070000Z").unwrap(),
            parse_iso("20261001T080000Z").unwrap(),
        ));
        iv.add_tag("zeta");
        iv.add_tag("alpha");
        assert_eq!(iv.serialize(), "inc 20261001T070000Z - 20261001T080000Z # alpha zeta");
    }

    // --- parse() round trip ---------------------------------------------

    #[test]
    fn parse_real_data_line_simple() {
        let iv = Interval::parse("inc 20261002T110000Z - 20261002T133000Z # MISC").unwrap().unwrap();
        assert_eq!(iv.range.start, parse_iso("20261002T110000Z").unwrap());
        assert_eq!(iv.range.end, parse_iso("20261002T133000Z").unwrap());
        assert_eq!(iv.tags, vec!["MISC".to_string()]);
    }

    #[test]
    fn parse_real_data_line_quoted_tag() {
        let iv = Interval::parse("inc 20261007T073000Z - 20261007T090000Z # \"1:1 Josef\"").unwrap().unwrap();
        assert_eq!(iv.tags, vec!["1:1 Josef".to_string()]);
    }

    #[test]
    fn parse_is_lenient_about_unquoted_hyphenated_tag() {
        // Real-world data (from our own earlier import) has unquoted
        // `# BAGE-DPP`; the real timew reads this fine, so we must too.
        let iv = Interval::parse("inc 20261002T080000Z - 20261002T090000Z # BAGE-DPP").unwrap().unwrap();
        assert_eq!(iv.tags, vec!["BAGE-DPP".to_string()]);
    }

    #[test]
    fn parse_open_interval_no_end() {
        let iv = Interval::parse("inc 20261001T070000Z # OPEN").unwrap().unwrap();
        assert!(iv.is_open());
        assert_eq!(iv.tags, vec!["OPEN".to_string()]);
    }

    #[test]
    fn parse_blank_line_is_none() {
        assert_eq!(Interval::parse("").unwrap(), None);
        assert_eq!(Interval::parse("   ").unwrap(), None);
    }

    #[test]
    fn parse_rejects_non_inc_line() {
        assert!(Interval::parse("foo bar").is_err());
    }

    #[test]
    fn parse_with_annotation() {
        let iv = Interval::parse("inc 20261001T070000Z - 20261001T080000Z # MISC # \"a note\"").unwrap().unwrap();
        assert_eq!(iv.tags, vec!["MISC".to_string()]);
        assert_eq!(iv.annotation.as_deref(), Some("a note"));
    }

    #[test]
    fn serialize_then_parse_round_trip_matches_original() {
        let mut iv = Interval::new(Range::new(
            parse_iso("20261015T140000Z").unwrap(),
            parse_iso("20261015T144500Z").unwrap(),
        ));
        iv.add_tag("BAG - EA - Microsoft Teams-");
        let line = iv.serialize();
        let parsed = Interval::parse(&line).unwrap().unwrap();
        assert_eq!(parsed, iv);
    }

    // --- JSON export -----------------------------------------------------

    #[test]
    fn json_matches_real_export_sample() {
        // Captured from: TIMEWARRIORDB=<sandbox> timew export ...
        // {"id":1,"start":"20261001T070000Z","end":"20261001T080000Z","tags":["a \"quoted\" tag"]}
        let mut iv = Interval::new(Range::new(
            parse_iso("20261001T070000Z").unwrap(),
            parse_iso("20261001T080000Z").unwrap(),
        ));
        iv.add_tag("a \"quoted\" tag");
        assert_eq!(
            iv.to_json(1),
            "{\"id\":1,\"start\":\"20261001T070000Z\",\"end\":\"20261001T080000Z\",\"tags\":[\"a \\\"quoted\\\" tag\"]}"
        );
    }

    #[test]
    fn json_open_interval_omits_end() {
        let mut iv = Interval::new(Range::new(parse_iso("20261001T070000Z").unwrap(), 0));
        iv.add_tag("OPEN");
        assert_eq!(iv.to_json(5), "{\"id\":5,\"start\":\"20261001T070000Z\",\"tags\":[\"OPEN\"]}");
    }
}
