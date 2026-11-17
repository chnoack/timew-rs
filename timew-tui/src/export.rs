//! `x`: export the rows of the table (the current filters) as CSV, with
//! the annotations (see [`timew_core::summary::to_csv`]).

use std::path::{Path, PathBuf};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use timew_core::rules::Rules;
use timew_core::summary;

use crate::app::App;
use crate::format::local_date;

/// The open export dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExportForm {
    /// The file to write, relative to the working directory or `~/…`.
    pub(crate) path: String,
    pub(crate) error: Option<String>,
    /// The file exists and the next Enter overwrites it.
    pub(crate) confirm_overwrite: bool,
}

impl App {
    pub(crate) fn open_export(&mut self) {
        self.export = Some(ExportForm { path: self.export_file_name(), error: None, confirm_overwrite: false });
    }

    /// `timew-2026-11-01_2026-11-30.csv`, `timew-2026-11-17-meetings.csv`
    /// (the shown period and tag), `timew-since-2026-11-01.csv`.
    pub(crate) fn export_file_name(&self) -> String {
        let r = &self.range;
        let mut name = String::from("timew-");
        if !r.is_started() {
            name.push_str("all");
        } else if !r.is_ended() {
            name.push_str(&format!("since-{}", local_date(r.start)));
        } else {
            let (from, to) = (local_date(r.start), local_date(r.end - 1));
            name.push_str(&if from == to { from } else { format!("{from}_{to}") });
        }
        if let Some(tag) = &self.tag {
            name.push('-');
            name.push_str(tag);
        }
        let name: String =
            name.chars().map(|c| if c.is_alphanumeric() || "-_.".contains(c) { c } else { '_' }).collect();
        name + ".csv"
    }

    pub(crate) fn on_export_key(&mut self, key: KeyEvent) {
        let Some(form) = &mut self.export else { return };
        match key.code {
            KeyCode::Esc => self.export = None,
            KeyCode::Enter => self.write_export(),
            KeyCode::Backspace => {
                form.path.pop();
                form.error = None;
                form.confirm_overwrite = false;
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                form.path.clear();
                form.error = None;
                form.confirm_overwrite = false;
            }
            KeyCode::Char(c) => {
                form.path.push(c);
                form.error = None;
                form.confirm_overwrite = false;
            }
            _ => {}
        }
    }

    /// `export.csv.separator` from `timewarrior.cfg` (read at each export,
    /// so a changed setting applies without a restart).
    pub(crate) fn csv_separator(&self) -> Result<char, String> {
        let file = self.edit_paths().config_file();
        if !file.exists() {
            return Ok(',');
        }
        let rules = Rules::load(&file)?;
        summary::csv_separator(rules.get(summary::CSV_SEPARATOR_SETTING))
    }

    fn write_export(&mut self) {
        if self.export.is_none() {
            return;
        }
        let separator = self.csv_separator();
        let Some(form) = &mut self.export else { return };
        let input = form.path.trim();
        if input.is_empty() {
            form.error = Some("Enter a file name.".into());
            return;
        }
        let path = expand_home(input);
        if path.is_dir() {
            form.error = Some(format!("{} is a directory.", path.display()));
            return;
        }
        if path.exists() && !form.confirm_overwrite {
            form.error = Some("The file exists. Enter overwrites it, Esc cancels.".into());
            form.confirm_overwrite = true;
            return;
        }
        let separator = match separator {
            Ok(c) => c,
            Err(e) => return form.error = Some(e),
        };
        match std::fs::write(&path, summary::to_csv(&self.summary, separator)) {
            Ok(()) => {
                let shown = std::path::absolute(&path).unwrap_or(path);
                let n = self.summary.rows.len();
                let s = if n == 1 { "" } else { "s" };
                self.notice = Some(format!("Exported {n} row{s} to {}.", shown.display()));
                self.notice_failed = false;
                self.export = None;
            }
            Err(e) => {
                form.error = Some(format!("Not exported: {e}"));
                form.confirm_overwrite = false;
            }
        }
    }
}

/// `~/x` (and `~`) in the home directory; anything else as given.
fn expand_home(input: &str) -> PathBuf {
    let home = std::env::var_os("HOME").filter(|h| !h.is_empty());
    match (input.strip_prefix("~"), home) {
        (Some(""), Some(h)) => PathBuf::from(h),
        (Some(rest), Some(h)) if rest.starts_with('/') => Path::new(&h).join(&rest[1..]),
        _ => PathBuf::from(input),
    }
}
