//! **The effect trails**: a shader drawn over kitty's collapsing quad, lit by
//! a long move and burning out after it -- the fire, and the family the owner
//! asked for beside it. What they share lives here: the ignition a move
//! lights (the path, the age, the seed and the cell's height), the records a
//! GPU path draws an effect with -- one band a cell row over the effect's
//! reach, then one soft disc a live particle -- and the uniform its fragment
//! function reads. What each effect draws is its own module: [`crate::fire`].
//!
//! **Everything here is a pure function of the ignition and its age.** The
//! driver hands a [`Vfx`] -- which effect, the emitter's corners this frame,
//! and the ignition at its age -- and this derives the records and the
//! uniform. No state between frames and no clock, so the pixel oracle
//! compares two GPU arms on it and a Rust reference can hold each shader to
//! its formulas.
//!
//! **Every effect is the cursor's colour** -- the one the quad trail is drawn
//! in, the owner's call -- and its intensity is its opacity. Premultiplied,
//! that is valid over a glass window and dark over a light theme, whose
//! cursor is dark, so one rule serves every ground. GPU only: the CPU path
//! draws the emitter as the quad it is.

use crate::atlas::{Kind, Tex};
use crate::color::Argb;
use crate::fire;
use crate::instance::{Instance, Screen};
use crate::trail::{Pt, Rect, Smear};
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
}

impl Effect {
    /// Every effect, in the order a GPU path indexes its pipelines by.
    pub const ALL: [Effect; 1] = [Effect::Fire];

    /// How many effects there are: a GPU path keeps one pipeline each.
    pub const COUNT: usize = Effect::ALL.len();

