//! timew-rs: a Rust implementation of Timewarrior. The library holds the
//! data model, the database, the configuration and all commands; it is
//! shared by the `timew-rs` command line program and `timew-tui`.

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
pub mod journal;
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

    let journal = journal::Journal::new(paths.data_dir().join("undo.data"), rules.get_int("journal.size", -1)?);
    let (db, notice, warning) = database::Database::open_and_repair(paths.data_dir())?;
    let mut stderr = String::new();
    if let Some(w) = warning {
        stderr.push_str(&format!("{w}\n"));
    }
    stdout.push_str(&notice);

    let extensions = extensions::Extensions::load(&paths.extensions_dir())?;
    let cli = Cli::parse(args, &extensions.names())?;

    let mut session = Session { cli, rules, db, journal, extensions, paths, out: std::mem::take(stdout), now };
    let result = commands::dispatch(&mut session);
    *stdout = std::mem::take(&mut session.out);
    let status = result?;
    session.db.commit()?;
    Ok((status, stderr))
}
