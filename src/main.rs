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

fn main() {
    println!("timew-rs: scaffold only, no commands implemented yet");
}
