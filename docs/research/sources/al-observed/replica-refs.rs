//! The pixel oracle's references (`baud-present-macos/tests/cells.rs`),
//! copied, with one knob a term so a ceiling arm can force that term to
//! zero. With [`Arm::All`] each is the oracle's reference unchanged; the
//! orb's is `baud_render::orb::light` spelled out term by term, and `main`
//! never relies on that without the "all" arm reproducing the census.

use baud_render::effect::{Effect, Vfx};
use baud_render::{Argb, Corners, Pt, fire, ice, lightning, liquid, orb, plasma, ufo};

/// Which term a ceiling arm forces to zero. `All` forces nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Arm {
    All,
    /// No particle records drawn (embers, ions, sparks, shards, puffs).
    NoParticles,
    /// The fire: the halo term `0.12 e^(-2.5 d0) burn` zero.
    NoHalo,
    /// The fire: the lifted (tongue) body term zero outside the quad
    /// (`d0 > 0`); inside the quad the body is unchanged.
    NoTongues,
    /// The fire: no halo, no tongues, no embers -- the quad body alone.
    QuadOnly,
    /// The fire: nothing at all inside the quad (`d0 <= 0` drawn as zero):
    /// what is left is tongues, halo and embers outside it.
    OutsideOnly,
    /// The orb: the sphere's body term zero.
    NoBody,
    /// The orb: the comet term zero.
    NoComet,
    /// The orb / plasma: the halo (orb) or sheath outside the quad (plasma)
    /// zero.
    NoGlow,
    /// The orb: body only (no halo, no comet, no sparks).
    BodyOnly,
    /// The plasma: filaments zero.
    NoFilaments,
    /// The plasma: tube and sheath zero, filaments only.
    FilamentsOnly,
    /// Every band and particle at `x/100` of its opacity: a global opacity
    /// lever, priced.
    Opacity(u32),
}

impl Arm {
    pub fn parse(s: &str) -> Arm {
        match s {
            "all" => Arm::All,
            "noparticles" => Arm::NoParticles,
            "nohalo" => Arm::NoHalo,
            "notongues" => Arm::NoTongues,
            "quadonly" => Arm::QuadOnly,
            "outsideonly" => Arm::OutsideOnly,
            "nobody" => Arm::NoBody,
            "nocomet" => Arm::NoComet,
            "noglow" => Arm::NoGlow,
            "bodyonly" => Arm::BodyOnly,
            "nofilaments" => Arm::NoFilaments,
            "filamentsonly" => Arm::FilamentsOnly,
            o if o.starts_with("opacity") => Arm::Opacity(o[7..].parse().expect("opacityNN")),
            o => panic!("arm {o}"),
        }
    }

    pub fn name(self) -> String {
        match self {
            Arm::Opacity(x) => format!("opacity{x}"),
            a => format!("{a:?}").to_lowercase(),
        }
    }

    pub fn particles(self) -> bool {
        !matches!(self, Arm::NoParticles | Arm::QuadOnly | Arm::BodyOnly)
    }

    pub fn scale(self) -> f32 {
        match self {
            Arm::Opacity(x) => x as f32 / 100.0,
            _ => 1.0,
        }
    }
}

/// A hash word: the PCG output permutation, as `cells.rs` spells it.
#[derive(Clone, Copy)]
struct Word(u32);

impl Word {
    fn hash(self) -> Word {
        let s = self.0.wrapping_mul(747_796_405).wrapping_add(2_891_336_453);
        let w = ((s >> ((s >> 28) + 4)) ^ s).wrapping_mul(277_803_737);
        Word((w >> 22) ^ w)
    }
    fn unit(self) -> f32 {
        f32::from_bits((self.0 >> 9) | 0x3f80_0000) - 1.0
    }
    fn plus(self, o: Word) -> Word {
        Word(self.0.wrapping_add(o.0))
    }
}

