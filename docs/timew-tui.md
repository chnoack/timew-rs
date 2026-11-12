# timew-tui

A read-only terminal UI for browsing a Timewarrior database. It shows the
same rows as `timew summary`, with filters you can switch with single keys or
the mouse.

```sh
cargo build --release -p timew-tui
./target/release/timew-tui
```

## Which database it opens

1. `$TIMEWARRIORDB`, if set
2. `~/.timewarrior`, if that directory exists (older Timewarrior layout)
3. `$XDG_DATA_HOME/timewarrior`, which defaults to `~/.local/share/timewarrior`

If the chosen directory has no `data/` subdirectory, it exits with
`No Timewarrior database found at … (missing data/ directory).`

The database is opened read-only. Every load reads the files again, so `r`
shows changes made by `timew` while the TUI is running.

## The screen

```
 timew-tui   /path/to/db  (read-only)                                     ← header
╭───────╮ ╭─────────────╮ ╭─────────╮ ╭────────╮ ╭─────────╮   ╭───────╮
│ 1 Day │ │ 2 Yesterday │ │ 3 Month │ │ 4 Year │ │ 5 Range │   │ 6 Tag │  ← filter buttons
╰───────╯ ╰─────────────╯ ╰─────────╯ ╰────────╯ ╰─────────╯   ╰───────╯
╭ Summary · Month · 2026-11-01 – 2026-11-30 ──────╮╭ Entry details ────────────╮
│  Wk  Date       Day Tags      Start    End  …   ││  ID         @3            │  ← summary + details
│  W46 2026-11-11 Wed planning  8:00 12:00 4:00 … ││  Tags        planning     │
╰─────────────────────────────────────────────────╯╰───────────────────────────╯
 Total 7.00 h  ·  2 days  ·  3 intervals  ·  avg/day 3.50 h  ·  row 1/3        ← status
 ↑↓ move  1-4 range  ⇧3/⇧4 pick  5 range  6 tag  …  q quit                 ← help
```

- **Summary table:** columns Wk, Date, Day, Tags, Start, End, Time and Total.
  ID and Annotation are left out of the table but shown in the details.
  Wk/Date/Day are **cyan and magenta, alternating each ISO week**. Every
  other day has a slightly darker background, and the daily totals are
  yellow.
- **Times and durations:** start, end, the per-entry Time and the duration
  in the details are hours and minutes without seconds (`8:50`, `0:30`).
  Seconds are cut off, not rounded.
