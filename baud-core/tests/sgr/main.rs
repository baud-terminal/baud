//! SGR — character attributes.
//!
//! Its own directory because it spans protocol levels: the base attributes are
//! ECMA-48, 256-colour and truecolour are xterm, and the colon sub-parameter
//! form is ISO 8613-6. Filing it under any one of those would misplace the
//! other two.

#[path = "../support/mod.rs"]
mod support;

mod blink;
mod fonts;
mod groups;
mod ideogram;
mod param_overflow;
mod rep_align;
mod subparams;
mod underline;

/// **No gaps, and the gate stays so a new one has to say so.**
///
/// Both were about what the parameter walk hands on, and both were deliberate
/// trades whose reasons did not survive being tested:
///
/// * **A parameter past capacity overwrote the last one.** `sep` saturated at
///   the final real slot, so `CSI 0;...;1;4 m` applied the underline and lost
///   the bold -- the opposite of truncation. The comment called it a trade
///   against "a bounds check on the hot path for input no application sends",
///   and the trade was unnecessary: **one more array element** is the same
///   thing with no branch, because everything past capacity folds into a slot
///   nothing reads.
/// * **`REP` after a bulk run repeated nothing.** `blit` writes eight cells to
///   a store and never recorded what it wrote -- only `glyph` did, which is the
///   short-run path, so whether `REP` worked depended on how long the run
///   before it was. Fixing that found a second half: `last_cp` did not survive
///   a read boundary either.
#[test]
fn the_known_sgr_gaps() {
    support::ignores_are("sgr", &["param_overflow.rs", "rep_align.rs"], &[]);
}
