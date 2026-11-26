//! **A frame's end is closed when it lands**, counted in a process of its
//! own: the test binary holds this one test, so no other test opens or
//! closes a descriptor while the count is taken.

#![cfg(target_os = "linux")]

mod common;
mod scenes;

use baud_core::{Owned, Term};
use baud_domain::{Rows, WinSize};
use baud_font::Face;
use baud_present_vulkan::cells::Landing;
use baud_present_vulkan::frame::Gpu;
use baud_present_vulkan::present::{Cpu, Frame, Inputs, Presenter, Remade};
use baud_present_vulkan::vk::Has;
use baud_present_wayland::{DmabufShown, Px};
use baud_render::{Renderer, Stack, Theme};
use common::radv_explicit;
use scenes::{feed, jetbrains_mono};
use std::time::{Duration, Instant};

const WS: WinSize = WinSize {
    ws_col: 24,
    ws_row: 6,
};

const NONE: Inputs<'static> = Inputs {
    back: Rows::ZERO,
    highlight: None,
    composition: &None,
    predictions: &[],
};

/// The frame `t` makes through `r`, landed, its target given back.
fn frame(gpu: &Gpu, p: &mut Presenter, r: &mut Renderer<'_, '_>, t: &mut Term<'_>) -> Frame {
    let seal = t.seal_writes();
    let f = p.frame(gpu, r, t, seal, &NONE).expect("a frame");
    let until = Instant::now() + Duration::from_secs(60);
    while p.land(gpu).expect("landed") == Landing::Flying {
        assert!(Instant::now() < until, "the frame in flight never landed");
        std::thread::sleep(Duration::from_millis(1));
    }
    if let Frame::Drew(k) = f {
        p.shown(k, DmabufShown::No);
    }
    f
}

/// **A thousand frames leave the process's descriptors as they were**, on
/// RADV with explicit sync: each frame's end, a sync file, is closed when
/// the frame lands. Named break: an end kept past its landing, one
/// descriptor a frame.
#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn a_thousand_frames_leave_the_descriptors_as_they_were_on_radv() {
    let gpu = radv_explicit();
    assert_eq!(
        gpu.sync_fd,
        Has::Yes,
        "the premise: a frame's end is a sync file"
    );
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("the face")];
    let mut r = scenes::renderer(Stack::new(&faces).expect("a stack"), Theme::dark(), WS);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut p = Presenter::new(&gpu, &t).expect("the arm");
    let (w, h) = r.whole_size();
    assert_eq!(
        p.fit(&gpu, (Px(w.value()), Px(h.value())))
            .expect("targets"),
        Remade::Yes
    );
    // The pipelines built: the first frame drawn on the GPU arm.
    let until = Instant::now() + Duration::from_secs(60);
    while frame(&gpu, &mut p, &mut r, &mut t) == Frame::Cpu(Cpu::Building) {
        assert!(Instant::now() < until, "the cell pipelines never built");
        std::thread::sleep(Duration::from_millis(10));
    }
    let open = || std::fs::read_dir("/proc/self/fd").expect("the fds").count();
    let before = open();
    let mut drawn = 0;
    for _ in 0..1000 {
        feed(&mut t, b"x");
        if let Frame::Drew(_) = frame(&gpu, &mut p, &mut r, &mut t) {
            drawn += 1;
        }
    }
    assert_eq!(drawn, 1000, "every frame drawn");
    assert_eq!(open(), before, "descriptors before and after");
    p.destroy(&gpu);
}
