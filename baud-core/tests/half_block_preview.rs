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

/// A distinct foreground for every cell, built from `u8` components so there
/// is no cast: `(0, g, b)` walks the green/blue plane, one per cell, which is
/// 65_536 distinct colours -- far past a screenful. Non-zero blue keeps early
/// cells off the default.
fn distinct_foregrounds(count: usize) -> Vec<(u8, u8, u8)> {
    let mut v = Vec::with_capacity(count);
    'fill: for g in 0u8..=255 {
        for b in 1u8..=255 {
            v.push((0u8, g, b));
            if v.len() == count {
                break 'fill;
            }
        }
    }
    v
}

/// **The scramble a file manager's photo preview shows, proved without a
/// screenshot.** A half-block preview mints one distinct truecolour rendition
/// per cell, and a photo has thousands of them; every one is live at once,
/// because the whole image is on screen. When the rendition table is smaller
/// than the screen, past the last free slot `intern_style` evicts a slot an
/// on-screen cell still references, and that earlier cell then resolves through
/// `Term::style_of` to a *later* cell's colour -- the wrong colour, which is
/// the scramble. A garbage collector cannot help: nothing is dead to reclaim
/// while the image is displayed. The fix is capacity, so the table is sized to
/// the screen; this pins that a full screen of distinct renditions resolves
/// exactly.
///
/// Against a 1024-slot table this failed with 6976 of 8000 cells wrong, first
/// wrong row 0 -- the eviction corrupts the cells interned first, which is
/// exactly the top-band scramble the app showed over an otherwise coherent
/// image. Rows fill top to bottom, so the top band is what goes.
#[test]
fn a_photo_half_block_preview_does_not_scramble_its_colours() {
    let ws = support::CORPUS; // 200 x 50, sized so its cells hold distinct styles
    let rows = 40; // clear of the bottom, so no scroll retires a live cell
    let cols = usize::from(ws.ws_col);
    let mut s = Owned::new(&ws);
    let mut t = Term::new(s.storage(), &ws);

    // Every cell a unique foreground -- the shape of a real photo, where no two
    // adjacent pixels share a colour. They lay out row-major under autowrap,
    // one upper-half block a cell.
    let fgs = distinct_foregrounds(rows * cols);
    let bg = (7u8, 7u8, 7u8); // a constant lower pixel; the fg is what varies
    let mut bytes = Vec::new();
    for &fg in &fgs {
        bytes.extend(cell(fg, bg));
    }
    t.write_at(BytePos::ZERO, &bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));

    let mut wrong = 0usize;
    let mut first_wrong_row = None;
    for (idx, &fg) in fgs.iter().enumerate() {
        let row = t.row(Row::from_index(idx / cols));
        let sw = t.style_of(row[idx % cols]);
        if sw.fg().as_rgb() != Some(fg) || sw.bg().as_rgb() != Some(bg) {
            wrong += 1;
            first_wrong_row.get_or_insert(idx / cols);
        }
    }
    assert_eq!(
        wrong,
        0,
        "{wrong} of {} cells resolve to the wrong colour (first wrong row {:?}); \
         the {rows}x{cols} preview mints that many distinct renditions and the \
         table must hold a screenful or on-screen cells are evicted",
        fgs.len(),
        first_wrong_row,
    );
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
