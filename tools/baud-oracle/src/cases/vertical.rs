//! A variable font with vertical metrics whose composites place components
//! on the vertical phantom points, and one that places a nested composite's
//! first component by points.
//!
//! Writes `baud-font/tests/var/vertical-cases.ttf`. A component placed by
//! matched points may name any phantom of the parent or the child
//! (`[[source: ms-opentype-glyf]]`: "Phantom points from the parent or the
//! child may be referenced"), and the vertical two are the "top origin"
//! and the "advance height point"
//! (`[[source: ms-opentype-tt-instructing-glyphs]]`), which `vmtx` places:
//! the top origin at `yMax + tsb`, the advance height an advance height
//! below it (`[[source: ms-opentype-vmtx]]`). `component-cases.ttf` has no
//! `vmtx`, and its composites' header boxes are all 0, so its parent's top
//! phantom sits at the origin; this font has `vhea` and `vmtx`, a top side
//! bearing and an advance height of its own on every glyph, and composites
//! whose header yMax is not 0, so no phantom coincides with another or
//! with the origin.
//!
//! Simple glyphs, polygons that start on the curve, (advance height, tsb):
//!
//! ```text
//! tri    (0,0) (400,0) (200,300), (1000, 100): top 400, bottom -600; its
//!        apex moves (40, 60) at wght 900
//! bar    a 100 x 40 box, (800, 60): top 100, bottom -700
//! dot    a 60 x 60 box at x 20, (900, 140): top 200, bottom -700; its top
//!        phantom moves (30, 50) at wght 900 -- a simple glyph's phantom
//!        takes both deltas
//! short  dot's box, (1000, 120): top 180, bottom -820; the last glyph, so
//!        its tsb is in vmtx's trailing array
//! ```
//!
//! Composites, header box (0, 0, 400, 300) but where noted, each (1000,
//! 50): top 350, bottom -650 (p/c: the parent's and the child's point):
//!
//! ```text
//! pairv   bar, then bar at (200, 0); box (0, 0, 300, 40), (700, 30): top
//!         70, bottom -630; its own top phantom moves (40, 50) at wght
//!         900, of which the gvar text applies the Y alone
//! vtop    tri, dot p1/c6: the child's top phantom
//! vbot    tri, dot p1/c7: the child's advance height phantom
//! vcomp   tri, pairv p1/c10: a composite child's top phantom
//! vshort  tri, short p1/c6: a top phantom whose tsb is in the trailing
//!         array
//! vptop   tri, bar p5/c1: the parent's own top phantom, numbered after
//!         tri's three points and its left and advance phantoms; it moves
//!         (40, 50) at wght 900, of which the Y applies
//! vpbot   tri, bar p6/c1: the parent's own advance height phantom
//! vpast   tri, bar p7/c1: past the parent's four phantoms, which no text
//!         names
//! firstm  bar p0/c0 first, then tri: a first component placed by points,
//!         which "cannot be used for the first component glyph"
//!         (`[[source: ms-opentype-glyf]]`)
//! vnest   tri, then firstm at (500, 0): the same, one level down
//! ```
//!
//! .notdef is a 500 x 700 box at x 50, so a refused glyph's placeholder
//! shows: a cell drawn from it is not blank (baud-render's
//! tests/refused_glyph.rs).
//!
//! Every glyph's lsb is its xMin, so no glyph is shifted by its left
//! phantom. The composites keep the header boxes written above: no
//! bounding from components can reach a phantom point, which every
//! composite here names; head's box is written by hand, and hhea's, vhea's
//! and maxp's totals left at zero. Axis: wght 100-400-900, no avar; only
//! the 900 side carries deltas, so 650 is half of each. Deterministic: the
//! head timestamps are fixed. baud's own font, under the repository's
//! licence (`baud-font/tests/licences.rs` lists it).

use write_fonts::tables::glyf::{
    Anchor, Component, ComponentFlags, CompositeGlyph, Glyph, Transform,
};
use write_fonts::tables::hmtx::LongMetric;
use write_fonts::tables::vhea::Vhea;
use write_fonts::tables::vmtx::Vmtx;
use write_fonts::types::{FWord, UfWord, Version16Dot16};

use super::tuples::{gvar, tuple};
use super::{
    ZERO_EXTREMES, ZERO_TOTALS, all, bbox, bytes, cmap, empty_programs, font, fvar, gid, glyph_box,
    head, hhea, hmtx, maxp, name, os2, os2_bytes, poly, post_v2, rect,
};

/// Where a component goes: by offset, or by (parent, child) points.
#[derive(Clone, Copy)]
enum Place {
    At(i16, i16),
    Points(u16, u16),
}
use Place::{At, Points};

/// A composite: its components, its header box if not the default, and
/// its (advance height, tsb) if not the default.
type Composite = (
    &'static str,
    &'static [(&'static str, Place)],
    Option<(i16, i16, i16, i16)>,
    Option<(u16, i16)>,
);

const COMPOSITES: [Composite; 10] = [
    (
        "pairv",
        &[("bar", At(0, 0)), ("bar", At(200, 0))],
        Some((0, 0, 300, 40)),
        Some((700, 30)),
    ),
    (
        "vtop",
        &[("tri", At(0, 0)), ("dot", Points(1, 6))],
        None,
        None,
    ),
    (
        "vbot",
        &[("tri", At(0, 0)), ("dot", Points(1, 7))],
        None,
        None,
    ),
    (
        "vcomp",
        &[("tri", At(0, 0)), ("pairv", Points(1, 10))],
        None,
        None,
    ),
    (
        "vshort",
        &[("tri", At(0, 0)), ("short", Points(1, 6))],
        None,
        None,
    ),
    (
        "vptop",
        &[("tri", At(0, 0)), ("bar", Points(5, 1))],
        None,
        None,
    ),
    (
        "vpbot",
        &[("tri", At(0, 0)), ("bar", Points(6, 1))],
        None,
        None,
    ),
    (
        "vpast",
        &[("tri", At(0, 0)), ("bar", Points(7, 1))],
        None,
        None,
    ),
    (
        "firstm",
        &[("bar", Points(0, 0)), ("tri", At(0, 0))],
        None,
        None,
    ),
    (
        "vnest",
        &[("tri", At(0, 0)), ("firstm", At(500, 0))],
        None,
        None,
    ),
];

