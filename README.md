# timew-rs

A Rust implementation of [Timewarrior](https://timewarrior.net/) that uses
the same database format, plus **timew-tui**, a read-only terminal UI for
browsing that data.

> **Status:** the data model and all *read-only* commands work (`export`,
> `get`, `summary`, `tags`). Commands that modify the database (`track`,
> `start`, `stop`, `modify`, `undo`, …) are not implemented yet. Neither
> program writes to your database.

```
 timew-tui   ./examples/pm-2025  (read-only)
╭───────╮ ╭─────────────╮ ╭─────────────╮ ╭────────╮ ╭─────────╮   ╭───────╮
│ 1 Day │ │ 2 Yesterday │ │ 3 2025-03 ▾ │ │ 4 Year │ │ 5 Range │   │ 6 Tag │
╰───────╯ ╰─────────────╯ ╰─────────────╯ ╰────────╯ ╰─────────╯   ╰───────╯
╭ Summary · Month · 2025-03-01 – 2025-03-31 ───────────────╮╭ Entry details ───────────────────────────────╮
│                                                          █│                                              │
│  Wk  Date       Day Tags         Start   End Time Total  █│  ID         @2047                            │
│  W10 2025-03-03 Mon budget        8:50  9:20 0:30        █│  Tags        budget                          │
│                     stakeholders  9:20 10:20 1:00        ║│  Annotation (none)                           │
│                     meetings     10:20 11:20 1:00        ║│                                              │
│                     risks        11:20 12:25 1:05        ║│  Day        Monday                           │
│                     meetings     12:55 14:55 2:00        ║│  Week       W10 2025                         │
│                     reporting    14:55 15:40 0:45        ║│  Start      2025-03-03 Mon 08:50             │
│                     meetings     15:40 15:55 0:15        ║│  End        2025-03-03 Mon 09:20             │
│                     email        15:55 16:50 0:55  7.50  ║│  Duration   0:30                             │
│  W10 2025-03-04 Tue team          7:20  7:50 0:30        ║│                                              │
│                     stakeholders  7:50  8:35 0:45        ║╰──────────────────────────────────────────────╯
│                     email         8:35  9:20 0:45        ║╭ Hours per tag ───────────────────────────────╮
│                     team          9:20  9:50 0:30        ║│                                              │
│                     email         9:50 10:05 0:15        ║│  meetings      ████████████  29.67 h   18 %  │
│                     budget       10:05 10:35 0:30        ║│  team          ██████████▉   26.75 h   16 %  │
│                     team         10:35 11:35 1:00        ║│  email         ██████████▊   26.67 h   16 %  │
│                     reporting    11:35 12:00 0:25        ║│  planning      ████████▏     20.00 h   12 %  │
│                     meetings     12:45 13:00 0:15        ║│  budget        ███████▍      18.17 h   11 %  │
│                     team         13:00 14:00 1:00        ║│  stakeholders  ███████▎      18.00 h   11 %  │
│                     stakeholders 14:00 14:45 0:45        ║│  reporting     ██████▏       15.08 h    9 %  │
│                     meetings     14:45 15:00 0:15        ║│  risks         █████▍        13.33 h    8 %  │
│                     planning     15:00 15:15 0:15        ║│                                              │
│                     team         15:15 15:45 0:30        ║│  Total 167.67 h                              │
│                     planning     15:45 16:30 0:45  8.42  ║│                                              │
│  W10 2025-03-05 Wed team          7:55  8:10 0:15        ║│                                              │
╰──────────────────────────────────────────────────────────╯╰──────────────────────────────────────────────╯
 Total 167.67 h  ·  21 days  ·  206 intervals  ·  avg/day 7.98 h  ·  row 1/206
 ↑↓ move  1-4 range  ⇧3/⇧4 pick  5 range  6 tag  PgUp/Dn page  r reload  q quit
```

*timew-tui showing March 2025 of the example data.*

## Contents

| Path | What it is |
| --- | --- |
| `src/` | The `timew-rs` crate: the data model and commands (a library), plus the `timew-rs` CLI |
| `timew-tui/` | The `timew-tui` crate: the terminal UI, built on the library |
| `examples/` | A generator for example data, plus two years of generated data |
| `docs/` | Technical documentation (see below) |

## Building

You need a Rust toolchain that supports edition 2024 (Rust 1.85 or later).

```sh
cargo build --release --workspace     # builds both programs
cargo test --workspace                # runs all tests
```

The programs are written to `target/release/timew-rs` and
`target/release/timew-tui`. To install them into `~/.cargo/bin`:

```sh
cargo install --path .           # timew-rs
cargo install --path timew-tui   # timew-tui
```

## Quick start with the example data

The repository includes a year of generated project-manager data
(`examples/pm-2025`, `examples/pm-2026`). See
[docs/example-data.md](docs/example-data.md).

```sh
export TIMEWARRIORDB=$PWD/examples/pm-2025

# timew-rs
./target/release/timew-rs summary 2025-03-03 - 2025-03-08
./target/release/timew-rs export 2025-03-03 - 2025-03-04 meetings
./target/release/timew-rs get dom.tracked.count dom.tags.count
./target/release/timew-rs tags :lastyear

# timew-tui
./target/release/timew-tui
```

The data is from 2025, so the TUI's default view (the current month) is empty.
Press Shift+4 and pick 2025, or press `c` and enter `:lastyear`.

## timew-rs (command line)

`timew-rs` behaves like the `timew` command for the commands it supports. It
was compared against real `timew` output byte for byte.

| Command | Example |
| --- | --- |
| `summary [<range>] [<tags>] [@ids]` | `timew-rs summary :week` |
| `export [<range>] [<tags>] [@ids]` | `timew-rs export :lastmonth meetings` |
| `tags [<range>] [<tags>]` | `timew-rs tags :year` |
| `get <dom-reference>…` | `timew-rs get dom.active.duration` |

A range is either `:hint` (`:day`, `:week`, `:month`, `:lastyear`, …) or ISO
dates (`2025-03-01 - 2025-03-31`). `timew-rs` currently **requires**
`TIMEWARRIORDB` to be set. Full reference: [docs/timew-rs.md](docs/timew-rs.md).

## timew-tui (terminal UI)

A separate program for browsing the summary interactively:

- **Database:** opens `$TIMEWARRIORDB`, or Timewarrior's standard location
  if that isn't set.
- **Summary:** scrollable, with colors that alternate from week to week.
- **Details:** a side panel for the selected entry when the window is wide
  enough, otherwise a bottom panel opened with `Enter`.
- **Chart:** a bar chart of the hours per tag for the entries in the list,
  with one color per tag. It's always shown below the details in a wide
  window; in a narrow one, `b` shows it.
- **Filters:** quick buttons for day, yesterday, month and year, plus lists
  to pick a month (Shift+3) or one of the last 5 years (Shift+4), a form for
  your own date range (`5`), and a tag list (`6`).

Press `?` in the TUI for a list of all keys. Details: [docs/timew-tui.md](docs/timew-tui.md).

## Documentation

| Document | Contents |
| --- | --- |
| [docs/architecture.md](docs/architecture.md) | Crate and module layout, data flow, the invariants the code relies on, the test approach |
| [docs/timew-rs.md](docs/timew-rs.md) | CLI reference: commands, ranges, hints, DOM references, differences from Timewarrior |
| [docs/timew-tui.md](docs/timew-tui.md) | TUI user guide (keys, filters, layouts) and how it is built internally |
| [docs/data-format.md](docs/data-format.md) | The on-disk database format: directory layout, `.data` lines, `tags.data`, IDs |
| [docs/example-data.md](docs/example-data.md) | The example data generator and the included data sets |

## Relationship to Timewarrior

This is a clean-room reimplementation. The behavior was worked out by reading
the Timewarrior C++ source (GothenburgBitFactory/timewarrior, `develop`
branch, commit `3699d2e`) and comparing against the real `timew` binary.
Several tests contain output captured from `timew`. The two programs share
the same database format, so `timew-rs` and `timew-tui` can be used on an
existing Timewarrior database.
