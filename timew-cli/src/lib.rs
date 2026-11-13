//! timew-cli: a Rust implementation of Timewarrior. The library holds the
//! data model, the database, the configuration and all commands; it is
//! shared by the `timew-cli` command line program and `timew-tui`.

pub mod cli;
pub mod color;
pub mod commands;
pub mod composite;
pub mod database;
pub mod datafile;
pub mod datetime;
pub mod duration;
pub mod exclusion;
pub mod extensions;
pub mod interval;
pub mod paths;
pub mod range;
pub mod rules;
pub mod session;
pub mod table;
pub mod tags;
pub mod text;
pub mod validate;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

use std::io::IsTerminal;

use cli::{Cli, Role};
use session::Session;

/// Settings that hints stand for.
fn apply_hints(rules: &mut rules::Rules, cli: &Cli) {
    for arg in cli.args.iter().filter(|a| a.role == Role::Hint) {
        match arg.canonical.as_str() {
            ":debug" => rules.set("debug", "on"),
            ":quiet" => rules.set("verbose", "off"),
            ":color" => rules.set("color", "on"),
            ":nocolor" => rules.set("color", "off"),
            ":yes" => rules.set("confirmation", "off"),
            _ => {}
        }
    }
}

/// What a run produced: standard output, error output and exit status.
pub struct Outcome {
    pub stdout: String,
    pub stderr: String,
    pub status: i32,
}

/// Run the program with `args` (without the program name).
pub fn run(args: &[String]) -> Outcome {
    let mut stdout = String::new();
    match run_inner(args, &mut stdout) {
        Ok((status, stderr)) => Outcome { stdout, stderr, status },
        Err(e) => Outcome { stdout, stderr: format!("{e}\n"), status: 255 },
    }
}

fn run_inner(args: &[String], stdout: &mut String) -> Result<(i32, String), String> {
    let now = datetime::now();
    let first_pass = Cli::parse(args, &[])?;

    let mut rules = rules::Rules::default();
    apply_hints(&mut rules, &first_pass);
    let paths = paths::Paths::from_env();
    let ask = rules.get_bool("confirmation", true);
    let shiny = paths.initialize(&mut |q| !ask || session::confirm(stdout, q))?;

    let mut rules = rules::Rules::load(&paths.config_file())?;
    if !rules.has("color") {
        rules.set("color", if std::io::stdout().is_terminal() { "on" } else { "off" });
    }
    apply_hints(&mut rules, &first_pass);
    rules.set("temp.db", &paths.db_dir.display().to_string());
    rules.set("temp.extensions", &paths.extensions_dir().display().to_string());
    rules.set("temp.config", &paths.config_file().display().to_string());
    if shiny {
        rules.set("temp.shiny", "1");
    }
    for (name, value) in first_pass.overrides() {
        rules.set(&name, &value);
    }

    let (db, notice, warning) = database::Database::open_and_repair(paths.data_dir())?;
    let mut stderr = String::new();
    if let Some(w) = warning {
        stderr.push_str(&format!("{w}\n"));
    }
    stdout.push_str(&notice);

    let extensions = extensions::Extensions::load(&paths.extensions_dir())?;
    let cli = Cli::parse(args, &extensions.names())?;

    let mut session =
        Session { cli, rules, db, changes: Vec::new(), extensions, paths, out: std::mem::take(stdout), now };
    let result = commands::dispatch(&mut session);
    *stdout = std::mem::take(&mut session.out);
    let status = result?;
    session.db.commit()?;
    Ok((status, stderr))
}

/// Replace the stored interval `from` with `to`, the way `timew modify`
/// does: an overlap with another interval is refused, or with `adjust`
/// resolved like `:adjust` (the other intervals are shortened, split or
/// deleted). The tag counts are kept up to date.
///
/// Returns how many other intervals were adjusted, and every change made
/// (including those to adjusted intervals), which [`undo_change`] can
/// revert. Used by `timew-tui` to edit entries.
pub fn modify_interval(
    paths: &paths::Paths,
    from: &interval::Interval,
    to: &interval::Interval,
    adjust: bool,
) -> Result<(usize, Vec<session::Change>), String> {
    if from == to {
        return Ok((0, Vec::new()));
    }
    let now = datetime::now();
    if to.range.is_ended() && to.range.start >= to.range.end {
        return Err("The end must be after the start.".to_string());
    }
    if to.range.is_open() && to.range.start > now {
        return Err("A running entry cannot start in the future.".to_string());
    }
    let mut s = open_session(paths, now)?;
    if !s.db.intervals()?.contains(from) {
        return Err("The entry was changed by another program. Press r to reload.".to_string());
    }
    s.delete(from)?;
    let overlapping = s.tracked_in(&to.range, &[])?.len();
    validate::auto_adjust(&mut s, adjust, to)?;
    s.add(to)?;
    s.db.commit()?;
    Ok((overlapping, s.changes))
}

/// Revert the changes made by [`modify_interval`]. Only if the intervals
/// they left behind are still stored unchanged, and the restored intervals
/// don't overlap anything added since; otherwise nothing is changed.
pub fn undo_change(paths: &paths::Paths, changes: &[session::Change]) -> Result<(), String> {
    let mut s = open_session(paths, datetime::now())?;
    for change in changes.iter().rev() {
        s.db.modify_interval(&change.after, &change.before)
            .map_err(|_| "The entry was changed by another program since.".to_string())?;
    }
    let stored = s.db.intervals()?;
    for restored in changes.iter().map(|c| &c.before).filter(|b| !b.is_empty() && stored.contains(b)) {
        if stored.iter().any(|other| other != restored && other.range.intersects(&restored.range)) {
            return Err("It would overlap an entry added since.".to_string());
        }
    }
    s.db.commit()
}

fn open_session(paths: &paths::Paths, now: range::Epoch) -> Result<Session, String> {
    let config = paths.config_file();
    let mut rules = if config.exists() { rules::Rules::load(&config)? } else { rules::Rules::default() };
    rules.set("verbose", "off");
    let (db, _, _) = database::Database::open_and_repair(paths.data_dir())?;
    Ok(Session {
        cli: Cli::default(),
        rules,
        db,
        changes: Vec::new(),
        extensions: extensions::Extensions::default(),
        paths: paths.clone(),
        out: String::new(),
        now,
    })
}
