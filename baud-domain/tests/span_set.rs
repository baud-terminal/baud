//! `SpanSet`: sorted by `(span, key)`, each pair once, whatever order the
//! pairs come in and whichever door they come through; and its walks are
//! counted.

use std::collections::BTreeSet;

use baud_domain::{Inserted, SpanSet, Spanned, WalkSteps};
use proptest::prelude::*;

/// A lane and its first and last line.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct Span(u8, u8, u8);

impl Span {
    fn of(lane: u8, a: u8, b: u8) -> Span {
        Span(lane, a.min(b), a.max(b))
    }
}

impl Spanned for Span {
    type Lane = u8;
    type Line = u8;
    fn lane(self) -> u8 {
        self.0
    }
    fn first(self) -> u8 {
        self.1
    }
    fn last(self) -> u8 {
        self.2
    }
}

fn pairs() -> impl Strategy<Value = Vec<(Span, u8)>> {
    let pair = (0u8..3, 0u8..12, 0u8..12, 0u8..3).prop_map(|(l, a, b, k)| (Span::of(l, a, b), k));
    proptest::collection::vec(pair, 0..40)
}

/// What a `BTreeSet` of the pairs holds, in its order.
fn oracle(of: &[(Span, u8)]) -> Vec<(Span, u8)> {
    of.iter()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

proptest! {
    /// **Every door leaves the pairs sorted and each once**: `of_unsorted`,
    /// `insert` one at a time, and `extend_sorted` in two halves each hold
    /// what a `BTreeSet` of the pairs holds, in its order; `insert` says
    /// `New` exactly when the oracle did not hold the pair. Named break: a
    /// merge that keeps both of an equal pair -- the set holds one twice.
    #[test]
    fn every_door_keeps_the_pairs_sorted_and_once(all in pairs(), cut in 0usize..40) {
        let want = oracle(&all);
        let built = SpanSet::of_unsorted(all.iter().copied());
        prop_assert_eq!(built.iter().collect::<Vec<_>>(), want.clone());
        let mut one = SpanSet::new();
        let mut seen = BTreeSet::new();
        let mut steps = WalkSteps::ZERO;
        for &(s, k) in &all {
            let new = seen.insert((s, k));
            let got = one.insert(s, k, &mut steps);
            prop_assert_eq!(got, if new { Inserted::New } else { Inserted::Already });
        }
        prop_assert_eq!(one.iter().collect::<Vec<_>>(), want.clone());
        let cut = cut.min(all.len());
        let mut merged = SpanSet::of_unsorted(all[..cut].iter().copied());
        let mut more = all[cut..].to_vec();
        merged.extend_sorted(&mut more, &mut steps);
        prop_assert!(more.is_empty(), "more left empty");
        prop_assert_eq!(merged.iter().collect::<Vec<_>>(), want.clone());
        prop_assert!(merged.same(&built, &mut steps));
        prop_assert_eq!(merged.len().index(), want.len());
    }

    /// **`keys_of` is each key a span is held with, in order**, and
    /// `map_spans` and `retain` keep the set sorted and each pair once.
    /// Named break: `keys_of` that stops at the first key -- a span held
    /// under two keys reads one.
    #[test]
    fn keys_and_maps_keep_the_order(all in pairs(), shift in 0u8..4, lane in 0u8..3) {
        let mut set = SpanSet::of_unsorted(all.iter().copied());
        for (s, _) in oracle(&all) {
            let want: Vec<u8> = oracle(&all).into_iter().filter(|e| e.0 == s).map(|e| e.1).collect();
            prop_assert_eq!(set.keys_of(s).collect::<Vec<_>>(), want);
        }
        let to = |s: Span| Span::of(s.0, s.1 / (shift + 1), s.2 / (shift + 1));
        set.map_spans(to);
        let mapped: Vec<_> = all.iter().map(|&(s, k)| (to(s), k)).collect();
        prop_assert_eq!(set.iter().collect::<Vec<_>>(), oracle(&mapped));
        set.retain(|s, _| s.lane() != lane);
        let kept: Vec<_> = mapped.into_iter().filter(|e| e.0.lane() != lane).collect();
        prop_assert_eq!(set.iter().collect::<Vec<_>>(), oracle(&kept));
    }
}

/// **Two sets are compared by one walk**: the same 200 pairs compare in
/// 200 steps, one pair moved in at most as many, and sets of different
/// lengths in none. Named break: each pair looked for in the whole other
/// set -- 200 equal pairs take about 20,000 steps.
#[test]
fn a_compare_walks_each_pair_once() {
    let spans = |moved: u8| {
        SpanSet::of_unsorted((0..200u8).map(|n| {
            let at = if n == 100 { moved } else { n };
            (Span::of(n % 2, at, at), ())
        }))
    };
    let (a, b) = (spans(100), spans(100));
    let mut steps = WalkSteps::ZERO;
    assert!(a.same(&b, &mut steps), "the control: the same pairs");
    assert_eq!(steps.get(), 200);
    let mut steps = WalkSteps::ZERO;
    assert!(!a.same(&spans(250), &mut steps), "one pair moved");
    assert!((1..=200).contains(&steps.get()), "{} steps", steps.get());
    let mut fewer = spans(100);
    fewer.retain(|s, ()| s.first() != 0);
    let mut steps = WalkSteps::ZERO;
    assert!(!a.same(&fewer, &mut steps), "one pair fewer");
    assert_eq!(steps.get(), 0);
}

/// **A pair goes in by a binary search**: into n pairs, at most
/// ⌈log2 n⌉ + 1 compares -- 11 into 1,024 -- and the same pair, now one of
/// 1,025, says `Already` in at most 12. Named break: a walk to the place --
/// up to 1,024 compares.
#[test]
fn an_insert_compares_by_log() {
    let line = |n: u16| u8::try_from(n / 4).expect("a line");
    let mut set = SpanSet::of_unsorted((0..1_024u16).map(|n| (Span::of(0, line(n), 255), n % 4)));
    assert_eq!(set.len().index(), 1_024, "the control: every pair held");
    let mut steps = WalkSteps::ZERO;
    assert_eq!(
        set.insert(Span::of(0, 100, 200), 0, &mut steps),
        Inserted::New
    );
    assert!(steps.get() <= 11, "{} compares", steps.get());
    let mut steps = WalkSteps::ZERO;
    assert_eq!(
        set.insert(Span::of(0, 100, 200), 0, &mut steps),
        Inserted::Already
    );
    assert!(steps.get() <= 12, "{} compares", steps.get());
}
