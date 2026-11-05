//! A variable font with no GPOS, built so the shaper's fallback mark
//! placement reads glyph boxes at a variation.
//!
//! Writes `baud-font/tests/var/ink-cases.ttf`. A face without GPOS places
//! its marks geometrically against the base's ink box and the mark's own
//! (HarfBuzz's hb-ot-shape-fallback, which baud follows), so a varied face
//! must hand that placement its varied boxes. Every committed variable
//! font carries GPOS, so none reaches the fallback; this one does:
//!
//! ```text
//! a, o          bases whose top and side edges move with wght
//! grave, acute, macron, ring
//!               marks above (combining class 230) that widen and rise
//! dotbelow      a mark below (220) that sinks
//! cedilla       a mark attached below (202)
//! many, mid, heavy
//!               heavy is 40 mid, each 50 many, each 120 points: 240,000
//!               points flattened in 2,041 visits -- inside HarfBuzz's edge
//!               bound and past HB_GLYF_MAX_POINTS (200,000), so HarfBuzz
//!               refuses it and a renderer that caps its scratch there
//!               refuses it too, and counts it
//! colr, colrheavy
//!               COLR v0 glyphs, empty themselves: colr paints a and acute,
//!               colrheavy paints heavy and a, so a layered glyph at a
//!               variation is drawn from varied layers, and one whose layer
//!               is past the budget is refused whole
//! ```
//!
//! Every delta is even and the samples sit at wght 100, 250, 650 and 900
//! -- scalars of -1, -0.5, 0.5 and 1 -- so every varied coordinate is a
//! whole unit and no rounding rule is under test. No delta moves a left
//! phantom, so HarfBuzz's shift is zero. lsb is xMin. heavy's 240,000
//! points do not fit maxp's 16-bit maxCompositePoints, so the totals are
//! written saturated, and head's box by hand. Axes: wght 100-400-900, no
//! avar. Deterministic: the head timestamps are fixed. baud's own font,
//! under the repository's licence (`baud-font/tests/licences.rs` lists
//! it).

use write_fonts::tables::glyf::{
    Anchor, Bbox, Component, ComponentFlags, CompositeGlyph, Glyph, Transform,
};

use super::tuples::{gvar, tuple};
use super::{
    Totals, ZERO_EXTREMES, all, bbox, bytes, cmap, colr_v0, contours, cpal_v0, empty_programs,
    font, fvar, gid, glyph_box, head, hhea, hmtx, maxp, name, os2, os2_bytes, poly, post_v2, rect,
};

const ORDER: [&str; 14] = [
    ".notdef",
    "a",
    "o",
    "grave",
    "acute",
    "macron",
    "ring",
    "dotbelow",
    "cedilla",
    "many",
    "mid",
    "heavy",
    "colr",
    "colrheavy",
];

const MARKS: [&str; 6] = ["grave", "acute", "macron", "ring", "dotbelow", "cedilla"];

/// A box's edges: x from, y from, x to, y to.
type Edges = (i16, i16, i16, i16);

/// Each box glyph's edges.
const BOXES: [(&str, Edges); 8] = [
    ("a", (100, 0, 500, 500)),
    ("o", (50, 0, 550, 400)),
    ("grave", (220, 560, 320, 640)),
    ("acute", (250, 550, 350, 650)),
    ("macron", (150, 600, 450, 640)),
    ("ring", (240, 580, 360, 700)),
    ("dotbelow", (260, -150, 340, -70)),
    ("cedilla", (280, -180, 330, 0)),
];

/// `n` copies of `inner` at the origin, bounded as `many` is.
fn composite(inner: &str, n: usize, b: Bbox) -> CompositeGlyph {
    let c = || {
        Component::new(
            gid(&ORDER, inner),
            Anchor::Offset { x: 0, y: 0 },
            Transform::default(),
            ComponentFlags::default(),
        )
    };
    let mut g = CompositeGlyph::new(c(), b);
    for _ in 1..n {
        g.add_component(c(), b);
    }
    g
}

