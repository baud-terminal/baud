//! A variable font built to reach what the real fixture fonts never do.
//!
//! Writes `crates/baud-font/tests/var/gvar-cases.ttf`. JetBrains Mono and Fira
//! Code have no left-phantom delta, no intermediate region, one axis, and
//! no composite with its own variation data under USE_MY_METRICS; this
//! font has all of those, so HarfBuzz's drawing of it (`baud-oracle`'s
//! `gvar`, `outlines.txt`) decides the rules the real fonts leave untested:
//!
//! ```text
//! sq    one contour, a quadratic bump; a tuple over every point that moves
//!       the left phantom (the outline shifts), a sparse tuple (inference),
//!       an intermediate region, a two-axis peak, a one-point sparse tuple
//! two   two contours, the first with a run of off-curve points; sparse
//!       tuples with one referenced point per contour, one whose referenced
//!       neighbours share an x, three sparse tuples that score together,
//!       and an invalid intermediate region
//! one   a single point, which draws nothing
//! comp  sq and two as components, scaled and offset, sq under
//!       USE_MY_METRICS; the composite's own tuple moves both offsets and
//!       its left phantom (which USE_MY_METRICS then overrides)
//! nest  comp and a rotated sq; a sparse tuple on one component offset
//! empty no contours; its phantoms vary
//! fan64, edge2049, edge2050
//!       HarfBuzz's graph-edge bound (HB_MAX_GRAPH_EDGE_COUNT, 2048,
//!       checked before each visit is counted): fan64 is 64 empty
//!       components; edge2049 is 31 of it, 32 empties and sq -- 2,049
//!       glyphs visited, which HarfBuzz draws -- and edge2050 one empty
//!       more, which it refuses. The square is what makes the refusal
//!       visible.
//! ```
//!
//! Every glyph's lsb is its xMin, so HarfBuzz's absolute shift is the left
//! phantom's delta. Axes: wght 100-400-900 and wdth 50-100-200, no avar.
//! Deterministic: the head timestamps are fixed. baud's own font, under
//! the repository's licence (`crates/baud-font/tests/licences.rs` lists it).

use read_fonts::tables::glyf::CurvePoint;
use write_fonts::tables::glyf::{
    Anchor, Bbox, Component, ComponentFlags, CompositeGlyph, Glyph, SimpleGlyph, Transform,
};
use write_fonts::types::F2Dot14;

use super::tuples::{Tuple, gvar, tuple};
use super::{
    Extremes, Totals, all, bbox, bytes, cmap, empty_programs, font, fvar, gid, glyph_box, head,
    hhea, hmtx, maxp, name, os2, os2_bytes, post_v2,
};

const ORDER: [&str; 10] = [
    ".notdef", "sq", "two", "one", "comp", "nest", "empty", "fan64", "edge2049", "edge2050",
];

