//! Composite components placed by matched points and by scaled offsets,
//! against the glyf text, HarfBuzz and FreeType.
//!
//! `[[source: ms-opentype-glyf]]`, "Composite glyph description": with
//! ARGS_ARE_XY_VALUES clear, "argument1 is a point number in the parent
//! glyph (from contours incorporated and re-numbered from previous
//! component glyphs); and argument2 is a point number (prior to
//! re-numbering) from the child component glyph. Phantom points from the
//! parent or the child may be referenced. The child component glyph is
//! positioned within the parent glyph by aligning the two points. If a
//! scale or transform matrix is provided, the transformation is applied to
//! the child's point before the points are aligned." With
//! SCALED_COMPONENT_OFFSET, "the x and y offset values are deemed to be in
//! the component glyph's coordinate system, and the scale transformation is
//! applied to both values"; `[[source: ms-opentype-gvar]]` adds that the
//! deltas of a point-matched component are ignored and that "it is the
//! delta-adjusted offsets that are scaled".
//!
//! `tests/var/component-cases.ttf` (`scripts/component-cases-font.py`) is
//! the fixture, since no font on this machine or in the tree reaches
//! either rule, and `tests/var/components.txt` (`scripts/component-oracle.py`)
//! holds HarfBuzz 12.3.0's and FreeType 2.14.3's drawings of every glyph at
//! the default instance and three weights. The two engines disagree on
//! fourteen glyphs; [`SIDES`] says, glyph by glyph, which drawing the text
//! makes baud's, and where the text says nothing which drawing baud keeps.

use baud_domain::GlyphId;
use baud_font::var::{Fixed, Variation};
use baud_font::{Drawn, Face, Sink, Tag, VarPoint};
use std::collections::HashMap;

fn fixture() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/var/component-cases.ttf"
    ))
    .unwrap()
}

/// Which drawing a glyph's must equal.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Side {
    /// HarfBuzz's and FreeType's, which agree.
    Both,
    /// HarfBuzz's, where FreeType departs from the text: it refuses a
    /// phantom point (`invalid composite glyph`), scales an offset by each
    /// axis's magnitude rather than through the matrix, and scales one with
    /// both flags set, which the text calls invalid and gives the default.
    Hb,
    /// FreeType's, where HarfBuzz departs from the text: it numbers a
    /// nested composite's parent points across the outer glyph.
    Ft,
    /// Neither: the text leaves the case open, and baud keeps what it drew
    /// before -- the component at offset 0 after its matrix, which is these
    /// glyphs' drawings in order, at the same weight. For Jev.
    Unmatched(&'static [&'static str]),
    /// baud's own drawing at the default instance, where the glyph does not
    /// vary: `dot2`'s xMin is not its lsb, and `umm` takes `dot2`'s
    /// phantoms under USE_MY_METRICS, so both engines draw it from its left
    /// side bearing point, 20 along. This font sets `head` bit 1, which
    /// puts that point at 0 and requires the two to be equal, so it is
    /// malformed there, and baud keeps the drawing it made before -- the
    /// point's movement alone -- for Jev (note Z, step 8;
    /// `tests/bearings.rs` holds the rule where bit 1 is clear). At a
    /// variation that moves neither, it is drawn where the default stream
    /// draws it.
    Default,
    /// Neither engine's: the text's drawing at each weight, worked by hand
    /// from `scripts/component-cases-font.py`, where both depart. FreeType
    /// refuses every phantom point; HarfBuzz moves a composite's left
    /// phantom by its Y delta, which `[[source: ms-opentype-gvar]]` ignores
    /// ("For AW and LSB points, apply the net X delta adjustments, and
    /// ignore Y deltas"), and reads a parent point past the earlier
    /// components' as the child's own, where the glyf text numbers the
    /// parent's phantoms "after the last point number of the last contour"
    /// of the points incorporated so far.
    Text(&'static [(&'static str, &'static str)]),
}

