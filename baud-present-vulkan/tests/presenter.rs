//! **The Linux driver's GPU arm** (`present.rs`) on lavapipe: which frames
//! it draws, which it hands the CPU arm and which it owes, and what each
//! target is drawn with after a refusal, a CPU frame and a resize.

#![cfg(target_os = "linux")]

mod common;
mod scenes;

use baud_core::{Owned, Term};
use baud_domain::{Rows, WinSize};
use baud_font::Face;
use baud_present_vulkan::cells::Landing;
use baud_present_vulkan::damage::TargetIx;
use baud_present_vulkan::frame::Gpu;
use baud_present_vulkan::hooks::Fenced;
use baud_present_vulkan::pipeline::Built;
use baud_present_vulkan::present::{Cpu, Flies, Frame, Inputs, Owes, Presenter, Remade, Why};
use baud_present_vulkan::sys::{Readable, SyncFiles, put_write_fence, readable, takes_sync_files};
use baud_present_vulkan::vk::Has;
use baud_present_wayland::{Changed, DmabufShown, Px};
use baud_render::{Renderer, Stack, Theme};
use common::{lavapipe, radv, radv_explicit};
use scenes::{feed, jetbrains_mono};
use std::io::Read;
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

const WS: WinSize = WinSize {
    ws_col: 24,
    ws_row: 6,
};

fn renderer<'f, 'a>(faces: &'f [Face<'a>]) -> Renderer<'f, 'a> {
    scenes::renderer(Stack::new(faces).expect("a stack"), Theme::dark(), WS)
}

const NONE: Inputs<'static> = Inputs {
    back: Rows::ZERO,
    highlight: None,
    composition: &None,
    predictions: &[],
};

/// The frame `term` makes through `r`, sealed as the driver seals it, and
/// landed: what a test reads of it -- its pixels, its times, its acquire
/// point -- is the finished frame's.
fn frame(gpu: &Gpu, p: &mut Presenter, r: &mut Renderer<'_, '_>, t: &mut Term<'_>) -> Frame {
    let seal = t.seal_writes();
    let f = p.frame(gpu, r, t, seal, &NONE).expect("a frame");
    land(gpu, p);
    f
}

/// **The frame in flight met**, as the loop meets it: once its end is
/// readable, within a bound a held queue never reaches.
fn land(gpu: &Gpu, p: &mut Presenter) {
    let until = Instant::now() + Duration::from_secs(60);
    while p.land(gpu).expect("landed") == Landing::Flying {
        assert!(Instant::now() < until, "the frame in flight never landed");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// The whole frame's size, the targets' size.
fn size(r: &Renderer<'_, '_>) -> (Px, Px) {
    let (w, h) = r.whole_size();
    (Px(w.value()), Px(h.value()))
}

/// A presenter whose pipelines are built -- one frame asked, which starts
/// the builds, then waited for -- and both targets drawn once and refused,
/// so the next frame is drawn into the first.
fn warm(gpu: &Gpu, r: &mut Renderer<'_, '_>, t: &mut Term<'_>) -> Presenter {
    let mut p = Presenter::new(gpu, t).expect("the arm");
    assert_eq!(p.fit(gpu, size(r)).expect("targets"), Remade::Yes);
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        match frame(gpu, &mut p, r, t) {
            Frame::Cpu(Cpu::Building) => {
                assert!(Instant::now() < until, "the cell pipelines never built");
                std::thread::sleep(Duration::from_millis(10));
            }
            Frame::Drew(k) => {
                p.shown(k, DmabufShown::No);
                // The second target too, so the next frame is the first's.
                assert_eq!(frame(gpu, &mut p, r, t), Frame::Drew(TargetIx(1)));
                p.shown(TargetIx(1), DmabufShown::No);
                return p;
            }
            other => panic!("warming: {other:?}"),
        }
    }
}

/// Whether `p`'s wake descriptor becomes readable within `limit`.
fn woken(p: &Presenter, limit: Duration) -> bool {
    let fd = p.ready_fd().expect("a wake descriptor");
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

/// **A building cell pipeline hands the frame to the CPU arm, and its
/// build wakes the loop**: with the builds held, the frame returns at once
/// as the CPU arm's, counted, and the wake descriptor stays quiet; released,
/// it turns readable with nothing drawn, `finished` says so, and the next
/// frame is the GPU arm's. Named break: a frame that builds what it lacks
/// (`Pipelines::get`) never returns while the build is held.
#[test]
fn a_building_cell_pipeline_falls_back_to_the_cpu_arm_and_wakes_the_loop() {
    let gpu = lavapipe();
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("the face")];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"hello");
    let mut p = Presenter::new(&gpu, &t).expect("the arm");
    p.fit(&gpu, size(&r)).expect("targets");
    p.pipes().hold_builds();
    let at = Instant::now();
    assert_eq!(
        frame(&gpu, &mut p, &mut r, &mut t),
        Frame::Cpu(Cpu::Building)
    );
    assert!(at.elapsed() < Duration::from_secs(1), "the frame waited");
    assert_eq!(p.counts().building.0, 1, "counted");
    assert!(!woken(&p, Duration::from_millis(100)), "woken while held");
    p.pipes().release_builds();
    let until = Instant::now() + Duration::from_secs(60);
    let mut drew = None;
    while drew.is_none() {
        assert!(Instant::now() < until, "the cell pipelines never built");
        assert!(
            woken(&p, Duration::from_secs(30)),
            "a finished build woke nothing"
        );
        if p.finished() == Built::Yes {
            if let Frame::Drew(k) = frame(&gpu, &mut p, &mut r, &mut t) {
                drew = Some(k);
            }
        }
    }
    assert_eq!(drew, Some(TargetIx(0)), "the GPU arm's, into the first");
    p.destroy(&gpu);
}

