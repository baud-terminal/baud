//! The other image preview a file manager draws: half-block "ASCII" mode.
//!
//! When a terminal advertises no Sixel or Kitty support, a previewer (yazi)
//! renders an image as text -- one cell per two vertical pixels, the upper
//! pixel as the foreground and the lower as the background of an upper-half-
//! block `U+2580` glyph, in truecolour. It is what baud's own DA (`?6c`, no
//! Sixel) makes yazi choose, so it is the mode most users see, and it is
//! nothing but the text path: a glyph and a per-cell SGR foreground and
//! background. This pins that yazi's exact shape lands -- the glyph in the
//! cell and both truecolour channels resolved -- so "ASCII preview" is a
//! tested case rather than an assumed consequence of the SGR path.

#[path = "support/mod.rs"]
mod support;

use baud_core::*;

/// One image cell yazi emits: `SGR 38;2;fg 48;2;bg` then the upper half block.
fn cell(fg: (u8, u8, u8), bg: (u8, u8, u8)) -> Vec<u8> {
    format!(
        "\x1b[38;2;{};{};{};48;2;{};{};{}m\u{2580}",
        fg.0, fg.1, fg.2, bg.0, bg.1, bg.2
    )
    .into_bytes()
}

#[test]
fn a_half_block_image_preview_renders_glyph_and_both_truecolour_channels() {
    let ws = support::CORPUS;
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);

    // Two adjacent cells with distinct top/bottom pixel colours, as a two-pixel-
    // wide slice of an image would be.
    let a = ((255, 0, 0), (0, 0, 255)); // top red, bottom blue
    let b = ((0, 255, 0), (255, 255, 0)); // top green, bottom yellow
    let mut bytes = cell(a.0, a.1);
    bytes.extend(cell(b.0, b.1));
    t.write_at(BytePos::ZERO, &bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));

    let row = t.row(Row::from_index(0));
    let block = 0x2580;
    for (i, want) in [a, b].into_iter().enumerate() {
        let c = row[i];
        assert_eq!(
            c.content.as_codepoint().map(|p| p.value()),
            Some(block),
            "cell {i} is the upper half block"
        );
        let sw = t.style_of(c);
        assert_eq!(
            sw.fg().as_rgb(),
            Some(want.0),
            "cell {i} foreground (the upper pixel)"
        );
        assert_eq!(
            sw.bg().as_rgb(),
            Some(want.1),
            "cell {i} background (the lower pixel)"
        );
    }
}
