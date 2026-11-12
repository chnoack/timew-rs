# Example data

`examples/generate_pm_data.py` creates one year of made-up data for a
project manager, in a complete Timewarrior database directory. The data is
used for trying out both programs and as realistic test input.

## Included data

| Directory | Year | Working days | Entries | Average per working day |
| --- | --- | --- | --- | --- |
| `examples/pm-2025` | 2025 | 261 | 2,442 | 7.99 h |
| `examples/pm-2026` | 2026 | 261 | 2,442 | 7.99 h |

Both were generated with the default seed. Both years have 261 working days, so they get the same sequence of random numbers and end up with the same totals, but on different dates.

`pm-2026` covers the whole year, **including dates after today**. That is
useful for the TUI's current-month and current-year views, but means the
data contains future entries.

```sh
export TIMEWARRIORDB=$PWD/examples/pm-2025
./target/release/timew-rs summary :lastyear | tail -1     # 2086:35:00
```

## What the data looks like

| Rule | Value |
| --- | --- |
| Working days | Monday to Friday, every week of the year. Public holidays and vacations are **not** left out. |
| Start of work | Between 07:00 and 09:00 local time |
| End of work | Between 15:30 and 18:00 local time |
| Tracked time | Averages 8 h per working day (each day is drawn around 8 h with a spread of ~35 min) |
| Lunch break | 30–60 min, starting between 11:30 and 13:30. It is **not** tracked, so it shows as a gap. |
| Entries | 15–120 min each, on a 5-minute grid. Each has exactly one tag, and two entries in a row never share a tag. |
| Time zone | `Europe/Berlin` by default, stored in UTC as Timewarrior does |

The eight tags, and roughly how much of the time each gets:

| Tag | Share |
| --- | --- |
| `meetings` | 22 % |
| `email` | 14 % |
| `planning` | 14 % |
| `stakeholders` | 12 % |
| `team` | 12 % |
| `reporting` | 10 % |
| `risks` | 8 % |
| `budget` | 8 % |

## Generating data

```sh
python3 examples/generate_pm_data.py                   # 2025 → examples/pm-2025
python3 examples/generate_pm_data.py --year 2026       # → examples/pm-2026
python3 examples/generate_pm_data.py --year 2024 --out /tmp/pm-2024 --tz Europe/Vienna --seed 7
```

| Option | Default | Meaning |
| --- | --- | --- |
| `--year` | `2025` | The calendar year to generate |
| `--out` | `examples/pm-<year>` | The output directory (a database root) |
| `--tz` | `Europe/Berlin` | The time zone the working hours are in |
| `--seed` | `42` | The random seed. The same seed always gives the same data. |

The script needs only Python 3.10+ (it uses `zoneinfo` and `X | None` type hints) and no extra packages.
It writes `timewarrior.cfg` (empty), `data/YYYY-MM.data` and
`data/tags.data`, then prints the number of working days, the number of
entries, the average per day, and the hours per tag.

It **overwrites** the month files in the output directory but doesn't delete
other files there. To regenerate from scratch, delete the directory first.