const SIDES: &[(&str, Side)] = &[
    ("tri", Side::Both),
    ("bar", Side::Both),
    ("dot", Side::Both),
    ("dot2", Side::Default),
    ("pair", Side::Both),
    ("anch", Side::Both),
    ("anchrot", Side::Both),
    ("anchscale", Side::Both),
    ("anchadv", Side::Hb),
    ("anchlsb", Side::Hb),
    // The top phantom is vertical, and baud reads no `vmtx`.
    ("anchtop", Side::Unmatched(&["tri", "dot"])),
    ("anchcomp", Side::Both),
    ("anchflag", Side::Both),
    ("anchvar", Side::Both),
    ("nestfirst", Side::Both),
    ("nestlate", Side::Ft),
    // Past every point: HarfBuzz leaves it at 0 too.
    ("anchout", Side::Hb),
    // p3 of a parent with three points is its left phantom, (0, 0): bar's
    // second point laid on it. HarfBuzz reaches the child's own first
    // point, which agrees only where bar's first point does not move.
    (
        "anchself",
        Side::Text(&[
            ("-", "0,0;400,0;200,300|-100,0;0,0;0,40;-100,40"),
            ("100", "0,0;400,0;180,270|-100,0;0,0;0,40;-100,40"),
            ("650", "0,0;400,0;220,330|-95,5;0,0;0,40;-100,40"),
            ("900", "0,0;400,0;240,360|-90,10;0,0;0,40;-100,40"),
        ]),
    ),
    // The first component anchored, which the text says it must not be.
    ("anchfirst", Side::Unmatched(&["bar", "tri"])),
    ("scaled", Side::Both),
    ("scaledrot", Side::Hb),
    ("scaledxy", Side::Both),
    ("scaledneg", Side::Hb),
    ("scaledvar", Side::Both),
    ("unscaled", Side::Both),
    ("neither", Side::Both),
    ("both", Side::Hb),
    ("umm", Side::Default),
    ("pairy", Side::Both),
    // pairy's left phantom, which stays at (0, 0): its Y delta of 50 is
    // ignored, so the bars sit on tri's base line at every weight.
    (
        "anchcph",
        Side::Text(&[
            (
                "-",
                "0,0;400,0;200,300|400,0;500,0;500,40;400,40|600,0;700,0;700,40;600,40",
            ),
            (
                "100",
                "0,0;400,0;180,270|400,0;500,0;500,40;400,40|600,0;700,0;700,40;600,40",
            ),
            (
                "650",
                "0,0;400,0;220,330|405,5;500,0;500,40;400,40|605,5;700,0;700,40;600,40",
            ),
            (
                "900",
                "0,0;400,0;240,360|410,10;500,0;500,40;400,40|610,10;700,0;700,40;600,40",
            ),
        ]),
    ),
    // p4 of a parent with three points is its advance phantom, (600, 0)
    // moved by its X delta (30 at 900) and not its Y delta (50), read
    // before dot's metrics replace the parent's under USE_MY_METRICS.
    (
        "anchpadv",
        Side::Text(&[
            ("-", "0,0;400,0;200,300|600,0;660,0;660,60;600,60"),
            ("100", "0,0;400,0;180,270|600,0;660,0;660,60;600,60"),
            ("650", "0,0;400,0;220,330|615,0;675,0;675,60;615,60"),
            ("900", "0,0;400,0;240,360|630,0;690,0;690,60;630,60"),
        ]),
    ),
    // p5 is the parent's top phantom: vertical, and baud reads no `vmtx`.
    ("anchptop", Side::Unmatched(&["tri", "bar"])),
    // p3 is the parent's left phantom, moved by its X delta (40 at 900) and
    // not its Y (50); the glyph then shifts by that movement.
    (
        "anchplx",
        Side::Text(&[
            ("-", "0,0;400,0;200,300|-100,0;0,0;0,40;-100,40"),
            ("100", "0,0;400,0;180,270|-100,0;0,0;0,40;-100,40"),
            ("650", "-20,0;380,0;200,330|-95,5;0,0;0,40;-100,40"),
            ("900", "-40,0;360,0;200,360|-90,10;0,0;0,40;-100,40"),
        ]),
    ),
];

type Contours = Vec<Vec<(f32, f32)>>;

/// Each contour's points, the closing point dropped where it repeats the
/// first: the fixture's contours are polygons that start on the curve.
#[derive(Default)]
struct Points(Contours);

impl Sink for Points {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.push(vec![(x, y)]);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.last_mut().unwrap().push((x, y));
    }
    fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) {
        panic!("the fixture has no curves");
    }
    fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {
        panic!("the fixture has no curves");
    }
    fn close(&mut self) {
        let c = self.0.last_mut().unwrap();
        if c.len() > 1 && c.last() == c.first() {
            c.pop();
        }
    }
}

