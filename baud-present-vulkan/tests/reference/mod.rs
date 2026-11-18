//! **Each effect by its reference**, the renderer's own Rust model of every
//! fragment shader (`fire::light`, `plasma::parts`, ...), composited in
//! floats and rounded once to the target's bytes, with the pixels whose last
//! composite lands on a rounding tie marked. The same reference, rule for
//! rule, as `baud-present-macos/tests/cells.rs` holds the Metal arms to, so
//! the Vulkan arms are held to the bar measured there the way it was
//! measured.

#![allow(dead_code)]

use baud_domain::Chan;
use baud_render::effect::{Effect, Vfx};
use baud_render::{Argb, Instance, Kind, Mix, Pt, fire, ice, lightning, liquid, orb, plasma, ufo};

/// **An effect by its reference**: the band's rule for the pixel centred at
/// `p` in frame pixels, and the particle's for a pixel at `uv` in its
/// record, each premultiplied, in `color`'s channels 0 to 1.
pub struct Reference {
    band: fn(&Vfx, Pt, [f32; 4]) -> [f32; 4],
    particle: fn(Pt, [f32; 4]) -> [f32; 4],
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn scaled(a: f32, color: [f32; 4]) -> [f32; 4] {
    [color[0] * a, color[1] * a, color[2] * a, a]
}

/// `fire_flame`, through `fire::light`.
fn fire_reference(vfx: &Vfx, p: Pt, color: [f32; 4]) -> [f32; 4] {
    scaled(color[3] * fire::light(vfx, p), color)
}

/// `plasma_tube`, through `plasma::parts`, its light meeting the record's
/// alpha by the frame's cap, `Vfx::cap`.
fn plasma_reference(vfx: &Vfx, p: Pt, color: [f32; 4]) -> [f32; 4] {
    let Some(q) = plasma::parts(vfx, p) else {
        return [0.0; 4];
    };
    let light = (q.env * (q.tube + (1.0 - q.tube) * q.filaments)).clamp(0.0, 1.0);
    scaled(vfx.cap().of(light, color[3]), color)
}

/// `bolt_light`, through `lightning::light`.
fn lightning_reference(vfx: &Vfx, p: Pt, color: [f32; 4]) -> [f32; 4] {
    scaled(color[3] * lightning::light(vfx, p), color)
}

/// `ice_frost`, through `ice::light`.
fn ice_reference(vfx: &Vfx, p: Pt, color: [f32; 4]) -> [f32; 4] {
    scaled(color[3] * ice::light(vfx, p), color)
}

/// A shard, the hexagonal plate the ice draws a particle as, `ice::shard`.
fn ice_shard(uv: Pt, color: [f32; 4]) -> [f32; 4] {
    scaled(color[3] * ice::shard(uv), color)
}

/// The ufo's body, through `ufo::light`.
fn ufo_reference(vfx: &Vfx, p: Pt, color: [f32; 4]) -> [f32; 4] {
    scaled(color[3] * ufo::light(vfx, p), color)
}

/// An exhaust ring, `ufo::ring`.
fn ufo_ring(uv: Pt, color: [f32; 4]) -> [f32; 4] {
    scaled(color[3] * ufo::ring(uv), color)
}

/// `orb_light`, through `orb::light`.
fn orb_reference(vfx: &Vfx, p: Pt, color: [f32; 4]) -> [f32; 4] {
    scaled(color[3] * orb::light(vfx, p), color)
}

/// `liquid_light`, through `liquid::light`.
fn liquid_reference(vfx: &Vfx, p: Pt, color: [f32; 4]) -> [f32; 4] {
    scaled(color[3] * liquid::light(vfx, p), color)
}

/// `fx_particle`: a soft disc over its record, at `uv` -1 to 1 on each axis.
fn fx_particle(uv: Pt, color: [f32; 4]) -> [f32; 4] {
    let a = 1.0 - smoothstep(0.15, 1.0, (uv.x * uv.x + uv.y * uv.y).sqrt());
    scaled(color[3] * a * a, color)
}

/// The reference for `effect`.
pub fn reference(effect: Effect) -> Reference {
    let of = |band, particle| Reference { band, particle };
    match effect {
        Effect::Fire => of(fire_reference, fx_particle),
        Effect::Plasma => of(plasma_reference, fx_particle),
        Effect::Lightning => of(lightning_reference, fx_particle),
        Effect::Ice => of(ice_reference, ice_shard),
        Effect::Ufo => of(ufo_reference, ufo_ring),
        Effect::Orb => of(orb_reference, fx_particle),
        Effect::Liquid => of(liquid_reference, fx_particle),
    }
}

/// A record's colour as the shader reads it: channels 0 to 1, alpha last.
fn unit_color(c: Argb) -> [f32; 4] {
    let f = |v: Chan| f32::from(v.get()) / 255.0;
    [f(c.r()), f(c.g()), f(c.b()), f(c.a())]
}

/// A channel of premultiplied `s` over `d` by `mix`, the backdrop's alpha
/// `ab` and the source's `a`: `cs + cb (1 - as)` source-over, `cs + cb -
/// cs cb` by `screen` and `cs (1 - ab) + cb (1 - as + cs)` by `multiply`.
fn channel(s: f32, d: f32, (a, ab): (f32, f32), mix: Mix) -> f32 {
    match mix {
        Mix::Normal => s + d * (1.0 - a),
        Mix::Screen => s + d * (1.0 - s),
        Mix::Multiply => s * (1.0 - ab) + d * (1.0 - a + s),
    }
}

/// The stored pixel `dst`'s channels, 0 to 1: red, green, blue, alpha.
fn unit(dst: u32) -> (f32, f32, f32, f32) {
    let ch = |k: u32| f32::from(u8::try_from((dst >> k) & 0xff).unwrap_or(0)) / 255.0;
    (ch(16), ch(8), ch(0), ch(24))
}

/// Premultiplied `src` over the stored pixel `dst` by `mix`, rounded once
/// to the target's bytes; the alpha `as + ab (1 - as)` under every mode.
fn mixed(src: [f32; 4], dst: u32, mix: Mix) -> u32 {
    let (r, g, b, a) = unit(dst);
    let byte = |v: f32| u32::try_from((v * 255.0).round().clamp(0.0, 255.0) as i64).unwrap_or(0);
    let c = |s: f32, d: f32| byte(channel(s, d, (src[3], a), mix));
    (byte(src[3] + a * (1.0 - src[3])) << 24)
        | (c(src[0], r) << 16)
        | (c(src[1], g) << 8)
        | c(src[2], b)
}

/// Whether `src` over `dst` lands within a hundredth of a step of halfway
/// between two bytes in any channel: a tie a device may round either way.
fn on_tie(src: [f32; 4], dst: u32, mix: Mix) -> bool {
    let (r, g, b, a) = unit(dst);
    let near = |v: f32| {
        let v = v * 255.0;
        ((v - v.floor()) - 0.5).abs() < 0.01
    };
    let c = |s: f32, d: f32| near(channel(s, d, (src[3], a), mix));
    near(src[3] + a * (1.0 - src[3])) || c(src[0], r) || c(src[1], g) || c(src[2], b)
}

/// **The frame `records` make over `plain`** (`width` pixels a row) by
/// `rule`, each record clipped to the frame, in order; and which pixels'
/// last composite landed on a tie.
pub fn frame(
    rule: Reference,
    vfx: &Vfx,
    records: &[Instance],
    plain: &[u32],
    width: std::num::NonZeroUsize,
    mix: Mix,
) -> (Vec<u32>, Vec<bool>) {
    let fw = i32::try_from(width.get()).expect("a width");
    let fh = i32::try_from(plain.len() / width).expect("a height");
    let mut out = plain.to_vec();
    let mut ties = vec![false; plain.len()];
    for r in records {
        let (x0, y0) = (i32::from(r.x.0), i32::from(r.y.0));
        let (w, h) = (i32::from(r.w.0), i32::from(r.h.0));
        let color = unit_color(r.color);
        for y in y0.max(0)..(y0 + h).min(fh) {
            for x in x0.max(0)..(x0 + w).min(fw) {
                let at = usize::try_from(y * fw + x).expect("a pixel");
                let centre = Pt {
                    x: x as f32 + 0.5,
                    y: y as f32 + 0.5,
                };
                let src = if r.kind == Kind::Particle {
                    let uv = Pt {
                        x: (centre.x - x0 as f32) / w as f32 * 2.0 - 1.0,
                        y: (centre.y - y0 as f32) / h as f32 * 2.0 - 1.0,
                    };
                    (rule.particle)(uv, color)
                } else {
                    (rule.band)(vfx, centre, color)
                };
                ties[at] = on_tie(src, out[at], mix);
                out[at] = mixed(src, out[at], mix);
            }
        }
    }
    (out, ties)
}
