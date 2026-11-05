//! A variable font built to reach what JetBrains Mono never makes the bake
//! do.
//!
//! Writes `baud-font/tests/var/bake/cases.ttf`. Its instances beside it,
//! `cases-<wght>.ttf` at 250, 550, 700 and 875, are fontTools 4.63.0's
//! (`instantiateVariableFont` with its defaults, recalcTimestamp off), kept
//! as observations of that engine as `bake/VERSION` names it, and are not
//! written here. JetBrains Mono has no glyph program, no degenerate
//! component, no composite whose offsets stay whole beside one whose do
//! not, no left-phantom delta, one advance, no intermediate region and no
//! avar; the bake's rules for each were green under every perturbation
//! until this font. The glyphs:
//!
//! ```text
//! sq     one contour with a glyph program; a tuple over every point that
//!        moves the left phantom (so lsb is no longer xMin) and the advance,
//!        a sparse tuple that wraps round the contour (inference), and an
//!        intermediate region, whose scalar is not a 2.14 value
//! dot    a single point: a degenerate glyph
//! wide   another advance, which moves, so OS/2's average width does
//! intc   dot and sq, no variation data: every offset stays whole, so its
//!        bounds are its components' -- dot skipped as degenerate
//! fracc  dot and sq with fractional deltas on both offsets, one pushed
//!        from a byte to a word: it is flattened, dot included
//! nestc  fracc and intc, whole offsets: the union of the two bounds above;
//!        all three flag a program of no bytes
//! instc  sq under USE_MY_METRICS and ROUND_XY_TO_GRID, with a program
//! acc    a mark, attached to sq by anchors that vary through GDEF's store;
//!        GDEF names no mark glyph sets, so it drops to 1.0, not 1.2
//! e1-e3  empty; e2 and e3 share an advance, the trailing run hmtx trims
//!        to one record, which leaves its 46 bytes padded; twelve glyphs
//!        leave the short loca's 26 padded too. Their side bearings are
//!        not zero, so neither is what a compaction that left stale bytes
//!        in the padding would leave there
//! ```
//!
//! Axis: wght 100-400-900 with an avar. The mark anchors vary between
//! wght 100, 400 and 900, which the avar maps to -1, 0 and 1, so GDEF's
//! store holds their deltas at the two extremes. Deterministic: the head
//! timestamps are fixed, at 3,600,000,000 seconds after 1904.

use read_fonts::tables::glyf::CurvePoint;
use write_fonts::tables::avar::{Avar, AxisValueMap, SegmentMaps};
use write_fonts::tables::gdef::Gdef;
use write_fonts::tables::glyf::{
    Anchor, Bbox, Component, ComponentFlags, CompositeGlyph, Glyph, SimpleGlyph, Transform,
};
use write_fonts::tables::gpos::{
    AnchorFormat1, AnchorFormat3, AnchorTable, BaseArray, BaseRecord, Gpos, MarkArray, MarkBasePos,
    MarkBasePosFormat1, MarkRecord, PositionLookup, PositionLookupList,
};
use write_fonts::tables::layout::{
    ClassDef, CoverageTable, DeviceOrVariationIndex, Feature, FeatureList, FeatureRecord, LangSys,
    Lookup, LookupFlag, Script, ScriptList, ScriptRecord, VariationIndex,
};
use write_fonts::tables::variations::{
    ItemVariationData, ItemVariationStore, RegionAxisCoordinates, VariationRegion,
    VariationRegionList,
};
use write_fonts::types::{F2Dot14, GlyphId16, Tag};

use super::{
    Extremes, Totals, all, bbox, bytes, cmap, empty_programs, font, fvar, gid, glyph_box, gvar,
    head, hhea, hmtx, maxp, name, os2, os2_bytes, poly, post_v2, tuple,
};

const ORDER: [&str; 12] = [
    ".notdef", "sq", "dot", "wide", "intc", "fracc", "nestc", "instc", "acc", "e1", "e2", "e3",
];

