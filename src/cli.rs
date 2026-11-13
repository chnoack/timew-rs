//! Command line analysis.
//!
//! Every argument is classified as one of:
//!
//! - the command (the first word that is a command name or an unambiguous
//!   prefix of one; extension reports count as commands),
//! - a hint, `:name` (also unambiguous prefixes, such as `:ye` would be if
//!   it were unique),
//! - an ID, `@N` with N >= 1,
//! - a configuration override, `rc.name=value` or `rc.name:value`,
//! - a date (anything `datetime` can parse) or a duration,
//! - a range keyword: `from`, `since`, `to`, `until`, `-`, `for`, `before`,
//!   `after`, `ago`,
//! - a DOM reference, `dom.…`,
//! - otherwise a tag.

use std::collections::BTreeSet;

use crate::datetime;
use crate::duration;
use crate::range::{Epoch, Range};
use crate::text::auto_complete;

pub const COMMANDS: &[&str] = &[
    "annotate", "cancel", "config", "continue", "delete", "diagnostics", "export", "extensions", "fill", "gaps",
    "get", "help", "--help", "-h", "import", "join", "lengthen", "modify", "move", "report", "resize", "retag",
    "shorten", "show", "split", "start", "stop", "tag", "tags", "track", "undo", "untag",
];

/// Reports that are built in but behave like extensions.
pub const BUILTIN_REPORTS: &[&str] = &["day", "month", "summary", "week"];

pub const HINTS: &[&str] = &[
    ":adjust", ":all", ":annotations", ":no-annotations", ":blank", ":color", ":nocolor", ":day", ":debug", ":fill",
    ":fortnight", ":holidays", ":no-holidays", ":ids", ":no-ids", ":lastmonth", ":lastquarter", ":lastweek",
    ":lastyear", ":month", ":quarter", ":quiet", ":tags", ":no-tags", ":today", ":week", ":year", ":yes",
    ":yesterday", ":monday", ":tuesday", ":wednesday", ":thursday", ":friday", ":saturday", ":sunday",
];

const KEYWORDS: &[&str] = &["from", "since", "to", "until", "-", "for", "before", "after", "ago"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    Command,
    /// An extension report (or a built-in one such as `summary`).
    Report,
    /// A word consumed by a command as its subcommand (`modify start`).
    SubCommand,
    Hint,
    Id(u32),
    Override { name: String, value: String },
    Date,
    Duration,
    Keyword,
    Dom,
    Tag,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arg {
    pub raw: String,
    /// The full name for commands, reports and hints given as a prefix.
    pub canonical: String,
    pub role: Role,
}

#[derive(Debug, Clone, Default)]
pub struct Cli {
    pub args: Vec<Arg>,
}

/// Is the whole token a UUID (or the start of one, 8+ characters)? Such
/// tokens are never dates.
fn is_uuid_like(s: &str) -> bool {
    const PATTERN: &[u8] = b"xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx";
    let b = s.as_bytes();
    if b.len() < 8 || b.len() > 36 {
        return false;
    }
    b.iter().zip(PATTERN).all(|(c, p)| if *p == b'x' { c.is_ascii_hexdigit() } else { c == p })
}

fn is_quoted(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 2 && (b[0] == b'"' || b[0] == b'\'') && b[b.len() - 1] == b[0]
}

/// The kind of value a token is: a date, a duration or neither.
fn lex(s: &str) -> Role {
    if s.is_empty() || is_quoted(s) || is_uuid_like(s) {
        return Role::Tag;
    }
    if s.contains(char::is_whitespace) {
        return if duration::parse(s).is_some() { Role::Duration } else { Role::Tag };
    }
    if datetime::parse_point(s).is_some() {
        return Role::Date;
    }
    if duration::parse(s).is_some() {
        return Role::Duration;
    }
    Role::Tag
}

/// `@N`: `Ok(None)` if not an ID at all, an error for `@0`.
fn parse_id(token: &str) -> Result<Option<u32>, String> {
    let Some(digits) = token.strip_prefix('@') else { return Ok(None) };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Ok(None);
    }
    let value: u64 = digits.trim_start_matches('0').parse().unwrap_or(0);
    if value == 0 {
        return Err("'@0' is not a valid ID.".to_string());
    }
    u32::try_from(value).map(Some).map_err(|_| format!("'{token}' is not a valid ID."))
}

