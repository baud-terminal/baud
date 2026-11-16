//! **The lightning trail**: a thin, jagged bolt thrown from the cell the
//! cursor left to the cell it reached. It draws itself in like a stepped
//! leader over 40 ms, holds, and fades over a quarter second, the core
//! thinning as it goes; up to three short forks branch off it, a corona
//! brightens round the cursor as the bolt lands, and a splash of sparks falls
//! from it. Its shape never changes during its life: no flicker, no
//! re-strike, so every pixel it crosses sees one rise and one fall.
//! Designed in the effect-trail research (the lightning spec), in the
//! cursor's colour with intensity as opacity.
//!
//! **Its emitter is not kitty's quad but a box along the path**
//! ([`region`]), fixed for the ignition: a bolt's tail would be cut off by
//! the quad's trailing corners within 40 ms, and a mask that moves under a
//! pixel breaks the one-peak argument.
//!
//! **The bolt is midpoint displacement written as a sum**: one tent a level
//! (the Schauder expansion), so the offset at any point along the path is a
//! closed form and a pixel reads the three segments near it, not all 256.
//! This module is the Rust model of the shader, `bolt_light`: the geometry
//! and the shading both, which the unit tests hold to their properties and
//! the pixel oracle's reference calls.

use crate::det::Det;
use crate::effect::{self, Hash, Ignition, Particle, Vfx, smoothstep};
use crate::trail::{Corners, Pt, Rect};

/// Seconds from ignition to when the bolt is gone. Below [`REIGNITE`]'s 334
/// ms, so two bolts never overlap; the driver's own guard outlives the bolt.
pub const LIFE: f32 = 0.32;

/// Cells from the [`region`] the bolt can reach: its offset is at most
/// `CAP * LEVELS` cells, two, its glow is under half a step 0.28 cell out,
/// and the region is half a cell wide either side.
pub const REACH: f32 = 2.2;

/// Seconds for the leader to cross the path, tail to head; the moment the
/// corona and the sparks start.
pub const T_LEAD: f32 = 0.040;
/// Seconds each pixel takes to rise once the leader reaches it.
pub const T_RISE: f32 = 0.012;
/// When the fade starts; it ends at [`LIFE`]. At least `T_LEAD + T_RISE`,
/// so every pixel is full before any starts to fade: one peak a pixel.
pub const T_FALL: f32 = 0.070;
/// The core's half width in cells, floored at a pixel, and its glow's
/// width and peak.
pub const W_CORE: f32 = 0.06;
pub const W_GLOW: f32 = 0.12;
pub const G_MAX: f32 = 0.40;
/// The corona round the cursor: its peak and its width in cells.
pub const CORONA: f32 = 0.30;
pub const W_CORONA: f32 = 0.20;

/// The effect's own numbers, `k` in `EffectU`.
pub const PARAMS: [f32; effect::PARAMS] = [
    W_CORE, W_GLOW, G_MAX, T_LEAD, T_RISE, T_FALL, CORONA, W_CORONA,
];

const _: () = assert!(T_LEAD + T_RISE <= T_FALL);

/// The bolt's shape, which the shader spells as `#define`s the metallib
/// test pins: how far a level may push the path, as a share of its span and
/// at most in cells; the shortest segment in cells; the most levels; the
/// salts; the least path a fork grows on, in cells; how often a fork grows.
pub const JAG: f32 = 0.8;
pub const CAP: f32 = 0.25;
pub const S_MIN: f32 = 0.30;
pub const LEVELS: u32 = 8;
pub const BOLT_SALT: u32 = 0x51ED_270B;
pub const FORK_SALT: u32 = 0xA24B_AED4;
pub const FORK_MIN: f32 = 5.0;
pub const FORK_P: f32 = 0.75;

/// Sparks an ignition throws, their drag a second, and their fall in cells a
/// second squared.
const SPARKS: u32 = 10;
const DRAG: f32 = 14.0;
const GRAVITY: f32 = 8.0;
const SPARK_SALT: u32 = 0x3C6E_F372;

