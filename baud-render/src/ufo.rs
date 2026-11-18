//! **The ufo trail**: a small flying saucer materialises above the cell the
//! cursor left, swoops along the path on a gentle arc, banked into its
//! travel, a faint wake behind it and exhaust rings popping off; it hovers a
//! cell above the cursor with a small bob, lowers a translucent tractor beam
//! onto it with rings travelling down, then climbs away, shrinking and
//! fading. Designed in the effect-trail research (the ufo spec), in the
//! cursor's colour with intensity as opacity.
//!
//! **The flight is a spring onto the quad's centroid.** A critically damped
//! spring from rest (neovide's form) gives the S-curve an object is seen to
//! fly; it springs toward kitty's quad's centroid rather than the cursor, so
//! a retarget -- which moves the centroid continuously while the cursor
//! jumps -- re-flies the same saucer with no jump and no state.
//!
//! **Its bands cover only the saucer, its beam and its wake**
//! ([`Vfx::support`](crate::effect::Vfx::support)): the flight lags kitty's
//! quad by up to eight cells on a hop and thirty-seven on a long jump, so the
//! quad's reach cannot hold it, and what it draws is small.
//!
//! This module is the Rust model of the shader, `ufo_light`, which the unit
//! tests hold to its properties and the pixel oracle's reference calls.

use crate::det::Det;
use crate::effect::{self, Hash, Ignition, Particle, Seed, Vfx, smoothstep};
use crate::trail::{Corners, Pt, Rect};

/// The saucer's hover above the cursor, in cells; its scale; the beam's and
/// the wake's peak opacity; the rim lights' turns a second; the swoop's
/// arc in cells; the bank at full lean in radians; and the earliest age it
/// leaves at.
pub const HOVER: f32 = 1.0;
pub const SIZE: f32 = 1.0;
pub const BEAM: f32 = 0.34;
pub const WAKE: f32 = 0.30;
pub const LIGHT_HZ: f32 = 0.55;
pub const ARC: f32 = 0.50;
pub const TILT: f32 = 0.32;
pub const DEPART: f32 = 0.54;

/// The effect's own numbers, `k` in `EffectU`.
pub const PARAMS: [f32; effect::PARAMS] = [HOVER, SIZE, BEAM, WAKE, LIGHT_HZ, ARC, TILT, DEPART];

/// The longest the saucer is up: the slowest flight's departure, and the
/// 0.18 s it takes to go.
pub const LIFE: f32 = 1.6 * 0.32 + 0.25 + 0.18;

/// **Seconds the flight takes on a path `len` pixels long**, on cells `c`
/// tall: the spring's `4 / w`, from 0.16 s to 0.32 s.
pub fn flight(len: f32, c: f32) -> f32 {
    (0.12 + 0.006 * len / c).clamp(0.16, 0.32)
}

/// **When the saucer leaves**: 1.6 flights and a quarter second after it
/// set off, and never before [`DEPART`]. The spec left at a fixed 0.54 s and
/// opened the beam from 0.15 s, which on a long jump -- a 0.32 s flight,
/// within 0.6 cell of the cursor only near 0.5 s -- opened the beam and shut
/// it again at once: the beam and the departure now follow the flight.
pub fn depart(len: f32, c: f32) -> f32 {
    DEPART.max(1.6 * flight(len, c) + 0.25)
}

/// Cells past its bands' shapes the ufo draws: an anti-aliasing margin, as
/// the shapes already hold everything it draws.
pub const REACH: f32 = 0.1;

/// The ufo effect's rules as [`Effect::rules`](effect::Effect::rules) reads
/// them.
pub const RULES: effect::Rules = effect::Rules {
    fragment: "ufo_fragment",
    reach: REACH,
    life: LIFE,
    end,
};

/// Where the rim lights sit round the rim, as cosines and sines of fifths of
/// a turn -- the same literals as the shader's `UFO_LIGHTS`.
const LIGHTS: [(f32, f32); 5] = [
    (1.0, 0.0),
    (0.309_017, 0.951_057),
    (-0.809_017, 0.587_785),
    (-0.809_017, -0.587_785),
    (0.309_017, -0.951_057),
];

fn length(x: f32, y: f32) -> f32 {
    (x * x + y * y).sqrt()
}

fn fract(x: f32) -> f32 {
    x - x.floor()
}

fn sinpi(x: f32) -> f32 {
    (core::f32::consts::PI * x).det_sin()
}

fn cospi(x: f32) -> f32 {
    (core::f32::consts::PI * x).det_cos()
}

/// **What a flight is flown from**: the path, the quad's centroid the
/// spring pulls toward, the cursor's centre and its rectangle, the unit and
/// the seed.
#[derive(Clone, Copy)]
pub struct Flight {
    tail: Pt,
    len: f32,
    unit: f32,
    seed: Seed,
    cq: Pt,
    hole: Rect,
}