/// A composite's components: (glyph, x, y, flags), and its program; an
/// empty one is still flagged, as [`super::EmptyProgram`] writes it.
type Parts = (
    &'static [(&'static str, i16, i16, ComponentFlags)],
    &'static [u8],
);

const PLAIN: ComponentFlags = ComponentFlags {
    round_xy_to_grid: false,
    use_my_metrics: false,
    scaled_component_offset: false,
    unscaled_component_offset: false,
    overlap_compound: false,
};

const COMPOSITES: [(&str, Parts); 4] = [
    (
        "intc",
        (&[("dot", 650, 900, PLAIN), ("sq", 0, 0, PLAIN)], b""),
    ),
    (
        "fracc",
        (&[("dot", 0, 800, PLAIN), ("sq", 120, 0, PLAIN)], b""),
    ),
    (
        "nestc",
        (&[("fracc", 5, 5, PLAIN), ("intc", 1000, 0, PLAIN)], b""),
    ),
    (
        "instc",
        (
            &[
                (
                    "sq",
                    0,
                    0,
                    ComponentFlags {
                        round_xy_to_grid: true,
                        use_my_metrics: true,
                        ..PLAIN
                    },
                ),
                ("dot", -300, 40, PLAIN),
            ],
            // PUSHB[0] 2, POP.
            b"\xb0\x02\x21",
        ),
    ),
];

/// A composite bounded by its components' boxes at their offsets, each
/// box without area (a lone point) left out.
fn composite(glyphs: &[Glyph], (parts, program): &Parts) -> CompositeGlyph {
    let placed = |&(n, x, y, _): &(&str, i16, i16, ComponentFlags)| -> Option<Bbox> {
        let b = glyph_box(&glyphs[gid(&ORDER, n).to_u32() as usize])?;
        (b.x_min != b.x_max || b.y_min != b.y_max)
            .then(|| bbox(b.x_min + x, b.y_min + y, b.x_max + x, b.y_max + y))
    };
    let union = parts
        .iter()
        .filter_map(placed)
        .reduce(|a, b| a.union(b))
        .expect("a box");
    let component = |&(n, x, y, flags): &(&str, i16, i16, ComponentFlags)| {
        Component::new(
            gid(&ORDER, n),
            Anchor::Offset { x, y },
            Transform::default(),
            flags,
        )
    };
    let mut g = CompositeGlyph::new(component(&parts[0]), union);
    for p in &parts[1..] {
        g.add_component(component(p), union);
    }
    g.set_instructions(program);
    g
}

fn region(start: f32, peak: f32, end: f32) -> VariationRegion {
    VariationRegion::new(vec![RegionAxisCoordinates {
        start_coord: F2Dot14::from_f32(start),
        peak_coord: F2Dot14::from_f32(peak),
        end_coord: F2Dot14::from_f32(end),
    }])
}

/// GDEF: sq and wide are bases, acc a mark, and the store holds each
/// varied anchor coordinate's deltas at wght 100 and 900 (-1 and 1).
fn gdef(order: &[&str]) -> Gdef {
    let classes: ClassDef = [("sq", 1), ("wide", 1), ("acc", 3)]
        .iter()
        .map(|&(n, c)| (GlyphId16::new(gid(order, n).to_u32() as u16), c))
        .collect();
    // The rows, in the store's order: sq's top anchor x (280, 300, 337),
    // acc's anchor x (240, 250, 273), sq's top anchor y (505, 500, 531).
    let rows: [[i8; 2]; 3] = [[-20, 37], [-10, 23], [5, 31]];
    let deltas = rows.iter().flatten().map(|&d| d as u8).collect();
    let store = ItemVariationStore::new(
        VariationRegionList::new(1, vec![region(-1.0, -1.0, 0.0), region(0.0, 1.0, 1.0)]),
        vec![Some(ItemVariationData::new(3, 0, vec![0, 1], deltas))],
    );
    let mut g = Gdef::new(Some(classes), None, None, None);
    g.item_var_store = Some(store).into();
    g
}