/// **The region the bolt is drawn over**: a box along the path, from the
/// tail to the head and half a cell either side, the same for the whole
/// ignition.
pub fn region(ignition: &Ignition) -> Corners {
    let f = ignition;
    let n = normal(f.dir);
    let hw = 0.5 * f.unit;
    let head = head(f);
    let at = |p: Pt, s: f32| Pt {
        x: p.x + n.x * s,
        y: p.y + n.y * s,
    };
    Corners([at(f.tail, -hw), at(head, -hw), at(head, hw), at(f.tail, hw)])
}

fn head(f: &Ignition) -> Pt {
    Pt {
        x: f.tail.x + f.dir.x * f.len,
        y: f.tail.y + f.dir.y * f.len,
    }
}

fn normal(dir: Pt) -> Pt {
    Pt {
        x: -dir.y,
        y: dir.x,
    }
}

/// A point of the noise lattice: the tent's index along the path, and the
/// level.
struct Lattice(i32, i32);

/// `fx_lattice`: a number in [0, 1) at a lattice point.
fn lattice(at: Lattice, seed: Hash) -> f32 {
    let inner = Hash(at.1.cast_unsigned().wrapping_add(seed.0)).next().0;
    Hash(at.0.cast_unsigned().wrapping_add(inner)).next().unit()
}

/// **How many levels the bolt has**: as many as keep every segment at least
/// [`S_MIN`] cells long, up to [`LEVELS`]. Its segments number two to it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Levels(u32);

impl Levels {
    /// The levels a path `len` pixels long takes, on cells `c` tall. The
    /// span doubles as a float, as the shader's `float(1u << l)` is exact.
    pub fn of(len: f32, c: f32) -> Levels {
        let (mut l, mut span) = (0, 2.0f32);
        while l < LEVELS && len >= (c * span) * S_MIN {
            l += 1;
            span *= 2.0;
        }
        Levels(l)
    }

    /// The segments the bolt is cut into, which is its last vertex.
    pub fn segments(self) -> Node {
        Node(1i32 << self.0)
    }
}

/// A vertex of the bolt, counted from the tail.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Node(pub i32);

/// **The bolt's offset across the path at `u`**, 0 at the tail and 1 at the
/// head, in pixels: one tent a level, the level's lattice value its height.
/// Zero at both ends, exactly.
pub fn offset(u: f32, lv: Levels, f: &Ignition) -> f32 {
    let seed = Hash(f.seed.0 ^ BOLT_SALT);
    let (mut y, mut n) = (0.0f32, 1.0f32);
    for l in 0..lv.0 {
        let x = u * n;
        let i = x.floor();
        let t = x - i;
        let tent = 1.0 - (2.0 * t - 1.0).abs();
        let xi = 2.0 * lattice(Lattice(i as i32, l.cast_signed()), seed) - 1.0;
        let d = (JAG * f.len / (2.0 * n)).min(CAP * f.unit);
        y += d * xi * tent;
        n *= 2.0;
    }
    y
}

/// The bolt's vertex `j`.
pub fn vertex(j: Node, lv: Levels, f: &Ignition) -> Pt {
    #[cfg(test)]
    crate::det::tally::count(&crate::det::tally::VERTICES);
    let u = j.0 as f32 / lv.segments().0 as f32;
    let n = normal(f.dir);
    let y = offset(u, lv, f);
    Pt {
        x: f.tail.x + f.dir.x * (f.len * u) + n.x * y,
        y: f.tail.y + f.dir.y * (f.len * u) + n.y * y,
    }
}

/// `fx_segment`: the distance from `p` to the segment `a` to `b`.
pub fn segment(p: Pt, a: Pt, b: Pt) -> f32 {
    let (bx, by) = (b.x - a.x, b.y - a.y);
    let (px, py) = (p.x - a.x, p.y - a.y);
    let h = ((px * bx + py * by) / (bx * bx + by * by).max(1e-6)).clamp(0.0, 1.0);
    let (dx, dy) = (px - bx * h, py - by * h);
    (dx * dx + dy * dy).sqrt()
}

/// **The distance from `p` to the bolt's channel**, from the three segments
/// round its place along the path -- exact wherever the glow is lit,
/// because a segment is at least [`S_MIN`] cells long and the glow is out
/// by 0.28.
pub fn channel(p: Pt, f: &Ignition) -> f32 {
    let lv = Levels::of(f.len, f.unit);
    channel_by(p, f, lv, |j| vertex(j, lv, f))
}