fn parse(row: &str) -> Option<Contours> {
    if row.starts_with("ERROR") {
        return None;
    }
    if row == "-" {
        return Some(Vec::new());
    }
    Some(
        row.split('|')
            .map(|c| {
                c.split(';')
                    .map(|p| {
                        let (x, y) = p.split_once(',').unwrap();
                        (x.parse().unwrap(), y.parse().unwrap())
                    })
                    .collect()
            })
            .collect(),
    )
}

fn near(a: &Contours, b: &Contours, tolerance: f32) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(c, d)| {
            c.len() == d.len()
                && c.iter()
                    .zip(d)
                    .all(|(p, q)| (p.0 - q.0).abs() <= tolerance && (p.1 - q.1).abs() <= tolerance)
        })
}

fn at_weight(face: &mut Face<'_>, w: &str) {
    if w != "-" {
        face.set_variations(&[Variation {
            tag: Tag::from_bytes(*b"wght"),
            value: Fixed(w.parse::<i32>().unwrap() << 16),
        }]);
    }
}

/// `g` drawn the way a renderer draws it: `outline_at` with what scratch
/// there is, and again with exactly the need a refusal names.
fn drawn(face: &Face<'_>, g: GlyphId) -> (Drawn, Contours) {
    let mut scratch: Vec<VarPoint> = Vec::new();
    let mut p = Points::default();
    let d = face.outline_at(g, &mut scratch, &mut p);
    let Drawn::NeedsScratch(need) = d else {
        return (d, p.0);
    };
    assert!(p.0.is_empty(), "a refusal drew nothing");
    scratch.resize(usize::try_from(need.value()).unwrap(), VarPoint::EMPTY);
    let mut p = Points::default();
    let d = face.outline_at(g, &mut scratch, &mut p);
    assert!(!matches!(d, Drawn::NeedsScratch(_)), "the need was exact");
    (d, p.0)
}

