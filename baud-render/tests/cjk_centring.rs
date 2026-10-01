//! A CJK fallback face is centred in its two cells (note J).
//!
//! Hiragino's ideographs are a full em and Apple SD Gothic Neo's Hangul 0.865
//! em, against two cells of JetBrains Mono's 0.6 em: drawn from the cell's
//! left edge they sat 1-3 px left of the span's centre. The renderer offsets
//! every glyph of a face that maps 水 by half the slack between two cells
//! and that face's 水, so an ideograph's ink lands on the centre of its two
//! cells. Runs on macOS, where those faces are, and says so elsewhere.

mod support;

use baud_core::{Owned, Term};
use baud_domain::Codepoint;
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::{Bounds, Face, FaceIndex, Scale};
use baud_render::{Blend, Damage, PxSize, Renderer, Stack, Theme};
use support::font;

const HIRAGINO: &str = "/System/Library/Fonts/Hiragino Sans GB.ttc";
const GOTHIC: &str = "/System/Library/Fonts/AppleSDGothicNeo.ttc";

const WS: WinSize = WinSize {
    ws_row: 3,
    ws_col: 20,
};

/// Han and kana from Hiragino, Hangul from Apple SD Gothic Neo.
const WIDE: &str = "水中国あ한가";

/// Where each wide character's ink box sits on row 0, in pixels from the
/// centre of its two cells.
///
/// The renderer is made at `made` px and rescaled to `px` when they differ:
/// a zoom resets the atlas, which must take every face's new offset.
fn offsets(faces: &[Face<'_>], made: f32, px: f32) -> Vec<f64> {
    let mut r = Renderer::new(
        Stack::new(faces).unwrap(),
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
    let bytes = format!("{WIDE}\x1b[3;1H");
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
    (0..WIDE.chars().count())
        .map(|k| {
            let x0 = 2 * k * cw;
            // The span itself: its neighbours are wide characters too.
            // Each column's darkest pixel, then the ink's extent with the
            // edge columns' partial coverage: a box centred in the span,
            // which is what centring means for a glyph whose ink is not
            // symmetric (가's weight is all on one side, by design).
            let cols: Vec<f64> = (x0..x0 + 2 * cw)
                .map(|x| {
                    (0..ch)
                        .map(|y| {
                            let p = canvas.pixels[y * canvas.width + x];
                            ((green(p) - bg) / (fg - bg)).clamp(0.0, 1.0)
                        })
                        .fold(0.0, f64::max)
                })
                .collect();
            let lo = cols.iter().position(|&a| a > 0.02).unwrap();
            let hi = cols.iter().rposition(|&a| a > 0.02).unwrap();
            let left = lo as f64 + 1.0 - cols[lo];
            let right = hi as f64 + cols[hi];
            (left + right) / 2.0 - cw as f64
        })
        .collect()
}

/// Where `c`'s outline box sits in its own advance, in pixels from the
/// advance's middle at `px`: the designer's offset, which centring the
/// advance keeps. `가`'s box is well off its advance's middle, by design.
fn designed(faces: &[Face<'_>], c: char, px: f32) -> f64 {
    let stack = Stack::new(faces).unwrap();
    let cp = Codepoint::from_char(c);
    let f = stack.face(stack.select(cp));
    let g = f.glyph(cp).unwrap();
    let mut b = Bounds::new();
    f.outline(g, &mut b);
    let ink = b.ink().unwrap();
    let adv = f64::from(f.default_advance(g).unwrap().value());
    let mid = f64::from(ink.left + ink.right) / 2.0;
    f64::from(Scale::of(px, f.units_per_em()).px(1.0)) * (mid - adv / 2.0)
}

/// Every character's box lands where centring its advance in two cells puts
/// it: within 0.05 px for Han, kana and 한, and 0.27 px for 가, whose faint
/// stroke ends the extent measure reads. The grid phase left them 1-2 px off.
/// The last two sizes are made at one size and zoomed to the other.
#[test]
fn a_cjk_fallback_face_is_centred_in_its_two_cells() {
    if !cfg!(target_os = "macos") {
        eprintln!("skipped: the CJK faces are macOS's");
        return;
    }
    let jbm = font("JetBrainsMono-Regular.ttf");
    let hiragino = std::fs::read(HIRAGINO).unwrap();
    let gothic = std::fs::read(GOTHIC).unwrap();
    let faces = [
        Face::parse(&jbm).unwrap(),
        Face::parse_index(&hiragino, FaceIndex::of(0)).unwrap(),
        Face::parse_index(&gothic, FaceIndex::of(0)).unwrap(),
    ];
    let sizes = [
        (13.0, 13.0),
        (14.0, 14.0),
        (16.0, 16.0),
        (26.0, 26.0),
        (28.0, 28.0),
        (14.0, 26.0),
        (26.0, 13.0),
    ];
    for (made, px) in sizes {
        for (c, off) in WIDE.chars().zip(offsets(&faces, made, px)) {
            let want = designed(&faces, c, px);
            assert!(
                (off - want).abs() <= 0.3,
                "{c} at {px} px: its box is {off:+.3} px from the centre of its two cells, \
                 where the font puts it {want:+.3} px from the middle of its advance"
            );
        }
    }
}
