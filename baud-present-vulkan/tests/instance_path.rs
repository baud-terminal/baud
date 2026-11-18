//! **The instance path draws a frame**, on lavapipe and on RADV: the cells'
//! backgrounds, a flat record and a mask record over them, read back and
//! checked pixel for pixel against the CPU renderer's own blend
//! (`composite.rs`, `Blend::over`, native), with the one allowance
//! `common::assert_frame` gives a device that rounds term by term. RADV
//! needs the GPU, so it is `#[ignore]`d and run by name from
//! `scripts/linux-vulkan-radv.sh`.

#![cfg(target_os = "linux")]

mod common;

use ash::vk::DriverId;
use baud_present_vulkan::draw::{Pass, Records, Scene, Texel, Texels};
use baud_present_vulkan::frame::{Gpu, Premultiplied};
use baud_present_vulkan::pipeline::Pipelines;
use baud_present_wayland::Px;
use common::{Bar, Rounding, assert_frame, lavapipe, radv, record};

/// The CPU renderer's over (`Blend::over`, native) of an opaque straight
/// `color` at coverage `cov` over an opaque `dst`.
fn over(color: u32, cov: u32, dst: u32) -> u32 {
    let mix = |s: u32| {
        let (c, d) = ((color >> s) & 0xff, (dst >> s) & 0xff);
        (c * cov + d * (255 - cov) + 127) / 255
    };
    0xff00_0000 | mix(16) << 16 | mix(8) << 8 | mix(0)
}

/// **Backgrounds, a flat record and a mask record, as the pipelines blend
/// them**: an 8x4 grid of 8 px cells, every cell's background opaque and
/// its own; a flat opaque record over cell (1, 1); and a mask record over
/// cell (3, 2) whose coverage runs 0 to 255 across its columns, through
/// the identity curve, so each column is that coverage's over.
#[test]
fn the_instance_path_draws_backgrounds_and_records() {
    draw_and_check(&lavapipe(), Rounding::TermByTerm);
}

/// **The same frame on RADV**: every unblended pixel the CPU's, every blended
/// one within a step of it.
#[test]
#[ignore = "needs the GPU: scripts/linux-vulkan-radv.sh"]
fn the_instance_path_is_the_cpu_path_on_radv() {
    draw_and_check(&radv(), Rounding::OnceAtTies);
}

fn draw_and_check(gpu: &Gpu, rounding: Rounding) {
    let mut pipes = Pipelines::new(&gpu.device).expect("the pipelines");
    let (cols, rows, cell) = (8u32, 4u32, 8u32);
    let (w, h) = (cols * cell, rows * cell);
    let t = gpu.target(Px(w), Px(h)).expect("a target");
    let bg_of = |x: u32, y: u32| 0xff00_0000 | (x * 30) << 16 | (y * 60) << 8 | 0x40;
    let mut bg = Vec::new();
    for y in 0..rows {
        for x in 0..cols {
            bg.extend_from_slice(&bg_of(x, y).to_le_bytes());
        }
    }
    // The mask atlas: one 8x8 tile whose column c covers 255 * c / 7.
    let cov = |c: u32| 255 * c / 7;
    let mut mask = Vec::new();
    for _ in 0..8 {
        for c in 0..8 {
            mask.push(u8::try_from(cov(c)).expect("a byte"));
        }
    }
    // Curve row 0, the identity.
    let curves: Vec<u8> = (0..=255u8).collect();
    let color_atlas = [0u8; 4];
    let flat = 0xff20_a0e0;
    let ink = 0xfff0_e010;
    let mut records = Vec::new();
    records.extend_from_slice(&record((8, 8), (8, 8), (0, 0), (flat, 2)));
    records.extend_from_slice(&record((24, 16), (8, 8), (0, 0), (ink, 0)));
    let u = [
        w as f32,
        h as f32,
        cell as f32,
        cell as f32,
        cols as f32,
        rows as f32,
        8.0,
        8.0,
        1.0,
        1.0,
        0.0,
        0.0,
    ];
    let sheets = common::sheets(
        gpu,
        Texels {
            width: Px(8),
            height: Px(8),
            texel: Texel::Coverage,
            bytes: &mask,
        },
        Texels {
            width: Px(1),
            height: Px(1),
            texel: Texel::Argb,
            bytes: &color_atlas,
        },
        &curves,
    );
    let scene = Scene {
        u,
        backgrounds: Texels {
            width: Px(cols),
            height: Px(rows),
            texel: Texel::Argb,
            bytes: &bg,
        },
        sheets: &sheets,
        records: &records,
        count: Records(2),
    };
    gpu.draw(&mut pipes, &t, &scene, Pass::Whole)
        .expect("a drawn frame");
    let got = gpu.read(&t).expect("the frame read back");
    let (mut want, mut blended) = (Vec::new(), Vec::new());
    for py in 0..h {
        for px in 0..w {
            let (cx, cy) = (px / cell, py / cell);
            let under = bg_of(cx, cy);
            let c = cov(px % cell);
            want.push(match (cx, cy) {
                (1, 1) => flat,
                (3, 2) => over(ink, c, under),
                _ => under,
            });
            blended.push((cx, cy) == (3, 2) && c > 0 && c < 255);
        }
    }
    let width = usize::try_from(w).expect("a width");
    assert_frame(
        &got,
        &want,
        &blended,
        Bar {
            rounding,
            driver: gpu.driver(),
        },
        width,
    );
    sheets.destroy(gpu);
    gpu.drop_target(t);
    // The device is idle after `draw`'s wait.
    pipes.destroy();
}

