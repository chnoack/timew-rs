# timew-rs

A Rust implementation of [Timewarrior](https://timewarrior.net/) that uses
the same database format, plus **timew-tui**, a terminal UI for browsing
that data and correcting start and end times.

> **Status:** `timew-cli` implements all of Timewarrior's commands: tracking
> (`start`, `stop`, `track`, `continue`, …), editing (`modify`, `move`,
> `tag`, `join`, `split`, …), the reports (`summary`, `day`,
> `week`, `month`, `gaps`, …), extensions and configuration, except
> `undo`. It passes Timewarrior's own test suite (apart from the `undo`
> tests). `timew-tui` browses the data and can edit entries.

```
 timew-tui   ./examples/pm-2025
╭───────╮ ╭─────────────╮ ╭─────────────╮ ╭────────╮ ╭─────────╮   ╭───────╮
│ 1 Day │ │ 2 Yesterday │ │ 3 2025-03 ▾ │ │ 4 Year │ │ 5 Range │   │ 6 Tag │
╰───────╯ ╰─────────────╯ ╰─────────────╯ ╰────────╯ ╰─────────╯   ╰───────╯
╭ Summary · Month · 2025-03-01 – 2025-03-31 ───────────────────────────────────╮╭ Entry details ───────────────────────────────────────────╮
│                                                                              █│                                                          │
│  Wk  Date       Day Tags                             Start   End Time Total  █│  ID         @2047                                        │
│  W10 2025-03-03 Mon budget                            8:50  9:20 0:30        █│  Tags        budget                                      │
│                     stakeholders                      9:20 10:20 1:00        ║│  Annotation (none)                                       │
│                     meetings                         10:20 11:20 1:00        ║│                                                          │
│                     email, risks                     11:20 12:25 1:05        ║│  Day        Monday                                       │
│                     meetings, planning, stakeholders 12:55 14:55 2:00        ║│  Week       W10 2025                                     │
│                     reporting                        14:55 15:40 0:45        ║│  Start      2025-03-03 Mon 08:50                         │
│                     meetings                         15:40 15:55 0:15        ║│  End        2025-03-03 Mon 09:20                         │
│                     email                            15:55 16:50 0:55  7.50  ║│  Duration   0:30                                         │
│  W10 2025-03-04 Tue team                              7:20  7:50 0:30        ║│                                                          │
│                     stakeholders, team                7:50  8:35 0:45        ║╰──────────────────────────────────────────────────────────╯
│                     email                             8:35  9:20 0:45        ║╭ Hours per tag ───────────────────────────────────────────╮
│                     team                              9:20  9:50 0:30        ║│                                                          │
│                     email                             9:50 10:05 0:15        ║│  meetings      ███████████████  30.42 h  +7.50 h   18 %  │
│                     budget                           10:05 10:35 0:30        ║│  team          ██████████▌█▋    26.65 h  +5.35 h   16 %  │
│                     team                             10:35 11:35 1:00        ║│  email         ██████████▏▌     25.65 h  +3.35 h   15 %  │
│                     reporting                        11:35 12:00 0:25        ║│  planning      ████████▌█▍      21.43 h  +4.74 h   13 %  │
│                     budget, meetings                 12:45 13:00 0:15        ║│  stakeholders  ███████▋▉        19.38 h  +3.12 h   12 %  │
│                     team                             13:00 14:00 1:00        ║│  budget        ██████▌▎         16.33 h  +2.08 h   10 %  │
│                     stakeholders                     14:00 14:45 0:45        ║│  reporting     █████▌▌          14.01 h  +2.32 h    8 %  │
│                     meetings                         14:45 15:00 0:15        ║│  risks         █████▌▎          13.79 h  +2.04 h    8 %  │
│                     planning                         15:00 15:15 0:15        ║│                                                          │
│                     team                             15:15 15:45 0:30        ║│  Total 167.67 h    █ own share  █ shared (+)             │
│                     planning                         15:45 16:30 0:45  8.42  ║│                                                          │
│  W10 2025-03-05 Wed team                              7:55  8:10 0:15        ║│                                                          │
╰──────────────────────────────────────────────────────────────────────────────╯╰──────────────────────────────────────────────────────────╯
 Total 167.67 h  ·  21 days  ·  206 intervals  ·  avg/day 7.98 h  ·  row 1/206
 ↑↓ move  1-4 range  ⇧3/⇧4 pick  5 range  6 tag  PgUp/Dn page  r reload  q quit
```

*timew-tui showing March 2025 of the example data.*

## Contents

| Path | What it is |
| --- | --- |
| `timew-cli/` | The `timew-rs` crate: the data model and commands (the `timew_rs` library), plus the `timew-cli` command |
| `timew-tui/` | The `timew-tui` crate: the terminal UI, built on the library |
| `examples/` | A generator for example data, plus two years of generated data |
| `docs/` | Technical documentation (see below) |

## Building

You need a Rust toolchain that supports edition 2024 (Rust 1.85 or later).

```sh
cargo build --release     # builds both programs
cargo test                # runs all tests
```

The programs are written to `target/release/timew-cli` and
`target/release/timew-tui`. To install them into `~/.cargo/bin`:

