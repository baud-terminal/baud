//! The cursor trail: a fading ghost of the cursor sweeping from the cell
//! it left to the cell it reached, over a few frames. Off by default; the
//! feature sfte ships as `SFTE_CURSOR_TRAIL`, built to baud's blink
//! standard instead of sfte's full-wipe -- the geometry is pure and the
//! one translucent solid every path draws, the scrollbar's shape one
//! module over.
//!
//! The driver hands the renderer a [`Trail`] whose `progress` it computed
//! from the display link's timestamp -- the way [`crate::Blink`] is driven
//! -- and the renderer only interpolates, so a frame is a pure function of
//! the value it was handed and the pixel oracle can compare two GPU arms
//! on it. Sampling a clock inside the render path would make a golden frame
//! non-deterministic; the phase is a parameter, never a clock.

use crate::atlas::{Kind, Tex};
use crate::color::Argb;
use crate::det::Det;
use crate::effect::Vfx;
use crate::flash::Strength;
use crate::instance::{Instance, Screen};
use baud_domain::{Chan, Col, Row};
use baud_font::Px;

/// A cursor move to animate: the cell the cursor left, the cell it
/// reached, and how far along the sweep the driver has advanced it -- 0.0
/// at the start, 1.0 when the ghost lands on the new cell.
///
/// `origin_dy` is how far the cell it left has ridden a smooth scroll,
/// pixels, y down, and `lift` the frame's slide pan: both move the drawn
/// ghost with the text (`TrailLook::lifted`, the driver's `ride_ghost`). No
/// `Default`: every site says both, zero where nothing rides. `strength` is
/// how the driver weighed the sweep (note AL, option O2): a repeat over a
/// stretch of screen a full pass swept is drawn under the flash thresholds.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Trail {
    pub from: (Row, Col),
    pub to: (Row, Col),
    pub progress: f32,
    pub origin_dy: f32,
    pub lift: Px,
    pub strength: Strength,
}

/// The ghost's rectangle in pixels of the frame and the alpha it draws at
/// -- a cursor-cell-sized box at the interpolated position, fading to
/// nothing as it lands.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Ghost {
    pub left: Px,
    pub top: Px,
    pub width: Px,
    pub height: Px,
    pub alpha: Chan,
    /// The sweep's [`Trail::strength`].
    pub strength: Strength,
}

impl Ghost {
    /// Whether the ghost touches any pixel row of a band, top inclusive,
    /// bottom exclusive -- the scrollbar's `overlaps`, so a row is dirtied
    /// only while the ghost is over it and the damage stays bounded.
    pub fn overlaps(&self, top: Px, bottom: Px) -> bool {
        self.top.value() < bottom.value() && self.top.value() + self.height.value() > top.value()
    }
}

/// The alpha the ghost starts at, before the fade.
const BASE_ALPHA: u8 = 0xB0;

/// Round a float pixel to a `Px`, the `boxes` idiom -- `as i64` is not a
/// counted cast, so this is the one float-to-pixel conversion the audit
/// permits.
fn round_px(v: f32) -> Px {
    Px::of(u32::try_from(v.round().max(0.0) as i64).unwrap_or(0))
}

/// The ghost for `trail` on cells `cell_w` by `cell_h`: a cursor-sized box
/// linearly interpolated from the cell it left to the cell it reached, at
/// an alpha that fades to zero as it lands.
pub fn ghost(trail: Trail, cell_w: Px, cell_h: Px) -> Ghost {
    let p = trail.progress.clamp(0.0, 1.0);
    let col_px = |c: Col| {
        u32::try_from(c.index())
            .unwrap_or(0)
            .saturating_mul(cell_w.value())
    };
    let row_px = |r: Row| {
        u32::try_from(r.index())
            .unwrap_or(0)
            .saturating_mul(cell_h.value())
    };
    let lerp = |a: f32, b: f32| a + (b - a) * p;
    let left = round_px(lerp(col_px(trail.from.1) as f32, col_px(trail.to.1) as f32));
    // The cell it left moved with a scroll it rode, and the whole ghost with
    // the frame's slide; a ghost partly above the window draws clamped at
    // its top, for the tenth of a second it lives.
    let top = round_px(
        lerp(
            row_px(trail.from.0) as f32 + trail.origin_dy,
            row_px(trail.to.0) as f32,
        ) + trail.lift.value() as f32,
    );
    let fade = (f32::from(BASE_ALPHA) * (1.0 - p)).round().max(0.0) as i64;
    Ghost {
        left,
        top,
        width: cell_w,
        height: cell_h,
        alpha: Chan::from_u8(u8::try_from(fade).unwrap_or(0)),
        strength: trail.strength,
    }
}

/// The ghost as the one translucent solid every path draws, in `fill` -- the
/// cursor's colour, as every trail is -- at its faded alpha.
pub fn instance(g: Ghost, fill: Argb) -> Instance {
    Instance {
        x: Screen(i16::try_from(g.left.value()).unwrap_or(i16::MAX)),
        y: Screen(i16::try_from(g.top.value()).unwrap_or(i16::MAX)),
        w: Tex(u16::try_from(g.width.value()).unwrap_or(u16::MAX)),
        h: Tex(u16::try_from(g.height.value()).unwrap_or(u16::MAX)),
        u: Tex(0),
        v: Tex(0),
        color: fill.with_alpha(g.alpha),
        kind: Kind::Solid,
        curve: baud_domain::contrast::CurveRow::default(),
    }
}

// ---- The quad: four corners chasing the cursor's at per-corner rates, the
// effect kitty draws, written from the derivation in note W, section 8.

/// A point in frame pixels, y down.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Pt {
    pub x: f32,
    pub y: f32,
}

/// A rectangle in frame pixels, y down: `left <= right`, `top <= bottom`.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Rect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl Rect {
    /// The rectangle's corners, clockwise from the top right.
    pub fn corners(self) -> Corners {
        Corners([
            Pt {
                x: self.right,
                y: self.top,
            },
            Pt {
                x: self.right,
                y: self.bottom,
            },
            Pt {
                x: self.left,
                y: self.bottom,
            },
            Pt {
                x: self.left,
                y: self.top,
            },
        ])
    }

    pub(crate) fn center(self) -> Pt {
        Pt {
            x: (self.left + self.right) * 0.5,
            y: (self.top + self.bottom) * 0.5,
        }
    }

    /// Whether `p` is inside, left and top edges included.
    pub fn contains(self, p: Pt) -> bool {
        p.x >= self.left && p.x < self.right && p.y >= self.top && p.y < self.bottom
    }
}

/// A quadrilateral's corners clockwise from the top right -- top-right,
/// bottom-right, bottom-left, top-left, the order the shaders' uniforms
/// read -- each chasing the same corner of the cursor.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Corners(pub [Pt; 4]);

