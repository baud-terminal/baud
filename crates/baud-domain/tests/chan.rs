//! `Chan::from_unit`: a fraction to a channel.
//!
//! The conversion a config's `0.22` and an AppKit alpha both need, and the
//! one arithmetic in the glass path that is not AppKit's. It is here
//! rather than beside its caller because `Chan` owns it: the cast is the
//! newtype's own edge, which is the exemption this crate's audit budget
//! rests on.

use baud_domain::Chan;

/// The values the config's defaults resolve to, and the two ends.
///
/// **0.22 is 56.1 of 255 and 0.12 is 30.6**: one rounds down and the
/// other **up**, so a conversion that truncated would agree with this
/// test on `dim_dark` and give 30 for `dim_light`. That pair is the whole
/// reason the rounding is stated rather than left to the conversion's
/// default, and it is what makes this table a test of the arithmetic
/// rather than a restatement of it -- verified by truncating the
/// implementation, which fails this case and the `0.5` one.
///
/// A second test asserting the same thing about the product alone was
/// written and removed: with both sides literals it is const-folded, and
/// clippy refuses an assertion that cannot fail. It was right to.
#[test]
fn a_fraction_rounds_to_the_nearest_channel() {
    for (fraction, want) in [
        (0.0, 0),
        (0.12, 31),
        (0.14, 36),
        (0.22, 56),
        (0.54, 138),
        (0.5, 128),
        (1.0, 255),
    ] {
        assert_eq!(
            Chan::from_unit(fraction).get(),
            want,
            "{fraction} of 255 is {}",
            fraction * 255.0
        );
    }
}

/// Out of range is clamped rather than wrapped: an opacity past opaque is
/// opaque, and a negative one is clear. `as` on a float saturates, so the
/// clamp is belt and braces -- and a NaN, which no clamp can order, comes
/// out at the clear end.
#[test]
fn out_of_range_clamps_and_a_nan_is_clear() {
    assert_eq!(Chan::from_unit(1.5).get(), 255);
    assert_eq!(Chan::from_unit(-0.5).get(), 0);
    assert_eq!(Chan::from_unit(f32::INFINITY).get(), 255);
    assert_eq!(Chan::from_unit(f32::NEG_INFINITY).get(), 0);
    assert_eq!(Chan::from_unit(f32::NAN).get(), 0);
}
