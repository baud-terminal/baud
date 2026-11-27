//! **A window-slot index stays a permutation, its inverse with it**, under
//! any sequence of [`SlotIndex::rotate`] and [`SlotIndex::normalise`] over
//! regions anywhere in the ring -- straddling its end under a rotation,
//! rotated by nothing or by the height or more, one row, the whole ring --
//! and each operation does what an independent model of the ring's order
//! says: a rotation moves places, and a normalise moves the rows so each
//! place's row is stored at its own slot.
//!
//! Named break: `rotate` writes `slot` without `row_of` -- `row_of[slot[y]]
//! == y` fails at the first rotation that moves a place.
//! Named break: `rotate` reads a straddling region as `top + at ..= bot +
//! at` without wrapping -- the model disagrees, or the rotation panics past
//! the index, on the first region that crosses the ring's end.
//! Named break: `normalise` resets the index without swapping rows -- the
//! rows each place shows change, and the content check fails.

use baud_domain::{Row, RowRot, Rows, SlotIndex, SlotState, WindowSlot};
use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::TestRunner;

/// The most rows of a generated ring.
fn most() -> Rows {
    Rows::of_count(40)
}

/// One operation on the index.
#[derive(Clone, Debug)]
enum Op {
    /// Window rows `top ..= top + height - 1`, under ring rotation `at`,
    /// rotated up by `by`.
    Rotate {
        at: RowRot,
        top: Row,
        height: Rows,
        by: Rows,
    },
    /// The identity again, the rows following.
    Normalise,
}

/// A ring of `ring` rows and a sequence of operations on it.
#[derive(Clone, Debug)]
struct Program {
    ring: Rows,
    ops: Vec<Op>,
}

fn op(ring: Rows) -> impl Strategy<Value = Op> {
    let h = ring.index();
    let rotate = (0..h, 0..h, 1..=h, 0..=2 * h).prop_map(move |(at, top, height, by)| {
        // The region stays inside the window: its bottom row below the ring.
        let height = height.min(h - top);
        Op::Rotate {
            at: RowRot::NONE.advance(at, Rows::of_index(h)),
            top: Row::from_index(top),
            height: Rows::of_count(height),
            by: Rows::of_count(by),
        }
    });
    prop_oneof![8 => rotate, 1 => Just(Op::Normalise)]
}

fn program() -> impl Strategy<Value = Program> {
    (1..=most().index()).prop_flat_map(|h| {
        let ring = Rows::of_count(h);
        proptest::collection::vec(op(ring), 0..24).prop_map(move |ops| Program { ring, ops })
    })
}

/// Which places of a ring of `ring` rows window rows `top ..= bot` hold
/// under rotation `at`, in window order.
fn places(ring: Rows, at: RowRot, top: Row, height: Rows) -> Vec<Row> {
    (0..height.index())
        .map(|i| {
            Row::from_index(
                RowRot::NONE
                    .advance(at.index() + top.index() + i, ring)
                    .index(),
            )
        })
        .collect()
}

/// What each place shows, by the model: `order[p]` is the row of storage
/// place `p` shows, as `stored[s]` is what storage row `s` holds.
struct Model {
    order: Vec<Row>,
    stored: Vec<Row>,
}

impl Model {
    fn new(ring: Rows) -> Model {
        let all: Vec<Row> = (0..ring.index()).map(Row::from_index).collect();
        Model {
            order: all.clone(),
            stored: all,
        }
    }

    /// What place `p` shows.
    fn shown(&self, p: Row) -> Row {
        self.stored[self.order[p.index()].index()]
    }
}

/// Run `prog`, checking the index against the model after every operation.
fn run(prog: &Program) -> Result<(), TestCaseError> {
    let h = prog.ring.index();
    let mut store = vec![WindowSlot::UNSET; 2 * h];
    let mut state = SlotState::Identity;
    let mut ix = SlotIndex::identity(&mut store, &mut state, prog.ring);
    let mut model = Model::new(prog.ring);
    prop_assert!(ix.order().is_identity());
    for op in &prog.ops {
        match *op {
            Op::Rotate {
                at,
                top,
                height,
                by,
            } => {
                let bot = Row::from_index(top.index() + height.index() - 1);
                let at_places = places(prog.ring, at, top, height);
                let mut moved: Vec<Row> =
                    at_places.iter().map(|p| model.order[p.index()]).collect();
                moved.rotate_left(by.index() % height.index());
                for (p, s) in at_places.iter().zip(moved) {
                    model.order[p.index()] = s;
                }
                ix.rotate(at, top..=bot, by);
            }
            Op::Normalise => {
                let before: Vec<Row> = (0..h).map(|p| model.shown(Row::from_index(p))).collect();
                let mut swaps = Rows::ZERO;
                let returned = ix.normalise(|a, b| {
                    model.stored.swap(a.index(), b.index());
                    swaps = Rows::of_count(swaps.index() + 1);
                });
                prop_assert!(
                    swaps.index() < h.max(1),
                    "{} swaps over {h} rows",
                    swaps.index()
                );
                model.order = (0..h).map(Row::from_index).collect();
                prop_assert!(ix.order().is_identity(), "normalised to the identity");
                prop_assert_eq!(
                    returned,
                    swaps,
                    "the swaps it returns are the swaps it made"
                );
                prop_assert_eq!(ix.state(), SlotState::Identity);
                for (p, b) in before.iter().enumerate() {
                    prop_assert_eq!(
                        model.shown(Row::from_index(p)),
                        *b,
                        "place {} kept its row",
                        p
                    );
                }
            }
        }
        let o = ix.order();
        prop_assert!(
            o.is_permutation(),
            "a permutation with its inverse after {:?}",
            op
        );
        for (p, s) in model.order.iter().enumerate() {
            prop_assert_eq!(o.slot(p), s.index(), "place {} after {:?}", p, op);
            prop_assert_eq!(
                o.position(s.index()),
                p,
                "row_of[slot[{}]] after {:?}",
                p,
                op
            );
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(20_000))]

    #[test]
    fn any_rotations_and_normalises_leave_a_permutation_and_its_inverse(prog in program()) {
        run(&prog)?;
    }
}

