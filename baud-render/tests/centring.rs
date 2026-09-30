//! Every glyph is centred in its cell (note J, decision 7).
//!
//! A cell is the primary face's advance rounded to whole pixels, so drawing
//! each glyph at the cell's left edge leaves all the rounding slack on the
//! right: at 14 px JetBrains Mono's 8.4 px advance sits in an 8 px cell, and
//! every glyph was 0.2 px right of centre. The renderer draws every outline
//! half the slack over instead, one phase for the whole grid.
//!
//! The glyphs here are ones whose outline is mirror-symmetric in JetBrains
//! Mono (the bounding box is centred on the 600-unit advance, and the shape
//! is a reflection of itself), so their ink centroid is the middle of the
//! advance. Centred, it is the middle of the cell, to within the rasteriser's
//! own error; at the left edge it is half the slack off.

mod support;

use baud_core::{Owned, Term};
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::Face;
use baud_render::{Blend, Damage, PxSize, Renderer, Stack, Theme};
use support::font;

const WS: WinSize = WinSize {
    ws_row: 3,
    ws_col: 16,
};

/// Mirror-symmetric in JetBrains Mono 2.304: `H O 0 | I u U M = + - *`.
/// Not `m`: its box is centred but its ink is not, and its centroid moves
/// 0.06-0.10 px left with size, which is the design and not the placement.
/// `M` carries the most of that kind, 0.027 px at 32 px, which the tolerance
/// allows.
const SYMMETRIC: &str = "HO0|IuUM=+-*";

/// The ink centroid of each of the first `n` cells of row 0, in pixels from
/// the cell's left edge, and the cell width.
///
/// The renderer is made at `made` px and rescaled to `px` when they differ,
/// which is what a zoom does: the atlas resets, and must take the new
/// size's phase with it.
fn centroids(made: f32, px: f32, n: usize) -> (Vec<f64>, f64) {
    let data = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&data).expect("parse")];
    let mut r = Renderer::new(
        Stack::new(&faces).unwrap(),
        PxSize::of(made).unwrap(),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(usize::from(WS.ws_col)),
        Rows::of_index(usize::from(WS.ws_row)),
    )
    .unwrap();
    if made != px {
        r.rescale(PxSize::of(px).unwrap()).unwrap();
    }
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    // The text on row 0, and the cursor out of its way on row 3.
    let bytes = format!("{SYMMETRIC}\x1b[3;1H");
    t.write_at(BytePos::ZERO, bytes.as_bytes());
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    let mut canvas = support::Canvas::for_renderer(&r);
    support::paint(&mut r, &mut t, &mut canvas, Damage::Full);

    let m = r.metrics();
    let cw = usize::try_from(m.cell_w.value()).unwrap();
    let ch = usize::try_from(m.cell_h.value()).unwrap();
    let theme = Theme::dark();
    let green = |c: u32| f64::from((c >> 8) & 0xff);
    let (bg, fg) = (green(theme.bg.0), green(theme.fg.0));
    let out = (0..n)
        .map(|col| {
            let (mut ink, mut moment) = (0.0, 0.0);
            for y in 0..ch {
                for x in 0..cw {
                    let p = canvas.pixels[y * canvas.width + col * cw + x];
                    let a = ((green(p) - bg) / (fg - bg)).clamp(0.0, 1.0);
                    ink += a;
                    moment += a * (x as f64 + 0.5);
                }
            }
            assert!(ink > 1.0, "cell {col} at {px} px has ink");
            moment / ink
        })
        .collect();
    (out, cw as f64)
}

/// Centred at 1x and at 2x sizes, including the ones where the slack is
/// largest: 14 px (8.4 in 8) and 16 px (9.6 in 10). At the left edge those
/// glyphs sit 0.2 px off; centred, within the rasteriser's error of the
/// middle.
#[test]
fn a_symmetric_glyph_is_centred_in_its_cell() {
    let n = SYMMETRIC.chars().count();
    for px in [13.0, 14.0, 15.0, 16.0, 24.0, 26.0, 28.0, 30.0, 32.0] {
        let (cs, cw) = centroids(px, px, n);
        for (c, x) in SYMMETRIC.chars().zip(&cs) {
            let off = x - cw / 2.0;
            assert!(
                off.abs() <= 0.05,
                "{c:?} at {px} px: centroid {x:.3} in a {cw} px cell, {off:+.3} px off centre"
            );
        }
    }
}

/// After a zoom. 14 px wants the phase -0.2 and 16 px +0.2, so a reset that
/// kept the size it came from would put every glyph 0.4 px off.
#[test]
fn a_rescaled_renderer_centres_at_its_new_size() {
    let n = SYMMETRIC.chars().count();
    for (made, px) in [(14.0, 16.0), (16.0, 14.0), (13.0, 26.0)] {
        let (cs, cw) = centroids(made, px, n);
        for (c, x) in SYMMETRIC.chars().zip(&cs) {
            let off = x - cw / 2.0;
            assert!(
                off.abs() <= 0.05,
                "{c:?} at {px} px, made at {made}: {off:+.3} px off centre"
            );
        }
    }
}