/// **A frame with no free target is owed until a release**: two frames
/// shown without points hold both targets; the third draws nothing, is
/// counted, and leaves its rows owed; the first target released, the next
/// frame is drawn into it and names the third's change with the rest.
/// Named break: a frame drawn into a target the compositor still reads,
/// or handed to the CPU arm.
#[test]
fn a_frame_with_no_free_target_waits_for_a_release() {
    let gpu = lavapipe();
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("the face")];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut p = warm(&gpu, &mut r, &mut t);
    for (k, text) in [b"one\r\n".as_slice(), b"two\r\n"].into_iter().enumerate() {
        feed(&mut t, text);
        let f = frame(&gpu, &mut p, &mut r, &mut t);
        assert_eq!(f, Frame::Drew(TargetIx(k)), "frame {k}");
        p.shown(TargetIx(k), DmabufShown::Yes);
    }
    feed(&mut t, b"three\r\n");
    assert_eq!(frame(&gpu, &mut p, &mut r, &mut t), Frame::Owed);
    assert_eq!(p.counts().owed.0, 1, "counted");
    assert_eq!(p.counts().drew.0, 4, "the two warm frames and two");
    p.released(TargetIx(0));
    assert_eq!(
        frame(&gpu, &mut p, &mut r, &mut t),
        Frame::Drew(TargetIx(0)),
        "drawn into the one released"
    );
    p.destroy(&gpu);
}

/// **A presenter on a settled screen**: written, then the cursor at row 2
/// and both targets holding the frame, each drawn and released.
fn settled(gpu: &Gpu, r: &mut Renderer<'_, '_>, t: &mut Term<'_>) -> Presenter {
    let mut p = warm(gpu, r, t);
    for b in [b"x".as_slice(), b"x", b"\x1b[2;1H", b"", b""] {
        feed(t, b);
        let Frame::Drew(k) = frame(gpu, &mut p, r, t) else {
            panic!("settling: drawn")
        };
        p.shown(k, DmabufShown::Yes);
        p.released(k);
    }
    p
}

/// The pixel rows of the target grid row `row` covers.
fn band(r: &Renderer<'_, '_>, row: u32) -> (u32, u32) {
    let (top, h) = (r.grid_top().value(), r.metrics().cell_h.value());
    (top + row * h, top + (row + 1) * h)
}

/// Whether the frame drawn last names the pixel rows `rows`.
fn names(p: &Presenter, rows: (u32, u32)) -> bool {
    match p.damage() {
        Changed::Whole => true,
        Changed::Strips(s) => s.iter().any(|s| s.top.0 <= rows.0 && rows.1 <= s.bottom.0),
    }
}

/// **A refused frame's rows are drawn again**: on a settled screen the
/// cursor moves from row 2 to row 4 in a frame the compositor does not
/// take, then to row 5 in a frame into the other target; the next frame
/// into the first, which changes nothing, names row 2 -- which only the
/// refused frame drew -- and the whole is not drawn. Named break: the
/// dmabuf path's dropped `owed`, which leaves row 2's cursor on screen.
#[test]
fn a_refused_frame_keeps_its_rows() {
    let gpu = lavapipe();
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("the face")];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut p = settled(&gpu, &mut r, &mut t);
    feed(&mut t, b"\x1b[4;1H");
    let Frame::Drew(refused) = frame(&gpu, &mut p, &mut r, &mut t) else {
        panic!("drawn")
    };
    assert!(names(&p, band(&r, 1)), "the premise: the move drew row 2");
    p.shown(refused, DmabufShown::No);
    feed(&mut t, b"\x1b[5;1H");
    let Frame::Drew(other) = frame(&gpu, &mut p, &mut r, &mut t) else {
        panic!("drawn")
    };
    assert_ne!(other, refused, "the other target");
    p.shown(other, DmabufShown::Yes);
    p.released(other);
    assert_eq!(
        frame(&gpu, &mut p, &mut r, &mut t),
        Frame::Drew(refused),
        "back into the refused one"
    );
    assert!(
        matches!(p.damage(), Changed::Strips(_)),
        "rows, not the whole"
    );
    assert!(names(&p, band(&r, 1)), "row 2 is owed again");
    p.destroy(&gpu);
}

/// **A GPU frame after the CPU arm draws whole**: on a settled screen a
/// frame that changes nothing draws rows, not the whole; after a frame on
/// the CPU arm, the next frame into each target is the whole. Named break:
/// rows drawn to shared memory never reach a dmabuf.
#[test]
fn a_gpu_frame_after_the_cpu_arm_draws_what_it_missed() {
    let gpu = lavapipe();
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("the face")];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut p = settled(&gpu, &mut r, &mut t);
    let Frame::Drew(k) = frame(&gpu, &mut p, &mut r, &mut t) else {
        panic!("drawn")
    };
    assert!(
        matches!(p.damage(), Changed::Strips(_)),
        "the control: a settled frame is not the whole"
    );
    p.shown(k, DmabufShown::Yes);
    p.released(k);
    p.drawn_elsewhere();
    for n in 0..2 {
        let Frame::Drew(k) = frame(&gpu, &mut p, &mut r, &mut t) else {
            panic!("drawn")
        };
        assert!(
            matches!(p.damage(), Changed::Whole),
            "frame {n} after the CPU arm"
        );
        p.shown(k, DmabufShown::Yes);
        p.released(k);
    }
    p.destroy(&gpu);
}

/// **An image on screen takes the CPU arm, counted, as the cell path does
/// not draw it; a smear is the GPU arm's once the quad's stage is built**:
/// its first frame the CPU arm's while the stage builds, never waited for,
/// then drawn, and no frame counted as a trail the path cannot draw. The
/// control: the screen without either is the GPU arm's.
#[test]
fn an_image_takes_the_cpu_arm_and_a_smear_the_gpu_arm() {
    let gpu = lavapipe();
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("the face")];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS).with_images(baud_domain::ImageBudget::of(1 << 16));
    let mut t = Term::new(owned.storage(), &WS);
    let mut p = warm(&gpu, &mut r, &mut t);
    let (w, h) = (
        r.metrics().cell_w.value() as f32,
        r.metrics().cell_h.value() as f32,
    );
    let rect = |c: f32, row: f32| baud_render::Rect {
        left: c * w,
        top: row * h,
        right: (c + 1.0) * w,
        bottom: (row + 1.0) * h,
    };
    r.set_trail(Some(baud_render::TrailLook::Smear(baud_render::Smear {
        corners: rect(10.0, 1.0).corners(),
        hole: rect(1.0, 1.0),
        shape: baud_render::Shape::Quad,
        strength: baud_render::Strength::Full,
        opacity: baud_render::Smear::OPAQUE,
    })));
    assert_eq!(
        frame(&gpu, &mut p, &mut r, &mut t),
        Frame::Cpu(Cpu::Building),
        "the quad's stage building"
    );
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        assert!(Instant::now() < until, "the quad's stage never built");
        if woken(&p, Duration::from_secs(30)) && p.finished() == Built::Yes {
            if let Frame::Drew(_) = frame(&gpu, &mut p, &mut r, &mut t) {
                break;
            }
        }
    }
    assert_eq!(p.counts().trail.0, 0, "no smear frame for the CPU arm");
    r.set_trail(None);
    assert!(
        matches!(frame(&gpu, &mut p, &mut r, &mut t), Frame::Drew(_)),
        "the control"
    );
    // A sixel of one red pixel-band, placed at the cursor.
    feed(&mut t, b"\x1bPq#0;2;100;0;0#0~~~~\x1b\\");
    assert_eq!(frame(&gpu, &mut p, &mut r, &mut t), Frame::Cpu(Cpu::Images));
    assert_eq!(p.counts().images.0, 1);
    p.destroy(&gpu);
}

