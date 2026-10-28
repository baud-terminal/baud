//! Box drawing, block elements and braille drawn by baud rather than by a
//! font: a mask of exactly the character's cells -- one, or two for one
//! East Asian Width makes wide ([`draw_across`]) -- strokes snapped to
//! whole pixels and run to the edge, so two adjacent cells' strokes meet
//! with no seam and no face has to carry the range at all.
//!
//! A font's box glyph is designed to an em box and lands in the cell
//! through its own advance, its own bearings and a `Fit` with a pixel of
//! margin; rounded to the integer cell the strokes of two neighbours stop
//! a fraction of a pixel short of each other and the join shows a hairline.
//! Drawing from the cell rectangle makes the join exact by construction:
//! a horizontal stroke occupies the same rows in every cell and runs from
//! `x = 0` to `x = cell_w`, and the compositor places the mask at the cell
//! origin on every path (`Slot { left: 0, top: -baseline }`).
//!
//! What is procedural: `U+2500..=U+257F` (box drawing), `U+2580..=U+259F`
//! (block elements and shades) and `U+2800..=U+28FF` (braille). Nothing
//! else. The shape of each box glyph is a small descriptor -- which of the
//! four arms it has and at what weight, or a dashed, arc or diagonal form
//! -- and the 29 double-line glyphs carry their segments explicitly,
//! because a double junction is not the union of its arms: `╠` keeps the
//! gap between its verticals clear and `╬` is four inner corners with an
//! empty centre, which is what every font draws. The descriptors are
//! checked against the pinned `UnicodeData.txt` names by
//! `baud-render/tests/boxes.rs`, the way the cluster table is checked
//! against the rules it replaced: two spellings of one fact.
//!
//! Strokes are drawn as integer-aligned rectangles, which the accumulation
//! rasteriser fills to full coverage with no anti-aliased edge, so a light
//! stroke is exactly `t` pixels and reads the same in every cell. Arcs,
//! diagonals and braille dots are outlines through the same canvas the
//! font path rasterises with, so their edges are anti-aliased the way a
//! glyph's are. Shades are a constant coverage over the cell.

use crate::units::{Coverage, slots};
use baud_domain::Codepoint;
use baud_font::{Canvas, Extent, Point, Px};

/// A stroke's weight: the width it is drawn at, in multiples of the light
/// stroke.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Weight {
    Light,
    Heavy,
    /// Two light strokes a light stroke apart.
    Double,
}

/// The four arms a box glyph may carry, each with its weight or absent.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Arms {
    pub up: Option<Weight>,
    pub down: Option<Weight>,
    pub left: Option<Weight>,
    pub right: Option<Weight>,
}

/// Which way a dashed line runs.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    Horizontal,
    Vertical,
}

/// The dash count of a dashed line: `U+2504` is triple, `U+2508`
/// quadruple, `U+254C` double.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dashes {
    Two,
    Three,
    Four,
}

impl Dashes {
    fn count(self) -> f32 {
        match self {
            Dashes::Two => 2.0,
            Dashes::Three => 3.0,
            Dashes::Four => 4.0,
        }
    }
}

/// Which corner an arc rounds: the two arms it joins.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Arc {
    /// The vertical arm goes down (else up).
    pub down: bool,
    /// The horizontal arm goes right (else left).
    pub right: bool,
}

/// Which diagonals a glyph draws.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Diagonal {
    /// Upper right to lower left.
    pub falling: bool,
    /// Upper left to lower right.
    pub rising: bool,
}

/// A filled rectangle of the cell in eighths: `(x0, y0, x1, y1)`, each
/// `0..=8`, across then down.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Eighths(pub u8, pub u8, pub u8, pub u8);

/// The quadrants a block glyph fills, as four bits: upper left, upper
/// right, lower left, lower right.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Quadrants(pub u8);

impl Quadrants {
    pub const UL: u8 = 1;
    pub const UR: u8 = 2;
    pub const LL: u8 = 4;
    pub const LR: u8 = 8;
}

/// How much of the cell a shade covers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shade {
    Light,
    Medium,
    Dark,
}

/// The eight braille dots as a byte: dot `n` is bit `n - 1`, which is how
/// the block is laid out -- `U+2800 + bits`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Dots(pub u8);

/// What a procedural codepoint draws.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    Arms(Arms),
    Dashed(Axis, Weight, Dashes),
    Arc(Arc),
    Diagonal(Diagonal),
    /// A double-line glyph, drawn from the segment list in [`DOUBLE`].
    Double(Arms),
    Block(Eighths),
    Quadrants(Quadrants),
    Shade(Shade),
    Braille(Dots),
}

const NONE: Option<Weight> = None;
const L: Option<Weight> = Some(Weight::Light);
const H: Option<Weight> = Some(Weight::Heavy);
const D: Option<Weight> = Some(Weight::Double);

const fn arms(
    up: Option<Weight>,
    down: Option<Weight>,
    left: Option<Weight>,
    right: Option<Weight>,
) -> Shape {
    Shape::Arms(Arms {
        up,
        down,
        left,
        right,
    })
}

const fn double(
    up: Option<Weight>,
    down: Option<Weight>,
    left: Option<Weight>,
    right: Option<Weight>,
) -> Shape {
    Shape::Double(Arms {
        up,
        down,
        left,
        right,
    })
}