impl Flight {
    /// The flight a frame draws: toward this frame's quad.
    pub fn of(vfx: &Vfx) -> Flight {
        let q = vfx.emitter.corners.0;
        Flight {
            tail: vfx.ignition.tail,
            len: vfx.ignition.len,
            unit: vfx.ignition.unit,
            seed: vfx.ignition.seed,
            cq: Pt {
                x: (q[0].x + q[1].x + q[2].x + q[3].x) * 0.25,
                y: (q[0].y + q[1].y + q[2].y + q[3].y) * 0.25,
            },
            hole: vfx.emitter.hole,
        }
    }

    /// The flight with the quad landed at the ignition's head, which the
    /// exhaust -- a function of the ignition alone -- is shed along.
    fn landed(f: &Ignition) -> Flight {
        let head = Pt {
            x: f.tail.x + f.dir.x * f.len,
            y: f.tail.y + f.dir.y * f.len,
        };
        let c = f.unit;
        Flight {
            tail: f.tail,
            len: f.len,
            unit: c,
            seed: f.seed,
            cq: head,
            hole: Rect {
                left: head.x - 0.25 * c,
                top: head.y - 0.5 * c,
                right: head.x + 0.25 * c,
                bottom: head.y + 0.5 * c,
            },
        }
    }
}

/// **The saucer at an age**: its centre, its bank's cosine and sine, its
/// scale and visibility, its rim lights' turn, which side of the cursor it
/// hovers, how far it still is from the cursor, and its beam's reach and
/// strength.
#[derive(Clone, Copy, Debug)]
pub struct Pose {
    pub c: Pt,
    cs: f32,
    sn: f32,
    pub s: f32,
    pub vis: f32,
    cr: f32,
    sr: f32,
    pub side: f32,
    pub gap: f32,
    pub beam: f32,
    ext: f32,
}

/// The spring's rate: `4 / T`, the flight taking longer on a longer path.
fn omega(len: f32, c: f32) -> f32 {
    4.0 / flight(len, c)
}

/// **The saucer at `a` seconds** -- `ufo_pose` in the shader.
pub fn pose(fl: &Flight, a: f32) -> Pose {
    #[cfg(test)]
    crate::det::tally::count(&crate::det::tally::POSES);
    let c = fl.unit;
    let w = omega(fl.len, c);
    let sig = 1.0 - (1.0 + w * a) * (-w * a).det_exp();
    let on = Pt {
        x: fl.tail.x + (fl.cq.x - fl.tail.x) * sig,
        y: fl.tail.y + (fl.cq.y - fl.tail.y) * sig,
    };
    let hc = Pt {
        x: (fl.hole.left + fl.hole.right) * 0.5,
        y: (fl.hole.top + fl.hole.bottom) * 0.5,
    };
    let (gx, gy) = (hc.x - on.x, hc.y - on.y);
    let gap = length(gx, gy);
    let side = if fl.hole.top >= 1.35 * c { 1.0 } else { -1.0 };
    let (px, py) = (fl.cq.x - fl.tail.x, fl.cq.y - fl.tail.y);
    let pl = length(px, py);
    let (mut nx, mut ny) = if pl > 1e-3 {
        (-py / pl, px / pl)
    } else {
        (0.0, -side)
    };
    if ny * side > 0.0 {
        nx = -nx;
        ny = -ny;
    }
    let arc = (ARC * c).min(0.12 * fl.len) * 4.0 * sig * (1.0 - sig);
    let hov = 1.0 - smoothstep(0.1 * c, 0.6 * c, gap);
    let h1 = Hash(fl.seed.0 ^ 0x5bd1_e995).next().unit();
    let h2 = Hash(fl.seed.0).next().unit();
    let bob = 0.035 * c * sinpi(2.0 * fract(2.2 * a + h1) - 1.0) * hov;
    let t = flight(fl.len, c);
    let dep = depart(fl.len, c);
    let tl = (a - dep).max(0.0);
    let kk = (tl / 0.18).clamp(0.0, 1.0);
    let centre = Pt {
        x: on.x + nx * arc,
        y: on.y + ny * arc + (-side * HOVER * c + bob - side * 46.0 * c * tl * tl),
    };
    let th = TILT * (gx / (3.0 * c)).clamp(-1.0, 1.0)
        + 0.05 * sinpi(2.0 * fract(1.3 * a + h2) - 1.0) * hov;
    let grow = smoothstep(0.0, 0.08, a);
    let rho = 2.0 * fract(LIGHT_HZ * a + h2) - 1.0;
    let ext = smoothstep(1.3 * t, 1.3 * t + 0.1, a) * (1.0 - smoothstep(dep - 0.08, dep, a));
    Pose {
        c: centre,
        cs: th.det_cos(),
        sn: th.det_sin(),
        s: SIZE * (0.5 + 0.5 * grow) * (1.0 - 0.6 * kk * kk),
        vis: grow * (1.0 - smoothstep(0.0, 1.0, kk)),
        cr: cospi(rho),
        sr: sinpi(rho),
        side,
        gap,
        beam: ext * (1.0 - smoothstep(0.15 * c, 0.6 * c, gap)),
        ext,
    }
}

