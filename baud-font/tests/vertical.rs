//! Components placed on vertical phantom points, in a face with `vmtx`,
//! and a nested composite whose first component is placed by points.
//!
//! `[[source: ms-opentype-glyf]]`: "Phantom points from the parent or the
//! child may be referenced"; `[[source: ms-opentype-tt-instructing-glyphs]]`
//! numbers them -- "point “n+2” will be placed at the top origin; and point
//! “n+3” will be placed at the advance height point" -- and
//! `[[source: ms-opentype-vmtx]]` places those: "The y coordinate of a
//! glyph’s vertical origin is specified as the sum of the glyph’s top side
//! bearing (recorded in the 'vmtx' table) and the top (i.e. maximum y) of
//! the glyph’s bounding box", and "The advance height of a glyph starts from
//! the y coordinate of the glyph’s vertical origin and advances downwards".
//! Read so. The x of both, which "is not specified", is HarfBuzz's and
//! FreeType's 0.
//!
//! `tests/var/vertical-cases.ttf` (`scripts/vertical-cases-font.py`) is the
//! fixture and `tests/var/vertical.txt` (`tools/baud-oracle`'s `vertical`)
//! holds HarfBuzz 12.3.0's and FreeType 2.14.3's drawings of it. FreeType
//! refuses every phantom point in a match; HarfBuzz places a child's
//! vertical phantoms as the text does, applies the X delta of a composite's
//! own top phantom, which `[[source: ms-opentype-gvar]]` ignores ("For AH
//! and TSB points, apply the net Y delta adjustments, and ignore X
//! deltas"), and reads a parent point past the earlier components' as the
//! child's own. Where it departs, the text's drawing is worked by hand from
//! the script.

use baud_domain::GlyphId;
use baud_font::var::{Fixed, Variation};
use baud_font::{Drawn, Face, Sink, Tag, VarPoint};
use std::collections::HashMap;

fn fixture() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/var/vertical-cases.ttf"
    ))
    .unwrap()
}

type Contours = Vec<Vec<(f32, f32)>>;

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

/// (name, weight, engine) to contours, `None` where FreeType refused; and
/// each glyph's id.
type Oracle = (
    HashMap<(String, String, String), Option<Contours>>,
    HashMap<String, GlyphId>,
);

fn oracle() -> Oracle {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/var/vertical.txt"
    ))
    .unwrap();
    let mut rows = HashMap::new();
    let mut ids = HashMap::new();
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let w: Vec<&str> = line.splitn(5, ' ').collect();
        ids.insert(
            w[1].to_string(),
            GlyphId::from_shaper(w[0].parse().unwrap()),
        );
        rows.insert((w[1].into(), w[2].into(), w[3].into()), parse(w[4]));
    }
    (rows, ids)
}

fn at_weight(face: &mut Face<'_>, w: &str) {
    if w != "-" {
        face.set_variations(&[Variation {
            tag: Tag::from_bytes(*b"wght"),
            value: Fixed(w.parse::<i32>().unwrap() << 16),
        }]);
    }
}

/// `g` drawn the way a renderer draws it: `outline_at` with exactly the
/// need a refusal names.
fn drawn(face: &Face<'_>, g: GlyphId) -> (Drawn, Contours) {
    let mut scratch: Vec<VarPoint> = Vec::new();
    let mut p = Points::default();
    let d = face.outline_at(g, &mut scratch, &mut p);
    let Drawn::NeedsScratch(need) = d else {
        return (d, p.0);
    };
    scratch.resize(usize::try_from(need.value()).unwrap(), VarPoint::EMPTY);
    let mut p = Points::default();
    let d = face.outline_at(g, &mut scratch, &mut p);
    (d, p.0)
}

/// Which drawing a glyph's must equal.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Side {
    /// HarfBuzz's, and FreeType's where it draws the glyph.
    Hb,
    /// The text's, worked by hand from the script, at each weight.
    Text(&'static [(&'static str, &'static str)]),
    /// `Drawn::Refused`, nothing drawn.
    Refused,
}

const TRI: [&str; 4] = [
    "0,0;400,0;200,300",
    "0,0;400,0;200,300",
    "0,0;400,0;220,330",
    "0,0;400,0;240,360",
];

