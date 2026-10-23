//! **The liquid trail**, after macOS's Liquid Glass: a clear glass gel in
//! the cursor's colour stretching from where the cursor was to where it is,
//! a bright rim and a moving glint on it. Its back is pulled forward along
//! kitty's quad, it thins in the middle and pinches, the neck snapping into
//! satellite beads that are sucked back into the head and merge with it
//! through a smooth union; a meniscus round the cursor wobbles as the back
//! arrives, and it fades. Designed in the effect-trail research (the liquid
//! spec's design (a)), in the cursor's colour with intensity as opacity.
//!
//! **It reads nothing underneath.** The spec's design (b) bends the text
//! under the gel -- a second render pass a frame over the whole target, and
//! still unable to bend the wallpaper and blur that the window server
//! composites behind baud's translucent window. That is the owner's call, and
//! this is (a).
//!
//! **Everything that moves is a function of the ignition's clock**, as the
//! spec requires for a retarget: the spine's back is taken from the order
//! statistics of kitty's corners along the ignition's direction, which are
//! continuous where the cursor jumps; the neck, the beads, the glint and the
//! envelope run on the ignition's age. Unlike the spec, the state is derived
//! in the shader from the uniform's prefix rather than handed in Rust-built,
//! so the family's uniform does not grow: `liquid::state` is that derivation,
//! spelled once in Rust, and the oracle holds the shader to it.
//!
//! **Its bands cover its spine and the cursor** ([`Vfx::support`]), not the
//! quad: early in a move kitty's leading corners have not reached the
//! cursor, so the meniscus is outside the quad.

use crate::det::Det;
use crate::effect::{self, Hash, Ignition, Vfx};
use crate::trail::{Corners, Pt, TAU_SLOW};

/// The tube's radius across the move, as a share of the cell; the smooth
/// union's width, the head's gap and its wobble, in cells.
pub const TUBE: f32 = 0.45;
pub const UNION: f32 = 0.22;
pub const GAP: f32 = 0.10;
pub const WOBBLE: f32 = 0.08;
/// The bevel's depth in cells, and the body, rim, glint and counter-glint's
/// opacities of the cursor's colour.
pub const BEZEL: f32 = 0.25;
pub const BODY: f32 = 0.12;
pub const RIM: f32 = 0.55;
pub const SPEC: f32 = 0.85;

/// The effect's own numbers, `k` in `EffectU`.
pub const PARAMS: [f32; effect::PARAMS] = [TUBE, UNION, GAP, WOBBLE, BEZEL, BODY, RIM, SPEC];

/// **The back of the gel follows kitty's trailing corners**, which halve
/// their gap every tenth of their constant: the shader's `LQ_TRAIL`.
pub const TRAIL: f32 = TAU_SLOW / 10.0;

/// Seconds the glint's lean into the move takes to relax toward the upper
/// left by all but 1/e, and how much of the move's direction it starts
/// with, added to the rest light's.
pub const LEAN: f32 = 0.08;
pub const LEAN_AT: f32 = 1.2;

/// Toward the light at rest, y down: the upper left, as Liquid Glass's.
pub const REST: (f32, f32) = (-0.5, -0.866);

/// The salts of the bead count's hash and of each bead's chain, and the
/// seconds a bead takes to grow in.
pub const COUNT_SALT: u32 = 0x5bd1_e995;
pub const BEAD_SALT: u32 = 0x9E37_79B9;
pub const BEAD_IN: f32 = 0.02;

/// How far the head's wobble pulls it in across the move for each pixel it
/// pushes it out along it: a squash with its stretch.
pub const CROSS: f32 = 0.6;

/// The counter-glint's opacity; the wobble's damping and its frequency, 6.5
/// cycles a second. The shader spells each as a `#define`, pinned to these.
pub const BACK: f32 = 0.25;
pub const ZETA: f32 = 0.28;
pub const OMEGA: f32 = 2.0 * core::f32::consts::PI * 6.5;

/// Cells past the spine or the cursor the gel reaches: the tube, the union's
/// bulge and a pixel, with the head's gap and wobble inside it.
pub const REACH: f32 = TUBE + UNION + 0.15;

/// The longest the gel lasts: a long move's contact, its wobble dying down,
/// and the fade.
pub const LIFE: f32 = 0.60;

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn length(x: f32, y: f32) -> f32 {
    (x * x + y * y).sqrt()
}

/// **The gel's state this frame**, everything the field and the shading
/// need that is uniform over the frame: `liquid_state` in the shader.
#[derive(Clone, Copy, Debug)]
pub struct State {
    /// The spine: its back and its head.
    pub b: Pt,
    pub h: Pt,
    pub radius: f32,
    pub neck: f32,
    pub inv_len2: f32,
    pub lip: f32,
    /// The head drop's half size and corner radius.
    pub half: Pt,
    pub round: f32,
    pub k: f32,
    /// The satellite beads: centre and radius, a radius of zero absent.
    pub bead: [(Pt, f32); 3],
    /// Toward the light, y down.
    pub light: Pt,
    pub envelope: f32,
    pub end: f32,
}

