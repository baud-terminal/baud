//! The parallelograms the faces baud embeds draw: U+25B0 BLACK
//! PARALLELOGRAM and U+25B1 WHITE PARALLELOGRAM, each on the face's own
//! U+25A0 BLACK SQUARE and U+25A1 WHITE SQUARE. The black parallelogram is
//! the square's band and advance with its top edge moved right by a quarter
//! of its height and its bottom edge cut short by as much, so its sides lean
//! one unit across for four up; the white one is that outline with a counter
//! whose edges stand the white square's stroke inside it.

use baud_domain::{Codepoint, GlyphId};
use baud_font::{Canvas, Drawn, Extent, Face, Px, Rasterizer, Scale, Sink};

/// The four faces the app embeds as its own.
const FACES: [&str; 4] = ["Regular", "Bold", "Italic", "BoldItalic"];

const BLACK_SQUARE: char = '\u{25A0}';
const WHITE_SQUARE: char = '\u{25A1}';
const BLACK: char = '\u{25B0}';
const WHITE: char = '\u{25B1}';

/// How far a side leans across for each unit up.
const SLANT: f32 = 0.25;

/// The raster: pixels per em, the mask's side, and where the origin sits.
const PX: f32 = 100.0;
const SIDE: u16 = 80;
const LEFT: f32 = 10.0;
const BASELINE: f32 = 75.0;

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
}

