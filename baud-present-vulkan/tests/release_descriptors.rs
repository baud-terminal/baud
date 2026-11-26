//! **A wake at a release point is closed when it is used**, counted in a
//! process of its own: the test binary holds this one test, so no other
//! test opens or closes a descriptor while the count is taken.

#![cfg(target_os = "linux")]

mod common;
mod scenes;

use baud_core::{Owned, Term};
use baud_domain::{Rows, WinSize};
use baud_font::Face;
use baud_present_vulkan::cells::Landing;
use baud_present_vulkan::damage::TargetIx;
use baud_present_vulkan::frame::Gpu;
use baud_present_vulkan::present::{Cpu, Frame, Inputs, Owes, Presenter, Remade};
use baud_present_vulkan::sys::{Readable, readable};
use baud_present_wayland::DmabufShown;
use baud_present_wayland::Px;
use baud_present_wayland::dmabuf::Point;
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
};

/// The frame `t` makes through `r`, landed.
fn frame(gpu: &Gpu, p: &mut Presenter, r: &mut Renderer<'_, '_>, t: &mut Term<'_>) -> Frame {
    let seal = t.seal_writes();
    let f = p.frame(gpu, r, t, seal, &NONE).expect("a frame");
    let until = Instant::now() + Duration::from_secs(60);
    while p.land(gpu).expect("landed") == Landing::Flying {
        assert!(Instant::now() < until, "the frame in flight never landed");
        std::thread::sleep(Duration::from_millis(1));
    }
    f
}

/// Target `k`'s frame shown with its points: the compositor's until its
/// release point, which is returned.
fn shown(p: &mut Presenter, k: TargetIx) -> Point {
    let (_, release) = p.points(k).expect("explicit points");
    p.shown(k, DmabufShown::Yes);
    release
}

/// **A thousand frames owed to a release point and woken leave the
/// process's descriptors as they were**, on RADV with explicit sync: both
/// targets held to their release points, each frame is owed with a wake,
/// the compositor's release of one target turns the wake readable, and the
/// frame drawn into that target closes it, as does the node the wake was
/// asked on. Named break: a wake or a render node kept past its use, one
/// descriptor an owed frame.
#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn a_thousand_wakes_at_a_release_point_leave_the_descriptors_as_they_were_on_radv() {
    let gpu = radv_explicit();
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
    // The pipelines built: the first frame drawn on the GPU arm, refused
    // so the next is drawn into the first target again.
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        match frame(&gpu, &mut p, &mut r, &mut t) {
            Frame::Cpu(Cpu::Building) => {
                assert!(Instant::now() < until, "the cell pipelines never built");
                std::thread::sleep(Duration::from_millis(10));
            }
            Frame::Drew(k) => {
                p.shown(k, DmabufShown::No);
                break;
            }
            f => panic!("a frame while the pipelines build: {f:?}"),
        }
    }
    // Both targets the compositor's until their release points.
    let mut release = [Point(0); 2];
    for _ in 0..2 {
        feed(&mut t, b"x");
        let Frame::Drew(at) = frame(&gpu, &mut p, &mut r, &mut t) else {
            panic!("a target free");
        };
        release[at.0] = shown(&mut p, at);
    }
    let open = || std::fs::read_dir("/proc/self/fd").expect("the fds").count();
    let before = open();
    let mut woken = 0;
    for i in 0..1000 {
        feed(&mut t, b"y");
        assert_eq!(frame(&gpu, &mut p, &mut r, &mut t), Frame::Owed);
        let wake = p.release_fd().expect("a wake for the owed frame");
        assert_eq!(readable(wake), Readable::No, "no release point reached");
        // Each target released in turn, the owed frame drawn into it.
        let k = TargetIx(i % 2);
        let (_, tl) = p.target(k).expect("a target");
        gpu.signal(tl.expect("a timeline"), release[k.0])
            .expect("the compositor's release");
        let until = Instant::now() + Duration::from_secs(60);
        while p.release_fd().map(readable) != Some(Readable::Yes) {
            assert!(Instant::now() < until, "the wake never turned readable");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(p.release_reached(), Owes::Frame, "the owed frame");
        assert_eq!(frame(&gpu, &mut p, &mut r, &mut t), Frame::Drew(k));
        release[k.0] = shown(&mut p, k);
        woken += 1;
    }
    assert_eq!(woken, 1000, "every owed frame woken and drawn");
    assert_eq!(p.counts().unwoken.0, 0, "the kernel took every wake");
    assert_eq!(open(), before, "descriptors before and after");
    p.destroy(&gpu);
}