/// **A new size makes both targets anew**, each drawn whole first; the
/// same size keeps them. Named break: a frame drawn into a target of the
/// old size, which `present_dmabuf` refuses as `WrongSize` for ever.
#[test]
fn a_resize_remakes_both_targets() {
    let gpu = lavapipe();
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("the face")];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut p = warm(&gpu, &mut r, &mut t);
    let (w, h) = size(&r);
    assert_eq!(p.fit(&gpu, (w, h)).expect("kept"), Remade::No);
    let bigger = (Px(w.0 + 16), Px(h.0 + 8));
    assert_eq!(p.fit(&gpu, bigger).expect("made"), Remade::Yes);
    for k in 0..2 {
        let (tg, _) = p.target(TargetIx(k)).expect("a target");
        assert_eq!((tg.width, tg.height), bigger, "target {k}");
    }
    p.destroy(&gpu);
}

/// **A resize waits no more than a steady frame**: remaking both targets
/// waits on no fence, and the first frame into the new target, drawn
/// whole, waits as many times as a frame into a warm target, on targets
/// that refuse a sync file so each frame is waited for. Named break: a
/// remade target cleared and waited for before its first frame, two waits
/// more on every resize.
#[test]
fn a_resize_waits_no_more_than_a_steady_frame() {
    let gpu = lavapipe();
    gpu.refuse_sync_files();
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("the face")];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut p = warm(&gpu, &mut r, &mut t);
    let waits = |p: &mut Presenter, r: &mut Renderer<'_, '_>, t: &mut Term<'_>| {
        let before = gpu.waits().0;
        let Frame::Drew(k) = frame(&gpu, p, r, t) else {
            panic!("drawn")
        };
        p.shown(k, DmabufShown::No);
        gpu.waits().0 - before
    };
    feed(&mut t, b"x");
    let steady = waits(&mut p, &mut r, &mut t);
    assert!(steady > 0, "a frame on lavapipe waits for its fence");
    let (w, h) = size(&r);
    let before = gpu.waits();
    let bigger = (Px(w.0 + 16), Px(h.0 + 8));
    assert_eq!(p.fit(&gpu, bigger).expect("made"), Remade::Yes);
    assert_eq!(gpu.waits(), before, "waits across the remake");
    feed(&mut t, b"x");
    assert_eq!(
        waits(&mut p, &mut r, &mut t),
        steady,
        "the first frame's waits"
    );
    p.destroy(&gpu);
}

/// **A target shown with points is the compositor's until its release
/// point**, on RADV with explicit sync: its frame's acquire point is
/// signalled, both targets shown hold the third frame owed, and the first
/// target's release point reached -- as the compositor signals it -- frees
/// it for the next. Named break: a target drawn into while the compositor
/// may still read it.
#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn a_target_shown_with_points_is_free_at_its_release_point_on_radv() {
    let gpu = radv_explicit();
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("the face")];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut p = warm(&gpu, &mut r, &mut t);
    let mut released = Vec::new();
    for k in 0..2 {
        feed(&mut t, b"x");
        assert_eq!(
            frame(&gpu, &mut p, &mut r, &mut t),
            Frame::Drew(TargetIx(k))
        );
        let (acquire, release) = p.points(TargetIx(k)).expect("explicit points");
        let (_, tl) = p.target(TargetIx(k)).expect("a target");
        let tl = tl.expect("a timeline");
        assert_eq!(
            gpu.reached(tl),
            Ok(acquire),
            "target {k}: acquire signalled"
        );
        assert!(release > acquire, "target {k}: release after acquire");
        p.shown(TargetIx(k), DmabufShown::Yes);
        released.push(release);
    }
    feed(&mut t, b"y");
    assert_eq!(frame(&gpu, &mut p, &mut r, &mut t), Frame::Owed);
    let (_, tl) = p.target(TargetIx(0)).expect("a target");
    gpu.signal(tl.expect("a timeline"), released[0])
        .expect("the compositor's release");
    assert_eq!(
        frame(&gpu, &mut p, &mut r, &mut t),
        Frame::Drew(TargetIx(0)),
        "free at its release point"
    );
    p.destroy(&gpu);
}

/// **A target larger than the frame by an odd number of pixels holds the
/// frame on whole pixels**, the odd one right and bottom, as the Linux
/// driver's CPU arm places it (`origin.rs`): every pixel of the frame drawn
/// into a target 17 by 9 pixels larger is the frame drawn into one of its
/// own size, moved 8 right and 4 down. Named break: the frame centred on
/// half a pixel, every glyph resampled.
#[test]
fn a_frame_sits_on_whole_pixels_in_a_larger_target() {
    let gpu = lavapipe();
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("the face")];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"whole pixels\r\nonly");
    let mut p = warm(&gpu, &mut r, &mut t);
    let (w, h) = size(&r);
    let mut drawn = |p: &mut Presenter, at: (Px, Px)| {
        p.fit(&gpu, at).expect("targets");
        let Frame::Drew(k) = frame(&gpu, p, &mut r, &mut t) else {
            panic!("drawn")
        };
        p.shown(k, DmabufShown::No);
        let (tg, _) = p.target(k).expect("a target");
        gpu.read(tg).expect("read back")
    };
    let own = drawn(&mut p, (w, h));
    let big = (Px(w.0 + 17), Px(h.0 + 9));
    let moved = drawn(&mut p, big);
    let side = |p: Px| usize::try_from(p.0).expect("a side");
    let (fw, fh, bw) = (side(w), side(h), side(big.0));
    let apart = (0..fh)
        .flat_map(|y| (0..fw).map(move |x| (x, y)))
        .filter(|&(x, y)| moved[(y + 4) * bw + x + 8] != own[y * fw + x])
        .count();
    assert_eq!(apart, 0, "pixels apart from the frame moved (8, 4)");
    p.destroy(&gpu);
}

