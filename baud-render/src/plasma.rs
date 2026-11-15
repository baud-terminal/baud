//! **The plasma trail**: a translucent tube of ionised gas filling kitty's
//! collapsing quad, three thin filaments braided along it and crawling into
//! the cursor, and ions peeling off the path in cyclotron spirals. It starves
//! rather than burns: the tube, the sheath and the filaments dim together as
//! `(1 - k)^2` and the filaments thin, where the fire burns out from the tail.
//! The default trail, built to a plasma spec with these fixes to it:
//!
//! * **Nothing is drawn ahead of the cursor.** The braid swings to zero
//!   ahead of the head, so without a gate its three filaments lay on the
//!   centre line there and poked a spike out of the cursor's leading edge;
//!   the filaments fade in over the first 0.3 cell behind the head.
//! * **The braid's axis is the move the quad is making**, [`Vfx::axis`], which
//!   the driver carries from the segment it last took. The spec's axis from
//!   the quad's centroid to the hole fell back to the ignition's direction
//!   every time the corners landed -- the common case -- and popped.
//! * **The braid's width is the cursor's, across the axis**: a bar or an
//!   underline cursor collapses it to one line rather than erasing it.
//! * **The filament gain is a constant.** WCAG 2.3.1's area clause is per
//!   341x256 window, so a braid that stays inside the quad cannot exceed it
//!   whatever the move's length, and the spec's length cap never engaged.
//! * **A leftward move is the rightward one mirrored** ([`Hand`]), a
//!   departure from the spec, which took the braid's offset and the ions'
//!   side, turn and spin relative to the axis and so turned a leftward move
//!   over: a trail looks the same whichever way the cursor moves along a
//!   line (note W, section 5.5).
//!
//! In the cursor's colour, intensity its opacity; [`light`] is
//! `plasma_tube` in Rust, which the CPU path draws by.

use crate::det::Det;
use crate::effect::{self, Body, Hash, Ignition, Live, Particle, Vfx, smoothstep};
use crate::trail::Pt;
use core::ops::Range;

/// Seconds the body lasts: the envelope `(1 - k)^2` is zero here, so the
/// quad the driver drops at [`end`] leaves nothing visible behind.
pub const LIFE: f32 = 0.55;

/// Cells past the quad the sheath reaches: `SHEATH * e^-DECAY` is under half
/// a step of 255, so the fragment function rejects a pixel this far out.
pub const REACH: f32 = 1.0;

/// Opacity the tube adds inside the quad, over the sheath.
pub const TUBE: f32 = 0.08;

/// The sheath's opacity at the quad's edge, and inside it.
pub const SHEATH: f32 = 0.20;

/// The sheath's e-fold rate a cell, at ignition; it grows as the plasma
/// cools, so the sheath contracts toward the tube.
pub const DECAY: f32 = 5.0;

/// The filaments' gain.
pub const FILAMENT: f32 = 0.95;

/// How often a brightness knot passes a fixed point on a filament, a second.
pub const KNOT: f32 = 2.5;

/// The effect's own numbers this frame, `k` in `EffectU`: the braid's half
/// width across the axis in cells, the axis, then the constants above,
/// the tube's and the sheath's zero for a [`Body::Bare`] plasma.
pub fn params(vfx: &Vfx) -> [f32; effect::PARAMS] {
    let a = vfx.axis;
    let h = vfx.emitter.hole;
    let unit = vfx.ignition.unit.max(1.0);
    let half = 0.5 * (a.y.abs() * (h.right - h.left) + a.x.abs() * (h.bottom - h.top)) / unit;
    let (tube, sheath) = match vfx.ignition.body {
        Body::Filled => (TUBE, SHEATH),
        Body::Bare => (0.0, 0.0),
    };
    [half, a.x, a.y, tube, sheath, DECAY, FILAMENT, KNOT]
}

/// **The plasma's handedness**: the side of its move the braid's offset
/// across the axis, and the ions' launch side, launch turn and spin, are
/// taken on. A move with a leftward part is drawn left-handed, every other
/// -- rightward, or straight up or down -- right-handed, as the spec drew
/// every move. So a leftward move is the rightward one mirrored, where the
/// spec's sense, fixed to the axis, turned it over: the braid's filaments
/// crossed the other way and the ions curled to the other side, and a
/// cursor moved left and back drew two different trails. The hand is the
/// ignition's, read off its direction, so it holds for the ignition's
/// life and a retarget in flight cannot flip the braid. Negation is exact,
/// so a right-handed move keeps every bit it had.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hand {
    Right,
    Left,
}

impl Hand {
    /// The hand of an ignition moving along `ignition.dir`.
    pub fn of(ignition: &Ignition) -> Hand {
        match ignition.dir.x < 0.0 {
            true => Hand::Left,
            false => Hand::Right,
        }
    }

    /// `v` taken in this hand: itself for the right, negated for the left.
    pub fn signed(self, v: f32) -> f32 {
        match self {
            Hand::Right => v,
            Hand::Left => -v,
        }
    }
}

/// `px_wave`: sin(2 pi x) with the argument folded into [-1, 1) first, by
/// MSL's `fract` -- `x - floor(x)`, below one -- which `f32::fract` is not
/// for a negative `x`.
fn wave(x: f32) -> f32 {
    let f = (x + 0.5 - (x + 0.5).floor()).min(1.0 - f32::EPSILON / 2.0);
    (core::f32::consts::PI * (2.0 * f - 1.0)).det_sin()
}

/// **The seconds the waves, the knots and the ions have run**: the detail
/// clock's under a [`Live`], else the ignition's age.
fn phase(f: &Ignition) -> f32 {
    f.live.map_or(f.age, |l| l.phase)
}

/// **The fine wiggle a filament takes at `s`**, cells across, under a
/// [`Live`] that asks for one; zero otherwise.
fn fine(live: Option<Live>, s: f32, ph: f32) -> f32 {
    match live {
        Some(l) if l.fine[0] != 0.0 => {
            l.fine[0] * smoothstep(0.0, 2.0, s) * wave(l.fine[1] * s + ph + l.fine[2] * l.phase)
        }
        _ => 0.0,
    }
}

/// The filaments' table, spelled as the shader spells it: cycles a cell and
/// a second of each wave and harmonic, the harmonic's share, the width in
/// cells and the gain.
const PX_K: [f32; 3] = [0.23, 0.31, 0.17];
const PX_F: [f32; 3] = [1.10, 1.35, 0.80];
const PX_K2: [f32; 3] = [0.71, 0.93, 0.53];
const PX_F2: [f32; 3] = [-0.85, -1.05, -1.20];
const PX_M: [f32; 3] = [0.22, 0.20, 0.25];
const PX_W: [f32; 3] = [0.040, 0.034, 0.050];
const PX_G: [f32; 3] = [1.00, 0.85, 0.65];

/// **The plasma's parts at a pixel**, `plasma_tube`'s: the envelope, the
/// tube with its sheath, the filaments, and how far behind the head the
/// pixel is along the axis, in cells.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Parts {
    pub env: f32,
    pub tube: f32,
    pub filaments: f32,
    pub behind: f32,
}

