//! DEC Special Graphics `0x5F`, Blank, drawn: a Blank cell in a background
//! rendition paints the pixels a space in that rendition paints, though the
//! cell keeps `CellFlags::GRAPHICS`.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows};
use baud_font::Face;
use baud_render::{Blend, Damage, PxSize, Renderer, Theme};
use support::{Canvas, WS, font, paint};

/// One full CPU frame of `bytes`, as a digest.
fn frame(bytes: &[u8]) -> u64 {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let mut r = Renderer::new(
        baud_render::Stack::new(&faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    let mut c = Canvas::for_renderer(&r);
    paint(&mut r, &mut t, &mut c, Damage::Full);
    c.digest()
}

/// Blank in a blue background, alone and in a run between line-drawing
/// characters, is the frame of spaces in that background; `_` under ASCII
/// is not, the control that the comparison can tell a glyph from none.
#[test]
fn a_blank_cell_paints_as_a_space_in_its_rendition() {
    let blank = frame(b"\x1b[44m\x1b(0_____\x1b(B\x1b[0m\x1b[?25l");
    let spaces = frame(b"\x1b[44m     \x1b[0m\x1b[?25l");
    assert_eq!(blank, spaces, "Blank paints as a space");
    assert_eq!(
        frame(b"\x1b[44m\x1b(0q_q\x1b(B\x1b[0m\x1b[?25l"),
        frame(b"\x1b[44m\x1b(0q\x1b(B \x1b(0q\x1b(B\x1b[0m\x1b[?25l"),
        "Blank between line-drawing characters paints as a space"
    );
    assert_ne!(
        frame(b"\x1b[44m_____\x1b[0m\x1b[?25l"),
        spaces,
        "the control: `_` draws a glyph"
    );
}
