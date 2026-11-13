# Example data

`examples/generate_pm_data.py` creates one year of made-up data for a
project manager, in a complete Timewarrior database directory. The data is
used for trying out both programs and as realistic test input.

## Included data

| Directory | Year | Working days | Entries | With several tags | Average per working day |
| --- | --- | --- | --- | --- | --- |
| `examples/pm-2025` | 2025 | 261 | 2,442 | 342 (54 with three) | 7.99 h |
| `examples/pm-2026` | 2026 | 261 | 2,442 | 342 (54 with three) | 7.99 h |

Both were generated with the default seed. Both years have 261 working days, so they get the same sequence of random numbers and end up with the same totals, but on different dates.

`pm-2026` covers the whole year, **including dates after today**. That is
useful for the TUI's current-month and current-year views, but means the
data contains future entries.

```sh
export TIMEWARRIORDB=$PWD/examples/pm-2025
./target/release/timew-cli summary :lastyear | tail -2     # 2086:35:00
```

## What the data looks like

| Rule | Value |
| --- | --- |
| Working days | Monday to Friday, every week of the year. Public holidays and vacations are **not** left out. |
| Start of work | Between 07:00 and 09:00 local time |
| End of work | Between 15:30 and 18:00 local time |
| Tracked time | Averages 8 h per working day (each day is drawn around 8 h with a spread of ~35 min) |
| Lunch break | 30–60 min, starting between 11:30 and 13:30. It is **not** tracked, so it shows as a gap. |
| Entries | 15–120 min each, on a 5-minute grid. Two entries in a row never share their main tag. |
| Tags per entry | One main tag. About 15 % of the entries get a second tag, and about 15 % of those a third, for example `budget meetings` or `meetings planning stakeholders`. The extra tags are drawn with the same weights as the main tags. |
| Time zone | `Europe/Berlin` by default, stored in UTC as Timewarrior does |

The eight tags, their weight for the main tag, and the hours they end up
with in `pm-2025` (an entry with several tags counts for each of them, so
the hours add up to more than the 2,086.58 h tracked in total):

| Tag | Weight | Hours in 2025 | Entries |
| --- | --- | --- | --- |
| `meetings` | 22 % | 478.8 | 583 |
| `planning` | 14 % | 353.9 | 405 |
| `email` | 14 % | 316.4 | 374 |
| `team` | 12 % | 314.3 | 358 |
| `stakeholders` | 12 % | 302.6 | 353 |
| `reporting` | 10 % | 247.1 | 293 |
| `budget` | 8 % | 205.1 | 234 |
| `risks` | 8 % | 202.6 | 238 |

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
| `--multi` | `0.15` | The chance of an extra tag per entry. `0` gives one tag per entry. The extra tags use their own random sequence, so changing this keeps the times and main tags the same. |

The script needs only Python 3.10+ (it uses `zoneinfo` and `X | None` type hints) and no extra packages.
It writes `timewarrior.cfg` (with the two settings a new Timewarrior
database starts with, which turn on the ID and annotation columns of
`summary`), `data/YYYY-MM.data` and
`data/tags.data`, then prints the number of working days, the number of
entries (and how many have several tags), the average per day, and the hours
per tag.

It **overwrites** the month files in the output directory but doesn't delete
other files there. To regenerate from scratch, delete the directory first.