fn canonical(value: &str, names: &[&str]) -> Option<String> {
    match auto_complete(value, names.iter().copied()).as_slice() {
        [one] => Some(one.to_string()),
        _ => None,
    }
}

impl Cli {
    /// Analyze `argv[1..]`. `reports` are the names of the installed
    /// extensions, which may be used as commands.
    pub fn parse(args: &[String], reports: &[String]) -> Result<Cli, String> {
        let mut report_names: Vec<&str> = BUILTIN_REPORTS.to_vec();
        report_names.extend(reports.iter().map(String::as_str));

        let mut out = Vec::with_capacity(args.len());
        let mut have_command = false;
        for raw in args {
            // Control characters become spaces.
            let raw: String = raw.chars().map(|c| if (c as u32) <= 32 { ' ' } else { c }).collect();
            let raw = raw.trim().to_string();

            if let Some(rest) = raw.strip_prefix("rc.")
                && let Some(sep) = rest.find(['=', ':'])
                && sep > 0
            {
                let role = Role::Override { name: rest[..sep].to_string(), value: rest[sep + 1..].to_string() };
                out.push(Arg { canonical: raw.clone(), raw, role });
                continue;
            }
            if let Some(id) = parse_id(&raw)? {
                out.push(Arg { canonical: raw.clone(), raw, role: Role::Id(id) });
                continue;
            }
            if have_command && (raw == "--help" || raw == "-h") {
                // `timew <command> --help` is `timew help <command>`.
                for a in out.iter_mut() {
                    if matches!(a.role, Role::Command | Role::Report) {
                        a.role = Role::Tag;
                    }
                }
                out.push(Arg { canonical: "help".to_string(), raw, role: Role::Command });
                continue;
            }
            if !have_command
                && let Some(c) = canonical(&raw, COMMANDS) {
                    have_command = true;
                    out.push(Arg { raw, canonical: c, role: Role::Command });
                    continue;
                }
            if raw.starts_with(':')
                && let Some(h) = canonical(&raw, HINTS)
            {
                out.push(Arg { raw, canonical: h, role: Role::Hint });
                continue;
            }
            if !have_command && let Some(r) = canonical(&raw, &report_names) {
                have_command = true;
                out.push(Arg { raw, canonical: r, role: Role::Report });
                continue;
            }
            let role = if KEYWORDS.contains(&raw.as_str()) {
                Role::Keyword
            } else if raw.starts_with("dom.") {
                Role::Dom
            } else {
                lex(&raw)
            };
            out.push(Arg { canonical: raw.clone(), raw, role });
        }
        Ok(Cli { args: out })
    }

    /// The command or report name, if any.
    pub fn command(&self) -> Option<&str> {
        self.args.iter().find(|a| a.role == Role::Command).or_else(|| self.args.iter().find(|a| a.role == Role::Report)).map(|a| a.canonical.as_str())
    }

    /// Everything that is not the command, a hint or an override, as typed.
    pub fn words(&self) -> Vec<String> {
        self.args
            .iter()
            .filter(|a| !matches!(a.role, Role::Command | Role::Report | Role::SubCommand | Role::Hint | Role::Override { .. }))
            .map(|a| a.raw.clone())
            .collect()
    }