/// `U+2500..=U+257F`, in order. Arms are `(up, down, left, right)`.
const BOX: [Shape; 128] = [
    // 2500..2503: the plain lines.
    arms(NONE, NONE, L, L),
    arms(NONE, NONE, H, H),
    arms(L, L, NONE, NONE),
    arms(H, H, NONE, NONE),
    // 2504..250B: dashed.
    Shape::Dashed(Axis::Horizontal, Weight::Light, Dashes::Three),
    Shape::Dashed(Axis::Horizontal, Weight::Heavy, Dashes::Three),
    Shape::Dashed(Axis::Vertical, Weight::Light, Dashes::Three),
    Shape::Dashed(Axis::Vertical, Weight::Heavy, Dashes::Three),
    Shape::Dashed(Axis::Horizontal, Weight::Light, Dashes::Four),
    Shape::Dashed(Axis::Horizontal, Weight::Heavy, Dashes::Four),
    Shape::Dashed(Axis::Vertical, Weight::Light, Dashes::Four),
    Shape::Dashed(Axis::Vertical, Weight::Heavy, Dashes::Four),
    // 250C..250F: down and right.
    arms(NONE, L, NONE, L),
    arms(NONE, L, NONE, H),
    arms(NONE, H, NONE, L),
    arms(NONE, H, NONE, H),
    // 2510..2513: down and left.
    arms(NONE, L, L, NONE),
    arms(NONE, L, H, NONE),
    arms(NONE, H, L, NONE),
    arms(NONE, H, H, NONE),
    // 2514..2517: up and right.
    arms(L, NONE, NONE, L),
    arms(L, NONE, NONE, H),
    arms(H, NONE, NONE, L),
    arms(H, NONE, NONE, H),
    // 2518..251B: up and left.
    arms(L, NONE, L, NONE),
    arms(L, NONE, H, NONE),
    arms(H, NONE, L, NONE),
    arms(H, NONE, H, NONE),
    // 251C..2523: vertical and right.
    arms(L, L, NONE, L),
    arms(L, L, NONE, H),
    arms(H, L, NONE, L),
    arms(L, H, NONE, L),
    arms(H, H, NONE, L),
    arms(H, L, NONE, H),
    arms(L, H, NONE, H),
    arms(H, H, NONE, H),
    // 2524..252B: vertical and left.
    arms(L, L, L, NONE),
    arms(L, L, H, NONE),
    arms(H, L, L, NONE),
    arms(L, H, L, NONE),
    arms(H, H, L, NONE),
    arms(H, L, H, NONE),
    arms(L, H, H, NONE),
    arms(H, H, H, NONE),
    // 252C..2533: down and horizontal.
    arms(NONE, L, L, L),
    arms(NONE, L, H, L),
    arms(NONE, L, L, H),
    arms(NONE, L, H, H),
    arms(NONE, H, L, L),
    arms(NONE, H, H, L),
    arms(NONE, H, L, H),
    arms(NONE, H, H, H),
    // 2534..253B: up and horizontal.
    arms(L, NONE, L, L),
    arms(L, NONE, H, L),
    arms(L, NONE, L, H),
    arms(L, NONE, H, H),
    arms(H, NONE, L, L),
    arms(H, NONE, H, L),
    arms(H, NONE, L, H),
    arms(H, NONE, H, H),
    // 253C..254B: vertical and horizontal.
    arms(L, L, L, L),
    arms(L, L, H, L),
    arms(L, L, L, H),
    arms(L, L, H, H),
    arms(H, L, L, L),
    arms(L, H, L, L),
    arms(H, H, L, L),
    arms(H, L, H, L),
    arms(H, L, L, H),
    arms(L, H, H, L),
    arms(L, H, L, H),
    arms(H, L, H, H),
    arms(L, H, H, H),
    arms(H, H, H, L),
    arms(H, H, L, H),
    arms(H, H, H, H),
    // 254C..254F: double dash.
    Shape::Dashed(Axis::Horizontal, Weight::Light, Dashes::Two),
    Shape::Dashed(Axis::Horizontal, Weight::Heavy, Dashes::Two),
    Shape::Dashed(Axis::Vertical, Weight::Light, Dashes::Two),
    Shape::Dashed(Axis::Vertical, Weight::Heavy, Dashes::Two),
    // 2550..256C: the double-line family; its segments are in `DOUBLE`.
    double(NONE, NONE, D, D),
    double(D, D, NONE, NONE),
    double(NONE, L, NONE, D),
    double(NONE, D, NONE, L),
    double(NONE, D, NONE, D),
    double(NONE, L, D, NONE),
    double(NONE, D, L, NONE),
    double(NONE, D, D, NONE),
    double(L, NONE, NONE, D),
    double(D, NONE, NONE, L),
    double(D, NONE, NONE, D),
    double(L, NONE, D, NONE),
    double(D, NONE, L, NONE),
    double(D, NONE, D, NONE),
    double(L, L, NONE, D),
    double(D, D, NONE, L),
    double(D, D, NONE, D),
    double(L, L, D, NONE),
    double(D, D, L, NONE),
    double(D, D, D, NONE),
    double(NONE, L, D, D),
    double(NONE, D, L, L),
    double(NONE, D, D, D),
    double(L, NONE, D, D),
    double(D, NONE, L, L),
    double(D, NONE, D, D),
    double(L, L, D, D),
    double(D, D, L, L),
    double(D, D, D, D),
    // 256D..2570: arcs.
    Shape::Arc(Arc {
        down: true,
        right: true,
    }),
    Shape::Arc(Arc {
        down: true,
        right: false,
    }),
    Shape::Arc(Arc {
        down: false,
        right: false,
    }),
    Shape::Arc(Arc {
        down: false,
        right: true,
    }),
    // 2571..2573: diagonals.
    Shape::Diagonal(Diagonal {
        falling: true,
        rising: false,
    }),
    Shape::Diagonal(Diagonal {
        falling: false,
        rising: true,
    }),
    Shape::Diagonal(Diagonal {
        falling: true,
        rising: true,
    }),
    // 2574..257B: half lines.
    arms(NONE, NONE, L, NONE),
    arms(L, NONE, NONE, NONE),
    arms(NONE, NONE, NONE, L),
    arms(NONE, L, NONE, NONE),
    arms(NONE, NONE, H, NONE),
    arms(H, NONE, NONE, NONE),
    arms(NONE, NONE, NONE, H),
    arms(NONE, H, NONE, NONE),
    // 257C..257F: mixed half lines.
    arms(NONE, NONE, L, H),
    arms(L, H, NONE, NONE),
    arms(NONE, NONE, H, L),
    arms(H, L, NONE, NONE),
];

/// A coordinate in the double-line grid: the cell's edges, the outer and
/// inner strokes of a pair, and the single stroke's span.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum At {
    /// The cell's near edge, `0`.
    Edge0,
    /// The cell's far edge, `w` or `h`.
    Edge1,
    /// The near stroke of a pair: its start.
    A0,
    /// The near stroke of a pair: its end.
    A1,
    /// The far stroke of a pair: its start.
    B0,
    /// The far stroke of a pair: its end.
    B1,
    /// The single stroke: its start.
    S0,
    /// The single stroke: its end.
    S1,
}

/// One rectangle of a double-line glyph: an `x` range and a `y` range, each
/// named by [`At`] on its own axis.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Seg {
    x0: At,
    x1: At,
    y0: At,
    y1: At,
}

const fn seg(x0: At, x1: At, y0: At, y1: At) -> Seg {
    Seg { x0, x1, y0, y1 }
}

/// A vertical stroke: the pair's near (`A`) or far (`B`) or the single
/// (`S`), from `y0` to `y1`.
const fn va(y0: At, y1: At) -> Seg {
    seg(At::A0, At::A1, y0, y1)
}
const fn vb(y0: At, y1: At) -> Seg {
    seg(At::B0, At::B1, y0, y1)
}
const fn vs(y0: At, y1: At) -> Seg {
    seg(At::S0, At::S1, y0, y1)
}
/// A horizontal stroke: the pair's upper (`A`) or lower (`B`) or the
/// single (`S`), from `x0` to `x1`.
const fn ha(x0: At, x1: At) -> Seg {
    seg(x0, x1, At::A0, At::A1)
}
const fn hb(x0: At, x1: At) -> Seg {
    seg(x0, x1, At::B0, At::B1)
}
const fn hs(x0: At, x1: At) -> Seg {
    seg(x0, x1, At::S0, At::S1)
}

use At::{A0, A1, B0, B1, Edge0 as E0, Edge1 as E1, S0, S1};

