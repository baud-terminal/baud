//! **An effect whose pipeline is still building draws nothing and waits for
//! nothing, and its finished build wakes the loop**, on lavapipe: the build
//! held at the worker's gate (`Pipelines::hold_builds`, the `test-hooks`
//! feature), the frame drawn without the effect and the skip counted; then
//! the gate opened with nothing else to draw, the wake descriptor readable,
//! and the next frame draws the effect.

#![cfg(target_os = "linux")]

mod common;

use baud_present_vulkan::draw::{Records, Scene, Texel, Texels};
use baud_present_vulkan::effect::Drawn;
use baud_present_vulkan::frame::{Gpu, Premultiplied, Target};
use baud_present_vulkan::pipeline::{Blend, Built, Fx, Pipelines};
use baud_present_wayland::Px;
use common::{lavapipe, record};
use std::io::Read;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

const W: u32 = 32;
const H: u32 = 16;
const BLACK: u32 = 0xFF00_0000;

/// A fire's uniform over one 8 by 8 band at (8, 4), lit and young: the
/// emitter's corners round the band, the hole outside it.
fn fire_u() -> [u8; 144] {
    let floats: [f32; 36] = [
        8.0,
        4.0,
        16.0,
        4.0,
        16.0,
        12.0,
        8.0,
        12.0, // c01, c23
        100.0,
        100.0,
        108.0,
        108.0, // hole, far away
        0.0,
        0.0,
        8.0,
        8.0, // origin, tail
        1.0,
        0.0,
        8.0,
        8.0, // dir, len, unit
        0.02,
        0.5,
        f32::from_bits(5),
        4.0, // age, life, seed, reach
        2.0,
        0.6,
        0.3,
        0.0,
        0.0,
        0.0,
        0.0,
        0.0, // k
        0.0,
        0.0,
        0.0,
        0.0, // cap, pad
    ];
    let mut u = [0u8; 144];
    for (i, f) in floats.iter().enumerate() {
        u[4 * i..4 * i + 4].copy_from_slice(&f.to_le_bytes());
    }
    u
}

/// One band record over the fire, a black target, and the scene that draws
/// it.
fn frame(gpu: &Gpu) -> (Target, Vec<u8>) {
    let t = gpu.target(Px(W), Px(H)).expect("a target");
    gpu.clear(&t, Premultiplied(BLACK), None)
        .expect("a black target");
    // Kind 4 is a band (`Kind::Band`).
    let records = record((8, 4), (8, 8), (0, 0), (0xFFF4_A32C, 4)).to_vec();
    (t, records)
}

fn draw(gpu: &Gpu, pipes: &mut Pipelines, t: &Target, records: &[u8]) -> Drawn {
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
            W as f32, H as f32, 8.0, 8.0, 4.0, 2.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0,
        ],
        backgrounds: tex(Texel::Argb, &one),
        sheets: &sheets,
        records,
        count: Records(1),
    };
    let drawn = gpu
        .draw_effect_when_ready(pipes, t, &scene, Fx::Fire, Blend::Normal, &fire_u())
        .expect("a draw");
    sheets.destroy(gpu);
    drawn
}

/// Whether `pipes`' wake descriptor becomes readable within `limit`.
fn woken(pipes: &Pipelines, limit: Duration) -> bool {
    let fd = pipes.ready_fd().expect("a wake descriptor");
    let stream = UnixStream::from(fd.try_clone_to_owned().expect("a copy"));
    stream.set_nonblocking(true).expect("non-blocking");
    let until = Instant::now() + limit;
    let mut one = [0u8; 1];
    while Instant::now() < until {
        if matches!((&stream).read(&mut one), Ok(n) if n > 0) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    false
}

/// **The first lit frame, its pipeline held, draws nothing and never waits**:
/// the draw returns at once with `Building`, the target is still the black
/// it was cleared to, and the skip is counted. Named break: a draw that
/// builds the pipeline it lacks (`Pipelines::get`) never returns while the
/// build is held.
#[test]
fn a_building_effect_draws_nothing_and_never_waits() {
    let gpu = lavapipe();
    let mut pipes = Pipelines::new(&gpu.device).expect("the pipelines");
    pipes.hold_builds();
    let (t, records) = frame(&gpu);
    let at = Instant::now();
    assert_eq!(draw(&gpu, &mut pipes, &t, &records), Drawn::Building);
    assert!(at.elapsed() < Duration::from_secs(1), "the draw waited");
    let got = gpu.read(&t).expect("the frame read back");
    assert!(got.iter().all(|&p| p == BLACK), "drawn while building");
    assert!(pipes.not_ready.0 >= 1, "the skip is counted");
    assert!(
        !woken(&pipes, Duration::from_millis(100)),
        "woken while held"
    );
    pipes.release_builds();
    gpu.drop_target(t);
    pipes.destroy();
}

/// **A pipeline that finishes while nothing else is dirty still gets its
/// frame**: the gate opened, the wake descriptor turns readable with no draw
/// asked, `finished` says a pipeline is ready, and the next draw lights the
/// fire's band. Named break: a build the loop only sees at its next
/// `ready` -- no wake, and an idle window never draws the effect.
#[test]
fn a_finished_pipeline_gets_its_frame_when_nothing_else_is_dirty() {
    let gpu = lavapipe();
    let mut pipes = Pipelines::new(&gpu.device).expect("the pipelines");
    pipes.hold_builds();
    let (t, records) = frame(&gpu);
    assert_eq!(draw(&gpu, &mut pipes, &t, &records), Drawn::Building);
    pipes.release_builds();
    assert!(
        woken(&pipes, Duration::from_secs(10)),
        "the finished build woke nothing"
    );
    assert_eq!(pipes.finished(), Built::Yes);
    assert_eq!(draw(&gpu, &mut pipes, &t, &records), Drawn::Yes);
    let got = gpu.read(&t).expect("the frame read back");
    let lit = got.iter().filter(|&&p| p != BLACK).count();
    assert!(lit > 0, "the effect's frame drew nothing");
    gpu.drop_target(t);
    pipes.destroy();
}
