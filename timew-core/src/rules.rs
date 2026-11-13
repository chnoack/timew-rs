//! Configuration (`timewarrior.cfg`): built-in defaults, the file format,
//! and editing the file for `timew config`.
//!
//! The file contains `name = value` settings, comments starting with `#`,
//! `import /absolute/path` lines, and indented rule blocks:
//!
//! ```text
//! define tags:
//!   meeting:
//!     color = black on yellow
//! ```
//!
//! which is the same as `tags.meeting.color = black on yellow`. Rule
//! blocks may be of type `tags`, `reports`, `theme`, `holidays` and
//! `exclusions`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::text::{dequote, is_true};

pub const RULE_TYPES: [&str; 5] = ["tags", "reports", "theme", "holidays", "exclusions"];

const DEFAULTS: &[(&str, &str)] = &[
    ("confirmation", "on"),
    ("debug", "off"),
    ("verbose", "on"),
    ("reports.day.hours", "all"),
    ("reports.day.axis", "internal"),
    ("reports.day.lines", "2"),
    ("reports.day.spacing", "1"),
    ("reports.day.month", "no"),
    ("reports.day.week", "no"),
    ("reports.day.day", "yes"),
    ("reports.day.weekday", "yes"),
    ("reports.day.totals", "no"),
    ("reports.day.summary", "yes"),
    ("reports.day.holidays", "no"),
    ("reports.day.cell", "15"),
    ("reports.week.hours", "all"),
    ("reports.week.lines", "1"),
    ("reports.week.spacing", "1"),
    ("reports.week.month", "no"),
    ("reports.week.week", "yes"),
    ("reports.week.day", "yes"),
    ("reports.week.weekday", "yes"),
    ("reports.week.totals", "yes"),
    ("reports.week.summary", "yes"),
    ("reports.week.holidays", "yes"),
    ("reports.week.cell", "15"),
    ("reports.month.hours", "all"),
    ("reports.month.lines", "1"),
    ("reports.month.spacing", "1"),
    ("reports.month.month", "yes"),
    ("reports.month.week", "yes"),
    ("reports.month.day", "yes"),
    ("reports.month.weekday", "yes"),
    ("reports.month.totals", "yes"),
    ("reports.month.summary", "yes"),
    ("reports.month.holidays", "yes"),
    ("reports.month.cell", "15"),
    ("reports.summary.holidays", "yes"),
    ("theme.description", "Built-in default"),
    ("theme.colors.exclusion", "gray8 on gray4"),
    ("theme.colors.today", "white"),
    ("theme.colors.holiday", "gray4"),
    ("theme.colors.label", "gray4"),
];

/// The lines a new configuration file starts with.
pub const NEW_FILE: &str = "reports.summary.ids = yes\nreports.summary.annotations = yes\n";

#[derive(Debug, Clone)]
pub struct Rules {
    settings: BTreeMap<String, String>,
    file: PathBuf,
}

impl Default for Rules {
    fn default() -> Self {
        Rules {
            settings: DEFAULTS.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            file: PathBuf::new(),
        }
    }
}

impl Rules {
    /// The defaults, overlaid with the settings in `file`.
    pub fn load(file: &Path) -> Result<Rules, String> {
        let mut rules = Rules { file: file.to_path_buf(), ..Rules::default() };
        if !file.exists() {
            return Err("ERROR: Configuration file not found.".to_string());
        }
        let contents = fs::read_to_string(file)
            .map_err(|_| "ERROR: Configuration file cannot be read (insufficient privileges).".to_string())?;
        rules.parse(&contents, 1)?;
        Ok(rules)
    }

    pub fn file(&self) -> &Path {
        &self.file
    }

    pub fn has(&self, key: &str) -> bool {
        self.settings.contains_key(key)
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.settings.get(key).map(String::as_str)
    }

    pub fn get_or(&self, key: &str, default: &str) -> String {
        self.get(key).unwrap_or(default).to_string()
    }

    pub fn get_bool(&self, key: &str, default: bool) -> bool {
        self.get(key).map(is_true).unwrap_or(default)
    }