/// The segments of `U+2550..=U+256C`, in order, at most six each: the
/// junctions the way a font draws them -- a corner pairs outer with outer
/// and inner with inner, a tee's arm starts at the inner stroke of the pair
/// it meets and the inner stroke is broken for it, and `╬` is four inner
/// corners about an empty centre. `A` is the upper or left stroke of a
/// pair, `B` the lower or right.
const DOUBLE: [&[Seg]; 29] = [
    // 2550 ═
    &[ha(E0, E1), hb(E0, E1)],
    // 2551 ║
    &[va(E0, E1), vb(E0, E1)],
    // 2552 ╒ down single, right double
    &[vs(A0, E1), ha(S0, E1), hb(S0, E1)],
    // 2553 ╓ down double, right single
    &[va(S0, E1), vb(S0, E1), hs(A0, E1)],
    // 2554 ╔
    &[va(A0, E1), vb(B0, E1), ha(A0, E1), hb(B0, E1)],
    // 2555 ╕ down single, left double
    &[vs(A0, E1), ha(E0, S1), hb(E0, S1)],
    // 2556 ╖ down double, left single
    &[va(S0, E1), vb(S0, E1), hs(E0, B1)],
    // 2557 ╗
    &[va(B0, E1), vb(A0, E1), ha(E0, B1), hb(E0, A1)],
    // 2558 ╘ up single, right double
    &[vs(E0, B1), ha(S0, E1), hb(S0, E1)],
    // 2559 ╙ up double, right single
    &[va(E0, S1), vb(E0, S1), hs(A0, E1)],
    // 255A ╚
    &[va(E0, B1), vb(E0, A1), ha(B0, E1), hb(A0, E1)],
    // 255B ╛ up single, left double
    &[vs(E0, B1), ha(E0, S1), hb(E0, S1)],
    // 255C ╜ up double, left single
    &[va(E0, S1), vb(E0, S1), hs(E0, B1)],
    // 255D ╝
    &[va(E0, A1), vb(E0, B1), ha(E0, A1), hb(E0, B1)],
    // 255E ╞ vertical single, right double
    &[vs(E0, E1), ha(S0, E1), hb(S0, E1)],
    // 255F ╟ vertical double, right single
    &[va(E0, E1), vb(E0, E1), hs(B0, E1)],
    // 2560 ╠
    &[va(E0, E1), vb(E0, A1), vb(B0, E1), ha(B0, E1), hb(B0, E1)],
    // 2561 ╡ vertical single, left double
    &[vs(E0, E1), ha(E0, S1), hb(E0, S1)],
    // 2562 ╢ vertical double, left single
    &[va(E0, E1), vb(E0, E1), hs(E0, A1)],
    // 2563 ╣
    &[vb(E0, E1), va(E0, A1), va(B0, E1), ha(E0, A1), hb(E0, A1)],
    // 2564 ╤ down single, horizontal double
    &[ha(E0, E1), hb(E0, E1), vs(B0, E1)],
    // 2565 ╥ down double, horizontal single
    &[hs(E0, E1), va(S0, E1), vb(S0, E1)],
    // 2566 ╦
    &[ha(E0, E1), hb(E0, A1), hb(B0, E1), va(B0, E1), vb(B0, E1)],
    // 2567 ╧ up single, horizontal double
    &[ha(E0, E1), hb(E0, E1), vs(E0, A1)],
    // 2568 ╨ up double, horizontal single
    &[hs(E0, E1), va(E0, S1), vb(E0, S1)],
    // 2569 ╩
    &[hb(E0, E1), ha(E0, A1), ha(B0, E1), va(E0, A1), vb(E0, A1)],
    // 256A ╪ vertical single, horizontal double
    &[vs(E0, E1), ha(E0, E1), hb(E0, E1)],
    // 256B ╫ vertical double, horizontal single
    &[va(E0, E1), vb(E0, E1), hs(E0, E1)],
    // 256C ╬
    &[
        va(E0, A1),
        va(B0, E1),
        vb(E0, A1),
        vb(B0, E1),
        ha(E0, A1),
        ha(B0, E1),
        hb(E0, A1),
        hb(B0, E1),
    ],
];

/// `U+2580..=U+259F`, in order.
const BLOCK: [Shape; 32] = {
    use Quadrants as Q;
    // Spelled out: the eighth counts are descriptor data, not pixels.
    [
        Shape::Block(Eighths(0, 0, 8, 4)), // 2580 upper half
        Shape::Block(Eighths(0, 7, 8, 8)), // 2581 lower one eighth
        Shape::Block(Eighths(0, 6, 8, 8)),
        Shape::Block(Eighths(0, 5, 8, 8)),
        Shape::Block(Eighths(0, 4, 8, 8)), // 2584 lower half
        Shape::Block(Eighths(0, 3, 8, 8)),
        Shape::Block(Eighths(0, 2, 8, 8)),
        Shape::Block(Eighths(0, 1, 8, 8)), // 2587 lower seven eighths
        Shape::Block(Eighths(0, 0, 8, 8)), // 2588 full
        Shape::Block(Eighths(0, 0, 7, 8)), // 2589 left seven eighths
        Shape::Block(Eighths(0, 0, 6, 8)),
        Shape::Block(Eighths(0, 0, 5, 8)),
        Shape::Block(Eighths(0, 0, 4, 8)), // 258C left half
        Shape::Block(Eighths(0, 0, 3, 8)),
        Shape::Block(Eighths(0, 0, 2, 8)),
        Shape::Block(Eighths(0, 0, 1, 8)), // 258F left one eighth
        Shape::Block(Eighths(4, 0, 8, 8)), // 2590 right half
        Shape::Shade(Shade::Light),
        Shape::Shade(Shade::Medium),
        Shape::Shade(Shade::Dark),
        Shape::Block(Eighths(0, 0, 8, 1)), // 2594 upper one eighth
        Shape::Block(Eighths(7, 0, 8, 8)), // 2595 right one eighth
        Shape::Quadrants(Quadrants(Q::LL)),
        Shape::Quadrants(Quadrants(Q::LR)),
        Shape::Quadrants(Quadrants(Q::UL)),
        Shape::Quadrants(Quadrants(Q::UL | Q::LL | Q::LR)),
        Shape::Quadrants(Quadrants(Q::UL | Q::LR)),
        Shape::Quadrants(Quadrants(Q::UL | Q::UR | Q::LL)),
        Shape::Quadrants(Quadrants(Q::UL | Q::UR | Q::LR)),
        Shape::Quadrants(Quadrants(Q::UR)),
        Shape::Quadrants(Quadrants(Q::UR | Q::LL)),
        Shape::Quadrants(Quadrants(Q::UR | Q::LL | Q::LR)),
    ]
};

/// What `cp` draws, or `None` for a codepoint a font draws.
pub fn shape(cp: Codepoint) -> Option<Shape> {
    let v = cp.value();
    if let Some(i) = v.checked_sub(0x2500).and_then(|i| usize::try_from(i).ok()) {
        if let Some(s) = BOX.get(i) {
            return Some(*s);
        }
        if let Some(s) = i.checked_sub(BOX.len()).and_then(|j| BLOCK.get(j)) {
            return Some(*s);
        }
    }
    if let Some(bits) = v
        .checked_sub(0x2800)
        .filter(|&i| i < 0x100)
        .and_then(|i| u8::try_from(i).ok())
    {
        return Some(Shape::Braille(Dots(bits)));
    }
    None
}

