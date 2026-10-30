//! **The effect trails**: a shader drawn over kitty's collapsing quad, lit by
//! a long move and burning out after it -- the fire, and the family beside
//! it. What they share lives here: the ignition a move lights (the path, the
//! age, the seed and the cell's height), the records a GPU path draws an
//! effect with -- one band a cell row over the effect's reach, then one soft
//! disc a live particle -- and the uniform its fragment function reads. What
//! each effect draws is its own module: [`crate::fire`], [`crate::plasma`],
//! [`crate::lightning`], [`crate::ice`], [`crate::ufo`], [`crate::orb`],
//! [`crate::liquid`].
//!
//! **Everything here is a pure function of the ignition and its age.** The
//! driver hands a [`Vfx`] -- which effect, the emitter's corners this frame,
//! and the ignition at its age -- and this derives the records and the
//! uniform. No state between frames and no clock, so the pixel oracle
//! compares two GPU arms on it and a Rust reference can hold each shader to
//! its formulas.
//!
//! **Every effect is the cursor's colour** -- the one the quad trail is drawn
//! in -- and its intensity is its opacity. Premultiplied, that is valid over
//! a glass window and dark over a light theme, whose cursor is dark, so one
//! rule serves every ground.
//!
//! **Every path draws every effect.** The GPU's fragment functions and the
//! CPU compositor read one Rust model of each -- [`Vfx::light`] over a band,
//! [`Vfx::particle`] over a particle -- and the CPU path, which the Linux
//! driver and the macOS app's `BAUD_GPU=0` draw with, composites the records
//! by it (`Target::effect`); the macOS pixel oracle holds the GPU arms and
//! the CPU arm to the same composite. The models' math is
//! [`crate::det`]'s, so a frame has the same bits on every OS.

use crate::atlas::{Kind, Tex};
use crate::color::Argb;
use crate::det::Det;
use crate::fire;
use crate::ice;
use crate::instance::{Instance, Screen};
use crate::lightning;
use crate::liquid;
use crate::orb;
use crate::plasma;
use crate::trail::{Corners, Pt, Rect, Smear};
use crate::ufo;
use baud_domain::Chan;
use baud_font::Px;

/// **Which effect a trail draws.** Each is a fragment function over the
/// same records and the same uniform prefix, with its own reach, life,
/// particles and parameters.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Effect {
    /// A flame rising off the quad, burning out from the tail toward the
    /// cursor and shedding embers: [`crate::fire`].
    Fire,
    /// A tube of ionised gas with three filaments braided into the cursor,
    /// shedding ions in cyclotron spirals: [`crate::plasma`].
    Plasma,
    /// A jagged bolt thrown from the cell the cursor left to the one it
    /// reached, forks, a corona and falling sparks: [`crate::lightning`].
    Lightning,
    /// A frost lace of six-fold crystals laid along the path, a rime haze
    /// and a pane over the quad, melting tail first into falling shards:
    /// [`crate::ice`].
    Ice,
    /// A flying saucer springing along the path to hover over the cursor,
    /// lowering a tractor beam onto it and leaving: [`crate::ufo`].
    Ufo,
    /// A glowing sphere springing along the path with a halo and a comet's
    /// tail, bursting into sparks at the cursor: [`crate::orb`].
    Orb,
    /// A clear glass gel stretching from the cell the cursor left to the
    /// cursor, pinching into beads that merge back, with a rim and a glint:
    /// [`crate::liquid`].
    Liquid,
}

impl Effect {
    /// Every effect, in the order a GPU path indexes its pipelines by.
    pub const ALL: [Effect; 7] = [
        Effect::Fire,
        Effect::Plasma,
        Effect::Lightning,
        Effect::Ice,
        Effect::Ufo,
        Effect::Orb,
        Effect::Liquid,
    ];

    /// How many effects there are: a GPU path keeps one pipeline each.
    pub const COUNT: usize = Effect::ALL.len();