const SIDES: &[(&str, Side)] = &[
    ("tri", Side::Hb),
    ("bar", Side::Hb),
    ("dot", Side::Hb),
    ("short", Side::Hb),
    ("pairv", Side::Hb),
    // dot's top phantom, (0, 60 + 140), moved (30, 50) at 900: a simple
    // glyph's phantom takes both deltas.
    ("vtop", Side::Hb),
    // dot's advance height phantom, (0, 200 - 900).
    ("vbot", Side::Hb),
    // short's top phantom, (0, 60 + 120): its tsb from vmtx's trailing
    // array.
    ("vshort", Side::Hb),
    // pairv's top phantom, (0, 40 + 30), moved by the Y of its delta
    // (50 at 900) and not the X (40), laid on tri's (400, 0).
    (
        "vcomp",
        Side::Text(&[
            (
                "-",
                "|400,-70;500,-70;500,-30;400,-30|600,-70;700,-70;700,-30;600,-30",
            ),
            (
                "100",
                "|400,-70;500,-70;500,-30;400,-30|600,-70;700,-70;700,-30;600,-30",
            ),
            (
                "650",
                "|400,-95;500,-95;500,-55;400,-55|600,-95;700,-95;700,-55;600,-55",
            ),
            (
                "900",
                "|400,-120;500,-120;500,-80;400,-80|600,-120;700,-120;700,-80;600,-80",
            ),
        ]),
    ),
    // p5 of a parent with three points is its top phantom, (0, 300 + 50),
    // moved by the Y of its delta (50 at 900) and not the X (40); bar's
    // second point, (100, 0), is laid on it.
    (
        "vptop",
        Side::Text(&[
            ("-", "|-100,350;0,350;0,390;-100,390"),
            ("100", "|-100,350;0,350;0,390;-100,390"),
            ("650", "|-100,375;0,375;0,415;-100,415"),
            ("900", "|-100,400;0,400;0,440;-100,440"),
        ]),
    ),
    // p6 is its advance height phantom, (0, 350 - 1000).
    (
        "vpbot",
        Side::Text(&[
            ("-", "|-100,-650;0,-650;0,-610;-100,-610"),
            ("100", "|-100,-650;0,-650;0,-610;-100,-610"),
            ("650", "|-100,-650;0,-650;0,-610;-100,-610"),
            ("900", "|-100,-650;0,-650;0,-610;-100,-610"),
        ]),
    ),
    // p7 is past the parent's four phantoms, which no text names: bar
    // stays where its matrix put it (kept at offset 0 since
    // 2026-10-13).
    (
        "vpast",
        Side::Text(&[
            ("-", "|0,0;100,0;100,40;0,40"),
            ("100", "|0,0;100,0;100,40;0,40"),
            ("650", "|0,0;100,0;100,40;0,40"),
            ("900", "|0,0;100,0;100,40;0,40"),
        ]),
    ),
    ("firstm", Side::Refused),
    ("vnest", Side::Refused),
];

const WEIGHTS: [&str; 4] = ["-", "100", "650", "900"];

