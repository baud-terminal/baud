//! The Hebrew presentation forms U+FB1D-FB4F in baud's embedded font.
//!
//! Each canonical form is its decomposition drawn as one glyph
//! (`[[source: unicode-data]]`'s decompositions; `[[source: unicode-ch9]]`
//! 9.1.2), so its outline must be exactly what HarfBuzz draws for the letter
//! and points typed apart: the shape fixtures, hebrew-faces.txt, hold hb-shape
//! 14.5.1's glyphs and offsets for each decomposition on each built face,
//! and each glyph's outline is read from the face, by baud's reader, at the
//! offset HarfBuzz gives it.

use baud_domain::{Codepoint, GlyphId};
use baud_font::{Face, Sink};
use std::collections::HashMap;

const STYLES: [&str; 4] = ["Regular", "Bold", "Italic", "BoldItalic"];

/// UnicodeData.txt 17.0's canonical decompositions of the forms, all the
/// way down: the letter, then its points in order.
const CANONICAL: [(u32, &[u32]); 34] = [
    (0xFB1D, &[0x05D9, 0x05B4]),
    (0xFB1F, &[0x05F2, 0x05B7]),
    (0xFB2A, &[0x05E9, 0x05C1]),
    (0xFB2B, &[0x05E9, 0x05C2]),
    (0xFB2C, &[0x05E9, 0x05BC, 0x05C1]),
    (0xFB2D, &[0x05E9, 0x05BC, 0x05C2]),
    (0xFB2E, &[0x05D0, 0x05B7]),
    (0xFB2F, &[0x05D0, 0x05B8]),
    (0xFB30, &[0x05D0, 0x05BC]),
    (0xFB31, &[0x05D1, 0x05BC]),
    (0xFB32, &[0x05D2, 0x05BC]),
    (0xFB33, &[0x05D3, 0x05BC]),
    (0xFB34, &[0x05D4, 0x05BC]),
    (0xFB35, &[0x05D5, 0x05BC]),
    (0xFB36, &[0x05D6, 0x05BC]),
    (0xFB38, &[0x05D8, 0x05BC]),
    (0xFB39, &[0x05D9, 0x05BC]),
    (0xFB3A, &[0x05DA, 0x05BC]),
    (0xFB3B, &[0x05DB, 0x05BC]),
    (0xFB3C, &[0x05DC, 0x05BC]),
    (0xFB3E, &[0x05DE, 0x05BC]),
    (0xFB40, &[0x05E0, 0x05BC]),
    (0xFB41, &[0x05E1, 0x05BC]),
    (0xFB43, &[0x05E3, 0x05BC]),
    (0xFB44, &[0x05E4, 0x05BC]),
    (0xFB46, &[0x05E6, 0x05BC]),
    (0xFB47, &[0x05E7, 0x05BC]),
    (0xFB48, &[0x05E8, 0x05BC]),
    (0xFB49, &[0x05E9, 0x05BC]),
    (0xFB4A, &[0x05EA, 0x05BC]),
    (0xFB4B, &[0x05D5, 0x05B9]),
    (0xFB4C, &[0x05D1, 0x05BF]),
    (0xFB4D, &[0x05DB, 0x05BF]),
    (0xFB4E, &[0x05E4, 0x05BF]),
];

