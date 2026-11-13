//! timew-core: the Timewarrior data model shared by `timew-cli` (the
//! command line program) and `timew-tui` (the terminal UI).
//!
//! - Files: [`interval`] (one line of a datafile), [`datafile`],
//!   [`database`] (all datafiles and IDs), [`tags`] (`tags.data`).
//! - Configuration: [`rules`] (`timewarrior.cfg`), [`paths`] (where the
//!   files are), [`exclusion`] (exclusions and holidays).
//! - Time: [`range`], [`datetime`] (parsing dates, local calendar),
//!   [`duration`].
//! - Working on the data: [`session`] (changes with feedback),
//!   [`validate`] (overlaps, `:adjust`, `:fill`), [`edit`] (changing one
//!   entry and undoing that), [`summary`] (the rows of a summary).
//! - [`text`]: quoting, JSON strings, display widths and wrapping.

pub mod database;
pub mod datafile;
pub mod datetime;
pub mod duration;
pub mod edit;
pub mod exclusion;
pub mod interval;
pub mod paths;
pub mod range;
pub mod rules;
pub mod session;
pub mod summary;
pub mod tags;
pub mod text;
pub mod validate;

pub use edit::{
    delete_extending_previous, delete_interval, modify_interval, previous_interval, start_interval, stop_interval,
    undo_change,
};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