/// The plasma's parts at `p`, or none where its fragment function draws
/// nothing.
pub fn parts(vfx: &Vfx, p: Pt) -> Option<Parts> {
    let (e, f) = (vfx.emitter, vfx.ignition);
    if e.hole.contains(p) {
        return None;
    }
    let k = f.age / LIFE;
    if k >= 1.0 {
        return None;
    }
    let c = f.unit;
    let d0 = effect::distance(p, &e.corners) / c;
    if d0 > REACH {
        return None;
    }
    let q = params(vfx);
    let env = (1.0 - k) * (1.0 - k);
    let inside = 1.0 - smoothstep(-0.03, 0.03, d0);
    let tube = q[3] * inside + q[4] * (-q[5] * (1.0 + 1.5 * k) * d0.max(0.0)).det_exp();
    let (ax, ay) = (q[1], q[2]);
    let (rx, ry) = (
        p.x - 0.5 * (e.hole.left + e.hole.right),
        p.y - 0.5 * (e.hole.top + e.hole.bottom),
    );
    let s = -(rx * ax + ry * ay) / c;
    let mut fil = 0.0f32;
    if d0 < 0.15 {
        let w = Hand::of(&f).signed((ax * ry - ay * rx) / c);
        let amp = q[0] * 0.62 * smoothstep(0.0, 2.0, s);
        let salt = f.seed.0 ^ 0xA511_E9B3;
        let hashed = |n: u32| Hash(salt.wrapping_add(n)).next().unit();
        let kn = hashed(8);
        let t = phase(&f);
        for (i, n) in [0u32, 1, 2].into_iter().enumerate() {
            let (ph, ph2) = (hashed(n), hashed(16 + n));
            let along = amp
                * ((1.0 - PX_M[i]) * wave(PX_K[i] * s + ph + PX_F[i] * t)
                    + PX_M[i] * wave(PX_K2[i] * s + ph2 + PX_F2[i] * t))
                + fine(f.live, s, hashed(24 + n));
            let wid = (PX_W[i] * (1.0 - 0.5 * k)).max(1.1 / c);
            let x = (w - along) / wid;
            let knot = 0.72 + 0.28 * wave(0.45 * s + kn + n as f32 / 3.0 + q[7] * t);
            fil += PX_G[i] * (-x * x).det_exp() * knot;
        }
        fil = fil.clamp(0.0, 1.0)
            * q[6]
            * (1.0 - smoothstep(-0.02, 0.15, d0))
            * smoothstep(0.0, 0.3, s);
    }
    Some(Parts {
        env,
        tube,
        filaments: fil,
        behind: s,
    })
}

/// **The plasma's light at `p`**, `plasma_tube`: the tube and its sheath
/// dimming together and the braid over them, before the record's alpha.
pub fn light(vfx: &Vfx, p: Pt) -> f32 {
    parts(vfx, p).map_or(0.0, |q| {
        (q.env * (q.tube + (1.0 - q.tube) * q.filaments)).clamp(0.0, 1.0)
    })
}

/// **The plasma's light over a band's pixels**, [`Vfx::shade`]'s: [`light`]
/// at each pixel centre, bit for bit, at a fraction of its math. Every
/// value is the same function of the same numbers; what changes is how
/// often each is computed:
///
/// * **What a frame shares is computed once**: the parameters, the seven
///   hashes, the filaments' widths and phases, each quad edge's vector and
///   length ([`Quad`]).
/// * **What a column shares is computed once a column**: the braid's three
///   filaments and their knots are functions of `s`, the distance behind the
///   head along the axis, alone -- nine sines -- and are kept a column by
///   the bits of `s`'s numerator, so a pixel whose numerator is another's
///   reads its waves and any other computes its own. Along a level axis,
///   the splash's, every pixel of a column shares them, and every pixel of
///   a row shares the offset across the axis, kept the same way.
/// * **What the distance gives is kept by the distance's bits**, the last
///   of the row and the last of the column -- the distance in cells, the
///   tube's fill and the sheath's exponential: beside a level edge the
///   distance is one along a row, or down a column. At distance zero,
///   inside the quad, the exponential is 1, the series' exact value there.
/// * **A division is taken only where its quotient is not already known**:
///   a projection onto an edge at or before its start is 0 and at or past
///   its end 1, a ramp's step below it 0 and past it 1, as a correctly
///   rounded quotient gives them.
/// * **A level quad is measured a row and a column at a time**: an edge
///   whose rise is zero puts the pixel's offset along it, and so its
///   projection, in the column alone and the offset across it in the row
///   alone, and an edge whose run is zero the other way about, so each
///   edge's squared distance is a column's square plus a row's, the very
///   operations [`effect::distance`] runs, and the winding is a row's test
///   and a column's sign ([`Level`]). The splash's quad is level on every
///   frame; any other is measured whole.
/// * **A row's filament exponentials are taken together**, four at a
///   time ([`crate::det::exp_f32s`]), once its pixels have said which of
///   their three filaments reach them, and each pixel then sums its own in
///   the filaments' order: the same terms added in the same order.
/// * **A pixel deep inside a convex quad is not measured**: at least a
///   twentieth of a cell and a pixel inside each edge's line, it is
///   farther than 0.03 cells inside the quad, where every term the distance
///   enters is exactly what it is at any such depth -- the tube whole, the
///   sheath's exponential 1, the braid's edge fade 1.
/// * **A pixel the reach cannot meet is not visited**: each row runs only
///   over the columns within the reach and two pixels of the quad's
///   bounding box, a pixel wider, and each pixel is tested against the box
///   again; farther from the box is farther from the quad, which lies
///   inside it, so its light is zero.
/// * **A filament's term is not computed where it is zero**: past 10.3
///   widths across, `x * x` is past 105 and the exponential under e^-105,
///   which is under half of `f32`'s least subnormal, 2^-149, and rounds to
///   zero; adding zero leaves the sum.
///
/// `shade_is_the_light_at_every_pixel` holds it to [`light`]'s bits.
pub fn shade(
    vfx: &Vfx,
    xs: core::ops::Range<usize>,
    ys: core::ops::Range<usize>,
    mut each: impl FnMut(usize, usize, f32),
) {
    let Some(frame) = Frame::of(vfx) else {
        return;
    };
    // A pixel's centre, as the compositor reads it.
    let centre = |v: usize| f32::from(u16::try_from(v).unwrap_or(u16::MAX)) + 0.5;
    let mut start = xs.start;
    while start < xs.end {
        let cols = start..(start + COLUMNS).min(xs.end);
        let mut braids = [None::<Braid>; COLUMNS];
        let mut sheaths = [None::<Kept>; COLUMNS];
        let mut across = Columns::default();
        let mut dists = [0.0f32; COLUMNS];
        // A row's lights, the braided ones waiting on their filaments'
        // exponentials, which are taken together; each light is written
        // before it is read.
        let mut lits = [Lit::Dark; COLUMNS];
        let mut args = Args::default();
        if let Some(level) = &frame.level {
            for (i, x) in cols.clone().enumerate() {
                let edges = level.column(centre(x));
                for e in 0..4 {
                    across.squares[e][i] = edges.squares[e];
                    across.turns[e][i] = edges.turns[e];
                }
            }
        }
        for y in ys.clone() {
            let cy = centre(y);
            let Some((lo, hi)) = frame.reach(cy) else {
                continue;
            };
            let down = frame.level.as_ref().map(|level| level.row(cy));
            // The columns whose centres lie from `lo` to `hi`, a pixel wide
            // of each: the centre of column `x` is `x + 0.5`.
            let column = |v: f32| usize::try_from(v.max(0.0) as i64).unwrap_or(usize::MAX);
            let span =
                column(lo - 1.5).max(cols.start)..column(hi + 0.5).saturating_add(1).min(cols.end);
            let mut row = Row::default();
            let deep = frame.deep(cy);
            args.len = Gathered(0);
            if let (Some(level), Some(down)) = (&frame.level, &down) {
                let at = span.start.saturating_sub(cols.start)..span.end.saturating_sub(cols.start);
                level.distances(&across, down, at, &mut dists);
            }
            for x in span.clone() {
                let i = x - cols.start;
                let p = Pt {
                    x: centre(x),
                    y: cy,
                };
                let depth = match deep.is_some_and(|(lo, hi)| p.x >= lo && p.x <= hi) {
                    true => Depth::Deep,
                    false => Depth::Near,
                };
                let measure = || match down {
                    Some(_) => dists[i],
                    None => frame.quad.distance(p),
                };
                let kept = (&mut braids[i], &mut sheaths[i], &mut row);
                lits[i] = frame.light(p, depth, measure, kept, &mut args);
            }
            let exps = args.exps();
            // Each braided pixel's exponentials follow the last's, in the
            // order they were gathered.
            let mut at = 0;
            for x in span {
                if let Some(v) = frame.finish(lits[x - cols.start], exps, &mut at) {
                    each(x, y, v);
                }
            }
        }
        start = cols.end;
    }
}

/// Columns [`shade`] keeps the braids and distances of at once, on the
/// stack.
const COLUMNS: usize = 128;

/// **A quad, measured from many pixels**: its corners and each edge's
/// vector and squared length, which [`effect::distance`] recomputes a pixel.
struct Quad {
    corners: [Pt; 4],
    edges: [Pt; 4],
    lengths: [f32; 4],
}

