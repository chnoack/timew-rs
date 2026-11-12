# timew-rs command reference

`timew-rs` is a drop-in replacement for the `timew` command, limited to the
read-only commands. Its output matches `timew` for the cases covered by the
tests.

```
timew-rs <command> [<range>] [<tags>…] [@<id>…]
```

## Database

`timew-rs` reads `$TIMEWARRIORDB/data/`. The variable **must** be set; there
is no fallback to the standard location yet:

```sh
export TIMEWARRIORDB=~/.local/share/timewarrior
```

(`timew-tui` does fall back to the standard location; see
[timew-tui.md](timew-tui.md).)

## Arguments

The arguments are classified like this:

| Token | Meaning |
| --- | --- |
| `@N` (N ≥ 1) | An interval ID. `@1` is the most recent interval. `@0` is an error. |
| `:name` | A hint, for example a range hint such as `:week` |
| First other word | The command |
| `YYYY-MM-DD[THH:MM:SS[Z]]`, `-`, `to` | A date range (see below) |
| Anything else | A tag |

### Ranges

| Form | Range |
| --- | --- |
| `2025-03-01` | From that date (local midnight) onward, with no end |
| `2025-03-01 - 2025-03-08` | From 03-01 00:00 **up to, but not including,** 03-08 00:00 |
| `2025-03-01 to 2025-03-08` | The same |
| `2025-03-01T08:00:00 - 2025-03-01T12:00:00` | Exact local times |
| `…T08:00:00Z` | A UTC time |
| `20250301T080000Z` | The compact internal format (UTC) |

Dates without a `Z` are local time. Timewarrior's informal date words ("next
monday", "eom", …) are **not** supported.

### Range hints

| Hint | Range (local time) |
| --- | --- |
| `:day`, `:today` | Today |
| `:yesterday` | Yesterday |
| `:monday` … `:sunday` | The most recent day with that name |
| `:week`, `:lastweek` | This or last week (Monday to Sunday) |
| `:fortnight` | Last week plus this week |
| `:month`, `:lastmonth` | This or last calendar month |
| `:quarter`, `:lastquarter` | This or last calendar quarter |
| `:year`, `:lastyear` | This or last calendar year |
| `:all` | Everything |

A hint takes priority over dates if both are given.

### Tags

Every tag you give must be present on an interval for it to match:
`timew-rs export meetings budget` lists only intervals tagged with both.

### IDs

IDs are assigned when the data is read, never stored: `@1` is the most
recent interval, `@2` the one before, and so on. Adding or removing an
interval changes the IDs of all older ones. You cannot combine IDs with tags
or a range (that is an error, as in `timew`).

## Commands

### `summary`

```sh
timew-rs summary                      # today (the default range is :day)
timew-rs summary :week
timew-rs summary 2025-03-03 - 2025-03-08 meetings
timew-rs summary @3 @4
```

Prints the same table as `timew summary`:

```
Wk  Date       Day ID    Tags         Annotation    Start      End    Time   Total
--- ---------- --- ----- ------------ ---------- -------- -------- ------- -------
W10 2025-03-03 Mon @2047 budget                   8:50:00  9:20:00 0:30:00
                   @2046 stakeholders             9:20:00 10:20:00 1:00:00
…
```

- Week numbers are ISO 8601. Week, date and day appear only on the first
  row of each day.
- An interval that runs past midnight gets one row per day.
- A still-running interval shows `-` as its end, and its time counts up to
  now.
- The day's total is on its last row, and the grand total is at the bottom.
- The ID and Annotation columns are always shown. Timewarrior's installer
  turns them on in every new `timewarrior.cfg`, and `timew-rs` has no
  configuration file.
- If nothing matches, it prints
  `No filtered data found in the range <start> - <end>[ tagged with …].`

### `export`

```sh
timew-rs export                       # everything
timew-rs export :lastmonth meetings
timew-rs export @1
```

JSON, one object per line, oldest first:

```json
[
{"id":2045,"start":"20250303T092000Z","end":"20250303T102000Z","tags":["meetings"]},
{"id":2043,"start":"20250303T115500Z","end":"20250303T135500Z","tags":["meetings"]}
]
```

Times are in UTC. `end` is missing for a running interval. `tags` (sorted
alphabetically) and `annotation` appear only if the interval has any.

### `tags`

```sh
timew-rs tags                         # every tag in the database
timew-rs tags :year
timew-rs tags :year meetings          # tags used together with "meetings"
```

```
Tag          Description
------------ -----------
budget       -
email        -
```

The description is always `-`, since there is no configuration file. With
no matches it prints `No data found.`, or nothing if `:quiet` is given.

### `get`

```sh
timew-rs get dom.tracked.count dom.tags.count dom.active
```

Prints the values of one or more DOM references on one line, separated by
spaces. An unknown reference is an error that names it.

| Reference | Value |
| --- | --- |
| `dom.active` | `1` if an interval is running, else `0` |
| `dom.active.start` / `.end` / `.duration` / `.json` | Fields of the running interval |
| `dom.active.tags`, `.tags.count`, `.tags.<N>` | Its tags |
| `dom.tracked.count` | The number of intervals |
| `dom.tracked.ids` | All IDs, for example `@3 @2 @1 ` |
| `dom.tracked.tags`, `.tags.count`, `.tags.<N>` | Tags across all intervals |
| `dom.tracked.<N>.start` / `.end` / `.duration` / `.json` | The N-th most recent interval (`1` = the newest) |
| `dom.tracked.<N>.tags`, `.tags.count`, `.tags.<N>` | Its tags |
| `dom.tags`, `dom.tags.count`, `dom.tags.<N>` | Every tag in the database |
| `dom.tag.<N>`, `dom.tag.count` | An older alias for `dom.tags.*` |

Start and end are local time (`2025-03-03T08:50:00`). Durations use ISO 8601
format (`PT1H30M`, `P2DT3H`, and `PT0S` for zero). Tag lists are sorted
alphabetically, and `<N>` counts from 1.

## Not implemented (yet)

- Commands that change data: `track`, `start`, `stop`, `continue`,
  `modify`, `tag`, `untag`, `annotate`, `delete`, `cancel`, `undo`
  (`timew-rs track` prints `'track' is not a timew-rs command yet.`).
- The configuration file (`timewarrior.cfg`): no holidays, exclusions,
  colors, tag descriptions or `reports.*` settings.
- `dom.rc.*`, Timewarrior's informal date words, and other reports (`day`,
  `week`, `month`, `gaps`, …).
- Shortening the summary table to fit the terminal width.
