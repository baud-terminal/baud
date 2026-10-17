//! **The orb trail**: a glowing sphere of the cursor's colour that flies
//! from the cell the cursor left to the one it reached, a comet's tail behind
//! it and a soft halo round it, and bursts into sparks as it lands in the
//! cursor. Designed here rather than in the effect-trail research, which had
//! no orb spec; in the cursor's colour with intensity as opacity, GPU only.
//!
//! **The flight is a smootherstep over a fixed time, not kitty's corners.**
//! kitty's leading corners cover 87% of a move in 30 ms, too fast to see an
//! object travel. The orb leaves the tail from rest and lands at rest
//! exactly [`flight`] seconds later, 0.12 s on a short move and 0.25 s on a
//! long one, so its burst and its fade are timed off an arrival that happens.
//! A spring only approaches its arrival, and this one burst ahead of the orb.
//!
//! **Its aim is the cursor, reached through kitty's leading corners.** The
//! aim ([`aim`]) is the cursor's centre, displaced as far as the quad's
//! corners facing the ignition's direction still are from the cursor's. It
//! starts at the tail and reaches the cursor as fast as those corners do,
//! within a pixel by the end of the flight on every move measured. It moves
//! continuously when the cursor jumps, because the corners do, so a retarget
//! re-flies the orb with no jump and no state. After a retarget the corners
//! facing the first move may be the new move's slow ones, and the orb lands
//! on the new cursor only as fast as they arrive.
//!
//! **Its bands cover the line it can fly on**, from the tail to its aim,
//! widened by its reach ([`Vfx::support`]), not the quad: the orb trails
//! kitty's corners, and is outside the quad for much of its flight. The comet
//! is the orb's own last 80 ms of flight, on the same line.
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

/// **How far along its flight the orb is at `age`**, 0 to 1: a smootherstep
/// over the flight, `u^3 (10 - 15 u + 6 u^2)` with `u = age / T`, which
/// leaves from rest, lands at rest, and is 1 exactly at `T` and after.
pub fn along(age: f32, len: f32, c: f32) -> f32 {
    let u = (age / flight(len, c)).clamp(0.0, 1.0);
    u * u * u * (u * (u * 6.0 - 15.0) + 10.0)
}

/// **Where the orb is flying to**: the cursor's centre, displaced by the
/// mean of how far the quad's corners facing the ignition's direction --
/// those past the cursor's centre along it -- still are from the cursor's.
/// The tail at ignition, the cursor as those corners arrive, and
/// continuous when the cursor jumps. A cursor with no corner past its
/// centre, which only an empty one is, is its own centre.
pub fn aim(vfx: &Vfx) -> Pt {
    let (hole, dir) = (vfx.emitter.hole, vfx.ignition.dir);
    let m = hole.center();
    let (mut sx, mut sy, mut n) = (0.0f32, 0.0f32, 0.0f32);
    for (h, q) in hole.corners().0.iter().zip(vfx.emitter.corners.0.iter()) {
        if (h.x - m.x) * dir.x + (h.y - m.y) * dir.y > 0.0 {
            sx += q.x - h.x;
            sy += q.y - h.y;
            n += 1.0;
        }
    }
    if n == 0.0 {
        m
    } else {
        Pt {
            x: m.x + sx / n,
            y: m.y + sy / n,
        }
    }
}

/// **Where the orb is at `age`**: on the line from the tail to its aim, as
/// far as its flight has taken it.
pub fn centre(vfx: &Vfx, age: f32) -> Pt {
    let f = &vfx.ignition;
    let a = aim(vfx);
    let s = along(age, f.len, f.unit);
    Pt {
        x: f.tail.x + (a.x - f.tail.x) * s,
        y: f.tail.y + (a.y - f.tail.y) * s,
    }
}

/// **The line the orb can fly on**, tail to its aim, which the bands cover
/// to [`REACH`].
pub fn line(vfx: &Vfx) -> Corners {
    let t = vfx.ignition.tail;
    let a = aim(vfx);
    Corners([t, a, a, t])
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
            born: flight(f.len, f.unit),
            life: 0.18 + 0.12 * h1.unit(),
            h: [h0, h1, h2, h3],
        }
    }
}

