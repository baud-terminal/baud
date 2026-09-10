//! VT220 / VT320 — selective erase, status reports, and device control strings.
//!
//! What VT100 applications do not need but real ones do: DECSCA and the
//! selective erases, DECRQM, DECSCUSR, and the DCS layer carrying DECRQSS.

#[path = "../support/mod.rs"]
mod support;

mod dcs;
mod decrqm_cursor_style;
mod replies;
mod selective_erase;

/// Three gaps, and only the last is a defect rather than a missing feature.
///
/// **`S8C1T` closed and it was not found by these tests.** The conformance
/// inventory found it: `docs/vt-coverage.md` marked `ESC SP F` and `ESC SP G`
/// **done** while this core implemented neither, and nothing could tell,
/// because a dropped `ESC` changed nothing a snapshot reads. Counting it is
/// what made "implemented" and "silently discarded" different states.
///
/// * **C0 and `DEL` inside a DCS prefix.** `Node::Dcs` and `Node::DcsDollar`
///   route `$` and `q` and send everything else to `Str`, so a `DEL` between
///   the introducer and the `$` discards a sequence that should survive it.
/// * **CPR at the pending-wrap column.** The report should name the last
///   column, not the sentinel one past it -- the same clamp `Term::cursor`
///   grew when the cursor differential found it, not yet applied to the reply.
/// * **DECSC/DECRC and DECSCA.** `Saved` carries the charset slots but not the
///   protection, so a restore drops it.
#[test]
fn the_known_vt220_gaps() {
    support::ignores_are(
        "vt220",
        &["dcs.rs", "replies.rs", "selective_erase.rs"],
        &[
            "a_del_in_the_dcs_prefix_does_not_discard_it",
            "a_c0_in_the_dcs_prefix_does_not_discard_it",
            "cpr_clamps_the_pending_wrap_sentinel",
            "decsc_and_decrc_carry_the_protection",
        ],
    );
}
