//! `timew help [<command> | <topic>]`, also `timew <command> --help`.

use crate::session::Session;

const USAGE: &[&str] = &[
    "timew [--version]",
    "timew annotate @<id> [@<id> ...] <annotation>",
    "timew cancel",
    "timew config [<name> [<value> | '']]",
    "timew continue [@<id>] [<date>|<interval>]",
    "timew day [<interval>] [<tag> ...]",
    "timew delete @<id> [@<id> ...]",
    "timew diagnostics",
    "timew export [<interval>] [<tag> ...]",
    "timew extensions",
    "timew fill [@<id>]",
    "timew gaps [<interval>] [<tag> ...]",
    "timew get <DOM> [<DOM> ...]",
    "timew help [<command> | dates | dom | durations | hints | ranges]",
    "timew import [<file> ...]",
    "timew join @<id> @<id>",
    "timew lengthen @<id> [@<id> ...] <duration>",
    "timew modify (start|end) @<id> <date>",
    "timew modify range @<id> <interval>",
    "timew month [<interval>] [<tag> ...]",
    "timew move @<id> <date>",
    "timew [report] <report> [<interval>] [<tag> ...]",
    "timew resize @<id> [@<id> ...] <duration>",
    "timew retag @<id> [@<id> ...] <tag> [<tag> ...]",
    "timew shorten @<id> [@<id> ...] <duration>",
    "timew show",
    "timew split @<id> [@<id> ...]",
    "timew start [<date>] [<tag> ...]",
    "timew stop [<tag> ...]",
    "timew summary [<interval>] [<tag> ...]",
    "timew tag @<id> [@<id> ...] <tag> [<tag> ...]",
    "timew tags [<interval>] [<tag> ...]",
    "timew track <interval> [<tag> ...]",
    "timew untag @<id> [@<id> ...] <tag> [<tag> ...]",
    "timew week [<interval>] [<tag> ...]",
];

const TOPICS: &[(&str, &str)] = &[
    (
        "dates",
        "Dates are ISO 8601 (2025-03-01, 2025-03-01T08:00, 20250301T080000Z,\n\
         2025-W10, 08:00), informal times (8am, 8:30pm), or names: now, today,\n\
         yesterday, tomorrow, monday .. sunday, january .. december, 1st .. 31st,\n\
         sod/eod, sow/eow, som/eom, soq/eoq, soy/eoy (with p and n variants for\n\
         the previous and next period, such as sopw or eonm), soww/eoww, later,\n\
         easter, goodfriday, ascension, pentecost, midsommar.\n",
    ),
    (
        "dom",
        "DOM references for 'timew get':\n\
         dom.active, dom.active.start, dom.active.duration, dom.active.json,\n\
         dom.active.tags, dom.active.tags.count, dom.active.tags.<N>,\n\
         dom.tracked.count, dom.tracked.ids, dom.tracked.tags[.count|.<N>],\n\
         dom.tracked.<N>.start, .end, .duration, .json, .tags[.count|.<N>],\n\
         dom.tags, dom.tags.count, dom.tags.<N>, dom.rc.<name>\n",
    ),
    (
        "durations",
        "Durations are a number and a unit (90s, 30min, 1.5h, 2 days, 1w),\n\
         ISO 8601 (PT1H30M, P1D), a unit alone (hour, day, week), or a number\n\
         of seconds above 60.\n",
    ),
    (
        "hints",
        "Hints: :adjust :fill :quiet :yes :debug :color :nocolor :blank\n\
         :ids :no-ids :tags :no-tags :annotations :no-annotations\n\
         :holidays :no-holidays, and ranges :all :day :today :yesterday :week\n\
         :lastweek :fortnight :month :lastmonth :quarter :lastquarter :year\n\
         :lastyear :monday .. :sunday\n",
    ),
    (
        "ranges",
        "Ranges:\n\
         [from] <date>\n\
         [from] <date> to/- <date>\n\
         [from] <date> for <duration>\n\
         <duration> before/after <date>\n\
         <duration> ago\n\
         [for] <duration>\n\
         or a range hint such as :week.\n",
    ),
];

pub fn run(s: &mut Session) -> Result<i32, String> {
    let words = s.cli.words();
    if let Some(topic) = words.first() {
        if let Some((_, text)) = TOPICS.iter().find(|(t, _)| t == topic) {
            s.out.push_str(text);
            return Ok(0);
        }
        // The man page, if installed (as with Timewarrior's man pages).
        print!("{}", s.out);
        s.out.clear();
        let man = std::process::Command::new("man")
            .arg(format!("timew-{topic}"))
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::piped())
            .output();
        let man_error = match man {
            Ok(o) if o.status.success() => return Ok(0),
            Ok(o) => Some((o.status.code().unwrap_or(1), String::from_utf8_lossy(&o.stderr).into_owned())),
            Err(_) => None,
        };
        let lines: Vec<&&str> = USAGE.iter().filter(|u| u.split(' ').any(|w| w == topic.as_str())).collect();
        if lines.is_empty() {
            if let Some((code, text)) = man_error {
                eprint!("{text}");
                return Ok(code);
            }
            return Err(format!("No help available for '{topic}'."));
        }
        s.out.push_str("\nUsage: ");
        s.out.push_str(&lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n       "));
        s.out.push_str(&format!("\n\nSee 'man timew-{topic}' for details.\n\n"));
        return Ok(0);
    }

    s.out.push_str("\nUsage: ");
    s.out.push_str(&USAGE.join("\n       "));
    s.out.push_str("\n\n");
    let extensions = s.extensions.names();
    if !extensions.is_empty() {
        s.out.push_str("Extensions (extensions do not provide help):\n");
        for e in extensions {
            s.out.push_str(&format!("       {e}\n"));
        }
        s.out.push('\n');
    }
    s.out.push_str("Additional help:\n       timew help <command>\n");
    for (topic, _) in TOPICS {
        s.out.push_str(&format!("       timew help {topic}\n"));
    }
    s.out.push_str(concat!(
        "\n",
        "Interval:\n",
        "       [from] <date>\n",
        "       [from] <date> to/- <date>\n",
        "       [from] <date> for <duration>\n",
        "       <duration> before/after <date>\n",
        "       <duration> ago\n",
        "       [for] <duration>\n",
        "\n",
        "Tag:\n",
        "       Word\n",
        "       'Single Quoted Words'\n",
        "       \"Double Quoted Words\"\n",
        "       Escaped\\ Spaces\n",
        "\n",
        "Configuration overrides:\n",
        "       rc.<name>=<value>\n",
        "\n",
    ));
    Ok(0)
}
