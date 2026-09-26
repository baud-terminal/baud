//! **The fire trail**: a procedural flame rising off kitty's collapsing quad,
//! and embers the emitter sheds along its path -- the research's "burning
//! fuse", the owner's default trail since 2026-09-25 (note V). GPU only: a
//! flame costs the CPU 5-33 ms a frame, so the CPU path draws the emitter as
//! the quad it is.
//!
//! **Everything here is a pure function of the ignition and its age.** The
//! driver hands a [`Fire`] -- the emitter's corners this frame, the path the
//! move took, the seconds since ignition and the ignition's seed -- and this
//! derives the records and the uniform `fire_fragment` reads. No state
//! between frames and no clock, so the pixel oracle compares two GPU arms on
//! it and a Rust reference can hold the shader to its formulas.
//!
//! **The flame is the cursor's colour** -- the one the quad trail is drawn
//! in, the owner's call -- and heat is its opacity: a hot core is the cursor,
//! a tongue's tip and the halo are translucent. Premultiplied, that is valid
//! over a glass window and dark over a light theme, whose cursor is dark, so
//! one rule serves every ground.
//!
//! **Rust owns the numbers.** The flame's height, reach, rise and drift ride
//! in the uniform, so the shader and the embers built here read one spelling
//! of each.

use crate::atlas::{Kind, Tex};
use crate::color::Argb;
use crate::instance::{Instance, Screen};
use crate::trail::{Pt, Rect, Smear};
use baud_domain::Chan;
use baud_font::Px;

/// Seconds the flame burns, from ignition: the tail burns out first and the
/// head, at the cursor, last.
pub const LIFE: f32 = 0.45;

/// Cells a tongue of flame can climb above the emitter.
pub const HEIGHT: f32 = 1.6;

/// Cells from the emitter past which the flame draws nothing: a tongue lifts
/// a sample at most [`HEIGHT`] cells, and the halo is under half a step of
/// 255 by then. `fire_fragment` rejects a pixel this far out before any noise,
/// and the records cover exactly this reach.
pub const REACH: f32 = HEIGHT + 0.4;

/// Cells a second the flame's pattern climbs.
pub const RISE: f32 = 2.2;

/// Cells a second the pattern trails behind the move.
pub const DRIFT: f32 = 0.6;

/// An ignition's seed: every hash the flame and its embers draw from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Seed(pub u32);

/// A hash word, advanced by PCG's output permutation over one step of its
/// LCG (O'Neill 2014), which Jarzynski and Olano recommend as the GPU hash
/// (`trail/effects/papers/hash.txt` 360-366). Written from the published
/// algorithm; the shader's `hash` is the same function.
#[derive(Clone, Copy)]
struct Hash(u32);

impl Hash {
    fn next(self) -> Hash {
        let s = self.0.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
        let w = ((s >> ((s >> 28) + 4)) ^ s).wrapping_mul(277_803_737);
        Hash((w >> 22) ^ w)
    }

    /// A uniform number in [0, 1) from the word's top 23 bits: the mantissa
    /// of a float in [1, 2), less one -- exact, with no integer conversion.
    fn unit(self) -> f32 {
        f32::from_bits((self.0 >> 9) | 0x3f80_0000) - 1.0
    }
}

/// **One ignition of the flame**, as a frame sees it: the path the move took
/// -- from the centre of where the cursor was to the centre of where it
/// went -- the seconds since the ignition, its seed, and the flame's unit,
/// the cell's height in pixels.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Flame {
    pub tail: Pt,
    /// The unit vector from the tail to the head.
    pub dir: Pt,
    /// The distance from the tail to the head, pixels.
    pub len: f32,
    pub age: f32,
    pub seed: Seed,
    pub unit: f32,
}