fn fx_lattice(x: f32, y: f32, seed: Word) -> f32 {
    let (ix, iy) = (Word((x as i32) as u32), Word((y as i32) as u32));
    ix.plus(iy.plus(seed).hash()).hash().unit()
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn fx_noise(p: Pt, seed: Word) -> f32 {
    let (fx, fy) = (p.x.floor(), p.y.floor());
    let (x, y) = (p.x - fx, p.y - fy);
    let fade = |v: f32| v * v * v * (v * (v * 6.0 - 15.0) + 10.0);
    let (wx, wy) = (fade(x), fade(y));
    let a = fx_lattice(fx, fy, seed);
    let b = fx_lattice(fx + 1.0, fy, seed);
    let c = fx_lattice(fx, fy + 1.0, seed);
    let d = fx_lattice(fx + 1.0, fy + 1.0, seed);
    mix(mix(a, b, wx), mix(c, d, wx), wy)
}

fn fx_fbm(mut p: Pt, seed: Word) -> f32 {
    let (mut t, mut a) = (0.0f32, 0.5f32);
    for o in 0..4u32 {
        t += a * fx_noise(p, seed.plus(Word(o.wrapping_mul(0x9E37_79B9))));
        p = Pt {
            x: (0.8 * p.x - 0.6 * p.y) * 2.02,
            y: (0.6 * p.x + 0.8 * p.y) * 2.02,
        };
        a *= 0.5;
    }
    t
}

fn fx_distance(p: Pt, c: &Corners) -> f32 {
    baud_render::effect::distance(p, c)
}

/// `fire_reference`, with the arm's knobs.
fn fire(vfx: &Vfx, p: Pt, arm: Arm) -> f32 {
    let (e, f) = (vfx.emitter, vfx.ignition);
    if e.hole.contains(p) {
        return 0.0;
    }
    let k = f.age / fire::LIFE;
    if k >= 1.0 {
        return 0.0;
    }
    let c = f.unit;
    let d0 = fx_distance(p, &e.corners) / c;
    if d0 > fire::REACH {
        return 0.0;
    }
    if arm == Arm::OutsideOnly && d0 <= 0.0 {
        return 0.0;
    }
    let rode = vfx.params()[3];
    let s = Pt {
        x: p.x / c * 2.6 - f.dir.x * (f.age * fire::DRIFT),
        y: ((p.y - rode) / c + f.age * fire::RISE) * 0.7 - f.dir.y * (f.age * fire::DRIFT),
    };
    let n = fx_fbm(s, Word(f.seed.0));
    let lift = fire::HEIGHT * c * smoothstep(0.25, 0.85, n);
    let d = fx_distance(
        Pt {
            x: p.x,
            y: p.y + lift,
        },
        &e.corners,
    ) / c;
    let mut tongue = 1.0 - smoothstep(-0.10, 0.20, d);
    if matches!(arm, Arm::NoTongues | Arm::QuadOnly) && d0 > 0.0 {
        tongue = 0.0;
    }
    let body = tongue.max(0.8 * (1.0 - smoothstep(-0.25, 0.05, d0)));
    let cool = (d0.max(0.0) / fire::HEIGHT).clamp(0.0, 1.0);
    let u = (((p.x - f.tail.x) * f.dir.x + (p.y - f.tail.y) * f.dir.y) / f.len.max(1.0))
        .clamp(0.0, 1.0);
    let burn = smoothstep(k - 0.15, k + 0.35, u) * (1.0 - k * k);
    let heat = (body * (1.0 - 0.85 * cool) * (0.50 + 0.60 * n) * burn).clamp(0.0, 1.0);
    let mut halo = 0.12 * (-2.5 * d0.max(0.0)).exp() * burn;
    if matches!(arm, Arm::NoHalo | Arm::QuadOnly) {
        halo = 0.0;
    }
    (smoothstep(0.05, 0.95, heat) + halo).clamp(0.0, 1.0)
}

fn px_wave(x: f32) -> f32 {
    let f = (x + 0.5 - (x + 0.5).floor()).min(1.0 - f32::EPSILON / 2.0);
    (core::f32::consts::PI * (2.0 * f - 1.0)).sin()
}

const PX_K: [f32; 3] = [0.23, 0.31, 0.17];
const PX_F: [f32; 3] = [1.10, 1.35, 0.80];
const PX_K2: [f32; 3] = [0.71, 0.93, 0.53];
const PX_F2: [f32; 3] = [-0.85, -1.05, -1.20];
const PX_M: [f32; 3] = [0.22, 0.20, 0.25];
const PX_W: [f32; 3] = [0.040, 0.034, 0.050];
const PX_G: [f32; 3] = [1.00, 0.85, 0.65];

/// `plasma_parts` + `plasma_reference`, with the arm's knobs.
fn plasma(vfx: &Vfx, p: Pt, arm: Arm) -> f32 {
    let (e, f) = (vfx.emitter, vfx.ignition);
    if e.hole.contains(p) {
        return 0.0;
    }
    let k = f.age / plasma::LIFE;
    if k >= 1.0 {
        return 0.0;
    }
    let c = f.unit;
    let d0 = fx_distance(p, &e.corners) / c;
    if d0 > plasma::REACH {
        return 0.0;
    }
    let q = vfx.params();
    let env = (1.0 - k) * (1.0 - k);
    let inside = 1.0 - smoothstep(-0.03, 0.03, d0);
    let mut tube = q[3] * inside + q[4] * (-q[5] * (1.0 + 1.5 * k) * d0.max(0.0)).exp();
    if arm == Arm::NoGlow && d0 > 0.0 {
        tube = q[3] * inside;
    }
    if arm == Arm::FilamentsOnly {
        tube = 0.0;
    }
    let (ax, ay) = (q[1], q[2]);
    let (rx, ry) = (
        p.x - 0.5 * (e.hole.left + e.hole.right),
        p.y - 0.5 * (e.hole.top + e.hole.bottom),
    );
    let s = -(rx * ax + ry * ay) / c;
    let mut fil = 0.0f32;
    if d0 < 0.15 && arm != Arm::NoFilaments {
        let w = (ax * ry - ay * rx) / c;
        let amp = q[0] * 0.62 * smoothstep(0.0, 2.0, s);
        let salt = Word(f.seed.0 ^ 0xA511_E9B3);
        let kn = salt.plus(Word(8)).hash().unit();
        for i in 0..3usize {
            let n = i as u32;
            let ph = salt.plus(Word(n)).hash().unit();
            let ph2 = salt.plus(Word(16 + n)).hash().unit();
            let along = amp
                * ((1.0 - PX_M[i]) * px_wave(PX_K[i] * s + ph + PX_F[i] * f.age)
                    + PX_M[i] * px_wave(PX_K2[i] * s + ph2 + PX_F2[i] * f.age));
            let wid = (PX_W[i] * (1.0 - 0.5 * k)).max(1.1 / c);
            let x = (w - along) / wid;
            let knot = 0.72 + 0.28 * px_wave(0.45 * s + kn + n as f32 / 3.0 + q[7] * f.age);
            fil += PX_G[i] * (-x * x).exp() * knot;
        }
        fil = fil.clamp(0.0, 1.0)
            * q[6]
            * (1.0 - smoothstep(-0.02, 0.15, d0))
            * smoothstep(0.0, 0.3, s);
    }
    (env * (tube + (1.0 - tube) * fil)).clamp(0.0, 1.0)
}

fn length(x: f32, y: f32) -> f32 {
    (x * x + y * y).sqrt()
}

/// `orb::light`, term by term, with the arm's knobs.
fn orb_light(vfx: &Vfx, p: Pt, arm: Arm) -> f32 {
    let f = &vfx.ignition;
    if vfx.emitter.hole.contains(p) {
        return 0.0;
    }
    let c = f.unit;
    let t = orb::flight(f.len, c);
    let end = t + orb::LINGER + orb::FADE;
    if f.age >= end {
        return 0.0;
    }
    if fx_distance(p, &orb::line(vfx)) / c > orb::REACH {
        return 0.0;
    }
    let env = smoothstep(0.0, orb::APPEAR, f.age)
        * (1.0 - smoothstep(t + orb::LINGER, end, f.age));
    let o = orb::centre(vfx, f.age);
    let r = orb::RADIUS * c;
    let d = length(p.x - o.x, p.y - o.y);
    let cov = (0.5 - (d - r)).clamp(0.0, 1.0);
    let kk = (d / r).min(1.0);
    let (hx, hy) = (o.x - 0.35 * r, o.y - 0.35 * r);
    let hs = length(p.x - hx, p.y - hy) / (0.30 * r);
    let mut body = (0.45 + 0.30 * (1.0 - kk * kk) + 0.25 * (-hs * hs).exp()) * cov;
    let hd = (d - r).max(0.0) / (orb::HALO_W * c);
    let mut halo = orb::HALO * (-hd * hd).exp();
    let q = orb::centre(vfx, (f.age - orb::COMET_S).max(0.0));
    let (bx, by) = (q.x - o.x, q.y - o.y);
    let (px, py) = (p.x - o.x, p.y - o.y);
    let h = ((px * bx + py * by) / (bx * bx + by * by).max(1e-6)).clamp(0.0, 1.0);
    let ds = length(px - bx * h, py - by * h);
    let cw = r * (0.8 - 0.6 * h);
    let cd = ds / cw;
    let mut comet = orb::COMET * (1.0 - h) * (1.0 - h) * (-cd * cd).exp();
    match arm {
        Arm::NoBody => body = 0.0,
        Arm::NoGlow => halo = 0.0,
        Arm::NoComet => comet = 0.0,
        Arm::BodyOnly => {
            halo = 0.0;
            comet = 0.0;
        }
        _ => {}
    }
    (body.max(halo).max(comet) * env).clamp(0.0, 1.0)
}

/// An effect's band rule at `p`, premultiplied, in `color`'s channels.
pub fn band(vfx: &Vfx, p: Pt, color: [f32; 4], arm: Arm) -> [f32; 4] {
    let l = match vfx.effect {
        Effect::Fire => fire(vfx, p, arm),
        Effect::Plasma => plasma(vfx, p, arm),
        Effect::Orb => orb_light(vfx, p, arm),
        Effect::Lightning => lightning::light(vfx, p),
        Effect::Ice => ice::light(vfx, p),
        Effect::Ufo => ufo::light(vfx, p),
        Effect::Liquid => liquid::light(vfx, p),
    };
    let a = color[3] * l * arm.scale();
    [color[0] * a, color[1] * a, color[2] * a, a]
}

/// An effect's particle rule at `uv` in its record.
pub fn particle(effect: Effect, uv: Pt, color: [f32; 4], arm: Arm) -> [f32; 4] {
    let l = match effect {
        Effect::Ice => ice::shard(uv),
        Effect::Ufo => ufo::ring(uv),
        _ => {
            let a = 1.0 - smoothstep(0.15, 1.0, (uv.x * uv.x + uv.y * uv.y).sqrt());
            a * a
        }
    };
    let a = color[3] * l * arm.scale();
    [color[0] * a, color[1] * a, color[2] * a, a]
}

/// A record's colour as the shader reads it.
pub fn unit_color(c: Argb) -> [f32; 4] {
    let f = |v: baud_domain::Chan| f32::from(v.get()) / 255.0;
    [f(c.r()), f(c.g()), f(c.b()), f(c.a())]
}

/// Premultiplied `src` over the stored pixel `dst`, rounded to bytes.
pub fn over(src: [f32; 4], dst: u32) -> u32 {
    let ch = |k: u32| ((dst >> k) & 0xff) as f32 / 255.0;
    let (r, g, b, a) = (ch(16), ch(8), ch(0), ch(24));
    let mix = |s: f32, d: f32| {
        let v = ((s + d * (1.0 - src[3])) * 255.0).round().clamp(0.0, 255.0);
        v as u32
    };
    (mix(src[3], a) << 24) | (mix(src[0], r) << 16) | (mix(src[1], g) << 8) | mix(src[2], b)
}

/// The arm's opacity factor, for the smears.
pub fn scale(arm: Arm) -> f32 {
    arm.scale()
}