    /// The fragment function a GPU path draws this effect's records with.
    pub const fn fragment(self) -> &'static str {
        match self {
            Effect::Fire => "fire_fragment",
            Effect::Plasma => "plasma_fragment",
            Effect::Lightning => "lightning_fragment",
            Effect::Ice => "ice_fragment",
            Effect::Ufo => "ufo_fragment",
            Effect::Orb => "orb_fragment",
            Effect::Liquid => "liquid_fragment",
        }
    }

    /// Cells from the emitter past which the effect draws nothing: its
    /// fragment function rejects a pixel this far out, and the bands cover
    /// exactly this reach.
    pub const fn reach(self) -> f32 {
        match self {
            Effect::Fire => fire::REACH,
            Effect::Plasma => plasma::REACH,
            Effect::Lightning => lightning::REACH,
            Effect::Ice => ice::REACH,
            Effect::Ufo => ufo::REACH,
            Effect::Orb => orb::REACH,
            Effect::Liquid => liquid::REACH,
        }
    }

    /// Seconds the effect's body lasts from ignition; its particles may
    /// outlive it ([`Effect::end`]).
    pub const fn life(self) -> f32 {
        match self {
            Effect::Fire => fire::LIFE,
            Effect::Plasma => plasma::LIFE,
            Effect::Lightning => lightning::LIFE,
            Effect::Ice => ice::LIFE,
            Effect::Ufo => ufo::LIFE,
            Effect::Orb => orb::LIFE,
            Effect::Liquid => liquid::LIFE,
        }
    }

    /// **Seconds from ignition to when the effect is out**: its life, or its
    /// last particle's death if that is later.
    pub fn end(self, ignition: &Ignition) -> f32 {
        match self {
            Effect::Fire => fire::end(ignition),
            Effect::Plasma => plasma::end(ignition),
            Effect::Lightning => lightning::end(ignition),
            Effect::Ice => ice::end(ignition),
            Effect::Ufo => ufo::end(ignition),
            Effect::Orb => orb::end(ignition),
            Effect::Liquid => liquid::end_of(ignition),
        }
    }
}

/// An ignition's seed: every hash the effect and its particles draw from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Seed(pub u32);

/// A hash word, advanced by PCG's output permutation over one step of its
/// LCG (O'Neill 2014), which Jarzynski and Olano recommend as the GPU hash
/// (`trail/effects/papers/hash.txt` 360-366). Written from the published
/// algorithm; the shader's `fx_hash` is the same function.
#[derive(Clone, Copy)]
pub(crate) struct Hash(pub(crate) u32);

impl Hash {
    pub(crate) fn next(self) -> Hash {
        let s = self.0.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
        let w = ((s >> ((s >> 28) + 4)) ^ s).wrapping_mul(277_803_737);
        Hash((w >> 22) ^ w)
    }

    /// A uniform number in [0, 1) from the word's top 23 bits: the mantissa
    /// of a float in [1, 2), less one -- exact, with no integer conversion.
    pub(crate) fn unit(self) -> f32 {
        f32::from_bits((self.0 >> 9) | 0x3f80_0000) - 1.0
    }
}

/// A lattice point of an effect's noise.
struct Lattice(i32, i32);

/// `fx_lattice`.
fn lattice(at: Lattice, seed: Hash) -> f32 {
    let inner = Hash(at.1.cast_unsigned().wrapping_add(seed.0)).next().0;
    Hash(at.0.cast_unsigned().wrapping_add(inner)).next().unit()
}

/// `fx_noise`: value noise on the hash's lattice, with the quintic fade --
/// the fire's flame and the ice's rime.
pub(crate) fn noise(x: f32, y: f32, seed: Hash) -> f32 {
    let (fx, fy) = (x.floor(), y.floor());
    let (ix, iy) = (fx as i32, fy as i32);
    let (u, v) = (x - fx, y - fy);
    let fade = |t: f32| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let (wx, wy) = (fade(u), fade(v));
    let a = lattice(Lattice(ix, iy), seed);
    let b = lattice(Lattice(ix + 1, iy), seed);
    let c = lattice(Lattice(ix, iy + 1), seed);
    let d = lattice(Lattice(ix + 1, iy + 1), seed);
    let mix = |p: f32, q: f32, t: f32| p + (q - p) * t;
    mix(mix(a, b, wx), mix(c, d, wx), wy)
}

/// MSL's `smoothstep`.
pub(crate) fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// **A particle's soft disc**, `fx_particle`: its opacity at `uv`, -1 to 1
/// on each axis of its record, before the record's alpha -- an ember, an
/// ion, a spark.
pub fn disc(uv: Pt) -> f32 {
    let a = 1.0 - smoothstep(0.15, 1.0, (uv.x * uv.x + uv.y * uv.y).sqrt());
    a * a
}

/// **One ignition of an effect**, as a frame sees it: the path the move took
/// -- from the centre of where the cursor was to the centre of where it
/// went -- the seconds since the ignition, its seed, and the effect's unit,
/// the cell's height in pixels.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Ignition {
    pub tail: Pt,
    /// The unit vector from the tail to the head.
    pub dir: Pt,
    /// The distance from the tail to the head, pixels.
    pub len: f32,
    pub age: f32,
    pub seed: Seed,
    pub unit: f32,
    /// How far the path has ridden a smooth scroll since it was lit, pixels,
    /// y down -- every [`Ignition::translated`] adds to it, the ride's and a
    /// frame's lift alike. The fire's noise is anchored on it
    /// ([`fire::params`]), so a flame that rides keeps its shape rather than
    /// crawling through noise fixed to the frame. Zero for one that never
    /// rode, which draws exactly as before.
    pub rode: f32,
}

