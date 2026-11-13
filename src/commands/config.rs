//! `timew config [<name> [<value> | '']]`: show the configuration, set a
//! value, or remove a setting. `timew show` shows the configuration.

use crate::rules::{set_config_variable, unset_config_variable, Rules, Unset};
use crate::session::{confirm, Session};
use crate::text::dequote;

pub fn run(s: &mut Session) -> Result<i32, String> {
    let words = s.cli.words();
    let Some(name) = words.first().cloned().filter(|n| !n.is_empty()) else {
        return show(s);
    };
    let ask = s.rules.get_bool("confirmation", true);
    let rules = s.rules.clone();
    let out = &mut s.out;
    let mut confirm_fn = |q: &str| !ask || confirm(out, q);

    s.journal.start();
    let (changed, rc) = if words.len() > 1 {
        let value = dequote(&words[1..].join(" ")).to_string();
        match set_config_variable(&rules, &name, &value, &mut confirm_fn)? {
            Some(change) => {
                s.journal.record_config(change.before, change.after);
                (true, 0)
            }
            None => (false, 1),
        }
    } else {
        match unset_config_variable(&rules, &name, &mut confirm_fn)? {
            Unset::Removed(change) => {
                s.journal.record_config(change.before, change.after);
                (true, 0)
            }
            Unset::Declined => (false, 1),
            Unset::NotFound => return Err(format!("No entry named '{name}' found.")),
        }
    };
    s.journal.end()?;
    if s.verbose() {
        if changed {
            s.out.push_str(&format!("Config file '{}' modified.\n", s.rules.file().display()));
        } else {
            s.out.push_str("No changes made.\n");
        }
    }
    Ok(rc)
}

/// All settings, grouped the way rule blocks are written.
pub fn show(s: &mut Session) -> Result<i32, String> {
    let mut previous: Vec<String> = Vec::new();
    for name in s.rules.keys("") {
        if name.starts_with("temp.") {
            continue;
        }
        let parts: Vec<String> = name.split('.').map(str::to_string).collect();
        for (i, part) in parts.iter().enumerate() {
            if i == parts.len() - 1 {
                if previous.len() > 1 && parts.len() == 1 {
                    s.out.push('\n');
                }
                let value = s.rules.get(&name).unwrap_or("");
                s.out.push_str(&format!("{}{part} = {value}\n", "  ".repeat(parts.len() - 1)));
            } else if previous.len() <= i || previous[i] != *part {
                if i == 0 {
                    s.out.push('\n');
                    if Rules::is_rule_type(part) {
                        s.out.push_str("define ");
                    }
                }
                s.out.push_str(&format!("{}{part}:\n", "  ".repeat(i)));
            }
        }
        previous = parts;
    }
    Ok(0)
}