/// **A pass over the target keeps what it holds**: a target cleared to
/// transparent, then a translucent flat record drawn over one cell by
/// [`Pass::Over`], with no background pass. That cell is the record's
/// premultiplied colour over nothing, and every other pixel is still
/// transparent. A whole pass would have painted the backgrounds over it.
/// Exact on lavapipe too: over a transparent target the destination term
/// is zero, so there is no second term to round.
#[test]
fn a_pass_over_keeps_the_target_it_draws_on() {
    over_and_check(&lavapipe(), Rounding::OnceAtTies);
}

#[test]
#[ignore = "needs the GPU: scripts/linux-vulkan-radv.sh"]
fn a_pass_over_keeps_the_target_on_radv() {
    over_and_check(&radv(), Rounding::OnceAtTies);
}

fn over_and_check(gpu: &Gpu, rounding: Rounding) {
    let mut pipes = Pipelines::new(&gpu.device).expect("the pipelines");
    let (w, h, cell) = (32u32, 16u32, 8u32);
    let t = gpu.target(Px(w), Px(h)).expect("a target");
    gpu.clear(&t, Premultiplied(0), None)
        .expect("a cleared target");
    let ink = 0x8030_c0f0;
    let records = record((8, 8), (8, 8), (0, 0), (ink, 2));
    // Backgrounds a whole pass would paint opaque: an over pass must not.
    let bg = vec![0xffu8; 4 * 4 * 2];
    let one = [0u8; 4];
    let curves: Vec<u8> = (0..=255u8).collect();
    let texels = |bytes| Texels {
        width: Px(1),
        height: Px(1),
        texel: Texel::Argb,
        bytes,
    };
    let sheets = common::sheets(
        gpu,
        Texels {
            width: Px(1),
            height: Px(1),
            texel: Texel::Coverage,
            bytes: &one[..1],
        },
        texels(&one),
        &curves,
    );
    let scene = Scene {
        u: [
            w as f32,
            h as f32,
            cell as f32,
            cell as f32,
            4.0,
            2.0,
            1.0,
            1.0,
            1.0,
            1.0,
            0.0,
            0.0,
        ],
        backgrounds: Texels {
            width: Px(4),
            height: Px(2),
            texel: Texel::Argb,
            bytes: &bg,
        },
        sheets: &sheets,
        records: &records,
        count: Records(1),
    };
    gpu.draw(&mut pipes, &t, &scene, Pass::Over)
        .expect("a pass over the target");
    let got = gpu.read(&t).expect("the frame read back");
    let painted = baud_domain::premultiply(ink);
    let (mut want, mut blended) = (Vec::new(), Vec::new());
    for py in 0..h {
        for px in 0..w {
            let inside = (8..16).contains(&px) && (8..16).contains(&py);
            want.push(if inside { painted } else { 0 });
            blended.push(inside);
        }
    }
    assert_frame(
        &got,
        &want,
        &blended,
        Bar {
            rounding,
            driver: gpu.driver(),
        },
        usize::try_from(w).expect("a width"),
    );
    sheets.destroy(gpu);
    gpu.drop_target(t);
    pipes.destroy();
}

/// **The bias bound discriminates, and only where it is gated**: a step each
/// blended pixel takes, half one way and half the other, is rounding and
/// passes; the same step taken the same way on every blended pixel stays
/// within a step a pixel and must still fail on RADV, on the bias alone, and
/// pass on llvmpipe, whose lean is reported and not gated. No GPU: the frames
/// are made here, and the driver named.
#[test]
fn a_step_every_pixel_takes_one_way_fails() {
    let want: Vec<u32> = (0..64u32).map(|i| 0x8040_2010 + i * 0x0001_0101).collect();
    let blended: Vec<bool> = (0..64).map(|i| i % 4 != 0).collect();
    let step = |up: &dyn Fn(usize) -> bool| -> Vec<u32> {
        want.iter()
            .enumerate()
            .map(|(i, &w)| match (blended[i], up(i)) {
                (false, _) => w,
                (true, true) => w + 0x0101_0101,
                (true, false) => w - 0x0101_0101,
            })
            .collect()
    };
    let both_ways = step(&|i| i % 8 < 4);
    let (radv, llvmpipe) = (DriverId::MESA_RADV, DriverId::MESA_LLVMPIPE);
    assert_frame(
        &both_ways,
        &want,
        &blended,
        Bar {
            rounding: Rounding::TermByTerm,
            driver: radv,
        },
        8,
    );
    let planted = step(&|_| true);
    assert!(
        std::panic::catch_unwind(|| {
            assert_frame(
                &planted,
                &want,
                &blended,
                Bar {
                    rounding: Rounding::TermByTerm,
                    driver: radv,
                },
                8,
            )
        })
        .is_err(),
        "a +1 on every blended pixel passed on RADV: the bias bound tested nothing"
    );
    assert_frame(
        &planted,
        &want,
        &blended,
        Bar {
            rounding: Rounding::TermByTerm,
            driver: llvmpipe,
        },
        8,
    );
}
