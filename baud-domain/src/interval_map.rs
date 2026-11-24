//! Runs of lines, each with one value, kept sorted, disjoint and coalesced:
//! a value goes over a run of lines by a binary search and one splice, the
//! lines a value does not hold within a range are found by a binary search
//! and a walk of the runs that meet it, and a move of the runs resolves
//! every line two values both land on to no value.

#[cfg(feature = "alloc")]
use alloc::vec::Vec;

use crate::LineIdx;
#[cfg(feature = "alloc")]
use crate::WalkSteps;

/// **A line that knows its neighbours**: the line after it and the line
/// before it, `None` past either end of the lines it numbers, never a
/// line clamped to an end.
pub trait LineStep: Copy + Ord {
    /// The line after this one; `None` after the last.
    fn next(self) -> Option<Self>;
    /// The line before this one; `None` before the first.
    fn prev(self) -> Option<Self>;
}

impl LineStep for LineIdx {
    fn next(self) -> Option<LineIdx> {
        self.0.checked_add(1).map(LineIdx)
    }
    fn prev(self) -> Option<LineIdx> {
        self.0.checked_sub(1).map(LineIdx)
    }
}

/// **Lines from `first` to `last`, both held, never none**: made only with
/// its ends in order, so no interval is empty and none runs backwards. The
/// fields are private.
///
/// ```compile_fail,E0451
/// fn f(k: baud_domain::LineIdx) { let _ = baud_domain::LineInterval { first: k, last: k }; }
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::{LineIdx, LineInterval};
/// let i = LineInterval::new(LineIdx::of(5), LineIdx::of(2));
/// assert_eq!((i.first(), i.last()), (LineIdx::of(2), LineIdx::of(5)));
/// assert_eq!(LineInterval::one(LineIdx::of(3)).last(), LineIdx::of(3));
/// ```
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct LineInterval<K: LineStep> {
    first: K,
    last: K,
}

impl<K: LineStep> LineInterval<K> {
    /// From `a` to `b`, whichever way round they come.
    pub fn new(a: K, b: K) -> LineInterval<K> {
        LineInterval {
            first: a.min(b),
            last: a.max(b),
        }
    }

    /// The one line `k`.
    pub fn one(k: K) -> LineInterval<K> {
        LineInterval { first: k, last: k }
    }

    /// The first line.
    pub fn first(self) -> K {
        self.first
    }

    /// The last line, never before the first.
    pub fn last(self) -> K {
        self.last
    }
}

/// **What a [`IntervalMap::remap`] gives a line two values' moved runs both
/// cover**: `Neither`, so the line takes no value. A line whose value
/// cannot be told apart is given none, never one of them and never both;
/// each run of such lines is counted in the [`OverlapRuns`] `remap`
/// returns. There is no variant carrying a value.
///
/// ```compile_fail,E0599
/// let _ = baud_domain::RemapOverlap::Value(1u8);
/// ```
///
/// The legal path:
///
/// ```
/// let _ = baud_domain::RemapOverlap::Neither;
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RemapOverlap {
    Neither,
}

/// **The runs of lines a [`IntervalMap::remap`] left with no value**
/// under [`RemapOverlap::Neither`]: each maximal run of lines that two
/// values' moved runs both covered, counted once. Made only by `remap`:
/// the field is private.
///
/// ```compile_fail,E0423
/// let _ = baud_domain::OverlapRuns(0);
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::{IntervalMap, LineIdx, RemapOverlap, WalkSteps};
/// let mut m = IntervalMap::<LineIdx, u8>::new();
/// let runs = m.remap(|i| i, RemapOverlap::Neither, &mut WalkSteps::ZERO);
/// assert_eq!(runs.get(), 0);
/// ```
#[must_use]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct OverlapRuns(u64);

