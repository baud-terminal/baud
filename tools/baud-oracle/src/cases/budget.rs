//! A variable font whose glyphs need more scratch than a renderer's budget,
//! with a .notdef that has ink.
//!
//! Writes `crates/baud-font/tests/var/budget-cases.ttf`. A renderer draws a varied
//! glyph through scratch, one point a slot, and caps the scratch at
//! HarfBuzz's HB_GLYF_MAX_POINTS (200,000); a glyph past the cap cannot be
//! drawn, and TUS 5.3 leaves such a character "some placeholder", so the
//! cell must show the next face's glyph or this face's .notdef, never a
//! blank. ink-cases.ttf carries such a glyph but its .notdef is empty, so a
//! placeholder drawn from it cannot be told from a blank; this face's is a
//! box.
//!
//! ```text
//! .notdef       a box with a hole, the placeholder
//! a             a box whose top and right edges move with wght, so the
//!               face carries gvar and is read at a variation (M maps here
//!               too, because a renderer takes its cell width from M)
//! many, mid, heavy
//!               heavy is 40 mid, each 50 many, each 120 points: 240,000
//!               points flattened in 2,041 visits, past the budget at a
//!               variation; at the default instance it streams, with no
//!               scratch, and draws
//! heavym        heavy, then a placed by matched points (a's point 0 on
//!               heavy's point 0): drawn through scratch at every
//!               instance, so past the budget at the default one too
//! colrheavy     COLR v0, empty itself, paints heavy and a: its layers
//!               cannot draw at a variation and its own outline is empty
//! colrpast      COLR v0 painting heavy and a, whose own outline is 40 mid
//!               as heavy's is: at a variation neither its layers nor its
//!               outline can draw
//! full          a simple glyph of 65,536 points, the most a 16-bit
//!               endPtsOfContours can number, all on one spot so the glyph
//!               takes under a kilobyte: at a variation it needs 65,540,
//!               the most any simple glyph can, which a test of the need
//!               that skips simple glyphs must hold to; unmapped
//! colrink       COLR v0 painting heavy and a over an outline of its own,
//!               a's box: at a variation its layers cannot draw and its
//!               outline can, so it draws that outline as text
//! reflayer      a placed by matched points first, then a: refused at
//!               every instance
//! colrref       COLR v0 painting reflayer and a over a's box: its layers
//!               cannot draw at any instance and its outline can
//! ```
//!
//! cmap: a and M to a, H to heavy, K to heavym, D to colrheavy, E to
//! colrpast, U+E123 to colrink, U+E124 to colrref; JetBrains Mono maps
//! neither of the last two. Axes: wght 100-400-900, no avar; only a varies,
//! by even deltas at the extremes. lsb is xMin. heavy's 240,000 points do
//! not fit maxp's 16-bit totals, so maxp's are written as values,
//! saturated. Deterministic: the head timestamps are fixed. baud's own
//! font, under the repository's licence (`crates/baud-font/tests/licences.rs`
//! lists it).

use write_fonts::TableWriter;
use write_fonts::tables::glyf::{
    Anchor, Bbox, Component, ComponentFlags, CompositeGlyph, GlyfLocaBuilder, Glyph, SimpleGlyph,
    Transform,
};
use write_fonts::validate::{Validate, ValidationCtx};

use super::{
    Totals, ZERO_EXTREMES, all, bbox, bytes, cmap, colr_v0, contours, cpal_v0, font, fvar, gid,
    gvar, head, hhea, hmtx, maxp, name, os2, os2_bytes, post_v2, rect, tuple,
};

const ORDER: [&str; 12] = [
    ".notdef",
    "a",
    "many",
    "mid",
    "heavy",
    "heavym",
    "colrheavy",
    "colrpast",
    "full",
    "colrink",
    "reflayer",
    "colrref",
];

/// The points of `full`: 65,536, one more than a 16-bit count can hold
/// and as many as a 16-bit last index can name.
const FULL: usize = 65_536;

/// **Written here: `full`.** write-fonts numbers a contour's last point as
/// `(cur as u16 - 1)` (`SimpleGlyph::write_into`), which is 0 - 1 for a
/// contour of 65,536 points; so the glyph's bytes are written here: one
/// contour, its box (300, 300, 300, 300), endPtsOfContours 65,535, no
/// instructions, then the flags and coordinates as the glyf text packs
/// them -- the first point on the curve with both coordinates words, the
/// 65,535 after it on the curve with both the same as the one before
/// (flags 0x31), repeated in runs (0x39 and a count) of at most 256.
struct Full;

