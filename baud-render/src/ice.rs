//! **The ice trail**, a rime fuse: a frost lace of six-fold stellar
//! crystals laid along the path as kitty's leading corners pass, a patchy
//! rime haze round them, a faint pane of ice over the quad as it slides. The
//! lace holds with one soft glint moving tail to head, then sublimates tail
//! first, shedding hexagonal platelets -- diamond dust -- that pop up and
//! drift down. Designed in the effect-trail research (the ice spec), in the
//! cursor's colour with intensity as opacity.
//!
//! **The frost is laid on the path, not on the quad.** A point of the path
//! is inside kitty's quad for 10-40 ms at mid-path, too short for a crystal
//! to grow in 0.12 s, so the lace is drawn over the path's capsule as well as
//! the quad ([`Vfx::support`](crate::effect::Vfx::support)).
//!
//! **A crystal is a fold, not a table.** The pixel is rotated by a hashed
//! angle through the tangent half-angle form and folded into one 30-degree
//! wedge by two reflections, so six arms, their side branches at 60 degrees
//! and a hexagonal plate are one arm's distance -- no trigonometry anywhere
//! in the frost. Crystals sit on a one-dimensional lattice along the path,
//! and a pixel reads the three nearest.
//!
//! This module is the Rust model of the shader, `ice_frost` and the shard's
//! plate, which the unit tests hold to their properties and the pixel
//! oracle's reference calls.

use crate::det::Det;
use crate::effect::{self, Hash, Ignition, Particle, Vfx};
use crate::trail::{Pt, TAU_FAST};

/// Seconds from ignition to when the last crystal, at the head, is gone:
/// the hold, the melt's spread along the path, a crystal's melt and its
/// hashed delay.
pub const LIFE: f32 = HOLD + SPREAD + MELT + 0.03;

/// Cells from the path, or from the quad, past which nothing is drawn: a
/// crystal's centre is at most 0.43 cell off the path, its arm 0.62 and its
/// line 0.054 more -- 1.105, and a little over.
pub const REACH: f32 = 1.15;

/// The crystals' pitch along the path, the longest arm, the line's half
/// width, and the spacing of side branches along an arm, in cells.
pub const SPACING: f32 = 0.70;
pub const ARM: f32 = 0.62;
pub const WIDTH: f32 = 0.028;
pub const BRANCH: f32 = 0.21;

/// Seconds a crystal takes to grow, when the tail starts to melt, how long
/// the melt front takes from tail to head, and how long a crystal melts.
pub const GROW: f32 = 0.12;
pub const HOLD: f32 = 0.25;
pub const SPREAD: f32 = 0.20;
pub const MELT: f32 = 0.22;

/// The effect's own numbers, `k` in `EffectU`.
pub const PARAMS: [f32; effect::PARAMS] = [SPACING, ARM, WIDTH, BRANCH, GROW, HOLD, SPREAD, MELT];

/// **The frost's deposition follows kitty's leading corners**, which halve
/// their gap every tenth of their constant: the shader's `ICE_LEAD`, pinned
/// to this by the metallib test.
pub const LEAD: f32 = TAU_FAST / 10.0;

/// The salts that part the crystals' and the rime's hashes from the fire's,
/// on the same seed; the shader's `ICE_SALT` and `RIME_SALT`.
pub const ICE_SALT: u32 = 0x1CE0_1CE0;
pub const RIME_SALT: u32 = 0x51ED_0000;

/// Shards a cell of path, their salt, drag a second and fall in cells a
/// second squared: a terminal fall of two cells a second.
const SHARDS_PER_UNIT: f32 = 1.5;
const SHARD_SALT: u32 = 0x5AAD_5AAD;
const DRAG: f32 = 3.0;
const GRAVITY: f32 = 6.0;