/// **Every glyph is drawn as the text places it**, at the default instance
/// and at wght 100, 650 and 900: point-matched children aligned after
/// their matrix (the apex of `tri` carries `bar`, rotated, scaled, or a
/// composite child's sixth point), a child's horizontal phantoms reachable,
/// a nested composite's parent points counted from its own first point,
/// the deltas of a matched slot ignored and an offset's applied before it
/// is scaled, and a SCALED_COMPONENT_OFFSET offset taken through the
/// matrix. The comparison is to HarfBuzz's coordinates within 1e-3 and to
/// FreeType's within half a unit, which it rounds a varied point to.
#[test]
fn every_component_case_is_placed_as_the_text_places_it() {
    let data = fixture();
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/var/components.txt"
    ))
    .unwrap();
    // (name, weight, engine) -> contours, or None where FreeType refused.
    let mut oracle: HashMap<(String, String, String), Option<Contours>> = HashMap::new();
    let mut ids = HashMap::new();
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let w: Vec<&str> = line.splitn(5, ' ').collect();
        ids.insert(w[1].to_string(), w[0].parse::<u16>().unwrap());
        oracle.insert((w[1].into(), w[2].into(), w[3].into()), parse(w[4]));
    }
    let sides: HashMap<&str, Side> = SIDES.iter().copied().collect();
    assert_eq!(sides.len(), ids.len(), "every glyph has a side");
    let mut wrong = Vec::new();
    let mut counted: HashMap<String, usize> = HashMap::new();
    let plain = Face::parse(&data).unwrap();
    for weight in ["-", "100", "650", "900"] {
        let mut face = Face::parse(&data).unwrap();
        at_weight(&mut face, weight);
        for (name, side) in SIDES {
            let g = GlyphId::from_shaper(ids[*name]);
            let key = |e: &str| (name.to_string(), weight.to_string(), e.to_string());
            let hb = oracle[&key("hb")].clone().unwrap();
            let ft = oracle[&key("ft")].clone();
            let (d, got) = drawn(&face, g);
            let want: Vec<(Contours, f32)> = match side {
                Side::Default => {
                    let mut p = Points::default();
                    assert_eq!(plain.outline(g, &mut p), Drawn::Outline, "{name}");
                    vec![(p.0, 1e-3)]
                }
                Side::Both => vec![(hb, 1e-3), (ft.expect("FreeType draws it"), 0.5)],
                Side::Hb => vec![(hb, 1e-3)],
                Side::Ft => vec![(ft.expect("FreeType draws it"), 0.5)],
                Side::Text(rows) => vec![(
                    rows.iter()
                        .find(|(w, _)| *w == weight)
                        .and_then(|(_, row)| parse(row))
                        .expect("a drawing at each weight"),
                    1e-3,
                )],
                Side::Unmatched(parts) => vec![(
                    parts
                        .iter()
                        .flat_map(|p| {
                            oracle[&(p.to_string(), weight.to_string(), "hb".to_string())]
                                .clone()
                                .unwrap()
                        })
                        .collect(),
                    1e-3,
                )],
            };
            assert_eq!(d, Drawn::Outline, "{name} at {weight}");
            for (w, tol) in &want {
                if !near(&got, w, *tol) {
                    wrong.push(format!("{name} at {weight}:\n   got {got:?}\n  want {w:?}"));
                }
            }
            let kind = format!("{side:?}");
            let kind = kind.split('(').next().unwrap_or_default().to_string();
            *counted.entry(kind).or_default() += 1;
        }
    }
    eprintln!("{counted:?}");
    assert_eq!(
        counted.values().sum::<usize>(),
        4 * SIDES.len(),
        "every glyph at four weights"
    );
    assert!(
        wrong.is_empty(),
        "{} differ:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

/// **The hand-worked drawings are HarfBuzz's where it does not depart**:
/// `anchself` at the default and at wght 100, where bar's first point,
/// which HarfBuzz lays on bar's second, has not moved; `anchcph` there
/// too, where `pairy`'s left phantom has no Y delta to ignore. A wrong
/// hand derivation fails here, without baud's drawing in the comparison.
#[test]
fn the_texts_drawings_are_harfbuzzs_where_harfbuzz_does_not_depart() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/var/components.txt"
    ))
    .unwrap();
    let mut checked = 0;
    for (name, weight) in [
        ("anchself", "-"),
        ("anchself", "100"),
        ("anchcph", "-"),
        ("anchcph", "100"),
        ("anchplx", "-"),
        ("anchplx", "100"),
    ] {
        let hb = text
            .lines()
            .filter(|l| !l.starts_with('#'))
            .find_map(|l| {
                let w: Vec<&str> = l.splitn(5, ' ').collect();
                (w[1] == name && w[2] == weight && w[3] == "hb").then(|| parse(w[4]).unwrap())
            })
            .unwrap();
        let Some((_, Side::Text(rows))) = SIDES.iter().find(|(n, _)| *n == name) else {
            panic!("{name} is held to the text");
        };
        let mine = rows
            .iter()
            .find(|(w, _)| *w == weight)
            .and_then(|(_, r)| parse(r))
            .unwrap();
        assert!(
            near(&mine, &hb, 1e-3),
            "{name} at {weight}: {mine:?} against {hb:?}"
        );
        checked += 1;
    }
    assert_eq!(checked, 6);
}

/// The component flag words as the file stores them, read here with no
/// code of baud's: fontTools rewrites flags when it compiles, so the
/// fixture is held to what it says it builds.
fn flags_of(data: &[u8], g: GlyphId) -> Vec<u16> {
    let face = Face::parse(data).unwrap();
    let head = face.table(Tag::HEAD).unwrap().as_slice();
    let long = u16::from_be_bytes([head[50], head[51]]) == 1;
    let loca = face.table(Tag::LOCA).unwrap().as_slice();
    let glyf = face.table(Tag::GLYF).unwrap().as_slice();
    let at = |i: usize| -> usize {
        if long {
            usize::try_from(u32::from_be_bytes(
                loca[4 * i..4 * i + 4].try_into().unwrap(),
            ))
            .unwrap()
        } else {
            2 * usize::from(u16::from_be_bytes([loca[2 * i], loca[2 * i + 1]]))
        }
    };
    let body = &glyf[at(g.index())..at(g.index() + 1)];
    assert!(i16::from_be_bytes([body[0], body[1]]) < 0, "a composite");
    let mut out = Vec::new();
    let mut p = 10;
    loop {
        let f = u16::from_be_bytes([body[p], body[p + 1]]);
        out.push(f);
        p += 4 + if f & 1 != 0 { 4 } else { 2 };
        p += match () {
            _ if f & 0x08 != 0 => 2,
            _ if f & 0x40 != 0 => 4,
            _ if f & 0x80 != 0 => 8,
            _ => 0,
        };
        if f & 0x20 == 0 {
            return out;
        }
    }
}