impl Flame {
    /// The flame of a move from `tail` to the centre of `to`, at age 0.
    pub fn ignite(tail: Pt, to: Rect, seed: Seed, unit: f32) -> Flame {
        let head = to.center();
        let (dx, dy) = (head.x - tail.x, head.y - tail.y);
        let len = dx.hypot(dy).max(1.0);
        Flame {
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
    pub fn at(self, age: f32) -> Flame {
        Flame { age, ..self }
    }

    /// **Seconds from ignition to when the fire is out**: the flame's life,
    /// or the last ember's death if that is later.
    pub fn end(&self) -> f32 {
        self.embers()
            .map(|h| {
                let e = Ember::draw(h);
                e.born + e.life
            })
            .fold(LIFE, f32::max)
    }

    /// The embers the path sheds, each as its first hash: eight a unit of
    /// path -- the cell's height, so about 128 for a thirty-column hop, what
    /// the research rendered the look with -- between 16 and 160, ember `i`
    /// drawing from the seed with the index spread by the golden ratio's
    /// bits, so neighbours are unrelated.
    fn embers(&self) -> impl Iterator<Item = Hash> {
        let n = (self.len / self.unit.max(1.0) * EMBERS_PER_UNIT).clamp(16.0, 160.0);
        let seed = self.seed.0;
        (0..u32::try_from(n.round() as i64).unwrap_or(16))
            .map(move |i| Hash(seed ^ i.wrapping_mul(0x9E37_79B9)).next())
    }
}

/// Embers a unit of path sheds.
const EMBERS_PER_UNIT: f32 = 8.0;

/// Seconds the emitter takes to pass the path, which is when an ember is
/// born at its place along it.
const SWEEP: f32 = 0.10;

/// An ember's drag a second, and its buoyancy, cells a second squared
/// upward: it leaves fast, slows, and floats.
const DRAG: f32 = 3.0;
const BUOYANCY: f32 = 14.0;

/// **One ember's draw from the ignition's hashes**: where along the path it
/// is born and when, how long it lives, and the rest of its hashes.
struct Ember {
    along: f32,
    born: f32,
    life: f32,
    h: [Hash; 5],
}

impl Ember {
    fn draw(h0: Hash) -> Ember {
        let (h1, h2, h3, h4) = {
            let h1 = h0.next();
            let h2 = h1.next();
            let h3 = h2.next();
            (h1, h2, h3, h3.next())
        };
        let along = h0.unit();
        Ember {
            along,
            born: along * SWEEP,
            life: 0.30 + 0.45 * h1.unit(),
            h: [h0, h1, h2, h3, h4],
        }
    }
}

/// **A live ember at the flame's age**: its centre and radius in frame
/// pixels, and its glow, 1 when born fading to nothing as it dies -- the
/// opacity it draws the cursor's colour at.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Spark {
    pub centre: Pt,
    pub radius: f32,
    pub glow: f32,
}

/// **The embers alive at the flame's age**, each in closed form: born where
/// the emitter passes, thrown up and sideways, slowed by drag, lifted by
/// buoyancy, fluttering, `p = p0 + v0 (1 - e^-ka)/k + (g/k)(a - (1 - e^-ka)/k)`
/// -- the research's proposal B, a pure function of the seed and the age.
pub fn sparks(flame: &Flame) -> impl Iterator<Item = Spark> + '_ {
    flame.embers().filter_map(move |h| Ember::draw(h).at(flame))
}

impl Ember {
    /// This ember at the flame's age, or none before it is born or after it
    /// dies.
    fn at(&self, flame: &Flame) -> Option<Spark> {
        let a = flame.age - self.born;
        if a < 0.0 || a > self.life {
            return None;
        }
        let u = flame.unit;
        let perp = Pt {
            x: -flame.dir.y,
            y: flame.dir.x,
        };
        let carry = (flame.len / SWEEP).min(40.0 * u) * 0.05;
        let [_, h1, h2, h3, h4] = self.h;
        let side = (h2.unit() - 0.5) * u * 0.8;
        let p0 = Pt {
            x: flame.tail.x + flame.dir.x * flame.len * self.along + perp.x * side,
            y: flame.tail.y + flame.dir.y * flame.len * self.along + perp.y * side,
        };
        let v0 = Pt {
            x: (h3.unit() - 0.5) * 2.0 * u + flame.dir.x * carry,
            y: -(3.0 + 5.0 * h4.unit()) * u + flame.dir.y * carry,
        };
        let s = (1.0 - (-DRAG * a).exp()) / DRAG;
        let lift = BUOYANCY * u / DRAG * (a - s);
        let phase = core::f32::consts::TAU * Hash(h1.0 ^ 0x5bd1_e995).unit();
        let heat = 1.0 - a / self.life;
        Some(Spark {
            centre: Pt {
                x: p0.x + v0.x * s + 0.35 * u * (11.0 * a + phase).sin(),
                y: p0.y + v0.y * s - lift,
            },
            radius: u * (0.05 + 0.06 * h2.unit()) * (0.6 + 0.4 * heat),
            glow: heat * (0.55 + 0.45 * heat),
        })
    }
}

