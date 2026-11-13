# Database format

`timew-rs` and `timew-tui` use the same files as Timewarrior, so they work
on an existing Timewarrior database, and Timewarrior can read everything
`timew-rs` writes. This page describes the files as `timew-rs` reads and
writes them.

## Directory layout

```
$TIMEWARRIORDB/
├── timewarrior.cfg        configuration
├── extensions/            extension reports (executables)
└── data/
    ├── 2025-01.data       intervals that START in January 2025
    ├── 2025-02.data
    ├── …
    ├── tags.data          tag -> number of intervals using it
    └── undo.data          Timewarrior's undo journal (not used by timew-rs)
```

Without `$TIMEWARRIORDB`, the configuration and the data can be in
different places; see [timew-rs.md](timew-rs.md#database-and-configuration).

- One file per month, named `YYYY-MM.data`. Only names of exactly this form
  are read, and a missing `data/` directory counts as an empty database.
- An interval goes into the file of the month it **starts** in (in local
  time), even if it ends in a later month.
- File names sort lexicographically in chronological order. Reading the
  files in reverse, with the lines of each file also in reverse, gives the
  intervals newest first, which is how IDs are assigned (see below).

## Interval lines (`YYYY-MM.data`)

One interval per line:

```
inc <start> [- <end>] [# <tag> <tag> … ["# <annotation>"]]
```

Examples:

```
inc 20250303T074000Z - 20250303T091000Z # budget
inc 20250303T091000Z - 20250303T101000Z # "client-a" meetings
inc 20261112T070000Z - 20261112T083000Z # alpha # "a note"
inc 20261112T090000Z # working
```

| Part | Rule |
| --- | --- |
| Timestamps | Always 16 characters, `YYYYMMDDTHHMMSSZ`, **in UTC**. The fixed length is how the parser recognizes a timestamp. |
| `- <end>` | Missing for a running interval |
| Tags | Separated by spaces, sorted, no duplicates. A tag is quoted (`"…"`) if it contains a space, `"`, or any of `-+/()<^!=~_%`. A `"` inside a tag is written as `\"`. |
| Annotation | An optional second `#`, then a quoted text (`#` alone stands in for the tags if there are none) |
| Line order | Lines are sorted as plain strings when a file is written. Because of the fixed-width timestamp, string order is time order. |

Before a line is added, it is parsed back and compared with the interval,
so nothing is written that would read back differently. Files are written
atomically: to a temporary file first, then renamed over the original. A
file that would end up empty is deleted.

Changes are only written when a command succeeds; a command that fails
leaves all files as they were (except configuration changes, which are
written immediately).

## IDs

IDs such as `@1` are **not stored anywhere**. Each time the data is read,
the intervals are numbered newest first: `@1` is the most recent interval
(the running one, if any), `@2` the one before, and so on. Adding or
deleting an interval shifts the IDs of all older ones, so an ID is only a
position, never a permanent identity.

If exclusions are configured and the running interval spans excluded
time, it counts as several intervals (one per piece) for numbering.

`timew-rs export` writes the ID of each interval into its JSON at the time
of the export.

## `tags.data`

A JSON object that maps each tag to the number of intervals using it.
Entries are sorted by tag:

```json
{
  "budget":{"count":204},
  "email":{"count":332}
}
```

- Every change to an interval updates the counts. A tag whose count drops
  to 0 is removed rather than written as `{"count":0}`. An empty database
  is written as `{\n}`.
- A tag that wasn't in the file before is announced as
  `Note: 'tag' is a new tag.`
- If the file is missing or can't be parsed, `timew-rs` rebuilds it from
  the intervals (and says so: `Tags database does not exist. Recreating
  from interval data...`). `timew-tui` only writes it when it saves an edit.
- `timew-rs get dom.tags…` reads the tags from this file. `timew-rs tags`,
  `dom.tracked.tags…` and the TUI's tag list collect the tags from the
  intervals themselves.

## `undo.data`

Timewarrior keeps an undo journal in `data/undo.data`. `timew-rs` and
`timew-tui` neither write nor read it, and leave an existing file alone.
(So after changes made with `timew-rs`, `timew undo` would revert an older
Timewarrior change instead.)

## `timewarrior.cfg`

```
# comments start with '#'
verbose = on
reports.week.hours = auto
import /usr/share/timewarrior/holidays/holidays.de-DE

define tags:
  meeting:
    color = black on yellow
    description = Meetings
```

- `name = value` settings; everything after `#` is a comment.
- `import <absolute path>` reads another file (up to 10 levels deep).
- `define <type>:` blocks nest settings by indentation:
  `tags.meeting.color = black on yellow` above. Block types are `tags`,
  `reports`, `theme`, `holidays` and `exclusions`. Values in blocks may be
  quoted.
- `timew-rs config <name> <value>` changes the line `name = …` or appends
  a new one; `timew-rs config <name>` removes it.

## Time zones

Everything on disk is in UTC. Everything shown to the user, and every range
boundary (`:day`, `:month`, the summary's day rows, the month a datafile is
for, the TUI's month and year lists), uses the **system's local time zone**
(or `$TZ`) through `jiff`, so daylight saving time is handled correctly.
Weeks start on Monday, and week numbers follow ISO 8601.
