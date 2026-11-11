//! **What a GPU frame that fails, or never finishes, does to the
//! presenter**, through the fault seam (`gpu-fault`): the GPU runs every
//! frame, and only what its completion reports is replaced -- a failure
//! with a code, or the whole completion held until the test releases it.
//!
//! macOS only, and it needs a Metal device: it panics without one rather
//! than passing.
#![cfg(target_os = "macos")]

use baud_domain::{Cols, Rows};
use baud_font::Face;
use baud_present_macos::journal::Reshape;
use baud_present_macos::{
    Cause, Done, Drawn, ErrorNumber, FaultCode, Gpu, InFlight, Inject, Presenter, StatusWord,
    Waited, Wake,
};
use baud_render::{Blend, PxSize, Renderer, Stack, Tally, Theme};
use macos_liquid_glass::drawable::{ColorSpace, SurfaceHandle};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// How long a test waits for a completion the GPU owes it before failing:
/// a bound on a wait that is never expected to come near it.
const OWED: Duration = Duration::from_secs(30);

/// Counts what the presenter asks of the view.
#[derive(Default)]
struct Watch {
    presents: AtomicUsize,
    wakes: AtomicUsize,
}

#[derive(Clone, Default)]
struct Watcher(Arc<Watch>);

impl Watcher {
    fn seen(&self) -> (usize, usize) {
        (
            self.0.presents.load(Ordering::Relaxed),
            self.0.wakes.load(Ordering::Relaxed),
        )
    }
}

impl Wake for Watcher {
    fn present(&self, _: &SurfaceHandle) {
        self.0.presents.fetch_add(1, Ordering::Relaxed);
    }

    fn wake(&self) {
        self.0.wakes.fetch_add(1, Ordering::Relaxed);
    }
}

fn font() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../baud-font/tests/fonts/JetBrainsMono-Regular.ttf"
    ))
    .expect("the test font")
}

fn renderer<'f, 'a>(faces: &'f [Face<'a>]) -> Renderer<'f, 'a> {
    Renderer::new(
        Stack::new(faces).expect("a stack"),
        PxSize::of(14.0).expect("a size"),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(20),
        Rows::of_index(4),
    )
    .expect("a renderer")
}

/// A presenter on the GPU path, of the size `r` draws.
fn presenter(r: &Renderer<'_, '_>) -> Presenter {
    let gpu = Gpu::new().expect("a Metal device; this test cannot skip itself");
    let (w, h) = r.whole_size();
    Presenter::over(gpu, w, h, ColorSpace::Srgb).expect("surfaces")
}

/// The presenter's GPU, for the seam.
fn gpu(p: &Presenter) -> &Gpu {
    p.gpu().expect("the GPU path")
}

/// Waits for the `n`th completion to be delivered, failing if it never is.
fn delivered(p: &Presenter, n: usize) {
    assert_eq!(
        gpu(p).await_delivered(Tally::of(n), OWED),
        Tally::of(n),
        "completion {n} never came"
    );
}

/// **A frame that failed is not presented, and the window goes on**: the
/// layer keeps the last surface that drew, the responder is woken so the
/// next frame is drawn, the frame is counted failed and not landed, and
/// nothing is left waiting on it -- its count in flight is given back, so
/// a zoom does not defer, and the next frame draws and lands. Breaks: the
/// presenter presenting a failed frame as it presented every frame before,
/// and the presents read 2; `Journal::fail` not giving back the frame's
/// count in flight, and a reshape waits for a landing that cannot come.
#[test]
fn a_failed_frame_is_not_presented_and_the_window_goes_on() {
    let data = font();
    let faces = [Face::parse(&data).expect("a face")];
    let mut r = renderer(&faces);
    let mut p = presenter(&r);
    let watch = Watcher::default();

    let Some(Drawn::Frame(_)) = p.draw(watch.clone(), &mut r) else {
        panic!("the first frame");
    };
    delivered(&p, 1);
    assert_eq!(
        watch.seen(),
        (1, 1),
        "the control: a frame that drew is shown"
    );

    gpu(&p).inject(Inject::Fail(Cause::Error(FaultCode::OutOfMemory)));
    let Some(Drawn::Frame(f)) = p.draw(watch.clone(), &mut r) else {
        panic!("the failing frame commits");
    };
    assert_eq!(f.waited, Waited::No, "notified, not waited for");
    delivered(&p, 2);
    gpu(&p).inject(Inject::Pass);
    assert_eq!(
        watch.seen(),
        (1, 2),
        "the failed frame is not presented, and the responder is woken"
    );
    let c = p.counts();
    assert_eq!((c.landed, c.failed), (1, 1), "{c:?}");
    assert_eq!(p.in_flight(), InFlight::Idle, "counted out of flight");
    assert_eq!(p.reshape(), Reshape::Now, "nothing waits on a failed frame");
    let faults = p.faults().expect("the GPU path");
    assert_eq!(faults.of(FaultCode::OutOfMemory).value(), 1, "{faults}");

    let Some(Drawn::Frame(_)) = p.draw(watch.clone(), &mut r) else {
        panic!("the frame after");
    };
    delivered(&p, 3);
    assert_eq!(watch.seen(), (2, 3), "the frame after draws and is shown");
    assert_eq!(p.counts().landed, 2);
}

