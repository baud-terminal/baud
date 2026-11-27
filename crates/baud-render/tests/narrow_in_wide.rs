//! A glyph narrower than its two cells is centred in them (note Z, decision B).
//!
//! A character the terminal gives two cells is drawn from whatever face the
//! stack selects, and that face's advance is rarely two of the grid's cells:
//! JetBrains Mono's own `⚡` and fullwidth braces are one cell, Menlo's `☕`
//! one, Apple Symbols' `⺀` 0.95, Apple SD Gothic Neo's `〶` 1.44 and Apple
//! Color Emoji's 1.67. Before this, a glyph took the grid's phase unless its
//! face mapped 水, so the JetBrains Mono, Menlo, Apple Symbols and emoji
//! fixtures sat left of the middle of their span -- a one-cell advance 4 px
//! at 14 px, `🌍` 1.1 -- while Hiragino's and SD Gothic's were already
//! centred by that face's 水. The renderer takes the phase per glyph from
//! the cluster's base: in a two-cell span, half the slack between the span
//! and the base's advance; in one cell, the grid's. A Private Use Area icon
//! keeps the grid's, because the Nerd Fonts patcher's rules own its place.
//!
//! The faces are macOS's; elsewhere the macOS tests say so and pass.

mod support;

use baud_core::{Owned, Term};
use baud_domain::Codepoint;
use baud_domain::{BytePos, Cols, ReadLen, Rows, WinSize};
use baud_font::{Bounds, Face, FaceIndex, Scale};
use baud_render::{Blend, CellSpan, Damage, FaceStyle, PxSize, Renderer, Stack, Theme};
use support::font;

const WS: WinSize = WinSize {
    ws_row: 3,
    ws_col: 40,
};

const MENLO: &str = "/System/Library/Fonts/Menlo.ttc";
const EMOJI: &str = "/System/Library/Fonts/Apple Color Emoji.ttc";
const SYMBOLS: &str = "/System/Library/Fonts/Apple Symbols.ttf";
const HIRAGINO: &str = "/System/Library/Fonts/Hiragino Sans GB.ttc";
const GOTHIC: &str = "/System/Library/Fonts/AppleSDGothicNeo.ttc";

/// Every one is East Asian Wide and drawn from a face whose advance is not
/// two cells: JetBrains Mono (`⚡ ﹢ ｛ ｝`, one cell), Menlo (`☕ ♈`, one),
/// Apple Symbols (`⺀`, 0.95) and Apple SD Gothic Neo (`〶`, 1.44).
const OUTLINED: &str = "⚡﹢｛｝☕♈⺀〶";

/// A round emoji, so its artwork is centred in its advance: `⌚`'s is not,
/// by design, and would test the artist rather than the placement.
const EMOJI_ROUND: &str = "🌍";

const SIZES: [(f32, f32); 7] = [
    (13.0, 13.0),
    (14.0, 14.0),
    (16.0, 16.0),
    (26.0, 26.0),
    (28.0, 28.0),
    (14.0, 26.0),
    (26.0, 13.0),
];

struct Data {
    jbm: Vec<u8>,
    menlo: Vec<u8>,
    emoji: Vec<u8>,
    symbols: Vec<u8>,
    hiragino: Vec<u8>,
    gothic: Vec<u8>,
}

impl Data {
    fn read() -> Data {
        let sys = |p: &str| std::fs::read(p).unwrap();
        Data {
            jbm: font("JetBrainsMono-Regular.ttf"),
            menlo: sys(MENLO),
            emoji: sys(EMOJI),
            symbols: sys(SYMBOLS),
            hiragino: sys(HIRAGINO),
            gothic: sys(GOTHIC),
        }
    }

    fn faces(&self) -> [Face<'_>; 6] {
        [
            Face::parse(&self.jbm).unwrap(),
            Face::parse_index(&self.menlo, FaceIndex::of(0)).unwrap(),
            Face::parse_index(&self.emoji, FaceIndex::of(0)).unwrap(),
            Face::parse(&self.symbols).unwrap(),
            Face::parse_index(&self.hiragino, FaceIndex::of(0)).unwrap(),
            Face::parse_index(&self.gothic, FaceIndex::of(0)).unwrap(),
        ]
    }
}

fn renderer<'f, 'a>(faces: &'f [Face<'a>], made: f32, px: f32) -> Renderer<'f, 'a> {
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
    r
}