/// Whether `cp` is drawn here rather than by a font.
pub fn covers(cp: Codepoint) -> bool {
    shape(cp).is_some()
}

/// The cell a mask is drawn into, and the strokes' pixel geometry derived
/// from it: the light stroke is an eighth of the cell's width, at least a
/// pixel, and every other weight is a multiple of it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    pub w: Px,
    pub h: Px,
}

impl Cell {
    /// The light stroke.
    pub fn light(self) -> Px {
        Px::of((self.w.value() / 8).max(1))
    }

    /// The span `[start, start + size)` a stroke of `size` pixels centred on
    /// a length of `len` pixels occupies, snapped to whole pixels: the same
    /// rows in every cell for a horizontal stroke and the same columns for a
    /// vertical one, which is what makes a join exact.
    fn span(len: Px, size: Px) -> Range {
        let (len, size) = (len.value(), size.value().min(len.value()));
        Range::of(Px::of(len.saturating_sub(size) / 2), Px::of(size))
    }
}

/// **The box a mask is drawn into, and the cell its strokes are placed and
/// weighed by**: a character's whole box -- one cell, or both cells of a
/// character two cells wide -- with the light stroke of one cell and its
/// verticals centred in the first. A box line East Asian Width makes wide
/// under `ambiguous_width = "wide"` comes from the legacy sets where it is
/// full width ([[source: uax11]] ED6, § 4.2); no text says where in its two
/// columns its strokes sit. Its horizontals run the width of both cells, so
/// it joins the next character; its verticals, junctions and corners stay
/// at the first cell's centre, where a one-cell piece beside it
/// (U+2574-257F, the dashed U+254C-254F, a DEC Special Graphics line) has
/// its own; its strokes keep the one-cell weight, rather than the verticals
/// centred in both cells. VTE's minifont centres them in both
/// ([[source: vte-minifont-0-84]] 1157-1197, the box
/// `cell_width * columns`), as a CJK face's full-width glyphs do: practice
/// that differs. A diagonal runs corner to corner of the whole box, a
/// dashed line keeps its dash count across it, and a block element is its
/// named fraction of it, so U+258C fills exactly the first cell.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Area {
    w: Px,
    h: Px,
    /// The light stroke, [`Cell::light`] of the one cell.
    t: Px,
    /// The width a vertical stroke is centred in: the first cell's.
    c: Px,
    /// Where that cell begins: at the box's left edge as written, and in
    /// its last cell mirrored ([`Facing`]).
    x: Px,
}

/// **Which way a box drawing character faces in its cells**: as written,
/// or mirrored left to right as `?2500` draws one resolved right to left
/// (`[[source: terminal-wg-bidi-misc]]`, "mirror the U+2500 .. U+257F
/// glyphs in RTL"). In one cell a mirrored character is its counterpart
/// (`baud_domain::box_counterpart`); in two the whole mask is mirrored:
/// the counterpart, its verticals in the last cell -- the first in the
/// order the line is read -- at the place a one-cell piece in that cell
/// has its own, its horizontals run to the far edge.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Facing {
    Written,
    Mirrored,
}

impl Area {
    /// `cell`'s box, the cells `span` gives wide, weighed by `cell`, its
    /// verticals in the cell `facing` puts them in.
    fn across(cell: Cell, span: crate::cellmap::CellSpan, facing: Facing) -> Area {
        let cells = u32::try_from(span.cells().index()).unwrap_or(1);
        Area {
            w: Px::of(cell.w.value() * cells),
            h: cell.h,
            t: cell.light(),
            c: cell.w,
            x: match facing {
                Facing::Written => Px::of(0),
                Facing::Mirrored => Px::of(cell.w.value() * cells.saturating_sub(1)),
            },
        }
    }

    /// The columns a vertical stroke of `size` takes: centred in the cell
    /// its verticals are in.
    fn vertical(self, size: Px) -> Range {
        let r = Cell::span(self.c, size);
        Range::to(
            Px::of(r.start.value() + self.x.value()),
            Px::of(r.end.value() + self.x.value()),
        )
    }

    fn light(self) -> Px {
        self.t
    }

    fn width(self, wt: Weight) -> Px {
        let t = self.t.value();
        Px::of(match wt {
            Weight::Light => t,
            Weight::Heavy => t * 2,
            Weight::Double => t * 3,
        })
    }
}

/// A horizontal run of pixel rows or columns: `[start, end)`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Range {
    start: Px,
    end: Px,
}

impl Range {
    fn of(start: Px, size: Px) -> Range {
        Range {
            start,
            end: Px::of(start.value() + size.value()),
        }
    }

    fn to(start: Px, end: Px) -> Range {
        Range { start, end }
    }

    fn union(self, other: Range) -> Range {
        Range {
            start: Px::of(self.start.value().min(other.start.value())),
            end: Px::of(self.end.value().max(other.end.value())),
        }
    }
}

fn fx(p: Px) -> f32 {
    u16::try_from(p.value()).map(f32::from).unwrap_or(0.0)
}

/// A pixel from a float, rounded and clamped at zero -- the one place a
/// sub-pixel coordinate becomes a whole pixel, through `as i64` so the
/// audit sees a widening rather than a truncating cast.
fn round_px(v: f32) -> Px {
    Px::of(u32::try_from(v.round().max(0.0) as i64).unwrap_or(0))
}

/// The coverage buffer as a cell: straight strokes are filled to full
/// coverage over whole-pixel rectangles, so a stroke is exactly `t` pixels
/// and two neighbours' strokes meet with no fractional seam, and the
/// anti-aliased shapes -- arcs, diagonals, braille discs -- are drawn
/// through the outline rasteriser and taken at the greater coverage.
struct Mask<'a> {
    cov: &'a mut [u8],
    w: Px,
    h: Px,
}

impl Mask<'_> {
    /// `[x0, x1) x [y0, y1)` in pixels, rounded to whole pixels and clamped
    /// to the cell, filled to full coverage.
    fn rect(&mut self, x0: f32, y0: f32, x1: f32, y1: f32) {
        let (wf, hf) = (fx(self.w), fx(self.h));
        let cx = |v: f32| round_px(v.min(wf));
        let cy = |v: f32| round_px(v.min(hf));
        let (x0, x1) = (cx(x0), cx(x1));
        let (y0, y1) = (cy(y0), cy(y1));
        if x1.value() <= x0.value() || y1.value() <= y0.value() {
            return;
        }
        let w = slots(self.w);
        let (x0, x1) = (slots(x0), slots(x1));
        for y in slots(y0)..slots(y1) {
            for c in &mut self.cov[y * w + x0..y * w + x1] {
                *c = 0xff;
            }
        }
    }

    fn rect_px(&mut self, x: Range, y: Range) {
        self.rect(fx(x.start), fx(y.start), fx(x.end), fx(y.end));
    }
}

/// A closed outline into a scratch canvas, taken at the greater of the
/// existing coverage and the shape's: the anti-aliased primitives.
struct Pen<'a, 'b> {
    canvas: &'b mut Canvas<'a>,
}