/// **Every glyph is drawn as the text places it**, at the default instance
/// and at wght 100, 650 and 900: a child's top and advance height phantoms
/// at `yMax + tsb` and an advance height below, a composite child's own
/// top phantom moved by its Y delta alone, a tsb read from `vmtx`'s
/// trailing array, the parent's own vertical phantoms numbered after the
/// points so far and its left and advance phantoms, a number past them at
/// offset 0, and a first component placed by points refused at the top
/// level and one level down. To HarfBuzz within 1e-3 where it does not
/// depart and to the hand-worked drawing within 1e-3 where it does.
#[test]
fn every_vertical_case_is_placed_as_the_text_places_it() {
    let data = fixture();
    let (rows, ids) = oracle();
    assert_eq!(ids.len(), SIDES.len(), "every glyph has a side");
    let mut wrong = Vec::new();
    let mut counted: HashMap<&str, usize> = HashMap::new();
    for (k, weight) in WEIGHTS.iter().enumerate() {
        let mut face = Face::parse(&data).unwrap();
        at_weight(&mut face, weight);
        for (name, side) in SIDES {
            let g = ids[*name];
            let key = |e: &str| (name.to_string(), weight.to_string(), e.to_string());
            let (d, got) = drawn(&face, g);
            let want = match side {
                Side::Refused => {
                    assert_eq!((d, got.len()), (Drawn::Refused, 0), "{name} at {weight}");
                    assert_eq!(rows[&key("ft")], None, "{name}: FreeType refuses it too");
                    *counted.entry("refused").or_default() += 1;
                    continue;
                }
                Side::Hb => rows[&key("hb")].clone().unwrap(),
                Side::Text(drawings) => {
                    let row = drawings.iter().find(|(w, _)| w == weight).unwrap().1;
                    parse(&format!("{}{row}", TRI[k])).unwrap()
                }
            };
            assert_eq!(d, Drawn::Outline, "{name} at {weight}");
            *counted
                .entry(if *side == Side::Hb { "hb" } else { "text" })
                .or_default() += 1;
            if !near(&got, &want, 1e-3) {
                wrong.push(format!(
                    "{name} at {weight}:\n   got {got:?}\n  want {want:?}"
                ));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "{} differ:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
    assert_eq!(counted["hb"], 4 * 8);
    assert_eq!(counted["text"], 4 * 4);
    assert_eq!(counted["refused"], 4 * 2);
}

/// **`Face::refuses` answers what drawing answers**, glyph for glyph and at
/// every weight, so a renderer can pass a refused glyph over before it
/// draws it: true for exactly `firstm` and `vnest` here, and for exactly
/// `anchfirst` in `component-cases.ttf`, whose drawings are
/// `Drawn::Refused`, and false for every glyph that draws. The answer reads
/// the components' flags alone, so it is the same at every instance.
#[test]
fn refuses_answers_what_drawing_answers() {
    let (_, ids) = oracle();
    let components = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/var/component-cases.ttf"
    ))
    .unwrap();
    let cases = [
        (fixture(), vec![ids["firstm"], ids["vnest"]]),
        (components, vec![GlyphId::from_shaper(19)]),
    ];
    for (data, want) in cases {
        for weight in WEIGHTS {
            let mut face = Face::parse(&data).unwrap();
            at_weight(&mut face, weight);
            let mut refused = Vec::new();
            for i in 0..face.glyph_count().value() {
                let g = GlyphId::from_shaper(i);
                let (d, _) = drawn(&face, g);
                assert_eq!(face.refuses(g), d == Drawn::Refused, "{g:?} at {weight}");
                if face.refuses(g) {
                    refused.push(g);
                }
            }
            assert_eq!(refused, want, "at {weight}");
        }
    }
}

/// **The hand-worked drawings are HarfBuzz's where it does not depart**:
/// `vcomp` at the default and wght 100, where `pairv`'s top phantom has no
/// delta to take the X of. A wrong hand derivation fails here, without
/// baud's drawing in the comparison.
#[test]
fn the_texts_drawings_are_harfbuzzs_where_harfbuzz_does_not_depart() {
    let (rows, _) = oracle();
    for (k, weight) in [(0, "-"), (1, "100")] {
        let hb = rows[&("vcomp".into(), weight.into(), "hb".into())]
            .clone()
            .unwrap();
        let Some((_, Side::Text(drawings))) = SIDES.iter().find(|(n, _)| *n == "vcomp") else {
            panic!("vcomp is held to the text");
        };
        let row = drawings.iter().find(|(w, _)| *w == weight).unwrap().1;
        let mine = parse(&format!("{}{row}", TRI[k])).unwrap();
        assert!(
            near(&mine, &hb, 1e-3),
            "vcomp at {weight}: {mine:?} against {hb:?}"
        );
    }
}

/// `vhea`'s numOfLongVerMetrics and a glyph's `vmtx` entry, read from the
/// file with no code of baud's.
fn vertical_metrics(data: &[u8]) -> (u16, Vec<(u16, i16)>) {
    let tables = usize::from(u16::from_be_bytes([data[4], data[5]]));
    let table = |tag: &[u8; 4]| -> &[u8] {
        let r = (0..tables)
            .map(|i| 12 + 16 * i)
            .find(|&r| &data[r..r + 4] == tag)
            .unwrap();
        let word = |at: usize| {
            usize::try_from(u32::from_be_bytes(data[at..at + 4].try_into().unwrap())).unwrap()
        };
        &data[word(r + 8)..word(r + 8) + word(r + 12)]
    };
    let vhea = table(b"vhea");
    let long = u16::from_be_bytes([vhea[34], vhea[35]]);
    let vmtx = table(b"vmtx");
    let mut out = Vec::new();
    let mut last = 0;
    for g in 0..15 {
        if g < usize::from(long) {
            last = u16::from_be_bytes([vmtx[4 * g], vmtx[4 * g + 1]]);
            out.push((last, i16::from_be_bytes([vmtx[4 * g + 2], vmtx[4 * g + 3]])));
        } else {
            let at = 4 * usize::from(long) + 2 * (g - usize::from(long));
            out.push((last, i16::from_be_bytes([vmtx[at], vmtx[at + 1]])));
        }
    }
    (long, out)
}

/// **The fixture is what the cases need**: six long `vmtx` records, so
/// `short` and every composite after `vtop` take their tsb from the
/// trailing array; the advance heights and top side bearings the script
/// writes; and the composites' header yMax of 300 (`pairv`'s 40), so no
/// vertical phantom sits on the origin or on a left phantom.
#[test]
fn the_fixture_carries_the_vertical_metrics_it_was_built_to() {
    let data = fixture();
    let (long, metrics) = vertical_metrics(&data);
    assert_eq!(long, 6);
    let (_, ids) = oracle();
    let want = [
        ("tri", (1000, 100)),
        ("bar", (800, 60)),
        ("dot", (900, 140)),
        ("pairv", (700, 30)),
        ("vtop", (1000, 50)),
        ("vptop", (1000, 50)),
        ("short", (1000, 120)),
    ];
    for (name, m) in want {
        assert_eq!(metrics[ids[name].index()], m, "{name}");
    }
    assert!(
        ids["short"].index() >= usize::from(long),
        "short is in the trailing array"
    );
    assert!(
        ids["vptop"].index() >= usize::from(long),
        "vptop is in the trailing array"
    );
    let face = Face::parse(&data).unwrap();
    let glyf = face.table(Tag::GLYF).unwrap().as_slice();
    let loca = face.table(Tag::LOCA).unwrap().as_slice();
    let long_loca = {
        let head = face.table(Tag::HEAD).unwrap().as_slice();
        u16::from_be_bytes([head[50], head[51]]) == 1
    };
    let at = |i: usize| -> usize {
        if long_loca {
            usize::try_from(u32::from_be_bytes(
                loca[4 * i..4 * i + 4].try_into().unwrap(),
            ))
            .unwrap()
        } else {
            2 * usize::from(u16::from_be_bytes([loca[2 * i], loca[2 * i + 1]]))
        }
    };
    for (name, y_max) in [("pairv", 40), ("vptop", 300), ("vpbot", 300)] {
        let body = &glyf[at(ids[name].index())..];
        assert_eq!(i16::from_be_bytes([body[8], body[9]]), y_max, "{name}");
    }
}

/// The fixture with its `vhea` and `vmtx` tags renamed in the table
/// directory: the same glyphs in a face with no vertical metrics.
fn without_vertical_metrics(data: &[u8]) -> Vec<u8> {
    let mut out = data.to_vec();
    let tables = usize::from(u16::from_be_bytes([data[4], data[5]]));
    let mut renamed = 0;
    for r in (0..tables).map(|i| 12 + 16 * i) {
        let to: &[u8; 4] = match &data[r..r + 4] {
            b"vhea" => b"xhea",
            b"vmtx" => b"xmtx",
            _ => continue,
        };
        out[r..r + 4].copy_from_slice(to);
        renamed += 1;
    }
    assert_eq!(renamed, 2);
    out
}

/// **Without `vmtx`, the vertical phantoms are HarfBuzz's**: a tsb of 0
/// and an advance height of unitsPerEm, so the top origin at `(0, yMax)`
/// and the advance height point 1,000 below it -- the text gives no rule
/// for a face without vertical metrics (`[[source: ms-opentype-vmtx]]`),
/// and baud takes HarfBuzz's. The child's phantoms against HarfBuzz's
/// drawing of the same renamed font (`hb-novmtx`); the parent's, where
/// HarfBuzz's numbering departs, worked by hand: `vptop`'s top origin at
/// (0, 300), `vpbot`'s advance height point at (0, -700).
#[test]
fn without_vertical_metrics_the_phantoms_are_harfbuzzs() {
    let data = without_vertical_metrics(&fixture());
    let face = Face::parse(&data).unwrap();
    let (rows, ids) = oracle();
    let mut checked = 0;
    for name in ["vtop", "vbot", "vcomp", "vshort"] {
        let want = rows[&(name.into(), "-".into(), "hb-novmtx".into())]
            .clone()
            .unwrap();
        let (d, got) = drawn(&face, ids[name]);
        assert_eq!(d, Drawn::Outline, "{name}");
        assert!(near(&got, &want, 1e-3), "{name}: {got:?} against {want:?}");
        checked += 1;
    }
    for (name, bar) in [
        ("vptop", "|-100,300;0,300;0,340;-100,340"),
        ("vpbot", "|-100,-700;0,-700;0,-660;-100,-660"),
    ] {
        let want = parse(&format!("{}{bar}", TRI[0])).unwrap();
        let (d, got) = drawn(&face, ids[name]);
        assert_eq!(d, Drawn::Outline, "{name}");
        assert!(near(&got, &want, 1e-3), "{name}: {got:?} against {want:?}");
        checked += 1;
    }
    assert_eq!(checked, 6);
}
