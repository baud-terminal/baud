//! xterm extensions — the ones applications actually depend on.
//!
//! Not DEC protocol, and mostly not in any standard: the alternate screen,
//! mouse reporting, OSC, hyperlinks, bracketed paste, synchronised output and
//! XTVERSION. `docs/protocols/xterm.md` records which are implemented and which
//! are declined, with reasons.

#[path = "../support/mod.rs"]
mod support;

mod alt_screen;
mod embedder_modes;
mod hyperlinks;
mod keypad_version;
mod mouse;
mod osc;
mod soft_reset_scope;

/// One: an `OSC 8` link does not survive a wrap. The index rides in
/// `CellFlags`, so it should travel with the cell -- which makes this a defect
/// in the wrap path rather than in the link table, and the one place the
/// content-keyed design does not yet hold.
#[test]
fn the_known_xterm_gaps() {
    support::ignores_are("xterm", &["hyperlinks.rs"], &["a_link_survives_a_wrap"]);
}
