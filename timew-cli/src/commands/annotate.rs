//! `timew annotate [@<id> ...] <annotation>`: set (or, with no text,
//! remove) the annotation of intervals, by default the running one.

use crate::session::Session;

pub fn run(s: &mut Session) -> Result<i32, String> {
    let annotation = s.cli.text();
    s.flatten_latest()?;
    for t in super::ids_or_active(s, "annotate")? {
        let mut modified = t.interval.clone();
        modified.annotation = if annotation.is_empty() { None } else { Some(annotation.clone()) };
        s.modify(&t.interval, &modified)?;
        if s.verbose() {
            if annotation.is_empty() {
                s.out.push_str(&format!("Removed annotation from @{}\n", t.id));
            } else {
                s.out.push_str(&format!("Annotated @{} with \"{annotation}\"\n", t.id));
            }
        }
    }
    Ok(0)
}
