//! Runs of lines, each with a key, kept sorted and never twice: whether two
//! sets hold the same spans is one linear compare, a span goes in by a
//! binary search, and whether a line they hold lies in a range is one
//! binary search of their cover.

use alloc::vec::Vec;
use core::cmp::Ordering;

use crate::{PendingGen, Tracked, WalkSteps};

/// **A run of lines on one lane**: what a [`SpanSet`] holds. The
/// implementor keeps `first() <= last()`, and orders spans so that two
/// equal spans compare equal.
pub trait Spanned: Copy + Ord {
    /// What the lines are numbered on: two lanes' lines never meet.
    type Lane: Copy + Ord;
    /// A line of a lane.
    type Line: Copy + Ord;
    /// The lane the lines are on.
    fn lane(self) -> Self::Lane;
    /// The first line.
    fn first(self) -> Self::Line;
    /// The last line, never before the first.
    fn last(self) -> Self::Line;
}

/// **Whether [`SpanSet::insert`] put the pair in**: `New`, or `Already`
/// there.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Inserted {
    New,
    Already,
}

/// **How many pairs a [`SpanSet`] holds** ([`SpanSet::len`]), saturating.
///
/// Made only by a set: the field is private.
///
/// ```compile_fail,E0423
/// let _ = baud_domain::SpanCount(0);
/// ```
///
/// The legal path:
///
/// ```
/// # #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
/// # struct Span(u8, u8);
/// # impl baud_domain::Spanned for Span {
/// #     type Lane = ();
/// #     type Line = u8;
/// #     fn lane(self) {}
/// #     fn first(self) -> u8 { self.0 }
/// #     fn last(self) -> u8 { self.1 }
/// # }
/// let s = baud_domain::SpanSet::<Span, ()>::new();
/// let _: baud_domain::SpanCount = s.len();
/// ```
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct SpanCount(u32);

impl SpanCount {
    /// The count, as an index.
    pub fn index(self) -> usize {
        // A `u32` fits a `usize` on every target baud builds for.
        usize::try_from(self.0).unwrap_or(usize::MAX)
    }
}

/// **Spans, each with a key, sorted by `(span, key)` and never twice.**
///
/// Sorted, two sets are the same by one linear walk ([`SpanSet::same`]),
/// and a pair goes in by a binary search ([`SpanSet::insert`]). Nothing
/// writes the pairs but the methods that keep the order: the field is
/// private, and there is no `push`, `iter_mut` or `as_mut_slice`. There is
/// no `PartialEq` either, so every compare of two sets is counted.
///
/// ```compile_fail,E0616
/// fn f<S: baud_domain::Spanned>(s: &mut baud_domain::SpanSet<S, ()>) { s.items.clear(); }
/// ```
///
/// ```compile_fail,E0599
/// fn f<S: baud_domain::Spanned>(s: &mut baud_domain::SpanSet<S, ()>, x: S) { s.push((x, ())); }
/// ```
///
/// ```compile_fail,E0599
/// fn f<S: baud_domain::Spanned>(s: &mut baud_domain::SpanSet<S, ()>) { let _ = s.iter_mut(); }
/// ```
///
/// The legal path:
///
/// ```
/// # #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
/// # struct Span(u8, u8);
/// # impl baud_domain::Spanned for Span {
/// #     type Lane = ();
/// #     type Line = u8;
/// #     fn lane(self) {}
/// #     fn first(self) -> u8 { self.0 }
/// #     fn last(self) -> u8 { self.1 }
/// # }
/// use baud_domain::{Inserted, SpanSet, WalkSteps};
/// fn f<S: baud_domain::Spanned>(s: &mut SpanSet<S, ()>, x: S, steps: &mut WalkSteps) -> Inserted {
///     s.insert(x, (), steps)
/// }
/// let mut s = SpanSet::new();
/// let mut steps = WalkSteps::ZERO;
/// assert_eq!(f(&mut s, Span(1, 2), &mut steps), Inserted::New);
/// assert_eq!(f(&mut s, Span(1, 2), &mut steps), Inserted::Already);
/// ```
#[derive(Debug)]
pub struct SpanSet<S: Spanned, K: Copy + Ord> {
    items: Vec<(S, K)>,
}

impl<S: Spanned, K: Copy + Ord> Default for SpanSet<S, K> {
    fn default() -> Self {
        SpanSet::new()
    }
}

impl<S: Spanned, K: Copy + Ord> SpanSet<S, K> {
    /// None.
    pub const fn new() -> Self {
        SpanSet { items: Vec::new() }
    }

    /// `pairs`, in whatever order and however often each comes: sorted,
    /// each once.
    pub fn of_unsorted(pairs: impl IntoIterator<Item = (S, K)>) -> Self {
        let mut items: Vec<(S, K)> = pairs.into_iter().collect();
        items.sort_unstable();
        items.dedup();
        SpanSet { items }
    }

