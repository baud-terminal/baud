//! **Every effect's shader against the CPU path's effect**, on lavapipe and
//! on RADV: the effect's records drawn by its fragment over a target cleared
//! to opaque black, so the colour's destination term is zero and no device
//! rounds it twice, and every pixel is opaque, so the CPU's straight bytes
//! and the GPU's premultiplied ones are the same; read back and held to the
//! CPU path's frame of the same records over the same black buffer
//! (`Target::effect`, an opaque page), which is held in turn to the
//! effects' reference exactly, as the macOS oracle holds its CPU arm
//! (`baud-present-macos/tests/cells.rs`, `effect_scenes`).
//!
//! **The bar**: every channel of every pixel within one step of 255 of the
//! CPU's ([`allowed`]), on every device; on lavapipe, which rounds a blend's
//! terms each to a byte before adding them, two steps on a pixel two or
//! more records cover, counted from the records alone. And no drift: over
//! a scene's lit pixels the mean signed difference a channel stays within
//! [`BIAS`] of a step on every device the shared bar holds to it
//! ([`bias_gated`]: all but lavapipe, whose term-by-term rounding leans), so a
//! device a step high everywhere fails what each pixel alone would pass.
//! Reported per scene and device, held or not. RADV needs the
//! GPU, so it is `#[ignore]`d and run by name from
//! `scripts/linux-vulkan-radv.sh`.

#![cfg(target_os = "linux")]

mod common;
mod reference;

use baud_domain::Chan;
use baud_font::Px as Pixels;
use baud_present_vulkan::draw::{Records, Scene, Texel, Texels};
use baud_present_vulkan::frame::{Gpu, Multiply, Premultiplied};
use baud_present_vulkan::pipeline::{Blend, Fx, Pipelines};
use baud_present_wayland::Px;
use baud_render::effect::{Effect, Ignition, Seed, Vfx};
use baud_render::{
    Argb, Band, Corners, Instance, Mix, Page, Pt, Rect, Shape, Smear, Strength, Target, Theme,
};
use common::{BIAS, Bar, Rounding, bias_gated, lavapipe, mean_signed, radv};

/// The scene's size: 24 by 4 cells of 10 by 23 pixels.
const W: u32 = 240;
const H: u32 = 92;

/// The page under every scene: opaque black.
const BLACK: u32 = 0xFF00_0000;

/// The scene's pixels, as a buffer's length.
fn pixels() -> usize {
    usize::try_from(W * H).expect("a frame")
}

/// The scene's width, as the reference takes it.
fn width() -> std::num::NonZeroUsize {
    std::num::NonZeroUsize::new(usize::try_from(W).expect("a width")).expect("a width")
}

/// The cursor's colour, the records' fill.
const FILL: u32 = 0xFFF4_A32C;

/// The fragment shader that draws `effect`.
fn fx(effect: Effect) -> Fx {
    match effect {
        Effect::Fire => Fx::Fire,
        Effect::Plasma => Fx::Plasma,
        Effect::Lightning => Fx::Lightning,
        Effect::Ice => Fx::Ice,
        Effect::Ufo => Fx::Ufo,
        Effect::Orb => Fx::Orb,
        Effect::Liquid => Fx::Liquid,
    }
}

/// **`effect` lit by a move from row 0, column 2 to row 2, column 18**,
/// `age` seconds in: the quad between the two cells, the ignition at the
/// tail, seed 5.
fn vfx(effect: Effect, age: f32) -> Vfx {
    let cell = |c: f32, r: f32| Rect {
        left: c * 10.0,
        top: r * 23.0,
        right: c * 10.0 + 10.0,
        bottom: r * 23.0 + 23.0,
    };
    let (from, hole) = (cell(2.0, 0.0), cell(18.0, 2.0));
    let corners = Corners([
        Pt {
            x: from.right,
            y: from.top,
        },
        Pt {
            x: hole.right,
            y: hole.bottom,
        },
        Pt {
            x: hole.left,
            y: hole.bottom,
        },
        Pt {
            x: from.left,
            y: from.top,
        },
    ]);
    let tail = Pt {
        x: (from.left + from.right) * 0.5,
        y: (from.top + from.bottom) * 0.5,
    };
    Vfx {
        effect,
        emitter: Smear {
            corners,
            hole,
            shape: Shape::Quad,
            strength: Strength::Full,
            opacity: Smear::OPAQUE,
        },
        ignition: Ignition::ignite(tail, hole, Seed(5), 23.0).at(age),
        axis: Pt { x: 1.0, y: 0.0 },
    }
}

