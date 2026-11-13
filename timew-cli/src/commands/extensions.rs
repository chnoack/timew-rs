//! `timew extensions`: list the installed extension reports.

use crate::session::Session;
use crate::table::{terminal_width, Table};

pub fn run(s: &mut Session) -> Result<i32, String> {
    let mut table = Table::new(terminal_width(), s.color());
    table.add_column("Extension", true);
    table.add_column("Status", true);
    for script in &s.extensions.scripts {
        let row = table.add_row();
        table.set(row, 0, script.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string());
        table.set(row, 1, crate::extensions::status(script));
    }
    s.out.push_str(&format!("\nExtensions located in:\n  {}\n\n", s.paths.extensions_dir().display()));
    s.out.push_str(&table.render());
    s.out.push('\n');
    Ok(0)
}
