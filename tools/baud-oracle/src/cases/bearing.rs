//! Two variable fonts whose glyphs' left side bearings are not their xMin.
//!
//! Writes `baud-font/tests/var/bearing-cases.ttf`, with `head` flags bit 1
//! clear, and `bearing-cases-bit1.ttf`, the same font with bit 1 set. The
//! left side bearing point -- the first phantom point, the origin -- sits at
//! `pp1 = xMin - lsb` (`[[source: ms-opentype-hmtx]]`,
//! `[[source: ms-opentype-head]]`), and a glyph is drawn from it: "The
//! origin point is the original pen position. Imagine that the pen moves
//! the left-side bearing distance, draws the glyph"
//! (`[[source: apple-ttrm-ch2]]`). With bit 1 set the `head` text puts that
//! point at x = 0 for every glyph, and requires xMin and the lsb to be
//! equal, which these glyphs break: the second font is the malformed case,
//! drawn from `xMin - lsb` all the same (note Z, step 8, question 6).
//!
//! Simple glyphs, all a 60 x 60 box at x 20 (xMin 20), polygons that start
//! on the curve:
//!
//! ```text
//! box     lsb 0: pp1 20
//! neg     lsb 50: pp1 -30
//! same    lsb 20, xMin: pp1 0, and its left phantom moves +10 at wght 900
//! boxvar  lsb 0: pp1 20, its left phantom moves +30 and its first point
//!         +10 in x at wght 900
//! ```
//!
//! Composites, each header box written by hand, not one bounded from its
//! components (no bounding can match a phantom point):
//!
//! ```text
//! comp     box at (100, 0): xMin 120, lsb 105, pp1 15; its own left
//!          phantom moves +20 at wght 900 (the gvar text applies X alone)
//! umm      neg at (0, 0) under USE_MY_METRICS, then box at (200, 0): the
//!          composite takes neg's phantoms, pp1 -30
//! ummlast  neg, then box at (200, 0), both under USE_MY_METRICS: the last
//!          one's, box's, taken before it is placed: pp1 20
//! ummnest  umm under USE_MY_METRICS, then box at (300, 0): umm's, which is
//!          neg's: pp1 -30
//! anch     box, then neg with its point 0 laid on box's point 1 (80, 0):
//!          the matched walk at the default instance; xMin 20, lsb -5,
//!          pp1 25
//! anchlsb  box, then neg with its left phantom, point 4, laid on box's
//!          point 1: neg's phantom starts at its pp1, -30, so neg moves
//!          110; xMin 20, lsb 20, pp1 0
//! ```
//!
//! Axis: wght 100-400-900, no avar; only the 900 side carries deltas, so
//! 650 is half of each. Deterministic: the head timestamps are fixed.
//! baud's own font, under the repository's licence
//! (`baud-font/tests/licences.rs` lists it).

use write_fonts::tables::glyf::{
    Anchor, Bbox, Component, ComponentFlags, CompositeGlyph, Glyph, Transform,
};

use super::{
    all, bbox, bytes, cmap, font, fvar, gid, glyf_loca, gvar, head, hhea, hmtx, maxp, name, os2,
    os2_bytes, poly, post_v2, rect, tuple,
};

const ORDER: [&str; 11] = [
    ".notdef", "box", "neg", "same", "boxvar", "comp", "umm", "ummlast", "ummnest", "anch",
    "anchlsb",
];

/// Each simple glyph's lsb.
const SIMPLE: [(&str, i16); 4] = [("box", 0), ("neg", 50), ("same", 20), ("boxvar", 0)];

/// Where a component goes: by offset, or by (parent, child) points.
enum Place {
    At(i16, i16),
    Points(u16, u16),
}

/// A composite: its components (glyph, place, USE_MY_METRICS), its header
/// box and its lsb.
type Composite = (
    &'static str,
    &'static [(&'static str, Place, bool)],
    (i16, i16, i16, i16),
    i16,
);