/// The records and the uniform's bytes the GPU draws `v` with, the CPU
/// path's frame of the same records over an opaque black buffer -- which is
/// the reference's, pixel for pixel, as the macOS oracle holds the CPU arm
/// -- and how many records cover each pixel.
fn scene(v: &Vfx) -> (Vec<u8>, Records, [u8; 144], Vec<u32>, Vec<u8>) {
    let mut records: Vec<Instance> = Vec::new();
    v.instances((Pixels::of(W), Pixels::of(H)), Argb(FILL), &mut records);
    let bytes: Vec<u8> = records.iter().flat_map(|r| r.to_bytes()).collect();
    let mut u = [0u8; 144];
    for (i, f) in v.uniform([0.0, 0.0]).iter().enumerate() {
        u[4 * i..4 * i + 4].copy_from_slice(&f.to_le_bytes());
    }
    let mut cpu = vec![BLACK; pixels()];
    {
        let mut t =
            Target::new(&mut cpu, Pixels::of(W), Pixels::of(W), Pixels::of(H)).expect("a buffer");
        let all = Band {
            top: Pixels::of(0),
            bottom: Pixels::of(H),
        };
        t.effect(v, &records, baud_render::Blend::Native, Page::Opaque, all);
    }
    let plain = vec![BLACK; pixels()];
    let rule = reference::reference(v.effect);
    let (want, _) = reference::frame(rule, v, &records, &plain, width(), Mix::Normal);
    let apart = cpu.iter().zip(&want).filter(|(a, b)| a != b).count();
    assert_eq!(
        apart, 0,
        "{:?}: the CPU path is off the reference",
        v.effect
    );
    let n = u32::try_from(records.len()).expect("a record count");
    (bytes, Records(n), u, cpu, cover(&records))
}

/// **How many records cover each pixel**, each clipped to the frame: where
/// a device that rounds a blend term by term rounds twice.
fn cover(records: &[Instance]) -> Vec<u8> {
    let (w, h) = (
        i32::try_from(W).expect("a width"),
        i32::try_from(H).expect("a height"),
    );
    let mut n = vec![0u8; pixels()];
    for r in records {
        let (x0, y0) = (i32::from(r.x.0), i32::from(r.y.0));
        let (x1, y1) = (x0 + i32::from(r.w.0), y0 + i32::from(r.h.0));
        for y in y0.max(0)..y1.min(h) {
            for x in x0.max(0)..x1.min(w) {
                let at = usize::try_from(y * w + x).expect("a pixel");
                n[at] = n[at].saturating_add(1);
            }
        }
    }
    n
}

/// **The frame the GPU draws**: the records by `shader` and `blend` over a
/// target cleared to opaque black, read back.
fn gpu_frame(
    gpu: &Gpu,
    pipes: &mut Pipelines,
    (shader, blend, page): (Fx, Blend, u32),
    records: &[u8],
    n: Records,
    u: &[u8; 144],
) -> Vec<u32> {
    let t = gpu.target(Px(W), Px(H)).expect("a target");
    gpu.clear(&t, Premultiplied(page), None)
        .expect("a cleared target");
    let one = [0u8; 4];
    let curves: Vec<u8> = (0..=255u8).collect();
    let tex = |texel, bytes| Texels {
        width: Px(1),
        height: Px(1),
        texel,
        bytes,
    };
    let sheets = common::sheets(
        gpu,
        tex(Texel::Coverage, &one[..1]),
        tex(Texel::Argb, &one),
        &curves,
    );
    let scene = Scene {
        u: [
            W as f32, H as f32, 10.0, 23.0, 24.0, 4.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0,
        ],
        backgrounds: tex(Texel::Argb, &one),
        sheets: &sheets,
        records,
        count: n,
        framing: None,
    };
    gpu.draw_effect(pipes, &t, &scene, shader, blend, u)
        .expect("the effect drawn");
    let got = gpu.read(&t).expect("the frame read back");
    sheets.destroy(gpu);
    gpu.drop_target(t);
    got
}

/// **How far `got` is from `want` under the bar**: the pixels past what
/// [`allowed`] gives them, the worst step, and each channel's mean signed
/// difference over the lit pixels (red, green, blue, alpha).
/// Pixels counted.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct Count(usize);

/// Steps of 255 a channel is off.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct Steps(u32);

