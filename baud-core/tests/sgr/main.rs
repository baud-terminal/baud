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

/// Two, both about what the parameter walk hands on.
///
/// * A parameter past `NPARAM` is merged into the last slot rather than
///   dropped, so `CSI 1;2;...;40 m` applies an attribute nobody sent.
/// * `REP` after a bulk ASCII run repeats the wrong character: `blit` writes
///   the run without recording what the last one was, so `last_cp` is stale.
#[test]
fn the_known_sgr_gaps() {
    support::ignores_are(
        "sgr",
        &["param_overflow.rs", "rep_align.rs"],
        &[
            "the_parameter_past_capacity_is_dropped_not_merged",
            "rep_sees_bulk_ascii",
        ],
    );
}
