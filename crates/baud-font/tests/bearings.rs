//! A TrueType glyph is drawn from its left side bearing point, which sits
//! at `xMin - lsb`, and not from x = 0.
//!
//! `[[source: ms-opentype-hmtx]]`: "If pp1 and pp2 are TrueType phantom
//! points used to control lsb and rsb, their initial position in the
//! X-direction is calculated as follows: pp1 = xMin - lsb";
//! `[[source: ms-opentype-tt-instructing-glyphs]]`: point n "will be placed
//! at the glyph left sidebearing point"; `[[source: apple-ttrm-ch2]]`: that
//! point is the origin, and "The origin point is the original pen position.
//! Imagine that the pen moves the left-side bearing distance, draws the
//! glyph". `[[source: ms-opentype-head]]`'s flags bit 1 is "Left sidebearing
//! point at x=0", and "If bit 1 of the flags field is set, then pp1 = 0 for
//! all glyphs and the xMin and left sidebearing for each glyph must be
//! equal": a font that sets it and breaks the equality is malformed, and it
//! is drawn from `xMin - lsb` as with the bit clear.
//!
//! `tests/var/bearing-cases.ttf` and `bearing-cases-bit1.ttf`
//! (`tools/baud-oracle cases bearing`) are the fixtures, the same glyphs with
//! bit 1 clear and set, and `tests/var/bearings.txt`
//! (`tools/baud-oracle`'s `bearing`) holds HarfBuzz 12.3.0's and FreeType
//! 2.14.3's drawings of both at the default instance and three weights,
//! with where each glyph's left phantom starts, read by read-fonts.

use baud_domain::GlyphId;
use baud_font::var::{Fixed, Variation};
use baud_font::{Drawn, Face, Sink, Tag, VarPoint};
use std::collections::HashMap;

const CLEAR: &str = "bearing-cases.ttf";
const SET: &str = "bearing-cases-bit1.ttf";
const WEIGHTS: [&str; 4] = ["-", "100", "650", "900"];
/// The glyphs the matched walk draws at the default instance: the stream
/// refuses them, so the rest are the stream's.
const MATCHED: [&str; 2] = ["anch", "anchlsb"];
/// The fixtures' simple glyphs (`tools/baud-oracle cases bearing`); the other
/// six are composites.
const SIMPLE: [&str; 4] = ["box", "neg", "same", "boxvar"];

fn read(name: &str) -> Vec<u8> {
    std::fs::read(format!("{}/tests/var/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

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

/// The oracle: (font, name, weight, engine) to contours, `None` where
/// FreeType refused; (font, name) to the left phantom's start; and each
/// glyph's id.
struct Oracle {
    drawings: HashMap<(String, String, String, String), Option<Contours>>,
    starts: HashMap<(String, String), f32>,
    ids: HashMap<String, u16>,
}

fn oracle() -> Oracle {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/var/bearings.txt"
    ))
    .unwrap();
    let mut o = Oracle {
        drawings: HashMap::new(),
        starts: HashMap::new(),
        ids: HashMap::new(),
    };
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let w: Vec<&str> = line.splitn(6, ' ').collect();
        o.ids.insert(w[2].to_string(), w[1].parse().unwrap());
        if w[3] == "start" {
            o.starts
                .insert((w[0].into(), w[2].into()), w[4].parse().unwrap());
        } else {
            o.drawings.insert(
                (w[0].into(), w[2].into(), w[3].into(), w[4].into()),
                parse(w[5]),
            );
        }
    }
    o
}

fn at_weight(face: &mut Face<'_>, w: &str) {
    if w != "-" {
        face.set_variations(&[Variation {
            tag: Tag::from_bytes(*b"wght"),
            value: Fixed(w.parse::<i32>().unwrap() << 16),
        }]);
    }
}

