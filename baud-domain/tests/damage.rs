//! `DirtyRows` against a reference set, and `OverlayChanges` over every
//! combination of its six overlays.

use std::collections::BTreeSet;

use baud_domain::{
    CursorChange, DIRTY_ROWS, DirtyRows, FrameDamage, OverlayChanges, Row, WholeWhy,
};
use proptest::prelude::*;

/// Every row a word of the set can hold, the ones past `DIRTY_ROWS` too.
const PAST: usize = 11 * 64;

fn row(i: usize) -> Row {
    Row::from_index(i)
}

/// The set holding `rows`, adding each in turn; `None` once one is past
/// `DIRTY_ROWS`.
fn build(rows: &[usize]) -> Option<DirtyRows> {
    rows.iter()
        .try_fold(DirtyRows::NONE, |s, &r| s.with(row(r)))
}

fn agrees(set: DirtyRows, want: &BTreeSet<usize>) {
    for i in 0..PAST {
        assert_eq!(set.contains(row(i)), want.contains(&i), "row {i}");
    }
    let got: Vec<usize> = set.iter().map(Row::index).collect();
    let want: Vec<usize> = want.iter().copied().collect();
    assert_eq!(got, want);
}

#[test]
fn a_row_past_dirty_rows_is_none() {
    assert_eq!(DIRTY_ROWS, baud_domain::MAX_ROWS + 2);
    assert!(DirtyRows::NONE.with(row(DIRTY_ROWS - 1)).is_some());
    for i in DIRTY_ROWS..PAST + 2 {
        assert_eq!(DirtyRows::NONE.with(row(i)), None, "row {i}");
    }
}

#[test]
fn the_rows_at_each_word_edge_are_held_and_read_back() {
    let edges = [
        0,
        1,
        62,
        63,
        64,
        65,
        127,
        128,
        575,
        576,
        639,
        640,
        DIRTY_ROWS - 1,
    ];
    let set = build(&edges).expect("every edge is under DIRTY_ROWS");
    agrees(set, &edges.iter().copied().collect());
}

#[test]
fn none_holds_no_row() {
    agrees(DirtyRows::NONE, &BTreeSet::new());
}

#[test]
fn a_span_holds_exactly_its_range() {
    for (from, to) in [
        (0, 0),
        (0, 1),
        (3, 70),
        (63, 65),
        (0, DIRTY_ROWS),
        (640, DIRTY_ROWS),
    ] {
        let set = DirtyRows::NONE.span(row(from), row(to)).expect("in range");
        agrees(set, &(from..to).collect());
    }
}

#[test]
fn a_span_past_dirty_rows_is_none_and_an_empty_one_is_not() {
    assert_eq!(DirtyRows::NONE.span(row(0), row(DIRTY_ROWS + 1)), None);
    assert_eq!(
        DirtyRows::NONE.span(row(DIRTY_ROWS), row(DIRTY_ROWS + 1)),
        None
    );
    assert_eq!(
        DirtyRows::NONE.span(row(DIRTY_ROWS + 5), row(DIRTY_ROWS + 5)),
        Some(DirtyRows::NONE)
    );
    assert_eq!(DirtyRows::NONE.span(row(9), row(4)), Some(DirtyRows::NONE));
}

proptest! {
    #[test]
    fn with_union_contains_and_iter_agree_with_a_reference_set(
        a in proptest::collection::vec(0..DIRTY_ROWS, 0..80),
        b in proptest::collection::vec(0..DIRTY_ROWS, 0..80),
        past in DIRTY_ROWS..PAST + 64,
    ) {
        let sa = build(&a).expect("every row is under DIRTY_ROWS");
        let sb = build(&b).expect("every row is under DIRTY_ROWS");
        prop_assert_eq!(sa.with(row(past)), None);
        prop_assert!(!sa.contains(row(past)));
        let ra: BTreeSet<usize> = a.iter().copied().collect();
        let rb: BTreeSet<usize> = b.iter().copied().collect();
        agrees(sa, &ra);
        agrees(sb, &rb);
        let both = sa.union(sb);
        agrees(both, &ra.union(&rb).copied().collect());
        prop_assert_eq!(both, sb.union(sa));
        prop_assert_eq!(sa.union(DirtyRows::NONE), sa);
    }

    #[test]
    fn a_span_adds_exactly_its_range_to_a_set(
        a in proptest::collection::vec(0..DIRTY_ROWS, 0..20),
        from in 0..DIRTY_ROWS + 10,
        to in 0..DIRTY_ROWS + 10,
    ) {
        let base = build(&a).expect("every row is under DIRTY_ROWS");
        let got = base.span(row(from), row(to));
        if from < to && to > DIRTY_ROWS {
            prop_assert_eq!(got, None);
        } else {
            let mut want: BTreeSet<usize> = a.iter().copied().collect();
            want.extend(from..to);
            agrees(got.expect("in range"), &want);
        }
    }
}

const OVERLAYS: [OverlayChanges; 6] = [
    OverlayChanges::SELECTION,
    OverlayChanges::FIND,
    OverlayChanges::HOVER,
    OverlayChanges::SCROLLBAR,
    OverlayChanges::IME,
    OverlayChanges::SPLASH,
];

fn overlays(mask: u8) -> OverlayChanges {
    OVERLAYS
        .iter()
        .enumerate()
        .filter(|(i, _)| mask & (1 << i) != 0)
        .fold(OverlayChanges::NONE, |s, (_, &o)| s.union(o))
}

#[test]
fn overlay_changes_union_and_contains_agree_over_every_combination() {
    for a in 0u8..64 {
        for b in 0u8..64 {
            let both = overlays(a).union(overlays(b));
            assert_eq!(both, overlays(a | b));
            for (i, &o) in OVERLAYS.iter().enumerate() {
                assert_eq!(both.contains(o), (a | b) & (1 << i) != 0, "{a} {b} {i}");
            }
        }
        assert_eq!(overlays(a) == OverlayChanges::NONE, a == 0);
    }
}

#[test]
fn a_frame_damage_is_copied_whole() {
    let rows = DirtyRows::NONE.with(row(3)).expect("in range");
    let d = FrameDamage::Rows {
        rows,
        cursor: CursorChange::Moved,
        overlays: OverlayChanges::FIND,
    };
    let e = d;
    assert_eq!(d, e);
    assert_ne!(FrameDamage::Whole(WholeWhy::Resized), e);
    let whys = [
        WholeWhy::Resized,
        WholeWhy::Screen,
        WholeWhy::Reset,
        WholeWhy::Erased,
        WholeWhy::Wiped,
        WholeWhy::Colours,
        WholeWhy::Clusters,
        WholeWhy::Renditions,
        WholeWhy::Overflow,
    ];
    for w in whys {
        assert!(matches!(FrameDamage::Whole(w), FrameDamage::Whole(x) if x == w));
    }
    for c in [
        CursorChange::Same,
        CursorChange::Moved,
        CursorChange::Restyled,
    ] {
        assert_eq!(c, c);
    }
}
