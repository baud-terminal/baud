//! What the two post-ASCII cluster states differ in, pinned.
//!
//! `put_run` resets `ClusterBreak` after a blitted ASCII run, which says
//! `prev = None`. Printable ASCII is class `Other`, so the truthful state is
//! `after_ascii_run`. They are observationally equivalent at the terminal
//! level and the reset is 1.5% cheaper, which is a claim worth being able to
//! check rather than re-deriving from the comment.

use baud_domain::{ClusterBreak, RunBlocker};

/// The one thing that differs, named. Everything else about the two states is
/// identical, which is why substituting one for the other changes no output.
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

/// Neither state carries a lookback, which is the half that makes the
/// substitution safe: GB11 needs a pictographic before the joiner, GB9c needs
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