impl Pen<'_, '_> {
    /// A closed polygon, reversed if it is not clockwise on screen so the
    /// interior accumulates positive.
    fn poly(&mut self, pts: &[Point]) {
        if pts.len() < 3 {
            return;
        }
        let mut area = 0.0;
        for (i, p) in pts.iter().enumerate() {
            let q = pts[(i + 1) % pts.len()];
            area += p.x * q.y - q.x * p.y;
        }
        let c = &mut *self.canvas;
        if area >= 0.0 {
            for (i, p) in pts.iter().enumerate() {
                c.line(*p, pts[(i + 1) % pts.len()]);
            }
        } else {
            for i in (0..pts.len()).rev() {
                let q = pts[(i + pts.len() - 1) % pts.len()];
                c.line(pts[i], q);
            }
        }
    }

    /// A quarter circle about `c` of radius `r` from angle `a0` to `a1`, as
    /// one cubic.
    fn arc(&mut self, c: Point, r: f32, a0: f32, a1: f32) {
        const KAPPA: f32 = 0.552_284_8;
        let (s0, c0) = a0.sin_cos();
        let (s1, c1) = a1.sin_cos();
        let p0 = Point::new(c.x + r * c0, c.y + r * s0);
        let p3 = Point::new(c.x + r * c1, c.y + r * s1);
        let sign = if a1 > a0 { 1.0 } else { -1.0 };
        let p1 = Point::new(p0.x - sign * KAPPA * r * s0, p0.y + sign * KAPPA * r * c0);
        let p2 = Point::new(p3.x + sign * KAPPA * r * s1, p3.y - sign * KAPPA * r * c1);
        self.canvas.cubic(p0, p1, p2, p3);
    }

    fn disc(&mut self, c: Point, r: f32) {
        use core::f32::consts::FRAC_PI_2;
        for k in 0..4 {
            let a0 = (k as f32) * FRAC_PI_2;
            self.arc(c, r, a0, a0 + FRAC_PI_2);
        }
    }
}

/// Rasterises a closed path built by `f` and takes it at the greater of the
/// current coverage and the shape's -- the anti-aliased primitives compose
/// over the straight strokes.
fn aa(
    cell: Area,
    cov: &mut [u8],
    scratch: &mut Vec<f32>,
    temp: &mut Vec<u8>,
    f: impl FnOnce(&mut Pen<'_, '_>),
) -> Option<()> {
    let extent = Extent::new(cell.w, cell.h);
    let cells = extent.cells()?;
    let need = extent.scratch()?;
    if scratch.len() < need {
        scratch.resize(need, 0.0);
    }
    if temp.len() < cells {
        temp.resize(cells, 0);
    }
    let mut canvas = Canvas::new(extent, scratch)?;
    let mut pen = Pen {
        canvas: &mut canvas,
    };
    f(&mut pen);
    canvas.coverage(&mut temp[..cells])?;
    for (c, &t) in cov.iter_mut().zip(temp.iter()) {
        *c = (*c).max(t);
    }
    Some(())
}

/// The straight-stroke glyphs: every arm runs from the cell's edge to the
/// far side of the strokes it meets, so a corner or a tee is the union of
/// its arms with the junction square filled.
fn draw_arms(mask: &mut Mask<'_>, cell: Area, a: Arms) {
    let (w, h) = (cell.w, cell.h);
    let vspan = |wt: Weight| cell.vertical(cell.width(wt));
    let hspan = |wt: Weight| Cell::span(h, cell.width(wt));
    let xs = [a.up, a.down]
        .into_iter()
        .flatten()
        .map(vspan)
        .reduce(Range::union);
    let ys = [a.left, a.right]
        .into_iter()
        .flatten()
        .map(hspan)
        .reduce(Range::union);
    let zero = Px::of(0);
    if let Some(wt) = a.up {
        let stop = ys.unwrap_or_else(|| hspan(wt)).end;
        mask.rect_px(vspan(wt), Range::to(zero, stop));
    }
    if let Some(wt) = a.down {
        let start = ys.unwrap_or_else(|| hspan(wt)).start;
        mask.rect_px(vspan(wt), Range::to(start, h));
    }
    if let Some(wt) = a.left {
        let stop = xs.unwrap_or_else(|| vspan(wt)).end;
        mask.rect_px(Range::to(zero, stop), hspan(wt));
    }
    if let Some(wt) = a.right {
        let start = xs.unwrap_or_else(|| vspan(wt)).start;
        mask.rect_px(Range::to(start, w), hspan(wt));
    }
}

/// The double-line glyphs, from their segment lists.
fn draw_double(mask: &mut Mask<'_>, cell: Area, segs: &[Seg]) {
    let (w, h) = (cell.w, cell.h);
    let t = cell.light().value();
    let resolve = |at: At, len: Px| -> Px {
        let l = len.value();
        let pair = Cell::span(len, Px::of(t * 3)).start.value();
        let single = Cell::span(len, Px::of(t)).start.value();
        Px::of(match at {
            At::Edge0 => 0,
            At::Edge1 => l,
            At::A0 => pair,
            At::A1 => pair + t,
            At::B0 => pair + 2 * t,
            At::B1 => pair + 3 * t,
            At::S0 => single,
            At::S1 => single + t,
        })
    };
    // Across, a pair or a single stroke is placed in the cell the
    // verticals are in and the edges are the whole box's.
    let across = |at: At| match at {
        At::Edge0 => Px::of(0),
        At::Edge1 => w,
        _ => Px::of(resolve(at, cell.c).value() + cell.x.value()),
    };
    for seg in segs {
        let x = Range::to(across(seg.x0), across(seg.x1));
        let y = Range::to(resolve(seg.y0, h), resolve(seg.y1, h));
        mask.rect_px(x, y);
    }
}

/// A dashed line: `n` dashes of six tenths of their period, the gaps
/// halved at the cell's edges so the pattern tiles across neighbours.
fn draw_dashed(mask: &mut Mask<'_>, cell: Area, axis: Axis, wt: Weight, dashes: Dashes) {
    let (w, h) = (cell.w, cell.h);
    let n = dashes.count();
    match axis {
        Axis::Horizontal => {
            let band = Cell::span(h, cell.width(wt));
            let (y0, y1) = (fx(band.start), fx(band.end));
            let period = fx(w) / n;
            let dash = (period * 0.6).max(1.0);
            let mut x = (period - dash) / 2.0;
            while x < fx(w) {
                mask.rect(x, y0, (x + dash).min(fx(w)), y1);
                x += period;
            }
        }
        Axis::Vertical => {
            let band = cell.vertical(cell.width(wt));
            let (x0, x1) = (fx(band.start), fx(band.end));
            let period = fx(h) / n;
            let dash = (period * 0.6).max(1.0);
            let mut y = (period - dash) / 2.0;
            while y < fx(h) {
                mask.rect(x0, y, x1, (y + dash).min(fx(h)));
                y += period;
            }
        }
    }
}

