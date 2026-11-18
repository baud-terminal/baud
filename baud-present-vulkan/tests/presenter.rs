//! **The Linux driver's GPU arm** (`present.rs`) on lavapipe: which frames
//! it draws, which it hands the CPU arm and which it owes, and what each
//! target is drawn with after a refusal, a CPU frame and a resize.

#![cfg(target_os = "linux")]

mod common;
mod scenes;

use baud_core::{Owned, Term};
use baud_domain::{Rows, WinSize};
use baud_font::Face;
use baud_present_vulkan::damage::TargetIx;
use baud_present_vulkan::frame::Gpu;
use baud_present_vulkan::pipeline::Built;
use baud_present_vulkan::present::{Cpu, Frame, Inputs, Presenter, Remade};
use baud_present_wayland::{Changed, DmabufShown, Px};
use baud_render::{Renderer, Stack, Theme};
use common::lavapipe;
use scenes::{feed, jetbrains_mono};
use std::io::Read;
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

/// The frame `term` makes through `r`, sealed as the driver seals it.
fn frame(gpu: &Gpu, p: &mut Presenter, r: &mut Renderer<'_, '_>, t: &mut Term<'_>) -> Frame {
    let seal = t.seal_writes();
    p.frame(gpu, r, t, seal, &NONE).expect("a frame")
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

/// The AMD GPU, its frames synchronised explicitly.
fn radv_explicit() -> Gpu {
    use baud_present_vulkan::choose::{Kind, SyncObj, choose};
    use baud_present_vulkan::vk::Vulkan;
    use baud_present_wayland::dmabuf::{
        Count, Feedback, Format, Fourcc, Modifier, Tranche, TrancheFlags,
    };
    let vulkan = Vulkan::open().expect("libvulkan");
    let (physical, gpu, render) = vulkan
        .candidates()
        .into_iter()
        .find_map(|(pd, c)| match c.kind {
            Kind::Gpu {
                render: Some(r), ..
            } => Some((pd, c, r)),
            _ => None,
        })
        .expect("a GPU with a render node");
    let offered = Feedback {
        main: Some(render.dev()),
        tranches: vec![Tranche {
            device: Some(render.dev()),
            flags: TrancheFlags(0),
            formats: vec![Format {
                fourcc: Fourcc::ARGB8888,
                modifier: Modifier::LINEAR,
            }],
        }],
        unknown_indices: Count(0),
    };
    let choice = choose(core::slice::from_ref(&gpu), &offered, SyncObj::Offered).expect("the GPU");
    assert_eq!(
        choice.sync,
        baud_present_vulkan::choose::Sync::Explicit,
        "the premise: explicit sync"
    );
    Gpu::new(vulkan, physical, choice).expect("a device")
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
        let (acquire, release) = p
            .points(&gpu, TargetIx(k))
            .expect("signalled")
            .expect("explicit points");
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
    p.destroy(&gpu);
}