    pub fn get_int(&self, key: &str, default: i64) -> Result<i64, String> {
        match self.get(key) {
            None => Ok(default),
            Some(v) => v.trim().parse().map_err(|_| format!("Invalid integer value for '{key}': '{v}'")),
        }
    }

    pub fn set(&mut self, key: &str, value: &str) {
        self.settings.insert(key.to_string(), value.to_string());
    }

    /// All keys, sorted, optionally only those starting with `prefix`.
    pub fn keys(&self, prefix: &str) -> Vec<String> {
        self.settings.keys().filter(|k| k.starts_with(prefix)).cloned().collect()
    }

    pub fn is_rule_type(name: &str) -> bool {
        RULE_TYPES.contains(&name)
    }

    /// Parse configuration text, `nest` levels deep in imports.
    pub fn parse(&mut self, input: &str, nest: usize) -> Result<(), String> {
        if nest > 10 {
            return Err("Rules files may only be nested to 10 levels.".to_string());
        }
        let mut block: Vec<String> = Vec::new();
        for raw in input.lines() {
            let line = match raw.find('#') {
                Some(i) => raw[..i].trim_end(),
                None => raw,
            };
            if line.trim().is_empty() {
                continue;
            }
            let indent = line.len() - line.trim_start_matches(' ').len();
            if !block.is_empty() {
                if indent > 0 {
                    block.push(line.to_string());
                    continue;
                }
                self.parse_block(&block)?;
                block.clear();
            }

            let trimmed = line.trim();
            let mut words = trimmed.split_whitespace();
            let first = words.next().unwrap_or("");
            if first == "define" {
                block.push(line.to_string());
            } else if first == "import"
                && let Some(path) = words.next()
                && words.next().is_none()
                && !trimmed.contains('=')
            {
                let path = Path::new(path);
                if !path.is_absolute() {
                    return Err(format!("Can only import files with absolute paths, not '{}'.", path.display()));
                }
                let contents = fs::read_to_string(path)
                    .map_err(|_| format!("Could not read imported file '{}'.", path.display()))?;
                self.parse(&contents, nest + 1)?;
            } else if let Some((name, value)) = line.split_once('=') {
                self.set(name.trim(), value.trim());
            } else {
                return Err(format!("Unrecognized construct: {line}"));
            }
        }
        if !block.is_empty() {
            self.parse_block(&block)?;
        }
        Ok(())
    }

    /// A `define <type>:` block: nested `group:` lines and `name = value`
    /// settings, structured by indentation.
    fn parse_block(&mut self, lines: &[String]) -> Result<(), String> {
        let header = lines[0].trim();
        let kind = header.trim_start_matches("define").trim().trim_end_matches(':').trim();
        if !Self::is_rule_type(kind) {
            return Err(format!("Unrecognized rule type '{header}'"));
        }
        let mut groups: Vec<(usize, String)> = vec![(0, kind.to_string())];
        for line in &lines[1..] {
            let indent = line.len() - line.trim_start_matches(' ').len();
            while groups.len() > 1 && groups.last().unwrap().0 >= indent {
                groups.pop();
            }
            let content = line.trim();
            if let Some((name, value)) = content.split_once('=') {
                let prefix: Vec<&str> = groups.iter().map(|(_, g)| g.as_str()).collect();
                let key = format!("{}.{}", prefix.join("."), name.trim());
                self.set(&key, dequote(value.trim()));
            } else if let Some(group) = content.strip_suffix(':') {
                groups.push((indent, group.trim().to_string()));
            } else {
                return Err(format!("Syntax error in rule: missing '=' in line '{line}'."));
            }
        }
        Ok(())
    }
}

/// A change to the configuration file: the line before and after.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigChange {
    pub before: String,
    pub after: String,
}

/// Where a `name = value` setting appears in the file's lines.
fn find_setting(lines: &[String], name: &str) -> Option<usize> {
    lines.iter().position(|line| {
        let content = match line.find('#') {
            Some(i) => &line[..i],
            None => line.as_str(),
        };
        content.split_once('=').map(|(k, _)| k.trim() == name).unwrap_or(false)
    })
}

