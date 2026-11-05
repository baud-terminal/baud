//! A variable font whose composites place components by matched points and
//! by scaled offsets.
//!
//! Writes `baud-font/tests/var/component-cases.ttf`. None of the 2.1
//! million components of the fonts on the development Mac or in the tree is
//! anchored by point numbers (ARGS_ARE_XY_VALUES clear) or carries
//! SCALED_COMPONENT_OFFSET, so this font is the only thing that reaches
//! either (`[[source: ms-opentype-glyf]]`, "Composite glyph description").
//!
//! Simple glyphs, all polygons that start on the curve:
//!
//! ```text
//! tri   (0,0) (400,0) (200,300); its apex moves with wght
//! bar   a 100 x 40 box; its first point moves with wght
//! dot   a 60 x 60 box at x 20, lsb 20: phantoms (0,0) and (300,0)
//! dot2  the same box with lsb 0: phantoms (20,0) and (320,0)
//! pair  a composite child, two bars 200 apart: 8 points
//! ```
//!
//! Composites, by what they place (p/c: the parent's and the child's
//! point):
//!
//! ```text
//! anch       bar by points, p2/c0, on tri's apex
//! anchrot    bar rotated 90 degrees, p2/c2: the matrix before the alignment
//! anchscale  bar scaled by half, p1/c1
//! anchadv    dot, p1/c5: the child's advance phantom
//! anchlsb    dot2, p1/c4: the child's left phantom, xMin - lsb = 20
//! anchtop    dot, p1/c6: the child's top phantom (vertical; the face has no
//!            vmtx, so it is at (0, yMax))
//! anchcomp   pair, p2/c5: a composite child's flattened points
//! anchflag   bar by points with a scale and SCALED_COMPONENT_OFFSET, which
//!            the specification says is ignored without ARGS_ARE_XY_VALUES
//! anchvar    as anch, with a delta on the anchored slot (ignored) and one
//!            on tri's offset (applied)
//! nestfirst  anch first, then a bar: the inner points numbered from 0
//! nestlate   a bar first, then anch: "a point number in the parent glyph"
//!            counts from the inner composite's own first point, where
//!            HarfBuzz counts across the outer glyph
//! anchout    p50: past every point
//! anchself   p3/c1: the parent has three points, so p3 is none of them
//! anchfirst  the first component anchored, which the specification says
//!            must not be: refused
//! scaled     bar at (100,50) scaled by half, SCALED_COMPONENT_OFFSET
//! scaledrot  the same rotated 90 degrees
//! scaledxy   the same scaled (0.5, 1.5)
//! scaledneg  the same scaled (-1, 1)
//! scaledvar  as scaled, its offset moved by a delta first
//! unscaled   as scaled, UNSCALED_COMPONENT_OFFSET
//! neither    as scaled, neither flag
//! both       as scaled, both flags
//! umm        dot2 under USE_MY_METRICS, then dot: the composite takes
//!            dot2's phantoms, whose left one starts at 20, so the
//!            shift by the left phantom's movement must follow it
//! pairy      as pair, with a Y delta on its own left phantom, which the
//!            gvar text ignores ("For AW and LSB points, apply the net X
//!            delta adjustments, and ignore Y deltas")
//! anchcph    pairy, p1/c8: a composite child's left phantom
//! anchpadv   dot under USE_MY_METRICS, p4/c0: the parent's own advance
//!            phantom, numbered after tri's three points, with a Y delta on
//!            it (ignored), read before this component's metrics replace it
//! anchptop   bar, p5/c1: the parent's own top phantom (vertical; no vmtx,
//!            and the header yMax is 0, so it is at (0, 0))
//! anchplx    bar, p3/c1: the parent's own left phantom, with an X delta
//!            (applied, and the top-level shift follows it) and a Y delta
//!            (ignored)
//! ```
//!
//! Every composite's header box is (0, 0, 0, 0) and its xMin and lsb 0,
//! and every simple glyph's lsb is its xMin but dot2's, so the left phantom
//! HarfBuzz shifts a top-level glyph by sits at x 0 except in dot2 and umm,
//! at 20, and in anchplx at 650 and 900, whose own left phantom moves by
//! its X delta, 20 and 40. Axes: wght 100-400-900, no avar. Deltas are even
//! and the oracle samples sit at scalars of -1, 0.5 and 1, so the varied
//! coordinates are whole or half units. Deterministic: the head timestamps
//! are fixed. baud's own font, under the repository's licence
//! (`baud-font/tests/licences.rs` lists it).

use write_fonts::tables::glyf::{
    Anchor, Component, ComponentFlags, CompositeGlyph, Glyph, Transform,
};
use write_fonts::types::F2Dot14;

