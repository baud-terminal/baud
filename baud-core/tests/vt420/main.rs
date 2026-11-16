//! VT420 — left and right margins, and the column operations that ride on them.
//!
//! DECSLRM, DECIC and DECDC. Small enough to be one module today; it is its own
//! directory because the protocol level is, and because `CSI s` is ambiguous
//! between SCOSC and DECSLRM in a way that belongs filed under VT420.

#[path = "../support/mod.rs"]
mod support;

mod corner;
mod decaln;
mod erase_colour;
mod index_columns;
mod line_starts;
mod locator;
mod origin;
mod presentation;
mod rect_ops;
mod rect_scroll;
mod reports;
mod side_margins;
mod state_reports;
mod status_line;
mod tab_backspace;
mod upss;

/// **No gaps, and the gate stays so that a new one has to say so.**
///
/// Left/right margins were the last, and they were a *feature* rather than an
/// accessor: `DECSLRM` is meaningless without `DECLRMM`, and wrapping, `DCH`,
/// `ICH`, `DECIC` and `DECDC` and the cursor report all have to consult the
/// margins -- which is why recording them without acting would have been the
/// worse half. Seven tests were pinned on it and all seven pass.
///
/// An empty list is the strongest form this gate takes: any `#[ignore]` added
/// to this suite fails it, and there is nothing left to close.
#[test]
fn the_known_vt420_gaps() {
    support::ignores_are(
        "vt420",
        &[
            "corner.rs",
            "decaln.rs",
            "index_columns.rs",
            "line_starts.rs",
            "origin.rs",
            "presentation.rs",
            "rect_ops.rs",
            "rect_scroll.rs",
            "reports.rs",
            "side_margins.rs",
            "status_line.rs",
            "tab_backspace.rs",
            "upss.rs",
        ],
        &[],
    );
}