/// Every call a glyph's outline makes, one list per contour, each list
/// ended by `close`; a curve panics, as no glyph here has one.
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
    fn quad_to(&mut self, _: f32, _: f32, x: f32, y: f32) {
        panic!("a curve to ({x}, {y}) in a glyph of straight edges");
    }
    fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, x: f32, y: f32) {
        panic!("a curve to ({x}, {y}) in a glyph of straight edges");
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

type Pt = (f32, f32);

/// A contour of straight edges as its corners in order: the move's point
/// and each line's end, without a last line back to the start.
fn corners(contour: &[Call]) -> Vec<Pt> {
    let mut pts = Vec::new();
    for &c in contour {
        let (Call::Move(x, y) | Call::Line(x, y)) = c;
        pts.push((x, y));
    }
    if pts.len() > 1 && pts.last() == pts.first() {
        pts.pop();
    }
    pts
}

/// Twice the signed area, positive anticlockwise with y up.
fn twice_area(pts: &[Pt]) -> f32 {
    let n = pts.len();
    (0..n)
        .map(|k| {
            let (a, b) = (pts[k], pts[(k + 1) % n]);
            a.0 * b.1 - b.0 * a.1
        })
        .sum()
}

/// The corners' extent: lowest and highest x, then lowest and highest y.
struct Box4 {
    x0: f32,
    x1: f32,
    y0: f32,
    y1: f32,
}

fn extent(pts: &[Pt]) -> Box4 {
    let xs = pts.iter().map(|p| p.0);
    let ys = pts.iter().map(|p| p.1);
    Box4 {
        x0: xs.clone().fold(f32::INFINITY, f32::min),
        x1: xs.fold(f32::NEG_INFINITY, f32::max),
        y0: ys.clone().fold(f32::INFINITY, f32::min),
        y1: ys.fold(f32::NEG_INFINITY, f32::max),
    }
}

/// The two corners at height `y`, left then right.
fn edge_at(pts: &[Pt], y: f32) -> (Pt, Pt) {
    let mut on: Vec<Pt> = pts.iter().copied().filter(|p| p.1 == y).collect();
    assert_eq!(on.len(), 2, "two corners at y = {y}: {pts:?}");
    on.sort_by(|a, b| a.0.total_cmp(&b.0));
    (on[0], on[1])
}

/// The square's outline and the white square's counter, each as corners.
struct Squares {
    outer: Vec<Pt>,
    counter: Vec<Pt>,
}

fn squares(face: &Face<'_>, style: &str) -> Squares {
    let black = drawn(face, mapped(face, style, BLACK_SQUARE));
    let white = drawn(face, mapped(face, style, WHITE_SQUARE));
    assert_eq!(black.len(), 1, "{style}: the black square is one contour");
    assert_eq!(white.len(), 2, "{style}: the white square is a frame");
    let outer = corners(&black[0]);
    assert_eq!(corners(&white[0]), outer, "{style}: □'s outline is ■'s");
    Squares {
        outer,
        counter: corners(&white[1]),
    }
}

/// `c`'s coverage at `PX`, row-major on a `SIDE` square, each 0..=1.
fn coverage(face: &Face<'_>, style: &str, c: char) -> Vec<f32> {
    let g = mapped(face, style, c);
    let size = Extent::new(Px::of(SIDE.into()), Px::of(SIDE.into()));
    let mut scratch = vec![0f32; size.scratch().unwrap()];
    let canvas = Canvas::new(size, &mut scratch).unwrap();
    let mut r = Rasterizer::new(
        canvas,
        Scale::of(PX, face.units_per_em()).placing(LEFT, BASELINE),
    );
    face.outline(g, &mut r);
    let mut cov = vec![0u8; size.cells().unwrap()];
    r.finish().coverage(&mut cov).unwrap();
    cov.into_iter().map(|v| f32::from(v) / 255.0).collect()
}

/// Font units to pixels at `PX`, for a face of `upem` units.
fn px(units: f32, upem: f32) -> f32 {
    units * PX / upem
}

/// The coverage of the pixel holding the point `p`, in font units.
fn at(cov: &[f32], p: Pt, upem: f32) -> f32 {
    let (x, y) = (LEFT + px(p.0, upem), BASELINE - px(p.1, upem));
    let side = usize::from(SIDE);
    for row in 0..SIDE {
        for col in 0..SIDE {
            let (cx, cy) = (f32::from(col), f32::from(row));
            if cx <= x && x < cx + 1.0 && cy <= y && y < cy + 1.0 {
                return cov[usize::from(row) * side + usize::from(col)];
            }
        }
    }
    panic!("({}, {}) is off the mask", p.0, p.1)
}

/// **Each embedded face maps both parallelograms itself**, each to a glyph
/// of its own -- neither square's -- that advances and bears as the black
/// square does, so a run of them lines up with the squares beside it.
#[test]
fn the_embedded_faces_map_both_parallelograms() {
    for style in FACES {
        let data = font(style);
        let face = Face::parse(&data).unwrap();
        let square = mapped(&face, style, BLACK_SQUARE);
        let frame = mapped(&face, style, WHITE_SQUARE);
        let black = mapped(&face, style, BLACK);
        let white = mapped(&face, style, WHITE);
        assert_ne!(black, white, "{style}");
        for g in [black, white] {
            assert_ne!(g, square, "{style}");
            assert_ne!(g, frame, "{style}");
            assert_eq!(
                face.default_advance(g),
                face.default_advance(square),
                "{style}"
            );
            assert_eq!(face.side_bearing(g), face.side_bearing(square), "{style}");
        }
    }
}

/// **The black parallelogram is the black square's band, slanted.** One
/// contour of four straight edges wound as the square's: its bottom edge on
/// the square's bottom from the square's left, its top edge on the square's
/// top to the square's right, each shorter than the square's by a quarter
/// of its height, so both sides lean `SLANT` across per unit up.
#[test]
fn the_black_parallelogram_is_the_squares_band_slanted() {
    for style in FACES {
        let data = font(style);
        let face = Face::parse(&data).unwrap();
        let Squares { outer: sq, .. } = squares(&face, style);
        let b = extent(&sq);
        let shape = drawn(&face, mapped(&face, style, BLACK));
        assert_eq!(shape.len(), 1, "{style}: ▰ is one contour");
        let pts = corners(&shape[0]);
        assert_eq!(pts.len(), 4, "{style}: ▰ has four corners: {pts:?}");
        let rise = b.y1 - b.y0;
        let shear = rise * SLANT;
        let (bl, br) = edge_at(&pts, b.y0);
        let (tl, tr) = edge_at(&pts, b.y1);
        assert_eq!(bl, (b.x0, b.y0), "{style}: bottom left");
        assert_eq!(br, (b.x1 - shear, b.y0), "{style}: bottom right");
        assert_eq!(tl, (b.x0 + shear, b.y1), "{style}: top left");
        assert_eq!(tr, (b.x1, b.y1), "{style}: top right");
        assert_eq!(
            twice_area(&pts).signum(),
            twice_area(&sq).signum(),
            "{style}: ▰ is wound as ■ is"
        );
    }
}

/// **The white parallelogram is the black one with the white square's
/// stroke.** Its first contour is the black parallelogram's, call for call;
/// its second is a counter wound as the white square's counter, whose
/// bottom and top edges sit at the white square's counter's, whose sides
/// lean as the outline's, and whose sides stand the stroke from the
/// outline's, measured square to the edge, to the nearest font unit.
#[test]
fn the_white_parallelogram_is_the_black_one_at_the_white_squares_stroke() {
    for style in FACES {
        let data = font(style);
        let face = Face::parse(&data).unwrap();
        let Squares { outer: sq, counter } = squares(&face, style);
        let (b, c) = (extent(&sq), extent(&counter));
        let stroke = c.y0 - b.y0;
        assert_eq!(b.y1 - c.y1, stroke, "{style}: □'s stroke");
        assert_eq!(c.x0 - b.x0, stroke, "{style}: □'s stroke");
        let black = drawn(&face, mapped(&face, style, BLACK));
        let white = drawn(&face, mapped(&face, style, WHITE));
        assert_eq!(white.len(), 2, "{style}: ▱ is a frame");
        assert_eq!(white[0], black[0], "{style}: ▱'s outline is ▰'s");
        let outer = corners(&white[0]);
        let inner = corners(&white[1]);
        assert_eq!(inner.len(), 4, "{style}: ▱'s counter: {inner:?}");
        assert_eq!(
            twice_area(&inner).signum(),
            twice_area(&counter).signum(),
            "{style}: ▱'s counter is wound as □'s"
        );
        let (obl, obr) = edge_at(&outer, b.y0);
        let (ibl, ibr) = edge_at(&inner, c.y0);
        let (itl, itr) = edge_at(&inner, c.y1);
        let rise = c.y1 - c.y0;
        assert_eq!(itl.0 - ibl.0, rise * SLANT, "{style}: the left side leans");
        assert_eq!(itr.0 - ibr.0, rise * SLANT, "{style}: the right side leans");
        // Each side's x where it crosses the outline's bottom, then the
        // gap between the parallel lines, square to them.
        let square_to = (1.0 + SLANT * SLANT).sqrt();
        let left = (ibl.0 - stroke * SLANT - obl.0) / square_to;
        let right = (obr.0 - (ibr.0 - stroke * SLANT)) / square_to;
        for (side, gap) in [("left", left), ("right", right)] {
            assert!(
                (gap - stroke).abs() < 0.5,
                "{style}: the {side} side stands {gap} from the outline, not {stroke}"
            );
        }
    }
}

/// **Drawn, ▰ is filled and ▱ outlined.** Rasterised, the black
/// parallelogram's ink is its outline's area, which is the black square's
/// less a quarter of its height across; the white one's is that less its
/// counter's area, so the counter is a hole. At the centre ▰ is ink and ▱
/// is not; in the corner the slant cuts from the square, at the bottom
/// right and the top left, neither has ink where ■ has.
#[test]
fn drawn_the_black_parallelogram_is_filled_and_the_white_one_outlined() {
    for style in FACES {
        let data = font(style);
        let face = Face::parse(&data).unwrap();
        let upem = f32::from(face.units_per_em().value());
        let Squares { outer: sq, .. } = squares(&face, style);
        let b = extent(&sq);
        let outline = corners(&drawn(&face, mapped(&face, style, BLACK))[0]);
        let counter = corners(&drawn(&face, mapped(&face, style, WHITE))[1]);
        let to_px = |area: f32| px(px(area, upem), upem);
        let square = coverage(&face, style, BLACK_SQUARE);
        let black = coverage(&face, style, BLACK);
        let white = coverage(&face, style, WHITE);
        let mass = |cov: &[f32]| cov.iter().sum::<f32>();
        let (width, rise) = (b.x1 - b.x0, b.y1 - b.y0);
        let filled = to_px(twice_area(&outline).abs() / 2.0);
        let framed = filled - to_px(twice_area(&counter).abs() / 2.0);
        for (what, got, want) in [
            ("■", mass(&square), to_px(width * rise)),
            ("▰", mass(&black), filled),
            ("▰ against ■", filled, to_px((width - rise * SLANT) * rise)),
            ("▱", mass(&white), framed),
        ] {
            assert!(
                (got - want).abs() < want * 0.01,
                "{style}: {what}'s ink is {got} px, not {want}"
            );
        }
        let centre = ((b.x0 + b.x1) / 2.0, (b.y0 + b.y1) / 2.0);
        let low_right = (b.x1 - rise * SLANT / 3.0, b.y0 + rise / 12.0);
        let high_left = (b.x0 + rise * SLANT / 3.0, b.y1 - rise / 12.0);
        assert_eq!(at(&black, centre, upem), 1.0, "{style}: ▰'s centre");
        assert_eq!(at(&white, centre, upem), 0.0, "{style}: ▱'s centre");
        for corner in [low_right, high_left] {
            assert_eq!(at(&square, corner, upem), 1.0, "{style}: the control, ■");
            assert_eq!(at(&black, corner, upem), 0.0, "{style}: ▰ at {corner:?}");
            assert_eq!(at(&white, corner, upem), 0.0, "{style}: ▱ at {corner:?}");
        }
    }
}
