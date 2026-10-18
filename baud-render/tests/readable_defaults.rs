//! **Every colour of the built-in sixteen reads at 4.5:1 on its own page**,
//! but the one that is the page's own extreme: black on the dark page and
//! white on the light one. The owner's answer of 2026-10-18 to note Y's
//! decisions 3 and 8 took five corrections to get there, each the colour's
//! OKLCH lightness moved and its hue kept: on the dark page xterm's ansi 1,
//! 4, 5 and 12, and on the light page Modus Operandi's ansi 7.
//!
//! WCAG 2.2 SC 1.4.3 asks 4.5:1 of text (`[[source: wcag22]]`). The ratio is
//! read twice: by the formula here, and by the floor itself, which at a
//! trigger of 4.5 lifts anything below it and must leave every entry alone.

use baud_domain::{Chan, ColorWord};
use baud_render::{Argb, Floor, Theme};

fn lin(c: Chan) -> f64 {
    let v = f64::from(c.get()) / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn lum(c: Argb) -> f64 {
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

fn contrast(a: Argb, b: Argb) -> f64 {
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

fn hex(c: Argb) -> u32 {
    (u32::from(c.r().get()) << 16) | (u32::from(c.g().get()) << 8) | u32::from(c.b().get())
}

/// The owner's five, as the decisions page put them.
const DARK: [(u8, u32); 4] = [(1, 0xe93327), (4, 0x3771ff), (5, 0xd61bd5), (12, 0x6469fe)];
const LIGHT: [(u8, u32); 1] = [(7, 0x6c6c6c)];

/// Whether the floor at 4.5 leaves entry `i` as it is, drawn as text on the
/// default page.
fn floor_keeps(theme: &Theme, i: u8) -> bool {
    let strict = Floor::Lift {
        trigger: 4.5,
        target: 4.5,
    };
    let c = theme.palette[usize::from(i)];
    let words = (ColorWord::indexed(Chan::from_u8(i)), ColorWord::DEFAULT);
    strict.lift(c, theme.bg, words) == c
}

#[test]
fn the_five_corrected_entries_read_at_four_and_a_half_on_their_page() {
    for (name, theme, fixes) in [
        ("dark", Theme::dark(), &DARK[..]),
        ("light", Theme::light(), &LIGHT[..]),
    ] {
        for &(i, want) in fixes {
            let c = theme.palette[usize::from(i)];
            assert_eq!(hex(c), want, "{name} entry {i}");
            let ratio = contrast(c, theme.bg);
            assert!(ratio >= 4.5, "{name} entry {i} is {ratio:.3}:1");
            assert!(
                floor_keeps(&theme, i),
                "{name} entry {i}: the floor lifts it"
            );
        }
    }
}

/// The whole sixteen, not only the five: dark 1-15 and light 0-14.
#[test]
fn every_colour_but_the_pages_own_extreme_reads_at_four_and_a_half() {
    for (name, theme, entries) in [
        ("dark", Theme::dark(), 1..=15u8),
        ("light", Theme::light(), 0..=14u8),
    ] {
        for i in entries {
            let ratio = contrast(theme.palette[usize::from(i)], theme.bg);
            assert!(ratio >= 4.5, "{name} entry {i} is {ratio:.3}:1");
            assert!(
                floor_keeps(&theme, i),
                "{name} entry {i}: the floor lifts it"
            );
        }
    }
}

/// **As pages the five go the other way**, as the bound in note Y section 5
/// says they must: a colour between the text and the page reads, as text on
/// the page times the text on it, exactly the page against the text, 13.35
/// dark and 12.99 light. The default text on each reads below 3 now, so the
/// default floor, at 1.2, leaves it, and Increase Contrast's, at 3, lifts it.
#[test]
fn as_pages_the_five_read_below_three_under_the_default_text() {
    let increase = Floor::Lift {
        trigger: 3.0,
        target: 4.5,
    };
    for (name, theme, fixes) in [
        ("dark", Theme::dark(), &DARK[..]),
        ("light", Theme::light(), &LIGHT[..]),
    ] {
        for &(i, _) in fixes {
            let page = theme.palette[usize::from(i)];
            let ratio = contrast(theme.fg, page);
            assert!(
                (2.8..3.0).contains(&ratio),
                "{name} entry {i}: the default text on it at {ratio:.3}:1"
            );
            let words = (ColorWord::DEFAULT, ColorWord::indexed(Chan::from_u8(i)));
            assert_eq!(Floor::DEFAULT.lift(theme.fg, page, words), theme.fg);
            let lifted = increase.lift(theme.fg, page, words);
            assert!(contrast(lifted, page) >= 4.5, "{name} entry {i}");
        }
    }
}