/// A block element: eighths of the cell, the edges rounded to whole pixels.
fn draw_block(mask: &mut Mask<'_>, cell: Area, e: Eighths) {
    let (wf, hf) = (fx(cell.w), fx(cell.h));
    let x0 = wf * f32::from(e.0) / 8.0;
    let y0 = hf * f32::from(e.1) / 8.0;
    let x1 = wf * f32::from(e.2) / 8.0;
    let y1 = hf * f32::from(e.3) / 8.0;
    mask.rect(x0, y0, x1, y1);
}

fn draw_quadrants(mask: &mut Mask<'_>, cell: Area, q: Quadrants) {
    let (wf, hf) = (fx(cell.w), fx(cell.h));
    let (mx, my) = (wf / 2.0, hf / 2.0);
    if q.0 & Quadrants::UL != 0 {
        mask.rect(0.0, 0.0, mx, my);
    }
    if q.0 & Quadrants::UR != 0 {
        mask.rect(mx, 0.0, wf, my);
    }
    if q.0 & Quadrants::LL != 0 {
        mask.rect(0.0, my, mx, hf);
    }
    if q.0 & Quadrants::LR != 0 {
        mask.rect(mx, my, wf, hf);
    }
}

/// A rounded corner: two light legs to the cell's edges and a quarter
/// annulus joining them, whose radius is what the cell allows. When the
/// cell is too small for an arc it degrades to a square corner.
fn draw_arc(mask: &mut Mask<'_>, scratch: &mut Vec<f32>, temp: &mut Vec<u8>, cell: Area, arc: Arc) {
    use core::f32::consts::PI;
    let t = cell.light();
    let vx = cell.vertical(t).start;
    let hy = Cell::span(cell.h, t).start;
    let (tf, wf, hf) = (fx(t), fx(cell.w), fx(cell.h));
    let cx = fx(vx) + tf / 2.0;
    let cy = fx(hy) + tf / 2.0;
    let (left, right) = (fx(cell.x), fx(cell.x) + fx(cell.c));
    let room = ((cx - left).min(right - cx)).min(cy.min(hf - cy));
    let r = room - tf / 2.0;
    if r < tf {
        draw_arms(
            mask,
            cell,
            Arms {
                up: (!arc.down).then_some(Weight::Light),
                down: arc.down.then_some(Weight::Light),
                left: (!arc.right).then_some(Weight::Light),
                right: arc.right.then_some(Weight::Light),
            },
        );
        return;
    }
    let sx = if arc.right { 1.0 } else { -1.0 };
    let sy = if arc.down { 1.0 } else { -1.0 };
    let c = Point::new(cx + sx * r, cy + sy * r);
    let vx0 = fx(vx);
    let vx1 = fx(Px::of(vx.value() + t.value()));
    let hy0 = fx(hy);
    let hy1 = fx(Px::of(hy.value() + t.value()));
    // The straight legs, filled to whole pixels.
    if arc.down {
        mask.rect(vx0, c.y, vx1, hf);
    } else {
        mask.rect(vx0, 0.0, vx1, c.y);
    }
    if arc.right {
        mask.rect(c.x, hy0, wf, hy1);
    } else {
        mask.rect(0.0, hy0, c.x, hy1);
    }
    // The annulus sector between the two tangent points, anti-aliased.
    let (ro, ri) = (r + tf / 2.0, r - tf / 2.0);
    let a_vert = if arc.right { PI } else { 0.0 };
    let a_horz = if arc.down { -PI / 2.0 } else { PI / 2.0 };
    let clockwise = (arc.down && arc.right) || (!arc.down && !arc.right);
    let (a0, mut a1) = if clockwise {
        (a_vert, a_horz)
    } else {
        (a_horz, a_vert)
    };
    if (a1 - a0) > PI {
        a1 -= 2.0 * PI;
    } else if (a1 - a0) < -PI {
        a1 += 2.0 * PI;
    }
    let _ = aa(cell, mask.cov, scratch, temp, |pen| {
        let (s0, c0) = a0.sin_cos();
        let (s1, c1) = a1.sin_cos();
        pen.arc(c, ro, a0, a1);
        pen.canvas.line(
            Point::new(c.x + ro * c1, c.y + ro * s1),
            Point::new(c.x + ri * c1, c.y + ri * s1),
        );
        pen.arc(c, ri, a1, a0);
        pen.canvas.line(
            Point::new(c.x + ri * c0, c.y + ri * s0),
            Point::new(c.x + ro * c0, c.y + ro * s0),
        );
    });
}

/// A diagonal stroke from corner to corner, a light stroke wide, clipped to
/// the cell.
fn draw_diagonal(
    cell: Area,
    cov: &mut [u8],
    scratch: &mut Vec<f32>,
    temp: &mut Vec<u8>,
    d: Diagonal,
) {
    let (wf, hf) = (fx(cell.w), fx(cell.h));
    let half = fx(cell.light()) / 2.0;
    let _ = aa(cell, cov, scratch, temp, |pen| {
        let mut stroke = |p: Point, q: Point| {
            let (dx, dy) = (q.x - p.x, q.y - p.y);
            let len = (dx * dx + dy * dy).sqrt().max(1.0);
            let (nx, ny) = (-dy / len * half, dx / len * half);
            let (ex, ey) = (dx / len * half, dy / len * half);
            let band = [
                Point::new(p.x + nx - ex, p.y + ny - ey),
                Point::new(q.x + nx + ex, q.y + ny + ey),
                Point::new(q.x - nx + ex, q.y - ny + ey),
                Point::new(p.x - nx - ex, p.y - ny - ey),
            ];
            pen.poly(&clip(&band, wf, hf));
        };
        if d.falling {
            stroke(Point::new(wf, 0.0), Point::new(0.0, hf));
        }
        if d.rising {
            stroke(Point::new(0.0, 0.0), Point::new(wf, hf));
        }
    });
}

/// Sutherland-Hodgman against `[0, w] x [0, h]`.
fn clip(poly: &[Point], w: f32, h: f32) -> Vec<Point> {
    let mut out: Vec<Point> = poly.to_vec();
    type Inside = fn(Point, f32, f32) -> bool;
    type Cross = fn(Point, Point, f32, f32) -> Point;
    let edges: [(Inside, Cross); 4] = [
        (|p, _, _| p.x >= 0.0, |p, q, _, _| at_x(p, q, 0.0)),
        (|p, w, _| p.x <= w, |p, q, w, _| at_x(p, q, w)),
        (|p, _, _| p.y >= 0.0, |p, q, _, _| at_y(p, q, 0.0)),
        (|p, _, h| p.y <= h, |p, q, _, h| at_y(p, q, h)),
    ];
    for (inside, cross) in edges {
        let input = core::mem::take(&mut out);
        if input.is_empty() {
            return out;
        }
        let mut prev = input[input.len() - 1];
        for cur in input {
            let (ci, pi) = (inside(cur, w, h), inside(prev, w, h));
            if ci {
                if !pi {
                    out.push(cross(prev, cur, w, h));
                }
                out.push(cur);
            } else if pi {
                out.push(cross(prev, cur, w, h));
            }
            prev = cur;
        }
    }
    out
}

