# Architecture

## Workspace layout

The repository is a Cargo workspace with three crates, one per folder,
plus the example data generator:

```
timew-rs/                    the repository
├── Cargo.toml               [workspace] members = the three crates + examples/generate-pm-data
├── timew-core/              library "timew_core": the shared data model
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs           module overview, VERSION
│       ├── interval.rs      one interval: .data line and JSON forms
│       ├── datafile.rs      one YYYY-MM.data file; atomic writes
│       ├── database.rs      all datafiles + tags.data; IDs
│       ├── tags.rs          tags.data (tag -> usage count)
│       ├── rules.rs         timewarrior.cfg: defaults, parsing, editing
│       ├── paths.rs         where config and data live; first-run setup
│       ├── exclusion.rs     exclusions and holidays
│       ├── range.rs         half-open time ranges and range arithmetic
│       ├── datetime.rs      date parsing, local calendar math, range hints
│       ├── duration.rs      duration parsing and formats
│       ├── session.rs       configuration + database + feedback; records changes
│       ├── validate.rs      overlaps (:adjust) and :fill
│       ├── summary.rs       the rows of a summary (shared by both programs)
│       ├── edit.rs          change one entry and undo that (used by the TUI)
│       └── text.rs          widths, wrapping, quoting, JSON strings
├── timew-cli/               program "timew-cli": the command line
│   ├── Cargo.toml           depends on timew-core
│   ├── src/
│   │   ├── main.rs          entry point; makes the core modules available as crate::…
│   │   ├── app.rs           startup: hints, paths, configuration, database, extensions
│   │   ├── session.rs       the core session + the parsed command line + extensions
│   │   ├── cli.rs           classifies arguments; the range grammar
│   │   ├── commands/        one module per command (dispatch in mod.rs)
│   │   ├── table.rs         column layout for the text reports
│   │   ├── composite.rs     layered text for the charts
│   │   ├── color.rs         color specifications -> ANSI codes
│   │   └── extensions.rs    extension reports (running scripts)
│   └── tests/cli.rs         end-to-end tests of the binary
├── timew-tui/               program "timew-tui": the terminal UI
│   ├── Cargo.toml           depends on timew-core, ratatui, jiff
│   └── src/main.rs          the whole TUI (state, input, drawing, tests)
└── examples/
    ├── generate-pm-data/    program "generate-pm-data": example data (depends on jiff only)
    └── pm-2025/, pm-2026/   the generated example databases
```

- **`timew-core`** holds everything both programs need: the files, the
  configuration, dates, overlaps, the summary rows and the edit functions.
  It has no command line parsing and no terminal output.
- **`timew-cli`** is the command line program on top of it: argument
  parsing, the commands, and their text output (tables, charts, colors).
- **`timew-tui`** is the terminal UI on top of the same core. It doesn't
  depend on `timew-cli`, and `ratatui` is never compiled into `timew-cli`.
- **`generate-pm-data`** writes the example databases (see
  [example-data.md](example-data.md)). It is not published and shares no
  code with the others.

Each crate has its own version (in its `Cargo.toml`); `timew-cli
--version` prints the CLI's, the TUI's info dialog (`i`) shows its own and
the core's. The workspace shares one `Cargo.lock` and one `target/`
directory at the top.

Dependencies are deliberately few:

| Crate | Used for |
| --- | --- |
| `jiff` | Time zones, local-time calendar math, ISO week numbers |
| `serde_json` | Reading JSON (`tags.data`, `import`) |
| `unicode-width` | Display width of characters in tables and charts |
| `ratatui` (TUI only) | Terminal UI widgets; its bundled `crossterm` handles terminal input/output |

## One run of `timew-cli`

```
main.rs ── app::run(args)
             │
             ├─ cli::Cli::parse            hints such as :yes, :quiet, :color
             ├─ paths::Paths::initialize   create directories / config file if missing
             ├─ rules::Rules::load         defaults + timewarrior.cfg (+ hints, rc.* overrides)
             ├─ database::Database::open_and_repair   rebuild tags.data if needed
             ├─ extensions::Extensions::load          extension names become commands
             ├─ cli::Cli::parse            again, now knowing the extensions
             └─ commands::dispatch(&mut Session)
                    │  reads:   session.tracked*, exclusions, rules
                    │  writes:  session.add / delete / modify  ("new tag" notes)
                    └─ on success: database.commit()   (datafiles, tags.data)
```

- **Output** is collected in `Session::out` and printed when the command
  is done (or before a confirmation question). Errors are returned as
  `Err(String)`, printed to stderr, and give exit status 255; on an error
  the database is not committed.
- **`Session`** (in `timew-core`) is the only place that changes
  intervals: `add`, `delete` and `modify` record each change in
  `Session::changes` (in memory only; `timew-tui` uses it to undo an edit)
  and print the "new tag" notes. There is no undo journal. The CLI's own
  `Session` wraps it together with the parsed command line and the
  extensions, and dereferences to it, so a command uses both through one
  value. `Session::feedback` is how the core shows a recorded interval;
  the CLI sets it to its "Recorded …" text (`commands::summarize`).
