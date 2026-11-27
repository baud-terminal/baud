//! `IntervalMap`: sorted, disjoint and coalesced whatever is assigned,
//! erased, kept or moved, each line holding what a per-line `BTreeMap`
//! holds; a line two values' moved runs both cover holds neither; and its
//! walks are counted.

use std::collections::{BTreeMap, BTreeSet};

use baud_domain::{
    IntervalMap, LineIdx, LineInterval, LineStep, OverlapRuns, RemapOverlap, WalkSteps,
};
use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::TestRunner;

/// The last line of the test's lines: few enough that intervals overlap
/// and touch often, and both ends are reached.
const LAST: u8 = 31;

/// A line from 0 to [`LAST`], with no line past either end.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct L(u8);

impl LineStep for L {
    fn next(self) -> Option<L> {
        (self.0 < LAST).then(|| L(self.0 + 1))
    }
    fn prev(self) -> Option<L> {
        self.0.checked_sub(1).map(L)
    }
}

fn iv(a: u8, b: u8) -> LineInterval<L> {
    LineInterval::new(L(a), L(b))
}

/// Where a remap sends a run of lines.
#[derive(Clone, Copy, Debug)]
enum Move {
    /// Every line `by` rows on, held at the ends.
    Shift(i8),
    /// A run meeting the band takes the band's hull.
    Hull(u8, u8),
    /// A run inside the band moves `by` rows, held inside it; a run
    /// meeting it otherwise takes its hull.
    Band(u8, u8, i8),
}