/// **The Linux driver draws no pan and no leaving row**: the frame's cell
/// uniform carries a pan of 0 and no slide rows, on a screen that scrolled
/// a line since the last frame. Named break: smooth scroll turned on by
/// accident, which is its own row.
#[test]
fn linux_draws_no_pan_and_no_leaving_row() {
    let gpu = lavapipe();
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("the face")];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut p = warm(&gpu, &mut r, &mut t);
    feed(&mut t, b"1\r\n2\r\n3\r\n4\r\n5\r\n6\r\n7\r\n");
    assert!(matches!(
        frame(&gpu, &mut p, &mut r, &mut t),
        Frame::Drew(_)
    ));
    let cu = p.uniforms();
    for name in ["pan", "slide_rows"] {
        assert_eq!(cu.get(name).map(|w| w.0), Some(0), "CU.{name}");
    }
    p.destroy(&gpu);
}

/// **An effect trail is drawn on the GPU arm once its pipeline is built**:
/// the first frame that lights the fire starts its build and is the CPU
/// arm's, counted as building, never waited for; the build wakes the loop,
/// and the next frame is the GPU arm's, the trail drawn in the cell pass.
/// Named break: an effect frame handed to the CPU arm for good.
#[test]
fn an_effect_trail_is_drawn_on_the_gpu_arm_once_built() {
    use baud_render::effect::{Effect, Ignition, Seed, Vfx};
    let gpu = lavapipe();
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("the face")];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut p = warm(&gpu, &mut r, &mut t);
    let (w, h) = (
        r.metrics().cell_w.value() as f32,
        r.metrics().cell_h.value() as f32,
    );
    let rect = |c: f32, row: f32| baud_render::Rect {
        left: c * w,
        top: row * h,
        right: (c + 1.0) * w,
        bottom: (row + 1.0) * h,
    };
    let (from, to) = (rect(1.0, 1.0), rect(20.0, 4.0));
    let tail = baud_render::Pt {
        x: (from.left + from.right) * 0.5,
        y: (from.top + from.bottom) * 0.5,
    };
    r.set_trail(Some(baud_render::TrailLook::Vfx(Vfx {
        effect: Effect::Fire,
        emitter: baud_render::Smear {
            corners: from.corners(),
            hole: to,
            shape: baud_render::Shape::Quad,
            strength: baud_render::Strength::Full,
            opacity: baud_render::Smear::OPAQUE,
        },
        ignition: Ignition::ignite(tail, to, Seed(3), h).at(0.1),
        axis: baud_render::Pt { x: 1.0, y: 0.0 },
    })));
    let at = Instant::now();
    assert_eq!(
        frame(&gpu, &mut p, &mut r, &mut t),
        Frame::Cpu(Cpu::Building)
    );
    assert!(at.elapsed() < Duration::from_secs(1), "the frame waited");
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        assert!(Instant::now() < until, "the fire's pipeline never built");
        if woken(&p, Duration::from_secs(30)) && p.finished() == Built::Yes {
            if let Frame::Drew(_) = frame(&gpu, &mut p, &mut r, &mut t) {
                break;
            }
        }
    }
    assert_eq!(p.counts().trail.0, 0, "no trail frame for the CPU arm");
    // What the trail's frame cost the GPU, read from its own queries: the
    // pass, the trail's draw within it, and its fragments where counted.
    let times = p.gpu_times();
    assert!(times.pass > Duration::ZERO, "the pass is timed: {times:?}");
    assert_eq!(times.effect_draws.0, 1, "one trail draw timed: {times:?}");
    assert!(
        times
            .effect
            .is_some_and(|e| e > Duration::ZERO && e <= times.pass),
        "the trail's draw is timed inside its pass: {times:?}"
    );
    if gpu.clock.statistics == baud_present_vulkan::vk::Has::Yes {
        assert!(
            times.effect_fragments.is_some_and(|f| f.0 > 0),
            "its fragments are counted: {times:?}"
        );
    }
    // A frame with no trail times no effect.
    r.set_trail(None);
    let Frame::Drew(_) = frame(&gpu, &mut p, &mut r, &mut t) else {
        panic!("drawn")
    };
    assert_eq!(p.gpu_times().effect, None, "no trail, no effect timed");
    p.destroy(&gpu);
}

/// A screen of 32 rows, each in two renditions of its own.
const BANDED: WinSize = WinSize {
    ws_col: 40,
    ws_row: 32,
};

/// Every row in a foreground and a background of its own, 64 renditions,
/// the screen filled top to bottom.
fn renditions_by_row() -> Vec<u8> {
    let mut out = b"\x1b[H".to_vec();
    for r in 0..usize::from(BANDED.ws_row) {
        out.extend(
            format!(
                "\x1b[{};1H\x1b[0;38;2;{};{};200mrow {r:02} \x1b[48;2;30;{};{}mon its own band",
                r + 1,
                r * 7,
                255 - r * 7,
                r * 5,
                100 + r
            )
            .as_bytes(),
        );
    }
    out
}

/// The provision of eight bands of four rows.
fn banded_provision() -> baud_core::Provision {
    baud_core::Provision::banded_in(
        &BANDED,
        baud_core::BandRows::of(Rows::of_index(4)),
        baud_core::BandSlots(1_024),
    )
    .expect("40x32 is admitted banded")
}

