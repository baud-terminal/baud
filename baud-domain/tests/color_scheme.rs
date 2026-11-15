//! The dark/light classifier behind `CSI ? 996 n` and `?2031`, and its report.
//!
//! The rule is WCAG 2.2 relative luminance of the
//! background against the text, dark when the background is the darker. The
//! core has no `powf`, so the sRGB linearisation is a table; these tests hold
//! the table and the verdicts to the formula computed in `f64`.

use baud_domain::*;

fn lin(c: u8) -> f64 {
    let v = f64::from(c) / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn lum((r, g, b): (u8, u8, u8)) -> f64 {
    0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)
}

fn word((r, g, b): (u8, u8, u8)) -> ColorWord {
    ColorWord::rgb(Chan::from_u8(r), Chan::from_u8(g), Chan::from_u8(b))
}

/// The `f64` verdict, and how far apart the two luminances are.
fn reference(fg: (u8, u8, u8), bg: (u8, u8, u8)) -> (ColorScheme, f64) {
    let (f, b) = (lum(fg), lum(bg));
    let s = if b < f {
        ColorScheme::Dark
    } else {
        ColorScheme::Light
    };
    (s, (f - b).abs())
}

/// Rounding each channel to 1/65535 moves a luminance by at most half a step
/// in each of three channels; two luminances, so twice that.
const QUANTUM: f64 = 1.0 / 65535.0;

#[test]
fn the_table_is_the_srgb_transfer_function() {
    for c in 0..=255u8 {
        let want = (lin(c) * 65535.0).round();
        assert_eq!(f64::from(LINEAR[usize::from(c)]), want, "entry {c}");
    }
}

#[test]
fn every_pair_of_greys_is_classified_as_wcag_does() {
    let mut checked = 0;
    for f in 0..=255u8 {
        for b in 0..=255u8 {
            let (fg, bg) = ((f, f, f), (b, b, b));
            let (want, gap) = reference(fg, bg);
            let got = ColorScheme::of(word(fg), word(bg)).expect("rgb");
            if gap > QUANTUM {
                assert_eq!(got, want, "fg {f} on bg {b}");
                checked += 1;
            }
        }
    }
    // Every pair of distinct greys has distinct luminances well apart, so all
    // but the 256 ties are checked -- the control that the bound excused
    // nothing it should not have.
    assert_eq!(checked, 256 * 256 - 256);
}

#[test]
fn seeded_random_pairs_are_classified_as_wcag_does() {
    // xorshift, so the sequence is fixed without a dependency.
    let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x.to_le_bytes()
    };
    let (mut checked, mut near) = (0, 0);
    for _ in 0..200_000 {
        let v = next();
        let (fg, bg) = ((v[0], v[1], v[2]), (v[3], v[4], v[5]));
        let (want, gap) = reference(fg, bg);
        let got = ColorScheme::of(word(fg), word(bg)).expect("rgb");
        if gap > 2.0 * QUANTUM {
            assert_eq!(got, want, "{fg:?} on {bg:?}");
            checked += 1;
        } else {
            near += 1;
        }
    }
    assert!(
        checked > 199_000,
        "only {checked} checked, {near} near a tie"
    );
}

#[test]
fn a_tie_reads_light_and_the_defaults_read_nothing() {
    let g = word((128, 128, 128));
    assert_eq!(ColorScheme::of(g, g), Some(ColorScheme::Light));
    assert_eq!(ColorScheme::of(ColorWord::DEFAULT, g), None);
    assert_eq!(ColorScheme::of(g, ColorWord::DEFAULT), None);
    assert_eq!(
        ColorScheme::of(ColorWord::indexed(Chan::from_u8(1)), g),
        None
    );
    // baud's two themes, and mid-grey on white, the pair where a
    // background-only rule and this one part: WCAG's comparison says dark
    // there.
    let (lt, dk) = (word((0xe5, 0xe5, 0xe5)), word((0x10, 0x10, 0x10)));
    assert_eq!(ColorScheme::of(lt, dk), Some(ColorScheme::Dark));
    assert_eq!(ColorScheme::of(dk, lt), Some(ColorScheme::Light));
    assert_eq!(
        ColorScheme::of(word((255, 255, 255)), word((0x80, 0x80, 0x80))),
        Some(ColorScheme::Dark)
    );
}

#[test]
fn the_report_is_exactly_997_1_or_2() {
    let enc = |s| {
        Report::ColorScheme(s)
            .encode(C1Transmit::SevenBit)
            .as_bytes()
            .to_vec()
    };
    assert_eq!(enc(ColorScheme::Dark), b"\x1b[?997;1n");
    assert_eq!(enc(ColorScheme::Light), b"\x1b[?997;2n");
    let c1 = Report::ColorScheme(ColorScheme::Dark).encode(C1Transmit::EightBit);
    assert_eq!(c1.as_bytes(), b"\x9b?997;1n");
}