fn at_x(p: Point, q: Point, x: f32) -> Point {
    let t = if (q.x - p.x).abs() < f32::EPSILON {
        0.0
    } else {
        (x - p.x) / (q.x - p.x)
    };
    Point::new(x, p.y + t * (q.y - p.y))
}

fn at_y(p: Point, q: Point, y: f32) -> Point {
    let t = if (q.y - p.y).abs() < f32::EPSILON {
        0.0
    } else {
        (y - p.y) / (q.y - p.y)
    };
    Point::new(p.x + t * (q.x - p.x), y)
}

/// Braille: two columns of four dots, each an anti-aliased disc.
fn draw_braille(
    cell: Area,
    cov: &mut [u8],
    scratch: &mut Vec<f32>,
    temp: &mut Vec<u8>,
    dots: Dots,
) {
    let (wf, hf) = (fx(cell.w), fx(cell.h));
    let (px, py) = (wf / 2.0, hf / 4.0);
    let r = (px.min(py) * 0.28).max(0.75);
    const AT: [(u8, u8); 8] = [
        (0, 0),
        (0, 1),
        (0, 2),
        (1, 0),
        (1, 1),
        (1, 2),
        (0, 3),
        (1, 3),
    ];
    let _ = aa(cell, cov, scratch, temp, |pen| {
        for (bit, (col, row)) in AT.iter().enumerate() {
            if dots.0 & (1 << bit) == 0 {
                continue;
            }
            let c = Point::new((f32::from(*col) + 0.5) * px, (f32::from(*row) + 0.5) * py);
            pen.disc(c, r);
        }
    });
}

/// The coverage a shade paints on every pixel.
pub fn shade_coverage(s: Shade) -> Coverage {
    Coverage(match s {
        Shade::Light => 0x40,
        Shade::Medium => 0x80,
        Shade::Dark => 0xc0,
    })
}

/// Rasterises `cp` into a mask of exactly `cell`, row-major, into the first
/// `w * h` bytes of `coverage` -- `scratch` is the accumulator the
/// anti-aliased shapes need and both are grown as required. `None` for a
/// codepoint that is not procedural or a cell too large for a mask.
pub fn draw(
    cp: Codepoint,
    cell: Cell,
    scratch: &mut Vec<f32>,
    coverage: &mut Vec<u8>,
) -> Option<Extent> {
    draw_in(
        cp,
        Area::across(cell, crate::cellmap::CellSpan::One, Facing::Written),
        scratch,
        coverage,
    )
}

/// [`draw`] for a character that takes `span` cells: the mask the width
/// of all of them, its strokes at `cell`'s weight ([`Area`]), facing as
/// `facing` says -- mirrored, a box drawing character is its counterpart
/// with its verticals in its last cell ([`Facing`]). One cell as written
/// is [`draw`].
pub fn draw_across(
    cp: Codepoint,
    cell: Cell,
    span: crate::cellmap::CellSpan,
    facing: Facing,
    scratch: &mut Vec<f32>,
    coverage: &mut Vec<u8>,
) -> Option<Extent> {
    let drawn = match facing {
        Facing::Written => cp,
        Facing::Mirrored => char::from_u32(cp.value())
            .and_then(baud_domain::box_counterpart)
            .map_or(cp, Codepoint::from_char),
    };
    draw_in(drawn, Area::across(cell, span, facing), scratch, coverage)
}

fn draw_in(
    cp: Codepoint,
    cell: Area,
    scratch: &mut Vec<f32>,
    coverage: &mut Vec<u8>,
) -> Option<Extent> {
    let shape = shape(cp)?;
    let extent = Extent::new(cell.w, cell.h);
    let cells = extent.cells()?;
    if coverage.len() < cells {
        coverage.resize(cells, 0);
    }
    coverage[..cells].fill(0);
    if let Shape::Shade(s) = shape {
        coverage[..cells].fill(shade_coverage(s).0);
        return Some(extent);
    }
    // The anti-aliased primitives need a scratch coverage of their own to
    // OR into the straight strokes; kept in `scratch`'s byte sibling.
    let mut temp: Vec<u8> = Vec::new();
    {
        let (cov, _) = coverage.split_at_mut(cells);
        let mut mask = Mask {
            cov,
            w: cell.w,
            h: cell.h,
        };
        match shape {
            Shape::Arms(a) => draw_arms(&mut mask, cell, a),
            Shape::Dashed(axis, wt, n) => draw_dashed(&mut mask, cell, axis, wt, n),
            Shape::Double(_) => {
                let i = usize::try_from(cp.value().saturating_sub(0x2550)).unwrap_or(usize::MAX);
                if let Some(segs) = DOUBLE.get(i) {
                    draw_double(&mut mask, cell, segs);
                }
            }
            Shape::Block(e) => draw_block(&mut mask, cell, e),
            Shape::Quadrants(q) => draw_quadrants(&mut mask, cell, q),
            Shape::Arc(a) => draw_arc(&mut mask, scratch, &mut temp, cell, a),
            Shape::Diagonal(_) | Shape::Braille(_) | Shape::Shade(_) => {}
        }
    }
    match shape {
        Shape::Diagonal(d) => draw_diagonal(cell, &mut coverage[..cells], scratch, &mut temp, d),
        Shape::Braille(d) => draw_braille(cell, &mut coverage[..cells], scratch, &mut temp, d),
        _ => {}
    }
    Some(extent)
}

/// Which part of an encircled run's outline a cell draws: `SGR 52`,
/// "encircled" (`[[source: ecma48]]` § 8.3.117), one enclosure a run as
/// `SGR 51`'s frame is one box a run. A cell between the ends draws the
/// top and bottom lines the frame draws; the end cells draw a side at the
/// run's edge between two quarter circles half a cell wide, which meet
/// those lines: a lone cell is a stadium; a run of two cells, a wide
/// character among them, is a rectangle with rounded corners of radius
/// half a cell's width. The curves keep to the corners, where the end
/// glyph has less ink than the half ellipse crossed. A stroke at the
/// cell's edge still touches ink at small sizes: 64 of 94 glyphs at
/// 13 px on a lone cell.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ring {
    /// The left end: a side at the cell's left edge between two rounded
    /// corners, open at the right edge.
    Starts,
    /// The right end: the left end mirrored, open at the left edge.
    Ends,
    /// Both ends in one cell: a stadium, its sides at the cell's edges
    /// and its top and bottom round.
    Alone,
}

impl Ring {
    /// Where the ring's masks sit among the procedural keys, above every
    /// codepoint, the face style and the span: `RING_KEY` in the cell
    /// shader, which probes the glyph map for the same keys.
    pub const KEY: u32 = 1 << 27;

    /// The glyph map's key for this part: `RING_KEY + 1`, `+ 2`, `+ 3`.
    pub fn key(self) -> crate::CellKey {
        crate::CellKey(
            Ring::KEY
                + match self {
                    Ring::Starts => 1,
                    Ring::Ends => 2,
                    Ring::Alone => 3,
                },
        )
    }
}