    /// The pairs, sorted.
    pub fn iter(&self) -> impl Iterator<Item = (S, K)> + '_ {
        self.items.iter().copied()
    }

    /// **`(span, key)` put in its place**, found by a binary search, each
    /// pair compared counted in `steps`; nothing if it is already there.
    pub fn insert(&mut self, span: S, key: K, steps: &mut WalkSteps) -> Inserted {
        let at = self.items.binary_search_by(|e| {
            steps.bump();
            e.cmp(&(span, key))
        });
        match at {
            Ok(_) => Inserted::Already,
            Err(i) => {
                self.items.insert(i, (span, key));
                Inserted::New
            }
        }
    }

    /// **`more`'s pairs put in, by one merge**: `more` is sorted first, and
    /// left empty. Each compare of the sort and the merge is counted in
    /// `steps`.
    pub fn extend_sorted(&mut self, more: &mut Vec<(S, K)>, steps: &mut WalkSteps) {
        more.sort_unstable_by(|a, b| {
            steps.bump();
            a.cmp(b)
        });
        more.dedup();
        let old = core::mem::take(&mut self.items);
        self.items.reserve(old.len() + more.len());
        let mut old = old.into_iter().peekable();
        let mut new = more.drain(..).peekable();
        while let (Some(a), Some(b)) = (old.peek(), new.peek()) {
            steps.bump();
            let next = match a.cmp(b) {
                Ordering::Less => old.next(),
                Ordering::Greater => new.next(),
                Ordering::Equal => {
                    let _ = new.next();
                    old.next()
                }
            };
            self.items.extend(next);
        }
        self.items.extend(old);
        self.items.extend(new);
    }

    /// The pairs `keep` keeps, in their order.
    pub fn retain(&mut self, mut keep: impl FnMut(S, K) -> bool) {
        self.items.retain(|&(s, k)| keep(s, k));
    }

    /// **Each span replaced by `to`'s**, then sorted again, each pair once.
    pub fn map_spans(&mut self, mut to: impl FnMut(S) -> S) {
        for e in &mut self.items {
            e.0 = to(e.0);
        }
        self.items.sort_unstable();
        self.items.dedup();
    }

    /// **The keys `span` is held with**, in order: a binary search to the
    /// first, then each of them.
    pub fn keys_of(&self, span: S) -> impl Iterator<Item = K> + '_ {
        let from = self.items.partition_point(|e| e.0 < span);
        self.items[from..]
            .iter()
            .take_while(move |e| e.0 == span)
            .map(|e| e.1)
    }

    /// **Whether `other` holds the pairs this does**: one walk of both,
    /// each pair compared counted in `steps`; none when their lengths
    /// differ.
    pub fn same(&self, other: &Self, steps: &mut WalkSteps) -> bool {
        self.items.len() == other.items.len()
            && self.items.iter().zip(&other.items).all(|(a, b)| {
                steps.bump();
                a == b
            })
    }

    /// How many pairs it holds.
    pub fn len(&self) -> SpanCount {
        SpanCount(u32::try_from(self.items.len()).unwrap_or(u32::MAX))
    }

    /// Whether it holds none.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// **The lines a set's spans hold, per lane, merged into sorted disjoint
/// runs**, tagged with the generation of the set it was made from: whether
/// any of those lines lies in a range is one binary search
/// ([`HeldCover::meets`]), however many spans there are. Spans that
/// overlap make one run; spans that only touch stay two, which changes no
/// answer.
///
/// Made only from a [`Tracked`] set ([`HeldCover::of`]), so its runs are
/// those of a set at the generation it names: the fields are private.
///
/// ```compile_fail,E0451
/// fn f<S: baud_domain::Spanned>(h: baud_domain::HeldCover<S>) { let _ = baud_domain::HeldCover { ..h }; }
/// ```
///
/// The legal path:
///
/// ```
/// # #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
/// # struct Span(u8, u8);
/// # impl baud_domain::Spanned for Span {
/// #     type Lane = ();
/// #     type Line = u8;
/// #     fn lane(self) {}
/// #     fn first(self) -> u8 { self.0 }
/// #     fn last(self) -> u8 { self.1 }
/// # }
/// use baud_domain::{HeldCover, SpanSet, Tracked, WalkSteps};
/// let held = Tracked::new(SpanSet::of_unsorted([(Span(2, 4), ()), (Span(3, 6), ())]));
/// let cover = HeldCover::of(&held);
/// let mut steps = WalkSteps::ZERO;
/// assert!(cover.meets((), 6, 9, &mut steps));
/// assert!(!cover.meets((), 7, 9, &mut steps));
/// assert_eq!(cover.at(), held.generation());
/// ```
#[derive(Debug)]
pub struct HeldCover<S: Spanned> {
    runs: Vec<(S::Lane, S::Line, S::Line)>,
    at: PendingGen,
}

impl<S: Spanned> HeldCover<S> {
    /// **The runs `held`'s spans make**, whatever keys hold them, at
    /// `held`'s generation: sorted by lane and first line, overlapping
    /// spans joined.
    pub fn of<K: Copy + Ord>(held: &Tracked<SpanSet<S, K>>) -> Self {
        let mut spans: Vec<(S::Lane, S::Line, S::Line)> = held
            .get()
            .iter()
            .map(|(s, _)| (s.lane(), s.first(), s.last()))
            .collect();
        spans.sort_unstable();
        let mut runs: Vec<(S::Lane, S::Line, S::Line)> = Vec::with_capacity(spans.len());
        for (lane, first, last) in spans {
            match runs.last_mut() {
                Some((l, _, end)) if *l == lane && first <= *end => *end = (*end).max(last),
                _ => runs.push((lane, first, last)),
            }
        }
        HeldCover {
            runs,
            at: held.generation(),
        }
    }

    /// The generation of the set it was made from.
    pub fn at(&self) -> PendingGen {
        self.at
    }

    /// **Whether a line from `first` to `last` of `lane` is held**: a
    /// binary search to the first run of the lane not ending before
    /// `first`, each run compared counted in `steps`, then that run's
    /// first line against `last`.
    pub fn meets(
        &self,
        lane: S::Lane,
        first: S::Line,
        last: S::Line,
        steps: &mut WalkSteps,
    ) -> bool {
        let at = self.runs.partition_point(|&(l, _, end)| {
            steps.bump();
            (l, end) < (lane, first)
        });
        self.runs
            .get(at)
            .is_some_and(|&(l, start, _)| l == lane && start <= last)
    }
}