/// Whether a device's lean is held to [`BIAS`] or only reported ([`bias_gated`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Lean {
    Held,
    Reported,
}

struct Verdict {
    over: Count,
    worst: Steps,
    /// Red, green, blue, alpha.
    bias: [f64; 4],
    lean: Lean,
}

impl Verdict {
    fn passes(&self) -> bool {
        self.over == Count(0) && (self.lean == Lean::Reported || self.leans_within())
    }

    fn leans_within(&self) -> bool {
        self.bias.iter().all(|b| b.abs() <= BIAS)
    }
}

/// The steps a pixel covered by `cover` records may be off on `rounding`:
/// one, or two where lavapipe rounds two blends term by term.
fn allowed(cover: u8, rounding: Rounding) -> u32 {
    match (rounding, cover >= 2) {
        (Rounding::TermByTerm, true) => 2,
        _ => 1,
    }
}

fn judge(got: &[u32], want: &[u32], cover: &[u8], bar: Bar) -> Verdict {
    judge_over(got, want, Premultiplied(BLACK), bar.driver, &|i| {
        Some(allowed(cover[i], bar.rounding))
    })
}

/// [`judge`] over `page`: pixel `i` allowed `allow(i)` steps, or not judged
/// where that is `None`, and lit where the reference is not the page.
fn judge_over(
    got: &[u32],
    want: &[u32],
    page: Premultiplied,
    driver: ash::vk::DriverId,
    allow: &dyn Fn(usize) -> Option<u32>,
) -> Verdict {
    let ch = |p: u32, k: u32| i64::from((p >> (8 * k)) & 0xff);
    let mut over = 0;
    let mut worst = 0;
    let mut lit = vec![false; got.len()];
    for i in 0..got.len() {
        let Some(allowed) = allow(i) else {
            continue;
        };
        let step = (0..4)
            .map(|k| ch(got[i], k).abs_diff(ch(want[i], k)))
            .max()
            .unwrap_or(0);
        let step = u32::try_from(step).unwrap_or(u32::MAX);
        worst = worst.max(step);
        over += usize::from(step > allowed);
        lit[i] = want[i] != page.0;
    }
    // `mean_signed` is blue, green, red, alpha.
    let [b, g, r, a] = mean_signed(got, want, &lit);
    Verdict {
        over: Count(over),
        worst: Steps(worst),
        bias: [r, g, b, a],
        lean: if bias_gated(driver) {
            Lean::Held
        } else {
            Lean::Reported
        },
    }
}

/// How the device under test rounds a blend: lavapipe, by its driver,
/// term by term.
fn rounding(gpu: &Gpu) -> Rounding {
    match gpu.driver() {
        ash::vk::DriverId::MESA_LLVMPIPE => Rounding::TermByTerm,
        _ => Rounding::OnceAtTies,
    }
}

/// The bar a device's frames are held to: its rounding and its driver.
fn bar(gpu: &Gpu) -> Bar {
    Bar {
        rounding: rounding(gpu),
        driver: gpu.driver(),
    }
}