fn sat(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn length(x: f32, y: f32) -> f32 {
    (x * x + y * y).sqrt()
}

/// When the frost reaches `u` of the path, 0 at the tail and 1 at the head:
/// kitty's leading-corner closed form, inverted.
fn deposit(u: f32) -> f32 {
    -LEAD * (1.0 - 0.999 * u).max(1e-3).det_log2()
}

fn line(d: f32, w: f32, aa: f32) -> f32 {
    1.0 - smoothstep(w - aa, w + aa, d)
}

/// The path's frame at a pixel: `rel` from the tail in cells, the direction
/// and its normal, and the path's length in cells.
struct Frame {
    rel: (f32, f32),
    d: (f32, f32),
    n: (f32, f32),
    lu: f32,
}

/// **Crystal `k` at the frame's pixel**: its arms, side branches and plate,
/// grown, held and melted by its place along the path -- `ice_crystal`.
fn crystal(k: f32, fr: &Frame, w: f32, aa: f32, sbase: Hash, age: f32) -> f32 {
    let h1 = Hash((k as i32).cast_unsigned().wrapping_add(sbase.0)).next();
    let h2 = h1.next();
    let h3 = h2.next();
    let h4 = h3.next();
    let h5 = h4.next();
    let h6 = h5.next();
    let along = (k + (h1.unit() - 0.5) * 0.5) * SPACING;
    let across = (h2.unit() - 0.5) * 0.7;
    let win = smoothstep(-0.25, 0.0, along) * (1.0 - smoothstep(fr.lu, fr.lu + 0.25, along));
    if win <= 0.0 {
        return 0.0;
    }
    let u = sat(along / fr.lu);
    let delay = 0.03 * h5.unit();
    let x = sat((age - deposit(u) - delay) / GROW);
    let grow = 1.0 - (1.0 - x) * (1.0 - x) * (1.0 - x);
    let m = HOLD + SPREAD * u + delay;
    let fade = 1.0 - smoothstep(m, m + MELT, age);
    let l = ARM * (0.45 + 0.55 * h4.unit()) * grow * (0.35 + 0.65 * fade);
    let exist = sat(l / (2.0 * w));
    if exist <= 0.0 || fade <= 0.0 {
        return 0.0;
    }
    let rx = fr.rel.0 - (fr.d.0 * along + fr.n.0 * across);
    let ry = fr.rel.1 - (fr.d.1 * along + fr.n.1 * across);
    let s = 0.577_350_27 * h3.unit();
    let den = 1.0 + s * s;
    let (cs, sn) = ((1.0 - s * s) / den, 2.0 * s / den);
    let (mut wx, mut wy) = ((cs * rx + sn * ry).abs(), (-sn * rx + cs * ry).abs());
    for (nx, ny) in [(-0.866_025_4f32, 0.5f32), (-0.5, 0.866_025_4)] {
        let t = 2.0 * (wx * nx + wy * ny).max(0.0);
        wx -= t * nx;
        wy -= t * ny;
    }
    let dm = length(wx - wx.clamp(0.0, l), wy);
    let wm = w * (1.4 - 0.7 * sat(wx / l.max(1e-4)));
    let main = 0.95 * line(dm, wm, aa);
    let (ex, ey) = (0.5f32, 0.866_025_4f32);
    let xr = wx - wy * 0.577_350_27;
    let xj = (xr / BRANCH + 0.5).floor().max(1.0) * BRANCH;
    let lj = (0.6 * (l - xj)).min(xj);
    let (vx, vy) = (wx - xj, wy);
    let h = (vx * ex + vy * ey).clamp(0.0, lj.max(0.0));
    let ds = length(vx - h * ex, vy - h * ey);
    let side = 0.60 * line(ds, w, aa) * sat(lj / (2.0 * w));
    let pr = 0.10 + 0.30 * h6.unit() * h6.unit();
    let dp = (wx * 0.866_025_4 + wy * 0.5) - pr * l * 0.866_025_4;
    let plate = 0.30 * (1.0 - smoothstep(-aa, aa, dp));
    main.max(side.max(plate)) * exist * fade * win
}

/// **The ice's opacity at `p`**, 0 to 1, before the record's alpha: the
/// pane over the quad, and over the path the crystals, their glint and the
/// rime -- `ice_frost` in the shader.
pub fn light(vfx: &Vfx, p: Pt) -> f32 {
    let f = &vfx.ignition;
    if vfx.emitter.hole.contains(p) {
        return 0.0;
    }
    let c = f.unit;
    let fr = Frame {
        rel: ((p.x - f.tail.x) / c, (p.y - f.tail.y) / c),
        d: (f.dir.x, f.dir.y),
        n: (-f.dir.y, f.dir.x),
        lu: f.len / c,
    };
    let along = fr.rel.0 * fr.d.0 + fr.rel.1 * fr.d.1;
    let cl = along.clamp(0.0, fr.lu);
    let dseg = length(fr.rel.0 - fr.d.0 * cl, fr.rel.1 - fr.d.1 * cl);
    let dq = effect::distance(p, &vfx.emitter.corners);
    if dseg > REACH && dq / c > REACH {
        return 0.0;
    }
    let pa = sat(1.0 - f.age / (0.6 * HOLD));
    let pane = pa
        * (0.14 * (1.0 - smoothstep(-0.5, 0.5, dq)))
            .max(0.5 * (1.0 - smoothstep(0.5, 1.5, dq.abs())));
    let mut a = pane;
    if dseg <= REACH {
        let w = WIDTH.max(0.75 / c);
        let aa = 0.5 / c;
        let sbase = Hash(f.seed.0.wrapping_add(ICE_SALT)).next();
        let k0 = (along / SPACING + 0.5).floor();
        let mut frost = 0.0f32;
        for dk in [-1.0f32, 0.0, 1.0] {
            frost = frost.max(crystal(k0 + dk, &fr, w, aa, sbase, f.age));
        }
        let gp = (f.age - 0.4 * HOLD) / (0.6 * HOLD) * (fr.lu + 2.0 * SPACING) - SPACING;
        frost = sat(frost * (1.0 + 0.3 * (1.0 - smoothstep(0.0, SPACING, (along - gp).abs()))));
        let u = sat(along / fr.lu);
        let front = (fr.lu + 0.6) * (1.0 - (-f.age / LEAD).det_exp2()) - 0.3;
        let dep = 1.0 - smoothstep(front - 0.3, front + 0.3, along);
        let mr = HOLD + SPREAD * u;
        let rfade = 1.0 - smoothstep(mr, mr + MELT, f.age);
        let s2 = f.seed.0 ^ RIME_SALT;
        let (qx, qy) = (
            0.8 * fr.rel.0 - 0.6 * fr.rel.1,
            0.6 * fr.rel.0 + 0.8 * fr.rel.1,
        );
        let n = 0.65 * effect::noise(fr.rel.0 * 2.3, fr.rel.1 * 2.3, Hash(s2))
            + 0.35 * effect::noise(qx * 6.7, qy * 6.7, Hash(s2.wrapping_add(0x9E37_79B9)));
        let rime =
            0.24 * (1.0 - smoothstep(0.15, 0.6, dseg)) * smoothstep(0.35, 0.75, n) * dep * rfade;
        a = a.max(frost.max(rime));
    }
    a
}

/// **A shard's plate at `uv`**, -1 to 1 across its record: iq's hexagon,
/// filled, with a brighter rim -- the particle branch of `ice_fragment`.
pub fn shard(uv: Pt) -> f32 {
    let (k0, k1, k2) = (-0.866_025_4f32, 0.5f32, 0.577_350_27f32);
    let r = 0.70f32;
    let (mut x, mut y) = (uv.x.abs(), uv.y.abs());
    let t = 2.0 * (k0 * x + k1 * y).min(0.0);
    x -= t * k0;
    y -= t * k1;
    x -= x.clamp(-k2 * r, k2 * r);
    y -= r;
    // MSL's `sign`, which is zero at zero where `f32::signum` is one.
    let sign = if y > 0.0 {
        1.0
    } else if y < 0.0 {
        -1.0
    } else {
        0.0
    };
    let d = length(x, y) * sign;
    (1.0 - smoothstep(-0.08, 0.12, d)) * (0.55 + 0.45 * smoothstep(-0.35, -0.02, d))
}

fn shed(f: &Ignition) -> impl Iterator<Item = Hash> + use<> {
    let n = (f.len / f.unit.max(1.0) * SHARDS_PER_UNIT).clamp(8.0, 96.0);
    let seed = f.seed.0;
    (0..u32::try_from(n.round() as i64).unwrap_or(8))
        .map(move |i| Hash(seed ^ i.wrapping_mul(0x9E37_79B9) ^ SHARD_SALT).next())
}

/// A shard's draws: where along the path, when it breaks off, how long it
/// lives, and the hashes the rest comes from.
struct Shard {
    along: f32,
    born: f32,
    life: f32,
    h: [Hash; 6],
}

impl Shard {
    fn draw(h0: Hash) -> Shard {
        let mut h = [h0; 6];
        for i in 1..6 {
            h[i] = h[i - 1].next();
        }
        let along = h0.unit();
        Shard {
            along,
            born: HOLD + SPREAD * along + 0.3 * MELT * h[5].unit(),
            life: 0.20 + 0.15 * h[1].unit(),
            h,
        }
    }

    fn at(&self, f: &Ignition) -> Option<Particle> {
        let a = f.age - self.born;
        if a < 0.0 || a > self.life {
            return None;
        }
        let c = f.unit;
        let [_, h1, h2, h3, h4, _] = self.h;
        let (nx, ny) = (-f.dir.y, f.dir.x);
        let side = (h2.unit() - 0.5) * 0.7 * c;
        let p0 = Pt {
            x: f.tail.x + f.dir.x * f.len * self.along + nx * side,
            y: f.tail.y + f.dir.y * f.len * self.along + ny * side,
        };
        let v0 = Pt {
            x: (h3.unit() - 0.5) * 1.2 * c,
            y: -(0.2 + 0.6 * h4.unit()) * c,
        };
        let s = (1.0 - (-DRAG * a).det_exp()) / DRAG;
        let phase = core::f32::consts::TAU * Hash(h1.0 ^ 0x5bd1_e995).unit();
        let heat = 1.0 - a / self.life;
        Some(Particle {
            centre: Pt {
                x: p0.x + v0.x * s + 0.10 * c * (9.0 * a + phase).det_sin(),
                y: p0.y + v0.y * s + GRAVITY * c / DRAG * (a - s),
            },
            radius: (c * (0.12 + 0.10 * h2.unit()) * (0.6 + 0.4 * heat)).max(2.0),
            glow: 0.9 * heat,
        })
    }
}

/// Every shard alive at the ignition's age.
pub fn shards(f: &Ignition) -> impl Iterator<Item = Particle> + '_ {
    shed(f).filter_map(move |h| Shard::draw(h).at(f))
}