impl Full {
    fn bytes() -> Vec<u8> {
        let mut b = vec![];
        b.extend(1i16.to_be_bytes());
        for v in [300i16; 4] {
            b.extend(v.to_be_bytes());
        }
        b.extend(u16::try_from(FULL - 1).expect("a u16").to_be_bytes());
        b.extend(0u16.to_be_bytes());
        b.push(0x01);
        let mut left = FULL - 1;
        while left > 0 {
            let run = left.min(256);
            if run == 1 {
                b.push(0x31);
            } else {
                b.push(0x39);
                b.push(u8::try_from(run - 1).expect("at most 255 repeats"));
            }
            left -= run;
        }
        b.extend(300i16.to_be_bytes());
        b.extend(300i16.to_be_bytes());
        b
    }
}

impl write_fonts::FontWrite for Full {
    fn write_into(&self, writer: &mut TableWriter) {
        writer.write_slice(&Full::bytes());
        writer.pad_to_2byte_aligned();
    }
}

impl Validate for Full {
    fn validate_impl(&self, _: &mut ValidationCtx) {}
}

impl write_fonts::tables::glyf::SomeGlyph for Full {}

fn component(n: &str, anchor: Anchor) -> Component {
    Component::new(
        gid(&ORDER, n),
        anchor,
        Transform::default(),
        ComponentFlags::default(),
    )
}

const AT_ORIGIN: Anchor = Anchor::Offset { x: 0, y: 0 };

/// `n` placed `count` times at the origin, bounded by its box.
fn repeated(n: &str, count: usize, b: Bbox) -> CompositeGlyph {
    let mut g = CompositeGlyph::new(component(n, AT_ORIGIN), b);
    for _ in 1..count {
        g.add_component(component(n, AT_ORIGIN), b);
    }
    g
}