fn every_effect(gpu: &Gpu) {
    let bar = bar(gpu);
    let rounding = bar.rounding;
    let mut pipes = Pipelines::new(&gpu.device).expect("the pipelines");
    for effect in Effect::ALL {
        for age in [0.1, 0.25, 0.4].map(|k| k * effect.life()) {
            let v = vfx(effect, age);
            let (records, n, u, want, cover) = scene(&v);
            let lit = want.iter().filter(|&&p| p != BLACK).count();
            assert!(
                lit > 10,
                "{effect:?} at {age}: the CPU path draws it ({lit} pixels)"
            );
            let got = gpu_frame(
                gpu,
                &mut pipes,
                (fx(effect), Blend::Normal, BLACK),
                &records,
                n,
                &u,
            );
            let v = judge(&got, &want, &cover, bar);
            eprintln!(
                "{rounding:?} {effect:?} at {age:.3}: {lit} lit, worst {}, {} over, bias r {:+.3} g {:+.3} b {:+.3} a {:+.3}",
                v.worst.0, v.over.0, v.bias[0], v.bias[1], v.bias[2], v.bias[3]
            );
            assert!(v.passes(), "{effect:?} at {age}: off the bar");
        }
    }
    // The controls, on the fire mid-life.
    let v = vfx(Effect::Fire, 0.25 * Effect::Fire.life());
    let (records, n, u, want, cover) = scene(&v);
    // **Fed**: the fire's records and uniform drawn by the ice's shader fail
    // the bar, so it tells two effects apart.
    let ice = gpu_frame(
        gpu,
        &mut pipes,
        (Fx::Ice, Blend::Normal, BLACK),
        &records,
        n,
        &u,
    );
    assert!(
        !judge(&ice, &want, &cover, bar).passes(),
        "fire drawn as ice passes"
    );
    // **A step too many**: one lit pixel two steps past what it is allowed
    // fails the step bound.
    let first = (0..want.len())
        .find(|&i| want[i] != BLACK && cover[i] == 1)
        .expect("a lit pixel one record covers");
    let mut planted = want.clone();
    // Its red two steps off: down where there is room, else up.
    let red = (want[first] >> 16) & 0xff;
    let moved = if red >= 2 { red - 2 } else { red + 2 };
    planted[first] = (want[first] & !0x00ff_0000) | (moved << 16);
    let step = judge(&planted, &want, &cover, bar);
    assert!(
        step.over > Count(0) && !step.passes(),
        "a pixel two steps off passes"
    );
    // **Drift**: every lit pixel a step high in red, green and blue passes
    // the step bound, its lean is measured past the bound, and a device whose
    // lean is held fails it.
    let high: Vec<u32> = want
        .iter()
        .map(|&p| match p {
            BLACK => p,
            _ => {
                let up = |k: u32| (((p >> k) & 0xff) + 1).min(255) << k;
                (p & 0xff00_0000) | up(16) | up(8) | up(0)
            }
        })
        .collect();
    let drift = judge(&high, &want, &cover, bar);
    assert_eq!(
        drift.over,
        Count(0),
        "a step high everywhere is within the step bound"
    );
    assert!(
        drift.bias[..3].iter().all(|b| *b > BIAS),
        "a step high everywhere is not measured as a lean: {:?}",
        drift.bias
    );
    assert!(
        drift.lean == Lean::Reported || !drift.passes(),
        "a step high everywhere passes the bias bound"
    );
    pipes.destroy();
}