impl Quad {
    fn of(c: &crate::trail::Corners) -> Quad {
        let q = c.0;
        let edges = core::array::from_fn(|i| {
            let (a, b) = (q[i], q[(i + 1) % 4]);
            Pt {
                x: b.x - a.x,
                y: b.y - a.y,
            }
        });
        let lengths = core::array::from_fn(|i| {
            let e: Pt = edges[i];
            (e.x * e.x + e.y * e.y).max(1e-6)
        });
        Quad {
            corners: q,
            edges,
            lengths,
        }
    }

    /// **The quad's edges as lines, if it is convex**: each edge's inward
    /// unit normal and offset, every turn between edges the same way and
    /// none of them straight or folded; none otherwise.
    fn lines(&self) -> Option<[(Pt, f32); 4]> {
        let e = self.edges;
        let turns: [f32; 4] = core::array::from_fn(|i| {
            let (a, b) = (e[i], e[(i + 1) % 4]);
            (a.x * b.y - a.y * b.x) / (self.lengths[i] * self.lengths[(i + 1) % 4]).sqrt()
        });
        let sense = if turns.iter().all(|&t| t > 1e-3) {
            1.0
        } else if turns.iter().all(|&t| t < -1e-3) {
            -1.0
        } else {
            return None;
        };
        Some(core::array::from_fn(|i| {
            let len = self.lengths[i].sqrt();
            let n = Pt {
                x: -sense * e[i].y / len,
                y: sense * e[i].x / len,
            };
            let a = self.corners[i];
            (n, n.x * a.x + n.y * a.y)
        }))
    }

    /// [`effect::distance`] to the quad, bit for bit: the least squared
    /// distance to an edge, then its root -- a correctly rounded square
    /// root never decreases, so the root of the least is the least of the
    /// roots.
    fn distance(&self, p: Pt) -> f32 {
        #[cfg(test)]
        crate::det::tally::count(&crate::det::tally::DISTANCES);
        let (mut d2, mut winding) = (f32::INFINITY, 0i32);
        for i in 0..4 {
            let (a, b) = (self.corners[i], self.corners[(i + 1) % 4]);
            let (bx, by) = (self.edges[i].x, self.edges[i].y);
            let (px, py) = (p.x - a.x, p.y - a.y);
            // `(dot / length).clamp(0, 1)`, dividing only between the ends:
            // a correctly rounded quotient is at most 0 for a dot at most 0
            // and at least 1 for a dot at least the length. A zero's sign
            // leaves `dx * dx + dy * dy` as it is.
            let dot = px * bx + py * by;
            let h = if dot <= 0.0 {
                0.0
            } else if dot >= self.lengths[i] {
                1.0
            } else {
                dot / self.lengths[i]
            };
            let (dx, dy) = (px - bx * h, py - by * h);
            d2 = d2.min(dx * dx + dy * dy);
            let side = bx * py - by * px;
            if a.y <= p.y {
                if b.y > p.y && side > 0.0 {
                    winding += 1;
                }
            } else if b.y <= p.y && side < 0.0 {
                winding -= 1;
            }
        }
        let d = d2.sqrt();
        if winding != 0 { -d } else { d }
    }
}

/// Which way an edge of a level quad runs.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Run {
    /// Its rise is zero: along a row.
    Across,
    /// Its run is zero, and its rise not: down a column.
    Down,
}

/// **A level quad, measured a row and a column at a time**: every edge
/// along a row or down a column, as the splash's always is.
///
/// For an edge along a row, `by` is zero, so `py * by` is a zero and the
/// dot product `px * bx` exactly, adding a zero changing no other number
/// and `dot <= 0` holding for either zero; the projection `h` is then the
/// column's alone, and so is `dx = px - bx * h`, while `dy = py - by * h`
/// is `py` less a zero, `py`. Its squared distance `dx * dx + dy * dy` is
/// the column's square plus the row's. Its ends share a height, so it
/// never crosses the row a pixel is on, and the winding never counts it.
/// For an edge down a column the same holds the other way about, and its
/// winding's `side`, `bx * py - by * px`, is a zero less `by * px`: above
/// zero just where `by * px` is below it, and below just where it is above.
struct Level {
    corners: [Pt; 4],
    edges: [Pt; 4],
    lengths: [f32; 4],
    runs: [Run; 4],
}

/// What a chunk's columns give each edge, an array an edge so a row's
/// pixels are measured eight at a time: [`Edges`] a column.
struct Columns {
    squares: [[f32; COLUMNS]; 4],
    turns: [[f32; COLUMNS]; 4],
}

impl Default for Columns {
    fn default() -> Columns {
        Columns {
            squares: [[0.0; COLUMNS]; 4],
            turns: [[0.0; COLUMNS]; 4],
        }
    }
}

/// What a column, or a row, gives each edge: its square, and for an edge
/// down a column the column's `by * px`, or the row's crossing (`1` where
/// the edge rises past the row, `-1` where it falls past it).
#[derive(Clone, Copy, Default)]
struct Edges {
    squares: [f32; 4],
    turns: [f32; 4],
}

impl Level {
    fn of(q: &Quad) -> Option<Level> {
        let mut runs = [Run::Across; 4];
        for (run, e) in runs.iter_mut().zip(&q.edges) {
            *run = if e.y == 0.0 {
                Run::Across
            } else if e.x == 0.0 {
                Run::Down
            } else {
                return None;
            };
        }
        Some(Level {
            corners: q.corners,
            edges: q.edges,
            lengths: q.lengths,
            runs,
        })
    }

    /// The projection of an offset whose dot product with an edge is
    /// `dot`, as [`Quad::distance`] takes it.
    fn projection(dot: f32, length: f32) -> f32 {
        if dot <= 0.0 {
            0.0
        } else if dot >= length {
            1.0
        } else {
            dot / length
        }
    }

    /// What the column whose centre is at `x` gives each edge.
    fn column(&self, x: f32) -> Edges {
        let mut out = Edges::default();
        for i in 0..4 {
            let (bx, by) = (self.edges[i].x, self.edges[i].y);
            let px = x - self.corners[i].x;
            match self.runs[i] {
                Run::Across => {
                    let h = Level::projection(px * bx, self.lengths[i]);
                    let dx = px - bx * h;
                    out.squares[i] = dx * dx;
                }
                Run::Down => {
                    out.squares[i] = px * px;
                    out.turns[i] = by * px;
                }
            }
        }
        out
    }

    /// What the row whose centre is at `y` gives each edge.
    fn row(&self, y: f32) -> Edges {
        let mut out = Edges::default();
        for i in 0..4 {
            let (a, b) = (self.corners[i], self.corners[(i + 1) % 4]);
            let by = self.edges[i].y;
            let py = y - a.y;
            match self.runs[i] {
                Run::Across => out.squares[i] = py * py,
                Run::Down => {
                    let h = Level::projection(py * by, self.lengths[i]);
                    let dy = py - by * h;
                    out.squares[i] = dy * dy;
                    out.turns[i] = if a.y <= y {
                        if b.y > y { 1.0 } else { 0.0 }
                    } else if b.y <= y {
                        -1.0
                    } else {
                        0.0
                    };
                }
            }
        }
        out
    }

    /// **[`Quad::distance`] at the row's pixels in the chunk's columns
    /// `at`**, bit for bit, eight at a time: the least of each edge's
    /// column square plus its row square, its correctly rounded root, and
    /// the sign the winding gives it, each lane by the scalar's operations.
    /// A lane past `at` reads a column the chunk holds and is not read.
    fn distances(&self, cols: &Columns, row: &Edges, at: Range<usize>, out: &mut [f32; COLUMNS]) {
        use core::simd::Select;
        use core::simd::prelude::{SimdFloat, SimdPartialEq, SimdPartialOrd, f32x8, i32x8};
        use std::simd::StdFloat;
        let zero = f32x8::splat(0.0);
        let mut i = at.start - at.start % 8;
        while i < at.end {
            let lane = |v: &[f32; COLUMNS]| f32x8::from_slice(&v[i..i + 8]);
            let mut d2 = f32x8::splat(f32::INFINITY);
            let mut winding = i32x8::splat(0);
            for e in 0..4 {
                d2 = d2.simd_min(lane(&cols.squares[e]) + f32x8::splat(row.squares[e]));
                if self.runs[e] == Run::Down {
                    let turn = lane(&cols.turns[e]);
                    let crossing = row.turns[e];
                    if crossing > 0.0 {
                        winding += turn.simd_lt(zero).select(i32x8::splat(1), i32x8::splat(0));
                    } else if crossing < 0.0 {
                        winding -= turn.simd_gt(zero).select(i32x8::splat(1), i32x8::splat(0));
                    }
                }
            }
            let d = d2.sqrt();
            let d = winding.simd_ne(i32x8::splat(0)).select(-d, d);
            d.copy_to_slice(&mut out[i..i + 8]);
            i += 8;
        }
    }
}

