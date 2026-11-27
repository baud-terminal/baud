//! Colour glyphs and the shapes a rendition draws, on the committed faces:
//! Bungee Tint's `COLR` layers, Noto Color Emoji's `CBDT` strike, an
//! underline, a strike-through and the two non-block cursors.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Codepoint, Cols, ReadLen, Rows, WinSize};
use baud_font::{Face, Px};
use baud_render::{Blend, Damage, Kind, PxSize, Renderer, Stack, Target, Theme};
use support::font;

const WS: WinSize = WinSize {
    ws_row: 2,
    ws_col: 24,
};

/// A codepoint the Noto subset carries a bitmap for: the first one its
/// `cmap` maps at all, whatever block the subset happened to keep.
fn an_emoji(face: &Face<'_>) -> u32 {
    (0x80..=0x10FFFF)
        .find(|&cp| Codepoint::new(cp).is_some_and(|c| face.glyph(c).is_some()))
        .expect("the CBDT subset maps something")
}

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

#[test]
fn colour_glyphs_and_rendition_shapes_reach_the_pixels() {
    let bungee = font("BungeeTint-Regular.ttf");
    let noto = font("NotoColorEmoji-subset.ttf");
    let faces = [Face::parse(&bungee).unwrap(), Face::parse(&noto).unwrap()];
    let emoji = an_emoji(&faces[1]);
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(20.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    let m = r.metrics();
    let (w, h) = r.pixel_size();
    let mut pixels = vec![0u32; usize::try_from(w.value() * h.value()).unwrap()];
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    // Underlined AB, a strike-through S, the emoji, then a bar cursor.
    let text = format!(
        "\x1b[4mAB\x1b[24m \x1b[9mS\x1b[29m {}\x1b[5 q",
        char::from_u32(emoji).unwrap()
    );
    feed(&mut t, text.as_bytes());
    let frame = {
        let mut target = Target::new(&mut pixels, w, w, h).unwrap();
        let seal = t.seal_writes();
        r.render(&t, seal, &mut target, Damage::Full)
    };
    // A and B and S are Bungee COLR glyphs, the emoji a CBDT image.
    assert!(frame.colored.value() >= 4, "{:?} coloured", frame.colored);
    // Two underline cells, one strike, one bar cursor.
    assert_eq!(frame.solids.value(), 4, "{:?} solids", frame.solids);
    let cell_w = usize::try_from(m.cell_w.value()).unwrap();
    let stride = usize::try_from(w.value()).unwrap();
    let fg = Theme::dark().fg.0;
    let at = |x: usize, y: usize| pixels[y * stride + x];
    // The underline under A is the foreground colour, the cell below it
    // (row 1, nothing written) is the background.
    let uy = usize::try_from(m.underline.value()).unwrap();
    assert_eq!(at(cell_w / 2, uy), fg, "underline");
    assert_eq!(
        at(cell_w / 2, uy + usize::try_from(m.cell_h.value()).unwrap()),
        Theme::dark().bg.0
    );
    // The strike crosses S at cell 3.
    let sy = usize::try_from(m.strike.value()).unwrap();
    assert_eq!(at(3 * cell_w + cell_w / 2, sy), fg, "strike");
    // The emoji at cell 5 has a chromatic pixel somewhere in its cell.
    let chroma = (0..usize::try_from(m.cell_h.value()).unwrap()).any(|y| {
        (5 * cell_w..7 * cell_w).any(|x| {
            let p = at(x, y);
            let (r, g, b) = ((p >> 16) & 255, (p >> 8) & 255, p & 255);
            r.abs_diff(g) > 40 || g.abs_diff(b) > 40
        })
    });
    assert!(chroma, "no colour in the emoji cell");
    // A Bungee letter is chromatic too: its layers are tinted.
    let tint = (0..usize::try_from(m.cell_h.value()).unwrap()).any(|y| {
        (0..cell_w).any(|x| {
            let p = at(x, y);
            let (r, g, b) = ((p >> 16) & 255, (p >> 8) & 255, p & 255);
            p != Theme::dark().bg.0 && (r.abs_diff(g) > 20 || g.abs_diff(b) > 20)
        })
    });
    assert!(tint, "no colour in the Bungee cell");
    // The bar cursor sits at the left edge of the cursor's cell, wherever
    // the emoji's width left it.
    let (_, cursor_col) = t.cursor();
    let cx = cursor_col.index() * cell_w;
    assert!(cursor_col.index() >= 6, "{}", cursor_col.index());
    assert_eq!(
        at(cx, 2),
        baud_render::BaudCursor::dark().fill.0,
        "bar cursor"
    );
    assert_eq!(
        frame.instances.value() - frame.solids.value() - frame.colored.value(),
        0,
        "no plain masks expected"
    );
    let _ = Kind::Mask;
    let _: Px = m.cell_w;
}