/// The age the back of kitty's quad comes within the head's reach, and the
/// wobble's amplitude, for a move `len` pixels long on cells `c` tall.
fn contact(len: f32, c: f32, radius: f32) -> (f32, f32) {
    let t = TRAIL * (len / (radius + GAP * c)).max(1.0).det_log2();
    (t, WOBBLE * c * smoothstep(3.0, 12.0, len / c))
}

/// **Seconds from ignition to when the gel is out**: the wobble after the
/// contact has died under half a pixel, and a little after; at least 0.22 s.
pub fn end(f: &Ignition, radius: f32) -> f32 {
    let (t, a) = contact(f.len, f.unit, radius);
    let tw = if a > 0.5 {
        (a / 0.5).det_ln() / (ZETA * OMEGA)
    } else {
        0.0
    };
    (t + tw + 0.05).max(0.22)
}

/// The tube's radius for a move along `d`, on a cursor `w` by `h`.
fn tube(d: Pt, w: f32, h: f32) -> f32 {
    TUBE * length(d.y * w, d.x * h)
}

/// **The gel's state at the ignition's age**, from the emitter's corners,
/// the cursor and the ignition alone.
pub fn state(vfx: &Vfx) -> State {
    let f = &vfx.ignition;
    let c = f.unit;
    let d = f.dir;
    let hole = vfx.emitter.hole;
    let (hw, hh) = (hole.right - hole.left, hole.bottom - hole.top);
    let h = Pt {
        x: (hole.left + hole.right) * 0.5,
        y: (hole.top + hole.bottom) * 0.5,
    };
    let q = vfx.emitter.corners.0;
    let m = Pt {
        x: (q[0].x + q[1].x + q[2].x + q[3].x) * 0.25,
        y: (q[0].y + q[1].y + q[2].y + q[3].y) * 0.25,
    };
    // The two smallest projections along the ignition's direction: the
    // quad's trailing edge, continuous where the cursor jumps.
    let pj = q.map(|p| (p.x - m.x) * d.x + (p.y - m.y) * d.y);
    let (mut lo, mut hi) = (f32::INFINITY, f32::INFINITY);
    for v in pj {
        if v < lo {
            hi = lo;
            lo = v;
        } else if v < hi {
            hi = v;
        }
    }
    let back = 0.5 * (lo + hi);
    let b = Pt {
        x: m.x + d.x * back,
        y: m.y + d.y * back,
    };
    let l = length(h.x - b.x, h.y - b.y);
    let inv_len2 = if l > 1e-3 { 1.0 / (l * l) } else { 0.0 };
    // The cell's extent, as the spec takes it: the cursor's, floored at the
    // nominal cell, so a bar or an underline carries a block's tube rather
    // than a hairline under a cell-sized drop.
    let r = tube(d, hw.max(0.5 * c), hh.max(c));
    let radius = r * smoothstep(0.0, 2.0 * r, l);
    let neck_max = 1.45 * smoothstep(1.5, 4.0, f.len / c);
    let neck = neck_max * smoothstep(0.03, 0.14, f.age);
    let lip = if l > 1e-3 {
        4.0 * neck * radius / l
    } else {
        0.0
    };
    // The contact and the end are the ignition's alone, on a nominal cell
    // half as wide as tall, so the driver -- which has no cursor -- and the
    // frame agree on when the gel is out.
    let nominal = tube(d, 0.5 * c, c);
    let (tc, amp) = contact(f.len, c, nominal);
    let sw = f.age - tc;
    let wob = if sw > 0.0 {
        let wd = OMEGA * (1.0 - ZETA * ZETA).sqrt();
        amp * (-ZETA * OMEGA * sw).det_exp() * (wd * sw).det_sin()
    } else {
        0.0
    };
    let (ax, ay) = (d.x.abs(), d.y.abs());
    let round = GAP * c + WOBBLE * c + 1.0;
    let half = Pt {
        x: (0.5 * hw + GAP * c + wob * ax - CROSS * wob * ay).max(round),
        y: (0.5 * hh + GAP * c + wob * ay - CROSS * wob * ax).max(round),
    };
    let mut bead = [(Pt { x: 0.0, y: 0.0 }, 0.0f32); 3];
    if neck_max > 1.0 {
        let xp = 0.5 - ((1.0 - 2.0 / neck_max).det_asin() / 3.0).det_sin();
        let tp = 0.03 + 0.11 * xp;
        let sg = f.age - tp;
        let count = if Hash(f.seed.0 ^ COUNT_SALT).next().0 & 1 == 0 {
            2
        } else {
            3
        };
        let (nx, ny) = (-d.y, d.x);
        if sg >= 0.0 {
            let mut jf = 0.0f32;
            let mut salt = BEAD_SALT;
            for slot in bead.iter_mut().take(count) {
                let h0 = Hash(f.seed.0 ^ salt).next();
                let h1 = h0.next();
                let h2 = h1.next();
                let h3 = h2.next();
                let u0 = 0.5 + (h0.unit() - 0.5) * 0.16;
                let tau = 0.045 + 0.04 * h2.unit();
                let u = u0 + (1.0 - u0) * (1.0 - (-sg / tau).det_exp());
                let rj = r
                    * (0.30 - 0.07 * jf)
                    * (0.85 + 0.3 * h1.unit())
                    * (1.0 - smoothstep(0.85, 0.99, u))
                    * smoothstep(0.0, BEAD_IN, sg);
                let o = (h3.unit() - 0.5) * 0.25 * r * (-sg / 0.06).det_exp();
                *slot = (
                    Pt {
                        x: b.x + (h.x - b.x) * u + nx * o,
                        y: b.y + (h.y - b.y) * u + ny * o,
                    },
                    rj,
                );
                jf += 1.0;
                salt = salt.wrapping_add(BEAD_SALT);
            }
        }
    }
    // The light starts leaning into the move -- the rest light plus LEAN_AT
    // of the move's direction, the spec's -- and turns back to the upper
    // left the short way round, by its angle. Blending the two vectors over
    // time instead passes through nothing for a move opposite the rest light
    // and turned the glint across the drop in a frame; the start itself is
    // never nothing, LEAN_AT being more than one.
    let rest = REST.1.det_atan2(REST.0);
    let lean = (REST.1 + LEAN_AT * d.y).det_atan2(REST.0 + LEAN_AT * d.x);
    let mut turn = lean - rest;
    if turn > core::f32::consts::PI {
        turn -= core::f32::consts::TAU;
    } else if turn <= -core::f32::consts::PI {
        turn += core::f32::consts::TAU;
    }
    let (ly, lx) = (rest + turn * (-f.age / LEAN).det_exp()).det_sin_cos();
    let end = end(f, nominal);
    State {
        b,
        h,
        radius,
        neck,
        inv_len2,
        lip,
        half,
        round,
        k: UNION * c,
        bead,
        light: Pt { x: lx, y: ly },
        envelope: smoothstep(0.0, 0.035, f.age) * (1.0 - smoothstep(end - 0.10, end, f.age)),
        end,
    }
}