#[cfg(any(test, feature = "posed-saucer"))]
impl Pose {
    /// **The saucer posed directly**, for a flight that is not a cursor's:
    /// centred at `c`, banked `bank` radians, at `scale`, its rim lights
    /// `rim_turns` round, fully visible, its beam down and open to its full
    /// reach. `tools/baud-pictures logo --ufo` flies it through the logo's
    /// letters ([`body_at`], [`beam_at`]). Behind the `posed-saucer` feature.
    #[must_use]
    pub fn posed(c: Pt, bank: f32, scale: f32, rim_turns: f32) -> Pose {
        let rho = 2.0 * fract(rim_turns) - 1.0;
        Pose {
            c,
            cs: bank.det_cos(),
            sn: bank.det_sin(),
            s: scale,
            vis: 1.0,
            cr: cospi(rho),
            sr: sinpi(rho),
            side: 1.0,
            gap: 0.0,
            beam: 1.0,
            ext: 1.0,
        }
    }
}

/// **The saucer's body at `p`**, posed by `pz`, on cells `c` tall: the hull,
/// the dome and the rim lights, as the trail draws them. Behind the
/// `posed-saucer` feature.
#[cfg(any(test, feature = "posed-saucer"))]
#[must_use]
pub fn body_at(p: Pt, pz: &Pose, c: f32) -> f32 {
    body(p, pz, c)
}

/// **The tractor beam at `p`** from the saucer posed by `pz` down onto
/// `hole`, its rings `age` seconds on, before its strength: as the trail
/// draws it. Behind the `posed-saucer` feature.
#[cfg(any(test, feature = "posed-saucer"))]
#[must_use]
pub fn beam_at(p: Pt, pz: &Pose, hole: Rect, c: f32, age: f32) -> f32 {
    beam(p, pz, hole, c, age)
}

fn cov(d: f32) -> f32 {
    (0.5 - d).clamp(0.0, 1.0)
}

/// The hull: a vesica, pointed tips along x.
fn vesica(qx: f32, qy: f32) -> f32 {
    let (px, py) = (qy.abs(), qx.abs());
    if (py - 0.53) * 1.221_818 > px * 0.53 {
        length(px, py - 0.53)
    } else {
        length(px + 1.221_818, py) - 1.331_818
    }
}

/// The dome: iq's cut disk, radius 0.22 cut at its centre, y up.
fn dome(x: f32, y: f32) -> f32 {
    let r = 0.22f32;
    let x = x.abs();
    let s = (-r * x * x + r * r * (r - 2.0 * y)).max(-r * y);
    if s < 0.0 {
        length(x, y) - r
    } else if x < r {
        -y
    } else {
        length(x - r, y)
    }
}

/// iq's quadratic smooth minimum.
fn smin(a: f32, b: f32, k: f32) -> f32 {
    let k = k * 4.0;
    let h = (k - (a - b).abs()).max(0.0) / k;
    a.min(b) - h * h * k * 0.25
}

/// **The saucer's body at `p`**: the hull, the dome and the rim lights.
fn body(p: Pt, pz: &Pose, c: f32) -> f32 {
    let u = pz.s * c;
    let (dx, dy) = (p.x - pz.c.x, p.y - pz.c.y);
    let lim = 0.62 * u + 1.0;
    if dx * dx + dy * dy > lim * lim {
        return 0.0;
    }
    let qx = (pz.cs * dx + pz.sn * dy) / u;
    let qy = (-pz.sn * dx + pz.cs * dy) / u;
    let dh = vesica(qx, qy) - 0.02;
    let sil = smin(dh, dome(qx, -0.04 - qy), 0.03);
    let mut light = 0.0f32;
    if (qy - 0.03).abs() < 0.09 {
        for (lx, ly) in LIGHTS {
            let cphi = pz.cr * lx - pz.sr * ly;
            let sphi = pz.sr * lx + pz.cr * ly;
            let d = length(qx - 0.40 * sphi, qy - 0.03) - 0.045;
            light = light.max(smoothstep(0.0, 0.35, cphi) * cov(d * u));
        }
    }
    (0.50 * cov(sil * u) + cov(dh * u) * (0.30 + 0.20 * light)) * pz.vis
}

