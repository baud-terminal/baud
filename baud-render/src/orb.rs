//! **The orb trail**: a glowing sphere of the cursor's colour that flies
//! from the cell the cursor left to the one it reached, a comet's tail behind
//! it and a soft halo round it, and bursts into sparks as it sinks into the
//! cursor. Designed here rather than in the effect-trail research, which had
//! no orb spec; in the cursor's colour with intensity as opacity, GPU only.
//!
//! **The flight is a spring, not kitty's corners.** kitty's leading corners
//! cover 87% of a move in 30 ms, too fast to see an object travel; the orb
//! rides a critically damped spring from rest (neovide's form), which
//! accelerates, arrives and settles without overshoot in about a fifth of a
//! second. It springs toward the quad's centroid rather than the cursor, so a
//! retarget -- which moves the centroid continuously while the cursor jumps --
//! re-flies it with no jump and no state, the ufo spec's device.
//!
//! **Its bands cover the line it can fly on**, from the tail to the quad's
//! centroid, widened by its reach ([`Vfx::support`]), not the quad: the orb
//! lags kitty's corners, so late in a flight it is outside the quad. The
//! comet is the orb's own last 80 ms of flight, on the same line.
//!
//! This module is the Rust model of the shader, `orb_light`, which the unit
//! tests hold to its properties and the pixel oracle's reference calls.

use crate::effect::{self, Hash, Ignition, Particle, Vfx};
use crate::trail::{Corners, Pt};

/// The orb's radius, its halo's width and peak, and its comet's peak.
pub const RADIUS: f32 = 0.40;
pub const HALO_W: f32 = 0.35;
pub const HALO: f32 = 0.35;
pub const COMET: f32 = 0.55;
/// Seconds of flight the comet shows.
pub const COMET_S: f32 = 0.08;
/// Seconds the orb takes to appear, and the fade after it arrives.
pub const APPEAR: f32 = 0.04;
pub const FADE: f32 = 0.20;
/// Seconds after its arrival the fade starts.
pub const LINGER: f32 = 0.05;

/// The effect's own numbers, `k` in `EffectU`.
pub const PARAMS: [f32; effect::PARAMS] =
    [RADIUS, HALO_W, HALO, COMET, COMET_S, APPEAR, FADE, LINGER];

/// Cells from the orb's line past which nothing is drawn: the orb, and its
/// halo to under half a step of 255 -- `0.35 * e^-(x/0.35)^2 < 0.5/255` from
/// 0.8 cell out.
pub const REACH: f32 = RADIUS + 0.85;

/// The longest the orb lasts: the slowest flight and its fade.
pub const LIFE: f32 = 0.25 + LINGER + FADE;

/// Sparks it bursts into, their drag and their salt.
const SPARKS: u32 = 12;
const DRAG: f32 = 8.0;
const SPARK_SALT: u32 = 0x0B0B_0B0B;

/// **Seconds the orb's flight takes on a path `len` pixels long**, on cells
/// `c` tall: longer for a longer move, from 0.12 s to 0.25 s.
pub fn flight(len: f32, c: f32) -> f32 {
    (0.10 + 0.004 * len / c).clamp(0.12, 0.25)
}

/// How far along its flight the orb is at `age`, 0 to 1: a critically
/// damped spring from rest, `1 - (1 + w a) e^(-w a)`, `w = 4 / T`.
pub fn along(age: f32, len: f32, c: f32) -> f32 {
    let w = 4.0 / flight(len, c);
    1.0 - (1.0 + w * age) * (-w * age).exp()
}

fn centroid(c: &Corners) -> Pt {
    let q = c.0;
    Pt {
        x: (q[0].x + q[1].x + q[2].x + q[3].x) * 0.25,
        y: (q[0].y + q[1].y + q[2].y + q[3].y) * 0.25,
    }
}

/// **Where the orb is at `age`**: on the line from the tail to the quad's
/// centroid, as far as the spring has taken it.
pub fn centre(vfx: &Vfx, age: f32) -> Pt {
    let f = &vfx.ignition;
    let cq = centroid(&vfx.emitter.corners);
    let s = along(age, f.len, f.unit);
    Pt {
        x: f.tail.x + (cq.x - f.tail.x) * s,
        y: f.tail.y + (cq.y - f.tail.y) * s,
    }
}

