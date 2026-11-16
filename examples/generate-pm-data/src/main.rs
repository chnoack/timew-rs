//! Generate one year of example Timewarrior data for a project manager.
//!
//! Writes a complete TIMEWARRIORDB root:
//!
//! ```text
//! <out>/timewarrior.cfg
//! <out>/data/YYYY-MM.data
//! <out>/data/tags.data
//! ```
//!
//! Rules:
//! - work days are Monday to Friday
//! - work starts between 07:00 and 09:00 (local time)
//! - work ends between 15:30 and 18:00 (local time)
//! - tracked work time averages 8 h per work day (the lunch break is not
//!   tracked)
//! - every interval carries one of 8 PM topic tags; about 15 % of them get
//!   a second topic tag and a few of those a third (e.g. "budget meetings")
//!
//! The random numbers are those of Python's `random` module (see [`Rng`]),
//! so the same seed gives the same data as the earlier Python version of
//! this generator, and the included data sets can be reproduced.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use jiff::civil::{Date, Weekday};
use jiff::tz::{self, TimeZone};

const USAGE: &str = "\
Usage: cargo run -p generate-pm-data -- [--year 2025] [--out examples/pm-2025]
                                        [--tz Europe/Berlin] [--seed 42] [--multi 0.15]";

/// Topic and relative weight (how much of a typical PM day it takes).
const TOPICS: [(&str, u32); 8] = [
    ("meetings", 22),
    ("email", 14),
    ("planning", 14),
    ("stakeholders", 12),
    ("reporting", 10),
    ("team", 12),
    ("risks", 8),
    ("budget", 8),
];

const STEP: i64 = 5; // minutes; all times are on this grid
const EARLIEST_START: i64 = 7 * 60;
const LATEST_START: i64 = 9 * 60;
const EARLIEST_END: i64 = 15 * 60 + 30;
const LATEST_END: i64 = 18 * 60;

// -- Random numbers -------------------------------------------------------

/// The Mersenne Twister (MT19937) with the methods of Python's
/// `random.Random` that the generator uses, computed exactly as Python
/// does.
struct Rng {
    state: [u32; 624],
    index: usize,
    /// The second number of the last pair `gauss` computed.
    gauss_next: Option<f64>,
}

impl Rng {
    /// Like `random.Random(seed)`: the seed's 32-bit words are the key.
    fn new(seed: u64) -> Rng {
        let mut key = vec![seed as u32];
        if seed >> 32 != 0 {
            key.push((seed >> 32) as u32);
        }
        let mut rng = Rng { state: [0; 624], index: 624, gauss_next: None };
        rng.init_by_array(&key);
        rng
    }

    fn init_genrand(&mut self, s: u32) {
        self.state[0] = s;
        for i in 1..624 {
            let prev = self.state[i - 1];
            self.state[i] = 1_812_433_253u32.wrapping_mul(prev ^ (prev >> 30)).wrapping_add(i as u32);
        }
        self.index = 624;
    }

    fn init_by_array(&mut self, key: &[u32]) {
        self.init_genrand(19_650_218);
        let mt = &mut self.state;
        let (mut i, mut j) = (1usize, 0usize);
        for _ in 0..624.max(key.len()) {
            let prev = mt[i - 1];
            mt[i] = (mt[i] ^ (prev ^ (prev >> 30)).wrapping_mul(1_664_525)).wrapping_add(key[j]).wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i >= 624 {
                mt[0] = mt[623];
                i = 1;
            }
            if j >= key.len() {
                j = 0;
            }
        }
        for _ in 0..623 {
            let prev = mt[i - 1];
            mt[i] = (mt[i] ^ (prev ^ (prev >> 30)).wrapping_mul(1_566_083_941)).wrapping_sub(i as u32);
            i += 1;
            if i >= 624 {
                mt[0] = mt[623];
                i = 1;
            }
        }
        mt[0] = 0x8000_0000;
    }