/// sfte's trail constant as a decay time: its tail closes `0.01` of the gap
/// a millisecond (`SFTE_CURSOR_TRAIL_DECAY`, `[[source: sfte]]` 13906,
/// stepped at 38133), an e-fold of 100 ms, and `2^(-10 t / ln 2)` is
/// `e^(-10 t)`. sfte steps it once a frame, so at 120 Hz its tail closes a
/// little faster than the rate it names; this is the rate.
pub const TAU_SWEEP: f32 = core::f32::consts::LN_2;

/// The decay time of the corner leading the move, seconds, and of the one
/// trailing it: the time each takes to close all but `2^-10` of its gap,
/// so it halves the gap every tenth of it. The numbers are kitty's
/// defaults, `cursor_trail_decay 0.1 0.4`
/// (the option at `[[source: kitty-options-definition-v0492]]` 467-469).
pub const TAU_FAST: f32 = 0.1;
pub const TAU_SLOW: f32 = 0.4;

/// A corner closer than this to its target on both axes has arrived,
/// pixels: the trail is over when every corner has, as kitty's is.
const ARRIVED: f32 = 0.5;

/// **One move of the quad**: the corners it starts from, the rectangle
/// they chase, and each corner's decay time, frozen when the move starts.
/// A corner closes its gap exponentially, so at `t` seconds it is at
/// `T + (P - T) * 2^(-10 t / tau)`: a frame is a function of the time
/// alone and never of how many frames came before it.
///
/// kitty re-picks the rates every frame, in its normalised device
/// coordinates, where a pixel is taller than it is wide by the window's
/// aspect ratio. Frozen, in pixels, the horizontal and vertical moves draw
/// as kitty's do and a diagonal differs by the aspect ratio's say, which is
/// a property of kitty's window rather than of the move.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Segment {
    pub from: Corners,
    pub to: Rect,
    pub tau: [f32; 4],
}

/// **How much a corner leads its move**: the cosine of the angle between
/// the way it travels, from `at` to `to`, and the way it points out of the
/// rectangle, from `centre` to `to` -- 1 for a corner travelling straight
/// out past its own side of the cursor, -1 for one travelling straight in.
/// `None` for a corner that does not travel, which points nowhere.
fn lead(at: Pt, to: Pt, centre: Pt) -> Option<f32> {
    let (dx, dy) = (to.x - at.x, to.y - at.y);
    let (ox, oy) = (to.x - centre.x, to.y - centre.y);
    let (travel, out) = (dx.det_hypot(dy), ox.det_hypot(oy));
    (travel >= 1e-3 && out > 0.0).then_some((dx * ox + dy * oy) / (travel * out))
}

impl Segment {
    /// A move from `from` to the corners of `to`, each corner's rate by how
    /// much it leads (`lead`): the corner leading most takes
    /// [`TAU_FAST`], the one leading least [`TAU_SLOW`], and the others the
    /// two interpolated by where their lead falls between. A corner that
    /// does not travel has no direction and takes no part; with no spread
    /// between the rest, none leads, and every corner trails.
    pub fn new(from: Corners, to: Rect) -> Segment {
        let target = to.corners().0;
        let centre = to.center();
        let leads: [Option<f32>; 4] = core::array::from_fn(|i| lead(from.0[i], target[i], centre));
        let (lo, hi) = leads
            .iter()
            .flatten()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &l| {
                (lo.min(l), hi.max(l))
            });
        let tau = leads.map(|l| match l {
            Some(l) if hi - lo > f32::EPSILON => {
                let s = (l - lo) / (hi - lo);
                TAU_FAST * s + TAU_SLOW * (1.0 - s)
            }
            _ => TAU_SLOW,
        });
        Segment { from, to, tau }
    }

    /// **A short move in flight**: every corner on the fast constant, so a
    /// keystroke's one-cell step after a jump folds the trail's tail in, in
    /// about a tenth of a second, rather than carrying it along for the
    /// slow constant, which is longer than the time between keystrokes and
    /// would keep the trail -- and a frame every tick -- alive while the
    /// user types. Neovide's rule: a move of at most two columns and no row
    /// animates every corner over its short length
    /// (`[[source: neovide-cursor-renderer]]` 158-177); baud takes kitty's
    /// default for "short", two cells on each axis, and snaps such a move
    /// from rest.
    pub fn quick(from: Corners, to: Rect) -> Segment {
        Segment {
            from,
            to,
            tau: [TAU_FAST; 4],
        }
    }

    /// **sfte's move**: one constant for every corner, so the tail is the
    /// cursor's rectangle translated, closing on it with sfte's decay.
    pub fn sweep(from: Corners, to: Rect) -> Segment {
        Segment {
            from,
            to,
            tau: [TAU_SWEEP; 4],
        }
    }

    /// The corners `t` seconds into the move.
    pub fn at(&self, t: f32) -> Corners {
        let target = self.to.corners().0;
        let t = t.max(0.0);
        let mut out = self.from;
        for (i, p) in out.0.iter_mut().enumerate() {
            let k = (-10.0 * t / self.tau[i]).det_exp2();
            p.x = target[i].x + (p.x - target[i].x) * k;
            p.y = target[i].y + (p.y - target[i].y) * k;
        }
        out
    }

    /// How long the move lasts, seconds: until the last corner is within
    /// half a pixel of its target on both axes, solved for the time: a gap
    /// `g` halves every `tau / 10`, so it is under [`ARRIVED`] after
    /// `tau / 10 * log2(g / ARRIVED)`.
    pub fn lasts(&self) -> f32 {
        let target = self.to.corners().0;
        let mut end = 0.0f32;
        for (i, p) in self.from.0.iter().enumerate() {
            let gap = (target[i].x - p.x).abs().max((target[i].y - p.y).abs());
            if gap > ARRIVED {
                end = end.max(self.tau[i] / 10.0 * (gap / ARRIVED).det_log2());
            }
        }
        end
    }
}

/// Which trail a smear draws: kitty's quad, the four corners filled, or
/// sfte's sweep, the box swept from the tail's rectangle -- the corners --
/// to the cursor's, at an alpha rising from none at the tail to half at the
/// cursor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    Quad,
    Sweep,
}

/// **What the renderer draws for a quad or a sweep**: the corners this frame
/// and the cursor rectangle the trail leaves bare, so it never covers the
/// cursor it is arriving at (`[[source: kitty-trail-slang-v0492]]` 25-36; sfte's
/// sweep skips the same rectangle).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Smear {
    pub corners: Corners,
    pub hole: Rect,
    pub shape: Shape,
    /// As the trail draws it, or a repeat pass under the flash thresholds;
    /// an effect's is its emitter's.
    pub strength: crate::flash::Strength,
    /// **How much of the trail is drawn**: [`Smear::OPAQUE`], but while
    /// the trail of a cursor a program hid fades out, as kitty's does
    /// (`[[source: kitty-cursor-trail]]` 64, 134-139). Every record's alpha
    /// is scaled by it ([`crate::flash::capped`]), and the CPU path's quad
    /// fill with them, so a smear's pixel is drawn that share of the way
    /// from what is under it to the whole trail. An effect's is its
    /// emitter's, and scales each of its records.
    pub opacity: Chan,
}