pub fn fonts() -> Vec<(&'static str, Vec<u8>)> {
    let mut glyphs = vec![Glyph::Empty];
    for (_, (x0, y0, x1, y1)) in BOXES {
        glyphs.push(Glyph::Simple(poly(&rect(x0, y0, x1, y1))));
    }
    // 120 points: thirty squares of four, each its own contour.
    let squares: Vec<[(i16, i16); 4]> = (0..30)
        .map(|i| {
            let (x, y) = (10 + (i % 10) * 50, 10 + (i / 10) * 50);
            rect(x, y, x + 30, y + 30)
        })
        .collect();
    let list: Vec<&[(i16, i16)]> = squares.iter().map(|s| &s[..]).collect();
    let many = contours(&list);
    let b = many.bbox;
    glyphs.push(Glyph::Simple(many));
    glyphs.push(Glyph::Composite(composite("many", 50, b)));
    glyphs.push(Glyph::Composite(composite("mid", 40, b)));
    glyphs.extend([Glyph::Empty, Glyph::Empty]);
    let metrics: Vec<(u16, i16)> = ORDER
        .iter()
        .zip(&glyphs)
        .map(|(n, g)| {
            let advance = if MARKS.contains(n) { 0 } else { 600 };
            (advance, glyph_box(g).map_or(0, |b| b.x_min))
        })
        .collect();
    let (glyf, loca, format) = empty_programs(&glyphs);
    let (hmtx, long) = hmtx(&metrics);
    // M maps too, to o, because a renderer takes its cell width from M.
    let map: Vec<(u32, _)> = [
        (0x61, "a"),
        (0x6F, "o"),
        (0x300, "grave"),
        (0x301, "acute"),
        (0x304, "macron"),
        (0x30A, "ring"),
        (0x323, "dotbelow"),
        (0x327, "cedilla"),
        (0x48, "heavy"),
        (0x4D, "o"),
        (0x43, "colr"),
        (0x44, "colrheavy"),
    ]
    .iter()
    .map(|&(c, n)| (c, gid(&ORDER, n)))
    .collect();
    let up = [(0.0, 1.0, 1.0)];
    let down = [(-1.0, -1.0, 0.0)];
    let z = (0, 0);
    // Deltas for a box's four points, then four still phantoms.
    let tv = |region: &[(f64, f64, f64)], four: [(i16, i16); 4]| {
        let mut d = four.to_vec();
        d.extend([z; 4]);
        tuple(region, &all(&d))
    };
    // Top edge up and right edge out at 900; down and in at 100.
    let gvar = gvar(
        &ORDER,
        1,
        vec![
            (
                "a",
                vec![
                    tv(&up, [z, (40, 0), (40, 100), (0, 100)]),
                    tv(&down, [z, (-20, 0), (-20, -80), (0, -80)]),
                ],
            ),
            (
                "o",
                vec![
                    tv(&up, [(-40, 0), z, (0, 60), (-40, 60)]),
                    tv(&down, [(20, 0), z, (0, -40), (20, -40)]),
                ],
            ),
            ("grave", vec![tv(&up, [z, (20, 0), (20, 0), z])]),
            (
                "acute",
                vec![
                    tv(&up, [z, (60, 0), (60, 40), (0, 40)]),
                    tv(&down, [z, (-40, 0), (-40, -20), (0, -20)]),
                ],
            ),
            (
                "macron",
                vec![tv(&up, [(-50, 0), (50, 0), (50, 20), (-50, 20)])],
            ),
            (
                "ring",
                vec![tv(&up, [(0, 30), (40, 30), (40, 70), (0, 70)])],
            ),
            (
                "dotbelow",
                vec![
                    tv(&up, [(0, -40), (20, -40), (20, 0), z]),
                    tv(&down, [(0, 20), (-10, 20), (-10, 0), z]),
                ],
            ),
            ("cedilla", vec![tv(&up, [(0, -60), (30, -60), (30, 0), z])]),
        ],
    );
    let colr = colr_v0(
        &ORDER,
        &[
            ("colr", &[("a", 0), ("acute", 1)]),
            ("colrheavy", &[("heavy", 0), ("a", 1)]),
        ],
    );
    let cpal = cpal_v0(&[(255, 0, 0, 255), (0, 0, 255, 255)]);
    let totals = Totals {
        points: 120,
        contours: 30,
        composite_points: 65535,
        composite_contours: 65535,
        component_elements: 50,
        component_depth: 2,
    };
    // Basic Latin and Combining Diacritical Marks.
    let mut os2 = os2(600, 0x43, 0x327, 0);
    os2.ul_unicode_range_1 = 0x41;
    let font = font(vec![
        (
            b"head",
            bytes(&head(0x0003, 0, bbox(0, -240, 600, 760), Some(format))),
        ),
        (b"hhea", bytes(&hhea(900, -200, &ZERO_EXTREMES, long))),
        (b"maxp", bytes(&maxp(ORDER.len() as u16, &totals))),
        (b"OS/2", os2_bytes(&os2)),
        (b"hmtx", bytes(&hmtx)),
        (b"cmap", bytes(&cmap(&map))),
        (b"loca", loca),
        (b"glyf", glyf),
        (
            b"name",
            bytes(&name("baud ink cases", "Regular", &["Weight"])),
        ),
        (b"post", bytes(&post_v2(&ORDER))),
        (b"COLR", colr),
        (b"CPAL", cpal),
        (b"fvar", bytes(&fvar(&[(b"wght", 100.0, 400.0, 900.0)]))),
        (b"gvar", gvar),
    ]);
    vec![("baud-font/tests/var/ink-cases.ttf", font)]
}
