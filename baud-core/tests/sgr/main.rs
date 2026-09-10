//! SGR — character attributes.
//!
//! Its own directory because it spans protocol levels: the base attributes are
//! ECMA-48, 256-colour and truecolour are xterm, and the colon sub-parameter
//! form is ISO 8613-6. Filing it under any one of those would misplace the
//! other two.

#[path = "../support/mod.rs"]
mod support;

mod fonts;
mod ideogram;
mod param_overflow;
mod rep_align;
mod subparams;

/// One, about what the parameter walk hands on.
///
/// **`REP` closed, and it was two defects rather than the one recorded here.**
/// `blit` writes eight cells to a store and never recorded what it wrote, so
/// whether `REP` worked depended on how long the run before it was -- the
/// short-run path goes through `glyph`, which did. And `last_cp` lived only on
/// `Paint`, which is built once per `feed`, so it did not survive a read
/// boundary either: `ab` then `CSI 3 b` in two reads repeated nothing where
/// the same bytes in one read repeat correctly.
///
/// * A parameter past `NPARAM` is merged into the last slot rather than
///   dropped, so `CSI 1;2;...;40 m` applies an attribute nobody sent.

#[test]
fn the_known_sgr_gaps() {
    support::ignores_are(
        "sgr",
        &["param_overflow.rs", "rep_align.rs"],
        &["the_parameter_past_capacity_is_dropped_not_merged"],
    );
}
