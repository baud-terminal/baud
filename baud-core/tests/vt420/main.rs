//! VT420 — left and right margins, and the column operations that ride on them.
//!
//! DECSLRM, DECIC and DECDC. Small enough to be one module today; it is its own
//! directory because the protocol level is, and because `CSI s` is ambiguous
//! between SCOSC and DECSLRM in a way that belongs filed under VT420.

#[path = "../support/mod.rs"]
mod support;

mod side_margins;

/// **Left/right margins, in one gap.** `Paint::set_side_margins` takes the
/// sequence and records nothing -- `DECSLRM`, `DECLRMM` and the column
/// operations that respect them are the one VT420 feature `baud-core` does not
/// have. It is a feature rather than an accessor: wrapping, `DCH`, `DECIC` and
/// `DECDC` all have to consult the margins, so recording them without acting
/// would be the worse half.
#[test]
fn the_known_vt420_gaps() {
    support::ignores_are(
        "vt420",
        &["side_margins.rs"],
        &[
            "csi_s_is_decslrm_with_declrmm",
            "text_wraps_within_the_margins",
            "decrqss_reports_the_side_margins",
            "dch_does_not_pull_from_outside_the_margins",
            "no_wrap_pins_to_the_right_margin",
            "decdc_deletes_columns_and_fills_at_the_margin",
            "the_column_operations_are_inert_outside_the_margins",
        ],
    );
}