/// The beam's axis: from under the saucer to the cursor's near edge.
fn beam_axis(pz: &Pose, hole: Rect, c: f32) -> (Pt, Pt) {
    let top = Pt {
        x: pz.c.x,
        y: pz.c.y + pz.side * 0.117 * c * pz.s,
    };
    let end = Pt {
        x: (hole.left + hole.right) * 0.5,
        y: if pz.side > 0.0 { hole.top } else { hole.bottom },
    };
    (top, end)
}

/// The beam's widest half width, at the cursor.
fn beam_half(hole: Rect, c: f32) -> f32 {
    (0.5 * (hole.right - hole.left) + 0.08 * c).max(0.22 * c)
}

/// **The tractor beam at `p`**: a trapezoid from the saucer to the cursor,
/// a brighter core, and rings travelling down it.
fn beam(p: Pt, pz: &Pose, hole: Rect, c: f32, age: f32) -> f32 {
    if pz.beam <= 0.0 {
        return 0.0;
    }
    let (top, end) = beam_axis(pz, hole, c);
    let (ax, ay) = (end.x - top.x, end.y - top.y);
    let ln = length(ax, ay);
    if ln < 1.0 {
        return 0.0;
    }
    let (nx, ny) = (ax / ln, ay / ln);
    let (dx, dy) = (p.x - top.x, p.y - top.y);
    let along = dx * nx + dy * ny;
    let across = (dx * ny - dy * nx).abs();
    let ff = (along / ln).clamp(0.0, 1.0);
    let hw = 0.14 * c * pz.s + (beam_half(hole, c) - 0.14 * c * pz.s) * ff;
    let edge = (hw - across + 0.5).clamp(0.0, 1.0)
        * (ln * pz.ext - along + 0.5).clamp(0.0, 1.0)
        * (along + 0.5).clamp(0.0, 1.0);
    if edge <= 0.0 {
        return 0.0;
    }
    let core = 1.0 - smoothstep(0.0, 0.6, across / hw.max(1.0));
    let fr = fract(along / (0.22 * c) - fract(5.0 * age));
    BEAM * pz.vis
        * pz.beam
        * edge
        * (0.55 + 0.45 * core)
        * (0.6 + 0.4 * ff)
        * (1.0 - 0.88 * fr * (1.0 - fr))
}

/// The wake's segment: from just behind the saucer to where it was 60 ms
/// ago, or none while that is under a tenth of a cell.
fn wake_line(fl: &Flight, pz: &Pose, age: f32) -> Option<(Pt, Pt)> {
    let c = fl.unit;
    if age <= 0.0 {
        return None;
    }
    let q = pose(fl, (age - 0.06).max(0.0));
    let (vx, vy) = (pz.c.x - q.c.x, pz.c.y - q.c.y);
    let l = length(vx, vy);
    if l < 0.1 * c {
        return None;
    }
    let a = Pt {
        x: pz.c.x - vx / l * (0.2475 * c * pz.s),
        y: pz.c.y - vy / l * (0.2475 * c * pz.s),
    };
    Some((a, q.c))
}

/// **The wake at `p`**: a tapered streak along the saucer's last 60 ms,
/// whose segment is `line` ([`wake_line`]).
fn wake(p: Pt, fl: &Flight, pz: &Pose, line: Option<(Pt, Pt)>) -> f32 {
    let c = fl.unit;
    let Some((a, b)) = line else {
        return 0.0;
    };
    let l = length(pz.c.x - b.x, pz.c.y - b.y);
    let (bx, by) = (b.x - a.x, b.y - a.y);
    let (px, py) = (p.x - a.x, p.y - a.y);
    let h = ((px * bx + py * by) / (bx * bx + by * by).max(1e-6)).clamp(0.0, 1.0);
    let dd = length(px - bx * h, py - by * h) - (0.055 * c + (0.012 * c - 0.055 * c) * h);
    WAKE * pz.vis * cov(dd) * (1.0 - h) * smoothstep(0.1 * c, 0.6 * c, l)
}

/// **The ufo's opacity at `p`**, 0 to 1, before the record's alpha: the
/// wake, the beam over it and the saucer over both -- `ufo_light`.
pub fn light(vfx: &Vfx, p: Pt) -> f32 {
    let f = &vfx.ignition;
    if vfx.emitter.hole.contains(p) || f.age >= end(f) {
        return 0.0;
    }
    lit(p, &Saucer::of(vfx))
}

/// **The ufo as a frame draws it, the same at every pixel**: its flight,
/// its pose, its wake's segment and its age.
struct Saucer {
    fl: Flight,
    pz: Pose,
    wake: Option<(Pt, Pt)>,
    age: f32,
}