/// **A fire as the renderer holds it for a frame**: the emitter -- kitty's
/// quad this frame, whose hole is the cursor -- and the flame it gives off.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Fire {
    pub emitter: Smear,
    pub flame: Flame,
}

/// The floats in the uniform `fire_fragment` reads, `FireU` in
/// `shaders.metal`.
pub const UNIFORM_FLOATS: usize = 28;

impl Fire {
    /// **The records a GPU path draws the fire with**, in `fill`, the
    /// cursor's colour: one band a cell row over the emitter's reach -- the
    /// pixels within [`REACH`] cells of it, which is everywhere
    /// `fire_fragment` can draw -- clipped to the frame, then one soft disc a
    /// live ember, at its glow.
    pub fn instances(&self, frame: (Px, Px), fill: Argb, out: &mut Vec<Instance>) {
        let unit = self.flame.unit.max(1.0);
        let r = REACH * unit;
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
                    kind: Kind::Fire,
                });
            }
            band += unit;
        }
        for s in sparks(&self.flame) {
            let side = (2.0 * s.radius).ceil().max(2.0);
            out.push(Instance {
                x: screen((s.centre.x - side * 0.5).floor()),
                y: screen((s.centre.y - side * 0.5).floor()),
                w: tex(side),
                h: tex(side),
                u: Tex(0),
                v: Tex(0),
                color: fill.with_alpha(Chan::from_unit(s.glow)),
                kind: Kind::Ember,
            });
        }
    }

    /// **The fire as `fire_fragment` reads it**, `FireU` in `shaders.metal`:
    /// the emitter's corners and hole, where the frame sits in the target,
    /// the path, the unit, the age, the life, the seed's bits, then the
    /// flame's height, reach, rise and drift, and a float of padding to the
    /// struct's 16-byte alignment.
    pub fn uniform(&self, origin: [f32; 2]) -> [f32; UNIFORM_FLOATS] {
        let c = self.emitter.corners.0;
        let h = self.emitter.hole;
        let f = self.flame;
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
            LIFE,
            f32::from_bits(f.seed.0),
            HEIGHT,
            REACH,
            RISE,
            DRIFT,
            0.0,
        ]
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
mod tests {
    use super::*;
    use crate::trail::{Segment, Shape};

    const CELL: (f32, f32) = (17.0, 34.0);

    fn cell(col: f32, row: f32) -> Rect {
        Rect {
            left: col * CELL.0,
            top: row * CELL.1,
            right: (col + 1.0) * CELL.0,
            bottom: (row + 1.0) * CELL.1,
        }
    }

    fn flame(from: Rect, to: Rect, seed: Seed) -> Flame {
        Flame::ignite(from.center(), to, seed, CELL.1)
    }

    /// The signed distance from `p` to the quad, negative inside by the
    /// non-zero rule -- `fire_distance`, spelled once more for the test.
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

