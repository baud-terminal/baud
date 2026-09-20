//! The three colon underline styles reach the pixels, differ from each other
//! and from the straight line, and share the straight line's colour plumbing.
//!
//! `[[source: kitty-underlines]]`: `4:3` curly, `4:4` dotted, `4:5` dashed.
//! Each is drawn as a run of thickness-wide solids, so the test asks the
//! only questions a run of solids can answer: that something was painted in
//! the underline band, that the four styles paint different things, and that
//! `SGR 58`'s colour is what gets painted.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::{Blend, Damage, PxSize, Renderer, Stack, Theme};
use support::font;

const WS: WinSize = WinSize {
    ws_row: 1,
    ws_col: 6,
};

/// The pixels of the whole canvas after `bytes`, plus the underline band's
/// row range so a caller can look only there.
fn painted(bytes: &[u8]) -> (Vec<u32>, usize, usize, usize) {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
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
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    let mut c = support::Canvas::for_renderer(&r);
    support::paint(&mut r, &mut t, &mut c, Damage::Full);
    let m = r.metrics();
    let ul = usize::try_from(m.underline.value()).unwrap();
    let th = usize::try_from(m.thickness.value().max(1)).unwrap();
    let w = c.width;
    (c.pixels, w, ul, ul + 2 * th)
}

/// Pixels in rows `y0..y1` that are not the default background.
fn band(px: &[u32], w: usize, y0: usize, y1: usize) -> Vec<u32> {
    let bg = Theme::dark().bg.0;
    (y0..y1)
        .flat_map(|y| px[y * w..(y + 1) * w].iter().copied())
        .filter(|&p| p != bg)
        .collect()
}

/// The band under a blank cell is painted for every style, and each style
/// paints a different pattern from the others -- a straight line included.
#[test]
fn every_style_paints_the_band_and_no_two_paint_it_alike() {
    let styles: [(&[u8], &str); 4] = [
        (b"\x1b[4m  ", "straight"),
        (b"\x1b[4:3m  ", "curly"),
        (b"\x1b[4:4m  ", "dotted"),
        (b"\x1b[4:5m  ", "dashed"),
    ];
    let mut bands = Vec::new();
    for (bytes, name) in styles {
        let (px, w, y0, y1) = painted(bytes);
        let b = band(&px, w, y0, y1);
        assert!(
            !b.is_empty(),
            "{name}: nothing painted in the underline band"
        );
        bands.push((name, px));
    }
    for i in 0..bands.len() {
        for j in i + 1..bands.len() {
            assert_ne!(
                bands[i].1, bands[j].1,
                "{} and {} paint the same pixels",
                bands[i].0, bands[j].0
            );
        }
    }
}

/// The dotted line paints fewer pixels than the straight one, the dashed
/// fewer than the straight and more than the dotted: the on/off periods are
/// what tell them apart, and this is that ordering measured.
#[test]
fn the_broken_styles_paint_less_than_the_solid_one() {
    let count = |bytes: &[u8]| {
        let (px, w, y0, y1) = painted(bytes);
        band(&px, w, y0, y1).len()
    };
    let straight = count(b"\x1b[4m  ");
    let dotted = count(b"\x1b[4:4m  ");
    let dashed = count(b"\x1b[4:5m  ");
    assert!(dotted < straight, "dotted {dotted} < straight {straight}");
    assert!(dashed < straight, "dashed {dashed} < straight {straight}");
    assert!(dotted < dashed, "dotted {dotted} < dashed {dashed}");
}

/// `SGR 58` colours a curly underline, the way it colours a straight one.
#[test]
fn the_underline_colour_reaches_a_styled_line() {
    // `?25l`: the block cursor after the two spaces is painted in the
    // foreground and would otherwise sit in the band as a third colour --
    // which is what the first run of this test reported, and it was the
    // harness, not the line.
    let (px, w, y0, y1) = painted(b"\x1b[?25l\x1b[58;2;255;0;0m\x1b[4:3m  ");
    let b = band(&px, w, y0, y1);
    assert!(!b.is_empty());
    let mut distinct: Vec<u32> = b.iter().map(|p| p & 0x00ff_ffff).collect();
    distinct.sort_unstable();
    distinct.dedup();
    assert!(
        b.iter().all(|&p| p & 0x00ff_ffff == 0x00ff_0000),
        "every painted pixel in the band is the underline's red; distinct colours in band rows {y0}..{y1}: {distinct:x?}"
    );
}
