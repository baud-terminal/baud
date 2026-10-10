//! What the two post-ASCII cluster states differ in, pinned.
//!
//! `Paint::piece` and `Paint::blit` write a run of printable ASCII without
//! asking `ClusterBreak`, and tell it what went past. Until 2026-10-10 they
//! told it `new()`, which says `prev = None`; printable ASCII is class
//! `Other`, so the truthful state is `after_ascii_run`, and that is what they
//! tell it now. `baud-core2` measured the reset 1.5% cheaper and argued the
//! two observationally equivalent at the terminal level. GB9's
//! `× (Extend | ZWJ)` and GB9a's `× SpacingMark` read `prev`, so they are
//! not: under the reset a continuation that is not zero-width, a SpacingMark
//! or an Extend with a width, split from the ASCII before it
//! (`ClusterBreak::after_ascii_run`'s doc has the cases, and
//! `every_line_of_the_suite_clusters_through_the_terminal` holds them). What
//! differs is worth being able to check rather than re-deriving from the
//! comment.

use baud_domain::{ClusterBreak, GraphemeClass, Lookback, RunBlocker, cluster_next};

/// The one thing that differs, named: `prev`. Everything else about the two
/// states is identical. `prev` is also what GB9 and GB9a see past GB1, so
/// substituting one for the other moves a continuation after ASCII that is
/// not zero-width -- a SpacingMark, or an Extend with a width -- between a
/// cell of its own and the cell before it, which is why the terminal tells
/// the machine the truthful one.
#[test]
fn the_two_post_ascii_states_differ_only_in_the_bulk_precondition() {
    let reset = ClusterBreak::new();
    let truthful = ClusterBreak::after_ascii_run();

    assert!(
        !reset.ordinary_run_ready(),
        "the reset says `prev = None`, so the bulk collector cannot start"
    );
    assert!(
        truthful.ordinary_run_ready(),
        "printable ASCII is class Other, so the truthful state can"
    );
    assert_eq!(
        reset.ordinary_run_blocker(),
        RunBlocker::Prev,
        "and the only thing blocking the reset is `prev`"
    );
    assert_eq!(truthful.ordinary_run_blocker(), RunBlocker::Ready);
}

/// Neither state carries a lookback, which is the half of the equivalence
/// argument that holds: GB11 needs a pictographic before the joiner, GB9c needs
/// an `InCB` consonant, and GB12/13 need an odd regional-indicator count.
/// An ASCII character is none of those, so both agree.
#[test]
fn neither_post_ascii_state_carries_a_lookback() {
    for (name, s) in [
        ("reset", ClusterBreak::new()),
        ("truthful", ClusterBreak::after_ascii_run()),
    ] {
        assert!(
            matches!(
                s.ordinary_run_blocker(),
                RunBlocker::Ready | RunBlocker::Prev
            ),
            "{name} reports a lookback it cannot have after an ASCII character"
        );
    }
}

/// **After a control the machine is where it is before any text**, on every
/// input: `ClusterBreak::after_control` is `new()`, and this is why that is
/// truthful rather than close.
///
/// The table: feed a Control to state 0, which is `new()`, and the row it
/// lands on answers every one of the 256 inputs -- a class and its lookback
/// bits -- with the same break bit and the same successor as row 0. The
/// rules: from `new()` and from `new()` fed a Control, every class with every
/// lookback breaks and leaves the same nine bits. The two state ids differ,
/// so the check has something to find.
#[test]
fn the_state_after_a_control_is_the_state_before_any_text() {
    // A class's half of the table's input byte is its place in
    // `GraphemeClass::FROM_INDEX`, which is what the trie stores.
    let index = |c: GraphemeClass| {
        let i = GraphemeClass::FROM_INDEX.iter().position(|k| *k == c);
        u8::try_from(i.expect("every class has an index")).expect("sixteen classes")
    };
    let control = index(GraphemeClass::Control);
    let after = cluster_next(0, control);
    assert!(
        after & 0x80 != 0,
        "GB1: the control itself starts a cluster"
    );
    let after = after & 0x7f;
    assert_ne!(after, 0, "a control lands on a state id of its own");
    for input in 0..=255u8 {
        assert_eq!(
            cluster_next(0, input),
            cluster_next(after, input),
            "input {input:#04x}: the row after a control answers otherwise"
        );
    }
    for cur in GraphemeClass::FROM_INDEX {
        for lb in 0..16u8 {
            let lb = Lookback::from_bits(lb);
            let mut fresh = ClusterBreak::new();
            let mut ctl = ClusterBreak::new();
            assert!(ctl.is_break_by_rules(GraphemeClass::Control, Lookback::from_bits(0)));
            let (a, b) = (
                fresh.is_break_by_rules(cur, lb),
                ctl.is_break_by_rules(cur, lb),
            );
            assert!(a && b, "{cur:?}: GB1 and GB4 both break");
            assert_eq!(
                fresh.to_bits(),
                ctl.to_bits(),
                "{cur:?}: the rules leave another state"
            );
        }
    }
    assert_eq!(
        ClusterBreak::after_control().to_bits(),
        ClusterBreak::new().to_bits()
    );
}

/// **Where a cluster breaks, the state after it does not depend on the state
/// before it**: from every state text can reach, on every input a codepoint
/// can be, a transition that breaks goes where state 0 -- `new()`, and so the
/// state after a control -- goes on that input.
///
/// This is what lets a carriage return tell the machine nothing. After it the
/// cursor is at column 0, where `glyph` writes the next character in a cell
/// of its own whatever the machine says. If the machine breaks there, this
/// test is why its successor is the one a control would have left; if it does
/// not, `glyph` starts it again from `after_control` and asks once more.
///
/// **Only what text can reach.** The table answers all 256 inputs from all 94
/// states, and an input no codepoint is -- a CR with a pictographic lookback
/// -- leads to states that remember it; walked over every input, state 16 is
/// one. So the inputs are every scalar value's class and lookback, and the
/// states are those reachable from 0 on them: 18 inputs, 24 states and 307
/// breaking transitions.
#[test]
fn a_break_forgets_the_state_before_it() {
    let index = |c: GraphemeClass| {
        let i = GraphemeClass::FROM_INDEX.iter().position(|k| *k == c);
        u8::try_from(i.expect("every class has an index")).expect("sixteen classes")
    };
    let mut inputs = std::collections::BTreeSet::new();
    for v in (0..=0x10ffffu32).filter(|v| !(0xd800..=0xdfff).contains(v)) {
        let (class, lb) = GraphemeClass::of_with_lookback(
            baud_domain::Codepoint::new(v).expect("a scalar value"),
        );
        inputs.insert(index(class) | (lb.bits() << 4));
    }
    let mut seen = [false; 24064 / 256];
    let mut todo = vec![0u8];
    seen[0] = true;
    let mut breaks = 0usize;
    while let Some(s) = todo.pop() {
        for &input in &inputs {
            let e = cluster_next(s, input);
            if e & 0x80 != 0 {
                breaks += 1;
                assert_eq!(
                    e & 0x7f,
                    cluster_next(0, input) & 0x7f,
                    "state {s}, input {input:#04x}: the successor of a break remembers the state"
                );
            }
            let n = usize::from(e & 0x7f);
            if !seen[n] {
                seen[n] = true;
                todo.push(e & 0x7f);
            }
        }
    }
    let reached = seen.iter().filter(|s| **s).count();
    assert!(
        inputs.len() > 14 && reached > 14 && breaks > reached * 4,
        "{} inputs, {reached} states, {breaks} breaks: the check found little to check",
        inputs.len()
    );
}
