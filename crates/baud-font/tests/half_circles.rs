//! The half-filled circles the faces baud embeds draw: U+25D0 CIRCLE WITH
//! LEFT HALF BLACK and U+25D1 CIRCLE WITH RIGHT HALF BLACK, each the face's
//! own U+25CB WHITE CIRCLE with one half filled as U+25CF BLACK CIRCLE fills
//! it. Each is two contours: the white circle's outer contour, which is the
//! black circle's outline, and a counter on the white side that is a run of
//! the white circle's inner contour closed by the vertical chord through the
//! centre. Every point is one of the white circle's, so the comparison is
//! exact.

use baud_domain::{Codepoint, GlyphId};
use baud_font::{Drawn, Face, Sink};

/// The four faces the app embeds as its own.
const FACES: [&str; 4] = ["Regular", "Bold", "Italic", "BoldItalic"];

const WHITE: char = '\u{25CB}';
const BLACK: char = '\u{25CF}';
const LEFT_BLACK: char = '\u{25D0}';
const RIGHT_BLACK: char = '\u{25D1}';

fn font(style: &str) -> Vec<u8> {
    let path = format!(
        "{}/../baud-app/fonts/built/{style}.ttf",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Call {
    Move(f32, f32),
    Line(f32, f32),
    Quad(f32, f32, f32, f32),
    Curve(f32, f32, f32, f32, f32, f32),
}

impl Call {
    fn end(self) -> (f32, f32) {
        match self {
            Call::Move(x, y) | Call::Line(x, y) | Call::Quad(_, _, x, y) => (x, y),
            Call::Curve(_, _, _, _, x, y) => (x, y),
        }
    }

    fn xs(self) -> Vec<f32> {
        match self {
            Call::Move(x, _) | Call::Line(x, _) => vec![x],
            Call::Quad(x1, _, x, _) => vec![x1, x],
            Call::Curve(x1, _, x2, _, x, _) => vec![x1, x2, x],
        }
    }

    fn shifted(self, dx: f32) -> Call {
        match self {
            Call::Move(x, y) => Call::Move(x + dx, y),
            Call::Line(x, y) => Call::Line(x + dx, y),
            Call::Quad(x1, y1, x, y) => Call::Quad(x1 + dx, y1, x + dx, y),
            Call::Curve(x1, y1, x2, y2, x, y) => Call::Curve(x1 + dx, y1, x2 + dx, y2, x + dx, y),
        }
    }
}

/// Every call a glyph's outline makes, one list per contour, each list
/// ended by `close`.
#[derive(Default)]
struct Calls {
    contours: Vec<Vec<Call>>,
    open: Vec<Call>,
}

impl Sink for Calls {
    fn move_to(&mut self, x: f32, y: f32) {
        assert!(self.open.is_empty(), "a contour opened inside another");
        self.open.push(Call::Move(x, y));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.open.push(Call::Line(x, y));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.open.push(Call::Quad(x1, y1, x, y));
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.open.push(Call::Curve(x1, y1, x2, y2, x, y));
    }
    fn close(&mut self) {
        let c = core::mem::take(&mut self.open);
        assert!(
            matches!(c.first(), Some(Call::Move(..))),
            "a contour closed without a move: {c:?}"
        );
        self.contours.push(c);
    }
}

fn drawn(face: &Face<'_>, g: GlyphId) -> Vec<Vec<Call>> {
    let mut calls = Calls::default();
    assert_eq!(face.outline(g, &mut calls), Drawn::Outline);
    assert!(calls.open.is_empty(), "a contour left open");
    calls.contours
}

fn mapped(face: &Face<'_>, style: &str, c: char) -> GlyphId {
    face.glyph(Codepoint::from_char(c))
        .unwrap_or_else(|| panic!("U+{:04X}: built/{style}.ttf does not map it", u32::from(c)))
}

/// The white circle's two contours and its centre's x.
struct Circles {
    outer: Vec<Call>,
    inner: Vec<Call>,
    cx: f32,
}

/// The face's white circle, checked against its black circle: the black
/// circle moved onto the white circle's centre is the white circle's outer
/// contour, call for call.
fn circles(face: &Face<'_>, style: &str) -> Circles {
    let white = drawn(face, mapped(face, style, WHITE));
    let black = drawn(face, mapped(face, style, BLACK));
    assert_eq!(white.len(), 2, "{style}: the white circle is a ring");
    assert_eq!(black.len(), 1, "{style}: the black circle is a disc");
    let (outer, inner) = (white[0].clone(), white[1].clone());
    let xs: Vec<f32> = outer.iter().flat_map(|c| c.xs()).collect();
    let lo = xs.iter().copied().fold(f32::INFINITY, f32::min);
    let hi = xs.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let cx = lo + (hi - lo) / 2.0;
    let bxs: Vec<f32> = black[0].iter().flat_map(|c| c.xs()).collect();
    let blo = bxs.iter().copied().fold(f32::INFINITY, f32::min);
    let bhi = bxs.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let dx = cx - (blo + (bhi - blo) / 2.0);
    let moved: Vec<Call> = black[0].iter().map(|c| c.shifted(dx)).collect();
    assert_eq!(
        moved, outer,
        "{style}: the black circle moved by {dx} is the white circle's outer contour"
    );
    Circles { outer, inner, cx }
}

/// The run of `inner`'s curves from the on-curve point at `(cx, from_y)` to
/// the one at `(cx, to_y)`, in the contour's own order and wrapping past its
/// end, as the contour a glyph that keeps only that run draws: a move to its
/// start, the run, and the line back to the start along `x = cx`.
fn counter(inner: &[Call], cx: f32, from_y: f32, to_y: f32) -> Vec<Call> {
    let start = inner[0].end();
    let mut at = start;
    let mut segments = Vec::new();
    for &c in &inner[1..] {
        assert!(
            matches!(c, Call::Quad(..)),
            "the inner contour is quadratic curves: {c:?}"
        );
        segments.push((at, c));
        at = c.end();
    }
    assert_eq!(
        at, start,
        "the inner contour's last curve ends where it starts"
    );
    let first = segments
        .iter()
        .position(|&(from, _)| from == (cx, from_y))
        .unwrap_or_else(|| panic!("no curve of the inner contour starts at ({cx}, {from_y})"));
    let mut run = vec![Call::Move(cx, from_y)];
    for k in 0..segments.len() {
        let (_, c) = segments[(first + k) % segments.len()];
        run.push(c);
        if c.end() == (cx, to_y) {
            run.push(Call::Line(cx, from_y));
            return run;
        }
    }
    panic!("the inner contour never reaches ({cx}, {to_y})")
}

fn extent(contour: &[Call]) -> (f32, f32) {
    let ys = contour.iter().map(|c| c.end().1);
    let lo = ys.clone().fold(f32::INFINITY, f32::min);
    let hi = ys.fold(f32::NEG_INFINITY, f32::max);
    (lo, hi)
}

/// **Each embedded face maps both half circles itself**, each to a glyph of
/// its own -- neither the white nor the black circle's -- that advances and
/// bears as the white circle does, so a cell of them lines up with a cell of
/// the circles beside it.
#[test]
fn the_embedded_faces_map_both_half_circles() {
    for style in FACES {
        let data = font(style);
        let face = Face::parse(&data).unwrap();
        let white = mapped(&face, style, WHITE);
        let black = mapped(&face, style, BLACK);
        let left = mapped(&face, style, LEFT_BLACK);
        let right = mapped(&face, style, RIGHT_BLACK);
        assert_ne!(left, right, "{style}");
        for g in [left, right] {
            assert_ne!(g, white, "{style}");
            assert_ne!(g, black, "{style}");
            assert_eq!(
                face.default_advance(g),
                face.default_advance(white),
                "{style}"
            );
            assert_eq!(face.side_bearing(g), face.side_bearing(white), "{style}");
        }
    }
}

/// **Each half circle is the white circle's outline with one half filled as
/// the black circle fills it.** Its first contour is the white circle's
/// outer contour call for call, which is the black circle's outline moved
/// onto the white circle's centre. Its second is the counter on the white
/// side: for U+25D0 the white circle's inner contour from its bottom through
/// its right side to its top, for U+25D1 from its top through its left side
/// to its bottom, each closed by a line back down or up `x = cx`, and every
/// point of it on that side of `x = cx`.
#[test]
fn each_half_circle_is_the_white_circles_outline_with_one_half_filled() {
    for style in FACES {
        let data = font(style);
        let face = Face::parse(&data).unwrap();
        let Circles { outer, inner, cx } = circles(&face, style);
        let (bottom, top) = extent(&inner);
        for (c, from_y, to_y, white_right) in [
            (LEFT_BLACK, bottom, top, true),
            (RIGHT_BLACK, top, bottom, false),
        ] {
            let name = format!("{style}, U+{:04X}", u32::from(c));
            let want = counter(&inner, cx, from_y, to_y);
            for x in want.iter().flat_map(|c| c.xs()) {
                assert!(
                    if white_right { x >= cx } else { x <= cx },
                    "{name}: the counter's run crosses x = {cx} at {x}"
                );
            }
            let got = drawn(&face, mapped(&face, style, c));
            assert_eq!(got.len(), 2, "{name}: an outer contour and one counter");
            assert_eq!(got[0], outer, "{name}: the white circle's outer contour");
            assert_eq!(got[1], want, "{name}: the counter on the white side");
        }
    }
}