- **Totals** (the Total column, the status line's total and average) are
  **decimal hours** with two places: 7:30 is shown as `7.50`, 8:25 as
  `8.42`. `timew-rs summary` still shows `H:MM:SS` everywhere, because its
  output matches `timew`.
- **Help line:** shows as many keys as fit the window width, and always ends
  with `q quit`. If some keys don't fit, it starts with `? help`, so the
  legend is always reachable.
- **Key legend:** `?` opens a window listing every key, mouse action and
  popup key. In a low window it scrolls (`↑↓`, `PgUp`/`PgDn`, `g`/`G`, or
  the mouse wheel). `?`, `Esc`, `q`, `Enter` or a click closes it.
- **Status line:** the total, the number of days with entries, the number of
  rows, the average per day with entries, and your position in the list.
- **Average per day** counts only days that have entries.

### Details panel

It shows ID, tags, annotation, weekday, ISO week (`W46 2026`; the year is
the week's own year), full start and end (or *still tracking*), the
duration of the whole entry, and, for entries that cross midnight, how many
days it spans.

Where it appears depends on the window width:

| Window | Details panel |
| --- | --- |
| **Wide**: the table's width plus 44 columns fits | Always shown **on the right**, for the selected row. The top row is selected after every load. |
| **Narrow** | Shown **below** the table after `Enter` (or a click on the selected row). `Enter`/`Esc` closes it. |

The table is as wide as its columns need (mostly the longest tag), and at
least as wide as its title. So the switching point can move slightly between
filters. The layout is chosen again on every redraw, so resizing the
terminal switches immediately.

### Hours-per-tag chart

A **bar chart of the hours per tag**, where it appears depending on the
window width:

| Window | Chart |
| --- | --- |
| **Wide** | Always shown on the right, **below the details** |
| **Narrow** | Shown below the table after `b`, in place of the details. `b` or `Esc` closes it, and `Enter` switches the panel to the details. |

```
╭ Hours per tag ───────────────────────────────────────────╮
│  meetings      ███████████████  30.42 h  +7.50 h   18 %  │
│  team          ██████████▌█▋    26.65 h  +5.35 h   16 %  │
│  email         ██████████▏▌     25.65 h  +3.35 h   15 %  │
│  …                                                       │
│  Total 167.67 h    █ own share  █ shared (+)             │
╰──────────────────────────────────────────────────────────╯
```

(March 2025 of the example data. In the terminal, the first part of each bar
is the tag's strong color and the second part a lighter shade of it.)

- **What it counts:** exactly the entries in the list, so it follows the
  period and the tag filter. The hours are taken from the table's Time
  column, so an entry that crosses the edge of the range counts only its
  part inside the range.
- **Several tags:** an entry's time is **split evenly** among its tags: an
  entry of 3 h with the tags `email meetings risks` gives 1 h to each of
  them. That is the tag's **own share**, shown as the strong part of the
  bar and as the yellow number. The own shares of all tags add up exactly
  to the total, and the percentages (own share ÷ total) to 100 %.
- **Shared time:** the rest of the time of the entries that a tag shares
  with other tags (2 h for each tag in the example) is the light part of
  the bar and the gray `+… h` number. Own share plus shared time is the
  full time of all entries with that tag. Shared time is counted for every
  tag of the entry, so it doesn't add up to anything.
- **Order and scale:** sorted by own share, largest first. All bars use the
  same scale, set by the longest bar (own share plus shared time), and are
  drawn with eighth blocks.
- **Entries without a tag** are shown as "(no tag)", in gray.
- **Legend:** when there is shared time, the Total line explains the two
  shades.
- **Colors:** every tag has its own color pair (strong and light) from the
  256-color palette, used for its name and its bar. The pair comes from the
  tag's position in the alphabetical list of all tags in the database, so a
  tag keeps its colors when you change the filters. There are 12 pairs;
  after that they repeat.
- **Space:** in a narrow window, at most 10 tags are shown. In a wide window,
  the chart uses the height left below the details. If that isn't enough
  for every tag, the total line is dropped first, then the smallest tags are
  summed up as "… N more tags", and the title shows the total number of
  tags ("Hours per tag · 8"). If the window is too low for even one bar,
  the chart is hidden.

## Keys

### Main view

| Key | Action |
| --- | --- |
| `↑` `↓` / `k` `j` | Move up or down a row |
| `PgUp` `PgDn` / `Space` | Move a page |
| `Home` `End` / `g` `G` | First or last row |
| `Enter` / `i` | Open or close the details (narrow layout) |
| `b` | Show or hide the **hours-per-tag chart** (narrow layout; the wide layout always shows it) |
| `1` / `d` | Today |
| `2` / `y` | Yesterday |
| `3` / `m` | The current month |
| **Shift+3** (`§` on German, `#` on US keyboards) / `M` | **Month list** |
| `4` / `Y` | The current year |
| **Shift+4** (`$`) | **Year list** |
| `←` `→` / `h` `l` / `Tab` | Previous or next of the four fixed periods |
| `5` / `c` | **Date range form** |
| `6` / `t` | **Tag list** |
| `r` | Reload from disk |
| `?` | Show the **key legend** (all keys in one window) |
| `q` / `Esc` | Quit (in the narrow layout, `Esc` closes the chart or details panel first) |
| `Ctrl+C` | Quit, from anywhere |

### Mouse

| Action | Effect |
| --- | --- |
| Click a button | Choose that filter, or open the form or list |
| Right-click **3 Month** / **4 Year** | Open the month or year list |
| Click a row | Select it. Clicking the selected row opens the details (narrow layout). |
| Mouse wheel | Scroll the table, or the open list |
| Click outside a popup | Close it without changes |

### Popups

While a popup is open, it receives every key; `q` doesn't quit. The key
legend (`?`) works the same way.

| Popup | Keys |
| --- | --- |
| **Month list** (Shift+3) | `↑↓`/`jk` and `g`/`G` move · `←→`/`hl` change the year · `Enter`/`Space` select · `⌫` **current month** (same as `3`) · `Esc`/`q` close |
| **Year list** (Shift+4) | The current year and the four before it, newest first · `↑↓`/`jk` and `g`/`G` move · `Enter`/`Space` select · `⌫` **current year** (same as `4`) · `Esc`/`q` close |
| **Tag list** (`t`) | "All tags" plus every tag in the database with its entry count · `↑↓`/`jk`, `PgUp`/`PgDn` and `g`/`G` move · `Enter`/`Space` select · `⌫` **remove the tag filter** · `Esc`/`q`/`t` close |
| **Date range** (`c`) | Type into **From**/**To** · `Tab`/`↑↓` or a click switches fields · `⌫` deletes a character · `Ctrl+U` clears the field · `Enter` apply · `Esc` cancel |

Each list opens on the current choice: the month or year being shown, or the
active tag.

## Filters

A view is a **period** combined with an optional **tag**.

| Period | Button label |
| --- | --- |
| Day, Yesterday | `1 Day`, `2 Yesterday` |
| Current month / a picked month | `3 Month` / `3 2025-03 ▾` |
| Current year / a picked year | `4 Year` / `4 2025 ▾` |
| Your own range | `5 Range` / `5 2025-03-01 – 2025-03-31 ▾` |

- The **tag** filter (`6 Tag`, or `6 meetings ▾` once a tag is selected) stays when you change the period.
- Choosing `3` or `4` directly always goes back to the *current* month or
  year.
- The month list always opens on the year being shown, so you can pick a
  year with Shift+4 and then a month with Shift+3.

### What the date range form accepts

| From | To | Result |
| --- | --- | --- |
| `2025-03-01` | `2025-03-31` | The whole of March (**To is inclusive** for plain dates) |
| `2025-03-05 08:30` | `2025-03-05T12:00` | Exact times: a space or `T`, and seconds are optional |
| `2025-03-01` | *(empty)* | From 03-01 with no end (shown as "since 2025-03-01") |
| `:lastyear`, `:week`, … | *(empty)* | Any [range hint](timew-rs.md#range-hints) |

Invalid input shows a red error in the form, which stays open.

## Implementation

Everything is in `timew-tui/src/main.rs`.

### State

All state is in one `App` struct:

- **What is shown:** `filter` (`Day | Yesterday | Month | Year | Custom`),
  `month: Option<(year, month)>`, `year: Option<year>`, `custom_range`, and
  `tag: Option<String>`. `None` means "current".
- **Loaded data:** `tracked: Vec<Tracked>` (the intervals in the range,
  after the tag filter), `summary: Summary` (rows from
  `commands::summary::build`), `all_tags` (tag and count for the whole
  database), and `error`.
- **UI state:** the table selection (`TableState`), `details`, `chart` and `wide`,
  and one `Option<…>` per popup (`picker`, `month_picker`, `year_picker`,
  `range_input`). `Some` means the popup is open.
- **Hit areas for the mouse** (button, table, list and field rectangles),
  recorded during each draw.

`App::load()` works out the range from the filter state, opens the
database, reads `all_tracked()` (for the tag list) and
`tracked_in_range()`, applies the tag filter and rebuilds the summary. Every
filter change calls it.

### Event loop

```
loop:
    terminal.draw(|f| draw(f, &mut app))     // draws, records hit areas, decides wide/narrow
    match event::read():
        Key   → app.on_key(key)
        Mouse → app.on_mouse(mouse)
```

Input goes to **the open popup first**, in this order: tag list, month list,
year list, range form. Only if none is open does it reach the main view.
`Ctrl+C` is handled before all of them.

### Drawing

`draw` splits the screen into header, filter bar, body, status and help.
The body becomes table + details side by side (wide), table over details
(narrow, when opened), or the table alone. Popups are drawn last, centered
over the body, on top of a `Clear` widget.

Layout constants:

| Constant | Meaning |
| --- | --- |
| `PAD` | The space between a frame and its content (`Padding::new(2, 2, 1, 0)`) |
| `DETAILS_MIN_WIDTH` | 44: the minimum width of the side panel |
| `DETAILS_HEIGHT` | The height of the bottom details panel |
| `CHART_MAX_BARS` | 10: the most bars in the bottom chart panel |
| `TAG_COLORS` | The 12 (strong, light) chart color pairs, assigned in alphabetical tag order (`tag_colors`) |
| `WEEK_COLORS` | `[Cyan, Magenta]`, alternating per ISO week (`week_parity`) |
| `ACCENT` | Cyan, for highlights |

### Terminal handling

`ratatui::init()` and `ratatui::restore()` switch to the alternate screen
and raw mode and back. Mouse capture is turned on after init and off before
restore. The program never prints to stdout while the UI is running; errors
are reported on stderr after the terminal has been restored.

### Tests

The tests draw into ratatui's `TestBackend` (usually 100×24, or 140 columns
for the wide layout) and simulate input by calling `on_key` and `on_mouse`.
The sample database has three intervals: yesterday 08–12 *planning*, today
08–10 *meetings* and 10–11 *email*. It is generated relative to the current
date, so the tests pass on any day.

```sh
cargo test -p timew-tui                                   # run
cargo test -p timew-tui -- --nocapture --test-threads=1   # also print the drawn screens
```