fn read_lines(file: &Path) -> Vec<String> {
    fs::read_to_string(file).map(|s| s.lines().map(str::to_string).collect()).unwrap_or_default()
}

fn write_lines(file: &Path, lines: &[String]) -> Result<(), String> {
    let mut contents = lines.join("\n");
    contents.push('\n');
    crate::datafile::atomic_write(file, &contents).map_err(|e| format!("Could not write {}: {e}", file.display()))
}

/// Set `name` to `value` in the configuration file. `confirm` is asked
/// before changing anything. Returns the change made, if any.
pub fn set_config_variable(
    rules: &Rules,
    name: &str,
    value: &str,
    confirm: &mut dyn FnMut(&str) -> bool,
) -> Result<Option<ConfigChange>, String> {
    let mut lines = read_lines(rules.file());
    let new_line = format!("{name} = {value}");
    let change = if let Some(i) = find_setting(&lines, name) {
        let current = rules.get(name).unwrap_or("");
        if current == value {
            return Ok(None);
        }
        if !confirm(&format!("Are you sure you want to change the value of '{name}' from '{current}' to '{value}'?")) {
            return Ok(None);
        }
        let before = std::mem::replace(&mut lines[i], new_line.clone());
        ConfigChange { before, after: new_line }
    } else {
        if rules.get(name) == Some(value) {
            return Ok(None);
        }
        let question = match rules.get(name) {
            Some(current) => {
                format!("Are you sure you want to change the value of '{name}' from '{current}' to '{value}'?")
            }
            None => format!("Are you sure you want to add '{name}' with a value of '{value}'?"),
        };
        if !confirm(&question) {
            return Ok(None);
        }
        lines.push(new_line.clone());
        ConfigChange { before: String::new(), after: new_line }
    };
    write_lines(rules.file(), &lines)?;
    Ok(Some(change))
}

/// Result of removing a setting from the configuration file.
#[derive(Debug, PartialEq, Eq)]
pub enum Unset {
    Removed(ConfigChange),
    Declined,
    NotFound,
}

/// Remove the `name = ...` line from the configuration file.
pub fn unset_config_variable(rules: &Rules, name: &str, confirm: &mut dyn FnMut(&str) -> bool) -> Result<Unset, String> {
    let mut lines = read_lines(rules.file());
    let Some(i) = find_setting(&lines, name) else {
        return Ok(Unset::NotFound);
    };
    if !confirm(&format!("Are you sure you want to remove '{name}'?")) {
        return Ok(Unset::Declined);
    }
    let before = lines.remove(i);
    write_lines(rules.file(), &lines)?;
    Ok(Unset::Removed(ConfigChange { before, after: String::new() }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults() {
        let r = Rules::default();
        assert!(r.get_bool("verbose", false));
        assert_eq!(r.get_int("reports.day.cell", 0).unwrap(), 15);
        assert_eq!(r.get("nonexistent"), None);
    }

    #[test]
    fn parses_settings_and_blocks() {
        let mut r = Rules::default();
        r.parse(
            "# comment\nverbose = off\nexclusions.monday = <8:00 >17:00   # trailing\n\ndefine tags:\n  meeting:\n    color = \"black on yellow\"\n  other:\n    color = red\ndefine holidays:\n  en-US:\n    2025_01_01 = New Year's Day\nafter = 1\n",
            1,
        )
        .unwrap();
        assert!(!r.get_bool("verbose", true));
        assert_eq!(r.get("exclusions.monday"), Some("<8:00 >17:00"));
        assert_eq!(r.get("tags.meeting.color"), Some("black on yellow"));
        assert_eq!(r.get("tags.other.color"), Some("red"));
        assert_eq!(r.get("holidays.en-US.2025_01_01"), Some("New Year's Day"));
        assert_eq!(r.get("after"), Some("1"));
    }

    #[test]
    fn rejects_garbage() {
        let mut r = Rules::default();
        assert!(r.parse("this is not valid\n", 1).is_err());
        assert!(r.parse("define nonsense:\n  a = b\n", 1).is_err());
        assert!(r.parse("import relative/path\n", 1).is_err());
    }
}
