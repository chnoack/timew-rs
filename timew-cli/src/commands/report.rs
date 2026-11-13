//! `timew [report] <report> [<range>] [<tag> ...]`: run an extension
//! report. The extension gets the configuration as `name: value` lines,
//! an empty line, then the intervals as JSON (like `export`) on stdin.

use crate::cli::Role;
use crate::session::Session;
use crate::text::{auto_complete, join_quoted};

pub fn run(s: &mut Session) -> Result<i32, String> {
    let name = s
        .cli
        .args
        .iter()
        .find(|a| a.role == Role::Report)
        .map(|a| a.canonical.clone())
        .or_else(|| {
            // `timew report <name>`: the first word names the report.
            s.cli.args.iter().find(|a| a.role == Role::Tag).map(|a| a.raw.clone())
        })
        .ok_or_else(|| "Specify which report to run.".to_string())?;
    let names = s.extensions.names();
    let script = match auto_complete(&name, names.iter().map(String::as_str)).as_slice() {
        [] => return Err(format!("The report '{name}' is not recognized.")),
        [one] => s.extensions.find(one).cloned().unwrap(),
        many => return Err(format!("The report '{name}' is ambiguous, and could mean one of: {}", many.join(", "))),
    };
    if s.cli.command() == Some("report") {
        // Don't treat the report name as a tag.
        if let Some(a) = s.cli.args.iter_mut().find(|a| a.role == Role::Tag && a.raw == name) {
            a.role = Role::SubCommand;
        }
    }
    let file_name = script.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
    let stem = file_name.rsplit_once('.').map(|(a, _)| a.to_string()).unwrap_or(file_name.clone());

    let default = super::default_report_range(s, &stem, "all");
    let (range, tracked) = super::select_for_report(s, default)?;
    let tags = s.cli.tags();
    s.rules.set("temp.report.start", &if range.is_started() { crate::interval::format_iso(range.start) } else { String::new() });
    s.rules.set("temp.report.end", &if range.is_ended() { crate::interval::format_iso(range.end) } else { String::new() });
    s.rules.set("temp.report.tags", &join_quoted(",", &tags));
    s.rules.set("temp.version", crate::VERSION);

    let mut input = String::new();
    for key in s.rules.keys("") {
        input.push_str(&format!("{key}: {}\n", s.rules.get(&key).unwrap_or("")));
    }
    input.push('\n');
    input.push_str(&super::export::json_array(&tracked));

    let (status, output) = crate::extensions::Extensions::call(&script, &input)?;
    if status != 0 && output.is_empty() {
        return Err(format!("'{}' returned {status} without producing output.", script.display()));
    }
    for line in output {
        s.out.push_str(&line);
        s.out.push('\n');
    }
    Ok(status)
}