/// [`channel`] on `lv` levels, each vertex `j` the one `vertex(j)` hands,
/// which is [`vertex`]'s.
fn channel_by(p: Pt, f: &Ignition, lv: Levels, mut vertex: impl FnMut(Node) -> Pt) -> f32 {
    let m = lv.segments().0;
    let u = ((p.x - f.tail.x) * f.dir.x + (p.y - f.tail.y) * f.dir.y) / f.len;
    let a0 = (((u * m as f32).floor() as i32).clamp(0, m - 1) - 1).max(0);
    let w = [0, 1, 2, 3].map(|i| vertex(Node((a0 + i).min(m))));
    segment(p, w[0], w[1])
        .min(segment(p, w[1], w[2]))
        .min(segment(p, w[2], w[3]))
}

/// **The distance from `p` to the nearest fork**: up to three, at a quarter,
/// a half and three quarters of the path, 25-45 degrees off it, each two
/// segments and at most 1.2 cells long -- on a path of [`FORK_MIN`] cells
/// or more.
pub fn forks(p: Pt, f: &Ignition) -> f32 {
    Forks::of(f).distance(p)
}

/// **The forks an ignition's bolt grows**, each its root, its bend and its
/// tip, the same at every pixel: none on a path under [`FORK_MIN`] cells.
struct Forks([Option<[Pt; 3]>; 3]);

impl Forks {
    fn of(f: &Ignition) -> Forks {
        let mut out = [None; 3];
        if f.len < FORK_MIN * f.unit {
            return Forks(out);
        }
        let lv = Levels::of(f.len, f.unit);
        let n = normal(f.dir);
        let base = Hash(f.seed.0 ^ FORK_SALT).next().0;
        for (k, fork) in (0..3u32).zip(&mut out) {
            let h0 = Hash(k.wrapping_add(base)).next();
            *fork = Self::fork(f, h0, 0.25 * (k + 1) as f32, (lv, n));
        }
        Forks(out)
    }

    /// The fork whose first hash is `h0`, rooted `q` of the way along the
    /// path, if it grows.
    fn fork(f: &Ignition, h0: Hash, q: f32, (lv, n): (Levels, Pt)) -> Option<[Pt; 3]> {
        if h0.unit() >= FORK_P {
            return None;
        }
        let h1 = h0.next();
        let h2 = h1.next();
        let h3 = h2.next();
        let h4 = h3.next();
        let yr = offset(q, Levels(lv.0.min(2)), f);
        let side = if yr.abs() > 0.6 * f.unit {
            -yr.signum()
        } else if h1.unit() < 0.5 {
            1.0
        } else {
            -1.0
        };
        let s = side * (0.466 + 0.534 * h2.unit());
        let (vx, vy) = (f.dir.x + n.x * s, f.dir.y + n.y * s);
        let l = (vx * vx + vy * vy).sqrt();
        let fd = Pt {
            x: vx / l,
            y: vy / l,
        };
        let lf = ((0.18 + 0.14 * h3.unit()) * f.len).min(1.2 * f.unit);
        let r = Pt {
            x: f.tail.x + f.dir.x * (f.len * q) + n.x * yr,
            y: f.tail.y + f.dir.y * (f.len * q) + n.y * yr,
        };
        let bend = (h4.unit() - 0.5) * 0.3 * lf;
        let e = Pt {
            x: r.x + fd.x * (0.5 * lf) - fd.y * bend,
            y: r.y + fd.y * (0.5 * lf) + fd.x * bend,
        };
        let t = Pt {
            x: r.x + fd.x * lf,
            y: r.y + fd.y * lf,
        };
        Some([r, e, t])
    }

    /// The distance from `p` to the nearest fork, [`forks`].
    fn distance(&self, p: Pt) -> f32 {
        let mut d = f32::INFINITY;
        for &[r, e, t] in self.0.iter().flatten() {
            d = d.min(segment(p, r, e).min(segment(p, e, t)));
        }
        d
    }
}

