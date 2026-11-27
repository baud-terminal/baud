//! `LinkSet`: the slots a run of cells names, as a fold over the cells one
//! at a time names them, whatever else the cells' flags hold; `LinkSlot`:
//! only the indices a cell's link bits spell, `NONE`'s not among them; and
//! `SlotMarks`: a slot judged once between clears.

use std::collections::{BTreeMap, BTreeSet};

use baud_domain::{CellFlags, LinkId, LinkSet, LinkSlot, PackedCell, SlotMarks};
use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::TestRunner;
use zerocopy::FromBytes;

/// The link bits of a flag word: seven, from bit 3.
const LINK_SHIFT: u32 = 3;
const LINK_BITS: u16 = 0x7f;

/// The highest index a cell's link bits spell.
const TOP: u16 = LINK_BITS;

/// A cell whose flags are `raw`, every bit as given.
fn cell(raw: u16) -> PackedCell {
    let flags = CellFlags::read_from_bytes(&raw.to_le_bytes()).expect("two bytes");
    PackedCell {
        flags,
        ..PackedCell::EMPTY
    }
}

/// The link index `raw` spells.
fn link_of(raw: u16) -> u16 {
    (raw >> LINK_SHIFT) & LINK_BITS
}

/// A flag word: mostly no link, else any index the bits spell, the top one
/// often; the other bits -- protection, the cluster mark, the guard, the
/// set received under -- anything.
fn raw() -> impl Strategy<Value = u16> {
    let link = prop_oneof![
        6 => Just(0u16),
        2 => 1..=TOP,
        1 => Just(TOP),
    ];
    (link, any::<u16>())
        .prop_map(|(l, other)| (other & !(LINK_BITS << LINK_SHIFT)) | (l << LINK_SHIFT))
}

/// A run of cells: long enough for several blocks of sixteen and a last one
/// cut short, a block often naming nothing.
fn run() -> impl Strategy<Value = Vec<u16>> {
    let block = prop_oneof![
        1 => proptest::collection::vec(raw().prop_map(|r| r & !(LINK_BITS << LINK_SHIFT)), 16..=16),
        2 => proptest::collection::vec(raw(), 1..=16),
    ];
    proptest::collection::vec(block, 0..6).prop_map(|b| b.concat())
}

/// The slots a fold over the cells one at a time names.
fn fold(raws: &[u16]) -> BTreeSet<LinkSlot> {
    raws.iter()
        .filter_map(|&r| cell(r).flags.link().slot())
        .collect()
}

fn of(raws: &[u16]) -> LinkSet {
    let cells: Vec<PackedCell> = raws.iter().map(|&r| cell(r)).collect();
    LinkSet::of_cells(&cells)
}

/// Every slot there is, by index.
fn every_slot() -> impl Iterator<Item = LinkSlot> {
    (1..=TOP).map(|i| LinkId::from_index(usize::from(i)).slot().expect("a slot"))
}

proptest! {
    /// **`of_cells` names what a fold over the cells names**, its slots
    /// lowest first, and `has`, `is_empty`, `with`, `union` and `meets`
    /// answer as the fold's sets do. Named break: read only the first
    /// fifteen cells of a block -- a link in a block's last cell is missed.
    #[test]
    fn of_cells_names_what_a_cell_by_cell_fold_names(a in run(), b in run()) {
        let (want_a, want_b) = (fold(&a), fold(&b));
        let (set_a, set_b) = (of(&a), of(&b));
        prop_assert_eq!(set_a.slots().collect::<Vec<_>>(), want_a.iter().copied().collect::<Vec<_>>());
        for s in every_slot() {
            prop_assert_eq!(set_a.has(s), want_a.contains(&s));
            let mut with = want_a.clone();
            with.insert(s);
            prop_assert_eq!(set_a.with(s).slots().collect::<BTreeSet<_>>(), with);
        }
        prop_assert_eq!(set_a.is_empty(), want_a.is_empty());
        let both: Vec<u16> = a.iter().chain(&b).copied().collect();
        prop_assert_eq!(set_a.union(set_b), of(&both));
        prop_assert_eq!(set_a.meets(set_b), !want_a.is_disjoint(&want_b));
    }

    /// **A slot is judged once between clears**: against a map of the marks
    /// kept after the last clear, every `get_or_judge` returns the kept mark
    /// where there is one and runs its judge only where there is none.
    /// Named break: keep `judged` across `clear` -- a slot judged before
    /// the clear is not judged again.
    #[test]
    fn a_slot_is_judged_once_between_clears(
        ops in proptest::collection::vec(prop_oneof![
            8 => (1..=TOP, any::<u8>()).prop_map(|(i, m)| Some((i, m))),
            1 => Just(None),
        ], 0..60)
    ) {
        let mut marks = SlotMarks::<u8>::new();
        let mut model: BTreeMap<u16, u8> = BTreeMap::new();
        for op in ops {
            match op {
                None => {
                    marks.clear();
                    model.clear();
                }
                Some((i, m)) => {
                    let s = LinkId::from_index(usize::from(i)).slot().expect("a slot");
                    let had = model.contains_key(&i);
                    let mut ran = false;
                    let got = marks.get_or_judge(s, || {
                        ran = true;
                        m
                    });
                    let kept = *model.entry(i).or_insert(m);
                    prop_assert_eq!(got, kept);
                    prop_assert_eq!(ran, !had);
                }
            }
        }
    }
}