const COMPOSITES: [Composite; 6] = [
    (
        "comp",
        &[("box", Place::At(100, 0), false)],
        (120, 0, 180, 60),
        105,
    ),
    (
        "umm",
        &[
            ("neg", Place::At(0, 0), true),
            ("box", Place::At(200, 0), false),
        ],
        (20, 0, 280, 60),
        20,
    ),
    (
        "ummlast",
        &[
            ("neg", Place::At(0, 0), true),
            ("box", Place::At(200, 0), true),
        ],
        (20, 0, 280, 60),
        20,
    ),
    (
        "ummnest",
        &[
            ("umm", Place::At(0, 0), true),
            ("box", Place::At(300, 0), false),
        ],
        (20, 0, 380, 60),
        20,
    ),
    (
        "anch",
        &[
            ("box", Place::At(0, 0), false),
            ("neg", Place::Points(1, 0), false),
        ],
        (20, 0, 140, 60),
        -5,
    ),
    (
        "anchlsb",
        &[
            ("box", Place::At(0, 0), false),
            ("neg", Place::Points(1, 4), false),
        ],
        (20, 0, 190, 60),
        20,
    ),
];

fn composite(parts: &[(&str, Place, bool)], b: Bbox) -> CompositeGlyph {
    let component = |(n, place, umm): &(&str, Place, bool)| {
        Component::new(
            gid(&ORDER, n),
            match *place {
                Place::At(x, y) => Anchor::Offset { x, y },
                Place::Points(base, component) => Anchor::Point { base, component },
            },
            Transform::default(),
            ComponentFlags {
                use_my_metrics: *umm,
                ..Default::default()
            },
        )
    };
    let mut g = CompositeGlyph::new(component(&parts[0]), b);
    for p in &parts[1..] {
        g.add_component(component(p), b);
    }
    g.bbox = b;
    g
}

fn build(flags: u16) -> Vec<u8> {
    let mut glyphs = vec![Glyph::Empty];
    let mut metrics = vec![(600, 0)];
    for (_, lsb) in SIMPLE {
        glyphs.push(Glyph::Simple(poly(&rect(20, 0, 80, 60))));
        metrics.push((600, lsb));
    }
    for (_, parts, (x0, y0, x1, y1), lsb) in &COMPOSITES {
        glyphs.push(Glyph::Composite(composite(parts, bbox(*x0, *y0, *x1, *y1))));
        metrics.push((600, *lsb));
    }
    let (glyf, loca, format) = glyf_loca(&glyphs);
    let (hmtx, long) = hmtx(&metrics);
    let map: Vec<(u32, _)> = ORDER[1..]
        .iter()
        .enumerate()
        .map(|(i, n)| (0x41 + i as u32, gid(&ORDER, n)))
        .collect();
    let up = [(0.0, 1.0, 1.0)];
    let z = (0, 0);
    // 4 points, then the left, right, top and bottom phantoms; a
    // composite's one component slot, then its four phantoms.
    let gvar = gvar(
        &ORDER,
        1,
        vec![
            (
                "same",
                vec![tuple(&up, &all(&[z, z, z, z, (10, 0), z, z, z]))],
            ),
            (
                "boxvar",
                vec![tuple(&up, &all(&[(10, 0), z, z, z, (30, 0), z, z, z]))],
            ),
            ("comp", vec![tuple(&up, &all(&[z, (20, 0), z, z, z]))]),
        ],
    );
    font(vec![
        (
            b"head",
            bytes(&head(flags, 0, bbox(0, 0, 380, 60), Some(format))),
        ),
        (
            b"hhea",
            bytes(&hhea(900, -200, &super::ZERO_EXTREMES, long)),
        ),
        (
            b"maxp",
            bytes(&maxp(ORDER.len() as u16, &super::ZERO_TOTALS)),
        ),
        (b"OS/2", os2_bytes(&os2(600, 0x41, 0x4A, 0))),
        (b"hmtx", bytes(&hmtx)),
        (b"cmap", bytes(&cmap(&map))),
        (b"loca", loca),
        (b"glyf", glyf),
        (
            b"name",
            bytes(&name("baud bearing cases", "Regular", &["Weight"])),
        ),
        (b"post", bytes(&post_v2(&ORDER))),
        (b"fvar", bytes(&fvar(&[(b"wght", 100.0, 400.0, 900.0)]))),
        (b"gvar", gvar),
    ])
}

/// Bit 0, the baseline at y = 0, in both; bit 1 in the second alone.
pub fn fonts() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("baud-font/tests/var/bearing-cases.ttf", build(0x0001)),
        ("baud-font/tests/var/bearing-cases-bit1.ttf", build(0x0003)),
    ]
}