impl Move {
    fn apply(self, i: LineInterval<L>) -> LineInterval<L> {
        let (f, l) = (i.first().0, i.last().0);
        let shift = |x: u8, by: i8, lo: u8, hi: u8| {
            let to = (i16::from(x) + i16::from(by)).clamp(i16::from(lo), i16::from(hi));
            u8::try_from(to).expect("held within the lines")
        };
        match self {
            Move::Shift(by) => iv(shift(f, by, 0, LAST), shift(l, by, 0, LAST)),
            Move::Hull(a, b) => {
                let (a, b) = (a.min(b), a.max(b));
                match f <= b && a <= l {
                    true => iv(f.min(a), l.max(b)),
                    false => i,
                }
            }
            Move::Band(a, b, by) => {
                let (a, b) = (a.min(b), a.max(b));
                match (a <= f && l <= b, f <= b && a <= l) {
                    (true, _) => iv(shift(f, by, a, b), shift(l, by, a, b)),
                    (false, true) => iv(f.min(a), l.max(b)),
                    (false, false) => i,
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Op {
    Assign(LineInterval<L>, u8),
    Erase(LineInterval<L>),
    KeepFrom(u8),
    Remap(Move),
    Clear,
    Gaps(u8, LineInterval<L>),
}

/// One line, any two ends, every line, and the lines up to or from an end.
fn interval() -> impl Strategy<Value = LineInterval<L>> {
    prop_oneof![
        (0..=LAST).prop_map(|a| iv(a, a)),
        (0..=LAST, 0..=LAST).prop_map(|(a, b)| iv(a, b)),
        Just(iv(0, LAST)),
        (0..=LAST).prop_map(|b| iv(0, b)),
        (0..=LAST).prop_map(|a| iv(a, LAST)),
    ]
}

fn moves() -> impl Strategy<Value = Move> {
    prop_oneof![
        (-6i8..=6).prop_map(Move::Shift),
        (0..=LAST, 0..=LAST).prop_map(|(a, b)| Move::Hull(a, b)),
        (0..=LAST, 0..=LAST, -4i8..=4).prop_map(|(a, b, by)| Move::Band(a, b, by)),
    ]
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        6 => (interval(), 0u8..3).prop_map(|(i, v)| Op::Assign(i, v)),
        2 => interval().prop_map(Op::Erase),
        1 => (0..=LAST).prop_map(Op::KeepFrom),
        2 => moves().prop_map(Op::Remap),
        1 => Just(Op::Clear),
        2 => (0u8..3, interval()).prop_map(|(v, i)| Op::Gaps(v, i)),
    ]
}

fn program() -> impl Strategy<Value = Vec<Op>> {
    proptest::collection::vec(op(), 0..40)
}

/// The per-line model: each line's value.
type Model = BTreeMap<u8, u8>;

/// The model's maximal runs of one value on consecutive lines.
fn runs_of(m: &Model) -> Vec<(LineInterval<L>, u8)> {
    let mut out: Vec<(u8, u8, u8)> = Vec::new();
    for (&line, &v) in m {
        match out.last_mut() {
            Some((_, last, w)) if *w == v && *last + 1 == line => *last = line,
            _ => out.push((line, line, v)),
        }
    }
    out.into_iter().map(|(a, b, v)| (iv(a, b), v)).collect()
}

fn lines(i: LineInterval<L>) -> impl Iterator<Item = u8> {
    i.first().0..=i.last().0
}

/// The model moved: each run to where `to` sends it, a line two values
/// land on dropped, and the maximal runs of such lines counted.
fn remapped(m: &Model, to: Move) -> (Model, u64) {
    let mut cover: BTreeMap<u8, Vec<u8>> = BTreeMap::new();
    for (i, v) in runs_of(m) {
        for line in lines(to.apply(i)) {
            let at = cover.entry(line).or_default();
            if !at.contains(&v) {
                at.push(v);
            }
        }
    }
    let mut out = Model::new();
    let mut both: Vec<u8> = Vec::new();
    for (line, vs) in cover {
        match vs.as_slice() {
            [v] => {
                out.insert(line, *v);
            }
            _ => both.push(line),
        }
    }
    let runs = both
        .iter()
        .enumerate()
        .filter(|&(k, &line)| k == 0 || both[k - 1] + 1 != line)
        .count();
    (out, u64::try_from(runs).expect("a small count"))
}

/// The lines of `span` the model does not hold `v` on, in runs.
fn gaps_in(m: &Model, v: u8, span: LineInterval<L>) -> Vec<LineInterval<L>> {
    let mut out: Vec<(u8, u8)> = Vec::new();
    for line in lines(span).filter(|l| m.get(l) != Some(&v)) {
        match out.last_mut() {
            Some((_, last)) if *last + 1 == line => *last = line,
            _ => out.push((line, line)),
        }
    }
    out.into_iter().map(|(a, b)| iv(a, b)).collect()
}

/// Sorted, disjoint, and no two touching runs of one value.
fn well_formed(m: &IntervalMap<L, u8>) -> bool {
    let runs: Vec<_> = m.runs().collect();
    runs.windows(2).all(|w| {
        let ((a, va), (b, vb)) = (w[0], w[1]);
        a.last() < b.first() && !(va == vb && a.last().next() == Some(b.first()))
    })
}

/// A shape an op of a program can take, which the reach test requires the
/// generator to make.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Shape {
    /// An interval over some of the lines held and not all of its own.
    Overlapping,
    /// An interval with a held line just above or just below it.
    Adjacent,
    /// An op on a map that holds nothing.
    OnEmpty,
    /// The interval of every line.
    FullCover,
    /// A remap that landed two values on one line.
    RemapOverlap,
    /// Gaps that are neither none nor the whole range.
    Gaps,
}

const SHAPES: [Shape; 6] = [
    Shape::Overlapping,
    Shape::Adjacent,
    Shape::OnEmpty,
    Shape::FullCover,
    Shape::RemapOverlap,
    Shape::Gaps,
];

/// What each op of a program does to the map and to the model, checked
/// after every op. Returns the shapes the program reached, for the reach
/// test.
fn run(program: &[Op]) -> Result<BTreeSet<Shape>, TestCaseError> {
    let mut map = IntervalMap::<L, u8>::new();
    let mut model = Model::new();
    let mut reached = BTreeSet::new();
    let mut steps = WalkSteps::ZERO;
    for &op in program {
        let span = match op {
            Op::Assign(i, _) | Op::Erase(i) | Op::Gaps(_, i) => Some(i),
            _ => None,
        };
        if let Some(i) = span {
            let held = lines(i).filter(|l| model.contains_key(l)).count();
            let all = usize::from(i.last().0 - i.first().0) + 1;
            let before = i.first().prev().is_some_and(|p| model.contains_key(&p.0));
            let after = i.last().next().is_some_and(|n| model.contains_key(&n.0));
            for (shape, met) in [
                (Shape::Overlapping, held > 0 && held < all),
                (Shape::Adjacent, before || after),
                (Shape::OnEmpty, model.is_empty()),
                (Shape::FullCover, i == iv(0, LAST)),
            ] {
                if met {
                    reached.insert(shape);
                }
            }
        }
        match op {
            Op::Assign(i, v) => {
                map.assign(i, v, &mut steps);
                for line in lines(i) {
                    model.insert(line, v);
                }
            }
            Op::Erase(i) => {
                map.erase(i, &mut steps);
                for line in lines(i) {
                    model.remove(&line);
                }
            }
            Op::KeepFrom(k) => {
                map.keep_from(L(k), &mut steps);
                model.retain(|&line, _| line >= k);
            }
            Op::Remap(to) => {
                let got: OverlapRuns =
                    map.remap(|i| to.apply(i), RemapOverlap::Neither, &mut steps);
                let (next, runs) = remapped(&model, to);
                model = next;
                prop_assert_eq!(got.get(), runs, "runs left valueless by {:?}", to);
                if runs > 0 {
                    reached.insert(Shape::RemapOverlap);
                }
            }
            Op::Clear => {
                map.clear();
                model.clear();
            }
            Op::Gaps(v, i) => {
                let mut got = Vec::new();
                map.gaps_of(v, i, &mut got, &mut steps);
                let want = gaps_in(&model, v, i);
                if !want.is_empty() && want != [i] {
                    reached.insert(Shape::Gaps);
                }
                prop_assert_eq!(got, want, "gaps of {} in {:?}", v, i);
            }
        }
        prop_assert!(well_formed(&map), "after {:?}: {:?}", op, map);
        prop_assert_eq!(
            map.runs().collect::<Vec<_>>(),
            runs_of(&model),
            "after {:?}",
            op
        );
        prop_assert_eq!(map.is_empty(), model.is_empty());
    }
    Ok(reached)
}

proptest! {
    /// **Every op leaves each line holding what a per-line `BTreeMap`
    /// holds, the runs sorted, disjoint and coalesced**: assign, erase,
    /// keep_from, remap under `Neither` and clear, with `gaps_of` against
    /// the model's gaps and each remap's `OverlapRuns` against the model's
    /// count of maximal runs of lines two values landed on. Named break:
    /// an assign that does not join a touching run of its own value -- the
    /// map holds two runs where the model's coalesced form holds one.
    #[test]
    fn every_op_holds_what_a_per_line_map_holds(program in program()) {
        run(&program)?;
    }
}

/// **The generator reaches every shape the map's edges live at**: an
/// interval over part of what is held, one touching a held line at
/// either end, an op on an empty map, the full cover, a remap that lands
/// two values on one line, and gaps that are neither none nor the whole
/// range, each in at least one of 2,000 programs drawn from a fixed seed.
/// Named break: intervals of one line only -- `Overlapping`, `FullCover`
/// and `Gaps` are never reached.
#[test]
fn the_generator_reaches_overlapping_adjacent_empty_and_full_cover() {
    let mut runner = TestRunner::deterministic();
    let mut reached = BTreeSet::new();
    for _ in 0..2000 {
        let p = program()
            .new_tree(&mut runner)
            .expect("a program")
            .current();
        reached.extend(run(&p).expect("the property holds"));
    }
    let missing: Vec<_> = SHAPES.iter().filter(|s| !reached.contains(s)).collect();
    assert!(missing.is_empty(), "no program reached {missing:?}");
}

/// [`RUNS`] one-line runs on lines 0 to `RUNS - 1`, values 1 and 2 in
/// turn.
fn alternating() -> IntervalMap<LineIdx, u8> {
    let mut m = IntervalMap::new();
    let mut steps = WalkSteps::ZERO;
    for k in 0..RUNS {
        let v = [1, 2][usize::from(k % 2 == 1)];
        m.assign(LineInterval::one(LineIdx::of(k)), v, &mut steps);
    }
    m
}

/// `⌈log2 (RUNS + 1)⌉`: the compares a binary search over [`RUNS`] runs
/// makes.
const SEARCH: u32 = u64::BITS - RUNS.leading_zeros();

const RUNS: u64 = 1024;

/// **An assign over one line steps by log** (`IntervalMap::assign`): over
/// 1,024 one-line runs, a line in the middle given the value of its
/// neighbours joins three runs into one, within a binary search and the
/// four runs that meet or touch it. The control: the runs are 1,022, the
/// three joined. Named break: an assign that finds its runs by walking from
/// the first -- about 512 steps.
#[test]
fn an_assign_on_one_line_steps_by_log() {
    let mut m = alternating();
    let mut steps = WalkSteps::ZERO;
    let line = LineIdx::of(RUNS / 2 + 1);
    m.assign(LineInterval::one(line), 1, &mut steps);
    let most = u64::from(SEARCH) + 4;
    assert!(
        steps.get() <= most,
        "an assign over one line took {} steps, against the bound {most}",
        steps.get()
    );
    assert_eq!(m.runs().count(), 1022, "the control: three runs joined");
    let mut steps = WalkSteps::ZERO;
    m.erase(LineInterval::one(LineIdx::of(3)), &mut steps);
    assert!(
        steps.get() <= most,
        "an erase of one line took {} steps, against the bound {most}",
        steps.get()
    );
}

/// **The gaps of one value on one line step by log**
/// (`IntervalMap::gaps_of`): over 1,024 one-line runs, the line the value
/// holds gives no gap and the line it does not gives that line, each
/// within a binary search and the two runs that meet it. Named break: a
/// `gaps_of` that walks every run -- 1,024 steps.
#[test]
fn the_gaps_of_one_line_step_by_log() {
    let m = alternating();
    let most = u64::from(SEARCH) + 2;
    for (k, want) in [(RUNS / 2, 0), (RUNS / 2 + 1, 1)] {
        let mut steps = WalkSteps::ZERO;
        let mut out: Vec<LineInterval<LineIdx>> = Vec::new();
        let line = LineInterval::one(LineIdx::of(k));
        m.gaps_of(1, line, &mut out, &mut steps);
        assert_eq!(out.len(), want, "the control: line {k}");
        assert!(
            steps.get() <= most,
            "the gaps of line {k} took {} steps, against the bound {most}",
            steps.get()
        );
    }
}

/// **A keep from a line drains a prefix by log** (`IntervalMap::keep_from`):
/// over 1,024 one-line runs, keeping from the first line drops nothing
/// in one compare, and keeping from line 100 drops the 100 runs above
/// it, within that compare, a binary search and those runs. Named break: a keep
/// that looks at every run -- 1,024 steps.
#[test]
fn a_keep_from_a_line_drains_a_prefix_by_log() {
    let mut m = alternating();
    let mut steps = WalkSteps::ZERO;
    m.keep_from(LineIdx::FIRST, &mut steps);
    assert_eq!(
        m.runs().count(),
        usize::try_from(RUNS).expect("small"),
        "kept"
    );
    assert!(
        steps.get() <= 1,
        "an unchanged keep took {} steps",
        steps.get()
    );
    let mut steps = WalkSteps::ZERO;
    m.keep_from(LineIdx::of(100), &mut steps);
    assert_eq!(m.runs().next().map(|r| r.0.first()), Some(LineIdx::of(100)));
    assert!(
        steps.get() <= 1 + u64::from(SEARCH) + 100,
        "a keep that drops 100 runs took {} steps",
        steps.get()
    );
}

/// **A line two values' moved runs cover holds neither**
/// (`IntervalMap::remap`, `RemapOverlap::Neither`): value 1 on lines 2-3
/// and value 2 on line 5, the band 2 to 5 taken by each, leaves no run and
/// one run counted; value 1 on two runs landing over each other is one
/// run of value 1, none counted. Named break: the later run written over
/// the earlier -- line 2 to 5 holds 2.
#[test]
fn a_line_two_values_moved_runs_cover_holds_neither() {
    let at = |a, b| LineInterval::new(LineIdx::of(a), LineIdx::of(b));
    let band = |i: LineInterval<LineIdx>| match i.first() <= LineIdx::of(5) {
        true => at(2, 5),
        false => i,
    };
    let mut steps = WalkSteps::ZERO;
    let mut m = IntervalMap::<LineIdx, u8>::new();
    m.assign(at(2, 3), 1, &mut steps);
    m.assign(at(5, 5), 2, &mut steps);
    m.assign(at(9, 9), 2, &mut steps);
    let both = m.remap(band, RemapOverlap::Neither, &mut steps);
    assert_eq!(both.get(), 1);
    assert_eq!(m.runs().collect::<Vec<_>>(), [(at(9, 9), 2)]);
    let mut m = IntervalMap::<LineIdx, u8>::new();
    m.assign(at(2, 3), 1, &mut steps);
    m.assign(at(5, 5), 1, &mut steps);
    let one = m.remap(band, RemapOverlap::Neither, &mut steps);
    assert_eq!(one.get(), 0, "the control: one value");
    assert_eq!(m.runs().collect::<Vec<_>>(), [(at(2, 5), 1)]);
}

/// **A line steps to its neighbours and stops at the ends** (`LineStep`
/// for `LineIdx`): no line before the first and none after the last.
/// Named break: a saturating step -- the first line's line before is
/// itself.
#[test]
fn a_line_steps_to_its_neighbours_and_stops_at_the_ends() {
    assert_eq!(LineStep::prev(LineIdx::FIRST), None);
    assert_eq!(LineStep::next(LineIdx::of(u64::MAX)), None);
    assert_eq!(LineStep::next(LineIdx::of(4)), Some(LineIdx::of(5)));
    assert_eq!(LineStep::prev(LineIdx::of(4)), Some(LineIdx::of(3)));
}