    fn next_u32(&mut self) -> u32 {
        if self.index >= 624 {
            let mt = &mut self.state;
            for k in 0..624 {
                let y = (mt[k] & 0x8000_0000) | (mt[(k + 1) % 624] & 0x7fff_ffff);
                let mag = if y & 1 == 1 { 0x9908_b0df } else { 0 };
                mt[k] = mt[(k + 397) % 624] ^ (y >> 1) ^ mag;
            }
            self.index = 0;
        }
        let mut y = self.state[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^ (y >> 18)
    }

    /// `random()`: a float in [0, 1) with 53 random bits.
    fn random(&mut self) -> f64 {
        let a = (self.next_u32() >> 5) as f64;
        let b = (self.next_u32() >> 6) as f64;
        (a * 67_108_864.0 + b) / 9_007_199_254_740_992.0
    }

    /// `_randbelow(n)`: an integer in [0, n), by rejection sampling.
    fn below(&mut self, n: u32) -> u32 {
        let bits = 32 - n.leading_zeros();
        loop {
            let r = if bits == 0 { 0 } else { self.next_u32() >> (32 - bits) };
            if r < n {
                return r;
            }
        }
    }

    /// `randrange(start, stop, step)` for a positive step.
    fn randrange(&mut self, start: i64, stop: i64, step: i64) -> i64 {
        let n = (stop - start + step - 1) / step;
        start + step * self.below(n as u32) as i64
    }

    /// `choice(items)`.
    fn choice<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len() as u32) as usize]
    }

    /// `choices(items, weights)[0]`.
    fn weighted<T: Copy>(&mut self, items: &[T], weights: &[u32]) -> T {
        let cumulative: Vec<f64> = weights
            .iter()
            .scan(0u32, |sum, w| {
                *sum += w;
                Some(*sum as f64)
            })
            .collect();
        let x = self.random() * cumulative[cumulative.len() - 1];
        // bisect_right over all but the last entry
        let i = cumulative[..cumulative.len() - 1].partition_point(|c| *c <= x);
        items[i]
    }

    /// `gauss(mu, sigma)`: normally distributed, computed in pairs.
    fn gauss(&mut self, mu: f64, sigma: f64) -> f64 {
        let z = match self.gauss_next.take() {
            Some(z) => z,
            None => {
                let angle = self.random() * std::f64::consts::TAU;
                let radius = (-2.0 * (1.0 - self.random()).ln()).sqrt();
                self.gauss_next = Some(angle.sin() * radius);
                angle.cos() * radius
            }
        };
        mu + z * sigma
    }
}

// -- The schedule ---------------------------------------------------------

/// Start, lunch start, lunch end and end of a work day, in minutes since
/// midnight.
fn plan_day(rng: &mut Rng) -> (i64, i64, i64, i64) {
    loop {
        let start = rng.randrange(EARLIEST_START, LATEST_START + 1, STEP);
        let work = (rng.gauss((8 * 60 - 4) as f64, 35.0) / STEP as f64).round_ties_even() as i64 * STEP;
        let lunch = rng.choice(&[30, 30, 30, 45, 45, 60]);
        let end = start + work + lunch;
        if !(EARLIEST_END..=LATEST_END).contains(&end) {
            continue;
        }
        // lunch somewhere between 11:30 and 13:30, leaving >= 1 h after it
        let lunch_start = rng.randrange(11 * 60 + 30, 13 * 60 + 31, STEP);
        let lunch_end = lunch_start + lunch;
        if lunch_start - start >= 120 && end - lunch_end >= 60 {
            return (start, lunch_start, lunch_end, end);
        }
    }
}

/// Split `[start, end)` into topic chunks of 15 to 120 minutes; two chunks
/// in a row never have the same topic.
fn split_block(rng: &mut Rng, start: i64, end: i64, mut prev: Option<usize>) -> Vec<(i64, i64, usize)> {
    let topics: Vec<usize> = (0..TOPICS.len()).collect();
    let weights: Vec<u32> = TOPICS.iter().map(|(_, w)| *w).collect();
    let mut out = Vec::new();
    let mut t = start;
    while t < end {
        let mut length = rng.choice(&[15, 30, 30, 45, 60, 60, 90, 120]);
        if end - (t + length) < 15 {
            length = end - t; // don't leave a tiny remainder
        }
        let mut topic = prev;
        while topic == prev {
            topic = Some(rng.weighted(&topics, &weights));
        }
        let topic = topic.unwrap();
        out.push((t, t + length, topic));
        prev = Some(topic);
        t += length;
    }
    out
}