/// A distance and its gradient.
#[derive(Clone, Copy)]
struct Field {
    d: f32,
    gx: f32,
    gy: f32,
}

/// iq's quadratic smooth minimum with its gradient.
fn smin(a: Field, b: Field, k: f32) -> Field {
    let h = 1.0 - ((a.d - b.d).abs() / (4.0 * k)).min(1.0);
    let s = h * h * k;
    let w = 0.5 * h;
    let mix = |p: f32, q: f32| p + (q - p) * w;
    if a.d < b.d {
        Field {
            d: a.d - s,
            gx: mix(a.gx, b.gx),
            gy: mix(a.gy, b.gy),
        }
    } else {
        Field {
            d: b.d - s,
            gx: mix(b.gx, a.gx),
            gy: mix(b.gy, a.gy),
        }
    }
}

fn dir(x: f32, y: f32, l: f32) -> (f32, f32) {
    if l > 1e-3 { (x / l, y / l) } else { (0.0, 0.0) }
}

/// **The gel's field at `p`**: the head drop, the necked tube and the beads
/// in one smooth union, with its gradient.
fn field(p: Pt, s: &State) -> Field {
    let (phx, phy) = (p.x - s.h.x, p.y - s.h.y);
    let sgx = if phx >= 0.0 { 1.0 } else { -1.0 };
    let sgy = if phy >= 0.0 { 1.0 } else { -1.0 };
    let wx = phx.abs() - (s.half.x - s.round);
    let wy = phy.abs() - (s.half.y - s.round);
    let g = wx.max(wy);
    let mut f = if g > 0.0 {
        let (px, py) = (wx.max(0.0), wy.max(0.0));
        let lw = length(px, py);
        Field {
            d: lw - s.round,
            gx: sgx * px / lw,
            gy: sgy * py / lw,
        }
    } else if wx > wy {
        Field {
            d: g - s.round,
            gx: sgx,
            gy: 0.0,
        }
    } else {
        Field {
            d: g - s.round,
            gx: 0.0,
            gy: sgy,
        }
    };
    if s.inv_len2 > 0.0 && s.radius > 0.0 {
        let (ex, ey) = (s.h.x - s.b.x, s.h.y - s.b.y);
        let u = (((p.x - s.b.x) * ex + (p.y - s.b.y) * ey) * s.inv_len2).clamp(0.0, 1.0);
        let (vx, vy) = (p.x - (s.b.x + ex * u), p.y - (s.b.y + ey * u));
        let l = length(vx, vy);
        let q = 1.0 - 4.0 * s.neck * u * (1.0 - u);
        let sl = s.lip * (2.0 * u - 1.0);
        let (gx, gy) = dir(vx, vy, l);
        let t = Field {
            d: (l - s.radius * q) / (1.0 + sl * sl).sqrt(),
            gx,
            gy,
        };
        f = smin(f, t, s.k);
    }
    for (c, r) in s.bead {
        if r > 0.0 {
            let (vx, vy) = (p.x - c.x, p.y - c.y);
            let l = length(vx, vy);
            let (gx, gy) = dir(vx, vy, l);
            f = smin(f, Field { d: l - r, gx, gy }, s.k);
        }
    }
    f
}