fn built(style: &str) -> Vec<u8> {
    let path = format!(
        "{}/../baud-app/fonts/built/{style}.ttf",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn cp(v: u32) -> Codepoint {
    Codepoint::new(v).unwrap()
}

/// An outline as the path commands it draws, each point moved by an offset.
struct Path {
    at: (f32, f32),
    cmds: Vec<(u8, Vec<f32>)>,
}

impl Path {
    fn put(&mut self, kind: u8, pts: &[f32]) {
        let moved = pts
            .chunks(2)
            .flat_map(|p| [p[0] + self.at.0, p[1] + self.at.1])
            .collect();
        self.cmds.push((kind, moved));
    }
}

impl Sink for Path {
    fn move_to(&mut self, x: f32, y: f32) {
        self.put(0, &[x, y]);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.put(1, &[x, y]);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.put(2, &[x1, y1, x, y]);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.put(3, &[x1, y1, x2, y2, x, y]);
    }
    fn close(&mut self) {
        self.put(4, &[]);
    }
}

fn outline(face: &Face<'_>, g: GlyphId, at: (f32, f32), into: &mut Path) {
    into.at = at;
    face.outline(g, into);
}

/// A glyph id and its offset from its cluster's base, in font units.
type Placed = (u16, i32, i32);

/// hb-shape's output for each (built face, codepoints) in the fixtures:
/// each glyph's id and its offset from its cluster's base, in logical
/// order (the line is right to left, so the visual order reversed).
fn harfbuzz() -> HashMap<(String, Vec<u32>), Vec<Placed>> {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/shape/hebrew-faces.txt"
    ))
    .unwrap();
    let mut out = HashMap::new();
    for line in text
        .lines()
        .filter(|l| l.starts_with("crates/baud-app/fonts/built/"))
    {
        let f: Vec<&str> = line.split('\t').collect();
        let style = f[0].rsplit('/').next().unwrap().trim_end_matches(".ttf");
        let cps: Vec<u32> = f[3]
            .split_whitespace()
            .map(|h| u32::from_str_radix(h, 16).unwrap())
            .collect();
        let mut pen = 0;
        let mut glyphs: Vec<Placed> = Vec::new();
        for g in f[4].trim_matches(|c| c == '[' || c == ']').split('|') {
            let (id, rest) = g.split_once('=').unwrap();
            let (pos, ax) = rest.split_once('+').unwrap();
            let (dx, dy) = match pos.split_once('@') {
                Some((_, o)) => {
                    let (x, y) = o.split_once(',').unwrap();
                    (x.parse::<i32>().unwrap(), y.parse::<i32>().unwrap())
                }
                None => (0, 0),
            };
            glyphs.push((id.parse().unwrap(), pen + dx, dy));
            pen += ax.parse::<i32>().unwrap();
        }
        glyphs.reverse();
        let base = glyphs[0].1;
        let rel = glyphs.iter().map(|&(g, x, y)| (g, x - base, y)).collect();
        out.insert((style.to_string(), cps), rel);
    }
    out
}

/// **Each canonical presentation form draws as HarfBuzz draws its
/// decomposition.** In all four built faces, each of the 34 forms with a
/// canonical decomposition is mapped, and its outline is, command for
/// command, the letter's outline and each point's at the offset hb-shape
/// gives it for the letter and points typed apart. The control: every
/// decomposition places at least one point off its letter's origin, so the
/// offsets are compared, not assumed zero.
#[test]
fn each_canonical_presentation_form_draws_its_decomposition() {
    let hb = harfbuzz();
    let mut moved = 0;
    for style in STYLES {
        let data = built(style);
        let face = Face::parse(&data).unwrap();
        for (form, parts) in CANONICAL {
            let g = face
                .glyph(cp(form))
                .unwrap_or_else(|| panic!("{style}: U+{form:04X} is not mapped"));
            let mut got = Path {
                at: (0.0, 0.0),
                cmds: Vec::new(),
            };
            outline(&face, g, (0.0, 0.0), &mut got);
            let shaped = hb
                .get(&(style.to_string(), parts.to_vec()))
                .unwrap_or_else(|| panic!("{style}: no hb-shape line for {parts:04X?}"));
            assert_eq!(shaped.len(), parts.len(), "{style}: {parts:04X?}");
            let mut want = Path {
                at: (0.0, 0.0),
                cmds: Vec::new(),
            };
            // The letter and then each point as the decomposition orders
            // them, whatever order HarfBuzz lists two points in.
            let ordered = parts.iter().map(|&p| {
                let id = u16::try_from(face.glyph(cp(p)).expect("a part mapped").index()).unwrap();
                *shaped
                    .iter()
                    .find(|s| s.0 == id)
                    .unwrap_or_else(|| panic!("{style}: U+{p:04X} not in {shaped:?}"))
            });
            for (id, dx, dy) in ordered {
                moved += usize::from(dx != 0 || dy != 0);
                #[allow(clippy::cast_precision_loss)] // font units, far below 2^24
                outline(
                    &face,
                    GlyphId::from_shaper(id),
                    (dx as f32, dy as f32),
                    &mut want,
                );
            }
            assert!(!want.cmds.is_empty(), "{style}: {parts:04X?} draws nothing");
            assert!(
                got.cmds == want.cmds,
                "{style}: U+{form:04X} is not its decomposition {parts:04X?} as hb-shape draws it"
            );
        }
    }
    assert!(
        moved >= 4 * 34,
        "the control: points placed off the origin, {moved}"
    );
}

