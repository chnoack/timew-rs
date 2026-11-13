//! `timew` without a command: show the running interval. Exits with 1 if
//! nothing is being tracked.

use crate::session::Session;

pub fn run(s: &mut Session) -> Result<i32, String> {
    if let Some(active) = s.active()? {
        if s.verbose() {
            let text = super::summarize(&s.rules, &active.interval, s.now);
            s.out.push_str(&text);
        }
        return Ok(0);
    }
    if s.rules.get_bool("temp.shiny", false) {
        s.out.push_str(concat!(
            "\n",
            "Welcome to Timewarrior.\n",
            "\n",
            "There is built-in help:\n",
            "    timew help\n",
            "    timew help <command>\n",
            "    (and more)\n",
            "\n",
            "There is a fully-detailed man page:\n",
            "    man timew\n",
            "\n",
        ));
        return Ok(0);
    }
    if s.verbose() {
        s.out.push_str("There is no active time tracking.\n");
    }
    Ok(1)
}
