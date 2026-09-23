//! A selection's drawing on the CPU path, where the pixel oracle cannot
//! reach: the oracle compares the instance arm against the cell shader, so
//! a rule both of them lost would pass it. This holds the instance and CPU
//! arms to the rule on their own. `docs/research/notes/k-selection.md` is
//! the spec.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Codepoint, Col, Cols, ReadLen, Row, Rows, WinSize};
use baud_font::Face;
use baud_render::{Blend, Damage, Highlight, PxSize, Region, Renderer, Stack, Theme};
use support::font;

const WS: WinSize = WinSize {
    ws_col: 12,
    ws_row: 4,
};

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// U+231A WATCH: East Asian Width `W` with emoji presentation, so the core
/// lays it out two columns wide, and one of the few codepoints the Noto
/// subset carries a bitmap for. Both are asserted rather than trusted.
fn a_wide_emoji(face: &Face<'_>) -> char {
    let watch = Codepoint::new(0x231A).unwrap();
    assert!(face.glyph(watch).is_some(), "the subset maps U+231A");
    char::from_u32(watch.value()).unwrap()
}

#[test]
fn a_blocks_edge_never_cuts_a_wide_character_in_two() {
    let mono = font("JetBrainsMono-Regular.ttf");
    let noto = font("NotoColorEmoji-subset.ttf");
    let faces = [Face::parse(&mono).unwrap(), Face::parse(&noto).unwrap()];
    let e = a_wide_emoji(&faces[1]);
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    // The emoji straddles the block's left edge on row 1 (lead 1, tail 2)
    // and its right edge on row 2 (lead 2, tail 3); the corners are narrow.
    feed(
        &mut t,
        format!("\x1b[?25labcdefgh\r\na{e}bc\r\nab{e}c\r\nabcdefgh").as_bytes(),
    );
    let view = t.viewport();
    assert!(view.row(Row::from_index(1))[2].content.is_wide_tail());
    assert!(view.row(Row::from_index(2))[3].content.is_wide_tail());
    r.set_highlight(Some(Highlight {
        from: (Row::ZERO, Col::at_index(2)),
        to: (Row::from_index(3), Col::at_index(2)),
        region: Region::Block,
    }));
    let _ = r.convert(&t, Rows::ZERO, Damage::Full);
    let bg = r.background();
    let at = |row: usize, col: usize| bg[row * usize::from(WS.ws_col) + col];
    // The corner at (0, 2) is swapped and (0, 8), never written and outside
    // the block, is not: those are the two words every cell is one of.
    let (on, off) = (at(0, 2), at(0, 8));
    assert_ne!(on, off, "a selected cell's background swaps");
    let swapped = |row: usize, col: usize| {
        let w = at(row, col);
        assert!(w == on || w == off, "({row}, {col}) is neither word");
        w == on
    };
    // Row 1: the lead is outside, so its tail inside the geometry is out.
    assert!(!swapped(1, 1));
    assert!(
        !swapped(1, 2),
        "a tail whose lead is outside is not selected"
    );
    // Row 2: the lead is inside, so its tail past the edge is in.
    assert!(swapped(2, 2));
    assert!(swapped(2, 3), "a tail whose lead is inside is selected");
    // A block is a rectangle: a middle row's cells past its columns are out.
    assert!(!swapped(1, 4) && !swapped(2, 5));
    assert!(swapped(3, 2) && !swapped(3, 3));
}