```sh
cargo install --path timew-cli   # timew-cli
cargo install --path timew-tui   # timew-tui
```

## Quick start with the example data

The repository includes a year of generated project-manager data
(`examples/pm-2025`, `examples/pm-2026`). See
[docs/example-data.md](docs/example-data.md).

```sh
export TIMEWARRIORDB=$PWD/examples/pm-2025

# timew-cli
./target/release/timew-cli summary 2025-03-03 - 2025-03-08
./target/release/timew-cli week 2025-03-03
./target/release/timew-cli export 2025-03-03 - 2025-03-04 meetings
./target/release/timew-cli get dom.tracked.count dom.tags.count
./target/release/timew-cli tags :lastyear

# timew-tui
./target/release/timew-tui
```

The data is from 2025, so the TUI's default view (the current month) is empty.
Press Shift+4 and pick 2025, or press `c` and enter `:lastyear`.

## timew-cli (command line)

`timew-cli` behaves like the `timew` command: same commands, same output,
same files. It was compared against the real `timew` byte for byte, and it
passes Timewarrior's own test suite (apart from the `undo` tests: there is
no `undo` command and no undo journal).

```sh
timew-cli start meetings client-a          # start tracking now
timew-cli stop                             # stop
timew-cli track 9am - 10:30 email          # record a past interval
timew-cli summary :week                    # this week's table
timew-cli week                             # this week's chart
timew-cli tag @2 budget                    # add a tag to the 2nd most recent
timew-cli modify end @1 16:45              # fix an end time
```

| Group | Commands |
| --- | --- |
| Tracking | `start`, `stop`, `track`, `continue`, `cancel` |
| Editing | `annotate`, `tag`, `untag`, `retag`, `delete`, `modify`, `move`, `lengthen`, `shorten`, `resize`, `split`, `join`, `fill`, `import` |
| Reports | `summary`, `day`, `week`, `month`, `gaps`, `tags`, `export`, `get`, extension reports |
| Other | `config`, `show`, `extensions`, `diagnostics`, `help` |

Dates can be ISO (`2025-03-01T08:00`), informal (`8am`, `yesterday`,
`monday`, `eom`) or range hints (`:week`, `:lastmonth`), durations
`1h`, `30min`. The configuration (`timewarrior.cfg`), exclusions, holidays,
and tag colors work as in Timewarrior (there is no `undo`; see below). The database is
found the same way (`$TIMEWARRIORDB`, `~/.timewarrior`, or the XDG
directories). Full reference, including the few deliberate differences:
[docs/timew-cli.md](docs/timew-cli.md).

## timew-tui (terminal UI)

A separate program for browsing the summary interactively:

- **Database:** opens `$TIMEWARRIORDB`, or Timewarrior's standard location
  if that isn't set.
- **Summary:** scrollable, with colors that alternate from week to week.
- **Details:** a side panel for the selected entry when the window is wide
  enough, otherwise a bottom panel opened with `Enter`.
- **Chart:** a bar chart of the hours per tag for the entries in the list,
  with one color per tag. An entry with several tags is split evenly among
  them, so the bars add up to the total; the time shared with other tags is
  shown in a lighter shade. It's always shown below the details in a wide
  window; in a narrow one, `b` shows it.
- **Filters:** quick buttons for day, yesterday, month and year, plus lists
  to pick a month (Shift+3) or one of the last 5 years (Shift+4), a form for
  your own date range (`5`), and a tag list (`6`).
- **Editing:** `e` edits the start and end date and time, the tags and the
  annotation of the selected entry in the details panel. Overlaps are refused, or adjusted with
  Shift+Enter (or Alt+Enter), and `u` undoes the last edit.

Press `?` in the TUI for a list of all keys. Details: [docs/timew-tui.md](docs/timew-tui.md).

## Documentation

| Document | Contents |
| --- | --- |
| [docs/architecture.md](docs/architecture.md) | Crate and module layout, data flow, the invariants the code relies on, the test approach |
| [docs/timew-cli.md](docs/timew-cli.md) | CLI reference: commands, dates, ranges, hints, configuration, differences from Timewarrior |
| [docs/timew-tui.md](docs/timew-tui.md) | TUI user guide (keys, filters, layouts) and how it is built internally |
| [docs/data-format.md](docs/data-format.md) | The on-disk format: directory layout, `.data` lines, `tags.data`, `timewarrior.cfg`, IDs |
| [docs/example-data.md](docs/example-data.md) | The example data generator and the included data sets |

## Relationship to Timewarrior

This is a clean-room reimplementation. The behavior was worked out by reading
the Timewarrior C++ source (GothenburgBitFactory/timewarrior, `develop`
branch, commit `3699d2e`) and its tests, and by comparing against the real
`timew` binary. The tests in `timew-cli/tests/cli.rs` contain output captured from
`timew`, and `timew-cli` passes Timewarrior's Python test suite (except
for `undo`). The two
programs share the same files, so `timew-cli` and `timew-tui` can be used on
an existing Timewarrior database, and Timewarrior can keep using a database
that `timew-cli` wrote to.