    /// Like [`Cli::words`] but without IDs: the free text of a command.
    pub fn text(&self) -> String {
        self.args
            .iter()
            .filter(|a| {
                !matches!(a.role, Role::Command | Role::Report | Role::SubCommand | Role::Hint | Role::Override { .. } | Role::Id(_))
            })
            .map(|a| a.raw.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn ids(&self) -> BTreeSet<u32> {
        self.args.iter().filter_map(|a| if let Role::Id(id) = a.role { Some(id) } else { None }).collect()
    }

    pub fn tags(&self) -> Vec<String> {
        let set: BTreeSet<String> = self.args.iter().filter(|a| a.role == Role::Tag).map(|a| a.raw.clone()).collect();
        set.into_iter().collect()
    }

    /// The (last) duration given, in seconds.
    pub fn duration(&self) -> Option<i64> {
        self.args.iter().filter(|a| a.role == Role::Duration).filter_map(|a| duration::parse(&a.raw)).next_back()
    }

    pub fn dom_refs(&self) -> Vec<String> {
        self.args.iter().filter(|a| a.role == Role::Dom).map(|a| a.raw.clone()).collect()
    }

    pub fn overrides(&self) -> Vec<(String, String)> {
        self.args
            .iter()
            .filter_map(|a| match &a.role {
                Role::Override { name, value } => Some((name.clone(), value.clone())),
                _ => None,
            })
            .collect()
    }

    pub fn has_hint(&self, name: &str) -> bool {
        let wanted = format!(":{name}");
        self.args.iter().any(|a| a.role == Role::Hint && a.canonical == wanted)
    }

    /// `:name` → true, `:no-name` → false, otherwise `default`.
    pub fn hint_or(&self, name: &str, default: bool) -> bool {
        if self.has_hint(name) {
            true
        } else if self.has_hint(&format!("no-{name}")) {
            false
        } else {
            default
        }
    }

    /// Claim the first tag that is one of `names` as the subcommand.
    pub fn take_subcommand(&mut self, names: &[&str]) -> Option<String> {
        let arg = self.args.iter_mut().find(|a| a.role == Role::Tag && names.contains(&a.raw.as_str()))?;
        arg.role = Role::SubCommand;
        Some(arg.raw.clone())
    }

    /// The raw text of the dates given.
    pub fn dates(&self) -> Vec<String> {
        self.args.iter().filter(|a| a.role == Role::Date).map(|a| a.raw.clone()).collect()
    }

    /// The range given on the command line, or `default` if none:
    ///
    /// ```text
    /// [from] <date>                  (a lone date may name a whole day/week/month)
    /// [from] <date> to|- <date>
    /// [from] <date> for <duration>
    /// <duration> before|after <date>
    /// <duration> ago
    /// [for] <duration>
    /// :hint
    /// ```
    pub fn range(&self, default: Range, now: Epoch) -> Result<Range, String> {
        #[derive(PartialEq)]
        enum T {
            Date,
            Duration,
            All,
            Word(&'static str),
        }
        let mut shape: Vec<T> = Vec::new();
        let mut dates: Vec<Epoch> = Vec::new();
        let mut first_date = String::new();
        let mut dur = 0;
        let mut names: Vec<String> = Vec::new();

        for a in &self.args {
            match &a.role {
                Role::Hint => {
                    if let Some(r) = datetime::expand_range_hint_at(&a.canonical[1..], now) {
                        if r.is_unbounded() {
                            shape.push(T::All);
                            names.push("<all>".into());
                        } else {
                            dates.push(r.start);
                            dates.push(r.end);
                            shape.extend([T::Date, T::Word("to"), T::Date]);
                            names.extend(["<date>".into(), "-".into(), "<date>".into()]);
                        }
                    }
                }
                Role::Date => {
                    if first_date.is_empty() {
                        first_date = a.raw.clone();
                    }
                    dates.push(datetime::parse_point(&a.raw).ok_or_else(|| format!("'{}' is not a valid date.", a.raw))?);
                    shape.push(T::Date);
                    names.push("<date>".into());
                }
                Role::Duration => {
                    if shape.iter().all(|t| *t != T::Duration) {
                        dur = duration::parse(&a.raw).unwrap_or(0);
                    }
                    shape.push(T::Duration);
                    names.push("<duration>".into());
                }
                Role::Keyword => {
                    let word = match a.raw.as_str() {
                        "since" => "from",
                        "until" => "to",
                        "-" => "to",
                        w => KEYWORDS.iter().find(|k| **k == w).copied().unwrap_or("to"),
                    };
                    shape.push(T::Word(word));
                    names.push(a.raw.clone());
                }
                _ => {}
            }
        }

        use T::*;
        let d = |i: usize| dates.get(i).copied().unwrap_or(0);
        let range = match shape.as_slice() {
            [] => default,
            [All] => Range::empty(),
            [Date] => datetime::parse_range(&first_date)?,
            [Word("from"), Date] => Range::new(d(0), 0),
            [Date, Word("to"), Date] | [Word("from"), Date, Word("to"), Date] => Range::new(d(0), d(1)),
            [Date, Word("for"), Duration] | [Word("from"), Date, Word("for"), Duration] => Range::new(d(0), d(0) + dur),
            [Duration, Word("before"), Date] => Range::new(d(0) - dur, d(0)),
            [Duration, Word("after"), Date] => Range::new(d(0), d(0) + dur),
            [Duration, Word("ago")] => Range::new(now - dur, 0),
            [Word("for"), Duration] | [Duration] => Range::new(now - dur, now),
            _ => return Err(format!("Unrecognized date range: '{}'.", names.join(" "))),
        };
        if range.end != 0 && range.start > range.end {
            return Err("The end of a date range must be after the start.".to_string());
        }
        Ok(range)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datetime::make_local;

    fn cli(words: &[&str]) -> Cli {
        let args: Vec<String> = words.iter().map(|s| s.to_string()).collect();
        Cli::parse(&args, &["totals.py".to_string()]).unwrap()
    }

    #[test]
    fn classifies() {
        let c = cli(&["track", "@3", ":adj", "2025-03-01T08:00", "-", "09:00", "foo", "rc.verbose=off", "dom.active"]);
        assert_eq!(c.command(), Some("track"));
        assert_eq!(c.ids().into_iter().collect::<Vec<_>>(), vec![3]);
        assert!(c.has_hint("adjust"));
        assert_eq!(c.tags(), vec!["foo".to_string()]);
        assert_eq!(c.overrides(), vec![("verbose".to_string(), "off".to_string())]);
        assert_eq!(c.dom_refs(), vec!["dom.active".to_string()]);
        assert_eq!(c.dates().len(), 2);
    }

    #[test]
    fn command_prefixes_and_reports() {
        assert_eq!(cli(&["sum"]).command(), Some("summary"));
        assert_eq!(cli(&["tot"]).command(), Some("totals.py"));
        assert_eq!(cli(&["st"]).command(), None);
        assert_eq!(cli(&["start", "--help"]).command(), Some("help"));
        assert_eq!(cli(&["start", "--help"]).words(), vec!["start".to_string()]);
        // After the command, report names are just tags.
        assert_eq!(cli(&["start", "summary"]).tags(), vec!["summary".to_string()]);
    }

    #[test]
    fn ids() {
        assert!(Cli::parse(&["delete".into(), "@0".into()], &[]).is_err());
        assert_eq!(cli(&["tag", "@12x"]).tags(), vec!["@12x".to_string()]);
    }

    #[test]
    fn ranges() {
        let now = make_local(2025, 3, 5, 12, 0, 0);
        let r = |w: &[&str]| cli(w).range(Range::new(1, 2), now);
        assert_eq!(r(&["export"]).unwrap(), Range::new(1, 2));
        assert_eq!(r(&["export", ":all"]).unwrap(), Range::empty());
        assert_eq!(
            r(&["export", "2025-03-01", "-", "2025-03-03"]).unwrap(),
            Range::new(make_local(2025, 3, 1, 0, 0, 0), make_local(2025, 3, 3, 0, 0, 0))
        );
        assert_eq!(
            r(&["export", "2025-03-01"]).unwrap(),
            Range::new(make_local(2025, 3, 1, 0, 0, 0), make_local(2025, 3, 2, 0, 0, 0))
        );
        assert_eq!(r(&["export", "from", "2025-03-01"]).unwrap(), Range::new(make_local(2025, 3, 1, 0, 0, 0), 0));
        assert_eq!(r(&["export", "1h", "ago"]).unwrap(), Range::new(now - 3600, 0));
        assert_eq!(r(&["export", "1h"]).unwrap(), Range::new(now - 3600, now));
        assert_eq!(
            r(&["export", "2025-03-01T08:00", "for", "2h"]).unwrap(),
            Range::new(make_local(2025, 3, 1, 8, 0, 0), make_local(2025, 3, 1, 10, 0, 0))
        );
        assert!(r(&["export", "2025-03-03", "-", "2025-03-01"]).is_err());
        assert!(r(&["export", ":week", "2025-03-01"]).is_err());
    }
}