/// **Seconds from ignition to when the ice is out**: its life, or its last
/// shard's death.
pub fn end(f: &Ignition) -> f32 {
    shed(f)
        .map(|h| {
            let s = Shard::draw(h);
            s.born + s.life
        })
        .fold(LIFE, f32::max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::tests::{CELL, cell, ignition};
    use crate::effect::{Effect, Seed};
    use crate::trail::{Rect, Segment, Shape, Smear};

    fn vfx(from: Rect, to: Rect, seed: Seed, age: f32) -> Vfx {
        let f = ignition(from, to, seed);
        Vfx {
            effect: Effect::Ice,
            emitter: Smear {
                corners: Segment::new(from.corners(), to).at(age),
                hole: to,
                shape: Shape::Quad,
                strength: crate::flash::Strength::Full,
            },
            ignition: f.at(age),
            axis: f.dir,
        }
    }

    /// Crystal `k`'s centre, in pixels, for a test to look at.
    fn centre(f: &Ignition, k: f32) -> Pt {
        let sbase = Hash(f.seed.0.wrapping_add(ICE_SALT)).next();
        let h1 = Hash((k as i32).cast_unsigned().wrapping_add(sbase.0)).next();
        let h2 = h1.next();
        let along = (k + (h1.unit() - 0.5) * 0.5) * SPACING * f.unit;
        let across = (h2.unit() - 0.5) * 0.7 * f.unit;
        Pt {
            x: f.tail.x + f.dir.x * along - f.dir.y * across,
            y: f.tail.y + f.dir.y * along + f.dir.x * across,
        }
    }

    /// **A crystal is six-fold**: at its full size, its frost at a point and
    /// at the same point turned 60 degrees about its centre agree -- the
    /// fold's two mirrors, and nothing else, make the other five arms.
    #[test]
    fn a_crystal_is_six_fold() {
        let (from, to) = (cell(10.0, 5.0), cell(40.0, 5.0));
        let v = vfx(from, to, Seed(12), 0.2);
        let f = v.ignition;
        let mut compared = 0;
        for k in 5..25 {
            let o = centre(&f, k as f32);
            for r in [0.08f32, 0.2, 0.35] {
                for t in 0..12 {
                    let th = t as f32 * 0.52;
                    let at = |a: f32| Pt {
                        x: o.x + r * f.unit * a.det_cos(),
                        y: o.y + r * f.unit * a.det_sin(),
                    };
                    let (a, b) = (at(th), at(th + core::f32::consts::FRAC_PI_3));
                    let (fa, fb) = (crystal_only(&v, a, k as f32), crystal_only(&v, b, k as f32));
                    assert!(
                        (fa - fb).abs() < 0.02,
                        "crystal {k} at {r}: {fa} against {fb}"
                    );
                    compared += usize::from(fa > 0.1);
                }
            }
        }
        assert!(compared > 100, "only {compared} lit points compared");
    }

    /// Crystal `k` alone at `p`, as `light` computes it.
    fn crystal_only(v: &Vfx, p: Pt, k: f32) -> f32 {
        let f = &v.ignition;
        let c = f.unit;
        let fr = Frame {
            rel: ((p.x - f.tail.x) / c, (p.y - f.tail.y) / c),
            d: (f.dir.x, f.dir.y),
            n: (-f.dir.y, f.dir.x),
            lu: f.len / c,
        };
        let sbase = Hash(f.seed.0.wrapping_add(ICE_SALT)).next();
        crystal(k, &fr, WIDTH.max(0.75 / c), 0.5 / c, sbase, f.age)
    }

    /// The most frost within a cell of the path at `u` of it.
    fn frost_near(v: &Vfx, u: f32) -> f32 {
        let f = &v.ignition;
        let mut most = 0.0f32;
        for i in -20..=20 {
            for j in -20..=20 {
                let p = Pt {
                    x: f.tail.x + f.dir.x * f.len * u + i as f32 * 0.05 * f.unit,
                    y: f.tail.y + f.dir.y * f.len * u + j as f32 * 0.05 * f.unit,
                };
                if effect::distance(p, &v.emitter.corners) / f.unit < 1.6 {
                    continue;
                }
                most = most.max(light(v, p));
            }
        }
        most
    }

    /// **The frost is laid behind the quad's leading edge and melts tail
    /// first**: 5 ms in, the tail has frost and the head none; held, both
    /// ends have it; at 0.47 s the tail has melted and the head has not; at
    /// its life nothing is left.
    #[test]
    fn frost_is_laid_behind_the_lead_and_melts_tail_first() {
        let (from, to) = (cell(10.0, 5.0), cell(70.0, 5.0));
        let v = |age| vfx(from, to, Seed(3), age);
        assert!(frost_near(&v(0.06), 0.1) > 0.2, "the tail frosts first");
        assert!(frost_near(&v(0.005), 0.85) < 0.01, "not yet the head");
        assert!(frost_near(&v(0.2), 0.1) > 0.2 && frost_near(&v(0.2), 0.85) > 0.2);
        let (tail, head) = (frost_near(&v(0.47), 0.1), frost_near(&v(0.47), 0.85));
        assert!(tail < head, "the tail melts first: {tail} against {head}");
        assert_eq!(frost_near(&v(LIFE + 0.001), 0.5), 0.0, "gone");
    }

    /// **The deposition keeps pace with kitty's leading corners**: the
    /// frost reaches `u` of the path when the corners have closed that share
    /// of their gap.
    #[test]
    fn the_deposit_time_is_kittys_leading_edge() {
        for u in [0.1f32, 0.5, 0.9] {
            let t = deposit(u);
            let closed = 1.0 - (-10.0 * t / TAU_FAST).det_exp2();
            assert!((closed - 0.999 * u).abs() < 1e-4, "{u}: {closed}");
        }
    }

    /// **Shards break off as the lace melts, pop up and drift down, fading**,
    /// and the ice is out when the last is gone.
    #[test]
    fn shards_break_off_the_melt_and_fall() {
        let f = ignition(cell(10.0, 5.0), cell(40.0, 5.0), Seed(9));
        assert_eq!(
            shards(&f.at(HOLD - 0.001)).count(),
            0,
            "none before the melt"
        );
        let mut seen = 0;
        for h in shed(&f) {
            let s = Shard::draw(h);
            assert!(s.born >= HOLD && s.born <= HOLD + SPREAD + 0.3 * MELT);
            let birth = s.at(&f.at(s.born)).expect("at birth");
            let late = s.at(&f.at(s.born + s.life * 0.99)).expect("at death");
            assert!(late.centre.y > birth.centre.y, "it falls");
            assert!(late.glow < birth.glow, "fading");
            assert!(birth.radius >= 2.0);
            seen += 1;
        }
        assert_eq!(seen, 23, "one and a half a cell of a fifteen-cell path");
        let out = end(&f);
        assert!((LIFE..0.9).contains(&out), "out at {out}");
        assert_eq!(shards(&f.at(out + 0.001)).count(), 0);
        assert_eq!(Effect::Ice.end(&f), out);
    }

    /// **A shard is a hexagon with a brighter rim**: full inside, nothing
    /// outside, its flats at 0.70 and its corners further out.
    #[test]
    fn a_shard_is_a_rimmed_hexagon() {
        assert!(shard(Pt { x: 0.0, y: 0.0 }) > 0.5);
        assert!(
            shard(Pt { x: 0.0, y: 0.6 }) > shard(Pt { x: 0.0, y: 0.0 }),
            "rim"
        );
        assert_eq!(shard(Pt { x: 0.0, y: 0.95 }), 0.0, "past the flat");
        assert!(
            shard(Pt { x: 0.72, y: 0.0 }) > 0.2,
            "a corner reaches further"
        );
    }

    /// **The cursor is never covered**: over the whole life, no pixel of the
    /// cursor's rectangle is lit -- while the same rectangle, with the hole
    /// moved away, is frosted, so there was frost there to leave bare.
    #[test]
    fn the_cursor_is_never_frosted() {
        let (from, to) = (cell(10.0, 5.0), cell(40.0, 5.0));
        let in_hole = |v: &Vfx| {
            let mut most = 0.0f32;
            for y in 0..34 {
                for x in 0..17 {
                    let p = Pt {
                        x: to.left + x as f32 + 0.5,
                        y: to.top + y as f32 + 0.5,
                    };
                    most = most.max(light(v, p));
                }
            }
            most
        };
        let mut frosted = 0.0f32;
        for i in 1..14 {
            let age = i as f32 * 0.05;
            let v = vfx(from, to, Seed(5), age);
            assert_eq!(in_hole(&v), 0.0, "the cursor lit at {age}");
            let mut bare = v;
            bare.emitter.hole = cell(0.0, 0.0);
            frosted = frosted.max(in_hole(&bare));
        }
        assert!(frosted > 0.3, "no frost where the cursor is: {frosted}");
    }

    /// **Every pixel the ice lights is in a band.** The bands cover what
    /// `Vfx::support` names, so a support that left out the path would clip
    /// the lace in both GPU arms and the oracle's reference alike and pass;
    /// this scans the frame with the model instead, over a hop, a diagonal
    /// and a retarget whose quad heads away from the path.
    #[test]
    fn every_pixel_the_ice_lights_is_in_a_band() {
        let frame = (baud_font::Px::of(1200), baud_font::Px::of(400));
        for (from, to, quad_to, age) in [
            (cell(5.0, 3.0), cell(40.0, 3.0), cell(40.0, 3.0), 0.2),
            (cell(5.0, 1.0), cell(60.0, 9.0), cell(60.0, 9.0), 0.3),
            (cell(5.0, 3.0), cell(40.0, 3.0), cell(20.0, 9.0), 0.25),
        ] {
            let mut v = vfx(from, to, Seed(8), age);
            v.emitter.corners = Segment::new(to.corners(), quad_to).at(age - 0.1);
            v.emitter.hole = quad_to;
            let mut bands = Vec::new();
            v.instances(frame, crate::color::Argb(0xFFFF_FFFF), &mut bands);
            bands.retain(|r| r.kind == crate::atlas::Kind::Band);
            let mut lit = 0;
            for y in 0..400 {
                for x in 0..1200 {
                    let p = Pt {
                        x: x as f32 + 0.5,
                        y: y as f32 + 0.5,
                    };
                    if light(&v, p) <= 0.0 {
                        continue;
                    }
                    lit += 1;
                    let covered = bands.iter().any(|r| {
                        let (x0, y0) = (i32::from(r.x.0), i32::from(r.y.0));
                        x >= x0 && x < x0 + i32::from(r.w.0) && y >= y0 && y < y0 + i32::from(r.h.0)
                    });
                    assert!(covered, "{x},{y} lit outside every band");
                }
            }
            assert!(lit > 2000, "only {lit} pixels lit");
        }
    }

    /// **The pane is the quad's, and it is gone by 0.15 s**; the cursor is
    /// never covered. A retarget sends the quad far from the path, so the
    /// pane is read alone: inside the quad at 20 ms, nothing at 0.16 s.
    #[test]
    fn the_pane_fades_and_the_cursor_stays_bare() {
        let (from, to) = (cell(10.0, 5.0), cell(40.0, 5.0));
        let v = vfx(from, to, Seed(1), 0.02);
        let mid = Pt {
            x: (to.left + to.right) * 0.5,
            y: (to.top + to.bottom) * 0.5,
        };
        assert_eq!(light(&v, mid), 0.0, "the hole");
        let far = cell(40.0, 15.0);
        let pane = |age: f32| {
            let mut v = vfx(from, to, Seed(1), age);
            v.emitter.corners = Segment::new(cell(20.0, 15.0).corners(), far).at(age);
            v.emitter.hole = far;
            let c = v.emitter.corners.0;
            let inside = Pt {
                x: (c[0].x + c[1].x + c[2].x + c[3].x) / 4.0 - CELL.0,
                y: (c[0].y + c[1].y + c[2].y + c[3].y) / 4.0,
            };
            assert!(effect::distance(inside, &v.emitter.corners) < 0.0);
            light(&v, inside)
        };
        // 14% of the cursor's colour, a fifth of the way through its fade.
        let want = 0.14 * (1.0 - 0.02 / (0.6 * HOLD));
        assert!(
            (pane(0.02) - want).abs() < 1e-4,
            "a pane of ice: {}",
            pane(0.02)
        );
        assert_eq!(pane(0.16), 0.0, "gone by 0.15 s");
    }
}
