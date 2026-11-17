# timew-cli command reference

`timew-cli` is a replacement for the `timew` command. It implements all of
Timewarrior's commands except `undo`, reads and writes the same files
(except the undo journal), and its output matches `timew` (Timewarrior
1.10). It passes Timewarrior's own test suite, apart from the `undo`
tests. The places where it deliberately behaves differently are listed in
[Differences from Timewarrior](#differences-from-timewarrior).

```
timew-cli [<command>] [<range>] [<tag> ...] [@<id> ...] [:<hint> ...] [rc.<name>=<value> ...]
```

Without a command it shows the running interval. It exits with 1 if
nothing is being tracked.

## Database and configuration

| What | Where |
| --- | --- |
| Configuration, extensions | `$TIMEWARRIORDB`, else `~/.timewarrior` if it exists, else `$XDG_CONFIG_HOME/timewarrior` (`~/.config/timewarrior`) |
| Data | `$TIMEWARRIORDB`, else `~/.timewarrior` if it exists, else `$XDG_DATA_HOME/timewarrior` (`~/.local/share/timewarrior`) |

On the first run the directories are created (after asking, unless
`:yes` is given or `confirmation = off`), plus `data/`, `extensions/` and
a `timewarrior.cfg` containing:

```
reports.summary.ids = yes
reports.summary.annotations = yes
```

The file formats are described in [data-format.md](data-format.md).
`rc.<name>=<value>` (or `rc.<name>:<value>`) overrides a setting for one
run.

## Arguments

Each argument is one of:

| Argument | Meaning |
| --- | --- |
| The first command word | The command. An unambiguous prefix works too (`sum` is `summary`), and so does the name of an extension report. |
| `@N` (N ≥ 1) | An interval ID. `@1` is the most recent interval. `@0` is an error. |
| `:name` | A hint (an unambiguous prefix works too, e.g. `:adj`) |
| `rc.name=value` | A configuration override |
| A date or a duration | Part of the range |
| `from`, `since`, `to`, `until`, `-`, `for`, `before`, `after`, `ago` | Range keywords |
| `dom.…` | A DOM reference (`get`) |
| Anything else | A tag |

### Dates

| Form | Examples |
| --- | --- |
| ISO 8601 | `2025-03-01`, `2025-03-01T08:00`, `2025-03-01T08:00:00`, `2025-03-01T07:00:00Z`, `2025-03-01T08:00+01:00`, `20250301T070000Z`, `2025-03`, `2025-W10`, `2025-W10-3`, `2025-060` |
| Times (today) | `08:00`, `08:00:00`, `0800`, `08:00Z` |
| Informal times (today) | `8am`, `8:30pm`, `12p`, `9:15` |
| Epoch seconds | `1740812400` |
| Days | `now`, `today`, `yesterday`, `tomorrow` (`yest` and other 3+ letter abbreviations work), `monday` … `sunday`, `mon` …, `1st` … `31st` |
| Periods | `sod`, `eod`, `sow`, `eow`, `som`, `eom`, `soq`, `eoq`, `soy`, `eoy` (start and end of day, week, month, quarter, year), with `p`/`n` for the previous/next one (`sopd`, `eonw`, …), work weeks `soww`, `eoww` (also `sopww`, `eonww`, …), month names `january` … `december` |
| Holidays | `easter`, `eastermonday`, `goodfriday`, `ascension`, `pentecost`, `midsommar`, `midsommarafton`, `juhannus`; `later`, `someday` |

Dates without `Z` or an offset are local time. Times and day names refer
to today or the most recent such day, never the future. Weeks start on
Monday and are numbered per ISO 8601. The `eo…` names are the **end** of
the period, i.e. midnight after it.

### Durations

`90s`, `30min`, `1.5h`, `2 hours`, `3d`, `1w`, `PT1H30M`, `P1D`, `P2W`, a
unit on its own (`hour`, `day`, `week`), or a number of seconds greater
than 60. Note that `m` means months, so write `min` for minutes.

### Ranges

| Form | Range |
| --- | --- |
| `<date>` | A date alone is its whole day, week or month (`2025-03-01`, `2025-W10`, `2025-03`, `yesterday`, `monday`, `march`). A date with a time, or an instant like `now`, starts an open range. |
| `from <date>` | From then on |
| `[from] <date> to\|- <date>` | Between the two (the end is excluded) |
| `[from] <date> for <duration>` | From the date for that long |
| `<duration> before\|after <date>` | The duration before or after the date |
| `<duration> ago` | From that long ago on |
| `[for] <duration>` | The last `<duration>` up to now |
| `:hint` | A range hint |

### Hints

| Hint | Meaning |
| --- | --- |
| `:day`, `:today`, `:yesterday` | Today, yesterday |
| `:week`, `:lastweek`, `:fortnight` | This week, last week, both |
| `:month`, `:lastmonth`, `:quarter`, `:lastquarter`, `:year`, `:lastyear` | Calendar periods |
| `:monday` … `:sunday` | The most recent such day |
| `:all` | All time |
| `:adjust` | Resolve overlaps by shortening, splitting or removing existing intervals |
| `:fill` | Extend to the neighboring intervals |
| `:quiet` | No feedback (`verbose = off`) |
| `:yes` | Don't ask (`confirmation = off`) |
| `:color`, `:nocolor` | Force colors on/off (by default on for a terminal) |
| `:ids`, `:no-ids`, `:tags`, `:no-tags`, `:annotations`, `:no-annotations`, `:holidays`, `:no-holidays` | Summary columns |
| `:blank` | Charts and gaps without the tracked intervals |
| `:debug` | Accepted, but prints nothing extra |

### IDs

IDs are assigned when the data is read, never stored: `@1` is the most
recent interval, `@2` the one before, and so on. Adding or removing an
interval changes the IDs of all older ones.

When exclusions are configured and the running interval spans excluded
time, it is shown as several pieces, the newest being `@1`. Commands that
change intervals first store these pieces as separate intervals.

### Overlaps

Intervals may not overlap. A command that would create an overlap fails
with "You cannot overlap intervals…", unless `:adjust` is given: then the
new or changed interval wins, and the intervals it overlaps are shortened,
split or deleted. Starting an interval while another one runs is not an
overlap: the running one is stopped. Starting one with the same tags as
the running one does nothing.

### Exclusions and holidays

```
exclusions.monday = <8:00 12:00-12:45 >17:30   # before 8, lunch, after 17:30
exclusions.days.2025_12_24 = off               # an extra day off
exclusions.days.2025_12_27 = on                # a working day despite a holiday
holidays.de-DE.2025_12_25 = 1. Weihnachtstag
```

`track` leaves excluded time that lies inside the range out of the
interval (it is split around it). Charts show excluded time in gray, and
`gaps` doesn't report it.

## Commands that record time

| Command | Does |
| --- | --- |
| `start [<date>] [<tag> ...]` | Start tracking (now or at a past `<date>`); stops the running interval. `:fill` starts at the end of the previous interval. |
| `stop [<date>] [<tag> ...]` | Stop tracking. With tags, only those stop and tracking continues with the others. |
| `track <range> [<tag> ...]` | Record a closed interval; without an end, like `start`. `:fill` extends it to its neighbors. |
| `continue [@<id> \| <tag> ...] [<date> \| <range>]` | Track again with the tags and annotation of the last interval, of `@<id>`, or of the last one with all the tags. |
| `cancel` | Discard the running interval. |

## Commands that change intervals

Without IDs, `annotate`, `tag`, `untag` and `retag` work on the running
interval.

| Command | Does |
| --- | --- |
| `annotate [@<id> ...] <text>` | Set the annotation (all the words after the IDs); with no text, remove it. |
| `tag [@<id> ...] <tag> ...` | Add tags. |
| `untag [@<id> ...] <tag> ...` | Remove tags. |
| `retag [@<id> ...] <tag> ...` | Replace all tags. |
| `delete @<id> ...` | Delete intervals. |
| `modify start @<id> <date>` | Change the start (`:fill`: to the end of the previous interval). |
| `modify end @<id> <date>` | Change the end (`:fill`: to the start of the next interval). |
| `modify range @<id> <range>` | Change both. |
| `move @<id> <date>` | Move to start at `<date>`, keeping the length. |
| `lengthen @<id> ... <duration>` | Move the end later. |
| `shorten @<id> ... <duration>` | Move the end earlier. |
| `resize @<id> ... <duration>` | Set the length. |
| `split @<id> ...` | Split into two halves. |
| `join @<id> @<id>` | Merge two intervals, with the tags of both. |
| `fill [@<id>]` | Extend to the neighboring intervals. |
| `import [<file> ...]` | Add intervals from JSON (the `export` format), from files or stdin. |

## Reports

| Command | Default range | Shows |
| --- | --- | --- |
| `summary` | `:day` | A table per interval and day with daily and total times |
| `day`, `week`, `month` | `:day`, `:week`, `:month` | A chart, one line per day and a cell per hour |
| `gaps` | `:day` | Untracked time, leaving out excluded time |
| `tags` | all | The tags used, with `tags.<tag>.description` |
| `export` | all | JSON, one interval per line; with `:csv` a CSV table (see below) |
| `get <dom> ...` | all | Values, space-separated (see below) |
| `[report] <name>` | all | An extension report |

The default ranges can be changed with `reports.<name>.range` (and
`reports.range` for all). Reports take a range and tags, or IDs (not
both).

### `export`

The intervals as JSON, the format `import` reads (Timewarrior's format).
With the hint `:csv`, a CSV table instead, the same that `timew-tui`
writes with `x`:

```
$ timew-cli export :csv 2026-11-16 meetings
Wk,Date,Day,Tags,Start,End,Time,Total,Annotation
W47,2026-11-16,Mon,"meetings, team",8:05,10:05,2:00,,
W47,2026-11-16,Mon,meetings,14:15,15:15,1:00,3.00,"weekly, with notes"
```

- One row per interval and day (an interval past midnight gets two), with
  a heading line.
- Wk, Date and Day on every row, times as `H:MM`, Total (on the last row
  of a day) in decimal hours, without uncounted (`_tag`) intervals.
- Fields are separated by a comma, or by `export.csv.separator`: one
  character (not `"`), or `tab`. Fields with the separator, a quote or a
  line break are in double quotes.

```
$ timew-cli config export.csv.separator ';'
$ timew-cli export :csv :month rc.export.csv.separator=tab   # just this once
```

### `summary`

```
Wk  Date       Day ID    Tags                             Annotation    Start      End    Time   Total
--- ---------- --- ----- -------------------------------- ---------- -------- -------- ------- -------
W10 2025-03-03 Mon @2047 budget                                       8:50:00  9:20:00 0:30:00
                   @2046 stakeholders                                 9:20:00 10:20:00 1:00:00
                   @2045 meetings                                    10:20:00 11:20:00 1:00:00
…
```

Columns: `reports.summary.weeks`, `.weekdays`, `.tags` (on by default),
`.ids`, `.annotations` (off by default, on in a new configuration file),
and the hints above. An interval that runs past midnight gets one row per
day; a running one counts up to now. Holidays in the range are listed
below. The table fits itself to the terminal width, wrapping long cells.

An interval with a tag that starts with exactly one underscore (`_break`,
not `__x`) is listed with its time but left out of the daily totals and
the grand total, also when it has other tags. The same goes for the totals
and the "Tracked" line of `day`, `week` and `month` (the interval is still
drawn). This is not in Timewarrior.

### `day`, `week`, `month`

Settings `reports.<day|week|month>.<name>`:

| Name | Values (default for day / week / month) |
| --- | --- |
| `hours` | `all` (0-23) or `auto` (only the hours with data) |
| `cell` | Minutes per character (15) |
| `lines` | Lines per day (2 / 1 / 1) |
| `spacing` | Blank columns between hours (1) |
| `axis` | `internal` puts the hours into the chart (day), otherwise a header line |
| `month`, `week`, `weekday`, `day` | Labels at the start of each line |
| `totals`, `summary`, `holidays` | Daily totals, the summary below, the holiday list |

`:ids` adds the IDs to the labels; `:blank` leaves out the intervals.

### `get`

| Reference | Value |
| --- | --- |
| `dom.active` | `1` if an interval is running, else `0` |
| `dom.active.start`, `.duration`, `.json`, `.tags`, `.tags.count`, `.tags.<N>` | The running interval |
| `dom.tracked.count`, `dom.tracked.ids` | The intervals in the range (and with the tags) given |
| `dom.tracked.tags`, `.tags.count`, `.tags.<N>` | Their tags |
| `dom.tracked.<N>.start`, `.end`, `.duration`, `.json`, `.tags…` | The N-th most recent of them |
| `dom.tags`, `dom.tags.count`, `dom.tags.<N>` | All tags in `tags.data` |
| `dom.rc.<name>` | A configuration value (empty if unset) |

Times are local (`2025-03-03T08:50:00`), durations ISO 8601 (`PT1H30M`).
`.tag.` instead of `.tags.` still works but prints a deprecation warning.

### Extension reports

An executable in the `extensions` directory is a report named after its
file (an unambiguous prefix works). It gets the configuration as
`name: value` lines (including `temp.report.start`, `temp.report.end`,
`temp.report.tags`, `temp.version`), an empty line, and the intervals as
JSON on stdin, and prints the report. `timew-cli extensions` lists them.

## Configuration commands

| Command | Does |
| --- | --- |
| `config` / `show` | Show all settings, grouped like rule blocks. |
| `config <name> <value>` | Set a value (asks first unless `:yes`). Exits with 1 if nothing changed. |
| `config <name> ''` | Set an empty value. |
| `config <name>` | Remove the setting from the file. |
| `extensions` | List extension reports. |
| `diagnostics` | Version, files and settings, for bug reports. |
| `help [<command> \| dates \| dom \| durations \| hints \| ranges]` | Usage; `timew-cli <command> --help` is the same as `help <command>`. Shows Timewarrior's man page if it is installed. |
| `info` | Version, license, copyright and project page. |
| `--version` | The version. |

## Differences from Timewarrior

These are deliberate; most fix something the original gets wrong.

- `annotate` uses all words after the IDs as the annotation (Timewarrior
  uses only the last word).
- A date alone as a range: `2025-03` is the whole month and `2025-W10` the
  whole week (Timewarrior: only the first day), and days are calendar
  days, so a day with a daylight saving change has 23 or 25 hours
  (Timewarrior: always 24 hours, spilling into the next day).
- `eod`, `eow`, `eom`, … are midnight after the period (Timewarrior:
  23:59:59 in some places, midnight in others).
- Report names are only recognized as the command word. Timewarrior also
  takes a later word that is a prefix of a report name, such as `d` for
  `day`, as the report and drops it from the tags.
- Without colors (output not to a terminal), `gaps` and the chart totals
  contain no color codes.
- `exclusions.days.<date> = on|off` works as documented (Timewarrior
  treats both as days off).
- There is no `undo` command, and no undo journal (`data/undo.data`) is
  written or read. Changes are final; `timew-tui` can undo its last edit.
- `config <name>` removes the line instead of leaving it blank; values are
  written as given.
- `lengthen`, `shorten` and `resize` require a duration, and `move` a date
  (Timewarrior uses 0 or fails later).
- `since` and `until` work like `from` and `to`.
- `dom.rc.<name>` of an unset name is empty instead of an error.
- `help <command>` without installed man pages shows the command's usage.
- `info` is new: version, license and project page.
- `export :csv` is new: a CSV table instead of JSON.
- Intervals with a tag starting with exactly one underscore (`_break`) are
  left out of the totals of `summary`, `day`, `week` and `month`
  (Timewarrior counts them).
- Backslashes in annotations are kept.
- `:debug` produces no debug output.