fn var(inner: u16) -> Option<DeviceOrVariationIndex> {
    Some(DeviceOrVariationIndex::VariationIndex(VariationIndex::new(
        0, inner,
    )))
}

/// GPOS: one mark-to-base lookup under `mark` for DFLT. Class 0 is dot's,
/// on plain anchors; class 1 acc's, on anchors whose varied coordinates
/// name GDEF's rows.
fn gpos(order: &[&str]) -> Gpos {
    let cov = |names: &[&str]| -> CoverageTable {
        names
            .iter()
            .map(|n| GlyphId16::new(gid(order, n).to_u32() as u16))
            .collect()
    };
    let marks = MarkArray::new(vec![
        MarkRecord::new(0, AnchorTable::Format1(AnchorFormat1::new(250, 0))),
        MarkRecord::new(
            1,
            AnchorTable::Format3(AnchorFormat3::new(250, 600, var(1), None)),
        ),
    ]);
    let bases = BaseArray::new(vec![BaseRecord::new(vec![
        Some(AnchorTable::Format1(AnchorFormat1::new(300, 0))),
        Some(AnchorTable::Format3(AnchorFormat3::new(
            300,
            500,
            var(0),
            var(2),
        ))),
    ])]);
    let subtable = MarkBasePos::Format1(MarkBasePosFormat1::new(
        cov(&["dot", "acc"]),
        cov(&["sq"]),
        marks,
        bases,
    ));
    let lookup = Lookup::new(LookupFlag::empty(), vec![subtable]);
    Gpos::new(
        ScriptList::new(vec![ScriptRecord::new(
            Tag::new(b"DFLT"),
            Script::new(Some(LangSys::new(vec![0])), vec![]),
        )]),
        FeatureList::new(vec![FeatureRecord::new(
            Tag::new(b"mark"),
            Feature::new(None, vec![0]),
        )]),
        PositionLookupList::new(vec![PositionLookup::MarkToBase(lookup)]),
    )
}

fn avar() -> Avar {
    let map = |from: f32, to: f32| AxisValueMap {
        from_coordinate: F2Dot14::from_f32(from),
        to_coordinate: F2Dot14::from_f32(to),
    };
    Avar::new(vec![SegmentMaps::new(vec![
        map(-1.0, -1.0),
        map(-0.5, -0.3),
        map(0.0, 0.0),
        map(0.4, 0.55),
        map(1.0, 1.0),
    ])])
}