impl Smear {
    /// The opacity of a trail drawn whole.
    pub const OPAQUE: Chan = Chan::from_offset(255, 0);

    /// The smear moved `dy` pixels down, rigidly: every corner and the hole.
    pub fn translated(self, dy: f32) -> Smear {
        Smear {
            corners: Corners(self.corners.0.map(|p| Pt {
                x: p.x,
                y: p.y + dy,
            })),
            hole: Rect {
                top: self.hole.top + dy,
                bottom: self.hole.bottom + dy,
                ..self.hole
            },
            ..self
        }
    }

    /// The pixels the quad can touch, in whole pixels of the frame: its
    /// corners' bounds, floored and ceiled. That already holds the
    /// anti-aliased edge -- coverage reaches half a pixel past an edge, so
    /// the last pixel whose centre it reaches is the one the edge is in.
    pub fn bounds(&self) -> (Px, Px, Px, Px) {
        let xs = self.corners.0.map(|p| p.x);
        let ys = self.corners.0.map(|p| p.y);
        let min = |v: [f32; 4]| v.iter().copied().fold(f32::INFINITY, f32::min);
        let max = |v: [f32; 4]| v.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let (mut x0, mut y0, mut x1, mut y1) = (min(xs), min(ys), max(xs), max(ys));
        // A sweep runs from the tail to the cursor, so it spans both.
        if self.shape == Shape::Sweep {
            x0 = x0.min(self.hole.left);
            y0 = y0.min(self.hole.top);
            x1 = x1.max(self.hole.right);
            y1 = y1.max(self.hole.bottom);
        }
        let floor = |v: f32| round_px(v.floor());
        let ceil = |v: f32| round_px(v.ceil());
        (floor(x0), floor(y0), ceil(x1), ceil(y1))
    }

    /// Whether the quad touches any pixel row of a band, top inclusive,
    /// bottom exclusive.
    pub fn overlaps(&self, top: Px, bottom: Px) -> bool {
        let (_, y0, _, y1) = self.bounds();
        y0.value() < bottom.value() && y1.value() > top.value()
    }

    /// **The columns the smear can cover on the pixel rows from `top` to
    /// `bottom`**, left inclusive and right exclusive, inside its
    /// [`Smear::bounds`]; none where it covers nothing. Every pixel
    /// [`coverage`] is above zero at is inside, so a row drawn only there is
    /// the row drawn over the bounds: a quad covers only within half a
    /// pixel of the region its winding fills, which is inside its corners'
    /// hull; a sweep only where its alpha is above zero, at a centre that
    /// projects past the tail's centre onto the move: up to the cursor's
    /// centre such a centre lies across the move from its projection,
    /// within the cursor's box of it, and past it in the cursor's box --
    /// inside the hull of the cursor's box and the chord across the move
    /// through the tail's centre, as wide as the box. A pixel whose centre is
    /// within `grow` of the hull -- half a pixel for a quad, none for a
    /// sweep -- has its nearest point in the rows' strip, `grow` past the
    /// centres, and its centre within `grow` of the strip's columns; a
    /// quarter pixel more on each, for the rounding. On a jump across the
    /// window the bounds are the window and the shape a strip a cell or two
    /// wide: what a CPU frame measures and repaints.
    pub(crate) fn columns(&self, top: Px, bottom: Px) -> Option<(Px, Px)> {
        let c = self.corners.0;
        let (points, n, grow) = match self.shape {
            Shape::Quad => ([c[0], c[1], c[2], c[3], c[0], c[0], c[0], c[0]], 4, 0.5),
            Shape::Sweep => {
                // `sweep_alpha`'s centres and half-extents.
                let (tl, br) = (c[3], c[1]);
                let c0 = Pt {
                    x: (tl.x + br.x) * 0.5,
                    y: (tl.y + br.y) * 0.5,
                };
                let h = self.hole;
                let (rx, ry) = ((h.right - h.left) * 0.5, (h.bottom - h.top) * 0.5);
                let c1 = Pt {
                    x: h.left + rx,
                    y: h.top + ry,
                };
                // A tail on the cursor draws nothing, as `sweep_alpha` says.
                let (ax, ay) = (c1.x - c0.x, c1.y - c0.y);
                if ax * ax + ay * ay < 0.001 {
                    return None;
                }
                let at = |p: Pt, sx: f32, sy: f32| Pt {
                    x: p.x + sx * rx,
                    y: p.y + sy * ry,
                };
                let corners = |p: Pt| {
                    [
                        at(p, -1.0, -1.0),
                        at(p, 1.0, -1.0),
                        at(p, 1.0, 1.0),
                        at(p, -1.0, 1.0),
                    ]
                };
                // The chord: from the tail's centre across the move, each way
                // to the box's edge.
                let l = (ax * ax + ay * ay).sqrt();
                let (ux, uy) = (-ay / l, ax / l);
                let s = (rx / ux.abs()).min(ry / uy.abs());
                let chord = |k: f32| Pt {
                    x: c0.x + k * s * ux,
                    y: c0.y + k * s * uy,
                };
                let [e, f, g, k] = corners(c1);
                ([chord(-1.0), chord(1.0), e, f, g, k, e, e], 6, 0.0)
            }
        };
        let hull = &points[..n];
        let row = |v: Px| f32::from(u16::try_from(v.value()).unwrap_or(u16::MAX));
        let reach = grow + 0.25;
        let (lo, hi) = (row(top) + 0.5 - reach, row(bottom) - 0.5 + reach);
        let (mut xmin, mut xmax) = (f32::INFINITY, f32::NEG_INFINITY);
        // The hull met with the strip: every segment between two of its
        // points clipped to it, a point by itself among them -- the hull's
        // edges are such segments, and the rest lie inside it.
        for (i, &a) in hull.iter().enumerate() {
            for &b in &hull[i..] {
                let (mut t0, mut t1) = (0.0f32, 1.0f32);
                let dy = b.y - a.y;
                if dy.abs() < 1e-6 {
                    if a.y < lo || a.y > hi {
                        continue;
                    }
                } else {
                    let (ta, tb) = ((lo - a.y) / dy, (hi - a.y) / dy);
                    t0 = t0.max(ta.min(tb));
                    t1 = t1.min(ta.max(tb));
                    if t0 > t1 {
                        continue;
                    }
                }
                for t in [t0, t1] {
                    let x = a.x + (b.x - a.x) * t;
                    xmin = xmin.min(x);
                    xmax = xmax.max(x);
                }
            }
        }
        if xmin > xmax {
            return None;
        }
        let (x0, _, x1, _) = self.bounds();
        // A centre at or past `xmin - reach` is pixel `xmin - reach - 0.5` or
        // later; one at or before `xmax + reach`, pixel `xmax + reach - 0.5`
        // or earlier.
        let left = round_px((xmin - reach - 0.5).floor())
            .value()
            .max(x0.value());
        let right = (round_px((xmax + reach - 0.5).floor()).value() + 1).min(x1.value());
        (left < right).then(|| (Px::of(left), Px::of(right)))
    }