/// **What the GPU arm draws of `owned`** after [`renditions_by_row`]: a
/// frame drawn, then `then` fed and the frame after it read back.
fn banded_frame(gpu: &Gpu, faces: &[Face<'_>], owned: &mut Owned, then: &[u8]) -> Vec<u32> {
    let mut r = scenes::renderer(Stack::new(faces).expect("a stack"), Theme::dark(), BANDED);
    let mut t = Term::new(owned.storage(), &BANDED);
    feed(&mut t, &renditions_by_row());
    let mut p = warm(gpu, &mut r, &mut t);
    feed(&mut t, then);
    let Frame::Drew(k) = frame(gpu, &mut p, &mut r, &mut t) else {
        panic!("drawn")
    };
    p.shown(k, DmabufShown::No);
    let (tg, _) = p.target(k).expect("a target");
    let pixels = gpu.read(tg).expect("read back");
    p.destroy(gpu);
    pixels
}

/// **A banded screen draws on the GPU arm what one table draws**: eight
/// bands of four rows, each row resolving through its own band's table at
/// its draw row's `seg_base`, paint pixel for pixel what the same feed
/// paints through the single mode's one table. Named break: every base 0,
/// so the rows of bands 1-7 draw band 0's renditions.
#[test]
fn a_banded_screen_draws_on_the_gpu_arm_what_one_table_draws() {
    let gpu = lavapipe();
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("the face")];
    let want = banded_frame(&gpu, &faces, &mut Owned::new(&BANDED), b"");
    let mut banded = Owned::new(banded_provision());
    assert_eq!(banded.band_rows().rows().index(), 4, "bands of 4 rows");
    let got = banded_frame(&gpu, &faces, &mut banded, b"");
    assert!(
        want.iter().any(|&p| p != want[0]),
        "the control: a frame with something on it"
    );
    let differ = want.iter().zip(&got).filter(|(a, b)| a != b).count();
    assert_eq!(differ, 0, "pixels that differ from the single mode's");
}

/// **Bases that move are uploaded again**: after the first frames, five
/// lines scrolled in at the bottom bring a band into view and move every
/// base, and the frame after them still paints what one table paints.
/// Named break: the bases written once, at the set's first frame.
#[test]
fn a_band_scrolled_into_view_draws_on_the_gpu_arm_what_one_table_draws() {
    let gpu = lavapipe();
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("the face")];
    let scroll = b"\x1b[32;1H\r\n\x1b[38;2;250;20;20mone\r\ntwo\r\nthree\r\nfour\r\nfive";
    let want = banded_frame(&gpu, &faces, &mut Owned::new(&BANDED), scroll);
    let got = banded_frame(&gpu, &faces, &mut Owned::new(banded_provision()), scroll);
    let differ = want.iter().zip(&got).filter(|(a, b)| a != b).count();
    assert_eq!(differ, 0, "pixels that differ from the single mode's");
}

/// **[`a_band_scrolled_into_view_draws_on_the_gpu_arm_what_one_table_draws`]
/// on RADV**, whose sets may stage their memory: the bases that move are
/// owed to the device copy and given it.
#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn a_band_scrolled_into_view_draws_what_one_table_draws_on_radv() {
    let gpu = radv();
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("the face")];
    let scroll = b"\x1b[32;1H\r\n\x1b[38;2;250;20;20mone\r\ntwo\r\nthree\r\nfour\r\nfive";
    let want = banded_frame(&gpu, &faces, &mut Owned::new(&BANDED), scroll);
    let got = banded_frame(&gpu, &faces, &mut Owned::new(banded_provision()), scroll);
    let differ = want.iter().zip(&got).filter(|(a, b)| a != b).count();
    assert_eq!(differ, 0, "pixels that differ from the single mode's");
}

/// **The first frame a new presenter draws**, its pipelines built first:
/// the frames asked while they build are the CPU arm's.
fn first_drawn(gpu: &Gpu, r: &mut Renderer<'_, '_>, t: &mut Term<'_>) -> (Presenter, Vec<u32>) {
    let mut p = Presenter::new(gpu, t).expect("the arm");
    assert_eq!(p.fit(gpu, size(r)).expect("targets"), Remade::Yes);
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        match frame(gpu, &mut p, r, t) {
            Frame::Cpu(Cpu::Building) => {
                assert!(Instant::now() < until, "the cell pipelines never built");
                std::thread::sleep(Duration::from_millis(10));
            }
            Frame::Drew(k) => {
                p.shown(k, DmabufShown::No);
                let (tg, _) = p.target(k).expect("a target");
                let pixels = gpu.read(tg).expect("read back");
                return (p, pixels);
            }
            other => panic!("the first frame: {other:?}"),
        }
    }
}

/// **A lost device fails the frame, the arm comes down whole, and a new
/// one draws what the lost one drew**: a loss forced at a frame's
/// submission returns it, the presenter is destroyed with no target left,
/// and the first frame on a new device, through the renderer that drew
/// before, is pixel for pixel the frame the lost device drew of the same
/// screen. The oracle is that frame, not a second presenter in the same
/// process, so state carried from one presenter to the next cannot agree
/// with itself. Named break: a new presenter that keeps the old one's
/// style stamps, so the renderer takes its zeroed tables as resolved.
#[test]
fn a_lost_device_comes_down_whole_and_a_new_one_draws_what_it_drew() {
    use baud_present_vulkan::frame::Failed;
    use baud_present_vulkan::hooks::{Calls, Live};
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("the face")];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(
        &mut t,
        b"\x1b[1;31mred\x1b[0m before\r\n\x1b[44mblue ground\r\n\x1b[0mafter \x1b[32mgreen",
    );
    let lost = lavapipe();
    let mut p = warm(&lost, &mut r, &mut t);
    let Frame::Drew(k) = frame(&lost, &mut p, &mut r, &mut t) else {
        panic!("drawn")
    };
    p.shown(k, DmabufShown::No);
    let (tg, _) = p.target(k).expect("a target");
    let want = lost.read(tg).expect("read back");
    lost.fail_at(Calls(0), Failed::LOST);
    let seal = t.seal_writes();
    assert_eq!(p.frame(&lost, &mut r, &t, seal, &NONE), Err(Failed::LOST));
    p.destroy(&lost);
    assert_eq!(lost.targets_live(), Live(0), "no target left");
    drop(lost);
    let gpu = lavapipe();
    let (p, again) = first_drawn(&gpu, &mut r, &mut t);
    p.destroy(&gpu);
    assert!(
        want.iter().any(|&px| px != want[0]),
        "the control: a frame with something on it"
    );
    let differ = want.iter().zip(&again).filter(|(a, b)| a != b).count();
    assert_eq!(differ, 0, "pixels apart from the lost device's frame");
}