/// **The generator reaches what the blocks can get wrong**, each in at least
/// one of 2,000 runs drawn from a fixed seed: no cell; a block of sixteen
/// naming no link before one naming one; a link in a block's first and in
/// its last cell; the top slot; a cell naming no link with other bits set;
/// one slot named twice; and a link in a last block cut short. Named break:
/// runs of whole blocks only -- the short last block is never reached.
#[test]
fn the_generator_reaches_skipped_blocks_block_edges_and_the_top_slot() {
    let mut runner = TestRunner::deterministic();
    let mut reached = BTreeSet::new();
    for _ in 0..2000 {
        let raws = run().new_tree(&mut runner).expect("a run").current();
        if raws.is_empty() {
            reached.insert("empty");
        }
        let blocks: Vec<&[u16]> = raws.chunks(16).collect();
        let names = |b: &[u16]| b.iter().any(|&r| link_of(r) != 0);
        if blocks
            .windows(2)
            .any(|w| w[0].len() == 16 && !names(w[0]) && names(w[1]))
        {
            reached.insert("skipped block");
        }
        if blocks.iter().any(|b| link_of(b[0]) != 0) {
            reached.insert("first cell");
        }
        if blocks.iter().any(|b| b.len() == 16 && link_of(b[15]) != 0) {
            reached.insert("last cell");
        }
        if raws.iter().any(|&r| link_of(r) == TOP) {
            reached.insert("top slot");
        }
        if raws.iter().any(|&r| link_of(r) == 0 && r != 0) {
            reached.insert("other bits");
        }
        let linked: Vec<u16> = raws
            .iter()
            .map(|&r| link_of(r))
            .filter(|&l| l != 0)
            .collect();
        if linked.len() > linked.iter().collect::<BTreeSet<_>>().len() {
            reached.insert("twice");
        }
        if blocks.last().is_some_and(|b| b.len() < 16 && names(b)) {
            reached.insert("short last block");
        }
    }
    let want = [
        "empty",
        "first cell",
        "last cell",
        "other bits",
        "short last block",
        "skipped block",
        "top slot",
        "twice",
    ];
    assert_eq!(reached.into_iter().collect::<Vec<_>>(), want);
}

/// **Only the indices a cell's link bits spell are slots**, `NONE` not
/// among them, and a slot is the link it was made from. Named break: drop
/// the bound in `slot` -- index 128 is a slot.
#[test]
fn a_link_id_is_a_slot_only_where_a_cell_can_name_it() {
    assert_eq!(LinkId::NONE.slot(), None);
    for i in 1..=usize::from(TOP) {
        let s = LinkId::from_index(i).slot().expect("a slot");
        assert_eq!(s.id(), LinkId::from_index(i));
    }
    assert_eq!(LinkId::from_index(usize::from(TOP) + 1).slot(), None);
    assert_eq!(every_slot().count(), usize::from(TOP));
    assert!(LinkSet::EMPTY.is_empty());
    assert_eq!(LinkSet::EMPTY.slots().next(), None);
}