/// What a generated rotation covers, counted over many programs.
#[derive(Debug)]
struct Reach {
    straddles_rotated: Rows,
    by_nothing: Rows,
    by_height_or_more: Rows,
    one_row: Rows,
    whole_ring: Rows,
    normalises: Rows,
}

fn more(n: &mut Rows) {
    *n = Rows::of_count(n.index() + 1);
}

/// **The generator reaches every shape the property is about**: a region
/// straddling the ring's end under a rotation, a rotation by nothing, one
/// by the height or more, a region of one row, the whole ring, and a
/// normalise.
///
/// Named break: the generator keeps `top + height` below the ring at
/// rotation 0 -- `straddles_rotated` stays at zero.
#[test]
fn the_generator_reaches_every_shape() {
    let mut runner = TestRunner::deterministic();
    let mut reach = Reach {
        straddles_rotated: Rows::ZERO,
        by_nothing: Rows::ZERO,
        by_height_or_more: Rows::ZERO,
        one_row: Rows::ZERO,
        whole_ring: Rows::ZERO,
        normalises: Rows::ZERO,
    };
    let strategy = program();
    for _ in 0..20_000 {
        let prog = strategy.new_tree(&mut runner).expect("a program").current();
        for op in &prog.ops {
            match *op {
                Op::Rotate {
                    at,
                    top,
                    height,
                    by,
                } => {
                    let first = at.index() + top.index();
                    let last = first + height.index() - 1;
                    if at != RowRot::NONE
                        && first < prog.ring.index()
                        && last >= prog.ring.index()
                        && by.index() % height.index() != 0
                    {
                        more(&mut reach.straddles_rotated);
                    }
                    if by == Rows::ZERO {
                        more(&mut reach.by_nothing);
                    }
                    if by.index() >= height.index() {
                        more(&mut reach.by_height_or_more);
                    }
                    if height == Rows::of_count(1) {
                        more(&mut reach.one_row);
                    }
                    if height == prog.ring {
                        more(&mut reach.whole_ring);
                    }
                }
                Op::Normalise => more(&mut reach.normalises),
            }
        }
    }
    for (name, n) in [
        ("a straddling region rotated", reach.straddles_rotated),
        ("a rotation by nothing", reach.by_nothing),
        ("a rotation by the height or more", reach.by_height_or_more),
        ("a region of one row", reach.one_row),
        ("the whole ring", reach.whole_ring),
        ("a normalise", reach.normalises),
    ] {
        assert!(n.index() > 0, "the generator never made {name}: {reach:?}");
    }
}

/// **Rotations compose and undo**: up by `k` then by the height less `k` is
/// the identity again, on a region straddling the ring's end.
///
/// Named break: `rotate` turns the region down -- the first assertion fails.
#[test]
fn a_straddling_rotation_moves_places_and_undoes() {
    let ring = Rows::of_count(6);
    let mut store = vec![WindowSlot::UNSET; 12];
    let mut state = SlotState::Identity;
    let mut ix = SlotIndex::identity(&mut store, &mut state, ring);
    let at = RowRot::NONE.advance(4, ring);
    // Window rows 0..=3 at rotation 4 are places 4, 5, 0, 1.
    ix.rotate(at, Row::ZERO..=Row::from_index(3), Rows::of_count(1));
    let o = ix.order();
    let shown: Vec<usize> = (0..6).map(|p| o.slot(p)).collect();
    assert_eq!(shown, vec![1, 4, 2, 3, 5, 0]);
    ix.rotate(at, Row::ZERO..=Row::from_index(3), Rows::of_count(3));
    assert!(ix.order().is_identity());
}

/// **An index at the identity normalises with no swap and no place
/// compared**, and a rotation marks it permuted until a normalise: a
/// rotation and its inverse leave the order at the identity and the state
/// permuted, and the normalise after them swaps nothing and restores it.
///
/// Named break: `rotate` leaves the state at the identity -- the state after
/// the first rotation is not permuted.
#[test]
fn an_index_at_the_identity_normalises_with_no_swap() {
    let ring = Rows::of_count(6);
    let mut store = vec![WindowSlot::UNSET; 12];
    let mut state = SlotState::Permuted;
    let mut ix = SlotIndex::identity(&mut store, &mut state, ring);
    assert_eq!(ix.state(), SlotState::Identity);
    let mut handed = 0;
    assert_eq!(ix.normalise(|_, _| handed += 1), Rows::ZERO);
    assert_eq!(handed, 0);
    let region = Row::ZERO..=Row::from_index(3);
    ix.rotate(RowRot::NONE, region.clone(), Rows::of_count(1));
    assert_eq!(ix.state(), SlotState::Permuted);
    ix.rotate(RowRot::NONE, region, Rows::of_count(3));
    assert!(ix.order().is_identity());
    assert_eq!(ix.normalise(|_, _| handed += 1), Rows::ZERO);
    assert_eq!((handed, ix.state()), (0, SlotState::Identity));
    assert_eq!(SlotIndex::none().state(), SlotState::Identity);
}