/// Where each character of `text` -- every one two cells wide -- puts its
/// ink box on row 0, in pixels from the middle of its two cells.
///
/// The renderer is made at `made` px and rescaled to `px` when they differ:
/// a zoom resets the atlas, whose key carries the phase.
fn offsets(faces: &[Face<'_>], text: &str, made: f32, px: f32) -> Vec<f64> {
    let mut r = renderer(faces, made, px);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let bytes = format!("{text}\x1b[3;1H");
    t.write_at(BytePos::ZERO, bytes.as_bytes());
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    let mut canvas = support::Canvas::for_renderer(&r);
    support::paint(&mut r, &mut t, &mut canvas, Damage::Full);

    let m = r.metrics();
    let cw = usize::try_from(m.cell_w.value()).unwrap();
    let ch = usize::try_from(m.cell_h.value()).unwrap();
    let bg = Theme::dark().bg.0;
    // Coverage from the channel furthest from the background, so a colour
    // glyph's ink counts as well as a grey one's.
    let ink = |p: u32| {
        (0..3)
            .map(|k| ((p >> (8 * k)) & 0xff).abs_diff((bg >> (8 * k)) & 0xff))
            .max()
            .map_or(0.0, |d| f64::from(d) / 255.0)
    };
    (0..text.chars().count())
        .map(|k| {
            let x0 = 2 * k * cw;
            let cols: Vec<f64> = (x0..x0 + 2 * cw)
                .map(|x| {
                    (0..ch)
                        .map(|y| ink(canvas.pixels[y * canvas.width + x]))
                        .fold(0.0, f64::max)
                })
                .collect();
            let lo = cols.iter().position(|&a| a > 0.02).unwrap();
            let hi = cols.iter().rposition(|&a| a > 0.02).unwrap();
            let left = lo as f64 + 1.0 - cols[lo].min(1.0);
            let right = hi as f64 + cols[hi].min(1.0);
            (left + right) / 2.0 - cw as f64
        })
        .collect()
}

/// Where `c`'s outline box sits in its own advance at `px`, in pixels from
/// the advance's middle: the designer's offset, which centring keeps.
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

/// Every outlined fixture's box lands where centring its advance in two
/// cells puts it, within 0.4 px -- `⺀`'s single thin stroke is what the
/// extent measure reads least well. The grid's phase leaves the one-cell
/// advances 4-9 px off and `〶` 2-5.
/// The last two sizes are made at one size and zoomed to the other.
#[test]
fn a_glyph_narrower_than_its_two_cells_is_centred_in_them() {
    if !cfg!(target_os = "macos") {
        eprintln!("skipped: the fallback faces are macOS's");
        return;
    }
    let data = Data::read();
    let faces = data.faces();
    for (made, px) in SIZES {
        for (c, off) in OUTLINED.chars().zip(offsets(&faces, OUTLINED, made, px)) {
            let want = designed(&faces, c, px);
            assert!(
                (off - want).abs() <= 0.4,
                "{c} at {px} px (made at {made}): its box is {off:+.3} px from the middle \
                 of its two cells, where the font puts it {want:+.3} px from the middle \
                 of its advance"
            );
        }
    }
}

/// A colour emoji, whose advance is 1.67 cells, is centred too: within
/// 0.5 px, where the grid's phase left it 1-3 px left.
#[test]
fn a_colour_emoji_is_centred_in_its_two_cells() {
    if !cfg!(target_os = "macos") {
        eprintln!("skipped: the emoji face is macOS's");
        return;
    }
    let data = Data::read();
    let faces = data.faces();
    for (made, px) in SIZES {
        let off = offsets(&faces, EMOJI_ROUND, made, px)[0];
        assert!(
            off.abs() <= 0.5,
            "{EMOJI_ROUND} at {px} px (made at {made}): {off:+.3} px from the middle \
             of its two cells"
        );
    }
}

/// A mark moves with its base: every glyph of a cluster in a two-cell span
/// takes the base's phase, which is the centring one and not the grid's.
#[test]
fn every_glyph_of_a_cluster_takes_its_bases_place() {
    let jbm = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&jbm).unwrap()];
    let stack = Stack::new(&faces).unwrap();
    let mut r = renderer(&faces, 14.0, 14.0);
    let base = Codepoint::from_char('⚡');
    let acute = Codepoint::from_char('\u{0301}');
    let face = stack.select(base);
    let centred = r.phase_in(face, base, CellSpan::Two);
    assert_ne!(
        centred,
        r.metrics().phase,
        "control: the base is centred off the grid's phase"
    );
    let glyphs = r.cluster(&[base, acute], FaceStyle::Regular, CellSpan::Two);
    assert_eq!(glyphs.len(), 2, "the base and its mark");
    for g in &glyphs {
        assert_eq!(g.phase, centred, "every glyph takes the base's phase");
    }
    // And in one cell, the grid's.
    for g in &r.cluster(&[base, acute], FaceStyle::Regular, CellSpan::One) {
        assert_eq!(g.phase, r.metrics().phase);
    }
}

/// An icon keeps the grid's phase in two cells: the Nerd Fonts patcher's
/// rules place it, not centring. The control is a text glyph of the same
/// one-cell advance, which centring moves.
#[test]
fn an_icon_keeps_the_grid_phase() {
    let nerd = std::fs::read(format!(
        "{}/../baud-app/fonts/symbols-nerd-font/SymbolsNerdFontMono-Regular.ttf",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let jbm = font("JetBrainsMono-Regular.ttf");
    let faces = [Face::parse(&jbm).unwrap(), Face::parse(&nerd).unwrap()];
    let stack = Stack::new(&faces).unwrap();
    let r = renderer(&faces, 14.0, 14.0);
    let grid = r.metrics().phase;
    for v in [0xE738u32, 0xF0186] {
        let icon = Codepoint::new(v).unwrap();
        let face = stack.select(icon);
        assert_eq!(face.index(), 1, "U+{v:04X} is the icon face's");
        assert_eq!(r.phase_in(face, icon, CellSpan::Two), grid, "U+{v:04X}");
    }
    let bolt = Codepoint::from_char('⚡');
    assert_ne!(
        r.phase_in(stack.select(bolt), bolt, CellSpan::Two),
        grid,
        "control: a text glyph of one cell's advance is centred"
    );
}