    /// The smear as `quad_fragment` reads it: the four corners, the hole's
    /// left, top, right and bottom, in frame pixels, where the frame sits in
    /// the target, and which shape -- 0 a quad, 1 a sweep.
    pub fn uniform(&self, origin: [f32; 2]) -> [f32; 16] {
        let c = self.corners.0;
        let mode = match self.shape {
            Shape::Quad => 0.0,
            Shape::Sweep => 1.0,
        };
        [
            c[0].x,
            c[0].y,
            c[1].x,
            c[1].y,
            c[2].x,
            c[2].y,
            c[3].x,
            c[3].y,
            self.hole.left,
            self.hole.top,
            self.hole.right,
            self.hole.bottom,
            origin[0],
            origin[1],
            mode,
            0.0,
        ]
    }
}

/// The trail a driver hands the renderer for the next frame: the ghost's
/// sweep, the quad's or the sweep's corners, or an effect -- the fire and
/// its family, riding the quad.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum TrailLook {
    Ghost(Trail),
    Smear(Smear),
    Vfx(Vfx),
}

impl TrailLook {
    /// The look moved down by a smooth scroll's `pan`, as the cells are
    /// drawn: the driver's geometry is in the grid's pixels, and a slide
    /// draws every row `pan` lower. Zero is the identity.
    pub fn lifted(self, pan: Px) -> TrailLook {
        let dy = pan.value() as f32;
        match self {
            TrailLook::Ghost(t) => TrailLook::Ghost(Trail {
                lift: Px::of(t.lift.value() + pan.value()),
                ..t
            }),
            TrailLook::Smear(s) => TrailLook::Smear(s.translated(dy)),
            TrailLook::Vfx(v) => TrailLook::Vfx(v.translated(dy)),
        }
    }
}

/// The trail as the renderer holds it for a frame, in pixels. An effect
/// keeps the rows its records span, top inclusive and bottom exclusive: its
/// reach and every particle, which is what it can dirty. The fireworks
/// hold the effect's slot while they burn ([`crate::fireworks`]), and keep
/// their stars' rows the same way.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Drawn {
    Ghost(Ghost),
    Smear(Smear),
    Vfx(Vfx, Px, Px),
    Fireworks(crate::fireworks::Stamp, Px, Px),
}

impl Drawn {
    /// Whether the trail touches any pixel row of a band: what dirties a
    /// row, so the damage stays the rows the trail is over or was.
    pub fn overlaps(&self, top: Px, bottom: Px) -> bool {
        match self {
            Drawn::Ghost(g) => g.overlaps(top, bottom),
            Drawn::Smear(s) => s.overlaps(top, bottom),
            Drawn::Vfx(_, y0, y1) | Drawn::Fireworks(_, y0, y1) => {
                y0.value() < bottom.value() && y1.value() > top.value()
            }
        }
    }
}

/// The distance from `p` to the segment `a`-`b`.
fn to_segment(p: Pt, a: Pt, b: Pt) -> f32 {
    let (bx, by) = (b.x - a.x, b.y - a.y);
    let (px, py) = (p.x - a.x, p.y - a.y);
    let len2 = (bx * bx + by * by).max(1e-6);
    let h = ((px * bx + py * by) / len2).clamp(0.0, 1.0);
    (px - bx * h).det_hypot(py - by * h)
}

/// **How much of the pixel centred at `p` the trail covers**, 0 to 1, and
/// nothing inside the hole. A quad covers by the non-zero winding rule,
/// which fills a concave quad and both lobes of one a retarget has twisted
/// -- the 0.2-1% of retargets whose quad kitty's triangle fan draws wrong --
/// with a pixel of anti-aliasing across the edge; a sweep by sfte's rule,
/// below. `quad_fragment` in `shaders.metal` is the same two rules; the CPU
/// compositor calls this one.
pub fn coverage(s: &Smear, p: Pt) -> f32 {
    if s.hole.contains(p) {
        return 0.0;
    }
    match s.shape {
        Shape::Quad => quad_cover(&s.corners, p),
        Shape::Sweep => sweep_alpha(s, p),
    }
}