impl Ignition {
    /// The ignition of a move from `tail` to the centre of `to`, at age 0.
    pub fn ignite(tail: Pt, to: Rect, seed: Seed, unit: f32) -> Ignition {
        let head = to.center();
        let (dx, dy) = (head.x - tail.x, head.y - tail.y);
        let len = dx.det_hypot(dy).max(1.0);
        Ignition {
            tail,
            dir: Pt {
                x: dx / len,
                y: dy / len,
            },
            len,
            age: 0.0,
            seed,
            unit,
            rode: 0.0,
        }
    }

    /// The same ignition `age` seconds after it.
    pub fn at(self, age: f32) -> Ignition {
        Ignition { age, ..self }
    }

    /// The path moved `dy` pixels down, rigidly: its tail and so its head,
    /// with its direction, length, seed and age kept. Stretching it to keep
    /// the head on the cursor would re-lay the geometry each effect reads
    /// off the length -- the lightning's levels, the fire's ember count, the
    /// ice's lattice.
    pub fn translated(self, dy: f32) -> Ignition {
        Ignition {
            tail: Pt {
                x: self.tail.x,
                y: self.tail.y + dy,
            },
            rode: self.rode + dy,
            ..self
        }
    }
}

/// **A live particle at the ignition's age**: its centre and radius in
/// frame pixels, and its glow, the opacity it draws the cursor's colour at.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Particle {
    pub centre: Pt,
    pub radius: f32,
    pub glow: f32,
}

/// **An effect as the renderer holds it for a frame**: which one, the
/// emitter -- kitty's quad this frame, whose hole is the cursor -- the
/// ignition it gives off, and the axis the quad is moving along.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Vfx {
    pub effect: Effect,
    pub emitter: Smear,
    pub ignition: Ignition,
    /// The unit direction of the move the quad is making: from where the
    /// segment the driver last took started to where it ends. The
    /// ignition's direction is its first move's, which a retarget inside
    /// the driver's reignite window keeps; this turns only when the quad turns,
    /// and stays when its corners land.
    pub axis: Pt,
}

/// The floats in an effect's own numbers, `k` in `EffectU`.
pub const PARAMS: usize = 8;

/// The floats in the uniform an effect's fragment function reads, `EffectU`
/// in `shaders.metal`: a shared prefix, then [`PARAMS`].
pub const UNIFORM_FLOATS: usize = 24 + PARAMS;

impl Vfx {
    /// The effect moved `dy` pixels down, rigidly: its emitter's corners and
    /// hole, and its ignition's path. The axis is a direction and does not
    /// move.
    pub fn translated(self, dy: f32) -> Vfx {
        Vfx {
            emitter: self.emitter.translated(dy),
            ignition: self.ignition.translated(dy),
            ..self
        }
    }

