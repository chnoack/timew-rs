# Architecture

## Workspace layout

The repository is a Cargo workspace with two crates:

```
timew-rs/                    crate "timew-rs" (workspace root)
├── Cargo.toml               package + [workspace] members = ["timew-tui"]
├── src/
│   ├── main.rs              the timew-rs binary: calls timew_rs::run, prints
│   ├── lib.rs               startup: hints, paths, configuration, database
│   ├── cli.rs               classifies arguments; the range grammar
│   ├── session.rs           one run: database + config + journal + output
│   ├── validate.rs          overlaps (:adjust) and :fill
│   ├── commands/            one module per command (dispatch in mod.rs)
│   ├── datetime.rs          date parsing, local calendar math, range hints
│   ├── duration.rs          duration parsing and formats
│   ├── range.rs             half-open time ranges and range arithmetic
│   ├── interval.rs          one interval: .data line and JSON forms
│   ├── datafile.rs          one YYYY-MM.data file; atomic writes
│   ├── database.rs          all datafiles + tags.data; IDs
│   ├── tags.rs              tags.data (tag -> usage count)
│   ├── journal.rs           undo.data
│   ├── rules.rs             timewarrior.cfg: defaults, parsing, editing
│   ├── paths.rs             where config and data live; first-run setup
│   ├── exclusion.rs         exclusions and holidays
│   ├── extensions.rs        extension reports (running scripts)
│   ├── table.rs             column layout for the text reports
│   ├── composite.rs         layered text for the charts
│   ├── color.rs             color specifications -> ANSI codes
│   └── text.rs              widths, wrapping, quoting, JSON strings
├── tests/cli.rs             end-to-end tests of the binary
└── timew-tui/               crate "timew-tui"
    ├── Cargo.toml           depends on timew-rs (path = ".."), ratatui, jiff
    └── src/main.rs          the whole TUI (state, input, drawing, tests)
```

The `timew-rs` crate builds a **library** and the **`timew-rs` binary**.
`timew-tui` uses only the library, and only for reading. Keeping the TUI in
its own crate means `ratatui` is never compiled into `timew-rs`.

Dependencies are deliberately few:

| Crate | Used for |
| --- | --- |
| `jiff` | Time zones, local-time calendar math, ISO week numbers |
| `serde`, `serde_json` | Reading JSON (`tags.data`, `import`, the undo journal) |
| `unicode-width` | Display width of characters in tables and charts |
| `ratatui` (TUI only) | Terminal UI widgets; its bundled `crossterm` handles terminal input/output |

## One run of `timew-rs`

```
main.rs ── timew_rs::run(args)
             │
             ├─ cli::Cli::parse            hints such as :yes, :quiet, :color
             ├─ paths::Paths::initialize   create directories / config file if missing
             ├─ rules::Rules::load         defaults + timewarrior.cfg (+ hints, rc.* overrides)
             ├─ database::Database::open_and_repair   rebuild tags.data if needed
             ├─ extensions::Extensions::load          extension names become commands
             ├─ cli::Cli::parse            again, now knowing the extensions
             └─ commands::dispatch(&mut Session)
                    │  reads:   session.tracked*, exclusions, rules
                    │  writes:  session.add / delete / modify  (journal + "new tag" notes)
                    └─ on success: database.commit()   (datafiles, tags.data)
```

- **Output** is collected in `Session::out` and printed when the command
  is done (or before a confirmation question). Errors are returned as
  `Err(String)`, printed to stderr, and give exit status 255; on an error
  the database is not committed.
- **`Session`** is the only place that changes intervals: `add`, `delete`
  and `modify` record each change in the undo journal and print the "new
  tag" notes. Commands that change something call `journal.start()` and
  `journal.end()` around their changes.
- **Reading** goes through `Session::tracked()` (all intervals with IDs,
  newest first), `tracked_in(range, tags)` and `tracked_ids(ids)` (both
  oldest first). These include the synthetic pieces of a running interval
  (see below).
- **`validate::auto_adjust`** is called before every add of an interval
  that could overlap. It stops a running interval when a new one starts,
  and either rejects overlaps or, with `:adjust`, resolves them.

### The summary as structured data

`commands::summary` is split in two:

- `summary::build(&Range, &[Tracked]) -> Summary` produces a `Vec<Row>` and
  the grand total. Each `Row` holds the text of every cell (week, date, day,
  ID, tags, annotation, start, end, time, daily total) plus `source`, the
  index of the `Tracked` it came from.
- `summary::render` lays the rows out as the `timew summary` table, with
  the columns the configuration asks for.

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
cargo test --workspace
```

- **Library (unit tests in each module):** date and duration parsing,
  range arithmetic, the configuration parser, colors, table layout, the
  journal, the database files.
- **Command line (`tests/cli.rs`):** runs the `timew-rs` binary in
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

`timew-rs` also passes the Python test suite of the original project
(`test/*.t` in the Timewarrior repository; the exceptions are tests that
fail for the original too, such as those needing installed man pages).
The suite runs `../../src/timew` relative to its `basetest` directory, so
point that at `timew-rs`:

```sh
cp -r ~/src/timewarrior/test ~/src/timewarrior/ext /tmp/tw/   # test/ and ext/
mkdir -p /tmp/tw/src && ln -s $PWD/target/release/timew-rs /tmp/tw/src/timew
cd /tmp/tw/test && for t in *.t; do python3 $t; done 2>&1 | grep -E '^(ok|not ok)'
```

## Extending

- **A new command:** add `commands/<name>.rs` with
  `pub fn run(s: &mut Session) -> Result<i32, String>`, register it in
  `commands/mod.rs` (`dispatch`) and add its name to `cli::COMMANDS`.
- **Changing data:** use `Session::add`/`delete`/`modify` between
  `s.journal.start()` and `s.journal.end()`, and call
  `validate::auto_adjust` before adding an interval that might overlap.
- **The TUI** reads only through `Database::open`, `all_tracked`,
  `tracked_in_range` and `summary::build`. It has no code that writes.
