//! `timew tag|untag|retag [@<id> ...] <tag> ...`: add tags to, remove tags
//! from, or replace the tags of intervals, by default the running one.

use crate::session::Session;
use crate::text::join_quoted;

#[derive(Clone, Copy)]
pub enum Change {
    Tag,
    Untag,
    Retag,
}

pub fn run(s: &mut Session, change: Change) -> Result<i32, String> {
    let name = match change {
        Change::Tag => "tag",
        Change::Untag => "untag",
        Change::Retag => "retag",
    };
    let tags = s.cli.tags();
    if tags.is_empty() {
        return Err(format!("At least one tag must be specified. See 'timew help {name}'."));
    }
    let tag_list = join_quoted(" ", &tags);
    s.flatten_latest()?;
    for t in super::ids_or_active(s, name)? {
        let mut modified = t.interval.clone();
        match change {
            Change::Tag => tags.iter().for_each(|tag| modified.add_tag(tag.clone())),
            Change::Untag => tags.iter().for_each(|tag| modified.remove_tag(tag)),
            Change::Retag => {
                modified.tags.clear();
                tags.iter().for_each(|tag| modified.add_tag(tag.clone()));
            }
        }
        s.modify(&t.interval, &modified)?;
        if s.verbose() {
            let line = match change {
                Change::Tag => format!("Added {tag_list} to @{}\n", t.id),
                Change::Untag => format!("Removed {tag_list} from @{}\n", t.id),
                Change::Retag => format!("Retagged @{} as {tag_list}\n", t.id),
            };
            s.out.push_str(&line);
        }
    }
    Ok(0)
}