impl OverlapRuns {
    /// The runs counted.
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// **Runs of lines, each with one value, sorted, disjoint and
/// coalesced**: no two runs share a line, and two runs that touch -- one
/// begins on the line after the other ends -- hold different values. `V:
/// Eq` is what makes two touching runs one; a value with no equality, an
/// `f32`, cannot be held.
///
/// Nothing writes the runs but the methods that keep that: the field is
/// private, and there is no `push`, `IndexMut` or `DerefMut`. Every method
/// that walks the runs counts each run it compares or visits in a
/// [`WalkSteps`].
///
/// ```compile_fail,E0616
/// fn f(m: &baud_domain::IntervalMap<baud_domain::LineIdx, u8>) { let _ = &m.items; }
/// ```
///
/// ```compile_fail,E0599
/// fn f(m: &mut baud_domain::IntervalMap<baud_domain::LineIdx, u8>, i: baud_domain::LineInterval<baud_domain::LineIdx>) { m.push((i, 1)); }
/// ```
///
/// ```compile_fail,E0599
/// fn f(i: baud_domain::LineInterval<baud_domain::LineIdx>, s: &mut baud_domain::WalkSteps) { baud_domain::IntervalMap::<baud_domain::LineIdx, f32>::new().assign(i, 1.0, s); }
/// ```
///
/// A move of the runs names what a line two values land on takes:
///
/// ```compile_fail,E0061
/// fn f(m: &mut baud_domain::IntervalMap<baud_domain::LineIdx, u8>, s: &mut baud_domain::WalkSteps) { let _ = m.remap(|i| i, s); }
/// ```
///
/// The legal path:
///
/// ```
/// use baud_domain::{IntervalMap, LineIdx, LineInterval, RemapOverlap, WalkSteps};
/// let at = |a, b| LineInterval::new(LineIdx::of(a), LineIdx::of(b));
/// let mut m = IntervalMap::<LineIdx, u8>::new();
/// let mut s = WalkSteps::ZERO;
/// m.assign(at(0, 3), 1, &mut s);
/// m.assign(at(4, 5), 1, &mut s);
/// m.assign(at(2, 2), 2, &mut s);
/// let runs: Vec<_> = m.runs().collect();
/// assert_eq!(runs, [(at(0, 1), 1), (at(2, 2), 2), (at(3, 5), 1)]);
/// let mut gaps = Vec::new();
/// m.gaps_of(1, at(0, 7), &mut gaps, &mut s);
/// assert_eq!(gaps, [at(2, 2), at(6, 7)]);
/// let both = m.remap(|_| at(0, 0), RemapOverlap::Neither, &mut s);
/// assert_eq!((both.get(), m.runs().count()), (1, 0));
/// ```
#[cfg(feature = "alloc")]
#[derive(Debug)]
pub struct IntervalMap<K: LineStep, V> {
    items: Vec<(LineInterval<K>, V)>,
}

#[cfg(feature = "alloc")]
impl<K: LineStep, V: Copy + Eq> Default for IntervalMap<K, V> {
    fn default() -> Self {
        IntervalMap::new()
    }
}

#[cfg(feature = "alloc")]
impl<K: LineStep, V: Copy + Eq> IntervalMap<K, V> {
    /// No run.
    pub const fn new() -> Self {
        IntervalMap { items: Vec::new() }
    }

    /// Every run gone.
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Whether it holds no run.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The runs, in order of their lines.
    pub fn runs(&self) -> impl Iterator<Item = (LineInterval<K>, V)> + '_ {
        self.items.iter().copied()
    }

    /// **`span`'s lines take `v`**: whatever held them gives way there and
    /// keeps the rest of its run.
    pub fn assign(&mut self, span: LineInterval<K>, v: V, steps: &mut WalkSteps) {
        self.put(span, Some(v), steps);
    }

    /// **`span`'s lines take no value**: whatever held them keeps the rest
    /// of its run.
    pub fn erase(&mut self, span: LineInterval<K>, steps: &mut WalkSteps) {
        self.put(span, None, steps);
    }

    /// **The index of the first run not ending before `line`**: a binary
    /// search, each run compared counted in `steps`.
    fn first_reaching(&self, line: K, steps: &mut WalkSteps) -> usize {
        self.items.partition_point(|(r, _)| {
            steps.bump();
            r.last < line
        })
    }

    /// **`span`'s lines given `v`, or no value**: a binary search to the
    /// first run that meets the span or touches it from above, a walk of
    /// the runs that meet or touch it, each counted in `steps`, and one
    /// splice. A touching run of `v` joins the new run; any other run
    /// keeps its lines outside the span.
    fn put(&mut self, span: LineInterval<K>, v: Option<V>, steps: &mut WalkSteps) {
        let from = match span.first.prev() {
            Some(above) => self.first_reaching(above, steps),
            None => 0,
        };
        let below = span.last.next();
        let mut to = from;
        while let Some(&(r, _)) = self.items.get(to) {
            steps.bump();
            if below.is_some_and(|b| r.first > b) {
                break;
            }
            to += 1;
        }
        let (mut first, mut last) = (span.first, span.last);
        let mut left = None;
        let mut right = None;
        if let (Some(&(r, w)), Some(above)) = (self.items[from..to].first(), span.first.prev()) {
            if r.first < span.first {
                match Some(w) == v {
                    true => first = r.first,
                    false => left = Some((LineInterval { last: above, ..r }, w)),
                }
            }
        }
        if let (Some(&(r, w)), Some(below)) = (self.items[from..to].last(), below) {
            if r.last > span.last {
                match Some(w) == v {
                    true => last = r.last,
                    false => right = Some((LineInterval { first: below, ..r }, w)),
                }
            }
        }
        let new = v.map(|v| (LineInterval { first, last }, v));
        self.items
            .splice(from..to, left.into_iter().chain(new).chain(right));
    }