/// A component: its glyph, offset, matrix (xx, yx, xy, yy) and
/// USE_MY_METRICS.
type Part = (&'static str, (i16, i16), (f32, f32, f32, f32), bool);

const ONE: (f32, f32, f32, f32) = (1.0, 0.0, 0.0, 1.0);

fn component(&(n, (x, y), (xx, yx, xy, yy), umm): &Part) -> Component {
    let f = F2Dot14::from_f32;
    Component::new(
        gid(&ORDER, n),
        Anchor::Offset { x, y },
        Transform {
            xx: f(xx),
            yx: f(yx),
            xy: f(xy),
            yy: f(yy),
        },
        ComponentFlags {
            use_my_metrics: umm,
            ..Default::default()
        },
    )
}

/// A composite with the header box fontTools bounded its placed points by.
fn composite(parts: &[Part], b: Bbox) -> CompositeGlyph {
    let mut g = CompositeGlyph::new(component(&parts[0]), b);
    for p in &parts[1..] {
        g.add_component(component(p), b);
    }
    g.bbox = b;
    g
}

fn repeated(n: &'static str, count: usize) -> Vec<Part> {
    vec![(n, (0, 0), ONE, false); count]
}

fn simple(contours: &[&[CurvePoint]]) -> SimpleGlyph {
    let mut g = SimpleGlyph {
        contours: contours.iter().map(|c| c.to_vec().into()).collect(),
        ..Default::default()
    };
    g.recompute_bounding_box();
    g
}

pub fn fonts() -> Vec<(&'static str, Vec<u8>)> {
    let (on, off) = (CurvePoint::on_curve, CurvePoint::off_curve);
    let sq = simple(&[&[
        on(100, 0),
        on(500, 0),
        off(600, 250),
        on(500, 500),
        on(100, 500),
    ]]);
    // One on-curve point, then three off-curve ones: implied midpoints. It
    // starts on the curve because a contour that starts on two off-curve
    // points is where baud's emission (ttf-parser's) and HarfBuzz's
    // (FreeType's) pick different start points for the same closed shape.
    let two = simple(&[
        &[on(100, 300), off(300, 100), off(400, 300), off(300, 500)],
        &[on(500, 0), on(700, 0), on(700, 400)],
    ]);
    let one = simple(&[&[on(250, 250)]]);
    let comp = composite(
        &[
            ("sq", (50, 20), ONE, true),
            ("two", (300, 100), (0.5, 0.0, 0.0, 0.5), false),
        ],
        bbox(150, 20, 650, 520),
    );
    let nest = composite(
        &[
            ("comp", (10, 10), ONE, false),
            ("sq", (0, 600), (0.8, 0.2, -0.2, 0.8), false),
        ],
        bbox(-20, 30, 660, 1100),
    );
    let fan64 = composite(&repeated("empty", 64), bbox(0, 0, 0, 0));
    let edge = |empties| {
        let mut parts = repeated("fan64", 31);
        parts.extend(repeated("empty", empties));
        parts.push(("sq", (0, 0), ONE, false));
        composite(&parts, bbox(100, 0, 600, 500))
    };
    let glyphs = vec![
        Glyph::Empty,
        Glyph::Simple(sq),
        Glyph::Simple(two),
        Glyph::Simple(one),
        Glyph::Composite(comp),
        Glyph::Composite(nest),
        Glyph::Empty,
        Glyph::Composite(fan64),
        Glyph::Composite(edge(32)),
        Glyph::Composite(edge(33)),
    ];
    let metrics: Vec<(u16, i16)> = glyphs
        .iter()
        .map(|g| (900, glyph_box(g).map_or(0, |b| b.x_min)))
        .collect();
    let (glyf, loca, format) = empty_programs(&glyphs);
    let (hmtx, long) = hmtx(&metrics);
    let map: Vec<(u32, _)> = ORDER[1..]
        .iter()
        .enumerate()
        .map(|(i, n)| (0x41 + i as u32, gid(&ORDER, n)))
        .collect();
    let gvar = gvar(&ORDER, 2, variations());
    // hhea's extremes and maxp's totals as fontTools recalculated them.
    let extremes = Extremes {
        advance_width_max: 900,
        min_lsb: -20,
        min_rsb: 200,
        x_max_extent: 700,
    };
    let totals = Totals {
        points: 7,
        contours: 2,
        composite_points: 17,
        composite_contours: 4,
        component_elements: 65,
        component_depth: 2,
    };
    let font = font(vec![
        (
            b"head",
            bytes(&head(0x0003, 0, bbox(-20, 0, 700, 1100), Some(format))),
        ),
        (b"hhea", bytes(&hhea(900, -100, &extremes, long))),
        (b"maxp", bytes(&maxp(ORDER.len() as u16, &totals))),
        (b"OS/2", os2_bytes(&os2(900, 0x41, 0x49, 0))),
        (b"hmtx", bytes(&hmtx)),
        (b"cmap", bytes(&cmap(&map))),
        (b"loca", loca),
        (b"glyf", glyf),
        (
            b"name",
            bytes(&name("baud gvar cases", "Regular", &["Weight", "Width"])),
        ),
        (b"post", bytes(&post_v2(&ORDER))),
        (
            b"fvar",
            bytes(&fvar(&[
                (b"wght", 100.0, 400.0, 900.0),
                (b"wdth", 50.0, 100.0, 200.0),
            ])),
        ),
        (b"gvar", gvar),
    ]);
    vec![("crates/baud-font/tests/var/gvar-cases.ttf", font)]
}

/// Each glyph's tuples, a region per axis (wght, wdth); `None` leaves a
/// point to inference.
fn variations() -> Vec<(&'static str, Vec<Tuple>)> {
    const N: (f64, f64, f64) = (0.0, 0.0, 0.0);
    const UP: (f64, f64, f64) = (0.0, 1.0, 1.0);
    const DOWN: (f64, f64, f64) = (-1.0, -1.0, 0.0);
    let w = |r| [r, N];
    let d = |r| [N, r];
    let z = Some((0, 0));
    let s = Some;
    vec![
        (
            "sq",
            // 5 points, then left, right, top, bottom phantoms.
            vec![
                tuple(
                    &w(UP),
                    &all(&[
                        (10, 0),
                        (20, 5),
                        (30, -10),
                        (0, 15),
                        (-5, 5),
                        (7, 0),
                        (25, 0),
                        (0, 0),
                        (0, 0),
                    ]),
                ),
                tuple(
                    &w(DOWN),
                    &[
                        s((-20, -10)),
                        None,
                        None,
                        s((15, 20)),
                        None,
                        s((-6, 0)),
                        None,
                        None,
                        None,
                    ],
                ),
                tuple(
                    &w((0.2, 0.5, 0.8)),
                    &[None, None, s((40, 40)), None, None, None, None, None, None],
                ),
                tuple(
                    &[UP, (0.0, 0.5, 1.0)],
                    &all(&[
                        (3, 3),
                        (3, 3),
                        (3, 3),
                        (3, 3),
                        (3, 3),
                        (0, 0),
                        (0, 0),
                        (0, 0),
                        (0, 0),
                    ]),
                ),
                tuple(
                    &d(DOWN),
                    &[None, s((0, -30)), None, None, None, None, None, None, None],
                ),
            ],
        ),
        (
            "two",
            // 4 + 3 points, then the phantoms.
            vec![
                // Two points of the first contour move apart, so its shape
                // changes: a later sparse tuple that read these positions
                // instead of the defaults would infer differently.
                tuple(
                    &w(UP),
                    &[
                        s((12, 0)),
                        None,
                        s((-10, 6)),
                        None,
                        None,
                        s((0, 18)),
                        None,
                        None,
                        None,
                        None,
                        None,
                    ],
                ),
                tuple(
                    &d(UP),
                    &[
                        None,
                        None,
                        s((-9, 4)),
                        None,
                        s((5, 5)),
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                    ],
                ),
                // Points 5 and 6 share an x (700) and are both referenced:
                // point 4, between them round the contour, takes the
                // equal-position rule.
                tuple(
                    &w(DOWN),
                    &[
                        None,
                        None,
                        None,
                        None,
                        None,
                        s((3, 0)),
                        s((4, 0)),
                        None,
                        None,
                        None,
                        None,
                    ],
                ),
                // A third sparse tuple scoring beside the first two (wght
                // and wdth both above default): the positions inference
                // reads are the defaults, captured once, not what the
                // earlier flushes left.
                tuple(
                    &[UP, UP],
                    &[
                        None,
                        s((-7, 11)),
                        None,
                        s((6, -4)),
                        None,
                        None,
                        s((5, 9)),
                        None,
                        None,
                        None,
                        None,
                    ],
                ),
                // An invalid intermediate region (it straddles 0 with a
                // nonzero peak): the axis is ignored, except that a
                // coordinate of 0 still ends the tuple first.
                tuple(
                    &d((-0.5, 0.5, 1.0)),
                    &[
                        s((2, 2)),
                        s((2, 2)),
                        s((2, 2)),
                        s((2, 2)),
                        s((2, 2)),
                        s((2, 2)),
                        s((2, 2)),
                        z,
                        z,
                        z,
                        z,
                    ],
                ),
            ],
        ),
        (
            "comp",
            // 2 component offsets, then the phantoms.
            vec![tuple(
                &w(UP),
                &all(&[(30, 10), (-15, 25), (12, 0), (40, 0), (0, 0), (0, 0)]),
            )],
        ),
        (
            "nest",
            vec![tuple(&d(UP), &[None, s((20, -20)), None, None, None, None])],
        ),
        (
            "one",
            vec![tuple(
                &w(UP),
                &all(&[(50, 50), (9, 0), (0, 0), (0, 0), (0, 0)]),
            )],
        ),
        (
            "empty",
            vec![tuple(&w(UP), &all(&[(5, 0), (60, 0), (0, 0), (0, 0)]))],
        ),
    ]
}