/// **Memory running out while the targets are made leaves none**: the
/// second target's allocation forced to fail fails the fit, the first is
/// dropped with it, and a fit after it makes both. Named break: the first
/// target kept, half a pair.
#[test]
fn memory_running_out_while_fitting_leaves_no_target() {
    use baud_present_vulkan::frame::Failed;
    use baud_present_vulkan::hooks::{Calls, Live};
    let gpu = lavapipe();
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("the face")];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut p = Presenter::new(&gpu, &t).expect("the arm");
    let (w, h) = size(&r);
    gpu.fail_at(Calls(1), Failed::OUT_OF_DEVICE_MEMORY);
    assert_eq!(p.fit(&gpu, (w, h)), Err(Failed::OUT_OF_DEVICE_MEMORY));
    assert_eq!(gpu.targets_live(), Live(0), "no target left");
    assert!(p.target(TargetIx(0)).is_none(), "the presenter holds none");
    assert_eq!(p.fit(&gpu, (w, h)).expect("targets"), Remade::Yes);
    assert_eq!(gpu.targets_live(), Live(2));
    let _ = frame(&gpu, &mut p, &mut r, &mut t);
    p.destroy(&gpu);
    assert_eq!(gpu.targets_live(), Live(0));
}

/// **A resize storm holds two targets**: two hundred sizes among four,
/// a frame drawn after every third, never leave more than the pair alive
/// or fail, and destroying the presenter leaves none.
#[test]
fn a_resize_storm_holds_two_targets_and_fails_nothing() {
    use baud_present_vulkan::hooks::Live;
    let gpu = lavapipe();
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("the face")];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, b"storm");
    let mut p = warm(&gpu, &mut r, &mut t);
    let (w, h) = size(&r);
    let sizes = [
        (w, h),
        (Px(w.0 + 3), h),
        (w, Px(h.0 + 5)),
        (Px(w.0 + 8), Px(h.0 + 2)),
    ];
    for i in 0..200 {
        p.fit(&gpu, sizes[i % sizes.len()]).expect("targets");
        assert_eq!(gpu.targets_live(), Live(2), "after fit {i}");
        if i % 3 == 0 {
            if let Frame::Drew(k) = frame(&gpu, &mut p, &mut r, &mut t) {
                p.shown(k, DmabufShown::No);
            }
        }
    }
    p.destroy(&gpu);
    assert_eq!(gpu.targets_live(), Live(0));
}

/// **A frame drawn with its landing held**, `p.frame` called directly: it
/// stays in flight until [`Gpu::release_landings`], however fast the device
/// draws it.
fn frame_held(gpu: &Gpu, p: &mut Presenter, r: &mut Renderer<'_, '_>, t: &mut Term<'_>) -> Frame {
    let seal = t.seal_writes();
    gpu.hold_landings();
    p.frame(gpu, r, t, seal, &NONE).expect("a frame")
}

/// **Until `fd` is readable**, within a bound.
fn until_readable(fd: impl Fn() -> Option<std::os::fd::OwnedFd>) {
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        let f = fd().expect("an end to wait for");
        if readable(f.as_fd()) == Readable::Yes {
            return;
        }
        assert!(Instant::now() < until, "the end never turned readable");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// **A frame asked for while the one before flies is busy and never
/// waits**, on RADV with explicit sync: a frame drawn with its landing held
/// returns in flight, its end a descriptor not yet readable; the next
/// answers busy, counted, drawing nothing; released, the end turns
/// readable, the frame lands, the busy frame is owed, and the next is
/// drawn. Named break: a frame that waits on its fence has landed by the
/// time it returns.
#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn a_frame_asked_while_one_flies_is_busy_and_never_waits_on_radv() {
    let gpu = radv_explicit();
    assert_eq!(
        gpu.sync_fd,
        Has::Yes,
        "the premise: a frame's end is a sync file"
    );
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("the face")];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut p = warm(&gpu, &mut r, &mut t);
    feed(&mut t, b"one");
    let f = frame_held(&gpu, &mut p, &mut r, &mut t);
    assert!(matches!(f, Frame::Drew(_)), "drawn: {f:?}");
    assert_eq!(p.land(&gpu), Ok(Landing::Flying), "returned in flight");
    let end = p.done_fd().expect("its end as a descriptor");
    assert_eq!(readable(end), Readable::No, "not finished while held");
    feed(&mut t, b"two");
    let seal = t.seal_writes();
    assert_eq!(p.frame(&gpu, &mut r, &t, seal, &NONE), Ok(Frame::Busy));
    assert_eq!(p.counts().busy.0, 1, "counted");
    gpu.release_landings();
    until_readable(|| {
        p.done_fd()
            .map(|fd| fd.try_clone_to_owned().expect("a copy"))
    });
    // The end turned readable; the fence it stands for is met next.
    land(&gpu, &mut p);
    assert_eq!(p.land(&gpu), Ok(Landing::Idle), "landed");
    assert_eq!(p.owed_on_landing(), Owes::Frame, "the busy frame is owed");
    assert!(matches!(
        frame(&gpu, &mut p, &mut r, &mut t),
        Frame::Drew(_)
    ));
    p.destroy(&gpu);
}

/// **The acquire point is the frame's own submission's to signal**, on
/// RADV with explicit sync: once the frame has landed, its acquire point is
/// reached before the driver takes the points to show it with. Named
/// break: the acquire point signalled from the CPU when the points are
/// taken, which the compositor can be told before the GPU has drawn.
#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn the_acquire_point_is_the_gpus_to_signal_on_radv() {
    let gpu = radv_explicit();
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("the face")];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut p = warm(&gpu, &mut r, &mut t);
    feed(&mut t, b"x");
    let Frame::Drew(k) = frame(&gpu, &mut p, &mut r, &mut t) else {
        panic!("drawn")
    };
    let (_, tl) = p.target(k).expect("a target");
    let reached = gpu.reached(tl.expect("a timeline")).expect("its value");
    let (acquire, _) = p.points(k).expect("explicit points");
    assert_eq!(
        reached, acquire,
        "reached by the GPU before the points were taken"
    );
    p.destroy(&gpu);
}