    /// The fragment function a GPU path draws this effect's records with.
    pub const fn fragment(self) -> &'static str {
        match self {
            Effect::Fire => "fire_fragment",
        }
    }

    /// Cells from the emitter past which the effect draws nothing: its
    /// fragment function rejects a pixel this far out, and the bands cover
    /// exactly this reach.
    pub const fn reach(self) -> f32 {
        match self {
            Effect::Fire => fire::REACH,
        }
    }

    /// Seconds the effect's body lasts from ignition; its particles may
    /// outlive it ([`Effect::end`]).
    pub const fn life(self) -> f32 {
        match self {
            Effect::Fire => fire::LIFE,
        }
    }

    /// The effect's own numbers, `k` in the shader's `EffectU`: Rust owns
    /// them, so the shader and the particles built here read one spelling.
    pub const fn params(self) -> [f32; PARAMS] {
        match self {
            Effect::Fire => fire::PARAMS,
        }
    }

    /// **Seconds from ignition to when the effect is out**: its life, or its
    /// last particle's death if that is later.
    pub fn end(self, ignition: &Ignition) -> f32 {
        match self {
            Effect::Fire => fire::end(ignition),
        }
    }

    /// Every particle alive at the ignition's age, in order.
    pub fn particles(self, ignition: &Ignition, mut each: impl FnMut(Particle)) {
        match self {
            Effect::Fire => fire::embers(ignition).for_each(&mut each),
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
}

impl Ignition {
    /// The ignition of a move from `tail` to the centre of `to`, at age 0.
    pub fn ignite(tail: Pt, to: Rect, seed: Seed, unit: f32) -> Ignition {
        let head = to.center();
        let (dx, dy) = (head.x - tail.x, head.y - tail.y);
        let len = dx.hypot(dy).max(1.0);
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
        }
    }

    /// The same ignition `age` seconds after it.
    pub fn at(self, age: f32) -> Ignition {
        Ignition { age, ..self }
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
/// emitter -- kitty's quad this frame, whose hole is the cursor -- and the
/// ignition it gives off.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Vfx {
    pub effect: Effect,
    pub emitter: Smear,
    pub ignition: Ignition,
}

/// The floats in an effect's own numbers, `k` in `EffectU`.
pub const PARAMS: usize = 8;

/// The floats in the uniform an effect's fragment function reads, `EffectU`
/// in `shaders.metal`: a shared prefix, then [`PARAMS`].
pub const UNIFORM_FLOATS: usize = 24 + PARAMS;

impl Vfx {
    /// **The records a GPU path draws the effect with**, in `fill`, the
    /// cursor's colour: one band a cell row over the emitter's reach -- the
    /// pixels within [`Effect::reach`] cells of it, which is everywhere the
    /// effect's fragment function can draw -- clipped to the frame, then one
    /// soft disc a live particle, at its glow.
    pub fn instances(&self, frame: (Px, Px), fill: Argb, out: &mut Vec<Instance>) {
        let unit = self.ignition.unit.max(1.0);
        let r = self.effect.reach() * unit;
        let (fw, fh) = (px(frame.0), px(frame.1));
        let q = self.emitter.corners.0;
        let ys = q.map(|p| p.y);
        let y0 = (min4(ys) - r).max(0.0);
        let y1 = (max4(ys) + r).min(fh);
        let mut band = (y0 / unit).floor() * unit;
        while band < y1 {
            let (lo, hi) = (band - r, band + unit + r);
            let (mut xmin, mut xmax) = (f32::INFINITY, f32::NEG_INFINITY);
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
            let (x0, x1) = ((xmin - r).max(0.0).floor(), (xmax + r).min(fw).ceil());
            let (top, bottom) = (band.max(0.0).floor(), (band + unit).min(fh).ceil());
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
                });
            }
            band += unit;
        }
        self.effect.particles(&self.ignition, |s| {
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
            });
        });
    }

    /// **The effect as its fragment function reads it**, `EffectU` in
    /// `shaders.metal`: the emitter's corners and hole, where the frame sits
    /// in the target, the path, the unit, the age, the life, the seed's bits
    /// and the reach, then the effect's own numbers.
    pub fn uniform(&self, origin: [f32; 2]) -> [f32; UNIFORM_FLOATS] {
        let c = self.emitter.corners.0;
        let h = self.emitter.hole;
        let f = self.ignition;
        let k = self.effect.params();
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

    /// Seconds from ignition to when this effect is out.
    pub fn end(&self) -> f32 {
        self.effect.end(&self.ignition)
    }
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
    use crate::trail::{Segment, Shape};

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

    /// The signed distance from `p` to the quad, negative inside by the
    /// non-zero rule -- `fx_distance`, spelled once more for the test.
    fn distance(p: Pt, c: &crate::trail::Corners) -> f32 {
        let q = c.0;
        let (mut d, mut winding) = (f32::INFINITY, 0i32);
        for i in 0..4 {
            let (a, b) = (q[i], q[(i + 1) % 4]);
            let (bx, by) = (b.x - a.x, b.y - a.y);
            let (px, py) = (p.x - a.x, p.y - a.y);
            let h = ((px * bx + py * by) / (bx * bx + by * by).max(1e-6)).clamp(0.0, 1.0);
            d = d.min((px - bx * h).hypot(py - by * h));
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
                    },
                    ignition: ignition(from, to, Seed(3)).at(t),
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
                        if distance(p, &vfx.emitter.corners) > effect.reach() * CELL.1 {
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
                },
                ignition: ignition(from, to, Seed(9)).at(t),
            };
            let fill = Argb(0x80C0_4020);
            let mut records = Vec::new();
            vfx.instances((Px::of(2040), Px::of(340)), fill, &mut records);
            let mut particles = Vec::new();
            effect.particles(&vfx.ignition, |p| particles.push(p));
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
                },
                ignition: ignition(cell(10.0, 5.0), to, Seed(0xDEAD_BEEF)).at(0.1),
            };
            let u = vfx.uniform([3.0, 5.0]);
            assert_eq!(core::mem::size_of_val(&u), 128);
            assert_eq!(&u[8..12], &[to.left, to.top, to.right, to.bottom]);
            assert_eq!(&u[12..14], &[3.0, 5.0]);
            assert_eq!(u[20], 0.1);
            assert_eq!(u[21], effect.life());
            assert_eq!(u[22].to_bits(), 0xDEAD_BEEF);
            assert_eq!(u[23], effect.reach());
            assert_eq!(&u[24..32], &effect.params());
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