/// The distance from `p` to the rectangle `r`, outside it.
fn to_rect(p: Pt, r: Rect) -> f32 {
    let dx = (p.x - (r.left + r.right) * 0.5).abs() - (r.right - r.left) * 0.5;
    let dy = (p.y - (r.top + r.bottom) * 0.5).abs() - (r.bottom - r.top) * 0.5;
    let (dx, dy) = (dx.max(0.0), dy.max(0.0));
    (dx * dx + dy * dy).sqrt()
}

/// **The bolt's opacity at `p`**, 0 to 1, before the record's alpha: the
/// core, the glow and the corona, as the leader reaches the pixel and the
/// fade takes it -- `bolt_light` in the shader.
pub fn light(vfx: &Vfx, p: Pt) -> f32 {
    let f = &vfx.ignition;
    let bolt = Bolt::of(f);
    lit(vfx, p, &bolt, |j| vertex(j, bolt.lv, f))
}

/// **What an ignition's bolt is drawn from, the same at every pixel**: its
/// levels, its region and its forks.
struct Bolt {
    lv: Levels,
    region: Corners,
    forks: Forks,
}

impl Bolt {
    fn of(f: &Ignition) -> Bolt {
        Bolt {
            lv: Levels::of(f.len, f.unit),
            region: region(f),
            forks: Forks::of(f),
        }
    }
}

/// The vertices a bolt has at most: two to [`LEVELS`] segments, and one.
pub(crate) const VERTICES: usize = (1 << LEVELS) + 1;

/// **The bolt's light over a band's pixels**, `xs` by `ys` in whole pixels:
/// `each(x, y, light)` at every pixel, [`light`] at its centre bit for bit,
/// the bolt taken once and each vertex the first time a pixel reads it,
/// where the light at each pixel takes the bolt and four vertices again.
pub fn shade(
    vfx: &Vfx,
    xs: core::ops::Range<usize>,
    ys: core::ops::Range<usize>,
    each: impl FnMut(usize, usize, f32),
) {
    let f = &vfx.ignition;
    let bolt = Bolt::of(f);
    let mut taken = [None::<Pt>; VERTICES];
    let light = |p| {
        lit(vfx, p, &bolt, |j| {
            match usize::try_from(j.0).ok().and_then(|i| taken.get_mut(i)) {
                Some(slot) => *slot.get_or_insert_with(|| vertex(j, bolt.lv, f)),
                None => vertex(j, bolt.lv, f),
            }
        })
    };
    effect::lights(xs, ys, light, each);
}

/// [`light`] from the ignition's `bolt`, each vertex `j` the one
/// `vertex(j)` hands.
fn lit(vfx: &Vfx, p: Pt, bolt: &Bolt, vertex: impl FnMut(Node) -> Pt) -> f32 {
    let f = &vfx.ignition;
    let hole = vfx.emitter.hole;
    if hole.contains(p) || f.age >= LIFE {
        return 0.0;
    }
    let c = f.unit;
    if crate::effect::distance(p, &bolt.region) / c > REACH {
        return 0.0;
    }
    let u = ((p.x - f.tail.x) * f.dir.x + (p.y - f.tail.y) * f.dir.y) / f.len;
    let tau = T_LEAD * u.clamp(0.0, 1.0);
    let rise = smoothstep(tau, tau + T_RISE, f.age);
    if rise <= 0.0 {
        return 0.0;
    }
    let fall = 1.0 - smoothstep(T_FALL, LIFE, f.age);
    let dm = channel_by(p, f, bolt.lv, vertex);
    let df = bolt.forks.distance(p);
    let dh = to_rect(p, hole);
    let w = (W_CORE * c).max(1.0) * (0.45 + 0.55 * fall);
    let gw = W_GLOW * c;
    let km = (dm / w) * (dm / w);
    let kf = (df / (0.7 * w)) * (df / (0.7 * w));
    let core = (-km * km).det_exp().max(0.7 * (-kf * kf).det_exp());
    let (gm, gf) = (dm / gw, df / (0.75 * gw));
    let glow = G_MAX * (-gm * gm).det_exp().max(0.6 * (-gf * gf).det_exp());
    let gc = dh / (W_CORONA * c);
    // No gate of its own: every corona pixel is where the leader arrives
    // last, so the pixel's own rise gates it already.
    let corona = CORONA * (-gc * gc).det_exp();
    (core * fall * fall + (glow + corona) * fall).clamp(0.0, 1.0) * rise
}

