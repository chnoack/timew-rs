//! The undo journal, `data/undo.data`: one transaction per command that
//! changed something, each a list of actions with a before and after state.
//!
//! ```text
//! txn:
//!   type: interval
//!   before: {"id":1,"start":"20250303T080000Z","tags":["foo"]}
//!   after: {"id":1,"start":"20250303T080000Z","end":"20250303T090000Z","tags":["foo"]}
//! ```
//!
//! For intervals, `before`/`after` are the JSON forms (empty when the
//! interval was added or deleted); for configuration changes they are the
//! changed lines of the configuration file. `journal.size` limits how many
//! transactions are kept (`-1`: unlimited, `0`: no journal).

use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    Interval,
    Config,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    pub kind: ActionKind,
    pub before: String,
    pub after: String,
}

pub type Transaction = Vec<Action>;

#[derive(Debug)]
pub struct Journal {
    path: PathBuf,
    size: i64,
    current: Option<Transaction>,
}

impl Journal {
    /// A journal at `path` keeping at most `size` transactions. With size 0
    /// the journal is disabled and an existing file is removed.
    pub fn new(path: impl Into<PathBuf>, size: i64) -> Self {
        let path = path.into();
        if size == 0 && fs::metadata(&path).map(|m| m.len() > 0).unwrap_or(false) {
            let _ = fs::remove_file(&path);
        }
        Journal { path, size, current: None }
    }

    pub fn enabled(&self) -> bool {
        self.size != 0
    }

    pub fn start(&mut self) {
        if self.enabled() && self.current.is_none() {
            self.current = Some(Vec::new());
        }
    }

    pub fn record_interval(&mut self, before: String, after: String) {
        self.record(ActionKind::Interval, before, after);
    }

    pub fn record_config(&mut self, before: String, after: String) {
        self.record(ActionKind::Config, before, after);
    }

    fn record(&mut self, kind: ActionKind, before: String, after: String) {
        if let Some(txn) = &mut self.current {
            txn.push(Action { kind, before, after });
        }
    }

    /// Write the current transaction, if it changed anything.
    pub fn end(&mut self) -> Result<(), String> {
        let Some(txn) = self.current.take() else { return Ok(()) };
        if txn.is_empty() {
            return Ok(());
        }
        let mut all = if self.size == 1 { Vec::new() } else { self.load()? };
        all.push(txn);
        if self.size > 0 && all.len() > self.size as usize {
            all.drain(..all.len() - self.size as usize);
        }
        self.save(&all)
    }

    /// Remove and return the most recent transaction.
    pub fn pop(&mut self) -> Result<Option<Transaction>, String> {
        if !self.enabled() {
            return Ok(None);
        }
        let mut all = self.load()?;
        let last = all.pop();
        if last.is_some() {
            if all.is_empty() {
                let _ = fs::remove_file(&self.path);
            } else {
                self.save(&all)?;
            }
        }
        Ok(last)
    }

    fn load(&self) -> Result<Vec<Transaction>, String> {
        let contents = match fs::read_to_string(&self.path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(format!("Could not read {}: {e}", self.path.display())),
        };
        parse(&contents)
    }

    fn save(&self, transactions: &[Transaction]) -> Result<(), String> {
        let mut out = String::new();
        for txn in transactions {
            out.push_str("txn:\n");
            for a in txn {
                let kind = match a.kind {
                    ActionKind::Interval => "interval",
                    ActionKind::Config => "config",
                };
                out.push_str(&format!("  type: {kind}\n  before: {}\n  after: {}\n", a.before, a.after));
            }
        }
        crate::datafile::atomic_write(&self.path, &out).map_err(|e| format!("Could not write {}: {e}", self.path.display()))
    }
}

fn parse(contents: &str) -> Result<Vec<Transaction>, String> {
    let mut all: Vec<Transaction> = Vec::new();
    let (mut kind, mut before) = (None, String::new());
    for line in contents.lines() {
        if line.starts_with("txn:") {
            all.push(Vec::new());
        } else if let Some(t) = line.strip_prefix("  type:") {
            kind = match t.trim() {
                "interval" => Some(ActionKind::Interval),
                "config" => Some(ActionKind::Config),
                other => return Err(format!("Unknown undo action type '{other}'")),
            };
        } else if let Some(b) = line.strip_prefix("  before:") {
            before = b.strip_prefix(' ').unwrap_or(b).to_string();
        } else if let Some(a) = line.strip_prefix("  after:") {
            let (Some(k), Some(txn)) = (kind, all.last_mut()) else {
                return Err(format!("Cannot handle line '{line}'"));
            };
            txn.push(Action { kind: k, before: std::mem::take(&mut before), after: a.strip_prefix(' ').unwrap_or(a).to_string() });
        } else if !line.is_empty() {
            return Err(format!("Cannot handle line '{line}'"));
        }
    }
    Ok(all)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("timew-rs-journal-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.join("undo.data")
    }

    #[test]
    fn records_and_pops() {
        let path = tmp("pop");
        let mut j = Journal::new(&path, -1);
        j.start();
        j.record_interval(String::new(), "{\"id\":1}".into());
        j.end().unwrap();
        j.start();
        j.record_config("a = 1".into(), "a = 2".into());
        j.end().unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text, "txn:\n  type: interval\n  before: \n  after: {\"id\":1}\ntxn:\n  type: config\n  before: a = 1\n  after: a = 2\n");
        let last = j.pop().unwrap().unwrap();
        assert_eq!(last[0].kind, ActionKind::Config);
        let first = j.pop().unwrap().unwrap();
        assert_eq!(first[0].after, "{\"id\":1}");
        assert!(!path.exists());
        assert_eq!(j.pop().unwrap(), None);
    }

    #[test]
    fn limits_size() {
        let path = tmp("size");
        let mut j = Journal::new(&path, 2);
        for i in 0..3 {
            j.start();
            j.record_interval(String::new(), format!("{i}"));
            j.end().unwrap();
        }
        assert_eq!(parse(&fs::read_to_string(&path).unwrap()).unwrap().len(), 2);
        assert_eq!(j.pop().unwrap().unwrap()[0].after, "2");
    }
}