    /// **The records a GPU path draws the effect with**, in `fill`, the
    /// cursor's colour: one band a cell row over the emitter's reach -- the
    /// pixels within [`Effect::reach`] cells of it, which is everywhere the
    /// effect's fragment function can draw -- clipped to the frame, then one
    /// soft disc a live particle, at its glow. **The bands tile the pixel
    /// rows**: a row is in the one band whose span holds its centre, so no
    /// row is drawn twice, at a unit of whole pixels (every cursor trail's,
    /// the cell's height) or not (the launch logo's).
    pub fn instances(&self, frame: (Px, Px), fill: Argb, out: &mut Vec<Instance>) {
        let unit = self.ignition.unit.max(1.0);
        let r = self.effect.reach() * unit;
        let (fw, fh) = (px(frame.0), px(frame.1));
        let mut shapes: Vec<[Pt; 4]> = Vec::new();
        self.support(|c| shapes.push(c.0));
        let (y0, y1) = shapes
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), q| {
                let ys = q.map(|p| p.y);
                (lo.min(min4(ys)), hi.max(max4(ys)))
            });
        let y0 = (y0 - r).max(0.0);
        let y1 = (y1 + r).min(fh);
        let mut band = (y0 / unit).floor() * unit;
        // A band's rows are those whose centre is in its span: an edge at
        // `e` is the row `ceil(e - 0.5)`, the first whose centre is not above
        // it -- the rule a GPU covers a pixel by, its one sample at the
        // pixel's centre ([[source: apple-metal-sample-positions]]), and the
        // centre the light is taken at. Each edge is rounded once and shared,
        // a band's top the last one's bottom, so the bands tile the rows and
        // none is drawn twice where a unit puts an edge between rows.
        let row = |e: f32| (e - 0.5).ceil();
        let mut top = row(band.max(0.0));
        while band < y1 {
            let (lo, hi) = (band - r, band + unit + r);
            let (mut xmin, mut xmax) = (f32::INFINITY, f32::NEG_INFINITY);
            for q in &shapes {
                for i in 0..4 {
                    let (a, b) = (q[i], q[(i + 1) % 4]);
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
            let (x0, x1) = ((xmin - r).max(0.0).floor(), (xmax + r).min(fw).ceil());
            let bottom = row((band + unit).min(fh));
            if xmin <= xmax && x1 > x0 && bottom > top {
                out.push(Instance {
                    x: screen(x0),
                    y: screen(top),
                    w: tex(x1 - x0),
                    h: tex(bottom - top),
                    u: Tex(0),
                    v: Tex(0),
                    color: fill.with_alpha(Chan::from_u8(0xFF)),
                    kind: Kind::Band,
                    curve: baud_domain::contrast::CurveRow::default(),
                });
            }
            band += unit;
            top = bottom;
        }
        self.particles(|s| {
            let side = (2.0 * s.radius).ceil().max(2.0);
            out.push(Instance {
                x: screen((s.centre.x - side * 0.5).floor()),
                y: screen((s.centre.y - side * 0.5).floor()),
                w: tex(side),
                h: tex(side),
                u: Tex(0),
                v: Tex(0),
                color: fill.with_alpha(Chan::from_unit(s.glow)),
                kind: Kind::Particle,
                curve: baud_domain::contrast::CurveRow::default(),
            });
        });
    }

    /// **The effect as its fragment function reads it**, `EffectU` in
    /// `shaders.metal`: the emitter's corners and hole, where the frame sits
    /// in the target, the path, the unit, the age, the life, the seed's bits
    /// and the reach, then the effect's own numbers.
    pub fn uniform(&self, origin: [f32; 2]) -> [f32; UNIFORM_FLOATS] {
        let c = self.region().0;
        let h = self.emitter.hole;
        let f = self.ignition;
        let k = self.params();
        [
            c[0].x,
            c[0].y,
            c[1].x,
            c[1].y,
            c[2].x,
            c[2].y,
            c[3].x,
            c[3].y,
            h.left,
            h.top,
            h.right,
            h.bottom,
            origin[0],
            origin[1],
            f.tail.x,
            f.tail.y,
            f.dir.x,
            f.dir.y,
            f.len,
            f.unit,
            f.age,
            self.effect.life(),
            f32::from_bits(f.seed.0),
            self.effect.reach(),
            k[0],
            k[1],
            k[2],
            k[3],
            k[4],
            k[5],
            k[6],
            k[7],
        ]
    }

    /// **The effect's own numbers this frame**, `k` in the shader's
    /// `EffectU`: Rust owns them, so the shader and the particles built here
    /// read one spelling.
    pub fn params(&self) -> [f32; PARAMS] {
        match self.effect {
            Effect::Fire => fire::params(&self.ignition),
            Effect::Plasma => plasma::params(self),
            Effect::Lightning => lightning::PARAMS,
            Effect::Ice => ice::PARAMS,
            Effect::Ufo => ufo::PARAMS,
            Effect::Orb => orb::PARAMS,
            Effect::Liquid => liquid::PARAMS,
        }
    }

    /// **The region the effect measures distance from**, the corners its
    /// fragment function reads: the quad the fire, the plasma and the ice's
    /// pane ride, or the lightning's own box along its path.
    pub fn region(&self) -> Corners {
        match self.effect {
            Effect::Fire
            | Effect::Plasma
            | Effect::Ice
            | Effect::Ufo
            | Effect::Orb
            | Effect::Liquid => self.emitter.corners,
            Effect::Lightning => lightning::region(&self.ignition),
        }
    }

    /// **Every shape the effect draws within its reach of**, which the
    /// bands cover: its region; for the ice the path as well -- the frost is
    /// laid along the path, where the quad is for a few tens of milliseconds
    /// at a point; for the ufo only the box its saucer, beam and wake are
    /// drawn in, which the quad's reach could not hold; for the orb the line
    /// it flies on, which it lags the quad along; and for the liquid its spine
    /// and the cursor, which kitty's leading corners have not reached early
    /// in a move.
    pub fn support(&self, mut each: impl FnMut(Corners)) {
        match self.effect {
            Effect::Fire | Effect::Plasma | Effect::Lightning => each(self.region()),
            Effect::Ice => {
                each(self.region());
                let f = &self.ignition;
                let head = Pt {
                    x: f.tail.x + f.dir.x * f.len,
                    y: f.tail.y + f.dir.y * f.len,
                };
                each(Corners([f.tail, head, head, f.tail]));
            }
            Effect::Ufo => each(ufo::region(self)),
            Effect::Orb => each(orb::line(self)),
            Effect::Liquid => liquid::support(self, each),
        }
    }

    /// **The effect's light at `p`**, a pixel centre in frame pixels: the
    /// opacity a band's record draws the cursor's colour at there, before
    /// the record's alpha -- the effect's fragment function over a band,
    /// which the CPU path evaluates and the pixel oracle holds the GPU's to.
    pub fn light(&self, p: Pt) -> f32 {
        match self.effect {
            Effect::Fire => fire::light(self, p),
            Effect::Plasma => plasma::light(self, p),
            Effect::Lightning => lightning::light(self, p),
            Effect::Ice => ice::light(self, p),
            Effect::Ufo => ufo::light(self, p),
            Effect::Orb => orb::light(self, p),
            Effect::Liquid => liquid::light(self, p),
        }
    }

    /// **The effect's light over a band's pixels**, `xs` by `ys` in whole
    /// pixels: `each(x, y, light)` for every pixel the light may be above
    /// zero at, the light [`Vfx::light`] at the pixel's centre, bit for
    /// bit. A pixel it is zero at may be left out. The CPU compositor draws
    /// a band by it (`Target::effect`); the plasma's has a form of its own
    /// ([`plasma::shade`]), every other effect is the light at each pixel.
    pub fn shade(
        &self,
        xs: core::ops::Range<usize>,
        ys: core::ops::Range<usize>,
        mut each: impl FnMut(usize, usize, f32),
    ) {
        match self.effect {
            Effect::Plasma => plasma::shade(self, xs, ys, each),
            _ => {
                // A pixel's centre, as the compositor reads it.
                let centre = |v: usize| f32::from(u16::try_from(v).unwrap_or(u16::MAX)) + 0.5;
                for y in ys {
                    let cy = centre(y);
                    for x in xs.clone() {
                        each(
                            x,
                            y,
                            self.light(Pt {
                                x: centre(x),
                                y: cy,
                            }),
                        );
                    }
                }
            }
        }
    }

    /// **A particle's opacity at `uv`**, -1 to 1 on each axis of its
    /// record, before the record's alpha: the ice's hexagonal shard, the
    /// ufo's exhaust ring, every other effect's soft disc.
    pub fn particle(&self, uv: Pt) -> f32 {
        match self.effect {
            Effect::Ice => ice::shard(uv),
            Effect::Ufo => ufo::ring(uv),
            Effect::Fire | Effect::Plasma | Effect::Lightning | Effect::Orb | Effect::Liquid => {
                disc(uv)
            }
        }
    }

    /// Seconds from ignition to when this effect is out.
    pub fn end(&self) -> f32 {
        self.effect.end(&self.ignition)
    }

    /// **Every particle alive at the effect's age, in order.** The orb's
    /// sparks burst from where the orb is, which only the emitter says;
    /// every other effect's particles read the ignition alone.
    pub fn particles(&self, mut each: impl FnMut(Particle)) {
        let ig = &self.ignition;
        match self.effect {
            Effect::Fire => fire::embers(ig).for_each(&mut each),
            Effect::Plasma => plasma::ions(ig).for_each(&mut each),
            Effect::Lightning => lightning::sparks(ig).for_each(&mut each),
            Effect::Ice => ice::shards(ig).for_each(&mut each),
            Effect::Ufo => ufo::puffs(ig).for_each(&mut each),
            Effect::Orb => orb::sparks(self).for_each(&mut each),
            Effect::Liquid => {}
        }
    }
}

