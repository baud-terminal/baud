//! **The property the run gate rests on**, checked against the shipped table
//! rather than argued from the rules.
//!
//! A bulk writer writes cells and asks the cluster machine nothing, so a run
//! may only reach one if the machine can be advanced across it *without*
//! walking it. That is possible exactly when the run's characters leave the
//! machine in a state that does not depend on where it started -- and for
//! class `Other` with no lookback it does not: an ordinary character ends any
//! regional-indicator run, clears the `Extended_Pictographic` lookback GB11
//! needs and clears the Indic lookback GB9c needs.
//!
//! That is a claim about `CLUSTER_NEXT`, so it is checked against
//! `CLUSTER_NEXT` for **every** reachable state rather than reasoned about.
use baud_domain::*;

/// The input byte for `(GraphemeClass::Other, no lookback)`: the class in the
/// low nibble, the lookback bits in the high one.
const ORDINARY: u8 = 0;

fn reachable() -> Vec<u8> {
    let mut seen = [false; 256];
    let mut stack = vec![0u8];
    while let Some(s) = stack.pop() {
        if seen[s as usize] {
            continue;
        }
        seen[s as usize] = true;
        for i in 0..=255u8 {
            let n = cluster_next(s, i) & 0x7f;
            if !seen[n as usize] {
                stack.push(n);
            }
        }
    }
    (0..=255u8).filter(|s| seen[*s as usize]).collect()
}

/// **One ordinary character lands the machine in one state, from all 94.**
///
/// This is what lets `put_run` hand a whole run to a bulk writer after asking
/// the machine about the *first* character only: characters two onward start
/// from a state that is the same whatever preceded the run, so their answers
/// cannot depend on it.
#[test]
fn one_ordinary_character_lands_in_one_state_from_every_state() {
    let states = reachable();
    assert_eq!(states.len(), 94, "the reachable state count moved");
    let landed: Vec<u8> = states
        .iter()
        .map(|s| cluster_next(*s, ORDINARY) & 0x7f)
        .collect();
    let mut uniq = landed.clone();
    uniq.sort_unstable();
    uniq.dedup();
    assert_eq!(
        uniq.len(),
        1,
        "an ordinary character does not converge: {uniq:?}",
    );

    // And that one state is the one the gate accepts, checked through the
    // public predicate rather than against a number.
    let mut m = ClusterBreak::new();
    m.is_break_classified(GraphemeClass::Other, Lookback::from_bits(0));
    assert!(
        m.ordinary_run_ready(),
        "the converged state is not the one `ordinary_run_ready` accepts",
    );
}

/// **From that state an ordinary character always breaks**, which is the other
/// half: it is what makes every character after the first in an ordinary run
/// its own cluster, and so its own cell.
#[test]
fn an_ordinary_character_always_breaks_from_the_converged_state() {
    // The converged state, taken from the table rather than from `to_bits`,
    // which is the *lookback* encoding and not the table id.
    let ready = cluster_next(0, ORDINARY) & 0x7f;
    assert!(
        cluster_next(ready, ORDINARY) & 0x80 != 0,
        "an ordinary character continues a cluster from the ready state",
    );
}