/// `g` drawn as a renderer draws it: `outline_at` with exactly the need a
/// refusal names.
fn drawn(face: &Face<'_>, g: GlyphId) -> Contours {
    let mut scratch: Vec<VarPoint> = Vec::new();
    let mut p = Points::default();
    if let Drawn::NeedsScratch(need) = face.outline_at(g, &mut scratch, &mut p) {
        scratch.resize(usize::try_from(need.value()).unwrap(), VarPoint::EMPTY);
        p = Points::default();
        assert_eq!(face.outline_at(g, &mut scratch, &mut p), Drawn::Outline);
    }
    p.0
}

/// `head` flags, read from the file with no code of baud's.
fn head_flags(data: &[u8]) -> u16 {
    let tables = usize::from(u16::from_be_bytes([data[4], data[5]]));
    (0..tables)
        .map(|i| 12 + 16 * i)
        .find(|&r| &data[r..r + 4] == b"head")
        .map(|r| {
            let at = usize::try_from(u32::from_be_bytes([
                data[r + 8],
                data[r + 9],
                data[r + 10],
                data[r + 11],
            ]))
            .unwrap();
            u16::from_be_bytes([data[at + 16], data[at + 17]])
        })
        .unwrap()
}

/// **The fixtures are what the tests below need**: bit 1 clear in one and
/// set in the other, and eight of the ten glyphs whose left phantom does
/// not start at 0 -- every case the script names, with fontTools' reading
/// of where it starts -- each drawn by HarfBuzz away from its stored
/// coordinates, which is the shift the engines make.
#[test]
fn the_fixtures_reach_every_case() {
    assert_eq!(head_flags(&read(CLEAR)) & 2, 0);
    assert_eq!(head_flags(&read(SET)) & 2, 2);
    let o = oracle();
    let want = [
        ("box", 20.0),
        ("neg", -30.0),
        ("same", 0.0),
        ("boxvar", 20.0),
        ("comp", 15.0),
        ("umm", -30.0),
        ("ummlast", 20.0),
        ("ummnest", -30.0),
        ("anch", 25.0),
        ("anchlsb", 0.0),
    ];
    assert_eq!(o.ids.len(), want.len());
    for font in [CLEAR, SET] {
        for (name, start) in want {
            assert_eq!(
                o.starts[&(font.into(), name.into())],
                start,
                "{font} {name}"
            );
        }
    }
    // box's stored points run from x 20; HarfBuzz draws them from 0.
    let hb = o.drawings[&(CLEAR.into(), "box".into(), "-".into(), "hb".into())]
        .clone()
        .unwrap();
    assert_eq!(
        hb,
        vec![vec![(0.0, 0.0), (60.0, 0.0), (60.0, 60.0), (0.0, 60.0)]]
    );
}

