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

![timew-cli showing a summary and a week chart of the example data](docs/images/timew-cli.jpg)

*timew-cli with the example data (`./examples/pm-2026`): `summary` of one day with IDs, and the `week` chart.*

<details>
<summary>Text version</summary>

```
$ timew-cli summary 2026-11-10 :ids

Wk  Date       Day ID   Tags               Annotation    Start      End    Time   Total
--- ---------- --- ---- ------------------ ---------- -------- -------- ------- -------
W46 2026-11-10 Tue @351 budget                         8:45:00  9:45:00 1:00:00
                   @350 stakeholders, team             9:45:00 10:00:00 0:15:00
                   @349 risks                         10:00:00 11:30:00 1:30:00
                   @348 meetings                      11:30:00 12:30:00 1:00:00
                   @347 stakeholders                  12:30:00 12:45:00 0:15:00
                   @346 meetings                      12:45:00 13:20:00 0:35:00
                   @345 risks                         14:05:00 15:05:00 1:00:00
                   @344 meetings, planning            15:05:00 15:50:00 0:45:00
                   @343 reporting                     15:50:00 17:00:00 1:10:00 7:30:00

                                                                                7:30:00

$ timew-cli week 2026-11-09 - 2026-11-14

           0    1    2    3    4    5    6    7    8    9    10   11   12   13   14   15   16   17   18   19   20   21   22   23     Total
W46 Mon  9                                           teaemmeetingsreemai    mriteplstakemeetemai                                      7:40
    Tue 10                                            budgestrisks  meetismee    risksmeereporti                                      7:30
    Wed 11                                         meetings  risks  te     emamerepemreportin                                         7:25
    Thu 12                                     risks  reportinstakeho   planning  meetsrepor                                          8:25
    Fri 13                                       smeetibudget    mee   plannememarestakeh                                             7:35

                                                                                                                                     38:35

           Tracked        38:35:00
           Available      81:25:00
           Total         120:00:00
```

</details>

![timew-tui showing November 2026 of the example data](docs/images/timew-tui.jpg)

*timew-tui with the same data, showing November 2026.*

<details>
<summary>Text version</summary>

```
 timew-tui   ./examples/pm-2026
╭───────╮ ╭─────────────╮ ╭─────────╮ ╭────────╮ ╭─────────╮   ╭───────╮
│ 1 Day │ │ 2 Yesterday │ │ 3 Month │ │ 4 Year │ │ 5 Range │   │ 6 Tag │
╰───────╯ ╰─────────────╯ ╰─────────╯ ╰────────╯ ╰─────────╯   ╰───────╯
╭ Summary · Month · 2026-11-01 – 2026-11-30 ────────────────────────────────╮╭ Entry details ──────────────────────────────────────────╮
│                                                                           ║│                                                         │
│  Wk  Date       Day Tags                          Start   End Time Total  █│  ID         @399                                        │
│  W45 2026-11-02 Mon email, team                    7:15  8:15 1:00        █│  Tags        stakeholders                               │
│                     planning                       8:15 10:00 1:45        █│  Annotation (none)                                      │
│                     meetings, reporting           10:30 10:45 0:15        ║│                                                         │
│                     meetings                      11:15 11:30 0:15        ║│  Day        Monday                                      │
│                     team                          11:30 11:50 0:20        ║│  Week       W45 2026                                    │
│                     email                         12:50 13:35 0:45        ║│  Start      2026-11-02 Mon 13:35                        │
│                     stakeholders                  13:35 14:20 0:45        ║│  End        2026-11-02 Mon 14:20                        │
│                     reporting                     14:20 16:20 2:00        ║│  Duration   0:45                                        │
│                     meetings, planning            16:20 16:45 0:25  7.50  ║│                                                         │
│  W45 2026-11-03 Tue stakeholders                   7:00  8:00 1:00        ║╰─────────────────────────────────────────────────────────╯
│                     email, meetings                8:00  9:00 1:00        ║╭ Hours per tag ──────────────────────────────────────────╮
│                     stakeholders                   9:00 10:00 1:00        ║│                                                         │
│                     meetings, planning            11:30 11:45 0:15        ║│  meetings      ███████████▊██  34.00 h  +6.50 h   21 %  │
│                     risks                         11:45 12:00 0:15        ║│  planning      ████████▎█▍     23.85 h  +6.15 h   14 %  │
│                     planning                      12:45 13:00 0:15        ║│  stakeholders  ███████▊▌       22.29 h  +2.46 h   13 %  │
│                     stakeholders                  13:00 15:00 2:00        ║│  team          ████████▎       20.25 h  +3.58 h   12 %  │
│                     budget, email, planning       15:00 16:10 1:10  6.92  ║│  email         ██████▌█▎       18.85 h  +4.99 h   11 %  │
│  W45 2026-11-04 Wed budget                         7:30  8:00 0:30        ║│  reporting     ██████▍         18.42 h  +0.50 h   11 %  │
│                     stakeholders                   8:00 10:00 2:00        ║│  budget        █████▎▍         15.18 h  +3.15 h    9 %  │
│                     budget, team                  10:00 10:30 0:30        ║│  risks         ████▌           13.00 h  +0.50 h    8 %  │
│                     meetings                      10:30 12:30 2:00        ║│                                                         │
│                     reporting                     12:30 13:30 1:00        ║│  Total 165.83 h    █ own share  █ shared (+)            │
╰───────────────────────────────────────────────────────────────────────────╯╰─────────────────────────────────────────────────────────╯
 Total 165.83 h  ·  21 days  ·  187 intervals  ·  avg/day 7.90 h  ·  row 7/187
 ? help  s start  ↑↓ move  1-4 range  ⇧3/⇧4 pick  5 range  6 tag  e edit  d delete  PgUp/Dn page  r reload  i info  q quit
```

