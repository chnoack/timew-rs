//! `timew-cli`: a Rust implementation of the Timewarrior command line
//! program. The data model is in [`timew_core`]; this crate adds the
//! command line parsing, the commands and their text output.

mod app;
mod cli;
mod color;
mod commands;
mod composite;
mod extensions;
mod session;
mod table;

// The shared modules, so the commands can name them as `crate::…`.
use timew_core::{database, datetime, duration, exclusion, interval, paths, range, rules, summary, text, validate};

use std::io::Write;
use std::process::ExitCode;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() == 1 && args[0] == "--version" {
        println!("{VERSION}");
        return ExitCode::SUCCESS;
    }
    let outcome = app::run(&args);
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(outcome.stdout.as_bytes());
    let _ = out.flush();
    eprint!("{}", outcome.stderr);
    ExitCode::from((outcome.status & 0xFF) as u8)
}