- **Reading** goes through `Session::tracked()` (all intervals with IDs,
  newest first), `tracked_in(range, tags)` and `tracked_ids(ids)` (both
  oldest first). These include the synthetic pieces of a running interval
  (see below).
- **`validate::auto_adjust`** is called before every add of an interval
  that could overlap. It stops a running interval when a new one starts,
  and either rejects overlaps or, with `:adjust`, resolves them.

### The summary as structured data

The summary is split in two:

- `timew_core::summary::build(&Range, &[Tracked]) -> Summary` produces a `Vec<Row>` and
  the grand total. Each `Row` holds the text of every cell (week, date, day,
  ID, tags, annotation, start, end, time, daily total) plus `source`, the
  index of the `Tracked` it came from.
- `render` in `timew-cli`'s `commands::summary` lays the rows out as the
  `timew summary` table, with the columns the configuration asks for.

The CLI uses both. The TUI uses only `build`, then draws the rows in a
ratatui `Table` and uses `source` to find the full interval for the details
panel. An interval that crosses midnight produces one row per day, all with
the same `source`.

## Invariants the code relies on

| Invariant | Where |
| --- | --- |
| **IDs are never stored.** `@1` is the most recent interval, `@2` the one before it, and so on. They are recomputed on every read, so an ID can refer to a different interval after the data changes. | `database.rs`, `session.rs` |
| Datafile names sort lexicographically in chronological order, and lines within a file are kept sorted as strings. Since timestamps have a fixed width, string order is time order. | `database.rs`, `datafile.rs` |
| An interval is stored in the file of the (local) month it **starts** in, even if it ends in a later month. | `database.rs`, `interval.rs` |
| Every line written is parsed back and compared before it is added. | `datafile.rs` |
| Timestamps are stored in UTC. Range hints, day boundaries and displayed times use **local time** (system tzdata via `jiff`), so DST is handled correctly. | `datetime.rs` |
| Weeks start on **Monday**. Week numbers are **ISO 8601** (for example, 2026-12-31 is W53). | `datetime.rs` |
| Ranges are half-open `[start, end)`. `end == 0` means open, either an interval that is still running or a range with no upper bound. | `range.rs` |
| Tags are kept sorted and deduplicated per interval, which also fixes the order in files and exports. | `interval.rs` |
| A tag filter with several tags matches only intervals that have **all** of them. | `session.rs` |
| Stored intervals never overlap (unless the file was edited by hand). | `validate.rs` |
| A running interval spanning exclusions is shown as *synthetic* pieces, but stored as one interval; commands that change intervals store the pieces first (`Session::flatten_latest`). | `session.rs` |

## Testing

```sh
cargo test
```

- **Library (unit tests in each module):** date and duration parsing,
  range arithmetic, the configuration parser, colors, table layout, the
  the database files.
- **Command line (`timew-cli/tests/cli.rs`):** runs the `timew-cli` binary in
  throwaway databases (with `TZ=Europe/Berlin`, 80 columns) and compares
  output and files with what the original `timew` produced for the same
  commands.
- **TUI:** the UI is drawn into ratatui's in-memory `TestBackend`, and the
  tests inspect the resulting screen text. Key presses and mouse clicks
  are simulated by calling `App::on_key` and `App::on_mouse` directly.
  To see the screens a test draws:

  ```sh
  cargo test -p timew-tui -- --nocapture --test-threads=1 wide_window
  ```

### Timewarrior's own test suite

`timew-cli` also passes the Python test suite of the original project
(`test/*.t` in the Timewarrior repository; the exceptions are `undo.t`,
since there is no `undo` command, and tests that fail for the original
too, such as those needing installed man pages).
The suite runs `../../src/timew` relative to its `basetest` directory, so
point that at `timew-cli`:

```sh
cp -r ~/src/timewarrior/test ~/src/timewarrior/ext /tmp/tw/   # test/ and ext/
mkdir -p /tmp/tw/src && ln -s $PWD/target/release/timew-cli /tmp/tw/src/timew
cd /tmp/tw/test && for t in *.t; do python3 $t; done 2>&1 | grep -E '^(ok|not ok)'
```

## Extending

- **A new command:** in `timew-cli`, add `commands/<name>.rs` with
  `pub fn run(s: &mut Session) -> Result<i32, String>`, register it in
  `commands/mod.rs` (`dispatch`) and add its name to `cli::COMMANDS`.
- **Something both programs need** belongs in `timew-core`; output
  formatting for the terminal and argument handling stay in `timew-cli`.
- **Changing data:** use `Session::add`/`delete`/`modify`, and call
  `validate::auto_adjust` before adding an interval that might overlap.
- **The TUI** reads through `Database::open`, `all_tracked`,
  `tracked_in_range` and `summary::build`. Its only write is
  `timew_core::modify_interval` (editing an entry), which goes through
  `Session`, so it is checked for overlaps like a command; its
  `undo_change` reverts the last edit.