/// The on-curve points of a code point's outline.
fn points(face: &Face<'_>, c: u32) -> Vec<(f32, f32)> {
    let g = face
        .glyph(cp(c))
        .unwrap_or_else(|| panic!("U+{c:04X} is not mapped"));
    let mut p = Path {
        at: (0.0, 0.0),
        cmds: Vec::new(),
    };
    face.outline(g, &mut p);
    p.cmds
        .iter()
        .filter(|(_, v)| v.len() >= 2)
        .map(|(_, v)| (v[v.len() - 2], v[v.len() - 1]))
        .collect()
}

fn bounds(pts: &[(f32, f32)]) -> (f32, f32, f32, f32) {
    pts.iter().fold(
        (f32::MAX, f32::MAX, f32::MIN, f32::MIN),
        |(a, b, c, d), &(x, y)| (a.min(x), b.min(y), c.max(x), d.max(y)),
    )
}

/// **The other presentation forms draw what the Standard describes.** In
/// all four built faces: the alternative ayin is the ayin; each wide letter
/// is its letter stretched across the cell, wider than the letter on both
/// sides and inside the cell; the alternative plus sign is an inverted T,
/// with no arm below its bar -- the ink at its foot runs the plus sign's
/// whole width, where the plus's own foot is its stem alone; the alef lamed
/// ligature rises above the alef with a lamed's mast; and varika is a mark,
/// with no advance.
#[test]
fn the_other_presentation_forms_draw_what_the_standard_describes() {
    for style in STYLES {
        let data = built(style);
        let face = Face::parse(&data).unwrap();
        assert_eq!(points(&face, 0xFB20), points(&face, 0x05E2), "{style}");
        for (wide, letter) in [
            (0xFB21, 0x05D0),
            (0xFB22, 0x05D3),
            (0xFB23, 0x05D4),
            (0xFB24, 0x05DB),
            (0xFB25, 0x05DC),
            (0xFB26, 0x05DD),
            (0xFB27, 0x05E8),
            (0xFB28, 0x05EA),
        ] {
            let (w, l) = (bounds(&points(&face, wide)), bounds(&points(&face, letter)));
            assert!(
                w.0 < l.0 - 20.0 && w.2 > l.2 + 20.0,
                "{style}: U+{wide:04X} {w:?} is not wider than its letter {l:?}"
            );
            // An italic leans 9 degrees about the letters' mid-height, 315,
            // so its foot may stand up to tan(9) x 315 = 50 units left of
            // the cell and its head as far right.
            let lean = if style.contains("Italic") { 50.0 } else { 0.0 };
            assert!(
                w.0 >= -lean && w.2 <= 600.0 + lean,
                "{style}: U+{wide:04X} {w:?} leaves the cell"
            );
        }
        let plus = bounds(&points(&face, 0x002B));
        let alt = points(&face, 0xFB29);
        let a = bounds(&alt);
        let foot = |pts: &[(f32, f32)], low: f32| {
            bounds(
                &pts.iter()
                    .copied()
                    .filter(|p| p.1 <= low + 30.0)
                    .collect::<Vec<_>>(),
            )
        };
        let (pf, af) = (foot(&points(&face, 0x002B), plus.1), foot(&alt, a.1));
        assert!(
            pf.2 - pf.0 < 0.4 * (plus.2 - plus.0),
            "the control: the plus's foot is its stem, {pf:?}"
        );
        assert!(
            af.2 - af.0 > 0.8 * (plus.2 - plus.0),
            "{style}: U+FB29's foot is not its bar, {af:?}"
        );
        assert!(a.3 < plus.3, "{style}: U+FB29 reaches the plus's top");
        let (lig, alef) = (
            bounds(&points(&face, 0xFB4F)),
            bounds(&points(&face, 0x05D0)),
        );
        assert!(
            lig.3 > alef.3 + 100.0,
            "{style}: U+FB4F {lig:?} has no mast"
        );
        let varika = face.glyph(cp(0xFB1E)).expect("U+FB1E mapped");
        assert_eq!(
            face.advance(varika).map(|a| a.value()),
            Some(0),
            "{style}: varika advances"
        );
    }
}