/// **Seconds from ignition to when the bolt is out**: its life -- every
/// spark is dead before it.
pub fn end(_: &Ignition) -> f32 {
    LIFE
}

/// **The sparks alive at the ignition's age**: ten, thrown from just outside
/// the cursor as the bolt lands, forward and to the sides but never straight
/// back along it, slowing and falling, fading from full.
pub fn sparks(f: &Ignition) -> impl Iterator<Item = Particle> + '_ {
    (0..SPARKS).filter_map(move |i| {
        let h = Hash(f.seed.0 ^ i.wrapping_mul(0x9E37_79B9) ^ SPARK_SALT).next();
        let (h1, h2, h3, h4) = {
            let h1 = h.next();
            let h2 = h1.next();
            let h3 = h2.next();
            (h1, h2, h3, h3.next())
        };
        let life = 0.10 + 0.10 * h3.unit();
        let a = f.age - T_LEAD;
        if a < 0.0 || a > life {
            return None;
        }
        let c = f.unit;
        let theta = f.dir.y.det_atan2(f.dir.x) + (h1.unit() - 0.5) * 1.6 * core::f32::consts::PI;
        let (s, co) = theta.det_sin_cos();
        let speed = (5.0 + 5.0 * h2.unit()) * c;
        let head = head(f);
        let go = (1.0 - (-DRAG * a).det_exp()) / DRAG;
        let fade = 1.0 - a / life;
        Some(Particle {
            centre: Pt {
                x: head.x + co * 0.55 * c + co * speed * go,
                y: head.y + s * 0.55 * c + s * speed * go + 0.5 * GRAVITY * c * a * a,
            },
            radius: (c * (0.035 + 0.025 * h4.unit())).max(2.0),
            glow: fade * fade,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::tests::{CELL, cell, ignition};
    use crate::effect::{Effect, Seed};
    use crate::trail::{Shape, Smear};

    fn moves() -> Vec<Ignition> {
        let mut v = Vec::new();
        for s in 0..40u32 {
            for (a, b) in [
                (cell(10.0, 5.0), cell(40.0, 5.0)),
                (cell(5.0, 0.0), cell(100.0, 9.0)),
                (cell(60.0, 0.0), cell(62.0, 9.0)),
                (cell(90.0, 8.0), cell(20.0, 1.0)),
                (cell(10.0, 5.0), cell(14.0, 5.0)),
            ] {
                v.push(ignition(a, b, Seed(s)));
            }
        }
        v
    }

    /// **The bolt starts at the tail and ends at the head**, exactly: every
    /// tent is zero at both ends.
    #[test]
    fn the_bolt_is_pinned_at_both_ends() {
        for f in moves() {
            let lv = Levels::of(f.len, f.unit);
            assert_eq!(offset(0.0, lv, &f), 0.0);
            assert_eq!(offset(1.0, lv, &f), 0.0);
        }
    }

    /// **The bolt stays within its bound, its segments are never short, and
    /// it jags**: its offset is at most `CAP` cells a level, every segment is
    /// at least `S_MIN` cells along the path, at most 256, and the kinks
    /// between them average about 17 degrees -- a bolt, not a line.
    #[test]
    fn the_bolt_is_bounded_spaced_and_jagged() {
        let (mut kinks, mut n, mut most) = (0.0f64, 0u32, 0.0f32);
        for f in moves() {
            let lv = Levels::of(f.len, f.unit);
            let m = lv.segments().0;
            assert!(m <= 256);
            if lv.0 > 0 {
                assert!(
                    f.len / m as f32 >= S_MIN * f.unit,
                    "{m} segments of {}",
                    f.len
                );
            }
            let pts: Vec<Pt> = (0..=m).map(|j| vertex(Node(j), lv, &f)).collect();
            for p in &pts {
                let y = (p.x - f.tail.x) * -f.dir.y + (p.y - f.tail.y) * f.dir.x;
                assert!(y.abs() <= CAP * lv.0 as f32 * f.unit + 1e-3);
                most = most.max(y.abs() / f.unit);
            }
            for w in pts.windows(3) {
                let (a, b) = (
                    (w[1].x - w[0].x, w[1].y - w[0].y),
                    (w[2].x - w[1].x, w[2].y - w[1].y),
                );
                let cos = (a.0 * b.0 + a.1 * b.1) / (a.0.det_hypot(a.1) * b.0.det_hypot(b.1));
                kinks += f64::from(cos.clamp(-1.0, 1.0).det_acos().to_degrees());
                n += 1;
            }
        }
        let mean = kinks / f64::from(n);
        assert!((10.0..30.0).contains(&mean), "mean kink {mean} degrees");
        assert!(most > 0.3 && most <= 2.0, "it wanders {most} cells at most");
    }

    /// **The same seed is the same bolt, and another seed another.**
    #[test]
    fn a_seed_is_a_bolt() {
        let (a, b) = (cell(10.0, 5.0), cell(40.0, 5.0));
        let (f, g, h) = (
            ignition(a, b, Seed(3)),
            ignition(a, b, Seed(3)),
            ignition(a, b, Seed(4)),
        );
        let lv = Levels::of(f.len, f.unit);
        let at = |f: &Ignition| (0..=8).map(|j| vertex(Node(j), lv, f)).collect::<Vec<_>>();
        assert_eq!(at(&f), at(&g));
        assert_ne!(at(&f), at(&h));
    }

    /// **The three-segment window is the whole bolt** wherever the glow is
    /// lit: within `S_MIN` cells of the channel, the distance from the three
    /// segments round a pixel's place equals the distance from all of them.
    #[test]
    fn the_window_is_the_whole_bolt_near_it() {
        for f in moves().into_iter().take(40) {
            let lv = Levels::of(f.len, f.unit);
            let m = lv.segments().0;
            let pts: Vec<Pt> = (0..=m).map(|j| vertex(Node(j), lv, &f)).collect();
            for i in 0..600 {
                let u = i as f32 / 599.0 * 1.2 - 0.1;
                for across in [-1.5f32, -0.6, -0.2, 0.0, 0.1, 0.4, 1.2] {
                    let n = normal(f.dir);
                    let p = Pt {
                        x: f.tail.x + f.dir.x * f.len * u + n.x * across * f.unit,
                        y: f.tail.y + f.dir.y * f.len * u + n.y * across * f.unit,
                    };
                    let all = pts
                        .windows(2)
                        .map(|w| segment(p, w[0], w[1]))
                        .fold(f32::INFINITY, f32::min);
                    if all > S_MIN * f.unit {
                        continue;
                    }
                    assert!(
                        (channel(p, &f) - all).abs() < 1e-3,
                        "at {u} across {across}: {} against {all}",
                        channel(p, &f)
                    );
                }
            }
        }
    }

    fn vfx(f: Ignition) -> Vfx {
        let to = Rect {
            left: f.tail.x + f.dir.x * f.len - CELL.0 * 0.5,
            top: f.tail.y + f.dir.y * f.len - CELL.1 * 0.5,
            right: f.tail.x + f.dir.x * f.len + CELL.0 * 0.5,
            bottom: f.tail.y + f.dir.y * f.len + CELL.1 * 0.5,
        };
        Vfx {
            effect: Effect::Lightning,
            emitter: Smear {
                corners: to.corners(),
                hole: to,
                shape: Shape::Quad,
                strength: crate::flash::Strength::Full,
                opacity: Smear::OPAQUE,
            },
            ignition: f,
            axis: f.dir,
        }
    }

    /// **Every pixel sees one flash**: sampled at 480 Hz over the bolt's
    /// life, each pixel's opacity rises once and falls once -- no pixel
    /// peaks twice, which is what keeps a bolt one flash of WCAG 2.3.1's
    /// three a second.
    #[test]
    fn every_pixel_rises_once_and_falls_once() {
        for f in moves().into_iter().take(10) {
            for i in 0..200 {
                let u = i as f32 / 199.0;
                for across in [-0.9f32, -0.3, 0.0, 0.05, 0.3, 1.0] {
                    let n = normal(f.dir);
                    let p = Pt {
                        x: f.tail.x + f.dir.x * f.len * u + n.x * across * f.unit,
                        y: f.tail.y + f.dir.y * f.len * u + n.y * across * f.unit,
                    };
                    let (mut peaks, mut rising, mut last) = (0, false, 0.0f32);
                    for t in 0..=160 {
                        let a = light(&vfx(f.at(t as f32 / 480.0)), p);
                        if a > last + 1.0 / 255.0 {
                            rising = true;
                        } else if a < last - 1.0 / 255.0 {
                            if rising {
                                peaks += 1;
                            }
                            rising = false;
                        }
                        if (a - last).abs() > 1.0 / 255.0 {
                            last = a;
                        }
                    }
                    assert!(peaks <= 1, "{peaks} peaks at {u}, {across}");
                }
            }
        }
    }

    /// **Ten sparks, born as the bolt lands just outside the cursor, never
    /// thrown straight back, fading from full, all dead before the bolt.**
    #[test]
    fn the_sparks_splash_from_the_cursor_and_die_first() {
        let f = ignition(cell(10.0, 5.0), cell(40.0, 5.0), Seed(8));
        assert_eq!(sparks(&f.at(T_LEAD - 0.001)).count(), 0, "none before");
        assert_eq!(sparks(&f.at(T_LEAD)).count(), 10, "ten as it lands");
        assert_eq!(sparks(&f.at(0.25)).count(), 0, "dead by 0.25 s");
        let head = head(&f);
        let mut prev: Vec<f32> = sparks(&f.at(T_LEAD)).map(|s| s.glow).collect();
        for t in 1..20 {
            let now: Vec<Particle> = sparks(&f.at(T_LEAD + t as f32 * 0.01)).collect();
            for s in &now {
                let d = (s.centre.x - head.x).det_hypot(s.centre.y - head.y);
                assert!(d <= 1.6 * f.unit, "{d} from the head");
            }
            if now.len() == prev.len() {
                for (s, &g) in now.iter().zip(&prev) {
                    assert!(s.glow <= g, "fading");
                }
            }
            prev = now.iter().map(|s| s.glow).collect();
        }
        for s in sparks(&f.at(T_LEAD + 0.001)) {
            assert!(s.centre.x > head.x - 0.5 * f.unit, "never straight back");
        }
        assert_eq!(end(&f), LIFE);
        assert_eq!(Effect::Lightning.end(&f), LIFE);
    }

    /// **The region holds the straight path and does not move**: the bolt is
    /// drawn over the same box at every age.
    #[test]
    fn the_region_is_the_path_and_fixed() {
        let f = ignition(cell(10.0, 5.0), cell(40.0, 9.0), Seed(2));
        let r = region(&f);
        for t in [0.0, 0.1, 0.3] {
            assert_eq!(region(&f.at(t)), r);
        }
        for i in 0..=10 {
            let u = i as f32 / 10.0;
            let p = Pt {
                x: f.tail.x + f.dir.x * f.len * u,
                y: f.tail.y + f.dir.y * f.len * u,
            };
            assert!(crate::effect::distance(p, &r) <= 0.0, "the path is in it");
        }
        assert_eq!(
            vfx(f).region(),
            r,
            "the family draws the bolt over this region"
        );
    }

    /// **Nothing is lit before the leader reaches it**: at 10 ms the head's
    /// end of the path is dark and the tail's is lit.
    #[test]
    fn the_leader_draws_the_bolt_in() {
        let f = ignition(cell(10.0, 5.0), cell(40.0, 5.0), Seed(6));
        let v = vfx(f.at(0.010));
        let at = |u: f32| {
            let p = Pt {
                x: f.tail.x + f.dir.x * f.len * u,
                y: f.tail.y + f.dir.y * f.len * u,
            };
            let lv = Levels::of(f.len, f.unit);
            let n = normal(f.dir);
            let y = offset(u, lv, &f);
            light(
                &v,
                Pt {
                    x: p.x + n.x * y,
                    y: p.y + n.y * y,
                },
            )
        };
        assert!(at(0.05) > 0.5, "the tail is lit: {}", at(0.05));
        assert_eq!(at(0.8), 0.0, "the head is not yet");
    }
}
