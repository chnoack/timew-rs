#![allow(dead_code)] // scaffold phase: many items are not wired up to commands yet

mod cli;
mod commands;
mod database;
mod datafile;
mod datetime;
mod interval;
mod journal;
mod range;
mod tags;
mod validate;

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

/// Resolve the TIMEWARRIORDB root the same way `paths.cpp` does for the
/// one case we need during development/differential-testing: an explicit
/// `TIMEWARRIORDB` environment variable. (Real timew also falls back to
/// `$XDG_DATA_HOME/timewarrior` / `~/.local/share/timewarrior` -- out of
/// scope for now since all testing goes through the sandboxed DB.)
fn resolve_db_root() -> Result<PathBuf, String> {
    env::var("TIMEWARRIORDB")
        .map(PathBuf::from)
        .map_err(|_| "TIMEWARRIORDB must be set (no default-location fallback implemented yet).".to_string())
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    let parsed = cli::parse(&args)?;

    let Some(command) = parsed.command.clone() else {
        return Err("No command given.".to_string());
    };

    let db_root = resolve_db_root()?;
    let mut database = database::Database::open(db_root.join("data")).map_err(|e| e.to_string())?;

    let output = match command.as_str() {
        "export" => commands::export::run(&mut database, parsed)?,
        other => return Err(format!("'{other}' is not a timew-rs command yet.")),
    };

    print!("{output}");
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