/// **The plasma's mixes over the four pages the theme gives them**: the
/// dark theme's page and its glass by `screen`, the light's and its glass
/// by `multiply`, each premultiplied as the target stores it.
fn mix_pages() -> [(&'static str, u32, Blend, Mix); 4] {
    let page = |t: Theme, alpha: Chan| {
        baud_domain::premultiply((u32::from(alpha.get()) << 24) | (t.bg.0 & 0x00ff_ffff))
    };
    [
        (
            "dark",
            page(Theme::dark(), Chan::from_u8(255)),
            Blend::Screen,
            Mix::Screen,
        ),
        (
            "dark glass",
            page(Theme::dark(), Chan::from_u8(179)),
            Blend::Screen,
            Mix::Screen,
        ),
        (
            "light",
            page(Theme::light(), Chan::from_u8(255)),
            Blend::Multiply,
            Mix::Multiply,
        ),
        (
            "light glass",
            page(Theme::light(), Chan::from_u8(153)),
            Blend::Multiply,
            Mix::Multiply,
        ),
    ]
}

/// The steps a mixed pixel covered by `cover` records may be off by `mix`:
/// one on a device that rounds once, but two for `multiply` where two or
/// more records cover the pixel, on every device -- each composite rounds
/// to the target's bytes, and the second takes the first's rounding into
/// its backdrop term (`multiply_s_second_step_is_its_second_rounding`
/// shows each such pixel within a step of the second composite over the
/// device's own first). On lavapipe, which rounds the backdrop's term apart
/// from the source's, one more on a pixel the plasma blends, and one more
/// again where two records do. A pixel no record covers is the page, a step
/// at most everywhere.
fn mix_allowed(cover: u8, rounding: Rounding, mix: Mix) -> u32 {
    match (rounding, mix) {
        (Rounding::TermByTerm, _) => 1 + u32::from(cover.min(2)),
        (_, Mix::Multiply) if cover >= 2 => 2,
        _ => 1,
    }
}

/// The reference's frame of the plasma `v` over `page` by `mix`, and how
/// many records cover each pixel.
fn mix_scene(v: &Vfx, page: u32, mix: Mix) -> (Vec<u8>, Records, [u8; 144], Vec<u32>, Vec<u8>) {
    let (bytes, n, u, _, cover) = scene(v);
    let mut records: Vec<Instance> = Vec::new();
    v.instances((Pixels::of(W), Pixels::of(H)), Argb(FILL), &mut records);
    let plain = vec![page; pixels()];
    let rule = reference::reference(v.effect);
    let (want, _) = reference::frame(rule, v, &records, &plain, width(), mix);
    (bytes, n, u, want, cover)
}

fn every_mix(gpu: &Gpu) {
    let rounding = rounding(gpu);
    let mut pipes = Pipelines::new(&gpu.device).expect("the pipelines");
    for (name, page, blend, mix) in mix_pages() {
        for age in [0.1, 0.25, 0.4].map(|k| k * Effect::Plasma.life()) {
            let v = vfx(Effect::Plasma, age);
            let (records, n, u, want, cover) = mix_scene(&v, page, mix);
            let lit = want.iter().filter(|&&p| p != page).count();
            assert!(
                lit > 10,
                "{name} at {age}: the reference draws it ({lit} pixels)"
            );
            let got = gpu_frame(gpu, &mut pipes, (Fx::Plasma, blend, page), &records, n, &u);
            let j = judge_over(&got, &want, Premultiplied(page), gpu.driver(), &|i| {
                Some(mix_allowed(cover[i], rounding, mix))
            });
            eprintln!(
                "{rounding:?} {name} {mix:?} at {age:.3}: {lit} lit, worst {}, {} over, bias r {:+.3} g {:+.3} b {:+.3} a {:+.3}",
                j.worst.0, j.over.0, j.bias[0], j.bias[1], j.bias[2], j.bias[3]
            );
            assert!(j.passes(), "{name} at {age}: off the bar");
            // **The wrong blend fails, allowance and all**: the same records
            // drawn by the other mix, and source-over.
            let other = match blend {
                Blend::Screen => Blend::Multiply,
                _ => Blend::Screen,
            };
            for wrong in [other, Blend::Normal] {
                let bad = gpu_frame(gpu, &mut pipes, (Fx::Plasma, wrong, page), &records, n, &u);
                let j = judge_over(&bad, &want, Premultiplied(page), gpu.driver(), &|i| {
                    Some(mix_allowed(cover[i], rounding, mix))
                });
                assert!(
                    !j.passes(),
                    "{name} at {age}: drawn by {wrong:?}, it passes"
                );
            }
        }
    }
    pipes.destroy();
}

/// **`multiply` in two passes, forced**, on a device that has the second
/// source: exact where one record covers a pixel, which is all the
/// fallback claims, so only those pixels are judged, at the bar of one
/// step; and the passes counted.
fn two_pass(mut gpu: Gpu) {
    gpu.multiply = Multiply::TwoPass;
    let rounding = rounding(&gpu);
    let mut pipes = Pipelines::new(&gpu.device).expect("the pipelines");
    for (name, page, blend, mix) in mix_pages() {
        if blend != Blend::Multiply {
            continue;
        }
        for age in [0.1, 0.25, 0.4].map(|k| k * Effect::Plasma.life()) {
            let v = vfx(Effect::Plasma, age);
            let (records, n, u, want, cover) = mix_scene(&v, page, mix);
            let got = gpu_frame(&gpu, &mut pipes, (Fx::Plasma, blend, page), &records, n, &u);
            let judged = |i: usize| (cover[i] <= 1).then(|| mix_allowed(cover[i], rounding, mix));
            let j = judge_over(&got, &want, Premultiplied(page), gpu.driver(), &judged);
            eprintln!(
                "{rounding:?} two-pass {name} at {age:.3}: worst {}, {} over, bias r {:+.3} g {:+.3} b {:+.3} a {:+.3}",
                j.worst.0, j.over.0, j.bias[0], j.bias[1], j.bias[2], j.bias[3]
            );
            assert!(j.passes(), "two-pass {name} at {age}: off the bar");
        }
    }
    assert_eq!(
        pipes.multiply_two_pass.0, 6,
        "every multiply drawn in two passes"
    );
    pipes.destroy();
}

/// Whether `r`, clipped to the frame, covers pixel `at`.
fn covers(r: &Instance, at: usize) -> bool {
    let w = usize::try_from(W).expect("a width");
    let (x, y) = (
        i32::try_from(at % w).expect("a column"),
        i32::try_from(at / w).expect("a row"),
    );
    let (x0, y0) = (i32::from(r.x.0), i32::from(r.y.0));
    (x0..x0 + i32::from(r.w.0)).contains(&x) && (y0..y0 + i32::from(r.h.0)).contains(&y)
}

/// **Where `multiply` is two steps off, the second step is the second
/// composite's rounding**: for each such pixel, the records before the last
/// one covering it are drawn and read back, the last is composited over
/// those bytes by the reference, and the device's pixel is within one step
/// of that. Prints each pixel, so the evidence for the two-step rule is
/// in the log. A device with no such pixel reports none.
fn isolate(gpu: &Gpu) {
    let mut pipes = Pipelines::new(&gpu.device).expect("the pipelines");
    let ch = |p: u32, k: u32| (p >> (8 * k)) & 0xff;
    let step = |x: u32, y: u32| {
        (0..4)
            .map(|k| ch(x, k).abs_diff(ch(y, k)))
            .max()
            .unwrap_or(0)
    };
    let mut seen = 0;
    for (name, page, blend, mix) in mix_pages() {
        if blend != Blend::Multiply {
            continue;
        }
        for age in [0.1, 0.25, 0.4].map(|k| k * Effect::Plasma.life()) {
            let v = vfx(Effect::Plasma, age);
            let (bytes, n, u, want, cover) = mix_scene(&v, page, mix);
            let mut records: Vec<Instance> = Vec::new();
            v.instances((Pixels::of(W), Pixels::of(H)), Argb(FILL), &mut records);
            let got = gpu_frame(gpu, &mut pipes, (Fx::Plasma, blend, page), &bytes, n, &u);
            for at in (0..got.len()).filter(|&i| step(got[i], want[i]) > 1) {
                seen += 1;
                let last = (0..records.len())
                    .rev()
                    .find(|&k| covers(&records[k], at))
                    .expect("a record over a pixel off the page");
                let before: Vec<u8> = records[..last].iter().flat_map(|r| r.to_bytes()).collect();
                let count = Records(u32::try_from(last).expect("a record count"));
                let under = gpu_frame(
                    gpu,
                    &mut pipes,
                    (Fx::Plasma, blend, page),
                    &before,
                    count,
                    &u,
                );
                let rule = reference::reference(v.effect);
                let (once, _) =
                    reference::frame(rule, &v, &records[last..=last], &under, width(), mix);
                let w = usize::try_from(W).expect("a width");
                eprintln!(
                    "{name} at {age:.3}: ({}, {}) cover {}: got {:08x}, reference {:08x}, under {:08x}, the last record over it {:08x}, {} step(s)",
                    at % w,
                    at / w,
                    cover[at],
                    got[at],
                    want[at],
                    under[at],
                    once[at],
                    step(got[at], once[at])
                );
                assert!(
                    step(got[at], once[at]) <= 1,
                    "{name} at {age}: pixel {at} is more than its second rounding off"
                );
            }
        }
    }
    eprintln!("{seen} pixel(s) two or more steps off, each isolated");
    pipes.destroy();
}

/// **The isolation on lavapipe.**
#[test]
fn multiply_s_second_step_is_its_second_rounding() {
    isolate(&lavapipe());
}

/// **The isolation on RADV**, where the three pixels are.
#[test]
#[ignore = "needs the GPU: scripts/linux-vulkan-radv.sh runs it by name"]
fn multiply_s_second_step_is_its_second_rounding_on_radv() {
    isolate(&radv());
}

/// **The plasma's mixes on lavapipe**, with its allowance.
#[test]
fn the_plasma_mixes_match_the_reference() {
    every_mix(&lavapipe());
}

/// **The same on RADV, at one step everywhere.**
#[test]
#[ignore = "needs the GPU: scripts/linux-vulkan-radv.sh runs it by name"]
fn the_plasma_mixes_match_the_reference_on_radv() {
    every_mix(&radv());
}

/// **The two-pass `multiply` on lavapipe.**
#[test]
fn multiply_in_two_passes_matches_where_one_record_covers() {
    two_pass(lavapipe());
}

/// **The same on RADV.**
#[test]
#[ignore = "needs the GPU: scripts/linux-vulkan-radv.sh runs it by name"]
fn multiply_in_two_passes_matches_where_one_record_covers_on_radv() {
    two_pass(radv());
}

/// **Every effect on lavapipe, within the oracle's bar.**
#[test]
fn every_effect_matches_the_cpu_path() {
    every_effect(&lavapipe());
}

/// **The same on RADV.**
#[test]
#[ignore = "needs the GPU: scripts/linux-vulkan-radv.sh runs it by name"]
fn every_effect_matches_the_cpu_path_on_radv() {
    every_effect(&radv());
}
