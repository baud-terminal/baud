//! **A flag after a Hebrew letter is the flag its letters name.**
//!
//! A regional indicator is Bidi_Class L, so after an R letter the pair
//! resolves to level 2 (I2) and is a level run of its own (BD7), and shaping
//! stays within a level run (3.5, `[[source: uax9-bidi]]`). The pair is
//! read in logical order, G then B, which names GB (`[[source: uts51-emoji]]`,
//! ED-14); walked the other way it would be the flag of BG. So `א🇬🇧`
//! draws the same flag `🇬🇧` does, and not the one `🇧🇬` does.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::{Face, FaceIndex};
use baud_render::{Blend, Damage, PxSize, Renderer, Stack, Theme};

const EMOJI: &str = "/System/Library/Fonts/Apple Color Emoji.ttc";

/// Whether a pixel has a hue: a colour emoji's do, the theme's grey text
/// and background do not.
fn hued(p: u32) -> bool {
    let ch = [(p >> 16) & 0xff, (p >> 8) & 0xff, p & 0xff];
    ch.iter().max().unwrap() - ch.iter().min().unwrap() > 24
}

/// The frame `text` paints on the CPU path, cut to the box around its hued
/// pixels, every other pixel in the box zeroed: the colour glyphs alone,
/// wherever on the row they were drawn.
fn colour_glyphs(text: &str) -> (usize, usize, Vec<u32>) {
    let built = std::fs::read(format!(
        "{}/../baud-app/fonts/built/Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let emoji = std::fs::read(EMOJI).unwrap();
    let faces = [
        Face::parse(&built).unwrap(),
        Face::parse_index(&emoji, FaceIndex::of(0)).unwrap(),
    ];
    let ws = WinSize {
        ws_row: 2,
        ws_col: 8,
    };
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(28.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(8),
        Rows::of_index(2),
    )
    .unwrap();
    let mut owned = Owned::new(&ws);
    let mut t = Term::new(owned.storage(), &ws);
    let bytes = format!("\x1b[8h{text}\x1b[?25l");
    t.write_at(BytePos::ZERO, bytes.as_bytes());
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    let mut c = support::Canvas::for_renderer(&r);
    support::paint(&mut r, &mut t, &mut c, Damage::Full);
    let at = |x: usize, y: usize| c.pixels[y * c.width + x];
    let mut xs = (usize::MAX, 0usize);
    let mut ys = (usize::MAX, 0usize);
    for y in 0..c.height {
        for x in 0..c.width {
            if hued(at(x, y)) {
                xs = (xs.0.min(x), xs.1.max(x));
                ys = (ys.0.min(y), ys.1.max(y));
            }
        }
    }
    assert!(xs.0 <= xs.1, "{text}: no colour glyph reached the pixels");
    let mut cut = Vec::new();
    for y in ys.0..=ys.1 {
        for x in xs.0..=xs.1 {
            let p = at(x, y);
            cut.push(if hued(p) { p } else { 0 });
        }
    }
    (xs.1 - xs.0 + 1, ys.1 - ys.0 + 1, cut)
}

#[test]
fn a_flag_after_a_hebrew_letter_is_the_flag_its_letters_name() {
    if !cfg!(target_os = "macos") {
        eprintln!("skipped: the emoji face is macOS's");
        return;
    }
    let gb = colour_glyphs("\u{1F1EC}\u{1F1E7}");
    let bg = colour_glyphs("\u{1F1E7}\u{1F1EC}");
    // Positive control: the two flags paint differently, so equal to one
    // is not equal to the other.
    assert!(gb != bg, "the flags of GB and BG paint the same pixels");
    assert!(gb.2.iter().filter(|&&p| p != 0).count() > 50);
    let after_alef = colour_glyphs("\u{5D0}\u{1F1EC}\u{1F1E7}");
    assert!(
        after_alef == gb,
        "after an alef, G B paints a flag other than GB"
    );
    assert!(after_alef != bg, "after an alef, G B paints BG");
}
