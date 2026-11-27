//! The cluster machine's two forms, stepped side by side.
//!
//! `ClusterBreak` is the table id the terminal runs and `ClusterRules` the
//! UAX #29 rules over four fields that the table is generated from. Until
//! 2026-10-16 they were one struct, each public method moved one half, and
//! a machine could answer one way by its table and another by its fields:
//! on `a43d9455`, 6977 of `from_bits`'s answers over the reachable states
//! and 228 of the constructors stepped once by the table disagreed. Now
//! each form answers only from itself, and this holds them to each other.

use baud_domain::{ClusterBreak, ClusterRules, GraphemeClass, Lookback, RunBlocker};
use std::collections::HashSet;

/// The fourteen classes the table has rows for, each with all sixteen
/// lookbacks: every input the emitter walks.
fn inputs() -> Vec<(GraphemeClass, Lookback)> {
    GraphemeClass::FROM_INDEX
        .into_iter()
        .take(14)
        .flat_map(|c| (0u8..16).map(move |l| (c, Lookback::from_bits(l))))
        .collect()
}

/// From every pair of matching constructors, every pair the inputs reach
/// agrees on the ready test and, on every input, on the break and on the
/// ready test after it. Each rule state pairs with exactly one table
/// machine, so the pairing is the table's own numbering and not a
/// coincidence of two walks: 94 rule states, 94 pairs.
#[test]
fn the_two_forms_agree_in_lockstep_from_every_constructor() {
    let starts = [
        ("new", ClusterBreak::new(), ClusterRules::new()),
        ("default", ClusterBreak::default(), ClusterRules::default()),
        (
            "after_control",
            ClusterBreak::after_control(),
            ClusterRules::new(),
        ),
        (
            "after_ascii_run",
            ClusterBreak::after_ascii_run(),
            ClusterRules::after_ascii_run(),
        ),
    ];
    let inputs = inputs();
    let mut pairs: HashSet<(ClusterBreak, u16)> = HashSet::new();
    let mut rules: HashSet<u16> = HashSet::new();
    let mut todo = Vec::new();
    for (name, t, r) in starts {
        if pairs.insert((t, r.to_bits())) {
            todo.push((name, t, r));
        }
    }
    let mut steps = 0usize;
    while let Some((name, t, r)) = todo.pop() {
        rules.insert(r.to_bits());
        assert_eq!(
            ClusterRules::from_bits(r.to_bits()),
            r,
            "from {name}: from_bits does not invert to_bits"
        );
        assert_eq!(
            t.ordinary_run_ready(),
            r.ordinary_run_blocker() == RunBlocker::Ready,
            "from {name}: rule state {:#05x} is ready one way by the table, another by the rules",
            r.to_bits()
        );
        for &(c, l) in &inputs {
            let (mut tn, mut rn) = (t, r);
            assert_eq!(
                tn.is_break_classified(c, l),
                rn.is_break_by_rules(c, l),
                "from {name}: rule state {:#05x}, {c:?} {:#x}: the break differs",
                r.to_bits(),
                l.bits()
            );
            steps += 1;
            if pairs.insert((tn, rn.to_bits())) {
                todo.push((name, tn, rn));
            }
        }
    }
    assert_eq!(rules.len(), 94, "the reachable rule states moved");
    assert_eq!(
        pairs.len(),
        rules.len(),
        "a rule state pairs with more than one table machine"
    );
    assert!(steps > 20_000, "only {steps} steps checked");
}
