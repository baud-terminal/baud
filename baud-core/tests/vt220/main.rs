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

/// **None, and the gate stays so a new one has to say so.**
///
/// What was here, and what each turned out to be:
///
/// * **C0 and `DEL` inside a DCS prefix.** `Node::Dcs` sent anything
///   unrecognised to `Str`, so a `DEL` between `ESC P` and the `$` turned a
///   `DECRQSS` into a generic control string -- the request dropped and no
///   reply sent, which for a *query* is the worst of the three outcomes.
///   `[[source: williams]]` gives those bytes the *ignore* action: they go
///   nowhere and the state does not move.
/// * **S8C1T.** Marked `done` in the coverage table and implemented nowhere,
///   and found by the conformance inventory rather than by these tests --
///   because a dropped `ESC` changed nothing a snapshot could read until
///   something counted it.
/// * **CPR at the pending-wrap column.** It reported the sentinel one past the
///   right margin: a column the application cannot address, off the end of its
///   own row. The same clamp `Term::cursor` carries.
/// * **DECSC and DECSCA.** `Saved` carried `flags` and not `protect`, which
///   are different words -- the first is what the write path stamps into a
///   cell, the second is what `DECSCA` sets for the *next* write. A `DECRC`
///   restored the rendition and left the protection wherever the stream had
///   since moved it, so a selective erase after a restore spared the wrong
///   cells.
#[test]
fn the_known_vt220_gaps() {
    support::ignores_are(
        "vt220",
        &["dcs.rs", "replies.rs", "selective_erase.rs"],
        &[],
    );
}