/// **A float's bits**, what [`shade`] keeps a value by: the same bits give
/// the same value through the same arithmetic.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Bits(u32);

impl Bits {
    fn of(v: f32) -> Bits {
        Bits(v.to_bits())
    }
}

/// Whether a pixel is deep inside the quad ([`Frame::deep`]).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Depth {
    Deep,
    Near,
}

/// A column's braid: its three filaments' offsets across the axis and
/// their knots, and the braid's fade-in behind the head, at the `s` whose
/// bits are `key`.
#[derive(Clone, Copy)]
struct Braid {
    key: Bits,
    along: [f32; 3],
    knot: [f32; 3],
    gate: f32,
}

/// What the distance gives a pixel at the distance whose bits are `key`:
/// the distance in cells, the tube's fill and the sheath's exponential.
#[derive(Clone, Copy)]
struct Kept {
    key: Bits,
    d0: f32,
    inside: f32,
    sheath: f32,
}

/// What a row keeps from its last pixel: its distance's terms, and the
/// braid's offset across the axis by its numerator's bits.
#[derive(Clone, Copy, Default)]
struct Row {
    near: Option<Kept>,
    across: Option<(Bits, f32)>,
}

/// **A pixel's light before its row's exponentials**: none, its light, or
/// its tube and its braid's terms, whose exponentials [`Args`] holds.
#[derive(Clone, Copy)]
enum Lit {
    Dark,
    Known(f32),
    Braided(Braided),
}

/// What a braided pixel keeps until its filaments' exponentials are
/// taken: the tube, the braid's edge fade and gate, the knots, and which
/// of the three filaments reach it.
#[derive(Clone, Copy)]
struct Braided {
    tube: f32,
    edge: f32,
    gate: f32,
    knot: [f32; 3],
    lit: Reached,
}

/// The filaments that reach a pixel, a bit each in their order.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Reached(u8);

/// How many arguments a row has gathered.
#[derive(Clone, Copy)]
struct Gathered(usize);

/// **A row's filament exponentials**, gathered pixel by pixel and taken
/// four at a time ([`crate::det::exp_f32s`]): at most three a pixel.
struct Args {
    values: [f32; 3 * COLUMNS],
    exps: [f32; 3 * COLUMNS],
    len: Gathered,
}

impl Default for Args {
    fn default() -> Args {
        Args {
            values: [0.0; 3 * COLUMNS],
            exps: [0.0; 3 * COLUMNS],
            len: Gathered(0),
        }
    }
}

impl Args {
    fn push(&mut self, v: f32) {
        self.values[self.len.0] = v;
        self.len.0 += 1;
    }

    /// e to each argument, rounded to `f32`, `det_exp`'s bits.
    fn exps(&mut self) -> &[f32] {
        let n = self.len.0;
        crate::det::exp_f32s(&self.values[..n], &mut self.exps[..n]);
        &self.exps[..n]
    }
}

/// **What every pixel of a frame's plasma shares**, as [`parts`] computes
/// it at each one.
struct Frame {
    hole: crate::trail::Rect,
    quad: Quad,
    /// A convex quad's edges as lines: the inward unit normal and its
    /// offset, `n . p - offset` the distance inside; none for a quad that is
    /// not convex.
    lines: Option<[(Pt, f32); 4]>,
    /// The quad's edges a row's and a column's share, if every edge is
    /// level or upright.
    level: Option<Level>,
    /// How far inside every line a pixel is deep, pixels.
    depth: f32,
    /// The quad's bounding box, and the square of the reach and two pixels.
    bounds: crate::trail::Rect,
    far: f32,
    unit: f32,
    env: f32,
    /// `-DECAY (1 + 1.5 k)`, the sheath's exponent a cell.
    rate: f32,
    q: [f32; effect::PARAMS],
    centre: Pt,
    amp: f32,
    ph: [f32; 3],
    ph2: [f32; 3],
    fa: [f32; 3],
    fa2: [f32; 3],
    kn: f32,
    thirds: [f32; 3],
    knot_age: f32,
    live: Option<Live>,
    ph3: [f32; 3],
    wid: [f32; 3],
    /// 10.3 filament widths: farther than this across from a filament its
    /// term is zero ([`shade`]).
    gone: [f32; 3],
    /// The ignition's [`Hand`], which the offset across the axis is taken
    /// in.
    hand: Hand,
}

/// MSL's `smoothstep` over a rising ramp, `e1 > e0`, dividing only inside
/// it: below or at `e0` the quotient is at most 0 and the step 0, at or past
/// `e1` at least 1 and the step 1, as [`smoothstep`] gives them.
fn ramp(e0: f32, e1: f32, x: f32) -> f32 {
    let (rise, span) = (x - e0, e1 - e0);
    if rise <= 0.0 {
        0.0
    } else if rise >= span {
        1.0
    } else {
        smoothstep(e0, e1, x)
    }
}

impl Frame {
    /// The frame's constants, or none once the body is out.
    fn of(vfx: &Vfx) -> Option<Frame> {
        let (e, f) = (vfx.emitter, vfx.ignition);
        let k = f.age / LIFE;
        if k >= 1.0 {
            return None;
        }
        let c = f.unit;
        let q = params(vfx);
        let salt = f.seed.0 ^ 0xA511_E9B3;
        let hashed = |n: u32| Hash(salt.wrapping_add(n)).next().unit();
        let xs = e.corners.0.map(|p| p.x);
        let ys = e.corners.0.map(|p| p.y);
        let lo = |v: [f32; 4]| v.iter().copied().fold(f32::INFINITY, f32::min);
        let hi = |v: [f32; 4]| v.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let far = REACH * c + 2.0;
        let quad = Quad::of(&e.corners);
        Some(Frame {
            hole: e.hole,
            lines: quad.lines(),
            level: Level::of(&quad),
            depth: 0.05 * c + 1.0,
            quad,
            bounds: crate::trail::Rect {
                left: lo(xs),
                top: lo(ys),
                right: hi(xs),
                bottom: hi(ys),
            },
            far: far * far,
            unit: c,
            env: (1.0 - k) * (1.0 - k),
            rate: -q[5] * (1.0 + 1.5 * k),
            q,
            centre: Pt {
                x: 0.5 * (e.hole.left + e.hole.right),
                y: 0.5 * (e.hole.top + e.hole.bottom),
            },
            amp: q[0] * 0.62,
            ph: [0, 1, 2].map(hashed),
            ph2: [16, 17, 18].map(hashed),
            fa: core::array::from_fn(|i| PX_F[i] * phase(&f)),
            fa2: core::array::from_fn(|i| PX_F2[i] * phase(&f)),
            kn: hashed(8),
            // `n as f32 / 3.0` for n of 0, 1 and 2: each whole number is
            // exact as an `f32`.
            thirds: [0.0f32, 1.0, 2.0].map(|n| n / 3.0),
            knot_age: q[7] * phase(&f),
            live: f.live,
            ph3: [24, 25, 26].map(hashed),
            wid: core::array::from_fn(|i| (PX_W[i] * (1.0 - 0.5 * k)).max(1.1 / c)),
            gone: core::array::from_fn(|i| 10.3 * (PX_W[i] * (1.0 - 0.5 * k)).max(1.1 / c)),
            hand: Hand::of(&f),
        })
    }

    /// **The pixels of the row at `y` the reach may meet**: the centres
    /// from the bounding box's left less the reach to its right and the
    /// reach, as [`Frame::light`]'s first test reads it, a pixel wider; none
    /// for a row past the reach.
    fn reach(&self, y: f32) -> Option<(f32, f32)> {
        let b = &self.bounds;
        let dy = (b.top - y).max(y - b.bottom).max(0.0);
        let left = self.far - dy * dy;
        if left < 0.0 {
            return None;
        }
        let half = left.sqrt() + 1.0;
        Some((b.left - half, b.right + half))
    }