/// **The gel's opacity at `p`**, 0 to 1, before the record's alpha: the
/// body, the rim, the glint and the counter-glint on a squircle bevel --
/// `liquid_light` in the shader.
pub fn light(vfx: &Vfx, p: Pt) -> f32 {
    let hole = vfx.emitter.hole;
    if hole.contains(p) {
        return 0.0;
    }
    let s = state(vfx);
    let c = vfx.ignition.unit;
    if vfx.ignition.age >= s.end {
        return 0.0;
    }
    let (ex, ey) = (s.h.x - s.b.x, s.h.y - s.b.y);
    let u0 = (((p.x - s.b.x) * ex + (p.y - s.b.y) * ey) * s.inv_len2).clamp(0.0, 1.0);
    let ds = if s.inv_len2 > 0.0 {
        length(p.x - (s.b.x + ex * u0), p.y - (s.b.y + ey * u0))
    } else {
        f32::INFINITY
    };
    let ox = (hole.left - p.x).max(p.x - hole.right).max(0.0);
    let oy = (hole.top - p.y).max(p.y - hole.bottom).max(0.0);
    if ds.min(length(ox, oy)) > REACH * c {
        return 0.0;
    }
    let f = field(p, &s);
    let cov = (0.5 - f.d).clamp(0.0, 1.0);
    if cov <= 0.0 {
        return 0.0;
    }
    let t = (-f.d / (BEZEL * c)).clamp(0.0, 1.0);
    let sv = 1.0 - t;
    let s2 = sv * sv;
    let s3 = s2 * sv;
    let z = (1.0 - s2 * s2).max(0.0).sqrt().sqrt();
    let z3 = z * z * z;
    let nz = z3 / (s3 * s3 + z3 * z3).max(1e-12).sqrt();
    let fr = 1.0 - nz;
    let rim = 0.04 + 0.96 * fr * fr;
    let gl = f.gx * f.gx + f.gy * f.gy;
    let (nx, ny) = if gl > 1e-12 {
        let r = gl.sqrt();
        (f.gx / r, f.gy / r)
    } else {
        (0.0, 0.0)
    };
    let bt = ((t - 0.04) * 2.5).clamp(0.0, 1.0);
    let band = 4.0 * bt * (1.0 - bt);
    let nl = nx * s.light.x + ny * s.light.y;
    let sp = nl.max(0.0);
    let sp = sp * sp * sp * band;
    let bk = (-nl).max(0.0);
    let bk = bk * bk * bk * band;
    s.envelope * cov * (BODY + RIM * rim + SPEC * sp + BACK * bk).clamp(0.0, 1.0)
}

/// **The shapes the gel's bands cover**: its spine and the cursor. The
/// spine's end at the cursor's centre does not hold the head drop on its
/// own: round a wide character's two cells, the drop and the tube's end cap
/// merge in the smooth union past the spine's reach.
pub fn support(vfx: &Vfx, mut each: impl FnMut(Corners)) {
    let s = state(vfx);
    each(Corners([s.b, s.h, s.h, s.b]));
    each(vfx.emitter.hole.corners());
}

