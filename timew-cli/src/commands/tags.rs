//! `timew tags [<range>] [<tag> ...]`: the tags used in the range (by
//! default all), with their descriptions (`tags.<tag>.description`).

use crate::color::Color;
use crate::range::Range;
use crate::session::Session;
use crate::table::{terminal_width, Table};

pub fn run(s: &mut Session) -> Result<i32, String> {
    let range = s.cli.range(Range::empty(), s.now)?;
    let filter = s.cli.tags();
    let mut tags: Vec<String> = s.tracked_in(&range, &filter)?.into_iter().flat_map(|t| t.interval.tags).collect();
    tags.sort();
    tags.dedup();
    if tags.is_empty() {
        if s.verbose() {
            s.out.push_str("No data found.\n");
        }
        return Ok(0);
    }
    let mut table = Table::new(terminal_width(), s.color());
    table.add_column("Tag", true);
    table.add_column("Description", true);
    for tag in &tags {
        let row = table.add_row();
        table.set_colored(row, 0, tag.clone(), super::tag_color(&s.rules, tag));
        let description = s.rules.get_or(&format!("tags.{tag}.description"), "-");
        table.set_colored(row, 1, description, Color::none());
    }
    s.out.push('\n');
    s.out.push_str(&table.render());
    s.out.push('\n');
    Ok(0)
}