    /// **The pixels of the row at `y` deep inside the quad**: the centres
    /// from the first to the last that are at least [`Frame::depth`] inside
    /// every line, a pixel short at each end, or none.
    fn deep(&self, y: f32) -> Option<(f32, f32)> {
        let (mut lo, mut hi) = (f32::NEG_INFINITY, f32::INFINITY);
        for (n, offset) in self.lines? {
            // n.x x + n.y y - offset >= depth.
            let rest = self.depth + offset - n.y * y;
            if n.x.abs() < 1e-6 {
                if rest > 0.0 {
                    return None;
                }
            } else if n.x > 0.0 {
                lo = lo.max(rest / n.x);
            } else {
                hi = hi.min(rest / n.x);
            }
        }
        (lo + 1.0 <= hi - 1.0).then_some((lo + 1.0, hi - 1.0))
    }

    /// The braid at `-along / unit`, as [`parts`] computes it at that `s`,
    /// kept by `along`'s bits.
    fn braid(&self, along_axis: f32) -> Braid {
        let s = -along_axis / self.unit;
        let amp = self.amp * smoothstep(0.0, 2.0, s);
        let mut along = [0.0f32; 3];
        let mut knot = [0.0f32; 3];
        for i in 0..3 {
            along[i] = amp
                * ((1.0 - PX_M[i]) * wave(PX_K[i] * s + self.ph[i] + self.fa[i])
                    + PX_M[i] * wave(PX_K2[i] * s + self.ph2[i] + self.fa2[i]))
                + fine(self.live, s, self.ph3[i]);
            knot[i] = 0.72 + 0.28 * wave(0.45 * s + self.kn + self.thirds[i] + self.knot_age);
        }
        Braid {
            key: Bits::of(along_axis),
            along,
            knot,
            gate: smoothstep(0.0, 0.3, s),
        }
    }

    /// **The distance's terms at `dist` pixels from the quad**, kept by the
    /// row and the column: the distance in cells, the tube's fill and the
    /// sheath's exponential (none is read past the reach), which is 1 at
    /// distance zero, inside the quad,
    /// the series' exact value there. Beside a level edge the distance is
    /// one along a row, or down a column.
    fn near(&self, dist: f32, column: &mut Option<Kept>, row: &mut Option<Kept>) -> Kept {
        let key = Bits::of(dist);
        if let Some(kept) = row.filter(|k| k.key == key) {
            return kept;
        }
        if let Some(kept) = column.filter(|k| k.key == key) {
            return kept;
        }
        let d0 = dist / self.unit;
        let z = d0.max(0.0);
        let kept = Kept {
            key,
            d0,
            inside: 1.0 - ramp(-0.03, 0.03, d0),
            // Past the reach the pixel draws nothing and the exponential is
            // never read.
            sheath: if z == 0.0 || d0 > REACH {
                1.0
            } else {
                (self.rate * z).det_exp()
            },
        };
        *row = Some(kept);
        *column = Some(kept);
        kept
    }

    /// [`light`] at `p`, or none where it is zero, at the `depth` the row
    /// puts it.
    fn light(
        &self,
        p: Pt,
        depth: Depth,
        measure: impl FnOnce() -> f32,
        (braid, column, row): (&mut Option<Braid>, &mut Option<Kept>, &mut Row),
        args: &mut Args,
    ) -> Lit {
        let b = &self.bounds;
        let dx = (b.left - p.x).max(p.x - b.right).max(0.0);
        let dy = (b.top - p.y).max(p.y - b.bottom).max(0.0);
        if dx * dx + dy * dy > self.far || self.hole.contains(p) {
            return Lit::Dark;
        }
        let c = self.unit;
        let q = &self.q;
        // Deep inside, every term the distance enters is 1: the tube's
        // `1 - smoothstep(-0.03, 0.03, d0)`, the sheath's e^0 and the
        // braid's `1 - smoothstep(-0.02, 0.15, d0)`.
        let (d0, inside, sheath) = if depth == Depth::Deep {
            (-1.0, 1.0, 1.0)
        } else {
            let k = self.near(measure(), column, &mut row.near);
            if k.d0 > REACH {
                return Lit::Dark;
            }
            (k.d0, k.inside, k.sheath)
        };
        let tube = q[3] * inside + q[4] * sheath;
        if d0 < 0.15 {
            let (ax, ay) = (q[1], q[2]);
            let (rx, ry) = (p.x - self.centre.x, p.y - self.centre.y);
            // `s` is `-along / c` and `w` is `across / c`, each kept by its
            // numerator's bits: the braid's by the column, `w` by the row;
            // `across` in the ignition's hand, as [`parts`] takes `w`, whose
            // negation is the quotient's.
            let (along, across) = (rx * ax + ry * ay, self.hand.signed(ax * ry - ay * rx));
            let key = Bits::of(along);
            let waves = match *braid {
                Some(kept) if kept.key == key => kept,
                _ => *braid.insert(self.braid(along)),
            };
            let w = match row.across {
                Some((k, w)) if k == Bits::of(across) => w,
                _ => {
                    let w = across / c;
                    row.across = Some((Bits::of(across), w));
                    w
                }
            };
            let mut lit = Reached(0);
            let widths = self.gone.iter().zip(&self.wid);
            for (n, (&along, (&gone, &wid))) in waves.along.iter().zip(widths).enumerate() {
                let off = w - along;
                // Past 10.3 widths `x * x` is past 105 whatever the rounding.
                if off.abs() > gone {
                    continue;
                }
                let x = off / wid;
                if x * x > 105.0 {
                    continue;
                }
                args.push(-x * x);
                lit.0 |= 1 << n;
            }
            let edge = if depth == Depth::Deep {
                1.0
            } else {
                1.0 - ramp(-0.02, 0.15, d0)
            };
            let braid = Braided {
                tube,
                edge,
                gate: waves.gate,
                knot: waves.knot,
                lit,
            };
            return match lit {
                Reached(0) => Lit::Known(self.braided(&braid, &[], &mut 0)),
                _ => Lit::Braided(braid),
            };
        }
        Lit::Known(self.glow(tube, 0.0))
    }

    /// The light from the tube and the braid's `fil`.
    fn glow(&self, tube: f32, fil: f32) -> f32 {
        (self.env * (tube + (1.0 - tube) * fil)).clamp(0.0, 1.0)
    }

    /// **A braided pixel's light**, its filaments summed in their order
    /// from their exponentials, those `exps` holds from `at` on, which it
    /// moves past them.
    fn braided(&self, b: &Braided, exps: &[f32], at: &mut usize) -> f32 {
        let mut fil = 0.0f32;
        for (n, (&gain, &knot)) in PX_G.iter().zip(&b.knot).enumerate() {
            if b.lit.0 & (1 << n) != 0 {
                fil += gain * exps[*at] * knot;
                *at += 1;
            }
        }
        let fil = fil.clamp(0.0, 1.0) * self.q[6] * b.edge * b.gate;
        self.glow(b.tube, fil)
    }

    /// A pixel's light once its row's exponentials are taken, or none
    /// where it is zero.
    fn finish(&self, lit: Lit, exps: &[f32], at: &mut usize) -> Option<f32> {
        match lit {
            Lit::Dark => None,
            Lit::Known(v) => Some(v),
            Lit::Braided(b) => Some(self.braided(&b, exps, at)),
        }
    }
}

/// Ions a unit of path, and the least and most an ignition sheds.
const IONS_PER_UNIT: f32 = 3.0;

/// Seconds over which the emitter sheds ions along the path.
const SWEEP: f32 = 0.10;

/// An ion's drag, a second, and its turn rate, radians a second -- two
/// turns, the sense set by its charge.
const DRAG: f32 = 2.5;
const TURN: f32 = 2.0 * core::f32::consts::TAU;

/// **Seconds from ignition to when the plasma is out**: its life, or its
/// last ion's death.
pub fn end(ignition: &Ignition) -> f32 {
    shed(ignition)
        .map(|h| {
            let i = Ion::draw(h);
            i.born + i.life
        })
        .fold(LIFE, f32::max)
}