use super::{
    ZERO_EXTREMES, ZERO_TOTALS, all, bbox, bytes, cmap, font, fvar, gid, glyf_loca, gvar, head,
    hhea, hmtx, maxp, name, os2, os2_bytes, poly, post_v2, rect, tuple,
};

const SIMPLE: [&str; 4] = ["tri", "bar", "dot", "dot2"];

/// Where a component goes: by offset, or by (parent, child) points.
#[derive(Clone, Copy)]
enum Place {
    At(i16, i16),
    Points(u16, u16),
}

/// A component's transform.
#[derive(Clone, Copy)]
enum Matrix {
    One,
    Scale(f32),
    Xy(f32, f32),
    /// 90 degrees: the matrix [[0, 1], [-1, 0]].
    Rot90,
}

const SCALED: u16 = 0x0800;
const UNSCALED: u16 = 0x1000;
const UMM: u16 = 0x0200;

use Matrix::{One, Rot90, Scale, Xy};
use Place::{At, Points};

type Part = (&'static str, Place, Matrix, u16);

const TRI: Part = ("tri", At(0, 0), One, 0);

const COMPOSITES: [(&str, &[Part]); 29] = [
    (
        "pair",
        &[("bar", At(0, 0), One, 0), ("bar", At(200, 0), One, 0)],
    ),
    ("anch", &[TRI, ("bar", Points(2, 0), One, 0)]),
    ("anchrot", &[TRI, ("bar", Points(2, 2), Rot90, 0)]),
    ("anchscale", &[TRI, ("bar", Points(1, 1), Scale(0.5), 0)]),
    ("anchadv", &[TRI, ("dot", Points(1, 5), One, 0)]),
    ("anchlsb", &[TRI, ("dot2", Points(1, 4), One, 0)]),
    ("anchtop", &[TRI, ("dot", Points(1, 6), One, 0)]),
    ("anchcomp", &[TRI, ("pair", Points(2, 5), One, 0)]),
    (
        "anchflag",
        &[TRI, ("bar", Points(2, 0), Scale(0.5), SCALED)],
    ),
    ("anchvar", &[TRI, ("bar", Points(2, 0), One, 0)]),
    (
        "nestfirst",
        &[("anch", At(0, 0), One, 0), ("bar", At(500, 0), One, 0)],
    ),
    (
        "nestlate",
        &[("bar", At(500, 0), One, 0), ("anch", At(0, 0), One, 0)],
    ),
    ("anchout", &[TRI, ("bar", Points(50, 0), One, 0)]),
    ("anchself", &[TRI, ("bar", Points(3, 1), One, 0)]),
    ("anchfirst", &[("bar", Points(1, 0), One, 0), TRI]),
    ("scaled", &[TRI, ("bar", At(100, 50), Scale(0.5), SCALED)]),
    ("scaledrot", &[TRI, ("bar", At(100, 50), Rot90, SCALED)]),
    (
        "scaledxy",
        &[TRI, ("bar", At(100, 50), Xy(0.5, 1.5), SCALED)],
    ),
    (
        "scaledneg",
        &[TRI, ("bar", At(100, 50), Xy(-1.0, 1.0), SCALED)],
    ),
    (
        "scaledvar",
        &[TRI, ("bar", At(100, 50), Scale(0.5), SCALED)],
    ),
    (
        "unscaled",
        &[TRI, ("bar", At(100, 50), Scale(0.5), UNSCALED)],
    ),
    ("neither", &[TRI, ("bar", At(100, 50), Scale(0.5), 0)]),
    (
        "both",
        &[TRI, ("bar", At(100, 50), Scale(0.5), SCALED | UNSCALED)],
    ),
    (
        "umm",
        &[("dot2", At(0, 0), One, UMM), ("dot", At(100, 0), One, 0)],
    ),
    (
        "pairy",
        &[("bar", At(0, 0), One, 0), ("bar", At(200, 0), One, 0)],
    ),
    ("anchcph", &[TRI, ("pairy", Points(1, 8), One, 0)]),
    ("anchpadv", &[TRI, ("dot", Points(4, 0), One, UMM)]),
    ("anchptop", &[TRI, ("bar", Points(5, 1), One, 0)]),
    ("anchplx", &[TRI, ("bar", Points(3, 1), One, 0)]),
];

fn order() -> Vec<&'static str> {
    let mut o = vec![".notdef"];
    o.extend(SIMPLE);
    o.extend(COMPOSITES.iter().map(|c| c.0));
    o
}

/// The matrix in the order the glyf text stores a two-by-two: xscale,
/// scale01, scale10, yscale.
fn transform(m: Matrix) -> Transform {
    let f = F2Dot14::from_f32;
    let (xx, yx, xy, yy) = match m {
        One => (1.0, 0.0, 0.0, 1.0),
        Scale(s) => (s, 0.0, 0.0, s),
        Xy(x, y) => (x, 0.0, 0.0, y),
        Rot90 => (0.0, 1.0, -1.0, 0.0),
    };
    Transform {
        xx: f(xx),
        yx: f(yx),
        xy: f(xy),
        yy: f(yy),
    }
}

