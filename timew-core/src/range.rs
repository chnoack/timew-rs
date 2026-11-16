//! Half-open time ranges `[start, end)`.
//!
//! `start == 0` means "not started" and `end == 0` means "not ended": a
//! running interval has a start but no end, and the range `{0, 0}` is
//! unbounded (`:all`).

pub type Epoch = i64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct Range {
    pub start: Epoch,
    pub end: Epoch,
}

impl Range {
    pub fn new(start: Epoch, end: Epoch) -> Self {
        Range { start, end }
    }

    pub fn empty() -> Self {
        Range { start: 0, end: 0 }
    }

    pub fn is_started(&self) -> bool {
        self.start > 0
    }

    pub fn is_ended(&self) -> bool {
        self.end > 0
    }

    pub fn is_open(&self) -> bool {
        self.is_started() && !self.is_ended()
    }

    /// Zero length (`start == end`), which includes `{0, 0}`.
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Neither bound is set: the range covers all time.
    pub fn is_unbounded(&self) -> bool {
        !self.is_started() && !self.is_ended()
    }

    /// `instant` lies strictly after the start and before the end.
    pub fn contains(&self, instant: Epoch) -> bool {
        (!self.is_started() || self.start < instant) && (!self.is_ended() || instant < self.end)
    }

    /// The two ranges share some time. Ranges that only touch don't.
    pub fn overlaps(&self, other: &Range) -> bool {
        if !self.is_started() || !other.is_started() {
            return false;
        }
        if other.is_ended() && other.end <= self.start {
            return false;
        }
        if self.is_ended() && other.start >= self.end {
            return false;
        }
        true
    }

    pub fn starts_within(&self, container: &Range) -> bool {
        if container.is_empty() {
            return false;
        }
        container.start == self.start || container.contains(self.start)
    }

    pub fn ends_within(&self, container: &Range) -> bool {
        if container.is_empty() {
            return false;
        }
        container.end == self.end || container.contains(self.end)
    }

    pub fn encloses(&self, other: &Range) -> bool {
        other.starts_within(self) && other.ends_within(self)
    }

    /// Overlap, or the same start (so that a zero-length range `[p, p)`
    /// intersects a range starting at `p`).
    pub fn intersects(&self, other: &Range) -> bool {
        self.overlaps(other) || (self.is_started() && other.is_started() && self.start == other.start)
    }

    /// The common part: the later start and the earlier end.
    pub fn intersect(&self, other: &Range) -> Range {
        if self.overlaps(other) {
            let start = self.start.max(other.start);
            let end = match (self.is_ended(), other.is_ended()) {
                (true, true) => self.end.min(other.end),
                (true, false) => self.end,
                (false, true) => other.end,
                (false, false) => 0,
            };
            return Range::new(start, end);
        }
        if self.intersects(other) {
            return Range::new(self.start, self.start);
        }
        Range::empty()
    }

    /// The smallest range covering both (open if either is open).
    pub fn combine(&self, other: &Range) -> Range {
        if !self.is_started() || !other.is_started() {
            return *self;
        }
        let start = self.start.min(other.start);
        let end = if self.is_open() || other.is_open() { 0 } else { self.end.max(other.end) };
        Range::new(start, end)
    }

    /// This range with `other` cut out: zero, one or two pieces.
    pub fn subtract(&self, other: &Range) -> Vec<Range> {
        if !self.overlaps(other) {
            return vec![*self];
        }
        let mut pieces = Vec::new();
        if self.start < other.start {
            pieces.push(Range::new(self.start, other.start));
            if other.is_ended() && (!self.is_ended() || self.end > other.end) {
                pieces.push(Range::new(other.end, self.end));
            }
        } else if other.is_ended() && (!self.is_ended() || self.end > other.end) {
            pieces.push(Range::new(other.end, self.end));
        }
        pieces
    }

    /// Length in seconds; an open range counts up to `now`.
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

/// Merge overlapping ranges.
pub fn merge(ranges: &[Range]) -> Vec<Range> {
    let mut sorted = ranges.to_vec();
    sorted.sort_by_key(|r| r.start);
    let mut out: Vec<Range> = Vec::with_capacity(sorted.len());
    for r in sorted {
        match out.last_mut() {
            Some(last) if last.overlaps(&r) => *last = last.combine(&r),
            _ => out.push(r),
        }
    }
    out
}

/// Cut every range in `subtractions` out of every range in `ranges`.
pub fn subtract_all(ranges: &[Range], subtractions: &[Range]) -> Vec<Range> {
    let mut result = ranges.to_vec();
    for s in subtractions {
        result = result.iter().flat_map(|r| r.subtract(s)).collect();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(start: Epoch, end: Epoch) -> Range {
        Range::new(start, end)
    }

    #[test]
    fn overlap_rules() {
        assert!(r(10, 20).overlaps(&r(15, 25)));
        assert!(!r(10, 20).overlaps(&r(20, 30)));
        assert!(r(10, 0).overlaps(&r(20, 30)));
        assert!(!r(10, 20).overlaps(&r(0, 0)));
        assert!(r(10, 10).intersects(&r(10, 20)));
        assert!(!r(10, 10).overlaps(&r(10, 20)));
    }

    #[test]
    fn within_rules() {
        assert!(r(10, 15).starts_within(&r(10, 20)));
        assert!(r(12, 20).ends_within(&r(10, 20)));
        assert!(r(10, 20).encloses(&r(12, 18)));
        assert!(!r(10, 20).encloses(&r(12, 25)));
        assert!(!r(10, 0).ends_within(&r(5, 20)));
    }

    #[test]
    fn intersect_and_subtract() {
        assert_eq!(r(10, 20).intersect(&r(15, 25)), r(15, 20));
        assert_eq!(r(10, 0).intersect(&r(15, 25)), r(15, 25));
        assert_eq!(r(10, 20).subtract(&r(12, 15)), vec![r(10, 12), r(15, 20)]);
        assert_eq!(r(10, 20).subtract(&r(5, 15)), vec![r(15, 20)]);
        assert_eq!(r(10, 20).subtract(&r(5, 25)), vec![]);
        assert_eq!(r(10, 20).subtract(&r(30, 40)), vec![r(10, 20)]);
        assert_eq!(r(10, 0).subtract(&r(12, 15)), vec![r(10, 12), r(15, 0)]);
    }

    #[test]
    fn merging() {
        assert_eq!(merge(&[r(5, 10), r(1, 6), r(20, 30)]), vec![r(1, 10), r(20, 30)]);
        assert_eq!(subtract_all(&[r(1, 100)], &[r(10, 20), r(50, 60)]), vec![r(1, 10), r(20, 50), r(60, 100)]);
    }

    #[test]
    fn totals() {
        assert_eq!(r(10, 25).total(100), 15);
        assert_eq!(r(10, 0).total(100), 90);
        assert_eq!(Range::empty().total(100), 0);
    }
}