fn shed(ignition: &Ignition) -> impl Iterator<Item = Hash> + use<> {
    let n = match ignition.live {
        Some(l) => (ignition.len / ignition.unit.max(1.0) * l.ions).clamp(0.0, 64.0),
        None => (ignition.len / ignition.unit.max(1.0) * IONS_PER_UNIT).clamp(8.0, 64.0),
    };
    let seed = ignition.seed.0;
    (0..u32::try_from(n.round() as i64).unwrap_or(8))
        .map(move |i| Hash(seed ^ i.wrapping_mul(0x9E37_79B9)).next())
}

/// An ion's draws: where along the path it is born and when, how long it
/// lives, and the hashes its place, charge, speed and size come from.
struct Ion {
    along: f32,
    born: f32,
    life: f32,
    h: [Hash; 7],
}

impl Ion {
    fn draw(h0: Hash) -> Ion {
        let mut h = [h0; 7];
        for i in 1..7 {
            h[i] = h[i - 1].next();
        }
        let along = h0.unit();
        Ion {
            along,
            born: along * SWEEP,
            life: 0.25 + 0.30 * h[1].unit(),
            h,
        }
    }

    /// The ion's charge: the sense it curls in.
    fn charge(&self) -> f32 {
        if self.h[2].unit() < 0.5 { 1.0 } else { -1.0 }
    }

    /// Where it starts and how fast, in pixels and pixels a second: the
    /// side it is born on and its turn off it taken in the ignition's
    /// [`Hand`].
    fn launch(&self, f: &Ignition) -> (Pt, Pt) {
        let u = f.unit;
        let hand = Hand::of(f);
        let perp = Pt {
            x: hand.signed(-f.dir.y),
            y: hand.signed(f.dir.x),
        };
        let [_, _, _, h3, h4, h5, _] = self.h;
        let side = (h3.unit() - 0.5) * 0.6 * u;
        let p0 = Pt {
            x: f.tail.x + f.dir.x * f.len * self.along + perp.x * side,
            y: f.tail.y + f.dir.y * f.len * self.along + perp.y * side,
        };
        let q = self.charge();
        let speed = (1.8 + 2.4 * h4.unit()) * u;
        let turn = hand.signed((h5.unit() - 0.5) * core::f32::consts::FRAC_PI_2);
        let (s, c) = turn.det_sin_cos();
        let (dx, dy) = (q * perp.x, q * perp.y);
        let carry = (f.len / SWEEP).min(40.0 * u) * 0.03;
        let v0 = Pt {
            x: speed * (dx * c - dy * s) + f.dir.x * carry,
            y: speed * (dx * s + dy * c) + f.dir.y * carry,
        };
        (p0, v0)
    }

    /// **Where it is `a` seconds after its birth**: its velocity decays and
    /// turns, `v(a) = v0 e^(-D a) R(w a)`, integrated in closed form --
    /// `p0 + v0 (e^((-D + i w) a) - 1) / (-D + i w)` as a complex number,
    /// its spin `w` taken in the ignition's [`Hand`].
    fn centre(&self, f: &Ignition, a: f32) -> Pt {
        let (p0, v0) = self.launch(f);
        let w = Hand::of(f).signed(self.charge() * TURN);
        let e = (-DRAG * a).det_exp();
        let (s, c) = (w * a).det_sin_cos();
        let den = DRAG * DRAG + w * w;
        let mr = (DRAG * (1.0 - e * c) + w * e * s) / den;
        let mi = (w * (1.0 - e * c) - DRAG * e * s) / den;
        Pt {
            x: p0.x + mr * v0.x - mi * v0.y,
            y: p0.y + mi * v0.x + mr * v0.y,
        }
    }

    fn at(&self, f: &Ignition) -> Option<Particle> {
        let a = phase(f) - self.born;
        if a < 0.0 || a > self.life {
            return None;
        }
        let heat = 1.0 - a / self.life;
        Some(Particle {
            centre: self.centre(f, a),
            // A floor of two pixels: under it the soft disc lights too little
            // of its record to be seen at 1x.
            radius: (f.unit * (0.05 + 0.04 * self.h[6].unit()) * (0.6 + 0.4 * heat)).max(2.0),
            glow: 0.85 * heat * heat.sqrt(),
        })
    }
}