    /// **Every pixel the flame can reach is in a band.** `fire_fragment`
    /// draws only inside the records, and the oracle's reference composites
    /// only inside them too, so a band that fell short would clip the flame
    /// in both arms and the reference alike and pass. Every pixel of the
    /// frame within [`REACH`] cells of the quad -- a hop, a diagonal and a
    /// long vertical, mid-chase, the diagonal's quad stretched and skewed --
    /// must lie in some band.
    #[test]
    fn every_pixel_within_reach_is_in_a_band() {
        let frame = (Px::of(2040), Px::of(340));
        for (from, to, t) in [
            (cell(10.0, 4.0), cell(40.0, 4.0), 0.05),
            (cell(5.0, 0.0), cell(100.0, 9.0), 0.08),
            (cell(60.0, 0.0), cell(62.0, 9.0), 0.03),
            (cell(90.0, 8.0), cell(20.0, 1.0), 0.12),
        ] {
            let seg = Segment::new(from.corners(), to);
            let fire = Fire {
                emitter: Smear {
                    corners: seg.at(t),
                    hole: to,
                    shape: Shape::Quad,
                },
                flame: flame(from, to, Seed(3)).at(t),
            };
            let mut bands = Vec::new();
            fire.instances(frame, Argb(0xFF10_2030), &mut bands);
            bands.retain(|r| r.kind == Kind::Fire);
            assert!(!bands.is_empty());
            let (mut near, mut missed) = (0usize, 0usize);
            for y in 0..340 {
                for x in 0..2040 {
                    let p = Pt {
                        x: x as f32 + 0.5,
                        y: y as f32 + 0.5,
                    };
                    if distance(p, &fire.emitter.corners) > REACH * CELL.1 {
                        continue;
                    }
                    near += 1;
                    let covered = bands.iter().any(|r| {
                        let (x0, y0) = (i32::from(r.x.0), i32::from(r.y.0));
                        x >= x0 && x < x0 + i32::from(r.w.0) && y >= y0 && y < y0 + i32::from(r.h.0)
                    });
                    missed += usize::from(!covered);
                }
            }
            assert!(near > 1000, "the reach holds {near} pixels");
            assert_eq!(
                missed, 0,
                "{missed} of {near} pixels within reach have no band"
            );
        }
    }

    /// **An ember is born where the emitter passes and floats up.** Born
    /// within [`SWEEP`] of the ignition, at its place along the path, within
    /// half a cell of it either side; by half its life it has risen at least
    /// 0.4 of a cell, whatever way it was thrown -- the slowest, shortest-lived
    /// ember rises about half a cell by then; after its life it is gone.
    #[test]
    fn an_ember_is_born_where_the_emitter_passes_and_floats_up() {
        let f = flame(cell(10.0, 5.0), cell(40.0, 5.0), Seed(11));
        let mut seen = 0;
        for h in f.embers() {
            let e = Ember::draw(h);
            assert!(e.born >= 0.0 && e.born <= SWEEP, "born at {}", e.born);
            assert!((0.30..=0.75).contains(&e.life), "lives {}", e.life);
            assert_eq!(e.at(&f.at(e.born - 0.001)), None, "before its birth");
            assert_eq!(
                e.at(&f.at(e.born + e.life + 0.001)),
                None,
                "after its death"
            );
            let at_birth = e.at(&f.at(e.born)).expect("at its birth");
            let path = Pt {
                x: f.tail.x + f.dir.x * f.len * e.along,
                y: f.tail.y + f.dir.y * f.len * e.along,
            };
            assert!(
                (at_birth.centre.x - path.x).abs() <= 0.4 * CELL.1 + 0.001
                    && (at_birth.centre.y - path.y).abs() <= 0.4 * CELL.1 + 0.001,
                "born at {:?}, the path at {path:?}",
                at_birth.centre
            );
            let later = e
                .at(&f.at(e.born + e.life * 0.5))
                .expect("at half its life");
            assert!(
                later.centre.y < at_birth.centre.y - 0.4 * CELL.1,
                "risen: {later:?} from {at_birth:?}"
            );
            assert!(
                later.glow < at_birth.glow && later.radius < at_birth.radius,
                "cooling"
            );
            seen += 1;
        }
        assert_eq!(seen, 120, "eight embers a unit of a fifteen-unit path");
    }

