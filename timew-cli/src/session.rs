//! A run of the command line program: the shared [`timew_core`] session
//! (configuration, database, output) plus the parsed command line and the
//! installed extensions. It dereferences to the core session, so commands
//! use both through one value.

use std::io::{self, BufRead, Write};
use std::ops::{Deref, DerefMut};

use crate::cli::Cli;
use crate::extensions::Extensions;

pub use timew_core::session::flatten;

pub struct Session {
    pub core: timew_core::session::Session,
    pub cli: Cli,
    pub extensions: Extensions,
}

impl Deref for Session {
    type Target = timew_core::session::Session;
    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl DerefMut for Session {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

/// Print `question` (after anything already in `out`) and read yes or no.
/// End of input means no.
pub fn confirm(out: &mut String, question: &str) -> bool {
    print!("{out}");
    out.clear();
    let stdin = io::stdin();
    loop {
        print!("{question} (yes/no) ");
        let _ = io::stdout().flush();
        let mut answer = String::new();
        match stdin.lock().read_line(&mut answer) {
            Ok(0) | Err(_) => return false,
            Ok(_) => {}
        }
        let answer = answer.trim().to_ascii_lowercase();
        match crate::text::auto_complete(&answer, ["yes", "no"]).as_slice() {
            ["yes"] => return true,
            ["no"] => return false,
            _ => {}
        }
    }
}