</details>

## Contents

| Path | What it is |
| --- | --- |
| `timew-core/` | The `timew-core` library: the data model, database, configuration, dates and overlaps, shared by both programs |
| `timew-cli/` | The `timew-cli` command line program, built on `timew-core` |
| `timew-tui/` | The `timew-tui` terminal UI, built on `timew-core` |
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

### Shorter names

For quicker typing, create aliases in your shell configuration
(`~/.zshrc` or `~/.bashrc`):

```sh
alias tw='timew-cli'
alias tu='timew-tui'
```

Then `tw summary :week` and `tu` work like the full names. For fish, use
`alias --save tw timew-cli` and `alias --save tu timew-tui`.

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
  where neighboring bars always have clearly different colors. An entry with several tags is split evenly among
  them, so the bars add up to the total; the time shared with other tags is
  shown in a lighter shade. It's always shown below the details in a wide
  window; in a narrow one, `b` shows it.
- **Filters:** quick buttons for day, yesterday, month and year, plus lists
  to pick a month (Shift+3) or one of the last 5 years (Shift+4), a form for
  your own date range (`5`), and a tag list (`6`).
- **Tracking:** `s` starts a new entry (asking for tags and annotation)
  or stops the running one; a running entry's details have a red border.
- **Editing:** `e` edits the start and end date and time, the tags and the
  annotation of the selected entry in the details panel; `d` deletes it
  after asking, optionally giving its time to the previous entry. Overlaps are refused, or adjusted with
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

This is a reimplementation in Rust, not a line-by-line translation. The behavior was worked out by reading
the Timewarrior C++ source (GothenburgBitFactory/timewarrior, `develop`
branch, commit `3699d2e`) and its tests, and by comparing against the real
`timew` binary. The tests in `timew-cli/tests/cli.rs` contain output captured from
`timew`, and `timew-cli` passes Timewarrior's Python test suite (except
for `undo`). The two
programs share the same files, so `timew-cli` and `timew-tui` can be used on
an existing Timewarrior database, and Timewarrior can keep using a database
that `timew-cli` wrote to.

## License

MIT, see [LICENSE](LICENSE). timew-rs is based on Timewarrior, which is
also MIT licensed; its copyright notice (Thomas Lauf, Paul Beckingham,
Federico Hernandez) is kept in the license. `timew-cli info` and the `i`
dialog in timew-tui show the version, license and copyright.