    /// **The fire is out when its last ember dies**, and not before: the
    /// flame's life or the last death, whichever is later.
    #[test]
    fn the_fire_is_out_when_its_last_ember_dies() {
        for seed in [Seed(1), Seed(2), Seed(99)] {
            let f = flame(cell(10.0, 5.0), cell(40.0, 5.0), seed);
            let end = f.end();
            assert!((LIFE..=SWEEP + 0.75).contains(&end), "out at {end}");
            if end > LIFE {
                assert!(
                    sparks(&f.at(end - 0.001)).next().is_some(),
                    "an ember at the end"
                );
            }
            assert!(sparks(&f.at(end + 0.001)).next().is_none(), "none after it");
        }
    }

    /// **Eight embers a unit of path, between 16 and 160**, and the same
    /// seed sheds them alike where another does not.
    #[test]
    fn embers_are_eight_a_unit_and_the_seed_decides_them() {
        let count = |cells: f32| {
            flame(cell(0.0, 0.0), cell(cells * CELL.1 / CELL.0, 0.0), Seed(1))
                .embers()
                .count()
        };
        assert_eq!(count(1.0), 16);
        assert_eq!(count(10.0), 80);
        assert_eq!(count(100.0), 160);
        let shed = |seed: Seed| -> Vec<Spark> {
            sparks(&flame(cell(10.0, 5.0), cell(40.0, 5.0), seed).at(0.2)).collect()
        };
        assert_eq!(shed(Seed(4)), shed(Seed(4)));
        assert_ne!(shed(Seed(4)), shed(Seed(5)));
    }

    /// **The records are the cursor's colour.** The oracle's reference reads
    /// each record's colour, so a band or an ember built in the wrong one
    /// would draw wrong in both arms and the reference alike: the bands are
    /// the fill, opaque, and each ember is the fill at its spark's glow, one
    /// record a live spark, after the bands.
    #[test]
    fn the_records_are_the_cursors_colour() {
        let (from, to) = (cell(10.0, 5.0), cell(40.0, 5.0));
        let t = 0.06;
        let fire = Fire {
            emitter: Smear {
                corners: Segment::new(from.corners(), to).at(t),
                hole: to,
                shape: Shape::Quad,
            },
            flame: flame(from, to, Seed(9)).at(t),
        };
        let fill = Argb(0x80C0_4020);
        let mut records = Vec::new();
        fire.instances((Px::of(2040), Px::of(340)), fill, &mut records);
        let sparks: Vec<Spark> = sparks(&fire.flame).collect();
        let (bands, embers) = records.split_at(records.len() - sparks.len());
        assert!(!bands.is_empty() && !embers.is_empty());
        for b in bands {
            assert_eq!(b.kind, Kind::Fire);
            assert_eq!(b.color, fill.with_alpha(Chan::from_u8(0xFF)));
        }
        for (e, s) in embers.iter().zip(&sparks) {
            assert_eq!(e.kind, Kind::Ember);
            assert_eq!(e.color, fill.with_alpha(Chan::from_unit(s.glow)));
        }
    }

    /// **The uniform is `FireU`'s layout**: 28 floats, the struct's 112
    /// bytes, with the seed's bits and the constants where the shader reads
    /// them. The oracle's reference catches a field the shader misreads;
    /// this names which.
    #[test]
    fn the_uniform_is_fire_us_layout() {
        let to = cell(40.0, 5.0);
        let fire = Fire {
            emitter: Smear {
                corners: to.corners(),
                hole: to,
                shape: Shape::Quad,
            },
            flame: flame(cell(10.0, 5.0), to, Seed(0xDEAD_BEEF)).at(0.1),
        };
        let u = fire.uniform([3.0, 5.0]);
        assert_eq!(core::mem::size_of_val(&u), 112);
        assert_eq!(&u[8..12], &[to.left, to.top, to.right, to.bottom]);
        assert_eq!(&u[12..14], &[3.0, 5.0]);
        assert_eq!(u[20], 0.1);
        assert_eq!(u[21], LIFE);
        assert_eq!(u[22].to_bits(), 0xDEAD_BEEF);
        assert_eq!(&u[23..27], &[HEIGHT, REACH, RISE, DRIFT]);
    }
}