/// **With bit 1 clear, every glyph is drawn from its left side bearing
/// point**, at the default instance and at wght 100, 650 and 900: a simple
/// glyph whose lsb is not its xMin, on either side of it; one whose left
/// phantom moves with the weight, by its start plus the delta; a
/// composite's own start and own delta; a USE_MY_METRICS component's start,
/// the last one's, and a nested one's; and the matched walk, which the
/// default instance takes for a glyph that places a component by points.
/// To HarfBuzz's coordinates within 1e-3 and FreeType's within half a unit,
/// which it rounds a varied point to; FreeType refuses a phantom point in a
/// match (`anchlsb`), and HarfBuzz is the drawing there. At the default
/// instance the stream draws every glyph but the matched ones, so both
/// paths are held.
#[test]
fn with_bit_1_clear_a_glyph_is_drawn_from_its_left_side_bearing_point() {
    let data = read(CLEAR);
    let o = oracle();
    let mut wrong = Vec::new();
    let mut compared = 0;
    for weight in WEIGHTS {
        let mut face = Face::parse(&data).unwrap();
        at_weight(&mut face, weight);
        for (name, &id) in &o.ids {
            let g = GlyphId::from_shaper(id);
            let key = |e: &str| {
                (
                    CLEAR.to_string(),
                    name.clone(),
                    weight.to_string(),
                    e.to_string(),
                )
            };
            let got = drawn(&face, g);
            let mut want = vec![(o.drawings[&key("hb")].clone().unwrap(), 1e-3)];
            if let Some(ft) = o.drawings[&key("ft")].clone() {
                want.push((ft, 0.5));
            } else {
                assert_eq!(name, "anchlsb", "FreeType refuses only the phantom match");
            }
            if weight == "-" {
                let mut p = Points::default();
                let d = face.outline(g, &mut p);
                if MATCHED.contains(&name.as_str()) {
                    assert!(matches!(d, Drawn::NeedsScratch(_)), "{name}");
                } else {
                    assert_eq!(d, Drawn::Outline, "{name}");
                    assert_eq!(p.0, got, "{name}: the stream draws what outline_at draws");
                }
            }
            for (w, tol) in &want {
                compared += 1;
                if !near(&got, w, *tol) {
                    wrong.push(format!("{name} at {weight}:\n   got {got:?}\n  want {w:?}"));
                }
            }
        }
    }
    assert_eq!(compared, 4 * (10 + 9));
    assert!(
        wrong.is_empty(),
        "{} wrong:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

/// **With bit 1 set, every glyph is still drawn from its left side bearing
/// point**, as with the bit clear. The `head` text puts the point at x = 0
/// for such a font and requires the lsb to be xMin, and `hmtx`'s formula
/// puts it at `xMin - lsb`; this font breaks the requirement, and the
/// formula is taken, which is HarfBuzz's and FreeType's drawing, bit 1
/// regardless: a simple glyph from its own start, a composite from its own
/// or its last USE_MY_METRICS component's. To HarfBuzz within 1e-3 and
/// FreeType within half a unit, at the default instance and three weights.
/// Three simple glyphs and five composites start off 0, so a drawing from
/// 0, or from the stored coordinates, differs; at the default instance the
/// stream draws what `outline_at` draws.
#[test]
fn with_bit_1_set_a_glyph_is_still_drawn_from_its_left_side_bearing_point() {
    let data = read(SET);
    let o = oracle();
    let mut wrong = Vec::new();
    let mut compared = 0;
    let (mut simple_off, mut composite_off) = (0, 0);
    for weight in WEIGHTS {
        let mut face = Face::parse(&data).unwrap();
        at_weight(&mut face, weight);
        for (name, &id) in &o.ids {
            let g = GlyphId::from_shaper(id);
            let key = |e: &str| {
                (
                    SET.to_string(),
                    name.clone(),
                    weight.to_string(),
                    e.to_string(),
                )
            };
            let start = o.starts[&(SET.into(), name.clone())];
            let simple = SIMPLE.contains(&name.as_str());
            simple_off += usize::from(simple && start != 0.0);
            composite_off += usize::from(!simple && start != 0.0);
            let got = drawn(&face, g);
            let mut want = vec![(o.drawings[&key("hb")].clone().unwrap(), 1e-3)];
            if let Some(ft) = o.drawings[&key("ft")].clone() {
                want.push((ft, 0.5));
            } else {
                assert_eq!(name, "anchlsb", "FreeType refuses only the phantom match");
            }
            for (w, tol) in &want {
                compared += 1;
                if !near(&got, w, *tol) {
                    wrong.push(format!("{name} at {weight}:\n   got {got:?}\n  want {w:?}"));
                }
            }
            if weight == "-" && !MATCHED.contains(&name.as_str()) {
                let mut p = Points::default();
                assert_eq!(face.outline(g, &mut p), Drawn::Outline, "{name}");
                assert_eq!(p.0, got, "{name}: the stream draws what outline_at draws");
            }
        }
    }
    assert_eq!(compared, 4 * (10 + 9));
    assert!(
        wrong.is_empty(),
        "{} wrong:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
    assert_eq!((simple_off, composite_off), (4 * 3, 4 * 5));
}