/// **The signed distance from `p` to a quad**, negative inside it by the
/// non-zero winding rule: `fx_distance` in the shader, which an effect
/// measures its reach by.
pub fn distance(p: Pt, c: &Corners) -> f32 {
    #[cfg(test)]
    crate::det::tally::count(&crate::det::tally::DISTANCES);
    let q = c.0;
    let (mut d, mut winding) = (f32::INFINITY, 0i32);
    for i in 0..4 {
        let (a, b) = (q[i], q[(i + 1) % 4]);
        let (bx, by) = (b.x - a.x, b.y - a.y);
        let (px, py) = (p.x - a.x, p.y - a.y);
        let h = ((px * bx + py * by) / (bx * bx + by * by).max(1e-6)).clamp(0.0, 1.0);
        let (dx, dy) = (px - bx * h, py - by * h);
        d = d.min((dx * dx + dy * dy).sqrt());
        let side = bx * py - by * px;
        if a.y <= p.y {
            if b.y > p.y && side > 0.0 {
                winding += 1;
            }
        } else if b.y <= p.y && side < 0.0 {
            winding -= 1;
        }
    }
    if winding != 0 { -d } else { d }
}

fn min4(v: [f32; 4]) -> f32 {
    v.iter().copied().fold(f32::INFINITY, f32::min)
}

fn max4(v: [f32; 4]) -> f32 {
    v.iter().copied().fold(f32::NEG_INFINITY, f32::max)
}

/// A pixel count as a float.
fn px(v: Px) -> f32 {
    f32::from(u16::try_from(v.value()).unwrap_or(u16::MAX))
}