fn composite(order: &[&str], parts: &[Part]) -> CompositeGlyph {
    let component = |&(n, place, m, flags): &Part| {
        Component::new(
            gid(order, n),
            match place {
                At(x, y) => Anchor::Offset { x, y },
                Points(base, component) => Anchor::Point { base, component },
            },
            transform(m),
            ComponentFlags {
                use_my_metrics: flags & UMM != 0,
                scaled_component_offset: flags & SCALED != 0,
                unscaled_component_offset: flags & UNSCALED != 0,
                ..Default::default()
            },
        )
    };
    // The header box is written as zero, not bounded from the components:
    // half of these name a phantom point or a number past the glyph.
    let zero = bbox(0, 0, 0, 0);
    let mut g = CompositeGlyph::new(component(&parts[0]), zero);
    for p in &parts[1..] {
        g.add_component(component(p), zero);
    }
    g
}

pub fn fonts() -> Vec<(&'static str, Vec<u8>)> {
    let order = order();
    let mut glyphs = vec![Glyph::Empty];
    glyphs.push(Glyph::Simple(poly(&[(0, 0), (400, 0), (200, 300)])));
    glyphs.push(Glyph::Simple(poly(&rect(0, 0, 100, 40))));
    glyphs.push(Glyph::Simple(poly(&rect(20, 0, 80, 60))));
    glyphs.push(Glyph::Simple(poly(&rect(20, 0, 80, 60))));
    for (_, parts) in &COMPOSITES {
        glyphs.push(Glyph::Composite(composite(&order, parts)));
    }
    let metrics: Vec<(u16, i16)> = order
        .iter()
        .map(|n| match *n {
            "dot" => (300, 20),
            "dot2" => (300, 0),
            _ => (600, 0),
        })
        .collect();
    let (glyf, loca, format) = glyf_loca(&glyphs);
    let (hmtx, long) = hmtx(&metrics);
    let map: Vec<(u32, _)> = order[1..]
        .iter()
        .enumerate()
        .map(|(i, n)| (0x41 + i as u32, gid(&order, n)))
        .collect();
    let up = [(0.0, 1.0, 1.0)];
    let down = [(-1.0, -1.0, 0.0)];
    let z = (0, 0);
    let gvar = gvar(
        &order,
        1,
        vec![
            // 3 points, then four phantoms.
            (
                "tri",
                vec![
                    tuple(&up, &all(&[z, z, (40, 60), z, z, z, z])),
                    tuple(&down, &all(&[z, z, (-20, -30), z, z, z, z])),
                ],
            ),
            (
                "bar",
                vec![tuple(&up, &all(&[(10, 10), z, z, z, z, z, z, z]))],
            ),
            // 2 component slots, then four phantoms: the anchored slot's
            // delta is ignored, the offset slot's applied.
            (
                "anchvar",
                vec![tuple(&up, &all(&[(30, 0), (78, 78), z, z, z, z]))],
            ),
            // The delta-adjusted offset is what is scaled.
            (
                "scaledvar",
                vec![tuple(&up, &all(&[z, (20, 40), z, z, z, z]))],
            ),
            // A composite's own left and advance phantoms take only X deltas.
            ("pairy", vec![tuple(&up, &all(&[z, z, (0, 50), z, z, z]))]),
            (
                "anchpadv",
                vec![tuple(&up, &all(&[z, z, z, (30, 50), z, z]))],
            ),
            (
                "anchplx",
                vec![tuple(&up, &all(&[z, z, (40, 50), z, z, z]))],
            ),
        ],
    );
    let font = font(vec![
        (
            b"head",
            bytes(&head(0x0003, 0, bbox(0, 0, 600, 400), Some(format))),
        ),
        (b"hhea", bytes(&hhea(900, -200, &ZERO_EXTREMES, long))),
        (b"maxp", bytes(&maxp(order.len() as u16, &ZERO_TOTALS))),
        (b"OS/2", os2_bytes(&os2(582, 0x41, 0x61, 0))),
        (b"hmtx", bytes(&hmtx)),
        (b"cmap", bytes(&cmap(&map))),
        (b"loca", loca),
        (b"glyf", glyf),
        (
            b"name",
            bytes(&name("baud component cases", "Regular", &["Weight"])),
        ),
        (b"post", bytes(&post_v2(&order))),
        (b"fvar", bytes(&fvar(&[(b"wght", 100.0, 400.0, 900.0)]))),
        (b"gvar", gvar),
    ]);
    vec![("baud-font/tests/var/component-cases.ttf", font)]
}