/// Every ion alive at the ignition's age.
pub fn ions(ignition: &Ignition) -> impl Iterator<Item = Particle> + '_ {
    shed(ignition).filter_map(move |h| Ion::draw(h).at(ignition))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas::Kind;
    use crate::color::Argb;
    use crate::det::tally;
    use crate::effect::tests::{CELL, cell, ignition};
    use crate::effect::{Effect, Seed};
    use crate::instance::Instance;
    use crate::splash;
    use crate::trail::{Rect, Segment, Shape, Smear};
    use crate::units::Tally;
    use baud_font::Px;
    use core::ops::Range;

    /// **The closed form is the motion it names**: a fine numeric
    /// integration of `v0 e^(-D a) R(w a)` lands where the closed form puts
    /// the ion, over its whole life, and at the start it leaves at `v0`.
    #[test]
    fn an_ions_spiral_is_its_integrated_velocity() {
        let f = ignition(cell(10.0, 5.0), cell(40.0, 5.0), Seed(5));
        for h in shed(&f).take(12) {
            let ion = Ion::draw(h);
            let (p0, v0) = ion.launch(&f);
            let w = ion.charge() * TURN;
            let (mut x, mut y) = (p0.x as f64, p0.y as f64);
            let steps = 200_000u32;
            let dt = f64::from(ion.life) / f64::from(steps);
            for i in 0..steps {
                let t = (f64::from(i) + 0.5) * dt;
                let (e, (s, c)) = (
                    (-f64::from(DRAG) * t).det_exp(),
                    (f64::from(w) * t).det_sin_cos(),
                );
                x += (f64::from(v0.x) * c - f64::from(v0.y) * s) * e * dt;
                y += (f64::from(v0.x) * s + f64::from(v0.y) * c) * e * dt;
            }
            let at = ion.centre(&f, ion.life);
            assert!(
                (f64::from(at.x) - x).abs() < 0.01 && (f64::from(at.y) - y).abs() < 0.01,
                "closed form {at:?}, integrated ({x}, {y})"
            );
            let a = 1e-3;
            let early = ion.centre(&f, a);
            assert!(
                (early.x - (p0.x + v0.x * a)).abs() < 0.01
                    && (early.y - (p0.y + v0.y * a)).abs() < 0.01,
                "it leaves at v0"
            );
        }
    }

    /// **Ions leave both ways and curl both ways**: half one charge, half
    /// the other, each thrown off its own side of the path, and each lives
    /// between its birth and its death only.
    #[test]
    fn ions_leave_both_sides_and_live_their_span() {
        let f = ignition(cell(10.0, 5.0), cell(40.0, 5.0), Seed(21));
        let (mut plus, mut minus) = (0, 0);
        for h in shed(&f) {
            let ion = Ion::draw(h);
            assert!(ion.born >= 0.0 && ion.born <= SWEEP);
            assert!((0.25..=0.55).contains(&ion.life));
            assert_eq!(ion.at(&f.at(ion.born - 0.001)), None);
            assert_eq!(ion.at(&f.at(ion.born + ion.life + 0.001)), None);
            let (_, v0) = ion.launch(&f);
            // The path runs along +x, so a positive charge goes to +y.
            if ion.charge() > 0.0 {
                plus += 1;
                assert!(v0.y > 0.0, "a positive ion leaves to its side");
            } else {
                minus += 1;
                assert!(v0.y < 0.0, "a negative ion leaves to the other");
            }
            let born = ion.at(&f.at(ion.born)).expect("at its birth");
            let late = ion.at(&f.at(ion.born + ion.life * 0.8)).expect("late");
            assert!(late.glow < born.glow, "fading");
            assert!(late.radius >= 2.0, "never below the floor");
        }
        assert_eq!(plus + minus, 45, "three a unit of a fifteen-unit path");
        assert!(plus > 16 && minus > 16, "both charges: {plus} and {minus}");
    }

    /// **A [`Live`] plasma runs its insides on its own clock**: with the
    /// clock at the age, no wiggle and a trail's three ions a unit it draws
    /// the trail's own plasma, part for part and ion for ion; the clock
    /// moved on alone moves the filaments and the ions and leaves the
    /// envelope and the tube where the age has them; the age moved alone
    /// moves the envelope; a wiggle moves the filaments only; and the ions
    /// it asks for are the ions it sheds, none for zero. The controls: each
    /// of those moves is seen at some point of the path.
    #[test]
    fn a_live_plasma_runs_its_insides_on_its_own_clock() {
        let f = ignition(cell(10.0, 5.0), cell(40.0, 5.0), Seed(5)).at(0.12);
        let vfx = |ig: Ignition| Vfx {
            effect: Effect::Plasma,
            emitter: Smear {
                corners: Rect {
                    right: cell(40.0, 5.0).right,
                    ..cell(10.0, 5.0)
                }
                .corners(),
                hole: cell(40.0, 5.0),
                shape: Shape::Quad,
                strength: crate::flash::Strength::Full,
                opacity: Smear::OPAQUE,
            },
            ignition: ig,
            axis: ig.dir,
        };
        let live = |l: Live| Ignition { live: Some(l), ..f };
        let same = Live {
            phase: f.age,
            fine: [0.0, 1.4, 5.0],
            ions: IONS_PER_UNIT,
        };
        // Points along the path and across it, in front of the hole.
        let points: Vec<Pt> = (0..240)
            .map(|i| Pt {
                x: 10.5 * CELL.0 + f32::from(u8::try_from(i % 60).unwrap()) * 0.5 * CELL.0,
                y: 5.5 * CELL.1 + (f32::from(u8::try_from(i / 60).unwrap()) - 1.5) * 0.1 * CELL.1,
            })
            .collect();
        let parts_at = |ig: Ignition| -> Vec<Option<Parts>> {
            points.iter().map(|&p| parts(&vfx(ig), p)).collect()
        };
        let ions_of = |ig: Ignition| ions(&ig).collect::<Vec<_>>();
        assert_eq!(parts_at(live(same)), parts_at(f), "at its age, the trail's");
        assert_eq!(ions_of(live(same)), ions_of(f), "and its ions");
        let moved = parts_at(live(Live {
            phase: f.age + 0.4,
            ..same
        }));
        let base = parts_at(f);
        let pairs = || moved.iter().zip(&base).filter_map(|(a, b)| a.zip(*b));
        assert!(pairs().count() > 100, "the points are on the plasma");
        assert!(
            pairs().all(|(a, b)| a.env == b.env && a.tube == b.tube),
            "the body stays"
        );
        assert!(
            pairs().any(|(a, b)| a.filaments != b.filaments),
            "the filaments move"
        );
        assert_ne!(
            ions_of(live(Live {
                phase: f.age + 0.1,
                ..same
            })),
            ions_of(f),
            "the ions move"
        );
        let aged = parts_at(Ignition {
            age: f.age + 0.1,
            live: Some(same),
            ..f
        });
        let aged_pairs = aged.iter().zip(&base).filter_map(|(a, b)| a.zip(*b));
        assert!(
            aged_pairs.clone().any(|(a, b)| a.env != b.env),
            "the age moves the body"
        );
        let fine = parts_at(live(Live {
            fine: [0.3, 1.4, 5.0],
            ..same
        }));
        let fine_pairs = || fine.iter().zip(&base).filter_map(|(a, b)| a.zip(*b));
        assert!(fine_pairs().all(|(a, b)| a.env == b.env && a.tube == b.tube));
        assert!(
            fine_pairs().any(|(a, b)| a.filaments != b.filaments),
            "the wiggle"
        );
        let shed_at = |ions: f32| shed(&live(Live { ions, ..same })).count();
        assert_eq!(shed_at(0.0), 0, "none asked, none shed");
        let unit = f.len / f.unit;
        assert_eq!(
            shed_at(0.6),
            usize::try_from((unit * 0.6).round() as i64).unwrap()
        );
        assert_eq!(shed(&f).count(), 45, "the trail's three a unit of fifteen");
    }

    /// **The plasma is out when its last ion dies**, and never before its
    /// life.
    #[test]
    fn the_plasma_is_out_when_its_last_ion_dies() {
        let f = ignition(cell(10.0, 5.0), cell(40.0, 5.0), Seed(4));
        let out = end(&f);
        assert!((LIFE..=0.65).contains(&out), "out at {out}");
        assert!(ions(&f.at(out - 0.01)).count() > 0 || out == LIFE);
        assert_eq!(ions(&f.at(out + 0.001)).count(), 0);
        assert_eq!(Effect::Plasma.end(&f), out);
    }

    /// **The braid lies along the move and is as wide as the cursor across
    /// it**: the axis is the one the driver hands, and a bar cursor's hole,
    /// two pixels wide, narrows a vertical move's braid to a line.
    #[test]
    fn the_braid_follows_the_axis_and_the_cursors_width() {
        let to = cell(40.0, 5.0);
        let mut vfx = Vfx {
            effect: Effect::Plasma,
            emitter: Smear {
                corners: to.corners(),
                hole: to,
                shape: Shape::Quad,
                strength: crate::flash::Strength::Full,
                opacity: Smear::OPAQUE,
            },
            ignition: ignition(cell(10.0, 5.0), to, Seed(1)),
            axis: Pt { x: 1.0, y: 0.0 },
        };
        let k = params(&vfx);
        assert_eq!((k[1], k[2]), (1.0, 0.0));
        assert!(
            (k[0] - 0.5).abs() < 1e-6,
            "half the cell's height: {}",
            k[0]
        );
        vfx.axis = Pt { x: 0.0, y: 1.0 };
        assert!((params(&vfx)[0] - 0.5 * CELL.0 / CELL.1).abs() < 1e-6);
        vfx.emitter.hole.right = vfx.emitter.hole.left + 2.0;
        assert!(
            (params(&vfx)[0] - 1.0 / CELL.1).abs() < 1e-6,
            "a bar's width"
        );
        assert_eq!(&params(&vfx)[3..], &[TUBE, SHEATH, DECAY, FILAMENT, KNOT]);
    }

    /// Every band record of `vfx` over a `w` by `h` frame, clipped to it as
    /// the compositor clips them: its columns and its rows.
    fn bands(vfx: &Vfx, (w, h): (Px, Px)) -> Vec<(Range<usize>, Range<usize>)> {
        let mut out = Vec::new();
        vfx.instances((w, h), Argb(0xFFF4_A32C), &mut out);
        ranges(&out, (w, h))
    }

    /// The pixel ranges of the band records in `out`, in a `w` by `h` frame.
    fn ranges(out: &[Instance], (w, h): (Px, Px)) -> Vec<(Range<usize>, Range<usize>)> {
        let (w, h) = (w.value(), h.value());
        let at = |v: i16| usize::try_from(v.max(0)).unwrap_or(0);
        let end = |v: i16, d: u16, lim: u32| {
            usize::try_from(i32::from(v) + i32::from(d))
                .unwrap_or(0)
                .min(usize::try_from(lim).unwrap_or(0))
        };
        out.iter()
            .filter(|r| r.kind == Kind::Band)
            .map(|r| {
                (
                    at(r.x.0)..end(r.x.0, r.w.0, w),
                    at(r.y.0)..end(r.y.0, r.h.0, h),
                )
            })
            .collect()
    }

    /// [`shade`] over every band record of `vfx` in a `w` by `h` frame
    /// against [`light`] at each of their pixels: the same bits wherever it
    /// shades, a light of zero wherever it does not, each pixel once.
    fn shades_as_it_lights(vfx: &Vfx, frame: (Px, Px), what: &str) -> Tally {
        let centre = |v: usize| f32::from(u16::try_from(v).unwrap_or(u16::MAX)) + 0.5;
        let mut lit = Tally::default();
        for (xs, ys) in bands(vfx, frame) {
            let mut got = vec![None::<f32>; xs.len() * ys.len()];
            vfx.shade(xs.clone(), ys.clone(), |x, y, v| {
                assert!(xs.contains(&x) && ys.contains(&y), "{what}: ({x}, {y})");
                let i = (y - ys.start) * xs.len() + (x - xs.start);
                assert!(got[i].replace(v).is_none(), "{what}: ({x}, {y}) twice");
            });
            for y in ys.clone() {
                for x in xs.clone() {
                    let want = light(
                        vfx,
                        Pt {
                            x: centre(x),
                            y: centre(y),
                        },
                    );
                    match got[(y - ys.start) * xs.len() + (x - xs.start)] {
                        Some(v) => {
                            assert_eq!(v.to_bits(), want.to_bits(), "{what}: ({x}, {y})");
                            if v > 0.0 {
                                lit.bump();
                            }
                        }
                        None => assert_eq!(want, 0.0, "{what}: ({x}, {y}) left out"),
                    }
                }
            }
        }
        lit
    }

    /// **The band path is the light, bit for bit**: [`shade`] gives
    /// [`light`]'s bits at every pixel of every band record and leaves out
    /// only pixels whose light is zero -- over the launch logo's plasma at
    /// every age its census draws at 60 frames a second at 1x, at its
    /// largest frames at 2x, and over cursor trails across, down, on the
    /// diagonal and back, a bar's and a block's, from the quad's first
    /// frame to past the plasma's life, on the axes the driver hands.
    /// Perturbation: one operation of [`Frame::braid`] reordered fails it.
    #[test]
    fn shade_is_the_light_at_every_pixel() {
        let mut lit = Tally::default();
        for ((w, h), ages) in [((544u16, 380u16), 60u16), ((3400, 1850), 0)] {
            let frame = (f32::from(w), f32::from(h));
            let px = (Px::of(u32::from(w)), Px::of(u32::from(h)));
            let s = splash::scale(frame);
            let at = splash::origin(frame, s);
            let census = (0u16..)
                .map(|n| f32::from(n) / f32::from(ages.max(1)))
                .take_while(|&a| ages > 0 && a < splash::GONE);
            for age in census.chain([0.0, splash::OUT]) {
                let what = format!("the splash {w}x{h} at {age}");
                lit.add(shades_as_it_lights(&splash::vfx(at, s, age), px, &what));
            }
        }
        let bar = |r: Rect| Rect {
            right: r.left + 2.0,
            ..r
        };
        let moves = [
            (cell(2.0, 3.0), cell(30.0, 3.0), Pt { x: 1.0, y: 0.0 }),
            (cell(30.0, 3.0), cell(4.0, 3.0), Pt { x: -1.0, y: 0.0 }),
            (cell(6.0, 1.0), cell(6.0, 9.0), Pt { x: 0.0, y: 1.0 }),
            (cell(3.0, 2.0), cell(20.0, 9.0), Pt { x: 0.95, y: 0.31 }),
            (
                cell(25.0, 9.0),
                bar(cell(5.0, 2.0)),
                Pt { x: -0.8, y: -0.6 },
            ),
        ];
        for (from, to, axis) in moves {
            let f = ignition(from, to, Seed(7));
            for t in [0.0f32, 0.02, 0.05, 0.1, 0.2, 0.4, 0.54, 0.6] {
                let vfx = Vfx {
                    effect: Effect::Plasma,
                    emitter: Smear {
                        corners: Segment::new(from.corners(), to).at(t),
                        hole: to,
                        shape: Shape::Quad,
                        strength: crate::flash::Strength::Full,
                        opacity: Smear::OPAQUE,
                    },
                    ignition: f.at(t),
                    axis,
                };
                let what = format!("{from:?} to {to:?} at {t}");
                lit.add(shades_as_it_lights(&vfx, (Px::of(640), Px::of(400)), &what));
            }
        }
        assert!(lit.value() > 100_000, "the positive control: {lit:?} lit");
    }

    /// **A splash frame's plasma is a fraction of its pixels' math, and
    /// measures no pixel whole**: every frame of the launch logo's plasma
    /// at 60 frames a second at 200 by 50 cells at 2x (3400 by 1850
    /// pixels), and its largest frames at 360 by 90 (6120 by 3330),
    /// shaded over its band records as the CPU path draws them -- split
    /// where its colour turns, each clipped to each cell row's band at the
    /// renderer's own cell height, as `Target::effect` is called a row at a
    /// time -- runs under 0.65 exponentials or sines a band pixel and
    /// measures the quad whole from under 0.05 of them (0.56 to 0.63 of
    /// them on every frame before the level quad was measured a row and a
    /// column at a time, none now), where the light at each pixel ran 3.39
    /// and measured from every one. Perturbation: `Level::of` accepting an
    /// edge whose rise is under a pixel fails `shade_is_the_light_at_every_pixel`. The positive control: a diagonal cursor trail, whose quad
    /// is not level, still measures. What the CPU path's frames cost is the
    /// `splash` bench's.
    #[test]
    fn a_2x_splash_frame_runs_a_fraction_of_its_pixels_math() {
        let font =
            |p: &str| std::fs::read(format!("{}/../{p}", env!("CARGO_MANIFEST_DIR"))).unwrap();
        let (regular, bold) = (
            font("baud-font/tests/fonts/JetBrainsMono-Regular.ttf"),
            font("baud-app/fonts/jetbrains-mono/JetBrainsMono-Bold.ttf"),
        );
        let faces = [
            baud_font::Face::parse(&regular).unwrap(),
            baud_font::Face::parse(&bold).unwrap(),
        ];
        let roles = [
            crate::FaceRole::Fallback,
            crate::FaceRole::Style(crate::FaceStyle::Bold),
        ];
        let frames = (0u16..)
            .map(|f| f32::from(f) / 60.0)
            .take_while(|&a| a < splash::GONE);
        let every: Vec<f32> = frames.collect();
        let largest = [0.0, splash::OUT, splash::OUT + splash::HOLD];
        for ((cols, rows), ages) in [((200u16, 50u16), &every[..]), ((360, 90), &largest[..])] {
            let r = crate::Renderer::new(
                crate::Stack::with_roles(&faces, &roles).unwrap(),
                crate::PxSize::of(28.0).unwrap(),
                crate::Theme::dark(),
                crate::Blend::Native,
                baud_domain::Cols::of_index(usize::from(cols)),
                baud_domain::Rows::of_index(usize::from(rows)),
            )
            .unwrap();
            let cell_h = usize::try_from(r.metrics().cell_h.value()).unwrap();
            let (w, h) = r.pixel_size();
            let frame = (w.value() as f32, h.value() as f32);
            let s = splash::scale(frame);
            let at = splash::origin(frame, s);
            for &age in ages {
                let vfx = splash::vfx(at, s, age);
                let mut out = Vec::new();
                vfx.instances((w, h), Argb(0xFFF4_A32C), &mut out);
                splash::paint(&mut out, at, s, splash::DARK);
                let records = ranges(&out, (w, h));
                let pixels: usize = records.iter().map(|(xs, ys)| xs.len() * ys.len()).sum();
                let bands = (0..usize::try_from(h.value()).unwrap()).step_by(cell_h);
                let (series, distances) = tally::of(|| {
                    for top in bands {
                        for (xs, ys) in &records {
                            let ys = ys.start.max(top)..ys.end.min(top + cell_h);
                            if ys.start < ys.end {
                                vfx.shade(xs.clone(), ys, |_, _, _| {});
                            }
                        }
                    }
                });
                let per = |n: u64| n as f64 / pixels as f64;
                let what = format!("{cols}x{rows} at {age}: {pixels} band pixels");
                assert!(pixels > 200_000, "{what}");
                assert!(per(series) < 0.65, "{what}, {series} series");
                assert!(per(distances) < 0.05, "{what}, {distances} distances");
            }
        }
        // The positive control: a quad that is not level is measured whole.
        let (from, to) = (cell(3.0, 2.0), cell(20.0, 9.0));
        let f = ignition(from, to, Seed(7));
        let vfx = Vfx {
            effect: Effect::Plasma,
            emitter: Smear {
                corners: Segment::new(from.corners(), to).at(0.05),
                hole: to,
                shape: Shape::Quad,
                strength: crate::flash::Strength::Full,
                opacity: Smear::OPAQUE,
            },
            ignition: f.at(0.05),
            axis: Pt { x: 0.95, y: 0.31 },
        };
        let (_, distances) = tally::of(|| {
            for (xs, ys) in bands(&vfx, (Px::of(640), Px::of(400))) {
                vfx.shade(xs, ys, |_, _, _| {});
            }
        });
        assert!(
            distances > 1_000,
            "the diagonal trail's {distances} distances"
        );
    }

    /// **The sheath is under half a step at the reach**, which is why the
    /// fragment function may reject a pixel past it.
    #[test]
    fn the_sheath_is_gone_at_the_reach() {
        assert!(SHEATH * (-DECAY * REACH).det_exp() < 0.5 / 255.0);
    }
}