/// A whole-pixel position as a record's: `as i64` is the widening the audit
/// permits, and anything past `i16` is off every frame anyway.
fn screen(v: f32) -> Screen {
    Screen(i16::try_from(v as i64).unwrap_or(if v < 0.0 { i16::MIN } else { i16::MAX }))
}

fn tex(v: f32) -> Tex {
    Tex(u16::try_from(v.max(0.0) as i64).unwrap_or(u16::MAX))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::trail::{Segment, Shape, TrailLook};

    pub(crate) const CELL: (f32, f32) = (17.0, 34.0);

    pub(crate) fn cell(col: f32, row: f32) -> Rect {
        Rect {
            left: col * CELL.0,
            top: row * CELL.1,
            right: (col + 1.0) * CELL.0,
            bottom: (row + 1.0) * CELL.1,
        }
    }

    pub(crate) fn ignition(from: Rect, to: Rect, seed: Seed) -> Ignition {
        Ignition::ignite(from.center(), to, seed, CELL.1)
    }

    /// **A look lifted by a slide moves every piece by the pan**, and only
    /// what is a position: the corners, the hole, the path's tail and its
    /// ride; not its direction, length, seed, age or unit, nor the axis --
    /// for every effect and for a smear. The ghost is in `trail.rs`.
    #[test]
    fn a_translated_look_moves_every_piece_by_the_same_amount() {
        let (from, to) = (cell(10.0, 8.0), cell(40.0, 2.0));
        let seg = Segment::new(from.corners(), to);
        let pan = Px::of(13);
        let dy = 13.0;
        let moved = |a: Pt, b: Pt| a.x == b.x && b.y - a.y == dy;
        let rect_moved = |a: Rect, b: Rect| {
            a.left == b.left
                && a.right == b.right
                && b.top - a.top == dy
                && b.bottom - a.bottom == dy
        };
        let smear = Smear {
            corners: seg.at(0.05),
            hole: to,
            shape: Shape::Quad,
            strength: crate::flash::Strength::Full,
        };
        let TrailLook::Smear(s) = TrailLook::Smear(smear).lifted(pan) else {
            panic!("a smear lifts to a smear");
        };
        assert!(
            smear
                .corners
                .0
                .iter()
                .zip(s.corners.0)
                .all(|(a, b)| moved(*a, b))
        );
        assert!(rect_moved(smear.hole, s.hole));
        assert_eq!(s.shape, smear.shape);
        for effect in Effect::ALL {
            let ig = ignition(from, to, Seed(5)).at(0.1);
            let vfx = Vfx {
                effect,
                emitter: smear,
                ignition: ig,
                axis: Pt { x: 0.6, y: -0.8 },
            };
            let TrailLook::Vfx(v) = TrailLook::Vfx(vfx).lifted(pan) else {
                panic!("an effect lifts to an effect");
            };
            assert!(
                vfx.emitter
                    .corners
                    .0
                    .iter()
                    .zip(v.emitter.corners.0)
                    .all(|(a, b)| moved(*a, b)),
                "{effect:?}"
            );
            assert!(rect_moved(vfx.emitter.hole, v.emitter.hole), "{effect:?}");
            assert!(moved(ig.tail, v.ignition.tail), "{effect:?}");
            assert_eq!(v.ignition.rode - ig.rode, dy, "{effect:?}");
            assert_eq!(
                (v.ignition.dir, v.ignition.len, v.ignition.age),
                (ig.dir, ig.len, ig.age)
            );
            assert_eq!((v.ignition.seed, v.ignition.unit), (ig.seed, ig.unit));
            assert_eq!(v.axis, vfx.axis, "{effect:?}");
            assert_eq!(v.effect, effect);
        }
        // Zero is the identity.
        assert_eq!(
            TrailLook::Smear(smear).lifted(Px::of(0)),
            TrailLook::Smear(smear)
        );
    }

    /// **A fire that never rode reads no offset**: its fourth number is the
    /// ride, zero from ignition, so its uniform is the one it always was; and
    /// a translation is what moves it.
    #[test]
    fn a_fire_that_never_rode_reads_no_offset() {
        let (from, to) = (cell(10.0, 8.0), cell(40.0, 2.0));
        let seg = Segment::new(from.corners(), to);
        let vfx = Vfx {
            effect: Effect::Fire,
            emitter: Smear {
                corners: seg.at(0.05),
                hole: to,
                shape: Shape::Quad,
                strength: crate::flash::Strength::Full,
            },
            ignition: ignition(from, to, Seed(5)).at(0.05),
            axis: Pt { x: 1.0, y: 0.0 },
        };
        assert_eq!(
            vfx.params(),
            [
                fire::HEIGHT,
                fire::RISE,
                fire::DRIFT,
                0.0,
                0.0,
                0.0,
                0.0,
                0.0
            ]
        );
        assert_eq!(vfx.translated(-34.0).params()[3], -34.0);
        assert_eq!(vfx.translated(-34.0).translated(12.0).params()[3], -22.0);
    }

    /// **Every pixel an effect can reach is in a band**, for every effect.
    /// The fragment function draws only inside the records, and the oracle's
    /// reference composites only inside them too, so a band that fell short
    /// would clip the effect in both arms and the reference alike and pass.
    /// Every pixel of the frame within the effect's reach of the quad -- a
    /// hop, a diagonal, a long vertical and a move up and left, mid-chase,
    /// the diagonal's quad stretched and skewed -- must lie in some band.
    #[test]
    fn every_pixel_within_reach_is_in_a_band() {
        let frame = (Px::of(2040), Px::of(340));
        for effect in Effect::ALL {
            for (from, to, t) in [
                (cell(10.0, 4.0), cell(40.0, 4.0), 0.05),
                (cell(5.0, 0.0), cell(100.0, 9.0), 0.08),
                (cell(60.0, 0.0), cell(62.0, 9.0), 0.03),
                (cell(90.0, 8.0), cell(20.0, 1.0), 0.12),
            ] {
                let seg = Segment::new(from.corners(), to);
                let vfx = Vfx {
                    effect,
                    emitter: Smear {
                        corners: seg.at(t),
                        hole: to,
                        shape: Shape::Quad,
                        strength: crate::flash::Strength::Full,
                    },
                    ignition: ignition(from, to, Seed(3)).at(t),
                    axis: ignition(from, to, Seed(3)).dir,
                };
                let mut bands = Vec::new();
                vfx.instances(frame, Argb(0xFF10_2030), &mut bands);
                bands.retain(|r| r.kind == Kind::Band);
                assert!(!bands.is_empty(), "{effect:?}: no band");
                let (mut near, mut missed) = (0usize, 0usize);
                for y in 0..340 {
                    for x in 0..2040 {
                        let p = Pt {
                            x: x as f32 + 0.5,
                            y: y as f32 + 0.5,
                        };
                        let mut d = f32::INFINITY;
                        vfx.support(|c| d = d.min(distance(p, &c)));
                        if d > effect.reach() * CELL.1 {
                            continue;
                        }
                        near += 1;
                        let covered = bands.iter().any(|r| {
                            let (x0, y0) = (i32::from(r.x.0), i32::from(r.y.0));
                            x >= x0
                                && x < x0 + i32::from(r.w.0)
                                && y >= y0
                                && y < y0 + i32::from(r.h.0)
                        });
                        missed += usize::from(!covered);
                    }
                }
                assert!(near > 1000, "{effect:?}: the reach holds {near} pixels");
                assert_eq!(
                    missed, 0,
                    "{effect:?}: {missed} of {near} pixels within reach have no band"
                );
            }
        }
    }

    /// **The bands tile the rows**: each band record starts on the row the
    /// one before it ends, so every pixel row is in one band and none is
    /// drawn twice -- for every effect, mid-chase on a hop, a diagonal, a
    /// long vertical and a move up and left, at a unit of whole pixels and
    /// at units that put edges between rows, as the launch logo's does
    /// (`each_pixel_of_the_plasma_is_in_one_band` holds its records). At a
    /// whole unit, which every cursor trail is lit at (the cell's height),
    /// each band is a unit of rows on the unit's grid, as it was before the
    /// edges were shared. Perturbations: a record's top
    /// rounded down and its bottom up overlaps a row at each fractional
    /// edge; a top rounded up and a bottom down leaves one out.
    #[test]
    fn the_bands_tile_the_rows_at_any_unit() {
        let frame = (Px::of(2040), Px::of(340));
        let units = [CELL.1, 33.6, 17.25, 28.8];
        let mut edges = 0usize;
        for unit in units {
            for effect in Effect::ALL {
                for (from, to, t) in [
                    (cell(10.0, 4.0), cell(40.0, 4.0), 0.05),
                    (cell(5.0, 0.0), cell(100.0, 9.0), 0.08),
                    (cell(60.0, 0.0), cell(62.0, 9.0), 0.03),
                    (cell(90.0, 8.0), cell(20.0, 1.0), 0.12),
                ] {
                    let lit = Ignition::ignite(from.center(), to, Seed(3), unit);
                    let vfx = Vfx {
                        effect,
                        emitter: Smear {
                            corners: Segment::new(from.corners(), to).at(t),
                            hole: to,
                            shape: Shape::Quad,
                            strength: crate::flash::Strength::Full,
                        },
                        ignition: lit.at(t),
                        axis: lit.dir,
                    };
                    let mut bands = Vec::new();
                    vfx.instances(frame, Argb(0xFF10_2030), &mut bands);
                    bands.retain(|r| r.kind == Kind::Band);
                    let what = format!("{effect:?} at a unit of {unit}, {from:?} to {to:?}");
                    assert!(!bands.is_empty(), "{what}: no band");
                    for p in bands.windows(2) {
                        let end = i32::from(p[0].y.0) + i32::from(p[0].h.0);
                        assert_eq!(
                            i32::from(p[1].y.0),
                            end,
                            "{what}: {:?} then {:?}",
                            p[0],
                            p[1]
                        );
                        edges += 1;
                    }
                    if unit.fract() == 0.0 {
                        let u = unit as i32;
                        let last = bands.len() - 1;
                        for (i, b) in bands.iter().enumerate() {
                            assert_eq!(i32::from(b.y.0) % u, 0, "{what}: {b:?}");
                            let clipped = i == last && i32::from(b.y.0) + i32::from(b.h.0) == 340;
                            assert!(i32::from(b.h.0) == u || clipped, "{what}: {b:?}");
                        }
                    }
                }
            }
        }
        assert!(edges > 100, "{edges} band edges");
    }

    /// **The records are the cursor's colour**, for every effect. The
    /// oracle's reference reads each record's colour, so a band or a
    /// particle built in the wrong one would draw wrong in both arms and the
    /// reference alike: the bands are the fill, opaque, and each particle is
    /// the fill at its glow, one record a live particle, after the bands.
    #[test]
    fn the_records_are_the_cursors_colour() {
        for effect in Effect::ALL {
            let (from, to) = (cell(10.0, 5.0), cell(40.0, 5.0));
            let t = 0.06;
            let vfx = Vfx {
                effect,
                emitter: Smear {
                    corners: Segment::new(from.corners(), to).at(t),
                    hole: to,
                    shape: Shape::Quad,
                    strength: crate::flash::Strength::Full,
                },
                ignition: ignition(from, to, Seed(9)).at(t),
                axis: ignition(from, to, Seed(9)).dir,
            };
            let fill = Argb(0x80C0_4020);
            let mut records = Vec::new();
            vfx.instances((Px::of(2040), Px::of(340)), fill, &mut records);
            let mut particles = Vec::new();
            vfx.particles(|p| particles.push(p));
            let (bands, motes) = records.split_at(records.len() - particles.len());
            assert!(!bands.is_empty(), "{effect:?}: no band");
            for b in bands {
                assert_eq!(b.kind, Kind::Band);
                assert_eq!(b.color, fill.with_alpha(Chan::from_u8(0xFF)));
            }
            for (m, p) in motes.iter().zip(&particles) {
                assert_eq!(m.kind, Kind::Particle);
                assert_eq!(m.color, fill.with_alpha(Chan::from_unit(p.glow)));
            }
        }
    }

    /// **The uniform is `EffectU`'s layout**, for every effect: 32 floats,
    /// the struct's 128 bytes, with the seed's bits, the life, the reach and
    /// the effect's own numbers where the shader reads them. The oracle's
    /// reference catches a field the shader misreads; this names which.
    #[test]
    fn the_uniform_is_effect_us_layout() {
        for effect in Effect::ALL {
            let to = cell(40.0, 5.0);
            let vfx = Vfx {
                effect,
                emitter: Smear {
                    corners: to.corners(),
                    hole: to,
                    shape: Shape::Quad,
                    strength: crate::flash::Strength::Full,
                },
                ignition: ignition(cell(10.0, 5.0), to, Seed(0xDEAD_BEEF)).at(0.1),
                axis: Pt { x: 1.0, y: 0.0 },
            };
            let u = vfx.uniform([3.0, 5.0]);
            assert_eq!(core::mem::size_of_val(&u), 128);
            assert_eq!(&u[8..12], &[to.left, to.top, to.right, to.bottom]);
            assert_eq!(&u[12..14], &[3.0, 5.0]);
            assert_eq!(u[20], 0.1);
            assert_eq!(u[21], effect.life());
            assert_eq!(u[22].to_bits(), 0xDEAD_BEEF);
            assert_eq!(u[23], effect.reach());
            assert_eq!(&u[24..32], &vfx.params());
        }
    }

    /// **Every effect is in `ALL` once**, which a GPU path's pipeline array
    /// is indexed by, and every effect names a fragment function of its own.
    #[test]
    fn every_effect_has_its_place_and_its_fragment() {
        for e in Effect::ALL {
            assert_eq!(
                Effect::ALL.iter().filter(|&&x| x == e).count(),
                1,
                "{e:?} is in ALL once"
            );
            assert!(e.fragment().ends_with("_fragment"), "{e:?}");
        }
        let mut names: Vec<&str> = Effect::ALL.iter().map(|e| e.fragment()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), Effect::COUNT);
    }
}