pub fn fonts() -> Vec<(&'static str, Vec<u8>)> {
    // The hole runs the other way round.
    let notdef = contours(&[
        &rect(50, 0, 550, 700),
        &[(150, 100), (150, 600), (450, 600), (450, 100)],
    ]);
    let a = contours(&[&rect(100, 0, 500, 500)]);
    // 120 points: thirty squares of four, each its own contour.
    let squares: Vec<[(i16, i16); 4]> = (0..30)
        .map(|i| {
            let (x, y) = (10 + (i % 10) * 50, 10 + (i / 10) * 50);
            rect(x, y, x + 30, y + 30)
        })
        .collect();
    let many = contours(&squares.iter().map(|s| &s[..]).collect::<Vec<_>>());
    let mid = repeated("many", 50, many.bbox);
    let heavy = repeated("mid", 40, mid.bbox);
    // a's point 0 (100, 0) laid on heavy's point 0 (10, 10) moves a by
    // (-90, 10): the box is heavy's and a's there.
    let mut heavym = CompositeGlyph::new(component("heavy", AT_ORIGIN), heavy.bbox);
    heavym.add_component(
        component(
            "a",
            Anchor::Point {
                base: 0,
                component: 0,
            },
        ),
        bbox(10, 10, 410, 510),
    );
    let colrpast = repeated("mid", 40, mid.bbox);
    // A first component placed by points has no position to bound it
    // from, so reflayer's box is a's, written as a value.
    let mut reflayer = CompositeGlyph::new(
        component(
            "a",
            Anchor::Point {
                base: 0,
                component: 0,
            },
        ),
        a.bbox,
    );
    reflayer.add_component(component("a", AT_ORIGIN), a.bbox);
    let boxes: Vec<i16> = vec![
        notdef.bbox.x_min,
        a.bbox.x_min,
        many.bbox.x_min,
        mid.bbox.x_min,
        heavy.bbox.x_min,
        heavym.bbox.x_min,
        0,
        colrpast.bbox.x_min,
        300,
        a.bbox.x_min,
        reflayer.bbox.x_min,
        a.bbox.x_min,
    ];
    let mut b = GlyfLocaBuilder::new();
    let simple = |b: &mut GlyfLocaBuilder, g: &SimpleGlyph| {
        b.add_glyph(g).expect("a glyph");
    };
    let comp = |b: &mut GlyfLocaBuilder, g: &CompositeGlyph| {
        b.add_glyph(g).expect("a glyph");
    };
    simple(&mut b, &notdef);
    simple(&mut b, &a);
    simple(&mut b, &many);
    comp(&mut b, &mid);
    comp(&mut b, &heavy);
    comp(&mut b, &heavym);
    b.add_glyph(&Glyph::Empty).expect("a glyph");
    comp(&mut b, &colrpast);
    b.add_glyph(&Full).expect("a glyph");
    simple(&mut b, &a);
    comp(&mut b, &reflayer);
    simple(&mut b, &a);
    let (glyf, loca, format) = b.build();
    let metrics: Vec<(u16, i16)> = boxes.iter().map(|&x| (600, x)).collect();
    let (hmtx, long) = hmtx(&metrics);
    let g = |n| gid(&ORDER, n);
    let map = [
        (0x61, g("a")),
        (0x4D, g("a")),
        (0x48, g("heavy")),
        (0x4B, g("heavym")),
        (0x44, g("colrheavy")),
        (0x45, g("colrpast")),
        (0xE123, g("colrink")),
        (0xE124, g("colrref")),
    ];
    let z = (0, 0);
    // Top edge up and right edge out at 900; down and in at 100. Then four
    // still phantoms.
    let gvar = gvar(
        &ORDER,
        1,
        vec![(
            "a",
            vec![
                tuple(
                    &[(0.0, 1.0, 1.0)],
                    &all(&[z, (40, 0), (40, 100), (0, 100), z, z, z, z]),
                ),
                tuple(
                    &[(-1.0, -1.0, 0.0)],
                    &all(&[z, (-20, 0), (-20, -80), (0, -80), z, z, z, z]),
                ),
            ],
        )],
    );
    let layers: &[(&str, u16)] = &[("heavy", 0), ("a", 1)];
    let totals = Totals {
        points: 65_535,
        contours: 30,
        composite_points: 65_535,
        composite_contours: 65_535,
        component_elements: 50,
        component_depth: 3,
    };
    let font = font(vec![
        (
            b"head",
            bytes(&head(0x0003, 0, bbox(0, 0, 600, 700), Some(format))),
        ),
        (b"hhea", bytes(&hhea(900, -200, &ZERO_EXTREMES, long))),
        (b"maxp", bytes(&maxp(ORDER.len() as u16, &totals))),
        (b"OS/2", os2_bytes(&os2(600, 0x44, 0xE124, 0x1000_0000))),
        (b"hmtx", bytes(&hmtx)),
        (b"cmap", bytes(&cmap(&map))),
        (b"loca", bytes(&loca)),
        (b"glyf", bytes(&glyf)),
        (
            b"name",
            bytes(&name("baud budget cases", "Regular", &["Weight"])),
        ),
        (b"post", bytes(&post_v2(&ORDER))),
        (b"fvar", bytes(&fvar(&[(b"wght", 100.0, 400.0, 900.0)]))),
        (b"gvar", gvar),
        (
            b"COLR",
            colr_v0(
                &ORDER,
                &[
                    ("colrheavy", layers),
                    ("colrpast", layers),
                    ("colrink", layers),
                    ("colrref", &[("reflayer", 0), ("a", 1)]),
                ],
            ),
        ),
        (b"CPAL", cpal_v0(&[(0xFF, 0, 0, 0xFF), (0, 0, 0xFF, 0xFF)])),
    ]);
    vec![("crates/baud-font/tests/var/budget-cases.ttf", font)]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `full` reads back as one contour of 65,536 points, every one on the
    /// curve at (300, 300). Decoded here by the glyf text: read-fonts
    /// 0.46.0 counts a contour's points as `last + 1` in a u16 and gives
    /// none for this glyph.
    #[test]
    fn full_is_65536_points_on_one_spot() {
        let b = Full::bytes();
        let be = |at: usize| i16::from_be_bytes([b[at], b[at + 1]]);
        assert_eq!(be(0), 1, "one contour");
        assert_eq!((be(2), be(4), be(6), be(8)), (300, 300, 300, 300));
        assert_eq!(u16::from_be_bytes([b[10], b[11]]), 65_535);
        assert_eq!(be(12), 0, "no instructions");
        let mut flags = vec![];
        let mut at = 14;
        while flags.len() < FULL {
            let f = b[at];
            at += 1;
            let n = if f & 0x08 != 0 {
                at += 1;
                1 + usize::from(b[at - 1])
            } else {
                1
            };
            flags.extend(std::iter::repeat_n(f & !0x08, n));
        }
        assert_eq!(flags.len(), FULL);
        assert_eq!(flags[0], 0x01, "on the curve, both coordinates words");
        assert!(
            flags[1..].iter().all(|&f| f == 0x31),
            "on, x and y the same"
        );
        assert_eq!((be(at), be(at + 2)), (300, 300));
        assert_eq!(b.len(), at + 4);
    }
}