/// **The sparks alive at the effect's age**: twelve, burst in every
/// direction from the orb as it lands, at the end of its flight, slowing and
/// fading from full. Their origin is the orb's centre at that age under
/// today's emitter, the comet's convention: the burst follows the orb's aim,
/// so a retarget after the landing carries it, still spreading, toward the
/// new cursor.
pub fn sparks(vfx: &Vfx) -> impl Iterator<Item = Particle> + '_ {
    let f = &vfx.ignition;
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
        let head = centre(vfx, s.born);
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
        seeded(from, to, age, Seed(4))
    }

    fn seeded(from: Rect, to: Rect, age: f32, seed: Seed) -> Vfx {
        let f = ignition(from, to, seed);
        Vfx {
            effect: Effect::Orb,
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

    /// **The orb leaves the tail from rest and lands on the cursor when its
    /// flight ends**: a fiftieth of its flight in it has gone under half a
    /// percent of the way, where a steady speed would have gone two; it never
    /// turns back; it is all the way there exactly at the end of its flight;
    /// and there, on every move in [`MOVES`], its centre is within a pixel of
    /// the cursor's. At the start it is on the tail.
    #[test]
    fn the_orb_leaves_from_rest_and_lands_on_the_cursor() {
        for ((fc, fr), (tc, tr)) in MOVES {
            let (from, to) = (cell(fc, fr), cell(tc, tr));
            let v0 = vfx(from, to, 0.0);
            let o = centre(&v0, 0.0);
            assert!(far(o, v0.ignition.tail) < 1e-3, "{o:?} at the start");
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
            assert_eq!(along(t, len, c), 1.0, "all the way when its flight ends");
            let off = far(centre(&vfx(from, to, t), t), to.center());
            assert!(off < 1.0, "{off} px from the cursor when its flight ends");
        }
    }

    /// The moves the landing is asked on: across, down, back, the clips'
    /// diagonal and three short diagonals, whose sideways corner runs on one
    /// of kitty's middling constants.
    const MOVES: [((f32, f32), (f32, f32)); 7] = [
        ((10.0, 5.0), (40.0, 5.0)),
        ((20.0, 2.0), (20.0, 14.0)),
        ((40.0, 5.0), (10.0, 5.0)),
        ((8.0, 6.0), (72.0, 20.0)),
        ((10.0, 5.0), (14.0, 7.0)),
        ((10.0, 5.0), (13.0, 6.0)),
        ((10.0, 5.0), (12.0, 8.0)),
    ];

    fn far(a: Pt, b: Pt) -> f32 {
        (a.x - b.x).hypot(a.y - b.y)
    }

    /// **The sparks burst from the orb as it lands, not ahead of it**: at
    /// every spark's birth the orb is within a pixel of the cursor's centre,
    /// and a spark starts on its rim -- across, down, back and on the
    /// diagonals. A spring from the old model was 61 px short when the first
    /// spark was born on a 30-column move.
    #[test]
    fn the_sparks_burst_from_the_orb_as_it_lands() {
        for ((fc, fr), (tc, tr)) in MOVES {
            let (from, to) = (cell(fc, fr), cell(tc, tr));
            let f = ignition(from, to, Seed(4));
            let r = RADIUS * f.unit;
            for b in burst(&f).map(|h| Spark::draw(&f, h).born) {
                let o = centre(&vfx(from, to, b), b);
                let off = far(o, to.center());
                assert!(
                    off < 1.0,
                    "the orb {off} px from the cursor as a spark is born at {b}"
                );
                let rim =
                    sparks(&vfx(from, to, b + 1e-4)).any(|p| (far(p.centre, o) - r).abs() < 0.5);
                assert!(rim, "no spark on the orb's rim at {b}");
            }
        }
    }

    /// The orb `age` into a move from `from` to `to` that the cursor left
    /// for `then` at `at`.
    fn retarget(from: Rect, to: Rect, then: Rect, at: f32, age: f32) -> Vfx {
        let mut v = vfx(from, to, age);
        let seg = Segment::new(from.corners(), to);
        v.emitter.corners = Segment::new(seg.at(at), then).at(age - at);
        v.emitter.hole = then;
        v
    }

    /// **A retarget moves the orb without a jump, and its sparks burst from
    /// where it is**: the cursor jumps a third of the way through a flight;
    /// the orb as the cursor jumps is where it was before, and every spark
    /// starts on the orb's rim, well away from the cursor the first move went
    /// to. Landing on the new cursor is not asked: the corners that faced the
    /// first move may be the new one's slow ones.
    #[test]
    fn a_retarget_moves_the_orb_and_its_burst_with_it() {
        let (from, to, then) = (cell(10.0, 5.0), cell(40.0, 5.0), cell(30.0, 12.0));
        let f = ignition(from, to, Seed(4));
        let at = flight(f.len, f.unit) / 3.0;
        let before = centre(&vfx(from, to, at), at);
        let after = centre(&retarget(from, to, then, at, at), at);
        assert!(far(before, after) < 1e-2, "{before:?} then {after:?}");
        let r = RADIUS * f.unit;
        for b in burst(&f).map(|h| Spark::draw(&f, h).born) {
            let o = centre(&retarget(from, to, then, at, b), b);
            let gone = far(o, to.center());
            assert!(
                gone > 5.0 * CELL.0,
                "the orb {gone} px from the first cursor"
            );
            let rim = sparks(&retarget(from, to, then, at, b + 1e-4))
                .any(|p| (far(p.centre, o) - r).abs() < 0.5);
            assert!(rim, "no spark on the orb's rim at {b}");
        }
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
    /// on the side it came from, where the sphere it has landed in shows past
    /// the cursor's edge, it is bright when the fade starts and all but gone
    /// just before its end.
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
        assert_eq!(sparks(&vfx(from, to, out + 0.001)).count(), 0);
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

    /// **Twelve sparks burst from the orb as it lands, spread all round and
    /// fading**: none before its flight ends, all twelve just after, on both
    /// sides of it.
    #[test]
    fn sparks_burst_all_round_on_arrival() {
        let (from, to) = (cell(10.0, 5.0), cell(40.0, 5.0));
        let at = |a: f32| seeded(from, to, a, Seed(2));
        let t = flight(at(0.0).ignition.len, CELL.1);
        assert_eq!(sparks(&at(0.99 * t)).count(), 0, "none before it lands");
        let v = at(t + 0.01);
        let live: Vec<Particle> = sparks(&v).collect();
        assert_eq!(live.len(), 12);
        let o = centre(&v, t);
        let (mut left, mut right) = (0, 0);
        for s in &live {
            if s.centre.x < o.x {
                left += 1;
            } else {
                right += 1;
            }
        }
        assert!(left > 1 && right > 1, "all round: {left} and {right}");
        let glow = |a: f32| sparks(&at(a)).map(|s| s.glow).sum::<f32>();
        assert!(glow(t + 0.1) < glow(t + 0.01), "fading");
    }
}
