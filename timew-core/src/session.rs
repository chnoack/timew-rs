//! Working on the data: the configuration and the database together, plus
//! the feedback text collected so far. The command line program wraps this
//! with the parsed arguments; `timew-tui` uses it through `edit`.
//!
//! All changes to intervals go through [`Session::add`],
//! [`Session::delete`] and [`Session::modify`], which record them in
//! [`Session::changes`] and print the "new tag" notes.
//!
//! # Exclusions and synthetic intervals
//!
//! When exclusions are configured (nights, lunch, weekends, holidays), a
//! running interval that spans excluded time is presented as several
//! pieces with the excluded time cut out. Only the running interval is
//! stored; the pieces are *synthetic* and get the lowest IDs. Commands
//! that change intervals first make the pieces real
//! ([`Session::flatten_latest`]).

use std::collections::BTreeSet;

use crate::database::{Database, Tracked};
use crate::exclusion;
use crate::interval::Interval;
use crate::paths::Paths;
use crate::range::{subtract_all, Epoch, Range};
use crate::rules::Rules;
use crate::text::quote_if_needed;

/// One change to the stored intervals: `before` was replaced by `after`.
/// An empty `before` means the interval was added, an empty `after` that it
/// was deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub before: Interval,
    pub after: Interval,
}

/// Text shown for an interval that was recorded or is being tracked.
pub type Feedback = fn(&Rules, &Interval, Epoch) -> String;

pub struct Session {
    pub rules: Rules,
    pub db: Database,
    /// Every change made to the stored intervals in this run, in order.
    pub changes: Vec<Change>,
    pub paths: Paths,
    /// Feedback for the user (standard output in the command line
    /// program), written when the work is done.
    pub out: String,
    /// The current time, fixed for the whole run.
    pub now: Epoch,
    /// How to describe an interval that a change records, such as a running
    /// one stopped by a new one (shown when `verbose` is on). By default
    /// nothing is shown.
    pub feedback: Feedback,
}

fn no_feedback(_: &Rules, _: &Interval, _: Epoch) -> String {
    String::new()
}

impl Session {
    pub fn new(rules: Rules, db: Database, paths: Paths, now: Epoch) -> Self {
        Session { rules, db, changes: Vec::new(), paths, out: String::new(), now, feedback: no_feedback }
    }

    pub fn verbose(&self) -> bool {
        self.rules.get_bool("verbose", true)
    }

    pub fn color(&self) -> bool {
        self.rules.get_bool("color", false)
    }

    // -- Changes --------------------------------------------------------

    pub fn add(&mut self, interval: &Interval) -> Result<(), String> {
        let new_tags = self.db.add_interval(interval)?;
        if self.verbose() {
            for tag in new_tags {
                self.out.push_str(&format!("Note: '{}' is a new tag.\n", quote_if_needed(&tag)));
            }
        }
        self.changes.push(Change { before: Interval::default(), after: interval.clone() });
        Ok(())
    }

    pub fn delete(&mut self, interval: &Interval) -> Result<(), String> {
        self.db.delete_interval(interval)?;
        self.changes.push(Change { before: interval.clone(), after: Interval::default() });
        Ok(())
    }

    pub fn modify(&mut self, from: &Interval, to: &Interval) -> Result<(), String> {
        if !from.is_empty() {
            self.delete(from)?;
        }
        if !to.is_empty() {
            self.add(to)?;
        }
        Ok(())
    }

    // -- Reading --------------------------------------------------------

    pub fn exclusions(&self, range: &Range) -> Result<Vec<Range>, String> {
        exclusion::all_exclusions(&self.rules, range)
    }

    /// The most recent stored interval, as stored.
    pub fn latest(&mut self) -> Result<Option<Interval>, String> {
        self.db.latest()
    }

    /// All intervals, newest first, with the running interval split at
    /// exclusions into synthetic pieces.
    pub fn tracked(&mut self) -> Result<Vec<Tracked>, String> {
        let intervals = self.db.intervals()?.to_vec();
        let mut out = Vec::with_capacity(intervals.len() + 4);
        let mut iter = intervals.into_iter();
        if let Some(latest) = iter.next() {
            let pieces = self.expand(&latest)?;
            let synthetic = pieces.len() > 1;
            for interval in pieces {
                out.push(Tracked { id: out.len() as u32 + 1, interval, synthetic });
            }
        }
        for interval in iter {
            out.push(Tracked { id: out.len() as u32 + 1, interval, synthetic: false });
        }
        Ok(out)
    }

