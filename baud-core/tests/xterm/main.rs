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
mod focus;
mod hyperlinks;
mod keypad_version;
mod mouse;
mod osc;
mod soft_reset_scope;

/// **No gaps, and there never was one here.**
///
/// This pinned "an `OSC 8` link does not survive a wrap" on the strength of a
/// failing test -- and the test was reading index **2048**, core2's row stride.
/// This core strides by the live column count, which is worth +14% on anything
/// that scrolls, so the assertion was inspecting a cell no addressing path can
/// reach. The link had survived the wrap the whole time, because the index
/// rides in `CellFlags` and therefore moves with the cell.
///
/// **Fifth instance of that porting bug**, after `erase.rs`, `irm.rs`,
/// `sgr/rep_align.rs` and `vt420/side_margins.rs` -- and the first where it
/// cost a *diagnosis* rather than a red test: the pin, and the paragraph in
/// `CLAUDE.md` explaining that the wrap path was at fault, were both written
/// from it. A number read off the wrong cell is a finding about the harness.
#[test]
fn the_known_xterm_gaps() {
    support::ignores_are("xterm", &["hyperlinks.rs"], &[]);
}