/// The quad's rule: the non-zero winding test with a pixel of edge.
fn quad_cover(c: &Corners, p: Pt) -> f32 {
    let q = c.0;
    let mut dist = f32::INFINITY;
    let mut winding = 0i32;
    for i in 0..4 {
        let (a, b) = (q[i], q[(i + 1) % 4]);
        dist = dist.min(to_segment(p, a, b));
        let side = (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
        if a.y <= p.y {
            if b.y > p.y && side > 0.0 {
                winding += 1;
            }
        } else if b.y <= p.y && side < 0.0 {
            winding -= 1;
        }
    }
    let signed = if winding != 0 { -dist } else { dist };
    (0.5 - signed).clamp(0.0, 1.0)
}

/// **sfte's sweep, `_sfte_render_trail`** (`[[source: sfte]]` 35962-36047):
/// the pixel's centre projected onto the line from the tail's centre to the
/// cursor's, clamped to it, is inside when it lies within the cursor's
/// half-extent of that point on each axis, and draws at `128 t` of 255 --
/// none at the tail, half at the cursor. Hard-edged, as sfte's is.
fn sweep_alpha(s: &Smear, p: Pt) -> f32 {
    let tail = s.corners.0;
    let (tl, br) = (tail[3], tail[1]);
    let c0 = Pt {
        x: (tl.x + br.x) * 0.5,
        y: (tl.y + br.y) * 0.5,
    };
    let head = s.hole;
    let (rx, ry) = (
        (head.right - head.left) * 0.5,
        (head.bottom - head.top) * 0.5,
    );
    let c1 = Pt {
        x: head.left + rx,
        y: head.top + ry,
    };
    let (ax, ay) = (c1.x - c0.x, c1.y - c0.y);
    let l2 = ax * ax + ay * ay;
    if l2 < 0.001 {
        return 0.0;
    }
    let t = (((p.x - c0.x) * ax + (p.y - c0.y) * ay) / l2).clamp(0.0, 1.0);
    if (p.x - c0.x - t * ax).abs() <= rx && (p.y - c0.y - t * ay).abs() <= ry {
        128.0 * t / 255.0
    } else {
        0.0
    }
}

/// The smear as the record every path is handed: its bounds, in the cursor's
/// colour, fully opaque (kitty's `cursor_trail_color none`,
/// `[[source: kitty-options-definition-v0492]]` 500-512, and sfte's
/// `trail_color`); the shader and the compositor cut it to the shape and
/// scale it by the coverage, which for a sweep is its alpha.
pub fn quad_instance(s: &Smear, fill: Argb) -> Instance {
    let (x0, y0, x1, y1) = s.bounds();
    let span =
        |a: Px, b: Px| Tex(u16::try_from(b.value().saturating_sub(a.value())).unwrap_or(u16::MAX));
    Instance {
        x: Screen(i16::try_from(x0.value()).unwrap_or(i16::MAX)),
        y: Screen(i16::try_from(y0.value()).unwrap_or(i16::MAX)),
        w: span(x0, x1),
        h: span(y0, y1),
        u: Tex(0),
        v: Tex(0),
        color: fill.with_alpha(Chan::from_u8(0xFF)),
        kind: Kind::Quad,
        curve: baud_domain::contrast::CurveRow::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELL_W: Px = Px::of(8);
    const CELL_H: Px = Px::of(18);

    /// **The ghost rides with the text**: an origin that rode a scroll moves
    /// where the ghost starts from, by the ride, and a lift moves the whole
    /// ghost by the pan -- at the start, mid-sweep and landed.
    #[test]
    fn a_ghost_rides_its_origin_and_its_lift() {
        let at = |t: Trail| ghost(t, CELL_W, CELL_H).top.value();
        let base = trail((6, 3), (2, 9), 0.0);
        let rode = Trail {
            origin_dy: -18.0,
            ..base
        };
        assert_eq!(at(base), 6 * 18);
        assert_eq!(at(rode), 5 * 18, "the origin rode one row up");
        let half = Trail {
            progress: 0.5,
            ..rode
        };
        assert_eq!(
            at(half),
            (5 * 18 + 2 * 18) / 2,
            "the sweep runs from the ridden origin"
        );
        let landed = Trail {
            progress: 1.0,
            ..rode
        };
        assert_eq!(at(landed), 2 * 18, "and lands on the cell it reached");
        let lifted = TrailLook::Ghost(half).lifted(Px::of(7));
        let TrailLook::Ghost(l) = lifted else {
            panic!("a ghost lifts to a ghost");
        };
        assert_eq!(at(l), at(half) + 7, "the lift moves it by the pan");
        assert_eq!(
            ghost(l, CELL_W, CELL_H).left,
            ghost(half, CELL_W, CELL_H).left
        );
    }

    fn trail(from: (usize, usize), to: (usize, usize), progress: f32) -> Trail {
        Trail {
            from: (Row::from_index(from.0), Col::at_index(from.1)),
            to: (Row::from_index(to.0), Col::at_index(to.1)),
            progress,
            origin_dy: 0.0,
            lift: Px::of(0),
            strength: crate::flash::Strength::Full,
        }
    }

    /// At the start the ghost is over the cell the cursor left, opaque;
    /// halfway it is halfway there and half-faded; at the end it is on the
    /// destination cell and gone.
    #[test]
    fn the_ghost_lerps_from_the_old_cell_to_the_new_and_fades() {
        // Row 2 col 3 -> row 2 col 13, a ten-cell move along one row.
        let start = ghost(trail((2, 3), (2, 13), 0.0), CELL_W, CELL_H);
        assert_eq!((start.left.value(), start.top.value()), (3 * 8, 2 * 18));
        assert_eq!(start.alpha.get(), 0xB0);

        let mid = ghost(trail((2, 3), (2, 13), 0.5), CELL_W, CELL_H);
        assert_eq!(mid.left.value(), (3 * 8 + 13 * 8) / 2);
        assert_eq!(mid.top.value(), 2 * 18);
        assert_eq!(mid.alpha.get(), 0x58);

        let end = ghost(trail((2, 3), (2, 13), 1.0), CELL_W, CELL_H);
        assert_eq!(end.left.value(), 13 * 8);
        assert_eq!(end.alpha.get(), 0);
        // Always one cell in size, wherever it is on the sweep.
        assert_eq!((end.width.value(), end.height.value()), (8, 18));
    }

    /// A vertical move sweeps the row band, so a mid-sweep ghost overlaps a
    /// row between the two endpoints and not the rows outside them.
    #[test]
    fn a_vertical_ghost_overlaps_only_the_rows_it_is_over() {
        // Row 1 -> row 5, half way: the ghost's top is at row 3.
        let g = ghost(trail((1, 0), (5, 0), 0.5), CELL_W, CELL_H);
        let band = |r: u32| (Px::of(r * 18), Px::of(r * 18 + 18));
        let (t3, b3) = band(3);
        assert!(g.overlaps(t3, b3), "over row 3");
        let (t0, b0) = band(0);
        assert!(!g.overlaps(t0, b0), "not over row 0");
        let (t5, b5) = band(5);
        assert!(!g.overlaps(t5, b5), "not yet at row 5");
    }

    /// Progress is clamped, so a driver that overshoots draws the landed,
    /// invisible ghost rather than reading past the endpoints.
    #[test]
    fn progress_is_clamped() {
        let over = ghost(trail((0, 0), (0, 4), 1.5), CELL_W, CELL_H);
        let end = ghost(trail((0, 0), (0, 4), 1.0), CELL_W, CELL_H);
        assert_eq!(over, end);
    }
    /// A block cursor's rectangle at `(col, row)` on 17 x 34 pixel cells --
    /// the kitty replay's cells, baud's app cell at 2x.
    fn block(col: f32, row: f32) -> Rect {
        Rect {
            left: col * 17.0,
            top: row * 34.0,
            right: (col + 1.0) * 17.0,
            bottom: (row + 1.0) * 34.0,
        }
    }

    fn close(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() <= tol
    }

    /// **A horizontal move is kitty's**, to the half pixel: the replay of
    /// `cursor_trail.c` at 1e1486eb, on a 90-column jump along row 10, puts
    /// the leading corner (TR) and the trailing one (TL) where the closed
    /// form does. The replay's clock starts a frame late -- its first step
    /// integrates from the render before the move -- so its times are these
    /// less 1/120 s.
    #[test]
    fn a_horizontal_move_is_kittys() {
        let seg = Segment::new(block(10.0, 10.0).corners(), block(100.0, 10.0));
        assert_eq!(seg.tau, [TAU_FAST, TAU_FAST, TAU_SLOW, TAU_SLOW]);
        let frame = 1.0 / 120.0;
        for (t, tr, tl) in [
            (0.0, 858.3, 375.7),
            (0.05, 1690.2, 1143.2),
            (0.1, 1716.2, 1465.9),
            (0.2, 1717.0, 1658.6),
            (0.4, 1717.0, 1698.7),
        ] {
            let c = seg.at(t + frame).0;
            assert!(close(c[0].x, tr, 0.5), "TR at {t}: {} against {tr}", c[0].x);
            assert!(close(c[3].x, tl, 0.5), "TL at {t}: {} against {tl}", c[3].x);
            assert_eq!(
                (c[0].y, c[3].y),
                (340.0, 340.0),
                "the move is along the row"
            );
        }
        // The half-pixel rule, solved: the trailing corner, 1530 px out on
        // a 0.4 s constant, arrives at 0.04 * log2(3060) s; the replay's
        // 120 Hz end is the first frame after it, 0.4583 s less a frame.
        assert!(close(seg.lasts(), 0.4632, 0.0005), "{}", seg.lasts());
    }

    /// **The corners facing the move lead**, whichever way it goes: a move
    /// down makes the bottom corners fast, a move left the left ones, and a
    /// diagonal puts the corner it points at fastest and the one opposite
    /// slowest.
    #[test]
    fn the_corners_facing_the_move_lead() {
        let from = block(50.0, 20.0).corners();
        let down = Segment::new(from, block(50.0, 40.0)).tau;
        assert_eq!(down, [TAU_SLOW, TAU_FAST, TAU_FAST, TAU_SLOW]);
        let left = Segment::new(from, block(5.0, 20.0)).tau;
        assert_eq!(left, [TAU_SLOW, TAU_SLOW, TAU_FAST, TAU_FAST]);
        let diag = Segment::new(from, block(80.0, 35.0)).tau;
        // Down and to the right: bottom-right leads, top-left trails.
        assert_eq!((diag[1], diag[3]), (TAU_FAST, TAU_SLOW));
        assert!(diag.iter().all(|&t| (TAU_FAST..=TAU_SLOW).contains(&t)));
    }

    /// **A corner leads by the cosine of its travel and its outward
    /// direction**, spread over the four: on a diagonal move from rest the
    /// two corners between the leader and the trailer take the constants
    /// interpolated by their cosines, computed here in f64 from the
    /// derivation in note W, section 8.
    #[test]
    fn a_corners_lead_is_the_cosine_of_its_travel_and_outward_direction() {
        let (from, to) = (block(50.0, 20.0), block(80.0, 35.0));
        let tau = Segment::new(from.corners(), to).tau;
        let c = to.center();
        let cos: Vec<f64> = (0..4)
            .map(|i| {
                let (p, q) = (from.corners().0[i], to.corners().0[i]);
                let (dx, dy) = (f64::from(q.x - p.x), f64::from(q.y - p.y));
                let (ox, oy) = (f64::from(q.x - c.x), f64::from(q.y - c.y));
                (dx * ox + dy * oy) / (dx.det_hypot(dy) * ox.det_hypot(oy))
            })
            .collect();
        let lo = cos.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = cos.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        for (i, k) in cos.iter().enumerate() {
            let s = (k - lo) / (hi - lo);
            let want = f64::from(TAU_FAST) * s + f64::from(TAU_SLOW) * (1.0 - s);
            assert!(
                (f64::from(tau[i]) - want).abs() < 1e-5,
                "corner {i}: {} against {want}",
                tau[i]
            );
        }
        assert!(tau[0] > TAU_FAST && tau[0] < TAU_SLOW, "{tau:?}");
    }

    /// **A corner that does not travel has no direction**, so it takes no
    /// part in the spread: of three corners still travelling after a
    /// retarget, the one leading most is fast and the one leading least
    /// slow, wherever the fourth, already home, would have sat among them.
    #[test]
    fn a_corner_already_home_takes_no_part_in_the_spread() {
        let to = block(50.0, 20.0);
        let [tr, br, bl, tl] = to.corners().0;
        let from = Corners([
            tr,
            Pt {
                x: br.x - 40.0,
                y: br.y - 40.0,
            },
            Pt {
                x: bl.x,
                y: bl.y - 60.0,
            },
            Pt {
                x: tl.x + 40.0,
                y: tl.y,
            },
        ]);
        let tau = Segment::new(from, to).tau;
        assert_eq!((tau[1], tau[3]), (TAU_FAST, TAU_SLOW), "{tau:?}");
        assert!(tau[2] > TAU_FAST && tau[2] < TAU_SLOW, "{tau:?}");
        assert_eq!(tau[0], TAU_SLOW, "a corner at home takes the slow one");
    }

    /// **No model of the quad divides by the half-diagonal**, the term an
    /// audit against kitty v0.49.2 found the old rate choice shared with
    /// kitty's text: a constant over all four corners, which the spread
    /// cancels. This file is read, whitespace removed, for a division by
    /// the names that term was spelled with. The research prototypes that
    /// once carried it were read too, and are not in this tree.
    #[test]
    fn no_model_of_the_quad_divides_by_the_half_diagonal() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
        let files = [format!("{root}/baud-render/src/trail.rs")];
        let mut found = Vec::new();
        for file in &files {
            let text: String = std::fs::read_to_string(file)
                .expect(file)
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect();
            for name in ["hd", "half", "half_diag"] {
                if text.contains(&format!("/{name}/")) {
                    found.push(format!("{file}: /{name}/"));
                }
            }
        }
        assert!(found.is_empty(), "{found:#?}");
    }

    /// **A quick move is fast at every corner** and ends in about a tenth of
    /// a second even from a whole jump away: 0.1 s over 10 of
    /// `log2(1530 / 0.5)`, 0.116 s.
    #[test]
    fn a_quick_move_is_fast_at_every_corner() {
        let seg = Segment::quick(block(10.0, 10.0).corners(), block(12.0, 10.0));
        assert_eq!(seg.tau, [TAU_FAST; 4]);
        let far = Segment::quick(block(10.0, 10.0).corners(), block(100.0, 10.0));
        assert!((far.lasts() - 0.1158).abs() < 0.0005, "{}", far.lasts());
    }

    /// **A move ends where it was going**, exactly, and at its start is where
    /// it came from.
    #[test]
    fn a_move_runs_from_its_start_to_its_target() {
        let from = block(3.0, 4.0).corners();
        let to = block(70.0, 30.0);
        let seg = Segment::new(from, to);
        assert_eq!(seg.at(0.0), from);
        let end = seg.at(seg.lasts()).0;
        for (p, q) in end.iter().zip(to.corners().0) {
            assert!(
                close(p.x, q.x, 0.5) && close(p.y, q.y, 0.5),
                "{p:?} against {q:?}"
            );
        }
    }

    fn smear(corners: [(f32, f32); 4], hole: Rect) -> Smear {
        Smear {
            corners: Corners(corners.map(|(x, y)| Pt { x, y })),
            hole,
            shape: Shape::Quad,
            strength: crate::flash::Strength::Full,
            opacity: Smear::OPAQUE,
        }
    }

    /// A sweep from the tail rectangle `tail` to the cursor at `head`.
    fn sweep(tail: Rect, head: Rect) -> Smear {
        Smear {
            corners: tail.corners(),
            hole: head,
            shape: Shape::Sweep,
            strength: crate::flash::Strength::Full,
            opacity: Smear::OPAQUE,
        }
    }

    /// **sfte's `_sfte_render_trail`, ported line for line**
    /// (`[[source: sfte]]` 35962-36047) with no window padding: the alpha, 0
    /// to 255, it blends the pixel centred at `up` with, for a block cursor
    /// `w` by `h` whose tail's corner is at `tail` and which is at `target`.
    /// sfte stores it as a `u8`, which truncates; so does this.
    fn sfte(tail: Pt, target: Pt, w: f32, h: f32, up: Pt) -> f32 {
        let (rx, ry) = (w * 0.5, h * 0.5);
        let (cx0, cx1) = (tail.x + rx, target.x + rx);
        let (cy0, cy1) = (tail.y + ry, target.y + ry);
        let (ab_x, ab_y) = (cx1 - cx0, cy1 - cy0);
        let l2 = ab_x * ab_x + ab_y * ab_y;
        if l2 < 0.001 {
            return 0.0;
        }
        let inv_l2 = 1.0 / l2;
        if up.x >= target.x && up.x < target.x + w && up.y >= target.y && up.y < target.y + h {
            return 0.0;
        }
        let t = ((up.x - cx0) * ab_x + (up.y - cy0) * ab_y) * inv_l2;
        let t = t.clamp(0.0, 1.0);
        if (up.x - cx0 - t * ab_x).abs() <= rx && (up.y - cy0 - t * ab_y).abs() <= ry {
            (128.0 * t).trunc()
        } else {
            0.0
        }
    }

    const NO_HOLE: Rect = Rect {
        left: 0.0,
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
    };

    /// **The quad covers its inside, fades across a pixel at its edge, and
    /// spares the hole.**
    #[test]
    fn coverage_is_the_inside_with_a_pixel_of_edge() {
        let square = [(20.0, 0.0), (20.0, 20.0), (0.0, 20.0), (0.0, 0.0)];
        let s = smear(square, NO_HOLE);
        assert_eq!(coverage(&s, Pt { x: 10.5, y: 10.5 }), 1.0, "deep inside");
        assert_eq!(coverage(&s, Pt { x: 30.5, y: 10.5 }), 0.0, "far outside");
        assert!(
            close(coverage(&s, Pt { x: 20.0, y: 10.5 }), 0.5, 0.01),
            "on the edge"
        );
        let holed = smear(
            square,
            Rect {
                left: 8.0,
                top: 8.0,
                right: 12.0,
                bottom: 12.0,
            },
        );
        assert_eq!(
            coverage(&holed, Pt { x: 10.5, y: 10.5 }),
            0.0,
            "in the hole"
        );
        assert_eq!(coverage(&holed, Pt { x: 5.5, y: 5.5 }), 1.0, "beside it");
    }

    /// **A concave quad and a twisted one are filled, not fanned**: the
    /// shapes a retarget can make, which kitty's triangle fan draws wrong. A
    /// dart's notch is outside; both lobes of a bowtie are inside.
    #[test]
    fn a_concave_quad_and_a_bowtie_are_filled_by_winding() {
        // A dart: the fourth corner pushed in past the diagonal.
        let dart = smear(
            [(40.0, 0.0), (40.0, 40.0), (0.0, 40.0), (30.0, 30.0)],
            NO_HOLE,
        );
        assert_eq!(
            coverage(&dart, Pt { x: 35.5, y: 20.5 }),
            1.0,
            "inside the dart"
        );
        assert_eq!(
            coverage(&dart, Pt { x: 10.5, y: 10.5 }),
            0.0,
            "in its notch"
        );
        // A bowtie: the two left corners swapped.
        let bow = smear(
            [(40.0, 0.0), (40.0, 40.0), (0.0, 0.0), (0.0, 40.0)],
            NO_HOLE,
        );
        assert_eq!(
            coverage(&bow, Pt { x: 36.5, y: 20.5 }),
            1.0,
            "the right lobe"
        );
        assert_eq!(coverage(&bow, Pt { x: 3.5, y: 20.5 }), 1.0, "the left lobe");
        assert_eq!(
            coverage(&bow, Pt { x: 20.5, y: 5.5 }),
            0.0,
            "between the lobes"
        );
    }

    /// **The bounds hold every pixel the trail can touch**, a quad's
    /// anti-aliased edge included, and a sweep's run from its tail to the
    /// cursor it spares.
    #[test]
    fn the_bounds_hold_every_covered_pixel() {
        let quad = smear(
            [(90.3, 4.6), (91.2, 60.1), (7.9, 58.8), (6.4, 3.3)],
            NO_HOLE,
        );
        let swept = sweep(
            Rect {
                left: 6.4,
                top: 3.3,
                right: 23.4,
                bottom: 37.3,
            },
            Rect {
                left: 85.0,
                top: 40.0,
                right: 102.0,
                bottom: 74.0,
            },
        );
        for s in [quad, swept] {
            let (x0, y0, x1, y1) = s.bounds();
            let mut seen = 0;
            for y in 0..90u16 {
                for x in 0..120u16 {
                    let p = Pt {
                        x: f32::from(x) + 0.5,
                        y: f32::from(y) + 0.5,
                    };
                    if coverage(&s, p) > 0.0 {
                        seen += 1;
                        let (x, y) = (u32::from(x), u32::from(y));
                        assert!(x >= x0.value() && x < x1.value(), "{:?} x {x}", s.shape);
                        assert!(y >= y0.value() && y < y1.value(), "{:?} y {y}", s.shape);
                    }
                }
            }
            assert!(seen > 100, "{:?} covered {seen} pixels", s.shape);
        }
    }

    /// **The columns hold every pixel the smear covers, on every row and
    /// every band of rows**: a quad and a sweep on a jump down the frame's
    /// diagonal, a hop along a row and a move up and left, from the first
    /// frame to past the end, a dart, a bowtie and a bar-thin quad, each
    /// pixel the coverage is above zero at is inside its own row's
    /// [`Smear::columns`] and its band's, at bands of 17 and 34 rows. The
    /// positive control: the jump's columns are a fraction of its bounds.
    /// Perturbation: the quad's half pixel of edge left out fails it.
    #[test]
    fn a_smears_columns_hold_every_pixel_it_covers() {
        let (w, h) = (612u16, 340u16);
        let mut smears = Vec::new();
        for (from, to) in [
            (block(1.0, 0.0), block(33.0, 9.0)),
            (block(4.0, 4.0), block(14.0, 4.0)),
            (block(30.0, 8.0), block(3.0, 1.0)),
        ] {
            for t in [0.0f32, 0.016, 0.033, 0.066, 0.1, 0.2, 0.6] {
                smears.push(smear(
                    Segment::new(from.corners(), to).at(t).0.map(|p| (p.x, p.y)),
                    to,
                ));
                smears.push(Smear {
                    corners: Segment::sweep(from.corners(), to).at(t),
                    ..sweep(from, to)
                });
            }
        }
        smears.push(smear(
            [(140.0, 0.0), (140.0, 140.0), (0.0, 140.0), (105.0, 105.0)],
            NO_HOLE,
        ));
        smears.push(smear(
            [(300.0, 10.0), (300.0, 200.0), (20.0, 10.0), (20.0, 200.0)],
            NO_HOLE,
        ));
        smears.push(smear(
            [(400.3, 20.2), (401.1, 20.2), (10.6, 300.7), (9.8, 300.7)],
            NO_HOLE,
        ));
        let mut covered = 0usize;
        for s in &smears {
            for y in 0..h {
                let row = (Px::of(u32::from(y)), Px::of(u32::from(y) + 1));
                let cols = s.columns(row.0, row.1);
                let bands = [17u32, 34].map(|bh| {
                    let top = u32::from(y) / bh * bh;
                    s.columns(Px::of(top), Px::of(top + bh))
                });
                for x in 0..w {
                    let p = Pt {
                        x: f32::from(x) + 0.5,
                        y: f32::from(y) + 0.5,
                    };
                    if coverage(s, p) <= 0.0 {
                        continue;
                    }
                    covered += 1;
                    let x = u32::from(x);
                    for (what, c) in [("row", cols), ("17", bands[0]), ("34", bands[1])] {
                        let inside = c.is_some_and(|(l, r)| x >= l.value() && x < r.value());
                        assert!(
                            inside,
                            "{:?} {:?}: ({x}, {y}) covered, outside the {what} columns {c:?}",
                            s.shape, s.corners
                        );
                    }
                }
            }
        }
        assert!(covered > 100_000, "the positive control: {covered} covered");
        // The jump a frame in: its quad's and its sweep's.
        for s in &smears[2..4] {
            let (x0, y0, x1, y1) = s.bounds();
            let spanned: u32 = (y0.value()..y1.value())
                .filter_map(|y| s.columns(Px::of(y), Px::of(y + 1)))
                .map(|(l, r)| r.value() - l.value())
                .sum();
            let bounded = (x1.value() - x0.value()) * (y1.value() - y0.value());
            assert!(
                spanned * 4 < bounded,
                "the positive control: {:?} spans {spanned} of {bounded} bounded",
                s.shape
            );
        }
    }

    /// **A sweep is sfte's, pixel for pixel**: its alpha is the one sfte's
    /// renderer blends, to the rounding of one step in 255, over a
    /// horizontal, a vertical and a diagonal move -- none at the tail's
    /// centre, rising to half at the cursor, nothing off the swept band and
    /// nothing in the cursor's own cell.
    #[test]
    fn a_sweep_is_sftes_box_swept_to_the_cursor() {
        let (w, h) = (17.0, 34.0);
        let cell = |x: f32, y: f32| Rect {
            left: x,
            top: y,
            right: x + w,
            bottom: y + h,
        };
        for (tail, head) in [
            ((10.0, 40.0), (160.0, 40.0)),
            ((60.0, 5.0), (60.0, 180.0)),
            ((150.0, 150.0), (12.0, 20.0)),
            ((3.3, 7.9), (171.6, 133.2)),
        ] {
            let s = sweep(cell(tail.0, tail.1), cell(head.0, head.1));
            let (mut inside, mut peak) = (0, 0.0f32);
            let corner = |(x, y): (f32, f32)| Pt { x, y };
            for y in 0..220u16 {
                for x in 0..200u16 {
                    let p = Pt {
                        x: f32::from(x) + 0.5,
                        y: f32::from(y) + 0.5,
                    };
                    let ours = coverage(&s, p) * 255.0;
                    let theirs = sfte(corner(tail), corner(head), w, h, p);
                    assert!(
                        (ours - theirs).abs() < 1.0,
                        "{tail:?} to {head:?} at ({x}, {y}): {ours} against {theirs}"
                    );
                    if theirs > 0.0 {
                        inside += 1;
                        peak = peak.max(theirs);
                    }
                }
            }
            assert!(inside > 500, "{tail:?} to {head:?} swept {inside} pixels");
            assert!(peak >= 100.0, "{tail:?} to {head:?} peaked at {peak}");
        }
    }

    /// **A sweep's tail closes as sfte's does**: every corner on the same
    /// constant, so the tail stays the cursor's rectangle, one e-fold in
    /// 100 ms -- sfte closes `0.01` of the gap a millisecond, and its own
    /// step integrated at a millisecond agrees to within a percent of the
    /// gap -- and it is over when the tail is within half a pixel.
    #[test]
    fn a_sweeps_tail_closes_as_sftes_does() {
        let (from, to) = (block(10.0, 10.0), block(40.0, 25.0));
        let seg = Segment::sweep(from.corners(), to);
        assert_eq!(seg.tau, [TAU_SWEEP; 4]);
        let (gx, gy) = (to.left - from.left, to.top - from.top);
        let (mut rx, mut ry) = (from.left, from.top);
        for ms in 1..=400u16 {
            rx += (to.left - rx) * 0.01;
            ry += (to.top - ry) * 0.01;
            let t = f32::from(ms) / 1000.0;
            let c = seg.at(t).0;
            assert!(
                close(c[3].x, rx, 0.01 * gx),
                "x at {ms} ms: {} against {rx}",
                c[3].x
            );
            assert!(
                close(c[3].y, ry, 0.01 * gy),
                "y at {ms} ms: {} against {ry}",
                c[3].y
            );
            assert!(
                close(c[1].x - c[3].x, 17.0, 0.001) && close(c[1].y - c[3].y, 34.0, 0.001),
                "the tail stays a cell"
            );
        }
        let e = seg.at(0.1).0[3];
        assert!(
            close(to.left - e.x, gx * (-1.0f32).det_exp(), 0.5),
            "one e-fold at 100 ms"
        );
        // Over when the tail is within half a pixel on both axes; sfte's own
        // test is a distance of 0.71 px, which a move along one axis reaches
        // 35 ms sooner, with the tail under a pixel of the cursor.
        let gap = gx.max(gy);
        assert!(
            close(seg.lasts(), (gap / 0.5).det_ln() / 10.0, 0.0005),
            "{}",
            seg.lasts()
        );
    }
}
