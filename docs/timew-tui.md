# timew-tui

A terminal UI for browsing a Timewarrior database. It shows the same rows
as `timew summary`, with filters you can switch with single keys or the
mouse. It can start and stop tracking (`s`), change an entry's start,
end, tags and annotation (`e`) and delete an entry (`d`); nothing else is
ever changed.

```sh
cargo build --release -p timew-tui
./target/release/timew-tui
./target/release/timew-tui --version     # prints the version and exits
```

## Which database it opens

1. `$TIMEWARRIORDB`, if set
2. `~/.timewarrior`, if that directory exists (older Timewarrior layout)
3. `$XDG_DATA_HOME/timewarrior`, which defaults to `~/.local/share/timewarrior`

If the chosen directory has no `data/` subdirectory, it exits with
`No Timewarrior database found at … (missing data/ directory).`

Every load reads the files again, so `r` shows changes made by `timew`
while the TUI is running. The only writes are saved edits (see
[Editing an entry](#editing-an-entry)). The configuration
(`timewarrior.cfg`) is read from the same place `timew-cli` reads it.

## The screen

```
 timew-tui   /path/to/db                                                  ← header
╭───────╮ ╭─────────────╮ ╭─────────╮ ╭────────╮ ╭─────────╮   ╭───────╮
│ 1 Day │ │ 2 Yesterday │ │ 3 Month │ │ 4 Year │ │ 5 Range │   │ 6 Tag │  ← filter buttons
╰───────╯ ╰─────────────╯ ╰─────────╯ ╰────────╯ ╰─────────╯   ╰───────╯
╭ Summary · Month · 2026-11-01 – 2026-11-30 ──────╮╭ Entry details ────────────╮
│  Wk  Date       Day Tags      Start    End  …   ││  ID         @3            │  ← summary + details
│  W46 2026-11-11 Wed planning  8:00 12:00 4:00 … ││  Tags        planning     │
│                                                 │╰───────────────────────────╯
│                                                 │╭ Month · … ───────────────╮
│                                                 ││  planning  ████  4.00 h  │  ← hours per tag
│                                                 ││  Total 7.00 h            │    and totals
│                                                 ││  2 days  ·  3 intervals  │
╰──────────────────────────────────────── row 1/3 ╯╰───────────────────────────╯
 ↑↓ move  1-4 range  ⇧1/⇧3/⇧4 pick  5 range  6 tag  …  q quit              ← help
```

- **Focus:** the view that receives the keys has a **yellow border**: the
  summary table normally, the edit form while you edit an entry. While a
  list, the date form or the legend is open, the table's border is plain.
- **Summary table:** columns Wk, Date, Day, Tags, Start, End, Time and Total.
  ID and Annotation are left out of the table but shown in the details.
  Wk/Date/Day are **cyan and magenta, alternating each ISO week**. Every
  other day has a slightly darker background, and the daily totals are
  yellow.
- **Gaps:** where there is untracked time between two entries of the same
  day, a thin dashed grey line separates them: the row above the gap is
  underlined in segments, from the Tags column to the Total column.
  There is no line between days, before the first or after the last
  entry, or after a running entry. (Terminals that can't color underlines
  draw it in the text color.)
- **Times and durations:** start, end, the per-entry Time and the duration
  in the details are hours and minutes without seconds (`8:50`, `0:30`).
  Seconds are cut off, not rounded.
- **Totals** (the Total column, the status line's total and average) are
  **decimal hours** with two places: 7:30 is shown as `7.50`, 8:25 as
  `8.42`. `timew-cli summary` still shows `H:MM:SS` everywhere, because its
  output matches `timew`.
- **Entries that don't count:** an entry with a tag that starts with
  **exactly one underscore** (`_break`, but not `__x` or `x_y`) is listed
  with its time, drawn in gray, but left out of the day's Total, the
  status line's total and average, and the chart's total and percentages.
  The whole entry is left out, also when it has other tags
  (`_break meetings`). `timew-cli` leaves these entries out of its totals
  the same way.
- **Help line:** always starts with `? help` (the full key legend) and
  ends with `q quit`; in between it shows as many keys as fit the window
  width.
- **Key legend:** `?` opens a window listing every key, mouse action and
  popup key. In a low window it scrolls (`↑↓`, `PgUp`/`PgDn`, `Ctrl+F`/`Ctrl+B`, `g`/`G`, or
  the mouse wheel). `?`, `Esc`, `q`, `Enter` or a click closes it.
- **Totals:** below the bars of the **Hours** view: the total, the number
  of days with entries, the number of rows and the average per day with
  entries. When the Hours view is hidden (narrow window without `b`, or
  nothing to select) or too low for them, a **status line** at the bottom
  shows them instead. Your position in the list (`row 1/3`) is on the
  table's bottom border.
- **Gaps:** when one day is shown (`1`, `2` or Shift+1), the totals also
  show the untracked time between the day's entries, right after the
  total: `Total 7.00 h  ·  Gaps 0.50 h`. Only gaps between two entries
  count (the ones marked with the dashed line), not the time before the
  first or after the last entry.
- **Messages:** the result of the last action ("Saved.", "Deleted. u
  undoes it.", or what went wrong, in red) is shown in the **Messages**
  panel until the next key. The panel is only there while there is a
  message; otherwise the view above it takes its room. In a wide window it
  is at the bottom of the right column, below "Hours"; in a narrow
  window below the list. Long messages wrap (up to three lines).
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

### Starting and stopping

`s` does what fits: **start** a new entry when nothing is running, **stop**
the running one otherwise. The help line shows `s start` or `s stop`.

- **Start:** a dialog asks for the **tags** (separated by spaces,
  `"in quotes"` with spaces) and an **annotation** (any text); both may be
  empty. While you type a tag, a known tag is suggested (see
  [Tag completion](#tag-completion)). `Tab`/`↑↓` switch between the
  fields, `Enter` starts the entry now, `Esc` cancels. Afterwards the view switches to **today** (a tag filter
  the new entry doesn't match is dropped), the new entry is selected and its
  details are open (in a narrow window, the panel below the table).
- **Stop:** the running entry ends now. The view and the filters stay as
  they are, and the stopped entry stays selected. As with `timew-cli stop`,
  configured exclusions inside it (lunch, nights) are cut out.
- While an entry runs, its **details panel has a red border** whenever it
  is the selected one (titled "Entry details · running"), and its times
  count up every second.
- `u` undoes a start or a stop like an edit.
- Starting refuses if the new entry would overlap a later one; stopping
  refuses if the running entry starts in the future.

### Deleting an entry

`d` asks whether to delete the selected entry and shows it: ID, tags,
annotation, start, end, duration, and the days it spans if it crosses
midnight (the whole entry is deleted, not just the row of one day). Only
`y` deletes; `n`, `Esc` or `q` cancel, and `Enter` does nothing, so an
accidental keypress can't delete anything. The view stays as it is, the
next entry is selected, and `u` brings the entry back.

If there is an entry before it, the dialog also offers **`e`: delete it and
extend the previous entry** to its end, so its time goes to the previous
entry (filling a gap between them as well). The dialog shows which entry
that is (it may be outside the shown period, e.g. yesterday's last entry
for the first one of today) and its new end. If the deleted entry is still
running, the previous entry runs on in its place. The extended entry is
selected afterwards, and `u` undoes both changes.

### Editing an entry

`e` turns the details panel of the selected entry into a form for its
tags, annotation, start and end (in a narrow window it opens below the
table):

```
╭ Edit entry ─────────────────────────────────╮
│  ID         @2                              │
│  Tags        meetings                       │
│  Annotation                                 │
│                                             │
│  Start       2026-11-13   08:00             │
│  End         2026-11-13   10:00             │
│  Duration   2:00                            │
│                                             │
│  YYYY-MM-DD  HH:MM                          │
╰────── Enter save · ⇧Enter adjust · Esc cancel ╯
```

- The cursor starts at the beginning of the start date. Typing replaces
  the digits under the cursor, so you can simply type the new value over
  the old one; `Tab` goes to the next field.
- Dates are `YYYY-MM-DD`, times `HH:MM` (or `HH:MM:SS`), in local time. A
  time you don't change keeps its seconds. An empty end date means the day
  the entry starts.
- For the running entry, leave both end fields empty to keep it running.
- **Tags** are a plain text field: tags separated by spaces, a tag with a
  space in double quotes (`"client a" meetings`). Typing inserts at the
  cursor; a long line scrolls sideways. `↑` from the start date or
  `Shift+Tab` gets there; an empty field removes all tags. New tags are
  added to `tags.data` like with `timew-cli tag`. Known tags are suggested
  while you type (see [Tag completion](#tag-completion)).
- **Annotation** is a plain text field too (`↑` from the start date gets
  there). Any text is fine, including quotes; leading and trailing spaces
  are dropped, and an empty field removes the annotation.
- The duration updates while you type.
- `Enter` saves. The change is made the way `timew-cli modify` makes it: an
  end before the start, or an overlap with another entry, is refused with
  a message in the form (which stays open). Otherwise the change is
  written, the entry stays selected, and the Messages panel confirms the
  save.
- **`Shift+Enter`** saves even if the entry now overlaps others, and
  adjusts them like `timew-cli modify … :adjust`: an entry it overlaps at
  one end is shortened, one it covers completely is deleted, one it lies
  inside is split around it. The message says how many were adjusted,
  and an undo reverts all of it at once.
- **`u` undoes** the last saved edit, including any adjusted entries. Only
  the last one: after an undo (or a restart) there is nothing more to
  undo, and a new edit replaces the remembered one. `u undo` appears at
  the start of the help line only while there is an edit to undo. The edit
  is remembered in memory only; nothing like an undo file is written. `u`
  refuses ("Not undone: …") and changes nothing if another program changed
  the entries the edit left behind, or if the restored entry would overlap
  one added since.
  Many terminals send the same code for `Shift+Enter` as for `Enter`;
  timew-tui turns on the extended keyboard protocol where the terminal
  supports it (kitty, WezTerm, foot, Ghostty, …). **`Alt+Enter`** does the
  same and works everywhere.
- `Esc` cancels without changing anything. While the form is open, all
  keys go to it.

### Adding an entry

`a` opens the same form for a **new** entry (titled "New entry"), right
after the selected one: on its date, starting when it ends, 30 minutes
long. If the next entry starts sooner, the end is that entry's start, so
the new one fits into the gap. The cursor starts in the tags field (with
[tag completion](#tag-completion)); start and end can be changed like in
the edit form, but the end can't be left empty.

- `Enter` adds the entry, `Shift+Enter`/`Alt+Enter` adds it and adjusts
  overlapping entries, `Esc` cancels. Overlaps are refused as when
  editing.
- Afterwards the new entry is selected, and `u` removes it again.
- A running entry has no end yet, so nothing can follow it; `a` then shows
  a message instead.

### Tag completion

In the tags field of the start dialog (`s`) and the edit form (`e`), the
rest of a known tag appears in dark gray after the cursor while you type:

```
Tags         meetings pl|anning
```

- `Tab`, `→` or `End` takes the suggestion; typing on ignores it. The
  hint line under the fields names the suggested tag.
- The suggestion is the **most used** tag in the whole database that
  starts with what you typed, **ignoring case**. Taking it replaces what
  you typed with the tag as it is stored: with a tag `ABC`, typing `ab`,
  `aB` or `abc` gives `ABC` (Timewarrior itself treats `ABC` and `abc` as
  different tags, so this keeps you from creating a second spelling). If
  only the case differs, nothing appears after the cursor, but the hint
  line shows the tag.
- If what you typed is a known tag exactly, other spellings of it are not
  suggested, only longer tags. Tags that are already in the field (in any
  spelling) are not suggested again. Characters other than letters must
  match exactly.
- It only appears right after typing a character with the cursor at the
  end of the field. Moving the cursor or switching fields hides it, so
  `Tab` and `→` keep their usual meaning there.
- A tag with a space needs quotes: after `"de`, `"deep work"` is suggested,
  including the closing quote.

### Hours view

A **bar chart of the hours per tag**, where it appears depending on the
window width:

| Window | Chart |
| --- | --- |
| **Wide** | Always shown on the right, **below the details** |
| **Narrow** | Shown below the table after `b`, in place of the details. `b` or `Esc` closes it, and `Enter` switches the panel to the details. |

```
╭ Month · 2025-03-01 – 2025-03-31 ─────────────────────────╮
│  meetings      ███████████████  30.42 h  +7.50 h   18 %  │
│  team          ██████████▌█▋    26.65 h  +5.35 h   16 %  │
│  email         ██████████▏▌     25.65 h  +3.35 h   15 %  │
│  …                                                       │
│                                                          │
│  █ own share  █ shared (+)                               │
│  Total 167.67 h                                          │
│                                                          │
│  21 days  ·  206 intervals  ·  avg/day 7.98 h            │
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
- **`_` tags** (one leading underscore, see above) get a bar with their
  hours (in light gray instead of yellow) but no percentage, as they are
  not part of the total. An entry
  with such a tag counts only for its `_` tags, not for its other tags, so
  the other bars still add up to the total.
- **Legend:** when there is shared time, the Total line explains the two
  shades.
- **Colors:** every bar has a color pair (strong and light) from the
  256-color palette, used for the tag's name and its bar. The bars take the
  12 pairs in order from the top, and the pairs are ordered so that each
  one clearly differs from the one before and after it, so **neighboring
  bars never have the same or a similar color**, also with more than 12
  bars. "(no tag)" is gray. Because the colors follow the bars' order, a
  tag's color can change when the filters change.
- **Space:** in a narrow window, at most 10 tags are shown. In a wide window,
  the chart uses the height left below the details. If that isn't enough
  for every tag, the totals below the bars move to the status line first,
  then the smallest tags are summed up as "… N more tags", and the title
  shows the total number of tags ("· 8 tags"). If the window is too low for
  even one bar, the view is hidden.
- **Title:** the period and the tag filter, like the table's title (without
  a name).
- **Below the bars:** the legend of the two shades (only when there is
  shared time), the total (and the gaps of a day), and on the **last line
  of the view** the number of days and intervals and the average per day.
  Free space in a tall view goes between the total and that last line.

## Keys

### Main view

| Key | Action |
| --- | --- |
| `↑` `↓` / `k` `j` | Move up or down a row |
| `PgUp` `PgDn` / `Space` | Move a page |
| `Ctrl+F` / `Ctrl+B` | Move a page forward / back |
| `Home` `End` / `g` `G` | First or last row |
| `Enter` | Open or close the details (narrow layout) |
| `s` | **Start** a new entry (asks for tags and annotation), or **stop** the running one |
| `e` | **Edit** the start, end, tags and annotation of the selected entry |
| `a` | **Add** an entry right after the selected one (see [Adding an entry](#adding-an-entry)) |
| `d` | **Delete** the selected entry, after asking (`y` deletes) |
| `u` | **Undo** the last saved edit, addition, start, stop or delete (only shown while there is one) |
| `b` | Show or hide the **Hours view** (hours per tag and totals; narrow layout, the wide layout always shows it) |
| `1` | Today |
| `2` | Yesterday |
| `3` / `m` | The current month |
| **Shift+1** (`!`) | The **day of the selected entry** |
| **Shift+3** (`§` on German, `#` on US keyboards) / `M` | **Month list** |
| `4` / `Y` | The current year |
| **Shift+4** (`$`) | **Year list** |
| `←` `→` / `h` `l` / `Tab` | Previous or next of the four fixed periods |
| `5` / `c` | **Date range form** |
| `6` | **Tag list** |
| `r` | Reload from disk |
| `?` | Show the **key legend** (all keys in one window) |
| `i` | Show the **info** dialog: the timew-tui version, the version of the `timew-core` library it uses, the license and copyright, and the project page on GitHub. Any key or a click closes it. |
| `Esc` | **Reset all filters**: back to the current month with all tags (a picked month or year and a date range are dropped). In the narrow layout it closes the chart or details panel first. It never quits. |
| `q` | Quit |
| `Ctrl+C` | Quit, from anywhere |

### Mouse

| Action | Effect |
| --- | --- |
| Click a button | Choose that filter, or open the form or list |
| Right-click **3 Month** / **4 Year** | Open the month or year list |
| Click a row | Select it. Clicking the selected row opens the details (narrow layout). |
| Mouse wheel | Scroll the table, or the open list |
| Click outside a popup | Close it without changes |
| Click a field of the edit form | Type in that field |

### Popups

While a popup is open, it receives every key; `q` doesn't quit. The key
legend (`?`) works the same way.

| Popup | Keys |
| --- | --- |
| **Month list** (Shift+3) | `↑↓`/`jk` and `g`/`G` move · `←→`/`hl` change the year · `Enter`/`Space` select · `⌫` **current month** (same as `3`) · `Esc`/`q` close |
| **Year list** (Shift+4) | The current year and the four before it, newest first · `↑↓`/`jk` and `g`/`G` move · `Enter`/`Space` select · `⌫` **current year** (same as `4`) · `Esc`/`q` close |
| **Tag list** (`6`) | "All tags" plus every tag in the database with its entry count · `↑↓`/`jk`, `PgUp`/`PgDn` and `g`/`G` move · `Enter`/`Space` select · `⌫` **remove the tag filter** · `Esc`/`q`/`6` close |
| **Date range** (`c`) | Type into **From**/**To** · `Tab`/`↑↓` or a click switches fields · `⌫` deletes a character · `Ctrl+U` clears the field · `Enter` apply · `Esc` cancel |
| **Delete** (`d`) | Shows the entry (ID, tags, annotation, start, end, duration, and the days it spans) · `y` deletes · `e` deletes and extends the previous entry to its end (only offered if there is one; the dialog shows which) · `n`/`Esc`/`q` cancel · `Enter`, other keys and clicks do nothing |
| **Start dialog** (`s`) | Type the tags and the annotation · `Tab`/`↑↓` switch fields · `←→`, `Home`/`End` move the cursor · `⌫`/`Del` delete · `Ctrl+U` clears the field · a click places the cursor · `Enter` start · `Esc` cancel |
| **Edit form** (`e`) | `Tab`/`↓` next field, `Shift+Tab`/`↑` previous (start date, start time, end date, end time, tags, annotation) · in dates and times typing **overwrites** the character under the cursor and steps over `-` and `:` (type `0930` over `08:00` to get `09:30`); in the tags and the annotation it inserts · `←→`, `Home`/`End` move the cursor (`←→` continue into the next field at the ends) · `⌫`/`Del` delete · `Ctrl+U` clears the field · a click places the cursor · `Enter` save · `Shift+Enter`/`Alt+Enter` save and adjust overlapping entries · `Esc` cancel |

Each list opens on the current choice: the month or year being shown, or the
active tag.

## Filters

A view is a **period** combined with an optional **tag**.

| Period | Button label |
| --- | --- |
| Today / the day of the selected entry | `1 Day` / `1 2026-03-04 ▾` |
| Yesterday | `2 Yesterday` |
| Current month / a picked month | `3 Month` / `3 2025-03 ▾` |
| Current year / a picked year | `4 Year` / `4 2025 ▾` |
| Your own range | `5 Range` / `5 2025-03-01 – 2025-03-31 ▾` |

- The **tag** filter (`6 Tag`, or `6 meetings ▾` once a tag is selected) stays when you change the period.
- Choosing `1`, `3` or `4` directly always goes back to *today* or the
  *current* month or year.
- **Shift+1** (`!`) shows the **day of the selected entry** (the day it is
  listed under), with the entry still selected, for example to look at one
  day of a month in detail.
- The month list always opens on the year being shown, so you can pick a
  year with Shift+4 and then a month with Shift+3.

### What the date range form accepts

| From | To | Result |
| --- | --- | --- |
| `2025-03-01` | `2025-03-31` | The whole of March (**To is inclusive** for plain dates) |
| `2025-03-05 08:30` | `2025-03-05T12:00` | Exact times: a space or `T`, and seconds are optional |
| `2025-03-01` | *(empty)* | From 03-01 with no end (shown as "since 2025-03-01") |
| `:lastyear`, `:week`, … | *(empty)* | Any [range hint](timew-cli.md#range-hints) |

Invalid input shows a red error in the form, which stays open.

## Implementation

The code is in `timew-tui/src/`, split by responsibility:

| File | Contents |
| --- | --- |
| `main.rs` | Finding the database, terminal setup, the event loop, `--version` |
| `app.rs` | The `App` state: loading, the selection, filters, pickers, scrolling |
| `input.rs` | Keyboard and mouse: what each key and click does, per dialog |
| `entries.rs` | Changing data: edit, start, stop, delete, undo (through `timew-core`) |
| `filter.rs` | The filters, the date range form, the month and year lists |
| `forms.rs` | The edit form, the start form and the delete dialog, and parsing what is typed |
| `format.rs` | Dates, times, durations and ranges for display |
| `ui/mod.rs` | `draw`: the layout, header, filter bar, shared colors and paddings |
| `ui/table.rs` | The summary table: columns, week colors, gap lines |
| `ui/details.rs` | The details panel and the edit form in it |
| `ui/chart.rs` | The Hours view: hours per tag and the totals |
| `ui/popups.rs` | Lists, forms, the delete dialog, the info dialog and the key legend |
| `ui/footer.rs` | The messages panel, the status line (when the Hours view can't show the totals) and the help line |
| `tests/` | The tests, by topic (`filters`, `layout`, `chart`, `help`, `edit`, `tracking`), with shared helpers in `tests/mod.rs`; `timew-tui/tests/version.rs` runs the binary |

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
  `range_input`, and `edit` for the edit form). `Some` means it is open.
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

Input goes to **the edit form or the open popup first**, in this order: edit
form, key legend, tag list, month list, year list, range form. Only if none
is open does it reach the main view. `Ctrl+C` is handled before all of
them.

Saving an edit calls `timew_core::modify_interval(paths, original, edited, adjust)`.
It opens the configuration and the database like `timew-cli` does, checks
that the entry is still stored unchanged (otherwise it asks for a reload
with `r`), deletes it, checks the edited one for overlaps with
`validate::auto_adjust`, adds it and commits. If anything fails, nothing
is written. It returns every change it made (`session::Change`: an
interval before and after), which the TUI keeps in `App::last_edit`; `u`
passes them to `timew_core::undo_change`, which reverts them, last first,
only if the intervals they left behind are still stored unchanged and the
restored ones overlap nothing.

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
| `TAG_COLORS` | The 12 (strong, light) chart color pairs, given to the bars in order (`bar_colors`); neighbors in the list contrast |
| `WEEK_COLORS` | `[Cyan, Magenta]`, alternating per ISO week (`week_parity`) |
| `ACCENT` | Cyan, for highlights |
| `FOCUS` | Yellow, the border of the view with the keyboard focus (`App::table_has_focus`) |

### Terminal handling

`ratatui::init()` and `ratatui::restore()` switch to the alternate screen
and raw mode and back. Mouse capture is turned on after init and off before
restore. The program never prints to stdout while the UI is running; errors
are reported on stderr after the terminal has been restored.

### Tests

The tests (in `src/tests/`) draw into ratatui's `TestBackend` (usually 100×24, or 140 columns
for the wide layout) and simulate input by calling `on_key` and `on_mouse`.
The sample database has three intervals: yesterday 08–12 *planning*, today
08–10 *meetings* and 10–11 *email*. It is generated relative to the current
date, so the tests pass on any day.

```sh
cargo test -p timew-tui                                   # run
cargo test -p timew-tui -- --nocapture --test-threads=1   # also print the drawn screens
```