impl Saucer {
    fn of(vfx: &Vfx) -> Saucer {
        let age = vfx.ignition.age;
        let fl = Flight::of(vfx);
        let pz = pose(&fl, age);
        Saucer {
            wake: wake_line(&fl, &pz, age),
            fl,
            pz,
            age,
        }
    }
}

/// [`light`] at `p`, outside the cursor and before the end, from the
/// frame's `saucer`.
fn lit(p: Pt, saucer: &Saucer) -> f32 {
    let (fl, pz) = (&saucer.fl, &saucer.pz);
    let mut a = wake(p, fl, pz, saucer.wake);
    let b = beam(p, pz, fl.hole, fl.unit, saucer.age);
    a = b + a * (1.0 - b);
    let s = body(p, pz, fl.unit);
    s + a * (1.0 - s)
}

/// **The ufo's light over a band's pixels**, `xs` by `ys` in whole pixels:
/// `each(x, y, light)` at every pixel, [`light`] at its centre bit for bit,
/// the saucer posed once where the light at each pixel poses it twice.
pub fn shade(
    vfx: &Vfx,
    xs: core::ops::Range<usize>,
    ys: core::ops::Range<usize>,
    each: impl FnMut(usize, usize, f32),
) {
    let f = &vfx.ignition;
    if f.age >= end(f) {
        return;
    }
    let saucer = Saucer::of(vfx);
    let hole = vfx.emitter.hole;
    effect::lights(
        xs,
        ys,
        |p| match hole.contains(p) {
            true => 0.0,
            false => lit(p, &saucer),
        },
        each,
    );
}

/// **The box the ufo draws in this frame**: the saucer's disc, the beam
/// while it is open, and the wake -- the shape its bands cover.
pub fn region(vfx: &Vfx) -> Corners {
    let fl = Flight::of(vfx);
    let age = vfx.ignition.age;
    let pz = pose(&fl, age);
    let c = fl.unit;
    let r = 0.62 * pz.s * c + 1.0;
    let (mut x0, mut y0, mut x1, mut y1) = (pz.c.x - r, pz.c.y - r, pz.c.x + r, pz.c.y + r);
    let mut take = |p: Pt, pad: f32| {
        x0 = x0.min(p.x - pad);
        y0 = y0.min(p.y - pad);
        x1 = x1.max(p.x + pad);
        y1 = y1.max(p.y + pad);
    };
    if pz.beam > 0.0 {
        let (top, end) = beam_axis(&pz, fl.hole, c);
        let pad = beam_half(fl.hole, c) + 1.0;
        take(top, pad);
        take(end, pad);
    }
    if let Some((a, b)) = wake_line(&fl, &pz, age) {
        let pad = 0.055 * c + 1.0;
        take(a, pad);
        take(b, pad);
    }
    Rect {
        left: x0,
        top: y0,
        right: x1,
        bottom: y1,
    }
    .corners()
}

/// **An exhaust ring at `uv`**, -1 to 1 across its record -- the particle
/// branch of `ufo_fragment`.
pub fn ring(uv: Pt) -> f32 {
    1.0 - smoothstep(0.0, 0.22, (length(uv.x, uv.y) - 0.72).abs())
}

/// **The exhaust rings alive at the ignition's age**: shed behind the saucer
/// as it passes, drifting back with drag, growing and fading.
pub fn puffs(f: &Ignition) -> impl Iterator<Item = Particle> + '_ {
    let fl = Flight::landed(f);
    let c = f.unit;
    let n = (2.0 * f.len / c).round().clamp(4.0, 20.0);
    let count = u32::try_from(n as i64).unwrap_or(4);
    let t = 4.0 / omega(f.len, c);
    (0..count).filter_map(move |i| {
        let h0 = Hash(f.seed.0 ^ i.wrapping_mul(0x9E37_79B9)).next();
        let h1 = h0.next();
        let h2 = h1.next();
        let h3 = h2.next();
        let born = 0.02 + (i as f32 + h0.unit()) / n * 0.9 * t;
        let life = 0.20 + 0.12 * h1.unit();
        let a = f.age - born;
        if a < 0.0 || a > life {
            return None;
        }
        let now = pose(&fl, born).c;
        let before = pose(&fl, born - 0.01).c;
        let (vx, vy) = (now.x - before.x, now.y - before.y);
        let l = length(vx, vy).max(1e-6);
        let (bx, by) = (-vx / l, -vy / l);
        let p0 = Pt {
            x: now.x + bx * 0.5 * 0.55 * c,
            y: now.y + by * 0.5 * 0.55 * c,
        };
        let sp = (1.2 + 1.0 * h2.unit()) * c;
        let off = (h3.unit() - 0.5) * c;
        let (v0x, v0y) = (bx * sp - by * off, by * sp + bx * off);
        let go = (1.0 - (-5.0 * a).det_exp()) / 5.0;
        let k = 1.0 - a / life;
        Some(Particle {
            centre: Pt {
                x: p0.x + v0x * go,
                y: p0.y + v0y * go,
            },
            radius: (c * (0.05 + 0.12 * a / life)).max(2.0),
            glow: 0.45 * k * k,
        })
    })
}

