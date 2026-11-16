//! `timew info`: version, license and project page (not in Timewarrior).

use crate::session::Session;

pub fn run(s: &mut Session) -> Result<i32, String> {
    s.out.push_str(&text());
    Ok(0)
}

fn text() -> String {
    let mut out = format!("\ntimew-cli {}\n{}\n\n", crate::VERSION, env!("CARGO_PKG_DESCRIPTION"));
    let mut row = |label: &str, value: &str| out.push_str(&format!("{label:<10}{value}\n"));
    row("Version", crate::VERSION);
    row("Library", &format!("timew-core {}", timew_core::VERSION));
    row("License", timew_core::LICENSE);
    for (i, line) in timew_core::COPYRIGHT.iter().enumerate() {
        row(if i == 0 { "Copyright" } else { "" }, line.trim_start_matches("Copyright "));
    }
    row("Project", env!("CARGO_PKG_REPOSITORY"));
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_version_license_and_project() {
        let t = text();
        assert!(t.contains(&format!("timew-cli {}", crate::VERSION)), "{t}");
        assert!(t.contains("License   MIT\n"), "{t}");
        assert!(t.contains("Copyright 2026, Christian Noack\n"), "{t}");
        assert!(t.contains("          2015 - 2023, Thomas Lauf, Paul Beckingham, Federico Hernandez (Timewarrior)\n"), "{t}");
        assert!(t.contains("Project   https://github.com/chnoack/timew-rs\n"), "{t}");
    }
}
