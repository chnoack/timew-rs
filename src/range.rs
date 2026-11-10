//! Range: a half-open time interval `[start, end)`, with `end` possibly
//! unset (an open/ongoing interval that hasn't been stopped yet).
//!
//! This is a clean-room port of the *semantics* of `Range.cpp` from the
//! reference C++ implementation (GothenburgBitFactory/timewarrior, `develop`
//! branch, commit 3699d2e) -- re-derived from its logic, not transcribed.
//! The overlap/containment semantics here are load-bearing: they drive the
//! `:adjust` auto-resolution logic (`validate.cpp::autoAdjust`) that decides
//! whether `timew track`/`modify` accepts, rejects, or rewrites existing
//! intervals. Getting these wrong reproduces exactly the kind of
//! silent-corruption bug we hit earlier with the real binary.
//!
//! Important, non-obvious properties confirmed against the C++ source
//! (NOT guessed):
//!   - An open interval (no end) is treated as unbounded for `overlaps`,
//!     i.e. `other.start >= self.end` is never true if `self` is open
//!     (there is no "clip to now" happening in Range itself -- only
//!     `contains` checks `is_ended()` to decide whether the upper bound
//!     applies at all).
//!   - `is_started`/`is_ended` are `> 0`, not `!= 0` (epochs are never
//!     negative in this domain, so this is equivalent, but documented here
//!     to match the source's stated invariant).
//!   - `startsWithin`/`endsWithin` are a bit surprising: `a.startsWithin(b)`
//!     asks "does an interval description become true where `a`'s START
//!     lands relative to `b`", evaluated as `b.start == a.start ||
//!     b.contains(a.start)`. Note the receiver/argument roles: it's
//!     `other.startsWithin(*this)` inside `encloses`, i.e. `encloses` asks
//!     "does `other`'s start/end fall within self".

/// Seconds since the Unix epoch (UTC). We intentionally avoid pulling in a
/// datetime crate for the core model -- all stored/serialized timestamps are
/// UTC, and formatting only ever needs the fixed `YYYYMMDDTHHMMSSZ` layout
/// (see interval.rs).
pub type Epoch = i64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Range {
    /// Start of the interval. `0` means "unset" (mirrors
    /// `Datetime::toEpoch() == 0` / default-constructed Datetime in the
    /// C++ source).
    pub start: Epoch,
    /// End of the interval, or `0` if still open.
    pub end: Epoch,
}

impl Range {
    pub fn new(start: Epoch, end: Epoch) -> Self {
        Range { start, end }
    }

    /// Sentinel "match everything" range, used by filters that should scan
    /// the whole database (mirrors `IntervalFilterAllInRange({0, 0})` usage
    /// in the C++ source, e.g. in `fillRangeStart`/`fillRangeEnd`).
    pub fn empty() -> Self {
        Range { start: 0, end: 0 }
    }

    pub fn is_started(&self) -> bool {
        self.start > 0
    }

    pub fn is_ended(&self) -> bool {
        self.end > 0
    }

    /// An open interval has a start but no end -- it's still being tracked.
    pub fn is_open(&self) -> bool {
        self.is_started() && !self.is_ended()
    }

    /// `Range::is_empty` in the C++ source is literally `start == end`
    /// (works for the common case of a zero-width closed interval; note
    /// this is ALSO true for a fully-unset default range, matching source
    /// behavior exactly rather than special-casing it).
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// `instant` is contained in `[start, end)`. An unset start means "no
    /// lower bound"; an unset end means "no upper bound" (open interval
    /// extends forever) -- this is NOT the same as clipping to "now".
    pub fn contains(&self, instant: Epoch) -> bool {
        (!self.is_started() || self.start < instant) && (!self.is_ended() || instant < self.end)
    }

    /// Does `self` overlap `other`? Ported from the case analysis in
    /// `Range::overlaps`: both must be started; then reject only if one
    /// range provably ends at/before the other starts.
    pub fn overlaps(&self, other: &Range) -> bool {
        if !self.is_started() || !other.is_started() {
            return false;
        }

        // Other range ends before this range starts.
        if other.is_ended() && other.end <= self.start {
            return false;
        }

        // Other range starts after this range ends.
        if self.is_ended() && other.start >= self.end {
            return false;
        }

        true
    }

    /// Does `other`'s start land within `self`? ("startsWithin" is always
    /// evaluated as `receiver.startsWithin(container)` at call sites, e.g.
    /// `interval.startsWithin(overlap)` in `autoAdjust` asks "did the new
    /// interval's start land inside the existing `overlap` interval".)
    pub fn starts_within(&self, container: &Range) -> bool {
        if container.is_empty() {
            return false;
        }
        container.start == self.start || container.contains(self.start)
    }

    /// Does `other`'s end land within `self`? Only meaningful if `other` is
    /// ended -- an open interval has no end to test. The C++ source doesn't
    /// special-case this explicitly, but `Datetime(0).toEpoch() == 0` would
    /// make `other.end == container.end` potentially true by coincidence
    /// only in the (invalid) 0-epoch case, so we guard it explicitly here
    /// for correctness and clarity.
    pub fn ends_within(&self, container: &Range) -> bool {
        if container.is_empty() {
            return false;
        }
        if !self.is_ended() {
            return false;
        }
        container.end == self.end || container.contains(self.end)
    }

    /// Does `self` fully contain `other`, i.e. `other`'s start AND end both
    /// land within `self`? (`other.startsWithin(self) && other.endsWithin(self)`)
    pub fn encloses(&self, other: &Range) -> bool {
        other.starts_within(self) && other.ends_within(self)
    }

