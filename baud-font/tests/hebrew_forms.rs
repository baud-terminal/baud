//! Hebrew letterforms in baud's embedded font keep, in every style, the
//! shape the design gives them.
//!
//! The Hebrew is drawn once upright, Regular and Bold, and the italics are
//! those shapes leaning 9 degrees right with the Latin
//! (`baud-app/fonts/glyphs/languages/hebrew/build/`). A widening or narrowing
//! cut through a stroke must move the whole stroke, or what was a square end
//! comes out slanted.

use baud_domain::Codepoint;
use baud_font::{Face, Sink};

/// The four embedded styles, each with the tangent of its lean: the
/// italics lean 9 degrees right (note AC, section 4c).
const STYLES: [(&str, f32); 4] = [
    ("Regular", 0.0),
    ("Bold", 0.0),
    ("Italic", 0.158_384_4),
    ("BoldItalic", 0.158_384_4),
];
const ZAYIN: char = '\u{05D6}';
/// The top of a Hebrew letter, in font units.
const TOP: f32 = 630.0;
/// The band holding the underside of zayin's head: Regular's is at 548,
/// Bold's at 517. The neck's points in it stand inside the head's ends, so
/// they are neither band's leftmost nor its rightmost.
const UNDERSIDE: (f32, f32) = (500.0, 560.0);

fn built(style: &str) -> Vec<u8> {
    let path = format!(
        "{}/../baud-app/fonts/built/{style}.ttf",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// Every on-curve point of an outline, in font units.
#[derive(Default)]
struct Points(Vec<(f32, f32)>);

impl Sink for Points {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.push((x, y));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.push((x, y));
    }
    fn quad_to(&mut self, _: f32, _: f32, x: f32, y: f32) {
        self.0.push((x, y));
    }
    fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, x: f32, y: f32) {
        self.0.push((x, y));
    }
    fn close(&mut self) {}
}

fn points(face: &Face<'_>, c: char) -> Vec<(f32, f32)> {
    let g = face
        .glyph(Codepoint::from_char(c))
        .unwrap_or_else(|| panic!("U+{:04X} is not mapped", u32::from(c)));
    let mut p = Points::default();
    face.outline(g, &mut p);
    p.0
}

/// **Zayin's head ends square in every style.** Round 1 widened the head
/// by moving every point above y 540 outward, 22 units each side
/// (`design.py`, `EDITS`). Regular's head underside is at 548, so the whole
/// end moved; Bold's thicker head starts at 517, so only its top corners
/// moved and each end slanted 22 units, in Bold and Bold Italic; the band
/// has started at y 500 since 2026-10-25. Each end's outermost point at the
/// top must stand over its outermost point at the underside, within a unit,
/// once the italics' lean is taken out.
#[test]
fn zayin_head_ends_stand_square_in_every_style() {
    let mut slanted = vec![];
    for (style, lean) in STYLES {
        let data = built(style);
        let face = Face::parse(&data).unwrap();
        let pts = points(&face, ZAYIN);
        // Undo the lean about the baseline: only the difference between
        // two heights is read, so the shear's origin does not matter.
        let upright: Vec<(f32, f32)> = pts.iter().map(|&(x, y)| (x - lean * y, y)).collect();
        let at = |lo: f32, hi: f32| -> Vec<f32> {
            upright
                .iter()
                .filter(|p| p.1 >= lo && p.1 <= hi)
                .map(|p| p.0)
                .collect()
        };
        let top = at(TOP - 1.0, TOP + 1.0);
        let under = at(UNDERSIDE.0, UNDERSIDE.1);
        // Ink-present controls: a band that missed the head cannot pass.
        assert!(top.len() >= 2, "{style}: {} points at the top", top.len());
        assert!(
            under.len() >= 2,
            "{style}: {} points at the underside",
            under.len()
        );
        let min = |v: &[f32]| v.iter().copied().fold(f32::INFINITY, f32::min);
        let max = |v: &[f32]| v.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let left = min(&top) - min(&under);
        let right = max(&top) - max(&under);
        if left.abs() > 1.0 || right.abs() > 1.0 {
            slanted.push(format!("{style}: left {left:.1}, right {right:.1}"));
        }
    }
    assert!(
        slanted.is_empty(),
        "zayin's head ends slant, in units: {slanted:?}"
    );
}