    /// **Only the lines from `first` on**: the runs ending before it are
    /// drained, each counted in `steps`, and the one it falls in begins at
    /// it. When the first run already ends at or after `first`, one compare.
    pub fn keep_from(&mut self, first: K, steps: &mut WalkSteps) {
        steps.bump();
        let drop = match self.items.first() {
            Some((r, _)) if r.last < first => self.first_reaching(first, steps),
            _ => 0,
        };
        for _ in self.items.drain(..drop) {
            steps.bump();
        }
        if let Some((r, _)) = self.items.first_mut() {
            r.first = r.first.max(first);
        }
    }

    /// **The lines of `span` that `v` does not hold**, in runs, in order,
    /// written to `out`: a binary search to the first run that meets the
    /// span, then the runs that meet it, each counted in `steps`.
    pub fn gaps_of(
        &self,
        v: V,
        span: LineInterval<K>,
        out: &mut impl Extend<LineInterval<K>>,
        steps: &mut WalkSteps,
    ) {
        let from = self.first_reaching(span.first, steps);
        // The first line of the span not yet found held or a gap.
        let mut at = Some(span.first);
        for &(r, w) in &self.items[from..] {
            steps.bump();
            let Some(line) = at else { break };
            if r.first > span.last || line > span.last {
                break;
            }
            if w != v {
                continue;
            }
            if let Some(end) = r.first.prev().filter(|&end| end >= line) {
                out.extend([LineInterval {
                    first: line,
                    last: end,
                }]);
            }
            at = r.last.next();
        }
        if let Some(line) = at.filter(|&line| line <= span.last) {
            out.extend([LineInterval {
                first: line,
                last: span.last,
            }]);
        }
    }

    /// **Every run moved to where `to` says its lines went**, with
    /// `overlap` naming what a line two values' moved runs both cover
    /// takes. Runs of one value that land touching or over each other
    /// are one run. Each run moved, each compare of the sort by first
    /// line, and each piece the moved runs are laid over and read back
    /// from is counted in `steps`.
    pub fn remap(
        &mut self,
        mut to: impl FnMut(LineInterval<K>) -> LineInterval<K>,
        overlap: RemapOverlap,
        steps: &mut WalkSteps,
    ) -> OverlapRuns {
        match overlap {
            RemapOverlap::Neither => {}
        }
        let mut moved: Vec<(LineInterval<K>, V)> = self
            .items
            .drain(..)
            .map(|(r, w)| {
                steps.bump();
                (to(r), w)
            })
            .collect();
        moved.sort_unstable_by(|a, b| {
            steps.bump();
            a.0.first.cmp(&b.0.first)
        });
        // Sorted disjoint pieces, each with the one value every moved run
        // over it holds, or none where two values meet.
        let mut pieces: Vec<(LineInterval<K>, Option<V>)> = Vec::with_capacity(moved.len());
        let mut laid: Vec<(LineInterval<K>, Option<V>)> = Vec::new();
        for (r, w) in moved {
            let from = pieces.partition_point(|(p, _)| {
                steps.bump();
                p.last < r.first
            });
            let mut end = from;
            // The first line of `r` not yet laid.
            let mut at = Some(r.first);
            while let Some(&(p, held)) = pieces.get(end) {
                steps.bump();
                if p.first > r.last {
                    break;
                }
                end += 1;
                if let Some(above) = r.first.prev().filter(|_| p.first < r.first) {
                    laid.push((LineInterval { last: above, ..p }, held));
                }
                if let Some(line) = at {
                    if let Some(before) = p.first.prev().filter(|&b| b >= line) {
                        laid.push((
                            LineInterval {
                                first: line,
                                last: before,
                            },
                            Some(w),
                        ));
                    }
                }
                let both = LineInterval {
                    first: p.first.max(r.first),
                    last: p.last.min(r.last),
                };
                laid.push((both, held.filter(|&h| h == w)));
                if let Some(below) = r.last.next().filter(|_| p.last > r.last) {
                    laid.push((LineInterval { first: below, ..p }, held));
                }
                at = both.last.next();
            }
            if let Some(line) = at.filter(|&line| line <= r.last) {
                laid.push((LineInterval { first: line, ..r }, Some(w)));
            }
            pieces.splice(from..end, laid.drain(..));
        }
        let mut runs = 0u64;
        let mut last_neither: Option<K> = None;
        for (p, held) in pieces {
            steps.bump();
            match held {
                Some(w) => match self.items.last_mut() {
                    Some((prev, v)) if *v == w && prev.last.next() == Some(p.first) => {
                        prev.last = p.last;
                    }
                    _ => self.items.push((p, w)),
                },
                None => {
                    if last_neither.and_then(LineStep::next) != Some(p.first) {
                        runs = runs.saturating_add(1);
                    }
                    last_neither = Some(p.last);
                }
            }
        }
        OverlapRuns(runs)
    }
}