    /// Like `overlaps`, but also true for two started ranges that share
    /// exactly the same start instant even if one or both are zero-width
    /// (e.g. `[p, p)` "intersects" `[p, q)` at the single point `p`, even
    /// though they don't `overlap` in the half-open sense). This is the
    /// predicate used by `IntervalFilterAllInRange` to decide whether a
    /// stored interval matches a query range -- using plain `overlaps`
    /// there would incorrectly exclude a query range of `[p, p)` (used by
    /// some range-hint queries) matching an interval that starts exactly at
    /// `p`.
    pub fn intersects(&self, other: &Range) -> bool {
        if self.overlaps(other) {
            return true;
        }
        self.is_started() && other.is_started() && self.start == other.start
    }

    /// Total duration in seconds. For an open interval this is measured
    /// against `now` (this IS a "clip to now" case, but it only applies to
    /// duration reporting, not to the overlap/containment predicates above).
    pub fn total(&self, now: Epoch) -> i64 {
        if self.is_ended() {
            self.end - self.start
        } else if self.is_started() {
            now - self.start
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_interval_is_open() {
        let r = Range::new(1000, 0);
        assert!(r.is_open());
        assert!(r.is_started());
        assert!(!r.is_ended());
    }

    #[test]
    fn closed_interval_total() {
        let r = Range::new(1000, 2500);
        assert_eq!(r.total(9999), 1500);
    }

    #[test]
    fn open_interval_total_uses_now() {
        let r = Range::new(1000, 0);
        assert_eq!(r.total(1500), 500);
    }

    #[test]
    fn disjoint_closed_ranges_do_not_overlap() {
        let a = Range::new(1000, 2000);
        let b = Range::new(2000, 3000);
        // half-open: touching at the boundary is NOT an overlap
        assert!(!a.overlaps(&b));
        assert!(!b.overlaps(&a));
    }

    #[test]
    fn overlapping_closed_ranges_detected_both_directions() {
        let a = Range::new(1000, 2000);
        let b = Range::new(1500, 2500);
        assert!(a.overlaps(&b));
        assert!(b.overlaps(&a));
    }

    #[test]
    fn open_interval_overlaps_anything_starting_after_it() {
        // An open interval has no upper bound, so a range starting well
        // after it still overlaps -- this matters for `timew start`: you
        // cannot start a new interval "in the past relative to now" while
        // one is already open, without :adjust.
        let open = Range::new(1000, 0);
        let later = Range::new(5000, 6000);
        assert!(open.overlaps(&later));
    }

    #[test]
    fn open_interval_does_not_overlap_something_fully_before_it() {
        let open = Range::new(5000, 0);
        let earlier = Range::new(1000, 2000);
        assert!(!open.overlaps(&earlier));
    }

    #[test]
    fn encloses_exact_match() {
        let a = Range::new(1000, 2000);
        let b = Range::new(1000, 2000);
        assert!(a.encloses(&b));
    }

    #[test]
    fn encloses_strict_subset() {
        let outer = Range::new(1000, 2000);
        let inner = Range::new(1200, 1800);
        assert!(outer.encloses(&inner));
        assert!(!inner.encloses(&outer));
    }

    #[test]
    fn intersects_matches_overlaps_for_normal_ranges() {
        let a = Range::new(1000, 2000);
        let b = Range::new(1500, 2500);
        assert!(a.intersects(&b));
        let c = Range::new(2000, 3000);
        assert!(!a.overlaps(&c));
        assert!(!a.intersects(&c)); // different start, no overlap -> no intersect either
    }

    #[test]
    fn intersects_true_for_shared_start_even_without_overlap() {
        // Zero-width [1000,1000) and [1000,2000) don't `overlap` (half-open
        // semantics), but DO `intersect` at the shared point 1000.
        let zero_width = Range::new(1000, 1000);
        let normal = Range::new(1000, 2000);
        assert!(!zero_width.overlaps(&normal));
        assert!(zero_width.intersects(&normal));
        assert!(normal.intersects(&zero_width));
    }

    #[test]
    fn starts_within_but_not_ends_within() {
        // This is exactly the autoAdjust "trim the tail of the existing
        // interval" case: a new interval starts inside an existing one but
        // extends past its end.
        let existing = Range::new(1000, 2000);
        let incoming = Range::new(1500, 3000);
        assert!(incoming.starts_within(&existing));
        assert!(!incoming.ends_within(&existing));
    }

    #[test]
    fn ends_within_but_not_starts_within() {
        let existing = Range::new(2000, 3000);
        let incoming = Range::new(1000, 2500);
        assert!(!incoming.starts_within(&existing));
        assert!(incoming.ends_within(&existing));
    }

    #[test]
    fn new_interval_enclosing_old_interval_is_neither() {
        // autoAdjust's "new interval encloses old interval -> delete old"
        // branch is reached when start_within_overlap is false AND
        // end_within_overlap is false for an incoming range that fully
        // swallows the existing one.
        let existing = Range::new(1500, 1800);
        let incoming = Range::new(1000, 2000);
        assert!(!incoming.starts_within(&existing));
        assert!(!incoming.ends_within(&existing));
        // and the enclosing direction does hold:
        assert!(incoming.encloses(&existing));
    }

    #[test]
    fn empty_range_detection() {
        let r = Range::new(1000, 1000);
        assert!(r.is_empty());
        let r2 = Range::new(1000, 1001);
        assert!(!r2.is_empty());
    }

    #[test]
    fn starts_within_against_empty_container_is_false() {
        let empty = Range::new(1000, 1000);
        let any = Range::new(1000, 2000);
        assert!(!any.starts_within(&empty));
    }
}
