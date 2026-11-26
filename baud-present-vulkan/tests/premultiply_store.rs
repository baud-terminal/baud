//! **A background is stored premultiplied as the CPU premultiplies it**:
//! no blend touches a background's pixel, so each channel holds
//! [`baud_domain::premultiply_chan`]'s byte, the nearest to `c a / 255`,
//! on every device. `bg_fragment` is swept over every channel and alpha,
//! and a page whose blue lies a hundredth of a step above a half is drawn
//! by both paths' background stages.

#![cfg(target_os = "linux")]

mod common;
mod scenes;

use baud_core::{Owned, Term};
use baud_domain::{Chan, WinSize, premultiply};
use baud_font::Face;
use baud_present_vulkan::draw::{Pass, Records, Scene, Texel, Texels};
use baud_present_vulkan::frame::{Gpu, Premultiplied};
use baud_present_vulkan::pipeline::Pipelines;
use baud_present_wayland::Px;
use baud_render::{Argb, Stack, Theme};
use common::{lavapipe, radv};
use scenes::{Arms, feed, jetbrains_mono};

/// Every byte: a channel's levels and an alpha's.
const LEVELS: u32 = 256;

/// The straight page: blue 200 at alpha 146, whose product, 114.51, lies a
/// hundredth of a step above a half.
const PAGE: u32 = 0x9200_00C8;

const WS: WinSize = WinSize {
    ws_col: 12,
    ws_row: 3,
};

/// **The pixels of `got` that are not `want`**, and the first of them.
fn misses(got: &[u32], want: impl Fn(usize) -> u32) -> (usize, Option<(usize, u32, u32)>) {
    let mut first = None;
    let mut n = 0;
    for (i, &px) in got.iter().enumerate() {
        if px != want(i) {
            n += 1;
            first.get_or_insert((i, px, want(i)));
        }
    }
    (n, first)
}

/// Pair `i`'s straight word: every channel the column `i % 256`, the alpha
/// the row `i / 256`.
fn straight(i: u32) -> u32 {
    ((i / LEVELS) << 24) | ((i % LEVELS) * 0x01_0101)
}

/// **Every straight word `C C C` at alpha `a` drawn by `bg_fragment`**, a
/// cell a pixel, `C` the column and `a` the row, read back.
fn swept(gpu: &Gpu) -> Vec<u32> {
    let mut pipes = Pipelines::new(gpu).expect("the pipelines");
    let sheets = common::sheets(
        gpu,
        Texels {
            width: Px(1),
            height: Px(1),
            texel: Texel::Coverage,
            bytes: &[0],
        },
        Texels {
            width: Px(1),
            height: Px(1),
            texel: Texel::Argb,
            bytes: &[0; 4],
        },
        &[0; 256],
    );
    let bg: Vec<u8> = (0..LEVELS * LEVELS)
        .flat_map(|i| straight(i).to_le_bytes())
        .collect();
    let t = gpu.target(Px(LEVELS), Px(LEVELS)).expect("a target");
    gpu.clear(&t, Premultiplied(0), None).expect("cleared");
    let n = LEVELS as f32;
    let scene = Scene {
        u: [n, n, 1.0, 1.0, n, n, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0],
        backgrounds: Texels {
            width: Px(LEVELS),
            height: Px(LEVELS),
            texel: Texel::Argb,
            bytes: &bg,
        },
        sheets: &sheets,
        records: &[],
        count: Records(0),
        framing: None,
    };
    gpu.draw(&mut pipes, &t, &scene, Pass::Whole)
        .expect("drawn");
    let got = gpu.read(&t).expect("read back");
    gpu.drop_target(t);
    sheets.destroy(gpu);
    pipes.destroy();
    got
}

fn every_channel_at_every_alpha(gpu: &Gpu) {
    let got = swept(gpu);
    assert_eq!(
        u32::try_from(got.len()).ok(),
        Some(LEVELS * LEVELS),
        "a pixel a pair"
    );
    let want = |i: usize| {
        let i = u32::try_from(i).expect("a pair");
        premultiply(straight(i))
    };
    let (n, first) = misses(&got, want);
    assert_eq!(
        (n, first),
        (0, None),
        "(pixel, got, premultiply) of the first"
    );
}

/// **A blank screen on [`PAGE`], its cursor hidden**, drawn by the cell
/// path (`cell_bg_vertex`) and the instance path (`bg_fragment`): every
/// pixel of each is the page premultiplied.
fn the_page(gpu: &Gpu) {
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("the face")];
    let theme = Theme {
        bg: Argb(0xFF00_0000 | PAGE),
        bg_alpha: Chan::from_u8(u8::try_from(PAGE >> 24).expect("an alpha")),
        ..Theme::dark()
    };
    let r = scenes::renderer(Stack::new(&faces).expect("a stack"), theme, WS);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut arms = Arms::new(gpu, gpu.arm(1), r, &t);
    feed(&mut t, b"\x1bc\x1b[?25l");
    let frames = arms.draw(&mut t);
    let want = premultiply(PAGE);
    assert_eq!(want, 0x9200_0073, "the premise: the CPU's blue is 115");
    for (path, got) in [("cell", &frames.cell), ("instance", &frames.instance)] {
        assert!(!got.is_empty(), "the premise: the {path} frame is drawn");
        let (n, first) = misses(got, |_| want);
        assert_eq!(
            (n, first),
            (0, None),
            "the {path} frame, (pixel, got, premultiply) of the first"
        );
    }
}

#[test]
fn bg_fragment_stores_every_channel_at_every_alpha_as_the_cpu() {
    every_channel_at_every_alpha(&lavapipe());
}

#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn bg_fragment_stores_every_channel_at_every_alpha_as_the_cpu_on_radv() {
    every_channel_at_every_alpha(&radv());
}

#[test]
fn a_page_at_a_tie_is_stored_as_the_cpu_on_both_paths() {
    the_page(&lavapipe());
}

#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn a_page_at_a_tie_is_stored_as_the_cpu_on_both_paths_on_radv() {
    the_page(&radv());
}