/// A whole number of quarter turns round from the right, clockwise on
/// screen (y down): 0 the right, 1 the bottom, 2 the left, 3 or -1 the
/// top.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Quarter(i32);

impl Quarter {
    /// The unit point at this turn: exact, so a point on an axis lies on
    /// it and no edge of a ring strays a fraction of a pixel outside the
    /// cell, where the accumulation would lose it.
    fn unit(self) -> (f32, f32) {
        match self.0.rem_euclid(4) {
            0 => (1.0, 0.0),
            1 => (0.0, 1.0),
            2 => (-1.0, 0.0),
            _ => (0.0, -1.0),
        }
    }
}

/// A quarter of an ellipse about `c` with semi-axes `rx`, `ry`, from turn
/// `from` to the adjacent turn `to`, as one cubic: the circle's
/// [`Pen::arc`] with each axis scaled by its own radius. Every control
/// point lies inside the ellipse's box, so the curve does too.
fn quarter(canvas: &mut Canvas<'_>, c: Point, rx: f32, ry: f32, from: Quarter, to: Quarter) {
    const KAPPA: f32 = 0.552_284_8;
    let (c0, s0) = from.unit();
    let (c1, s1) = to.unit();
    let sign = if to.0 > from.0 { 1.0 } else { -1.0 };
    let p0 = Point::new(c.x + rx * c0, c.y + ry * s0);
    let p3 = Point::new(c.x + rx * c1, c.y + ry * s1);
    let p1 = Point::new(p0.x - sign * KAPPA * rx * s0, p0.y + sign * KAPPA * ry * c0);
    let p2 = Point::new(p3.x + sign * KAPPA * rx * s1, p3.y - sign * KAPPA * ry * c1);
    canvas.cubic(p0, p1, p2, p3);
}

/// Rasterises `ring`'s part of an encircled run's outline into a mask of
/// exactly `cell`, a stroke `th` pixels wide, into the first `w * h`
/// bytes of `coverage`, growing both buffers as required.
///
/// The outer edge of the stroke runs along the cell's top and bottom and,
/// at an end, its outer side, its corners quarter circles of radius half
/// the cell's width; the inner edge is that shape `th` inside it, its
/// corners' radius less `th`. At an end cell's open edge the
/// stroke is the rows `0..th` and `h - th..h`, which are the rows of the
/// top and bottom lines the cells between the ends draw, so the outline
/// is continuous across the run. The right end is the left end mirrored,
/// pixel for pixel. `None` for a cell too small for a mask.
pub fn draw_ring(
    ring: Ring,
    cell: Cell,
    th: Px,
    scratch: &mut Vec<f32>,
    coverage: &mut Vec<u8>,
) -> Option<Extent> {
    let extent = Extent::new(cell.w, cell.h);
    let cells = extent.cells()?;
    let w = slots(cell.w);
    if coverage.len() < cells {
        coverage.resize(cells, 0);
    }
    coverage[..cells].fill(0);
    let need = extent.scratch()?;
    if scratch.len() < need {
        scratch.resize(need, 0.0);
    }
    let (wf, hf) = (fx(cell.w), fx(cell.h));
    let t = fx(Px::of(th.value().max(1)));
    // The corners' radius: half the cell's width, so a cell alone is a
    // stadium and an end's curve stays in the corner, where the end
    // glyph has less ink; the inner edge's, less the stroke.
    let r = (wf / 2.0).min(hf / 2.0);
    let ri = (r - t).max(0.0);
    let mut canvas = Canvas::new(extent, scratch)?;
    // The outer edge one way round and the inner one back, so the
    // accumulation leaves the inside of the stroke at zero. Quarter turn
    // 3 is the top, 2 the left, 1 the bottom, 0 the right.
    let (tl, bl) = (Point::new(r, r), Point::new(r, hf - r));
    match ring {
        Ring::Alone => {
            let (tr, br) = (Point::new(wf - r, r), Point::new(wf - r, hf - r));
            canvas.line(Point::new(wf - r, 0.0), Point::new(r, 0.0));
            quarter(&mut canvas, tl, r, r, Quarter(3), Quarter(2));
            canvas.line(Point::new(0.0, r), Point::new(0.0, hf - r));
            quarter(&mut canvas, bl, r, r, Quarter(2), Quarter(1));
            canvas.line(Point::new(r, hf), Point::new(wf - r, hf));
            quarter(&mut canvas, br, r, r, Quarter(1), Quarter(0));
            canvas.line(Point::new(wf, hf - r), Point::new(wf, r));
            quarter(&mut canvas, tr, r, r, Quarter(0), Quarter(-1));
            canvas.line(Point::new(wf - r, t), Point::new(r, t));
            quarter(&mut canvas, tr, ri, ri, Quarter(3), Quarter(4));
            canvas.line(Point::new(wf - t, r), Point::new(wf - t, hf - r));
            quarter(&mut canvas, br, ri, ri, Quarter(4), Quarter(5));
            canvas.line(Point::new(wf - r, hf - t), Point::new(r, hf - t));
            quarter(&mut canvas, bl, ri, ri, Quarter(5), Quarter(6));
            canvas.line(Point::new(t, hf - r), Point::new(t, r));
            quarter(&mut canvas, tl, ri, ri, Quarter(6), Quarter(7));
            canvas.line(Point::new(r, t), Point::new(wf - r, t));
        }
        Ring::Starts | Ring::Ends => {
            // The left end, open at its right edge, where the lines between
            // the ends go on: along the top, round the corner, down the
            // side, round the corner and along the bottom on the outer
            // edge, and back on the inner one.
            canvas.line(Point::new(wf, 0.0), Point::new(r, 0.0));
            quarter(&mut canvas, tl, r, r, Quarter(3), Quarter(2));
            canvas.line(Point::new(0.0, r), Point::new(0.0, hf - r));
            quarter(&mut canvas, bl, r, r, Quarter(2), Quarter(1));
            canvas.line(Point::new(r, hf), Point::new(wf, hf));
            canvas.line(Point::new(wf, hf), Point::new(wf, hf - t));
            canvas.line(Point::new(wf, hf - t), Point::new(r, hf - t));
            quarter(&mut canvas, bl, ri, ri, Quarter(1), Quarter(2));
            canvas.line(Point::new(t, hf - r), Point::new(t, r));
            quarter(&mut canvas, tl, ri, ri, Quarter(2), Quarter(3));
            canvas.line(Point::new(r, t), Point::new(wf, t));
            canvas.line(Point::new(wf, t), Point::new(wf, 0.0));
        }
    }
    canvas.coverage(&mut coverage[..cells])?;
    if ring == Ring::Ends {
        for row in coverage[..cells].chunks_mut(w.max(1)) {
            row.reverse();
        }
    }
    Some(extent)
}

/// The double-line glyph's segments, for the tests that check them against
/// the names: `None` for a codepoint outside `U+2550..=U+256C`.
pub fn double_arms(cp: Codepoint) -> Option<Arms> {
    match shape(cp)? {
        Shape::Double(a) => Some(a),
        _ => None,
    }
}
