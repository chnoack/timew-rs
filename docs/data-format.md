# Database format

`timew-rs` and `timew-tui` read the same files as Timewarrior, so they work
on an existing Timewarrior database. This page describes the files as
`timew-rs` understands them.

## Directory layout

```
$TIMEWARRIORDB/
├── timewarrior.cfg        configuration (ignored by timew-rs)
└── data/
    ├── 2025-01.data       intervals that START in January 2025
    ├── 2025-02.data
    ├── …
    ├── tags.data          tag -> usage count
    └── undo.data          undo journal (not used yet)
```

- One file per month, named `YYYY-MM.data`. Only names of exactly this form
  are read, and a missing `data/` directory counts as an empty database.
- An interval goes into the file of the month it **starts** in, even if it
  ends in a later month.
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
| Annotation | An optional second `#`, then a quoted text |
| Line order | Lines are sorted as plain strings when a file is written. Because of the fixed-width timestamp, string order is time order. |

Files are written atomically: to a temporary file first, then renamed over
the original. A file that would end up empty is deleted instead. These write
paths exist in the library, but no command uses them yet.

## IDs

IDs such as `@1` are **not stored anywhere**. Each time the data is read,
the intervals are numbered newest first: `@1` is the most recent interval
(the running one, if any), `@2` the one before, and so on. Adding or
deleting an interval shifts the IDs of all older ones, so an ID is only a
position, never a permanent identity.

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

- A tag whose count drops to 0 is removed rather than written as
  `{"count":0}`. An empty database is written as `{\n}`.
- A missing, empty or broken file is treated as empty, not as an error.
- `timew-rs tags`, `timew-rs get dom.tags…` and the TUI's tag list
  **don't** use this file. They collect the tags from the intervals
  themselves, so the result is correct even if `tags.data` is out of date.

## Time zones

Everything on disk is in UTC. Everything shown to the user, and every range
boundary (`:day`, `:month`, the summary's day rows, the TUI's month and year
lists), uses the **system's local time zone** through `jiff`, so daylight
saving time is handled correctly. Weeks start on Monday, and week numbers
follow ISO 8601.
