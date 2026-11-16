//! `timew-tui --version` prints the version without touching the terminal
//! or needing a database.

use std::process::Command;

#[test]
fn version_flag_prints_the_version() {
    let out = Command::new(env!("CARGO_BIN_EXE_timew-tui"))
        .arg("--version")
        .env("TIMEWARRIORDB", "/nonexistent")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), format!("{}\n", env!("CARGO_PKG_VERSION")));
    assert!(out.stderr.is_empty());
}
