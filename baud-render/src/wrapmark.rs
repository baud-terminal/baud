//! **A mark beside each line the word wrap continued**: a hook arrow,
//! `↪`, in the window's padding at the start of every line that carries
//! on a paragraph the display re-wrapped at its words
//! ([`crate::rebreak`], `[screen] wrap_words`), so a continuation that
//! begins `/jordan` does not read as a path the program printed.
//!
//! **Where.** In a band of the window's padding, never in a cell: the
//! grid keeps every column it had, a program is told the same size, and a
//! copy, a selection, a search and a mouse report never meet the mark,
//! since it is no stored cell. The band is the start side of the line:
//! the left for a line laid out left to right, the right for one laid out
//! right to left, mirrored. The driver lends the bands from the padding
//! ([`crate::Renderer::set_margins`]); with no padding, or too little for
//! the mark ([`room`]), nothing is drawn and no column is taken. The
//! exit-status strip, when kept, lies between the left band and the grid,
//! and a legacy scroller's gutter between the grid and the right band, so
//! neither meets a mark.
//!
//! **Which lines.** Only the lines baud itself re-wrapped: every line of a
//! paragraph the display drew at its words but its first. A row the
//! terminal cut at the last column, the alternate screen, the line being
//! typed and a command still running are the program's lines and have no
//! mark, as the display never re-wraps them.
//!
//! **The look.** A drawn mask, so every system draws the same pixels, in
//! the palette's entry 8 lifted until it reads at 3:1 against the page
//! over whatever the glass shows ([`crate::color::reads_at`]): WCAG 2.2's
//! SC 1.4.11, "a contrast ratio of at least 3:1 against adjacent
//! color(s)" for "Visual information required to identify user interface
//! components and states" (`[[source: wcag22]]`). The shape carries the
//! meaning, not the colour (SC 1.4.1).

use baud_domain::BidiDirection;
use baud_font::{Canvas, Extent, Point, Px};

use crate::det::Det;
use crate::units::slots;

/// **Whether the continuation mark is drawn**, `[screen] wrap_marker`: on
/// by default, beside the lines `wrap_words` re-wrapped.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum WrapMarker {
    #[default]
    On,
    Off,
}

/// Which band a mark is in: the left, for a line laid out left to right,
/// or the right, mirrored, for one laid out right to left.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Side {
    Left,
    Right,
}

impl Side {
    /// The start side of a line laid out `dir`.
    pub(crate) fn of(dir: BidiDirection) -> Side {
        match dir {
            BidiDirection::Ltr => Side::Left,
            BidiDirection::Rtl => Side::Right,
        }
    }
}

/// The narrowest box a mark is drawn in: below it the arrow's head and
/// hook are no longer told apart, so nothing is drawn.
pub const MIN_WIDTH: Px = Px::of(6);

/// **The box a mark takes in a band `band` wide**, for a cell `cell_w`
/// wide: the band less a pixel each side, at most a cell; none where that
/// is narrower than [`MIN_WIDTH`].
pub fn room(band: Px, cell_w: Px) -> Option<Px> {
    let w = band.value().saturating_sub(2).min(cell_w.value());
    (w >= MIN_WIDTH.value()).then_some(Px::of(w))
}

/// **The mark's mask**, `w` by `h`, the row's box: a hook arrow, its
/// hook a half ring at the start, its shaft along the lower half and its
/// head at the end, the whole centred on the row; mirrored for
/// [`Side::Right`]. Every part is one polygon turned the same way, so
/// where two overlap the coverage is full, not doubled or cancelled.
pub(crate) fn draw(
    side: Side,
    w: Px,
    h: Px,
    scratch: &mut Vec<f32>,
    coverage: &mut Vec<u8>,
) -> Option<Extent> {
    let extent = Extent::new(w, h);
    let cells = extent.cells()?;
    if coverage.len() < cells {
        coverage.resize(cells, 0);
    }
    coverage[..cells].fill(0);
    let need = extent.scratch()?;
    if scratch.len() < need {
        scratch.resize(need, 0.0);
    }
    let (wf, hf) = (fx(w), fx(h));
    // The glyph's square: the box's width, at most half the row's height.
    let s = wf.min(hf * 0.5);
    let t = (s / 7.0).max(1.0);
    let r = s * 0.24;
    let head = s * 0.36;
    let half = s * 0.3;
    // Its top is the hook's, its bottom the head's: centred on the row.
    let shaft = hf / 2.0 - (half - 2.0 * r - t / 2.0) / 2.0;
    let x0 = (wf - s) / 2.0;
    let (cx, cy) = (x0 + r + t / 2.0, shaft - r);
    let at = |x: f32, y: f32| match side {
        Side::Left => Point::new(x, y),
        Side::Right => Point::new(wf - x, y),
    };
    let mut canvas = Canvas::new(extent, scratch)?;
    // The hook: a half ring on the start side, from its top round to the
    // shaft.
    const STEPS: u16 = 16;
    let arc = |radius: f32, k: u16| {
        let a =
            core::f32::consts::FRAC_PI_2 + core::f32::consts::PI * f32::from(k) / f32::from(STEPS);
        let (sin, cos) = a.det_sin_cos();
        (cx + radius * cos, cy - radius * sin)
    };
    let mut hook: Vec<(f32, f32)> = (0..=STEPS).map(|k| arc(r + t / 2.0, k)).collect();
    hook.extend((0..=STEPS).rev().map(|k| arc(r - t / 2.0, k)));
    polygon(&mut canvas, &hook, at);
    // The shaft, from the hook's foot to the head's back.
    let end = x0 + s - head * 0.5;
    polygon(
        &mut canvas,
        &[
            (cx, shaft - t / 2.0),
            (end, shaft - t / 2.0),
            (end, shaft + t / 2.0),
            (cx, shaft + t / 2.0),
        ],
        at,
    );
    // The head.
    polygon(
        &mut canvas,
        &[
            (x0 + s, shaft),
            (x0 + s - head, shaft + half),
            (x0 + s - head, shaft - half),
        ],
        at,
    );
    canvas.coverage(&mut coverage[..cells])?;
    Some(extent)
}

/// A closed polygon of `points`, turned clockwise on the canvas whatever
/// order it is given in, through `at` (the mirror).
fn polygon(canvas: &mut Canvas<'_>, points: &[(f32, f32)], at: impl Fn(f32, f32) -> Point) {
    let p: Vec<Point> = points.iter().map(|&(x, y)| at(x, y)).collect();
    let n = p.len();
    let area: f32 = (0..n)
        .map(|i| {
            let (a, b) = (p[i], p[(i + 1) % n]);
            a.x * b.y - b.x * a.y
        })
        .sum();
    let order: Vec<usize> = match area >= 0.0 {
        true => (0..n).collect(),
        false => (0..n).rev().collect(),
    };
    for i in 0..n {
        canvas.line(p[order[i]], p[order[(i + 1) % n]]);
    }
}

fn fx(p: Px) -> f32 {
    // A cell is a few dozen pixels: exact in an `f32`.
    #[allow(clippy::cast_precision_loss)]
    let v = slots(p) as f32;
    v
}
