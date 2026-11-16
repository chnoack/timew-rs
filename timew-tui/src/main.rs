//! `timew-tui`: a terminal UI for a Timewarrior database.
//!
//! It opens the database from `$TIMEWARRIORDB` (else the standard
//! location), shows a scrollable summary with the same rows as
//! `timew summary`, filtered by day, yesterday, month, year, a date range or
//! a tag, and can start, stop, edit and delete entries. Changes go through
//! the `timew-core` library, so overlaps are refused; `u` undoes the last
//! change.
//!
//! - [`app`]: the state ([`app::App`]) and the actions that change the view
//! - [`input`]: what each key and click does
//! - [`entries`]: changing the data (edit, start, stop, delete, undo)
//! - [`filter`], [`forms`]: the state of the filters and of the dialogs
//! - [`format`](mod@format): dates, times and durations for display
//! - [`ui`]: drawing, one submodule per part of the screen
//!
//! This file finds the database and runs the event loop.

mod app;
mod entries;
mod filter;
mod format;
mod forms;
mod input;
mod ui;
#[cfg(test)]
mod tests;

use std::{env, io};
use std::path::PathBuf;

use ratatui::DefaultTerminal;
use ratatui::crossterm::{event, execute};
use ratatui::crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};

use crate::app::App;
use crate::ui::draw;

/// Resolve the database root like Timewarrior's `paths.cpp`:
///   1. `$TIMEWARRIORDB` if set
///   2. `~/.timewarrior` if it exists (legacy layout)
///   3. `$XDG_DATA_HOME/timewarrior`, defaulting to `~/.local/share/timewarrior`
fn resolve_db_root() -> Result<PathBuf, String> {
    if let Some(db) = env::var_os("TIMEWARRIORDB").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(db));
    }
    let home = env::var_os("HOME").filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(home) = &home {
        let legacy = home.join(".timewarrior");
        if legacy.is_dir() {
            return Ok(legacy);
        }
    }
    let data_home = env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| home.map(|h| h.join(".local").join("share")))
        .ok_or("Cannot locate the database: neither TIMEWARRIORDB nor HOME is set.")?;
    Ok(data_home.join("timewarrior"))
}

fn run(terminal: &mut DefaultTerminal, mut app: App) -> io::Result<()> {
    while !app.quit {
        terminal.draw(|f| draw(f, &mut app))?;
        // While an entry runs, its times count up: redraw every second.
        if app.running.is_some() && !event::poll(std::time::Duration::from_secs(1))? {
            app.tick();
            continue;
        }
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => app.on_key(key),
            Event::Mouse(mouse) => app.on_mouse(mouse),
            _ => {}
        }
    }
    Ok(())
}

fn main() -> std::process::ExitCode {
    // like `timew-cli --version`: just the version, no terminal setup
    if std::env::args().skip(1).eq(["--version"]) {
        println!("{}", env!("CARGO_PKG_VERSION"));
        return std::process::ExitCode::SUCCESS;
    }
    let db_root = match resolve_db_root() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return std::process::ExitCode::FAILURE;
        }
    };
    if !db_root.join("data").is_dir() {
        eprintln!("No Timewarrior database found at {} (missing data/ directory).", db_root.display());
        return std::process::ExitCode::FAILURE;
    }

    let app = App::new(db_root);
    let mut terminal = ratatui::init();
    let _ = execute!(io::stdout(), EnableMouseCapture);
    // lets terminals that support it report Shift+Enter (kitty keyboard protocol)
    let enhanced = ratatui::crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false)
        && execute!(io::stdout(), PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)).is_ok();
    let result = run(&mut terminal, app);
    if enhanced {
        let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
    }
    let _ = execute!(io::stdout(), DisableMouseCapture);
    ratatui::restore();

    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            std::process::ExitCode::FAILURE
        }
    }
}