/// **The line the orb can fly on**, tail to the quad's centroid, which the
/// bands cover to [`REACH`].
pub fn line(vfx: &Vfx) -> Corners {
    let t = vfx.ignition.tail;
    let cq = centroid(&vfx.emitter.corners);
    Corners([t, cq, cq, t])
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn length(x: f32, y: f32) -> f32 {
    (x * x + y * y).sqrt()
}

/// **The orb's opacity at `p`**, 0 to 1, before the record's alpha: the
/// sphere with its highlight, the halo and the comet, under one envelope --
/// `orb_light` in the shader.
pub fn light(vfx: &Vfx, p: Pt) -> f32 {
    let f = &vfx.ignition;
    if vfx.emitter.hole.contains(p) {
        return 0.0;
    }
    let c = f.unit;
    let t = flight(f.len, c);
    let end = t + LINGER + FADE;
    if f.age >= end {
        return 0.0;
    }
    if effect::distance(p, &line(vfx)) / c > REACH {
        return 0.0;
    }
    let env = smoothstep(0.0, APPEAR, f.age) * (1.0 - smoothstep(t + LINGER, end, f.age));
    let o = centre(vfx, f.age);
    let r = RADIUS * c;
    let d = length(p.x - o.x, p.y - o.y);
    let cov = (0.5 - (d - r)).clamp(0.0, 1.0);
    let k = (d / r).min(1.0);
    let (hx, hy) = (o.x - 0.35 * r, o.y - 0.35 * r);
    let hs = length(p.x - hx, p.y - hy) / (0.30 * r);
    let body = (0.45 + 0.30 * (1.0 - k * k) + 0.25 * (-hs * hs).exp()) * cov;
    let hd = (d - r).max(0.0) / (HALO_W * c);
    let halo = HALO * (-hd * hd).exp();
    let q = centre(vfx, (f.age - COMET_S).max(0.0));
    let (bx, by) = (q.x - o.x, q.y - o.y);
    let (px, py) = (p.x - o.x, p.y - o.y);
    let h = ((px * bx + py * by) / (bx * bx + by * by).max(1e-6)).clamp(0.0, 1.0);
    let ds = length(px - bx * h, py - by * h);
    let cw = r * (0.8 - 0.6 * h);
    let cd = ds / cw;
    let comet = COMET * (1.0 - h) * (1.0 - h) * (-cd * cd).exp();
    (body.max(halo).max(comet) * env).clamp(0.0, 1.0)
}

/// **Seconds from ignition to when the orb is out**: its flight, the linger,
/// its fade, and its last spark.
pub fn end(f: &Ignition) -> f32 {
    let t = flight(f.len, f.unit);
    burst(f)
        .map(|h| {
            let s = Spark::draw(f, h);
            s.born + s.life
        })
        .fold(t + LINGER + FADE, f32::max)
}

struct Spark {
    born: f32,
    life: f32,
    h: [Hash; 4],
}

/// The first hash of each spark of an ignition.
fn burst(f: &Ignition) -> impl Iterator<Item = Hash> + use<> {
    let seed = f.seed.0;
    (0..SPARKS).map(move |i| Hash(seed ^ i.wrapping_mul(0x9E37_79B9) ^ SPARK_SALT).next())
}

impl Spark {
    fn draw(f: &Ignition, h0: Hash) -> Spark {
        let h1 = h0.next();
        let h2 = h1.next();
        let h3 = h2.next();
        Spark {
            born: flight(f.len, f.unit) * (0.85 + 0.15 * h0.unit()),
            life: 0.18 + 0.12 * h1.unit(),
            h: [h0, h1, h2, h3],
        }
    }
}

/// **The sparks alive at the ignition's age**: twelve, burst from the head in
/// every direction as the orb arrives, slowing and fading from full.
pub fn sparks(f: &Ignition) -> impl Iterator<Item = Particle> + '_ {
    burst(f).filter_map(move |h| {
        let s = Spark::draw(f, h);
        let a = f.age - s.born;
        if a < 0.0 || a > s.life {
            return None;
        }
        let c = f.unit;
        let [_, _, h2, h3] = s.h;
        let th = core::f32::consts::TAU * h2.unit();
        let (sn, cs) = th.sin_cos();
        let speed = (2.5 + 3.0 * h3.unit()) * c;
        let go = (1.0 - (-DRAG * a).exp()) / DRAG;
        let start = RADIUS * c;
        let head = Pt {
            x: f.tail.x + f.dir.x * f.len,
            y: f.tail.y + f.dir.y * f.len,
        };
        let fade = 1.0 - a / s.life;
        Some(Particle {
            centre: Pt {
                x: head.x + cs * (start + speed * go),
                y: head.y + sn * (start + speed * go),
            },
            radius: (0.05 * c).max(2.0),
            glow: 0.8 * fade * fade,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::tests::{CELL, cell, ignition};
    use crate::effect::{Effect, Seed};
    use crate::trail::{Rect, Segment, Shape, Smear};

    fn vfx(from: Rect, to: Rect, age: f32) -> Vfx {
        let f = ignition(from, to, Seed(4));
        Vfx {
            effect: Effect::Orb,
            emitter: Smear {
                corners: Segment::new(from.corners(), to).at(age),
                hole: to,
                shape: Shape::Quad,
            },
            ignition: f.at(age),
            axis: f.dir,
        }
    }

    /// **The orb leaves the tail from rest and arrives without overshoot**:
    /// the spring starts at nothing with no speed -- a fiftieth of its flight
    /// in, it has gone under half a percent of the way, where a steady speed
    /// would have gone two -- never passes the end or turns back, and is
    /// within 2% of it by one and a half flights. At the start the orb is on
    /// the tail.
    #[test]
    fn the_orb_springs_from_the_tail_to_the_cursor() {
        let (from, to) = (cell(10.0, 5.0), cell(40.0, 5.0));
        let v0 = vfx(from, to, 0.0);
        let o = centre(&v0, 0.0);
        assert!((o.x - v0.ignition.tail.x).abs() < 1e-3);
        let (len, c) = (v0.ignition.len, CELL.1);
        let t = flight(len, c);
        assert_eq!(along(0.0, len, c), 0.0);
        let early = along(0.02 * t, len, c);
        assert!(early < 0.005, "from rest: {early}");
        let mut last = 0.0;
        for i in 1..=200 {
            let s = along(i as f32 * 0.01 * t, len, c);
            assert!(s >= last && s <= 1.0, "{s} after {last}");
            last = s;
        }
        let near = along(1.5 * t, len, c);
        assert!(near > 0.98, "{near} by one and a half flights");
    }

    /// **The comet is behind the orb and the cursor stays bare**: a pixel
    /// behind the orb on its path is lit, one as far ahead is darker, and the
    /// hole draws nothing.
    #[test]
    fn the_comet_trails_and_the_cursor_is_bare() {
        let (from, to) = (cell(10.0, 5.0), cell(40.0, 5.0));
        let v = vfx(from, to, 0.06);
        let o = centre(&v, 0.06);
        let r = RADIUS * CELL.1;
        let behind = light(
            &v,
            Pt {
                x: o.x - 1.5 * r,
                y: o.y,
            },
        );
        let ahead = light(
            &v,
            Pt {
                x: o.x + 1.5 * r,
                y: o.y,
            },
        );
        assert!(behind > ahead + 0.1, "comet {behind}, ahead {ahead}");
        assert!(light(&v, o) > 0.5, "the orb itself");
        let beside = light(
            &v,
            Pt {
                x: o.x,
                y: o.y + 1.5 * r,
            },
        );
        assert!(beside > 0.15, "a halo beside it: {beside}");
        let mid = Pt {
            x: (to.left + to.right) * 0.5,
            y: (to.top + to.bottom) * 0.5,
        };
        assert_eq!(light(&vfx(from, to, 0.3), mid), 0.0);
    }

    /// **The orb fades out rather than being cut**: just outside the cursor
    /// on the side it came from, where it sits as it arrives, it is bright
    /// when the fade starts and all but gone just before its end.
    #[test]
    fn the_orb_fades_before_it_goes_out() {
        let (from, to) = (cell(10.0, 5.0), cell(40.0, 5.0));
        let f = ignition(from, to, Seed(4));
        let t = flight(f.len, CELL.1);
        let p = Pt {
            x: to.left - 3.0,
            y: (to.top + to.bottom) * 0.5,
        };
        let lit = light(&vfx(from, to, t + LINGER), p);
        assert!(lit > 0.3, "{lit} as the fade starts");
        let late = light(&vfx(from, to, t + LINGER + FADE - 0.005), p);
        assert!(late < 0.02, "{late} just before the end");
    }

    /// **Nothing past the reach of its line, and nothing after its end.**
    #[test]
    fn the_orb_stays_in_reach_and_goes_out() {
        let (from, to) = (cell(10.0, 5.0), cell(40.0, 9.0));
        let f = ignition(from, to, Seed(4));
        let out = end(&f);
        assert!(out > flight(f.len, CELL.1) + LINGER + FADE - 1e-4);
        assert!(out <= LIFE + 0.4, "out at {out}");
        let v = vfx(from, to, out + 0.001);
        for y in 0..20 {
            for x in 0..80 {
                let p = Pt {
                    x: x as f32 * 17.0,
                    y: y as f32 * 17.0,
                };
                assert_eq!(light(&v, p), 0.0);
            }
        }
        assert_eq!(sparks(&f.at(out + 0.001)).count(), 0);
        assert_eq!(Effect::Orb.end(&f), out);
    }

    /// **The cut at the reach draws no edge**: just inside [`REACH`] of the
    /// line, square to it from the orb, the halo is under half a step of 255,
    /// so nothing the cut removes could have shown -- across and down, through
    /// the flight and the linger.
    #[test]
    fn the_reach_cuts_nothing_visible() {
        for (from, to) in [
            (cell(10.0, 5.0), cell(40.0, 5.0)),
            (cell(20.0, 2.0), cell(20.0, 14.0)),
        ] {
            let across = from.top == to.top;
            let f = ignition(from, to, Seed(4));
            let t = flight(f.len, CELL.1);
            for age in [0.4 * t, t, t + LINGER] {
                let v = vfx(from, to, age);
                let o = centre(&v, age);
                let off = REACH * CELL.1 - 0.5;
                for side in [-1.0, 1.0] {
                    let p = if across {
                        Pt {
                            x: o.x,
                            y: o.y + side * off,
                        }
                    } else {
                        Pt {
                            x: o.x + side * off,
                            y: o.y,
                        }
                    };
                    let edge = light(&v, p);
                    assert!(edge < 0.5 / 255.0, "{edge} at {age} just inside the reach");
                }
            }
        }
    }

    /// **Every pixel the orb lights is in a band**: the bands cover the line
    /// `support` names, which must hold the orb, its halo and its comet --
    /// over a hop, a diagonal and a retarget whose quad heads elsewhere.
    #[test]
    fn every_pixel_the_orb_lights_is_in_a_band() {
        let frame = (baud_font::Px::of(1400), baud_font::Px::of(500));
        for (from, to, then) in [
            (cell(10.0, 5.0), cell(40.0, 5.0), None),
            (cell(5.0, 1.0), cell(70.0, 12.0), None),
            (cell(10.0, 5.0), cell(40.0, 5.0), Some(cell(30.0, 12.0))),
        ] {
            for age in [0.03f32, 0.08, 0.15, 0.25] {
                let mut v = vfx(from, to, age);
                if let Some(t) = then {
                    let seg = Segment::new(from.corners(), to);
                    v.emitter.corners = Segment::new(seg.at(0.02), t).at(age - 0.02);
                    v.emitter.hole = t;
                }
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
                assert!(lit > 500, "only {lit} lit at {age}");
            }
        }
    }

    /// **Twelve sparks burst from the head as the orb arrives, spread all
    /// round and fading.**
    #[test]
    fn sparks_burst_all_round_on_arrival() {
        let f = ignition(cell(10.0, 5.0), cell(40.0, 5.0), Seed(2));
        let t = flight(f.len, f.unit);
        assert_eq!(sparks(&f.at(0.8 * t)).count(), 0, "none mid-flight");
        let at = f.at(t + 0.01);
        let live: Vec<Particle> = sparks(&at).collect();
        assert_eq!(live.len(), 12);
        let head = Pt {
            x: f.tail.x + f.dir.x * f.len,
            y: f.tail.y + f.dir.y * f.len,
        };
        let (mut left, mut right) = (0, 0);
        for s in &live {
            if s.centre.x < head.x {
                left += 1;
            } else {
                right += 1;
            }
        }
        assert!(left > 1 && right > 1, "all round: {left} and {right}");
        let glow = |a: f32| sparks(&f.at(a)).map(|s| s.glow).sum::<f32>();
        assert!(glow(t + 0.1) < glow(t + 0.01), "fading");
    }
}