/// The gel's end, from the ignition alone, for the driver: the tube's
/// radius taken on a block cursor a cell tall and half a cell wide.
pub fn end_of(f: &Ignition) -> f32 {
    end(f, tube(f.dir, 0.5 * f.unit, f.unit))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::tests::{CELL, cell, ignition};
    use crate::effect::{Effect, Seed};
    use crate::trail::{Rect, Segment, Shape, Smear};

    fn vfx(from: Rect, to: Rect, age: f32) -> Vfx {
        vfx_seeded(from, to, age, Seed(5))
    }

    fn vfx_seeded(from: Rect, to: Rect, age: f32, seed: Seed) -> Vfx {
        let f = ignition(from, to, seed);
        Vfx {
            effect: Effect::Liquid,
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

    fn mid(r: Rect) -> Pt {
        Pt {
            x: (r.left + r.right) * 0.5,
            y: (r.top + r.bottom) * 0.5,
        }
    }

    /// **The spine runs from the quad's back to the cursor, and its back is
    /// continuous across a retarget**: at ignition the back is the trailing
    /// edge of the cell the cursor left and the head is the cursor; when the
    /// cursor jumps again, the head jumps with it and the back does not move,
    /// because it is taken from kitty's corners, which a retarget starts from
    /// where they are. A millisecond after it, the new move's leading corners
    /// have already travelled several pixels, so the check is at the instant.
    #[test]
    fn the_spine_runs_from_the_quads_back_to_the_cursor() {
        let (from, to) = (cell(10.0, 5.0), cell(40.0, 5.0));
        let s = state(&vfx(from, to, 0.0));
        let (f0, t0) = (mid(from), mid(to));
        assert!((s.b.x - from.left).abs() < 1.0 && (s.b.y - f0.y).abs() < 1.0);
        assert!((s.h.x - t0.x).abs() < 1e-3 && (s.h.y - t0.y).abs() < 1e-3);
        let then = cell(30.0, 12.0);
        let seg = Segment::new(from.corners(), to);
        let at = |age: f32, hole: Rect, retarget: bool| {
            let mut v = vfx(from, to, age);
            if retarget {
                v.emitter.corners = Segment::new(seg.at(0.05), then).at(age - 0.05);
            }
            v.emitter.hole = hole;
            state(&v)
        };
        let before = at(0.05, to, false);
        let after = at(0.05, then, true);
        assert!(
            length(after.h.x - before.h.x, after.h.y - before.h.y) > 100.0,
            "the head did not jump"
        );
        assert!(
            length(after.b.x - before.b.x, after.b.y - before.b.y) < 0.01,
            "the back jumped from {:?} to {:?}",
            before.b,
            after.b
        );
        assert!((after.h.y - mid(then).y).abs() < 1e-3, "the head follows");
    }

    /// **A short hop stretches and snaps back with no pinch; a long one
    /// pinches into two or three beads**: a three-cell hop has no neck and no
    /// bead at any age; a thirty-cell hop has beads after its pinch, each
    /// moving toward the head, and both counts occur over sixteen seeds.
    #[test]
    fn a_long_move_pinches_into_beads_a_short_one_does_not() {
        let (from, short) = (cell(10.0, 5.0), cell(13.0, 5.0));
        for age in [0.02f32, 0.08, 0.15, 0.2] {
            let s = state(&vfx(from, short, age));
            assert_eq!(s.neck, 0.0, "a short hop necked at {age}");
            assert!(s.bead.iter().all(|&(_, r)| r == 0.0));
        }
        let long = cell(40.0, 5.0);
        let mut seen = [false; 2];
        for seed in 0..16 {
            let mut last = [f32::NEG_INFINITY; 3];
            let mut beads = 0;
            for i in 0..30 {
                let age = 0.08 + i as f32 * 0.005;
                let s = state(&vfx_seeded(from, long, age, Seed(seed)));
                let live: Vec<_> = s.bead.iter().filter(|&&(_, r)| r > 0.0).collect();
                beads = beads.max(live.len());
                for (j, &(c, r)) in s.bead.iter().enumerate() {
                    if r > 0.0 {
                        assert!(c.x >= last[j] - 1e-3, "bead {j} moved back at {age}");
                        last[j] = c.x;
                    }
                }
            }
            assert!(beads == 2 || beads == 3, "{beads} beads for seed {seed}");
            seen[beads - 2] = true;
        }
        assert!(seen[0] && seen[1], "both two and three beads occur");
    }

    /// **The cursor stays bare and the meniscus rings it**: nothing anywhere
    /// in the hole, through the stretch, the pinch and the wobble, and the
    /// gel lit just outside it on every side while it lasts.
    #[test]
    fn the_cursor_is_bare_and_the_meniscus_rings_it() {
        let (from, to) = (cell(10.0, 5.0), cell(40.0, 5.0));
        for age in [0.02f32, 0.12, 0.25] {
            let v = vfx(from, to, age);
            for y in 0..34 {
                for x in 0..17 {
                    let p = Pt {
                        x: to.left + x as f32 + 0.5,
                        y: to.top + y as f32 + 0.5,
                    };
                    assert_eq!(light(&v, p), 0.0, "{},{} at {age}", p.x, p.y);
                }
            }
        }
        let v = vfx(from, to, 0.12);
        let m = mid(to);
        for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
            let p = Pt {
                x: m.x + dx * (0.5 * CELL.0 + 1.5),
                y: m.y + dy * (0.5 * CELL.1 + 1.5),
            };
            assert!(light(&v, p) > 0.05, "no meniscus at {dx},{dy}");
        }
    }

    /// **The head wobbles after a long move arrives, and not after a short
    /// one**: the meniscus's half-width along the move swings above and
    /// below its rest after the contact, and a four-cell hop's never moves.
    #[test]
    fn the_head_wobbles_after_a_long_move_arrives() {
        let (from, to) = (cell(10.0, 5.0), cell(40.0, 5.0));
        let rest = 0.5 * CELL.0 + GAP * CELL.1;
        let (mut above, mut below) = (false, false);
        for i in 0..80 {
            let age = 0.15 + i as f32 * 0.004;
            let s = state(&vfx(from, to, age));
            above |= s.half.x > rest + 0.5;
            below |= s.half.x < rest - 0.5;
        }
        assert!(above && below, "no wobble: above {above}, below {below}");
        let near = cell(14.0, 5.0);
        for i in 0..60 {
            let age = i as f32 * 0.004;
            let s = state(&vfx(from, near, age));
            assert!((s.half.x - rest).abs() < 1e-3, "a short hop wobbled");
        }
    }

    /// **The gel fades in and out and is out at its end**: nothing at
    /// ignition, lit a little later, fading over its last tenth of a second,
    /// nothing after its end anywhere; its end is at least 0.22 s and within
    /// its life on the longest move, and the effect's end is the gel's.
    #[test]
    fn the_gel_fades_and_goes_out() {
        let (from, to) = (cell(10.0, 5.0), cell(40.0, 5.0));
        let f = ignition(from, to, Seed(5));
        let out = end_of(&f);
        assert_eq!(Effect::Liquid.end(&f), out);
        assert!(
            (0.34..0.44).contains(&out),
            "a thirty-cell hop out at {out}"
        );
        assert_eq!(state(&vfx(from, to, 0.0)).envelope, 0.0);
        assert!(state(&vfx(from, to, 0.05)).envelope > 0.99);
        let fading = state(&vfx(from, to, out - 0.05)).envelope;
        assert!(fading > 0.2 && fading < 0.8, "{fading} mid-fade");
        let gone = vfx(from, to, out + 0.001);
        for y in 0..14 {
            for x in 0..60 {
                let p = Pt {
                    x: x as f32 * 17.0 + 3.0,
                    y: y as f32 * 17.0 + 3.0,
                };
                assert_eq!(light(&gone, p), 0.0);
            }
        }
        let short = end_of(&ignition(from, cell(13.0, 5.0), Seed(5)));
        assert!((short - 0.22).abs() < 1e-6, "a short hop's floor: {short}");
        let far = ignition(cell(0.0, 0.0), cell(199.0, 49.0), Seed(5));
        assert!(end_of(&far) <= LIFE, "{} past its life", end_of(&far));
    }

    /// **The reach holds the gel**: every pixel it lights is well inside
    /// [`REACH`] of the spine or the cursor, so the cut there removes
    /// nothing -- over a hop, a diagonal and a vertical, through the stretch,
    /// the pinch and the wobble.
    #[test]
    fn the_reach_holds_the_gel() {
        for (from, to) in [
            (cell(10.0, 5.0), cell(40.0, 5.0)),
            (cell(5.0, 1.0), cell(60.0, 12.0)),
            (cell(20.0, 1.0), cell(20.0, 13.0)),
        ] {
            for age in [0.03f32, 0.1, 0.18, 0.25] {
                let v = vfx(from, to, age);
                let s = state(&v);
                let hole = v.emitter.hole;
                let mut lit = 0;
                for y in 0..16 * 34 {
                    for x in 0..70 * 17 {
                        let p = Pt {
                            x: x as f32 + 0.5,
                            y: y as f32 + 0.5,
                        };
                        if light(&v, p) <= 0.0 {
                            continue;
                        }
                        lit += 1;
                        let spine = crate::effect::distance(p, &Corners([s.b, s.h, s.h, s.b]));
                        let ox = (hole.left - p.x).max(p.x - hole.right).max(0.0);
                        let oy = (hole.top - p.y).max(p.y - hole.bottom).max(0.0);
                        let d = spine.min(length(ox, oy));
                        assert!(
                            d < REACH * CELL.1 - 1.0,
                            "{},{} lit {d} px out at {age}",
                            p.x,
                            p.y
                        );
                    }
                }
                assert!(lit > 200, "only {lit} lit at {age}");
            }
        }
    }

    /// **Every pixel the gel lights is in a band**: the bands cover the
    /// spine and the cursor, which the quad does not hold early in a move --
    /// kitty's leading corners have not reached it -- nor after a retarget.
    /// The widest cursor, a wide character's two cells, is here, through the
    /// wobble's first swing after the contact at about 0.19 s, across and
    /// down: there the head drop and the tube's end cap merge past the
    /// spine's reach, and only the cursor's own band holds them.
    #[test]
    fn every_pixel_the_gel_lights_is_in_a_band() {
        let frame = (baud_font::Px::of(1400), baud_font::Px::of(500));
        let wide = |r: Rect| Rect {
            right: r.right + CELL.0,
            ..r
        };
        for (from, to, then) in [
            (cell(10.0, 5.0), cell(40.0, 5.0), None),
            (cell(5.0, 1.0), cell(70.0, 12.0), None),
            (cell(10.0, 5.0), cell(40.0, 5.0), Some(cell(30.0, 12.0))),
            (cell(10.0, 5.0), wide(cell(40.0, 5.0)), None),
            (cell(40.0, 0.0), wide(cell(40.0, 13.0)), None),
        ] {
            for age in [0.01f32, 0.06, 0.12, 0.2, 0.23, 0.27] {
                let mut v = vfx(from, to, age);
                if let Some(t) = then {
                    let seg = Segment::new(from.corners(), to);
                    v.emitter.corners = Segment::new(seg.at(0.005), t).at(age - 0.005);
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
                assert!(lit > 200, "only {lit} lit at {age}");
            }
        }
    }

    /// **The glint leans into the move and relaxes to the upper left**: the
    /// light comes from ahead of the move at ignition -- across and down --
    /// and from the upper left once the gel has settled, as Liquid Glass's
    /// does, turning back by an angle that decays over [`LEAN`] seconds:
    /// after one of them it is 1/e of its start off the rest, to a third of
    /// a degree.
    #[test]
    fn the_glint_leans_into_the_move_and_relaxes() {
        let (from, to) = (cell(10.0, 5.0), cell(40.0, 5.0));
        let s0 = state(&vfx(from, to, 0.0));
        assert!(s0.light.x > 0.3, "leaning into the move: {:?}", s0.light);
        let s1 = state(&vfx(from, to, 0.3));
        assert!(
            s1.light.x < -0.4 && s1.light.y < -0.8,
            "upper left: {:?}",
            s1.light
        );
        let down = state(&vfx(cell(20.0, 1.0), cell(20.0, 13.0), 0.0));
        assert!(down.light.y > 0.3, "leaning down: {:?}", down.light);
        let off_rest =
            |p: Pt| p.y.det_atan2(p.x).to_degrees() - REST.1.det_atan2(REST.0).to_degrees();
        let start = off_rest(s0.light);
        let turned = off_rest(state(&vfx(from, to, LEAN)).light);
        let want = start / core::f32::consts::E;
        assert!(
            (turned - want).abs() < 0.3,
            "{turned} degrees off the rest after LEAN, against {want}"
        );
    }

    /// **The back is the quad's trailing edge along the move**: on a
    /// diagonal, where kitty's two rearmost corners are not level along the
    /// move, the back sits on the line through the quad's centre along the
    /// move, at the mean of those two corners' projections -- not at either
    /// corner's.
    #[test]
    fn the_back_is_the_trailing_edge_along_the_move() {
        let (from, to) = (cell(10.0, 2.0), cell(40.0, 12.0));
        let mut apart = 0.0f32;
        for i in 1..20 {
            let v = vfx(from, to, i as f32 * 0.01);
            let s = state(&v);
            let (q, d) = (v.emitter.corners.0, v.ignition.dir);
            let m = Pt {
                x: q.iter().map(|p| p.x).sum::<f32>() * 0.25,
                y: q.iter().map(|p| p.y).sum::<f32>() * 0.25,
            };
            let mut p = q.map(|c| (c.x - m.x) * d.x + (c.y - m.y) * d.y);
            p.sort_by(f32::total_cmp);
            apart = apart.max(p[1] - p[0]);
            let (bx, by) = (s.b.x - m.x, s.b.y - m.y);
            let along = bx * d.x + by * d.y;
            let across = -bx * d.y + by * d.x;
            assert!((along - 0.5 * (p[0] + p[1])).abs() < 1e-3, "along {along}");
            assert!(across.abs() < 1e-3, "across {across}");
        }
        assert!(apart > 5.0, "the rearmost corners were level: {apart}");
    }

    /// **The wobble is a damped swing, squashed across as it stretches
    /// along**: after the contact the head's half-width along a horizontal
    /// move is its rest plus WOBBLE of a cell times `e^-(ZETA OMEGA s)
    /// sin(OMEGA' s)`, and its half-height its rest less CROSS of that.
    #[test]
    fn the_wobble_is_a_damped_swing_squashed_across() {
        let (from, to) = (cell(10.0, 5.0), cell(40.0, 5.0));
        let f = ignition(from, to, Seed(5));
        let (tc, amp) = contact(f.len, CELL.1, tube(f.dir, 0.5 * CELL.1, CELL.1));
        assert!(
            (amp - WOBBLE * CELL.1).abs() < 1e-4,
            "a thirty-cell swing: {amp}"
        );
        let rest = (0.5 * CELL.0 + GAP * CELL.1, 0.5 * CELL.1 + GAP * CELL.1);
        let wd = OMEGA * (1.0 - ZETA * ZETA).sqrt();
        for s in [0.01f32, 0.025, 0.04, 0.06, 0.09, 0.12] {
            let st = state(&vfx(from, to, tc + s));
            let want = amp * (-ZETA * OMEGA * s).det_exp() * (wd * s).det_sin();
            assert!((st.half.x - rest.0 - want).abs() < 0.01, "along at {s}");
            assert!(
                (st.half.y - rest.1 + CROSS * want).abs() < 0.01,
                "across at {s}"
            );
        }
    }

    /// **The gel fades in over 35 ms**: half in at 17.5 ms.
    #[test]
    fn the_gel_fades_in_over_35_ms() {
        let e = state(&vfx(cell(10.0, 5.0), cell(40.0, 5.0), 0.0175)).envelope;
        assert!((e - 0.5).abs() < 0.01, "{e} at 17.5 ms");
    }

    /// **The rim is brighter than the body, and the glint's side than the
    /// counter-glint's**: across the tube at mid-spine early in a long move,
    /// the gel just inside its edge is lit well over its middle, and the
    /// edge the light falls on -- the upper one, the light leaning up and
    /// forward -- over the lower one.
    #[test]
    fn the_rim_and_the_glint_are_where_the_light_says() {
        let (from, to) = (cell(10.0, 5.0), cell(40.0, 5.0));
        let v = vfx(from, to, 0.03);
        let s = state(&v);
        let x = 0.5 * (s.b.x + s.h.x);
        let y0 = s.h.y;
        let at = |dy: f32| light(&v, Pt { x, y: y0 + dy });
        let edge = (0..40)
            .map(|i| i as f32)
            .take_while(|&dy| at(dy) > 0.0)
            .last()
            .unwrap();
        assert!(edge > 5.0, "the tube is {edge} px thick");
        let (middle, upper, lower) = (at(0.0), at(-(edge - 1.0)), at(edge - 1.0));
        assert!(
            upper.max(lower) > middle + 0.2,
            "rim {upper} {lower} over body {middle}"
        );
        let up = (1..6).map(|k| at(-(edge - k as f32))).fold(0.0, f32::max);
        let down = (1..6).map(|k| at(edge - k as f32)).fold(0.0, f32::max);
        assert!(up > down + 0.1, "glint {up} against counter-glint {down}");
        // The rim alone: within 0.04 of the bevel of the edge neither glint's
        // band reaches, so the gel there is the body and the rim. Found in
        // 0.02 px steps down to where the gel goes out, half a pixel past its
        // edge, then 0.7 px back in -- 0.2 px inside it.
        let out = (0..2000)
            .map(|i| i as f32 * 0.02)
            .find(|&dy| at(dy) == 0.0)
            .unwrap();
        let rimmed = at(out - 0.7);
        assert!(
            rimmed > 2.0 * middle,
            "the rim alone {rimmed} against the body {middle}"
        );
    }

    /// **The long move pinches, and its beads are drawn along the spine
    /// into the head**: its neck passes one -- the gap opens -- and every
    /// live bead's place along the spine rises frame to frame, within a
    /// pixel and its fading offset of the spine.
    #[test]
    fn the_beads_are_drawn_along_the_spine_into_the_head() {
        let (from, to) = (cell(10.0, 5.0), cell(40.0, 5.0));
        assert!(state(&vfx(from, to, 0.15)).neck > 1.0, "no pinch");
        for seed in 0..8 {
            let mut last = [f32::NEG_INFINITY; 3];
            for i in 0..20 {
                let s = state(&vfx_seeded(from, to, 0.1 + i as f32 * 0.005, Seed(seed)));
                let (ex, ey) = (s.h.x - s.b.x, s.h.y - s.b.y);
                let l2 = ex * ex + ey * ey;
                for (j, &(c, r)) in s.bead.iter().enumerate() {
                    if r <= 0.0 {
                        continue;
                    }
                    let u = ((c.x - s.b.x) * ex + (c.y - s.b.y) * ey) / l2;
                    assert!(u > last[j], "bead {j} at {u} after {}", last[j]);
                    last[j] = u;
                    let off = ((c.x - s.b.x) * ey - (c.y - s.b.y) * ex).abs() / l2.sqrt();
                    assert!(off < 0.13 * TUBE * CELL.1 + 0.5, "bead {j} {off} px off");
                }
            }
            assert!(
                last.iter().any(|&u| u > 0.8),
                "seed {seed}: no bead drawn in"
            );
        }
    }

    /// **The glint turns smoothly whatever the move**: over every direction
    /// a move can take, a degree apart, the light turns under 20 degrees
    /// between two frames at 120 Hz. A blend of the move's direction and the
    /// rest light passes through nothing for a move opposite the rest light,
    /// about 60 degrees below the horizontal toward the right, and turned
    /// the glint across the drop in one frame.
    #[test]
    fn the_glint_turns_smoothly_whatever_the_move() {
        let from = cell(60.0, 7.0);
        let c = from.center();
        for deg in 0..360 {
            let a = (deg as f32).to_radians();
            let to = cell(
                (c.x + 500.0 * a.det_cos()) / CELL.0 - 0.5,
                (c.y + 500.0 * a.det_sin()) / CELL.1 - 0.5,
            );
            let mut last: Option<Pt> = None;
            for k in 0..60 {
                let s = state(&vfx(from, to, k as f32 / 120.0));
                if let Some(l) = last {
                    let cos = (l.x * s.light.x + l.y * s.light.y).clamp(-1.0, 1.0);
                    let turn = cos.det_acos().to_degrees();
                    assert!(
                        turn <= 20.0,
                        "{turn} degrees in a frame, moving at {deg} degrees, frame {k}"
                    );
                }
                last = Some(s.light);
            }
        }
    }

    /// **The tube is the cell's whatever the cursor**: a bar or an underline
    /// cursor carries the tube a block does -- the cell's extent across the
    /// move, as the spec takes it -- rather than going sub-pixel across its
    /// thin side while the drop and the union stay cell-sized.
    #[test]
    fn the_tube_is_the_cells_whatever_the_cursor() {
        for (from, to) in [
            (cell(10.0, 5.0), cell(40.0, 5.0)),
            (cell(20.0, 1.0), cell(20.0, 13.0)),
        ] {
            let underline = Rect {
                top: to.bottom - 2.0,
                ..to
            };
            let bar = Rect {
                right: to.left + 2.0,
                ..to
            };
            for age in [0.03f32, 0.1] {
                let block = state(&vfx(from, to, age)).radius;
                for (name, hole) in [("underline", underline), ("bar", bar)] {
                    let mut v = vfx(from, to, age);
                    v.emitter.hole = hole;
                    let r = state(&v).radius;
                    assert!(
                        (r - block).abs() < 0.5,
                        "{name}: a tube of {r} px against the block's {block}"
                    );
                }
            }
        }
    }
}