/// **A frame without points flies too**, on RADV with implicit sync: its
/// end is put on its target's dmabuf as the write fence the compositor's
/// reads wait on, the kernel taking one for each frame drawn, so a frame
/// drawn with its landing held returns in flight, its end a descriptor
/// not yet readable, and no frame was finished on the CPU. Named breaks:
/// a frame without points waited for before it is shown has landed by
/// the time it returns; a frame left in flight with no write fence on its
/// target, which the compositor would read before it is drawn.
#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn a_frame_without_points_flies_with_its_end_on_the_target_on_radv() {
    let gpu = radv();
    assert_eq!(
        gpu.choice.sync,
        baud_present_vulkan::choose::Sync::Implicit,
        "the premise: implicit sync"
    );
    assert_eq!(
        gpu.sync_fd,
        Has::Yes,
        "the premise: a frame's end is a sync file"
    );
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("the face")];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut p = warm(&gpu, &mut r, &mut t);
    assert_eq!(p.flies(), Flies::Yes, "the targets take a sync file");
    assert_eq!(gpu.fences_put(), Fenced(2), "the warm frames' ends");
    feed(&mut t, b"one");
    let f = frame_held(&gpu, &mut p, &mut r, &mut t);
    assert!(matches!(f, Frame::Drew(_)), "drawn: {f:?}");
    assert_eq!(gpu.fences_put(), Fenced(3), "its end on its target");
    assert_eq!(p.land(&gpu), Ok(Landing::Flying), "returned in flight");
    let end = p.done_fd().expect("its end as a descriptor");
    assert_eq!(readable(end), Readable::No, "not finished while held");
    gpu.release_landings();
    land(&gpu, &mut p);
    assert_eq!(p.counts().waited.0, 0, "no frame finished on the CPU");
    p.destroy(&gpu);
}

/// **A frame whose end exports as `-1` has landed by the time it returns,
/// and is not counted waited**, on RADV with explicit sync: the export
/// answering `-1`, as an implementation may for a semaphore already
/// signalled, and the frame drawn with its landing held, so its fence is
/// never found finished by asking; it has landed when it returns, with no
/// end for the loop to poll, counted signalled and not waited, and the
/// next frame is drawn rather than busy. Named break: a frame left in
/// flight with no descriptor to poll, so a frame asked meanwhile is owed
/// to a wake that never comes.
#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn a_frame_whose_end_exports_as_minus_one_has_landed_on_radv() {
    let gpu = radv_explicit();
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("the face")];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut p = warm(&gpu, &mut r, &mut t);
    assert_eq!(p.flies(), Flies::Yes, "the premise: frames fly");
    let waited = p.counts().waited.0;
    gpu.signal_exports();
    feed(&mut t, b"one");
    let f = frame_held(&gpu, &mut p, &mut r, &mut t);
    assert!(matches!(f, Frame::Drew(_)), "drawn: {f:?}");
    assert_eq!(p.land(&gpu), Ok(Landing::Idle), "landed before it returned");
    assert_eq!(p.done_fd().map(|_| ()), None, "no end to poll");
    assert_eq!(p.counts().waited.0, waited, "not waited for");
    assert_eq!(p.counts().signalled.0, 1, "met at its export");
    feed(&mut t, b"two");
    let seal = t.seal_writes();
    let next = p.frame(&gpu, &mut r, &t, seal, &NONE).expect("a frame");
    assert!(matches!(next, Frame::Drew(_)), "not busy: {next:?}");
    gpu.release_landings();
    p.destroy(&gpu);
}

/// **A target is put a write fence only by the kernel, which takes only a
/// sync file**, on lavapipe: the target's dmabuf takes sync files, the
/// control, and a descriptor that is not one is refused. Named break: a
/// write fence answered taken without the kernel asked.
#[test]
fn a_target_refuses_a_write_fence_that_is_not_a_sync_file() {
    let gpu = lavapipe();
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("the face")];
    let r = renderer(&faces);
    let mut owned = Owned::new(&WS);
    let t = Term::new(owned.storage(), &WS);
    let mut p = Presenter::new(&gpu, &t).expect("the arm");
    assert_eq!(p.fit(&gpu, size(&r)).expect("targets"), Remade::Yes);
    let (target, _) = p.target(TargetIx(0)).expect("a target");
    let dmabuf = target.plane().fd;
    assert_eq!(takes_sync_files(dmabuf), SyncFiles::Taken, "the control");
    let (not_one, _) = UnixStream::pair().expect("a pair");
    assert_eq!(
        put_write_fence(dmabuf, not_one.as_fd()),
        SyncFiles::Refused,
        "not a sync file"
    );
    p.destroy(&gpu);
}

/// **Targets that refuse a sync file keep every frame finished before it
/// is shown, and counted**, on lavapipe with the refusal forced: the
/// presenter says why its frames wait, a frame drawn with its landing held
/// has landed by the time it returns, and each frame drawn is counted.
/// Named break: a frame left in flight with nothing to make the
/// compositor's reads wait for it, or a wait no count shows.
#[test]
fn targets_that_refuse_a_sync_file_keep_frames_waited_for_and_counted() {
    let gpu = lavapipe();
    gpu.refuse_sync_files();
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("the face")];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut p = warm(&gpu, &mut r, &mut t);
    assert_eq!(p.flies(), Flies::Waits(Why::FenceRefused), "why they wait");
    assert_eq!(p.counts().waited.0, 2, "the two warm frames, counted");
    feed(&mut t, b"one");
    let f = frame_held(&gpu, &mut p, &mut r, &mut t);
    assert!(matches!(f, Frame::Drew(_)), "drawn: {f:?}");
    assert_eq!(
        p.land(&gpu),
        Ok(Landing::Idle),
        "finished before it returned"
    );
    assert_eq!(p.done_fd().map(|_| ()), None, "no end to poll");
    assert_eq!(p.counts().waited.0, 3, "counted");
    gpu.release_landings();
    p.destroy(&gpu);
}