fn order() -> Vec<&'static str> {
    let mut o = vec![".notdef", "tri", "bar", "dot"];
    o.extend(COMPOSITES.iter().map(|c| c.0));
    o.push("short");
    o
}

fn composite(order: &[&str], parts: &[(&str, Place)], b: (i16, i16, i16, i16)) -> CompositeGlyph {
    let b = bbox(b.0, b.1, b.2, b.3);
    let c = |&(n, place): &(&str, Place)| {
        Component::new(
            gid(order, n),
            match place {
                At(x, y) => Anchor::Offset { x, y },
                Points(base, component) => Anchor::Point { base, component },
            },
            Transform::default(),
            ComponentFlags::default(),
        )
    };
    let mut g = CompositeGlyph::new(c(&parts[0]), b);
    for p in &parts[1..] {
        g.add_component(c(p), b);
    }
    g.bbox = b;
    g
}

pub fn fonts() -> Vec<(&'static str, Vec<u8>)> {
    let order = order();
    let dot = rect(20, 0, 80, 60);
    let mut glyphs = vec![
        Glyph::Simple(poly(&rect(50, 0, 550, 700))),
        Glyph::Simple(poly(&[(0, 0), (400, 0), (200, 300)])),
        Glyph::Simple(poly(&rect(0, 0, 100, 40))),
        Glyph::Simple(poly(&dot)),
    ];
    let mut vertical = vec![(1000, 0), (1000, 100), (800, 60), (900, 140)];
    for (_, parts, b, v) in &COMPOSITES {
        glyphs.push(Glyph::Composite(composite(
            &order,
            parts,
            b.unwrap_or((0, 0, 400, 300)),
        )));
        vertical.push(v.unwrap_or((1000, 50)));
    }
    glyphs.push(Glyph::Simple(poly(&dot)));
    vertical.push((1000, 120));
    let metrics: Vec<(u16, i16)> = glyphs
        .iter()
        .map(|g| (600, glyph_box(g).map_or(0, |b| b.x_min)))
        .collect();
    let (glyf, loca, format) = empty_programs(&glyphs);
    let (hmtx, long) = hmtx(&metrics);
    // vmtx keeps its trailing run of equal advance heights as one long
    // metric, as hmtx does its advances.
    let (v, long_v) = super::hmtx(&vertical);
    let vmtx = Vmtx::new(
        v.h_metrics
            .iter()
            .map(|m| LongMetric {
                advance: m.advance,
                side_bearing: m.side_bearing,
            })
            .collect(),
        v.left_side_bearings,
    );
    let vhea = Vhea {
        version: Version16Dot16::VERSION_1_0,
        ..Vhea::new(
            FWord::new(900),
            FWord::new(-100),
            FWord::new(0),
            UfWord::new(0),
            FWord::new(0),
            FWord::new(0),
            FWord::new(0),
            0,
            0,
            0,
            long_v,
        )
    };
    let map: Vec<(u32, _)> = order[1..]
        .iter()
        .enumerate()
        .map(|(i, n)| (0x41 + i as u32, gid(&order, n)))
        .collect();
    let up = [(0.0, 1.0, 1.0)];
    let z = (0, 0);
    let gvar = gvar(
        &order,
        1,
        vec![
            // 3 points, then left, right, top, bottom.
            ("tri", vec![tuple(&up, &all(&[z, z, (40, 60), z, z, z, z]))]),
            // 4 points, then the four phantoms: the top one moves in x and
            // y.
            (
                "dot",
                vec![tuple(&up, &all(&[z, z, z, z, z, z, (30, 50), z]))],
            ),
            // 2 component slots, then the four phantoms: the top one's X
            // is ignored.
            ("pairv", vec![tuple(&up, &all(&[z, z, z, z, (40, 50), z]))]),
            ("vptop", vec![tuple(&up, &all(&[z, z, z, z, (40, 50), z]))]),
        ],
    );
    let font = font(vec![
        (
            b"head",
            bytes(&head(0x0003, 0, bbox(0, 0, 600, 400), Some(format))),
        ),
        (b"hhea", bytes(&hhea(900, -200, &ZERO_EXTREMES, long))),
        (b"maxp", bytes(&maxp(order.len() as u16, &ZERO_TOTALS))),
        (b"OS/2", os2_bytes(&os2(600, 0x41, 0x4E, 0))),
        (b"hmtx", bytes(&hmtx)),
        (b"cmap", bytes(&cmap(&map))),
        (b"loca", loca),
        (b"glyf", glyf),
        (
            b"name",
            bytes(&name("baud vertical cases", "Regular", &["Weight"])),
        ),
        (b"post", bytes(&post_v2(&order))),
        (b"fvar", bytes(&fvar(&[(b"wght", 100.0, 400.0, 900.0)]))),
        (b"gvar", gvar),
        (b"vhea", bytes(&vhea)),
        (b"vmtx", bytes(&vmtx)),
    ]);
    vec![("baud-font/tests/var/vertical-cases.ttf", font)]
}
