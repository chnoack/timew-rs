//! `timew import [<file> ...]`: add intervals from JSON (as written by
//! `export`), read from the files or from stdin.

use std::io::Read;

use crate::interval::Interval;
use crate::session::Session;
use crate::validate::auto_adjust;

fn parse(content: &str) -> Result<Vec<Interval>, String> {
    if content.trim().is_empty() {
        return Err("Contents invalid.".to_string());
    }
    let value: serde_json::Value = serde_json::from_str(content).map_err(|_| "Contents invalid.".to_string())?;
    let items = value.as_array().ok_or_else(|| "Expected JSON array for import data.".to_string())?;
    items.iter().map(|item| Interval::from_json_value(item).map(|(i, _)| i)).collect()
}

fn import(s: &mut Session, intervals: &[Interval]) -> Result<(), String> {
    let adjust = s.cli.has_hint("adjust");
    for interval in intervals {
        if auto_adjust(s, adjust, interval)? {
            s.add(interval)?;
        }
    }
    Ok(())
}

pub fn run(s: &mut Session) -> Result<i32, String> {
    let files = s.cli.words();
    if files.is_empty() {
        let mut content = String::new();
        std::io::stdin().read_to_string(&mut content).map_err(|e| e.to_string())?;
        let intervals = parse(&content)?;
        import(s, &intervals)?;
        if s.verbose() {
            s.out.push_str(&format!("Imported {} interval(s).\n", intervals.len()));
        }
        return Ok(0);
    }
    for file in files {
        let result = std::fs::read_to_string(&file)
            .map_err(|_| format!("File {file} does not exist or cannot be read!"))
            .and_then(|content| parse(&content));
        let intervals = result.map_err(|e| format!("Error importing '{file}': {e}"))?;
        import(s, &intervals).map_err(|e| format!("Error importing '{file}': {e}"))?;
        if s.verbose() {
            s.out.push_str(&format!("Imported {} interval(s) from '{file}'.\n", intervals.len()));
        }
    }
    Ok(0)
}
