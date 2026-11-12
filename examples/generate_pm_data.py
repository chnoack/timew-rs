#!/usr/bin/env python3
"""Generate one year of example Timewarrior data for a project manager.

Writes a complete TIMEWARRIORDB root:

    <out>/timewarrior.cfg
    <out>/data/YYYY-MM.data
    <out>/data/tags.data

Rules:
  * work days are Monday..Friday
  * work starts between 07:00 and 09:00 (local time)
  * work ends between 15:30 and 18:00 (local time)
  * tracked work time averages 8h per work day (lunch break is untracked)
  * every interval carries exactly one of 8 PM topic tags

Usage:
    python3 examples/generate_pm_data.py [--year 2025] [--out examples/pm-2025]
                                         [--tz Europe/Berlin] [--seed 42]
"""

import argparse
import json
import random
from datetime import date, datetime, time, timedelta, timezone
from pathlib import Path
from zoneinfo import ZoneInfo

# topic -> relative weight (how much of a typical PM day it takes)
TOPICS = {
    "meetings": 22,
    "email": 14,
    "planning": 14,
    "stakeholders": 12,
    "reporting": 10,
    "team": 12,
    "risks": 8,
    "budget": 8,
}

STEP = 5  # minutes; all times are rounded to this grid
EARLIEST_START = 7 * 60
LATEST_START = 9 * 60
EARLIEST_END = 15 * 60 + 30
LATEST_END = 18 * 60


def plan_day(rng: random.Random) -> tuple[int, int, int, int]:
    """Return (start, lunch_start, lunch_end, end) in minutes since midnight."""
    while True:
        start = rng.randrange(EARLIEST_START, LATEST_START + 1, STEP)
        work = round(rng.gauss(8 * 60 - 4, 35) / STEP) * STEP
        lunch = rng.choice([30, 30, 30, 45, 45, 60])
        end = start + work + lunch
        if not (EARLIEST_END <= end <= LATEST_END):
            continue
        # lunch somewhere between 11:30 and 13:30, leaving >= 1h after it
        lunch_start = rng.randrange(11 * 60 + 30, 13 * 60 + 31, STEP)
        lunch_end = lunch_start + lunch
        if lunch_start - start >= 120 and end - lunch_end >= 60:
            return start, lunch_start, lunch_end, end


def split_block(rng: random.Random, start: int, end: int, prev: str | None):
    """Split [start, end) into topic chunks of 15..120 minutes."""
    names, weights = list(TOPICS), list(TOPICS.values())
    t = start
    while t < end:
        length = rng.choice([15, 30, 30, 45, 60, 60, 90, 120])
        if end - (t + length) < 15:  # don't leave a tiny remainder
            length = end - t
        topic = prev
        while topic == prev:
            topic = rng.choices(names, weights)[0]
        yield t, t + length, topic
        prev = topic
        t += length


def to_utc(day: date, minutes: int, tz: ZoneInfo) -> datetime:
    local = datetime.combine(day, time(minutes // 60, minutes % 60), tzinfo=tz)
    return local.astimezone(timezone.utc)


def fmt(dt: datetime) -> str:
    return dt.strftime("%Y%m%dT%H%M%SZ")


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--year", type=int, default=2025)
    ap.add_argument("--out", type=Path, default=None)
    ap.add_argument("--tz", default="Europe/Berlin")
    ap.add_argument("--seed", type=int, default=42)
    args = ap.parse_args()

    out = args.out or Path(__file__).parent / f"pm-{args.year}"
    tz = ZoneInfo(args.tz)
    rng = random.Random(args.seed)

    months: dict[str, list[str]] = {}
    counts = {t: 0 for t in TOPICS}
    minutes_per_topic = {t: 0 for t in TOPICS}
    days = 0
    total = 0

    day = date(args.year, 1, 1)
    while day.year == args.year:
        if day.weekday() < 5:
            days += 1
            start, l0, l1, end = plan_day(rng)
            prev = None
            for block in ((start, l0), (l1, end)):
                for a, b, topic in split_block(rng, *block, prev):
                    s, e = to_utc(day, a, tz), to_utc(day, b, tz)
                    months.setdefault(s.strftime("%Y-%m"), []).append(
                        f"inc {fmt(s)} - {fmt(e)} # {topic}"
                    )
                    counts[topic] += 1
                    minutes_per_topic[topic] += b - a
                    total += b - a
                    prev = topic
        day += timedelta(days=1)

    data = out / "data"
    data.mkdir(parents=True, exist_ok=True)
    (out / "timewarrior.cfg").touch()
    for ym, lines in sorted(months.items()):
        (data / f"{ym}.data").write_text("\n".join(lines) + "\n")
    body = ",\n".join(f'  {json.dumps(t)}:{{"count":{c}}}' for t, c in sorted(counts.items()))
    (data / "tags.data").write_text("{\n" + body + "\n}\n")

    print(f"wrote {out}: {days} work days, {sum(counts.values())} intervals")
    print(f"average tracked time per work day: {total / days / 60:.2f} h")
    for t in sorted(TOPICS, key=minutes_per_topic.get, reverse=True):
        print(f"  {t:<13} {minutes_per_topic[t] / 60:7.1f} h  ({counts[t]} intervals)")


if __name__ == "__main__":
    main()