/// **Frames on lavapipe never fly: a CPU device's export of a frame's end
/// waits for the frame, so each is finished before it is shown, and
/// counted**: the
/// premise, a device that hands out sync files and targets that take them,
/// so nothing else would keep a frame from flying; the presenter says why
/// its frames wait, the two warm frames are counted, and a frame drawn with
/// its landing held has landed by the time it returns, with no end to
/// poll. Named break: lavapipe's frames said to fly while each export
/// blocks for the frame, and no count showing the wait.
#[test]
fn lavapipe_s_frames_wait_for_its_sync_file_and_are_counted() {
    let gpu = lavapipe();
    assert_eq!(gpu.sync_fd, Has::Yes, "the premise: a sync file exports");
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("the face")];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut p = warm(&gpu, &mut r, &mut t);
    let (target, _) = p.target(TargetIx(0)).expect("a target");
    assert_eq!(
        takes_sync_files(target.plane().fd),
        SyncFiles::Taken,
        "the premise: the targets take a sync file"
    );
    assert_eq!(p.flies(), Flies::Waits(Why::ExportWaits), "why they wait");
    assert_eq!(p.counts().waited.0, 2, "the two warm frames, counted");
    feed(&mut t, b"one");
    let f = frame_held(&gpu, &mut p, &mut r, &mut t);
    assert!(matches!(f, Frame::Drew(_)), "drawn: {f:?}");
    assert_eq!(
        p.land(&gpu),
        Ok(Landing::Idle),
        "finished before it returned"
    );
    assert_eq!(p.done_fd().map(|_| ()), None, "no end to poll");
    assert_eq!(p.counts().waited.0, 3, "counted");
    gpu.release_landings();
    p.destroy(&gpu);
}

/// **A frame owed to a release point is woken when it is reached**, on
/// RADV with explicit sync: two frames shown with points hold both
/// targets, so the third is owed with a descriptor for the loop to poll,
/// not yet readable though each timeline stands at the point just before
/// its release point; the first target's release point signalled, as the
/// compositor signals it, turns it readable within a bound, the frame is
/// owed, and the next is drawn into that target. Named break: an owed
/// frame with nothing to wake the loop when the compositor lets a target
/// go.
#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn a_frame_owed_to_a_release_point_is_woken_when_it_is_reached_on_radv() {
    let gpu = radv_explicit();
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("the face")];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut p = warm(&gpu, &mut r, &mut t);
    let mut released = Vec::new();
    for k in 0..2 {
        feed(&mut t, b"x");
        assert_eq!(
            frame(&gpu, &mut p, &mut r, &mut t),
            Frame::Drew(TargetIx(k))
        );
        let (acquire, release) = p.points(TargetIx(k)).expect("explicit points");
        assert_eq!(acquire.0 + 1, release.0, "the release point is the next");
        p.shown(TargetIx(k), DmabufShown::Yes);
        released.push(release);
    }
    assert_eq!(
        p.release_fd().map(|_| ()),
        None,
        "no wake while one is free"
    );
    feed(&mut t, b"y");
    assert_eq!(frame(&gpu, &mut p, &mut r, &mut t), Frame::Owed);
    for (k, release) in released.iter().enumerate() {
        let (_, tl) = p.target(TargetIx(k)).expect("a target");
        let at = gpu.reached(tl.expect("a timeline")).expect("its value");
        assert_eq!(at.0 + 1, release.0, "the point before the release reached");
    }
    let wake = p.release_fd().expect("a wake for the owed frame");
    assert_eq!(readable(wake), Readable::No, "no release point reached");
    assert_eq!(p.counts().unwoken.0, 0, "the kernel took the wake");
    let (_, tl) = p.target(TargetIx(0)).expect("a target");
    gpu.signal(tl.expect("a timeline"), released[0])
        .expect("the compositor's release");
    until_readable(|| {
        p.release_fd()
            .map(|fd| fd.try_clone_to_owned().expect("a copy"))
    });
    assert_eq!(p.release_reached(), Owes::Frame, "the owed frame");
    assert_eq!(p.release_fd().map(|_| ()), None, "the wake given up");
    assert_eq!(
        frame(&gpu, &mut p, &mut r, &mut t),
        Frame::Drew(TargetIx(0)),
        "drawn into the target released"
    );
    p.destroy(&gpu);
}

/// **A wake the kernel refuses is counted, and the owed frame is drawn at
/// the next wake of the loop**, on RADV with explicit sync: wakes refused,
/// both targets held to their release points, each frame asked is owed
/// with no descriptor to poll and counted as unwoken, and once the first
/// target's release point is reached the next frame asked is drawn into
/// it. Named break: a frame owed with nothing to wake the loop, and
/// nothing to say so.
#[test]
#[ignore = "needs an AMD GPU: scripts/linux-vulkan-radv.sh"]
fn a_refused_wake_is_counted_and_the_frame_drawn_at_the_next_wake_on_radv() {
    let gpu = radv_explicit();
    let data = jetbrains_mono();
    let faces = [Face::parse(&data).expect("the face")];
    let mut r = renderer(&faces);
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut p = warm(&gpu, &mut r, &mut t);
    gpu.refuse_wakes();
    let mut released = Vec::new();
    for k in 0..2 {
        feed(&mut t, b"x");
        assert_eq!(
            frame(&gpu, &mut p, &mut r, &mut t),
            Frame::Drew(TargetIx(k))
        );
        let (_, release) = p.points(TargetIx(k)).expect("explicit points");
        p.shown(TargetIx(k), DmabufShown::Yes);
        released.push(release);
    }
    for n in 1..=2 {
        feed(&mut t, b"y");
        assert_eq!(frame(&gpu, &mut p, &mut r, &mut t), Frame::Owed);
        assert_eq!(p.release_fd().map(|_| ()), None, "no wake made");
        assert_eq!(p.counts().unwoken.0, n, "each owed frame counted");
    }
    let (_, tl) = p.target(TargetIx(0)).expect("a target");
    gpu.signal(tl.expect("a timeline"), released[0])
        .expect("the compositor's release");
    assert_eq!(
        frame(&gpu, &mut p, &mut r, &mut t),
        Frame::Drew(TargetIx(0)),
        "drawn into the target released"
    );
    assert_eq!(p.counts().unwoken.0, 2, "a frame drawn is not counted");
    p.destroy(&gpu);
}