/// **Every fault counter is fed**: one frame failed with each code the
/// header names and with one it does not, one completing with an odd
/// status, and the warm-up pass failed, each counted where it belongs and
/// nowhere else; the slowest frame is measured. Break: drop any one of the
/// handler's counts, and the counter it fed reads zero.
#[test]
fn every_fault_counter_is_fed() {
    let data = font();
    let faces = [Face::parse(&data).expect("a face")];
    let mut r = renderer(&faces);
    let mut g = Gpu::new().expect("a Metal device; this test cannot skip itself");
    let (w, h) = r.whole_size();
    let target = g.offscreen(w, h).expect("a target");
    let codes: Vec<FaultCode> = FaultCode::NAMED
        .into_iter()
        .chain([FaultCode::Unknown(ErrorNumber(77))])
        .collect();
    for code in &codes {
        g.inject(Inject::Fail(Cause::Error(*code)));
        let Some(Drawn::Frame(f)) = g.draw(&mut r, &target, Done::Wait) else {
            panic!("{code:?}: a frame");
        };
        match &f.waited {
            Waited::Failed(fault) => assert_eq!(fault.cause, Cause::Error(*code)),
            other => panic!("{code:?}: the wait found {other}"),
        }
    }
    g.inject(Inject::Fail(Cause::Odd(StatusWord(3))));
    let Some(Drawn::Frame(_)) = g.draw(&mut r, &target, Done::Wait) else {
        panic!("the odd frame");
    };
    g.inject(Inject::Fail(Cause::Error(FaultCode::Internal)));
    let before = g.await_delivered(Tally::ZERO, Duration::ZERO);
    g.warm();
    assert_eq!(
        g.await_delivered(before.inc(), OWED),
        before.inc(),
        "the warm-up pass completes"
    );
    g.inject(Inject::Pass);
    let faults = g.faults();
    for code in &codes {
        assert_eq!(faults.of(*code).value(), 1, "{code:?}: {faults}");
    }
    assert_eq!(faults.odd_status.value(), 1, "{faults}");
    assert_eq!(faults.warm_failed.value(), 1, "{faults}");
    assert_eq!(
        faults.frames_failed().value(),
        u64::try_from(codes.len()).unwrap() + 1,
        "the warm-up pass is no frame: {faults}"
    );
    assert!(faults.slowest > Duration::ZERO, "{faults}");
    // And a frame that draws counts nothing.
    let Some(Drawn::Frame(f)) = g.draw(&mut r, &target, Done::Wait) else {
        panic!("a frame");
    };
    assert!(f.waited.landed().is_ok(), "{}", f.waited);
    assert_eq!(g.faults().frames_failed(), faults.frames_failed());
}

/// **A held completion holds the whole frame in flight**, as a frame the
/// GPU never finishes does: the frame stays in flight, every draw after it
/// answers `Busy`, a reshape waits, nothing is presented, nothing woken
/// and nothing landed -- the window is frozen, and nothing yet says so.
/// Released, it is delivered once: counted out of flight once, landed,
/// shown and woken, and drawing goes on. Breaks: the handler counting the
/// frame out before it holds the rest, and the frame reads idle while
/// held; releasing twice delivering twice, and `landed` reads 2.
#[test]
fn a_held_completion_holds_the_frame_in_flight_until_released() {
    let data = font();
    let faces = [Face::parse(&data).expect("a face")];
    let mut r = renderer(&faces);
    let mut p = presenter(&r);
    let watch = Watcher::default();

    gpu(&p).inject(Inject::Hold);
    let Some(Drawn::Frame(_)) = p.draw(watch.clone(), &mut r) else {
        panic!("the held frame commits");
    };
    assert_eq!(
        gpu(&p).await_held(Tally::of(1), OWED).value(),
        1,
        "the GPU finished it"
    );
    gpu(&p).inject(Inject::Pass);
    for k in 0..50 {
        assert_eq!(p.in_flight(), InFlight::Busy, "draw {k}: still in flight");
        assert!(
            matches!(p.draw(watch.clone(), &mut r), Some(Drawn::Busy)),
            "draw {k}: busy"
        );
    }
    assert_eq!(p.reshape(), Reshape::Wait, "a zoom waits on it");
    assert_eq!(watch.seen(), (0, 0), "nothing presented, nothing woken");
    let c = p.counts();
    assert_eq!((c.landed, c.abandoned), (0, 50), "{c:?}");
    let faults = p.faults().expect("the GPU path");
    assert_eq!(
        faults.frames_failed().value(),
        0,
        "and nothing counts it: {faults}"
    );

    assert_eq!(gpu(&p).release().value(), 1, "one completion held");
    assert_eq!(gpu(&p).release().value(), 0, "and delivered once");
    assert_eq!(p.in_flight(), InFlight::Idle, "counted out of flight");
    assert_eq!(watch.seen(), (1, 1), "shown and woken");
    assert_eq!(p.counts().landed, 1);
    assert_eq!(p.reshape(), Reshape::Now);
    let Some(Drawn::Frame(_)) = p.draw(watch.clone(), &mut r) else {
        panic!("drawing goes on");
    };
    delivered(&p, 2);
    assert_eq!(watch.seen(), (2, 2));
    assert_eq!(p.counts().landed, 2);
}