/// **Seconds from ignition to when the ufo is out**: when it has left; every
/// ring is gone before.
pub fn end(f: &Ignition) -> f32 {
    depart(f.len, f.unit) + 0.18
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::Effect;
    use crate::effect::tests::{CELL, cell, ignition};
    use crate::trail::{Segment, Shape, Smear};

    fn vfx(from: Rect, to: Rect, age: f32) -> Vfx {
        let f = ignition(from, to, Seed(6));
        Vfx {
            effect: Effect::Ufo,
            emitter: Smear {
                corners: Segment::new(from.corners(), to).at(age),
                hole: to,
                shape: Shape::Quad,
                strength: crate::flash::Strength::Full,
                opacity: Smear::OPAQUE,
            },
            ignition: f.at(age),
            axis: f.dir,
        }
    }

    /// **The saucer leaves from above the tail, arrives over the cursor and
    /// hovers a cell above it**, and on the top rows below it.
    #[test]
    fn the_saucer_flies_to_hover_over_the_cursor() {
        let (from, to) = (cell(10.0, 8.0), cell(40.0, 7.0));
        let v = vfx(from, to, 0.0);
        let p0 = pose(&Flight::of(&v), 0.0);
        assert!(
            (p0.c.x - v.ignition.tail.x).abs() < 1e-3,
            "it leaves from the tail"
        );
        // At 0.5 s, before it leaves at 0.54: the spring is within a pixel.
        let v = vfx(from, to, 0.5);
        let p = pose(&Flight::of(&v), 0.5);
        let hc = (to.left + to.right) * 0.5;
        assert!(
            (p.c.x - hc).abs() < 0.1 * CELL.1,
            "over the cursor: {}",
            p.c.x
        );
        let above = (to.top + to.bottom) * 0.5 - p.c.y;
        assert!(
            (above - CELL.1).abs() < 0.1 * CELL.1,
            "a cell above: {above}"
        );
        let top = cell(40.0, 0.0);
        let v = vfx(cell(10.0, 0.0), top, 0.35);
        let p = pose(&Flight::of(&v), 0.35);
        assert!(p.c.y > (top.top + top.bottom) * 0.5, "below on the top row");
    }

    /// **The beam opens only over the cursor**: nothing while the saucer is
    /// more than 0.6 cell away, and lit between the saucer and the cursor
    /// once it hovers -- and the cursor itself is bare.
    #[test]
    fn the_beam_opens_over_the_cursor() {
        let (from, to) = (cell(10.0, 8.0), cell(40.0, 7.0));
        let early = vfx(from, to, 0.16);
        let pz = pose(&Flight::of(&early), 0.16);
        if pz.gap > 0.6 * CELL.1 {
            assert_eq!(pz.beam, 0.0);
        }
        let v = vfx(from, to, 0.45);
        let pz = pose(&Flight::of(&v), 0.45);
        assert!(pz.beam > 0.9, "the beam at 0.45 s: {}", pz.beam);
        let hc = (to.left + to.right) * 0.5;
        let mid = Pt {
            x: hc,
            y: to.top - 0.3 * CELL.1,
        };
        assert!(light(&v, mid) > 0.1, "the beam: {}", light(&v, mid));
        let inside = Pt {
            x: hc,
            y: (to.top + to.bottom) * 0.5,
        };
        assert_eq!(light(&v, inside), 0.0, "the cursor is bare");
    }

    /// A frame of a flight from `from` to `to` whose quad was retargeted at
    /// `at` to `then`, at `age`: the ignition keeps its first move, the quad
    /// and the cursor head elsewhere.
    fn retargeted(from: Rect, to: Rect, then: Rect, at: f32, age: f32) -> Vfx {
        let f = ignition(from, to, Seed(6));
        let seg = Segment::new(from.corners(), to);
        Vfx {
            effect: Effect::Ufo,
            emitter: Smear {
                corners: Segment::new(seg.at(at), then).at(age - at),
                hole: then,
                shape: Shape::Quad,
                strength: crate::flash::Strength::Full,
                opacity: Smear::OPAQUE,
            },
            ignition: f.at(age),
            axis: f.dir,
        }
    }

    /// **The beam waits for the saucer**: a retarget as the beam's window
    /// opens sends the saucer off again, and while it is more than 0.6 cell
    /// from the new cursor no beam is drawn, window or not.
    #[test]
    fn the_beam_waits_for_the_saucer_over_the_cursor() {
        let (from, to, then) = (cell(10.0, 8.0), cell(40.0, 7.0), cell(52.0, 10.0));
        let mut checked = 0;
        for i in 0..20 {
            let age = 0.29 + i as f32 * 0.005;
            let v = retargeted(from, to, then, 0.28, age);
            let pz = pose(&Flight::of(&v), age);
            if pz.gap > 0.6 * CELL.1 && pz.ext > 0.5 {
                assert_eq!(
                    pz.beam, 0.0,
                    "a beam at {age} with the saucer {} away",
                    pz.gap
                );
                checked += 1;
            }
        }
        assert!(
            checked > 3,
            "only {checked} frames had the window open and the saucer away"
        );
    }

    /// **Every move gets its beam, a long jump too**: the beam opens fully
    /// once the saucer is over the cursor and before it leaves -- which the
    /// spec's fixed window missed on a long jump.
    #[test]
    fn a_long_jump_still_opens_its_beam() {
        for (from, to) in [
            (cell(10.0, 8.0), cell(14.0, 8.0)),
            (cell(10.0, 8.0), cell(40.0, 7.0)),
            (cell(2.0, 2.0), cell(110.0, 14.0)),
        ] {
            let f = ignition(from, to, Seed(6));
            let leaves = depart(f.len, f.unit);
            let most = (0..200)
                .map(|i| i as f32 * 0.005)
                .filter(|&a| a < leaves)
                .map(|a| pose(&Flight::of(&vfx(from, to, a)), a).beam)
                .fold(0.0f32, f32::max);
            assert!(most > 0.95, "{from:?} to {to:?}: the beam reached {most}");
        }
    }

    /// **The rim lights are on the rim**: some pixel near the rim is lit by
    /// a light brighter than the hull there is without one.
    #[test]
    fn the_rim_lights_shine() {
        let (from, to) = (cell(10.0, 8.0), cell(40.0, 7.0));
        let v = vfx(from, to, 0.3);
        let pz = pose(&Flight::of(&v), 0.3);
        let c = CELL.1;
        let (mut most, mut plain) = (0.0f32, 1.0f32);
        for i in -30..=30 {
            let q = i as f32 / 30.0 * 0.45;
            let p = Pt {
                x: pz.c.x + q * pz.s * c * pz.cs,
                y: pz.c.y + q * pz.s * c * pz.sn + 0.03 * pz.s * c,
            };
            let b = body(p, &pz, c);
            most = most.max(b);
            plain = plain.min(b);
        }
        assert!(most > plain + 0.1, "lights {most} over {plain}");
    }

    /// **A saucer posed directly draws as the trail's**: at the hover of a
    /// flight, its beam fully out and the saucer fully seen, the posed body
    /// and beam -- its bank and rim read back off the flight's pose -- equal
    /// the flight's own at every pixel of the saucer and the beam, the beam
    /// before the flight's strength.
    #[test]
    fn a_saucer_posed_directly_draws_as_the_trails() {
        let (from, to) = (cell(10.0, 8.0), cell(40.0, 7.0));
        let age = 0.45;
        let v = vfx(from, to, age);
        let pz = pose(&Flight::of(&v), age);
        assert_eq!((pz.vis, pz.s, pz.ext), (1.0, 1.0, 1.0), "{pz:?}");
        let bank = pz.sn.det_atan2(pz.cs);
        let rim = 0.5 * (pz.sr.det_atan2(pz.cr) / core::f32::consts::PI + 1.0);
        let posed = Pose::posed(pz.c, bank, pz.s, rim);
        let c = CELL.1;
        let (mut bodies, mut beams) = (0, 0);
        for y in -20..=60 {
            for x in -20..=20 {
                let p = Pt {
                    x: pz.c.x + x as f32 * 0.05 * c,
                    y: pz.c.y + y as f32 * 0.05 * c,
                };
                let (a, b) = (body(p, &pz, c), body_at(p, &posed, c));
                assert!((a - b).abs() < 1e-4, "the body at {p:?}: {a} and {b}");
                bodies += usize::from(a > 0.0);
                let (a, b) = (beam(p, &pz, to, c, age), beam_at(p, &posed, to, c, age));
                assert!(
                    (a - b * pz.beam).abs() < 1e-5,
                    "the beam at {p:?}: {a} and {b}"
                );
                beams += usize::from(a > 0.0);
            }
        }
        assert!(bodies > 50 && beams > 50, "{bodies} {beams}");
    }

    /// **A retarget re-flies the saucer without a jump**: the pose 20 us
    /// either side of a retarget moves under half a pixel. Wider, kitty's
    /// fast corners -- which a retarget starts afresh -- move the centroid
    /// several pixels a millisecond, which is speed, not a jump.
    #[test]
    fn a_retarget_moves_the_saucer_smoothly() {
        let (from, to, then) = (cell(10.0, 8.0), cell(40.0, 7.0), cell(20.0, 12.0));
        let f = ignition(from, to, Seed(6));
        let at = |age: f32, retarget: bool| {
            let emitter = if retarget {
                let seg = Segment::new(from.corners(), to);
                Segment::new(seg.at(0.2), then).at(age - 0.2)
            } else {
                Segment::new(from.corners(), to).at(age)
            };
            let v = Vfx {
                effect: Effect::Ufo,
                emitter: Smear {
                    corners: emitter,
                    hole: if retarget { then } else { to },
                    shape: Shape::Quad,
                    strength: crate::flash::Strength::Full,
                    opacity: Smear::OPAQUE,
                },
                ignition: f.at(age),
                axis: f.dir,
            };
            pose(&Flight::of(&v), age).c
        };
        let (a, b) = (at(0.19998, false), at(0.20002, true));
        assert!(length(a.x - b.x, a.y - b.y) < 0.5, "{a:?} to {b:?}");
    }

    /// **Every pixel the ufo lights is in a band**: the bands cover the box
    /// `region` names, which must hold the saucer, the beam and the wake.
    #[test]
    fn every_pixel_the_ufo_lights_is_in_a_band() {
        let frame = (baud_font::Px::of(1400), baud_font::Px::of(500));
        for (from, to) in [
            (cell(10.0, 8.0), cell(40.0, 7.0)),
            (cell(70.0, 2.0), cell(5.0, 12.0)),
            (cell(30.0, 0.0), cell(60.0, 0.0)),
        ] {
            // The retarget sends the saucer a few cells on as its beam opens,
            // so the beam slants from a saucer still settling onto a wide
            // cursor -- the one beam the saucer's own disc does not hold. Those frames are found:
            // the window open and the saucer between 0.2 and 0.55 cell out.
            // Two cells wide, as the cursor is on a wide character: the
            // beam's foot is then wider than the saucer's disc reaches.
            let then = Rect {
                left: to.left + 4.0 * CELL.0,
                right: to.right + 5.0 * CELL.0,
                ..to
            };
            let slanted: Vec<f32> = (0..125)
                .map(|i| 0.30 + i as f32 * 0.002)
                .filter(|&a| {
                    let pz = pose(&Flight::of(&retargeted(from, to, then, 0.3, a)), a);
                    pz.ext > 0.5 && pz.gap > 0.2 * CELL.1 && pz.gap < 0.55 * CELL.1
                })
                .step_by(3)
                .take(4)
                .collect();
            assert!(slanted.len() >= 2, "{from:?}: slanted frames {slanted:?}");
            let frames = [0.05f32, 0.12, 0.3, 0.5, 0.6]
                .into_iter()
                .map(|a| (a, false))
                .chain(slanted.into_iter().map(|a| (a, true)));
            for (age, retarget) in frames {
                let v = if retarget {
                    retargeted(from, to, then, 0.3, age)
                } else {
                    vfx(from, to, age)
                };
                let mut bands = Vec::new();
                v.instances(frame, crate::color::Argb(0xFFFF_FFFF), &mut bands);
                bands.retain(|r| r.kind == crate::atlas::Kind::Band);
                let mut lit = 0;
                for y in 0..500 {
                    for x in 0..1400 {
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
                            x >= x0
                                && x < x0 + i32::from(r.w.0)
                                && y >= y0
                                && y < y0 + i32::from(r.h.0)
                        });
                        assert!(covered, "{x},{y} at {age} lit outside every band");
                    }
                }
                assert!(lit > 100, "only {lit} lit at {age}");
            }
        }
    }

    /// **Exhaust rings are shed along the flight, fade, and are gone before
    /// the saucer leaves.**
    #[test]
    fn exhaust_rings_fade_before_it_leaves() {
        let f = ignition(cell(10.0, 8.0), cell(40.0, 7.0), Seed(6));
        assert!(puffs(&f.at(0.1)).count() > 0);
        assert_eq!(puffs(&f.at(0.64)).count(), 0);
        assert_eq!(Effect::Ufo.end(&f), end(&f));
        assert!(end(&f) <= LIFE);
        assert!(ring(Pt { x: 0.72, y: 0.0 }) > 0.99);
        assert_eq!(ring(Pt { x: 0.0, y: 0.0 }), 0.0, "a ring, not a disc");
    }
}