/// Additional tags for an interval whose main tag is `topic`: one more
/// with probability `multi`, and of those a third one with probability
/// `multi` again. Never repeats a tag.
fn extra_tags(rng: &mut Rng, topic: usize, multi: f64) -> Vec<usize> {
    let mut extra = Vec::new();
    while extra.len() < 2 && rng.random() < multi {
        let names: Vec<usize> = (0..TOPICS.len()).filter(|t| *t != topic && !extra.contains(t)).collect();
        let weights: Vec<u32> = names.iter().map(|t| TOPICS[*t].1).collect();
        extra.push(rng.weighted(&names, &weights));
    }
    extra
}

// -- Command line ---------------------------------------------------------

struct Options {
    year: i16,
    out: Option<PathBuf>,
    tz: String,
    seed: u64,
    multi: f64,
}

fn parse_args() -> Result<Options, String> {
    let mut o = Options { year: 2025, out: None, tz: "Europe/Berlin".into(), seed: 42, multi: 0.15 };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "-h" || arg == "--help" {
            println!("Generate one year of example Timewarrior data for a project manager.\n\n{USAGE}");
            std::process::exit(0);
        }
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) => (n.to_string(), Some(v.to_string())),
            None => (arg.clone(), None),
        };
        let mut value = || inline.clone().or_else(|| args.next()).ok_or(format!("{name} needs a value"));
        let bad = |v: &str| format!("invalid value for {name}: '{v}'");
        match name.as_str() {
            "--year" => {
                let v = value()?;
                o.year = v.parse().map_err(|_| bad(&v))?;
            }
            "--out" => o.out = Some(PathBuf::from(value()?)),
            "--tz" => o.tz = value()?,
            "--seed" => {
                let v = value()?;
                o.seed = v.parse().map_err(|_| bad(&v))?;
            }
            "--multi" => {
                let v = value()?;
                o.multi = v.parse().map_err(|_| bad(&v))?;
            }
            _ => return Err(format!("unknown argument '{arg}'\n\n{USAGE}")),
        }
    }
    Ok(o)
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("generate-pm-data: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let o = parse_args()?;
    let out = o.out.clone().unwrap_or_else(|| {
        // next to this crate: examples/pm-<year>
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join(format!("pm-{}", o.year))
    });
    let tz: TimeZone = tz::db().get(&o.tz).map_err(|e| format!("unknown time zone '{}': {e}", o.tz))?;
    let mut rng = Rng::new(o.seed);
    // Extra tags come from their own generator, so the schedule (times and
    // main tags) is the same whatever --multi is.
    let mut tag_rng = Rng::new(o.seed + 1);

    let to_utc = |day: Date, minutes: i64| -> Result<jiff::Timestamp, String> {
        let local = day.at((minutes / 60) as i8, (minutes % 60) as i8, 0, 0);
        Ok(local.to_zoned(tz.clone()).map_err(|e| e.to_string())?.timestamp())
    };
    let fmt = |t: jiff::Timestamp| t.strftime("%Y%m%dT%H%M%SZ").to_string();

    let mut months: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut counts = [0u32; TOPICS.len()];
    let mut minutes_per_topic = [0i64; TOPICS.len()];
    let (mut days, mut total, mut multi_tagged) = (0u32, 0i64, 0u32);

    let mut day = Date::new(o.year, 1, 1).map_err(|e| e.to_string())?;
    while day.year() == o.year {
        if !matches!(day.weekday(), Weekday::Saturday | Weekday::Sunday) {
            days += 1;
            let (start, lunch_start, lunch_end, end) = plan_day(&mut rng);
            let mut prev = None;
            for (from, to) in [(start, lunch_start), (lunch_end, end)] {
                for (a, b, topic) in split_block(&mut rng, from, to, prev) {
                    let (s, e) = (to_utc(day, a)?, to_utc(day, b)?);
                    let mut tags: Vec<usize> = vec![topic];
                    tags.extend(extra_tags(&mut tag_rng, topic, o.multi));
                    let mut names: Vec<&str> = tags.iter().map(|t| TOPICS[*t].0).collect();
                    names.sort();
                    months
                        .entry(s.strftime("%Y-%m").to_string())
                        .or_default()
                        .push(format!("inc {} - {} # {}", fmt(s), fmt(e), names.join(" ")));
                    for t in &tags {
                        counts[*t] += 1;
                        minutes_per_topic[*t] += b - a;
                    }
                    multi_tagged += u32::from(tags.len() > 1);
                    total += b - a;
                    prev = Some(topic);
                }
            }
        }
        day = day.tomorrow().map_err(|e| e.to_string())?;
    }

    let data = out.join("data");
    let write = |path: PathBuf, text: String| fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()));
    fs::create_dir_all(&data).map_err(|e| format!("{}: {e}", data.display()))?;
    // What a new Timewarrior database starts with.
    write(out.join("timewarrior.cfg"), "reports.summary.ids = yes\nreports.summary.annotations = yes\n".into())?;
    for (month, lines) in &months {
        write(data.join(format!("{month}.data")), lines.join("\n") + "\n")?;
    }
    let mut by_name: Vec<usize> = (0..TOPICS.len()).collect();
    by_name.sort_by_key(|t| TOPICS[*t].0);
    let body: Vec<String> = by_name.iter().map(|t| format!("  \"{}\":{{\"count\":{}}}", TOPICS[*t].0, counts[*t])).collect();
    write(data.join("tags.data"), format!("{{\n{}\n}}\n", body.join(",\n")))?;

    let intervals: usize = months.values().map(Vec::len).sum();
    let shown = fs::canonicalize(&out).unwrap_or(out);
    println!("wrote {}: {days} work days, {intervals} intervals ({multi_tagged} with several tags)", shown.display());
    println!("average tracked time per work day: {:.2} h", total as f64 / days as f64 / 60.0);
    let mut by_time: Vec<usize> = (0..TOPICS.len()).collect();
    by_time.sort_by(|a, b| minutes_per_topic[*b].cmp(&minutes_per_topic[*a]));
    for t in by_time {
        println!("  {:<13} {:7.1} h  ({} intervals)", TOPICS[t].0, minutes_per_topic[t] as f64 / 60.0, counts[t]);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Values from Python: `r = random.Random(42)`.
    #[test]
    fn same_numbers_as_python() {
        let mut r = Rng::new(42);
        assert_eq!(r.random(), 0.6394267984578837);
        assert_eq!(r.random(), 0.025010755222666936);
        let mut r = Rng::new(42);
        assert_eq!(r.next_u32(), 2_746_317_213);
    }

    #[test]
    fn python_methods() {
        // r = random.Random(7); [r.randrange(420, 541, 5) for _ in range(5)]
        let mut r = Rng::new(7);
        let got: Vec<i64> = (0..5).map(|_| r.randrange(420, 541, 5)).collect();
        assert_eq!(got, PY_RANDRANGE);
        // r = random.Random(7); [r.gauss(476, 35) for _ in range(3)]
        let mut r = Rng::new(7);
        let got: Vec<f64> = (0..3).map(|_| r.gauss(476.0, 35.0)).collect();
        assert_eq!(got, PY_GAUSS);
        // r = random.Random(7); [r.choices(range(8), [22,14,14,12,10,12,8,8])[0] for _ in range(8)]
        let mut r = Rng::new(7);
        let topics: Vec<usize> = (0..8).collect();
        let weights = [22, 14, 14, 12, 10, 12, 8, 8];
        let got: Vec<usize> = (0..8).map(|_| r.weighted(&topics, &weights)).collect();
        assert_eq!(got, PY_CHOICES);
    }

    const PY_RANDRANGE: [i64; 5] = [470, 440, 480, 520, 425];
    const PY_GAUSS: [f64; 3] = [467.04418990433396, 493.90010293807796, 468.08663423259134];
    const PY_CHOICES: [usize; 8] = [1, 0, 4, 0, 3, 2, 0, 3];
}