pub fn fonts() -> Vec<(&'static str, Vec<u8>)> {
    // A line, a quadratic through (600, 250), a line and the close.
    let mut sq = SimpleGlyph {
        contours: vec![
            vec![
                CurvePoint::on_curve(100, 0),
                CurvePoint::on_curve(500, 0),
                CurvePoint::off_curve(600, 250),
                CurvePoint::on_curve(500, 500),
                CurvePoint::on_curve(100, 500),
            ]
            .into(),
        ],
        // PUSHB[0] 1, POP: a program is bytes to the bake, carried through.
        instructions: b"\xb0\x01\x21".to_vec(),
        ..Default::default()
    };
    sq.recompute_bounding_box();
    let mut glyphs = vec![
        Glyph::Empty,
        Glyph::Simple(sq),
        Glyph::Simple(poly(&[(250, 250)])),
        Glyph::Simple(poly(&[(50, 0), (1200, 0), (1200, 300)])),
    ];
    for (_, parts) in &COMPOSITES {
        let g = composite(&glyphs, parts);
        glyphs.push(Glyph::Composite(g));
    }
    glyphs.push(Glyph::Simple(poly(&[(200, 600), (300, 600), (250, 700)])));
    glyphs.extend([Glyph::Empty, Glyph::Empty, Glyph::Empty]);
    let metrics: Vec<(u16, i16)> = ORDER
        .iter()
        .zip(&glyphs)
        .map(|(n, g)| {
            let lsb = glyph_box(g).map_or(0, |b| b.x_min);
            // The empty glyphs keep side bearings of their own, so what the
            // trimmed hmtx leaves behind it is not zero by accident.
            match *n {
                "wide" => (1300, lsb),
                "acc" => (0, lsb),
                "e1" => (700, 7),
                "e2" => (900, 5),
                "e3" => (900, 3),
                _ => (900, lsb),
            }
        })
        .collect();
    let (glyf, loca, format) = empty_programs(&glyphs);
    let (hmtx, long) = hmtx(&metrics);
    let map: Vec<(u32, _)> = ORDER[1..]
        .iter()
        .enumerate()
        .map(|(i, n)| (0x41 + i as u32, gid(&ORDER, n)))
        .collect();
    let up = [(0.0, 1.0, 1.0)];
    let down = [(-1.0, -1.0, 0.0)];
    let z = (0, 0);
    let gvar = gvar(
        &ORDER,
        1,
        vec![
            (
                "sq",
                vec![
                    // 5 points, then left, right, top, bottom phantoms.
                    tuple(
                        &up,
                        &all(&[
                            (10, 0),
                            (20, 5),
                            (31, -10),
                            (0, 15),
                            (-5, 5),
                            (7, 0),
                            (45, 0),
                            z,
                            z,
                        ]),
                    ),
                    tuple(
                        &down,
                        &[
                            None,
                            Some((-20, -10)),
                            None,
                            None,
                            Some((15, 21)),
                            Some((-6, 0)),
                            None,
                            None,
                            None,
                        ],
                    ),
                    tuple(
                        &[(0.2, 0.5, 0.8)],
                        &[
                            None,
                            None,
                            Some((41, 43)),
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                        ],
                    ),
                ],
            ),
            ("dot", vec![tuple(&up, &all(&[(50, 50), z, z, z, z]))]),
            (
                "wide",
                vec![tuple(&up, &all(&[z, (40, 0), (40, 7), z, (133, 0), z, z]))],
            ),
            (
                "fracc",
                vec![
                    tuple(&up, &all(&[(7, 3), (21, -5), z, z, z, z])),
                    tuple(&down, &all(&[(-3, 11), (-9, 2), z, z, z, z])),
                ],
            ),
            ("instc", vec![tuple(&up, &all(&[z, (-17, 13), z, z, z, z]))]),
        ],
    );
    let extremes = Extremes {
        advance_width_max: 1300,
        min_lsb: 50,
        min_rsb: -700,
        x_max_extent: 1600,
    };
    let totals = Totals {
        points: 5,
        contours: 1,
        composite_points: 12,
        composite_contours: 4,
        component_elements: 2,
        component_depth: 2,
    };
    let font = font(vec![
        (
            b"head",
            bytes(&head(
                0x0003,
                3_600_000_000,
                bbox(50, 0, 1600, 700),
                Some(format),
            )),
        ),
        (b"hhea", bytes(&hhea(900, -100, &extremes, long))),
        (b"maxp", bytes(&maxp(ORDER.len() as u16, &totals))),
        (b"OS/2", os2_bytes(&os2(333, 0x41, 0x4B, 0))),
        (b"hmtx", bytes(&hmtx)),
        (b"cmap", bytes(&cmap(&map))),
        (b"loca", loca),
        (b"glyf", glyf),
        (
            b"name",
            bytes(&name("baud bake cases", "Regular", &["Weight"])),
        ),
        (b"post", bytes(&post_v2(&ORDER))),
        (b"fvar", bytes(&fvar(&[(b"wght", 100.0, 400.0, 900.0)]))),
        (b"avar", bytes(&avar())),
        (b"gvar", gvar),
        (b"GDEF", bytes(&gdef(&ORDER))),
        (b"GPOS", bytes(&gpos(&ORDER))),
    ]);
    vec![("baud-font/tests/var/bake/cases.ttf", font)]
}
