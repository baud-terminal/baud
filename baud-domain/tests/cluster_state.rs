//! What the two post-ASCII cluster states differ in, pinned.
//!
//! `Paint::piece` and `Paint::blit` reset `ClusterBreak` for a run of
//! printable ASCII, which says `prev = None`. Printable ASCII is class
//! `Other`, so the truthful state is `after_ascii_run`. `baud-core2` measured
//! the reset 1.5% cheaper and argued the two observationally equivalent at
//! the terminal level. GB9's `× (Extend | ZWJ)` and GB9a's `× SpacingMark`
//! read `prev`, so they are not: a continuation that is not zero-width, a
//! SpacingMark or an Extend with a width, splits from the ASCII before it
//! (`ClusterBreak::after_ascii_run`'s doc has the cases). What differs is
//! worth being able to check rather than re-deriving from the comment.

use baud_domain::{ClusterBreak, RunBlocker};

/// The one thing that differs, named: `prev`. Everything else about the two
/// states is identical. `prev` is also what GB9 and GB9a see past GB1, so
/// substituting one for the other moves a continuation after ASCII that is
/// not zero-width -- a SpacingMark, or an Extend with a width -- between a
/// cell of its own and the cell before it.
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