/// **The fixture carries the flags it was built to**: every `anch*` glyph's
/// second component (`anchfirst`'s first) has ARGS_ARE_XY_VALUES clear;
/// the scaled cases carry bit 11 alone, `unscaled` bit 12, `both` both,
/// `neither` neither; and every other component is an offset.
#[test]
fn the_fixture_carries_the_flags_it_was_built_to() {
    const XY: u16 = 0x0002;
    const SCALED: u16 = 0x0800;
    const UNSCALED: u16 = 0x1000;
    let data = fixture();
    let face = Face::parse(&data).unwrap();
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/var/components.txt"
    ))
    .unwrap();
    let id = |name: &str| -> GlyphId {
        GlyphId::from_shaper(
            text.lines()
                .find_map(|l| {
                    let w: Vec<&str> = l.split(' ').collect();
                    (w.get(1) == Some(&name)).then(|| w[0].parse().unwrap())
                })
                .unwrap(),
        )
    };
    assert_eq!(face.glyph_count().value(), 34);
    let mut anchored = 0;
    for name in [
        "anch",
        "anchrot",
        "anchscale",
        "anchadv",
        "anchlsb",
        "anchtop",
        "anchcomp",
        "anchflag",
        "anchvar",
        "anchout",
        "anchself",
        "anchcph",
        "anchpadv",
        "anchptop",
        "anchplx",
    ] {
        let f = flags_of(&data, id(name));
        assert_eq!(f[0] & XY, XY, "{name}: tri by offset");
        assert_eq!(f[1] & XY, 0, "{name}: matched points");
        anchored += 1;
    }
    let f = flags_of(&data, id("anchfirst"));
    assert_eq!((f[0] & XY, f[1] & XY), (0, XY), "anchfirst");
    anchored += 1;
    assert_eq!(anchored, 16);
    assert_eq!(flags_of(&data, id("anchflag"))[1] & SCALED, SCALED);
    for name in ["scaled", "scaledrot", "scaledxy", "scaledneg", "scaledvar"] {
        let f = flags_of(&data, id(name))[1];
        assert_eq!(f & (XY | SCALED | UNSCALED), XY | SCALED, "{name}");
    }
    let bits = |name| flags_of(&data, id(name))[1] & (SCALED | UNSCALED);
    assert_eq!(bits("unscaled"), UNSCALED);
    assert_eq!(bits("both"), SCALED | UNSCALED);
    assert_eq!(bits("neither"), 0);
}

/// **The default instance streams what it can and refuses what it
/// cannot**: a glyph whose tree matches points answers `NeedsScratch` from
/// `Face::outline`, with nothing drawn, and `Face::outline_need` names the
/// scratch `outline_at` then draws it with; one that only scales offsets
/// streams, and needs none.
#[test]
fn a_matched_tree_needs_scratch_at_the_default_and_an_offset_one_does_not() {
    let data = fixture();
    let face = Face::parse(&data).unwrap();
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/var/components.txt"
    ))
    .unwrap();
    let id = |name: &str| -> GlyphId {
        GlyphId::from_shaper(
            text.lines()
                .find_map(|l| {
                    let w: Vec<&str> = l.split(' ').collect();
                    (w.get(1) == Some(&name)).then(|| w[0].parse().unwrap())
                })
                .unwrap(),
        )
    };
    for name in ["anch", "anchcomp", "nestfirst", "nestlate", "anchout"] {
        let mut p = Points::default();
        let need = face.outline_need(id(name));
        assert!(need.value() > 0, "{name}");
        assert_eq!(
            face.outline(id(name), &mut p),
            Drawn::NeedsScratch(need),
            "{name}"
        );
        assert!(p.0.is_empty(), "{name}: nothing drawn");
    }
    for name in ["scaled", "scaledrot", "both", "pair", "tri"] {
        let mut p = Points::default();
        assert_eq!(face.outline_need(id(name)).value(), 0, "{name}");
        assert_eq!(face.outline(id(name), &mut p), Drawn::Outline, "{name}");
    }
}
