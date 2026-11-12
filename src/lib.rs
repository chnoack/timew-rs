//! timew-rs library: the Timewarrior data model, database access and
//! command implementations, shared by the `timew-rs` CLI and `timew-tui`.

#![allow(dead_code)] // scaffold phase: many items are not wired up to commands yet

pub mod cli;
pub mod commands;
pub mod database;
pub mod datafile;
pub mod datetime;
pub mod interval;
pub mod journal;
pub mod range;
pub mod tags;
pub mod validate;
