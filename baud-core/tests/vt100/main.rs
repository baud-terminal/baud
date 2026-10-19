//! VT100 / ANSI — the set every terminal must implement.
//!
//! One module per area rather than one flat file per test, so the protocol a
//! test belongs to is visible from its path: `tests/vt100/scrolling.rs` is
//! VT100 scrolling, and nothing else is.
//!
//! Conformance against the `vt100`, `vt102` and `ansi` terminfo profiles lives
//! in `tests/conformance/` — these are the behavioural tests behind it.

#[path = "../support/mod.rs"]
mod support;

mod c0;
mod charsets;
mod cursor;
mod cuu_cud;
mod erase;
mod erase_qualified;
mod irm;
mod mode_lists;
mod modes;
mod private_modes;
mod save_params;
mod scrolling;
mod tabs;

/// **No gaps, and the gate stays so a new one has to say so.**
///
/// Both closed, and neither was an accessor:
///
/// * **`DECAWM` off left the cursor one past the last column.** The
///   pending-wrap sentinel only makes sense where something will wrap, and
///   with `DECAWM` off nothing will -- so `ICH` inserted its blank off the
///   row. The test asserts on `ICH` rather than on the cursor, because an
///   operation is the only thing that can say where the cursor *really* is.
///   Fixing it found a second bug: `Term::resize` reset the vertical margins
///   and not the side ones, so a resize from 200 columns to 8 left the write
///   path believing it had 192 more than the grid has.
/// * **`?45` reverse wraparound.** Recorded nowhere, so `DECRQM` answered `0`
///   -- correctly, because recording a mode nothing acts on is the failure
///   `docs/vt-coverage.md` rates worst. The bit and the behaviour landed
///   together: a `BS` at the left margin now moves to the right margin of the
///   previous line, bounded by the scrolling region's top rather than by row 0.
#[test]
fn the_known_vt100_gaps() {
    support::ignores_are("vt100", &["cursor.rs", "private_modes.rs"], &[]);
}
