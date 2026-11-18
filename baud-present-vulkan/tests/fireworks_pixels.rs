//! **The fireworks' stars against the CPU path's**, on lavapipe and on
//! RADV: a show's records (`fireworks::records`) drawn by the fireworks'
//! stages over a target cleared to opaque black, read back and held, pixel
//! for pixel, to the CPU path's frame of the same records over the same
//! black buffer (`Target::fireworks`, an opaque page): every channel
//! within one step. Each star's coverage is formed as the byte the CPU
//! takes (`coverage_byte`, `alpha_byte`), so the two meet byte for byte but
//! where a float lands within a step of a byte boundary. RADV needs the
//! GPU, so it is `#[ignore]`d and run by name from
//! `scripts/linux-vulkan-radv.sh`.

#![cfg(target_os = "linux")]

mod common;

use baud_font::Px as Pixels;
use baud_present_vulkan::draw::{Records, Scene, Texel, Texels};
use baud_present_vulkan::effect::Drawn;
use baud_present_vulkan::frame::{Gpu, Premultiplied};
use baud_present_vulkan::pipeline::Pipelines;
use baud_present_wayland::Px;
use baud_render::effect::Seed;
use baud_render::fireworks::{self, BAUD, Form, HOLD, HOLE, ORIGINS, Show};
use baud_render::{Band, Instance, Page, Pt, Target};
use common::{BIAS, bias_gated, lavapipe, mean_signed, radv};

const W: u32 = 240;
const H: u32 = 92;
const BLACK: u32 = 0xFF00_0000;

/// The show `age` seconds in, every burst and the hole at the frame's
/// middle, at a scale of one.
fn show(age: f32, form: Form) -> Show {
    let at = Pt { x: 120.0, y: 46.0 };
    Show {
        age,
        hold: HOLD,
        origins: [at; ORIGINS],
        hole: at,
        scale: 1.0,
        seed: Seed(41),
        look: BAUD,
        form,
    }
}

/// The records and uniform the GPU draws `s` with, and the CPU path's
/// frame of the same records.
fn scene(s: &Show) -> (Vec<u8>, Records, [u8; 32], Vec<u32>) {
    let mut records: Vec<Instance> = Vec::new();
    fireworks::records(s, (Pixels::of(W), Pixels::of(H)), &mut records);
    let bytes: Vec<u8> = records.iter().flat_map(|r| r.to_bytes()).collect();
    let mut u = [0u8; 32];
    for (i, f) in fireworks::uniform([0.0, 0.0], s.hole, HOLE * s.scale)
        .iter()
        .enumerate()
    {
        u[4 * i..4 * i + 4].copy_from_slice(&f.to_le_bytes());
    }
    let mut cpu = vec![BLACK; usize::try_from(W * H).expect("a frame")];
    {
        let mut t =
            Target::new(&mut cpu, Pixels::of(W), Pixels::of(W), Pixels::of(H)).expect("a buffer");
        let all = Band {
            top: Pixels::of(0),
            bottom: Pixels::of(H),
        };
        t.fireworks(
            &records,
            (s.hole, HOLE * s.scale),
            baud_render::Blend::Native,
            Page::Opaque,
            all,
        );
    }
    let n = u32::try_from(records.len()).expect("a record count");
    (bytes, Records(n), u, cpu)
}

fn gpu_frame(
    gpu: &Gpu,
    pipes: &mut Pipelines,
    records: &[u8],
    n: Records,
    u: &[u8; 32],
) -> Vec<u32> {
    let t = gpu.target(Px(W), Px(H)).expect("a target");
    gpu.clear(&t, Premultiplied(BLACK), None)
        .expect("a black target");
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
    // The pipeline is built before the frame that draws it: a test may wait.
    let mut drawn = gpu.draw_fireworks(pipes, &t, &scene, u).expect("a draw");
    while drawn == Drawn::Building {
        std::thread::sleep(std::time::Duration::from_millis(5));
        let _ = pipes.finished();
        drawn = gpu.draw_fireworks(pipes, &t, &scene, u).expect("a draw");
    }
    let got = gpu.read(&t).expect("the frame read back");
    sheets.destroy(gpu);
    gpu.drop_target(t);
    got
}

fn every_show(gpu: &Gpu) {
    let mut pipes = Pipelines::new(&gpu.device).expect("the pipelines");
    for (age, form) in [
        (0.15, Form::Moving),
        (0.5, Form::Moving),
        (1.0, Form::Moving),
        (0.0, Form::Still),
    ] {
        let s = show(age, form);
        let (records, n, u, want) = scene(&s);
        let lit = want.iter().filter(|&&p| p != BLACK).count();
        assert!(
            lit > 10,
            "{form:?} at {age}: the CPU path draws it ({lit} pixels)"
        );
        let got = gpu_frame(gpu, &mut pipes, &records, n, &u);
        let step = |a: u32, b: u32| {
            (0..4)
                .map(|k| ((a >> (8 * k)) & 0xff).abs_diff((b >> (8 * k)) & 0xff))
                .max()
                .unwrap_or(0)
        };
        let worst = got
            .iter()
            .zip(&want)
            .map(|(&a, &b)| step(a, b))
            .max()
            .unwrap_or(0);
        let off = got.iter().zip(&want).filter(|(a, b)| a != b).count();
        // The mean signed difference a channel over the lit pixels, as the
        // shared bar measures it (blue, green, red, alpha), put red first.
        let lit_px: Vec<bool> = want.iter().map(|&w| w != BLACK).collect();
        let [b, g, r, a] = mean_signed(&got, &want, &lit_px);
        let bias = [r, g, b, a];
        eprintln!(
            "{form:?} at {age}: {lit} lit, {off} off, worst {worst}, bias r {:+.3} g {:+.3} b {:+.3} a {:+.3}",
            bias[0], bias[1], bias[2], bias[3]
        );
        assert!(worst <= 1, "{form:?} at {age}: a pixel {worst} steps off");
        assert!(
            !bias_gated(gpu.driver()) || bias.iter().all(|b| b.abs() <= BIAS),
            "{form:?} at {age}: a channel drifts {bias:?}"
        );
    }
    pipes.destroy();
}

/// **Every show on lavapipe, within a step of the CPU path's.**
#[test]
fn the_fireworks_match_the_cpu_path() {
    every_show(&lavapipe());
}

/// **The same on RADV.**
#[test]
#[ignore = "needs the GPU: scripts/linux-vulkan-radv.sh runs it by name"]
fn the_fireworks_match_the_cpu_path_on_radv() {
    every_show(&radv());
}
