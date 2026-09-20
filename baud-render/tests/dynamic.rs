//! `OSC 10` and `OSC 11` reach the pixels, and a change to them repaints the
//! whole screen even though no cell moved.
//!
//! The core stores the two colours and the renderer consults them before its
//! theme, the way it consults the core's `OSC 4` palette before its own table.
//! Both facts need a gate: the first because a stored colour nothing reads is
//! the failure `docs/vt-coverage.md` rates worst, the second because row
//! damage compares cell words and style slots, and an `OSC 11` moves neither
//! -- without the digest fold, a changed background would repaint nothing,
//! which is the same hole `set_theme` had before it dropped the damage cache.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::{Blend, Damage, PxSize, Renderer, Stack, Theme};
use support::font;

const WS: WinSize = WinSize {
    ws_row: 2,
    ws_col: 12,
};

fn renderer<'f>(faces: &'f [Face<'f>]) -> Renderer<'f, 'f> {
    Renderer::new(
        Stack::new(faces).unwrap(),
        PxSize::of(14.0).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap()
}

fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

/// The margin pixel -- row 0, column 0 of the canvas is inside the first
/// cell, which the text never covers because it starts at column 1.
fn corner(c: &support::Canvas) -> u32 {
    c.pixels[0]
}

/// An `OSC 11` changes what a blank cell is painted, and `OSC 111` puts the
/// theme back.
#[test]
fn osc_11_changes_the_background_and_111_restores_it() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut c = support::Canvas::for_renderer(&r);

    feed(&mut t, b" A");
    support::paint(&mut r, &mut t, &mut c, Damage::Full);
    let theme_bg = corner(&c);

    feed(&mut t, b"\x1b]11;rgb:aa/bb/cc\x07");
    support::paint(&mut r, &mut t, &mut c, Damage::Incremental);
    let set_bg = corner(&c);
    assert_ne!(set_bg, theme_bg, "OSC 11 did not reach the pixels");
    assert_eq!(
        set_bg & 0x00ff_ffff,
        0x00aa_bbcc,
        "the pixel is the colour asked for"
    );

    feed(&mut t, b"\x1b]111\x07");
    support::paint(&mut r, &mut t, &mut c, Damage::Incremental);
    assert_eq!(corner(&c), theme_bg, "OSC 111 did not restore the theme");
}

/// **The repaint is whole, on incremental damage.** No cell word and no style
/// slot changed, so row damage alone would find nothing dirty; the frame
/// reports every row repainted because the dynamic colours ride in the
/// palette digest that forces a full repaint when it moves.
#[test]
fn a_changed_dynamic_colour_repaints_every_row() {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut c = support::Canvas::for_renderer(&r);

    feed(&mut t, b"A\r\nB");
    support::paint(&mut r, &mut t, &mut c, Damage::Full);
    // A quiet incremental frame: nothing dirty. The control that says the
    // next assertion is about the colour and not about incremental damage
    // always repainting.
    let quiet = support::paint(&mut r, &mut t, &mut c, Damage::Incremental);
    assert_eq!(
        quiet.rows_dirty.value(),
        0,
        "a quiet frame repaints nothing"
    );

    feed(&mut t, b"\x1b]10;#ff0000\x07");
    let f = support::paint(&mut r, &mut t, &mut c, Damage::Incremental);
    assert_eq!(
        f.rows_dirty.value(),
        f.rows.value(),
        "an OSC 10 with no cell change must repaint every row"
    );
}
