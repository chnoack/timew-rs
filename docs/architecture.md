# Architecture

## Workspace layout

The repository is a Cargo workspace with two crates:

```
timew-rs/                    crate "timew-rs" (workspace root)
├── Cargo.toml               package + [workspace] members = ["timew-tui"]
├── src/
│   ├── lib.rs               library root: re-exports every module
│   ├── main.rs              the timew-rs CLI (thin: parse args, dispatch)
│   ├── cli.rs               argument classification
│   ├── datetime.rs          ISO date parsing and :hint expansion
│   ├── range.rs             half-open time ranges
│   ├── interval.rs          one tracked interval + its .data line format
│   ├── datafile.rs          one YYYY-MM.data file
│   ├── database.rs          all datafiles + ID assignment
│   ├── tags.rs              tags.data (tag -> usage count)
│   ├── journal.rs           undo.data (placeholder)
│   ├── validate.rs          overlap / :adjust logic (placeholder)
│   └── commands/            one module per command
│       ├── export.rs  get.rs  summary.rs  tags.rs      (implemented)
│       └── track.rs start.rs stop.rs …                 (placeholders)
└── timew-tui/               crate "timew-tui"
    ├── Cargo.toml           depends on timew-rs (path = ".."), ratatui, jiff
    └── src/main.rs          the whole TUI (state, input, drawing, tests)
```

The `timew-rs` crate builds both a **library** (`src/lib.rs`) and the
**`timew-rs` binary** (`src/main.rs`). `timew-tui` uses only the library.
Keeping the TUI in its own crate means `ratatui` is never compiled into
`timew-rs`.

Dependencies are deliberately few:

| Crate | Used for |
| --- | --- |
| `jiff` | Time zones, local-time calendar math, ISO week numbers |
| `serde`, `serde_json` | `tags.data` and JSON output |
| `ratatui` (TUI only) | Terminal UI widgets; its bundled `crossterm` handles terminal input/output |

## Layers

```
            timew-rs (CLI)                     timew-tui
   main.rs → cli::parse → commands::*      App (state) → draw / on_key / on_mouse
                    │                                │
                    ▼                                ▼
        commands::{export,get,summary,tags}   commands::summary::build
                    │                                │
                    └──────────────┬─────────────────┘
                                   ▼
                     database::Database  ── tags::TagsDatabase
                                   │
                         datafile::Datafile  (one per YYYY-MM.data)
                                   │
                interval::Interval ── range::Range ── datetime
```

1. **Storage** (`datafile`, `tags`): read and write files, nothing else.
2. **Model** (`range`, `interval`, `datetime`): pure data and parsing, no I/O.
3. **Database** (`database`): combines all datafiles, answers queries
   (`all_tracked`, `tracked_in_range`, `tracked_by_ids`, `latest`), and
   assigns IDs.
4. **Commands** (`commands::*`): each takes a `&mut Database` plus the
   parsed arguments and **returns the output as a `String`**. Commands never
   print; `main.rs` does. This keeps them testable and lets the TUI reuse
   them.
5. **Front ends**: the CLI (`src/main.rs`) and the TUI (`timew-tui`).

### The summary as structured data

`commands::summary` is split in two:

- `summary::build(&Range, &[Tracked]) -> Summary` produces a `Vec<Row>` and
  the grand total. Each `Row` holds the text of every cell (week, date, day,
  ID, tags, annotation, start, end, time, daily total) plus `source`, the
  index of the `Tracked` it came from.
- `format_table` turns those rows into the exact text that `timew summary`
  prints.

The CLI uses both. The TUI uses only `build`, then draws the rows in a
ratatui `Table` and uses `source` to find the full interval for the details
panel. An interval that crosses midnight produces one row per day, all with
the same `source`.

## Invariants the code relies on

These are the non-obvious rules. They are explained in detail in the doc
comments of the modules listed.

| Invariant | Where |
| --- | --- |
| **IDs are never stored.** `@1` is the most recent interval, `@2` the one before it, and so on. They are recomputed on every read, so an ID can refer to a different interval after the data changes. | `database.rs` |
| Datafile names sort lexicographically in chronological order, and lines within a file are kept sorted as strings. Since timestamps have a fixed width, string order is time order. | `database.rs`, `datafile.rs` |
| An interval is stored in the file of the month it **starts** in, even if it ends in a later month. | `database.rs` |
| Timestamps are stored in UTC. Range hints (`:day`, `:month`, …), day boundaries in the summary, and displayed times use **local time** (system tzdata via `jiff`), so DST is handled correctly. | `datetime.rs`, `summary.rs` |
| Weeks start on **Monday**. Week numbers are **ISO 8601** (for example, 2026-12-31 is W53). | `datetime.rs`, `summary.rs` |
| Ranges are half-open `[start, end)`. `end == 0` means open, either an interval that is still running or a range with no upper bound. | `range.rs` |
| Tags are kept sorted and deduplicated per interval, which also fixes the order in exports. | `interval.rs` |
| A tag filter with several tags matches only intervals that have **all** of them. | `commands/export.rs`, `commands/summary.rs` |

## Database location

| Program | Rule |
| --- | --- |
| `timew-rs` | `$TIMEWARRIORDB` only (exits with an error otherwise) |
| `timew-tui` | `$TIMEWARRIORDB`, else `~/.timewarrior` if that directory exists, else `$XDG_DATA_HOME/timewarrior`, else `~/.local/share/timewarrior` (the same order as Timewarrior's `paths.cpp`) |

Both then read `<root>/data/`.

## Testing

```sh
cargo test --workspace
```

- **Library and CLI (136 tests):** unit tests in each module. Many assert
  against output **captured from the real `timew` binary** in a sandbox
  (`TIMEWARRIORDB=<tmp> timew …`), so formatting is checked byte for byte.
  Tests that need a database create a throwaway one under the system temp
  directory.
- **TUI (16 tests):** the UI is drawn into ratatui's in-memory
  `TestBackend`, and the tests inspect the resulting screen text. Key
  presses and mouse clicks are simulated by calling `App::on_key` and
  `App::on_mouse` directly, so no real terminal is needed. To see the
  screens a test draws:

  ```sh
  cargo test -p timew-tui -- --nocapture --test-threads=1 wide_window
  ```

## Extending

- **A new CLI command:** add `commands/<name>.rs` with
  `pub fn run(&mut Database, …) -> Result<String, String>`, register it in
  `commands/mod.rs`, and add a match arm in `src/main.rs`.
- **Anything that writes data:** `Database::add_interval`, `delete_interval`,
  `modify_interval` and `commit` exist, and datafile writes are atomic
  (written to a temporary file, then renamed over the original). Overlap
  handling (`validate.rs`) and the undo journal (`journal.rs`) are still
  placeholders and must be done before any write command can be trusted.
- **The TUI** reads only through `Database::open`, `all_tracked`,
  `tracked_in_range` and `summary::build`. It has no code that writes.