    /// The most recent interval (or piece of it), with ID 1.
    pub fn first(&mut self) -> Result<Option<Tracked>, String> {
        Ok(self.tracked()?.into_iter().next())
    }

    /// The running interval (or its current piece), if any.
    pub fn active(&mut self) -> Result<Option<Tracked>, String> {
        Ok(self.first()?.filter(|t| t.interval.is_open()))
    }

    /// Intervals intersecting `range` (all for an unbounded range) that
    /// have all of `tags`, oldest first.
    pub fn tracked_in(&mut self, range: &Range, tags: &[String]) -> Result<Vec<Tracked>, String> {
        let mut out: Vec<Tracked> = self
            .tracked()?
            .into_iter()
            .filter(|t| range.is_unbounded() || t.interval.range.intersects(range))
            .filter(|t| tags.iter().all(|tag| t.interval.has_tag(tag)))
            .collect();
        out.reverse();
        Ok(out)
    }

    /// The intervals with the given IDs, oldest first. An unknown ID is an
    /// error.
    pub fn tracked_ids(&mut self, ids: &BTreeSet<u32>) -> Result<Vec<Tracked>, String> {
        let mut out: Vec<Tracked> = self.tracked()?.into_iter().filter(|t| ids.contains(&t.id)).collect();
        if let Some(missing) = ids.iter().find(|id| !out.iter().any(|t| t.id == **id)) {
            return Err(format!("ID '@{missing}' does not correspond to any tracking."));
        }
        out.reverse();
        Ok(out)
    }

    /// The running interval split at exclusions, newest piece first (just
    /// the interval itself if nothing is excluded).
    fn expand(&self, latest: &Interval) -> Result<Vec<Interval>, String> {
        if latest.is_open() && latest.range.start < self.now {
            let exclusions = self.exclusions(&Range::new(latest.range.start, self.now))?;
            if !exclusions.is_empty() {
                let mut pieces = flatten(latest, &exclusions, self.now);
                if pieces.len() > 1 {
                    pieces.reverse();
                    return Ok(pieces);
                }
            }
        }
        Ok(vec![latest.clone()])
    }

    /// Store the synthetic pieces of the running interval as real
    /// intervals.
    pub fn flatten_latest(&mut self) -> Result<(), String> {
        let Some(latest) = self.latest()? else { return Ok(()) };
        let pieces = self.expand(&latest)?;
        if pieces.len() > 1 {
            self.delete(&latest)?;
            for piece in pieces.iter().rev() {
                self.add(piece)?;
            }
        }
        Ok(())
    }

    /// Time in `range` that is neither excluded nor tracked (with `tags`).
    pub fn untracked(&mut self, range: &Range, tags: &[String]) -> Result<Vec<Range>, String> {
        let tracked: Vec<Range> = self
            .db
            .intervals()?
            .iter()
            .filter(|i| range.is_unbounded() || i.range.intersects(range))
            .filter(|i| tags.iter().all(|t| i.has_tag(t)))
            .map(|i| i.range)
            .collect();
        let available = subtract_all(&[*range], &self.exclusions(range)?);
        Ok(subtract_all(&available, &tracked))
    }
}

/// Cut the exclusions that lie completely inside `interval` out of it.
/// For a running interval, pieces that would start in the future are
/// dropped.
pub fn flatten(interval: &Interval, exclusions: &[Range], now: Epoch) -> Vec<Interval> {
    let enclosed: Vec<Range> = exclusions.iter().filter(|e| interval.range.encloses(e)).copied().collect();
    let mut out = Vec::new();
    for range in subtract_all(&[interval.range], &enclosed) {
        if interval.is_open() && range.start > now {
            break;
        }
        let mut piece = interval.clone();
        piece.range = range;
        out.push(piece);
    }
    out
}
