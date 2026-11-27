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
use baud_present_macos::redraw::SurfaceIx;
use baud_present_macos::{
    Came, Cause, Changed, Deadline, Done, Drawn, ErrorNumber, FaultCode, GaveUp, Gpu, GpuUse,
    InFlight, Inject, Mode, Presented, Presenter, Refusal, StatusWord, Switched, Waited, Wake,
};
use baud_render::{Argb, Blend, PxSize, Redraw, Renderer, Stack, Tally, Theme};
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
/// nowhere else; the slowest frame is measured. A frame that lands follows
/// each failure, so no three are in a row and the GPU is not given up; the
/// two codes that give it up at once are each counted on a `Gpu` of their
/// own. Break: drop any one of the handler's counts, and the counter it
/// fed reads zero.
#[test]
fn every_fault_counter_is_fed() {
    let data = font();
    let faces = [Face::parse(&data).expect("a face")];
    let mut r = renderer(&faces);
    let mut g = Gpu::new().expect("a Metal device; this test cannot skip itself");
    let (w, h) = r.whole_size();
    let target = g.offscreen(w, h).expect("a target");
    let refusing = [FaultCode::AccessRevoked, FaultCode::NotPermitted];
    let codes: Vec<FaultCode> = FaultCode::NAMED
        .into_iter()
        .filter(|c| !refusing.contains(c))
        .chain([FaultCode::Unknown(ErrorNumber(77))])
        .collect();
    let landed = |g: &mut Gpu, r: &mut Renderer<'_, '_>| {
        g.inject(Inject::Pass);
        let Some(Drawn::Frame(f)) = g.draw(r, &target, Done::Within(Deadline::WAIT)) else {
            panic!("a frame between");
        };
        assert!(f.waited.landed().is_ok(), "{}", f.waited);
    };
    for code in &codes {
        g.inject(Inject::Fail(Cause::Error(*code)));
        let Some(Drawn::Frame(f)) = g.draw(&mut r, &target, Done::Within(Deadline::WAIT)) else {
            panic!("{code:?}: a frame");
        };
        match &f.waited {
            Waited::Failed(fault) => assert_eq!(fault.cause, Cause::Error(*code)),
            other => panic!("{code:?}: the wait found {other}"),
        }
        landed(&mut g, &mut r);
    }
    g.inject(Inject::Fail(Cause::Odd(StatusWord(3))));
    let Some(Drawn::Frame(_)) = g.draw(&mut r, &target, Done::Within(Deadline::WAIT)) else {
        panic!("the odd frame");
    };
    landed(&mut g, &mut r);
    for code in refusing {
        let mut own = Gpu::new().expect("a Metal device");
        let at = own.offscreen(w, h).expect("a target");
        own.inject(Inject::Fail(Cause::Error(code)));
        let Some(Drawn::Frame(_)) = own.draw(&mut r, &at, Done::Within(Deadline::WAIT)) else {
            panic!("{code:?}: a frame");
        };
        let f = own.faults();
        assert_eq!(
            (f.of(code).value(), f.frames_failed().value()),
            (1, 1),
            "{code:?}: {f}"
        );
    }
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
    let Some(Drawn::Frame(f)) = g.draw(&mut r, &target, Done::Within(Deadline::WAIT)) else {
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

/// **A wait past its bound returns, and says so** (T2): a frame whose
/// completion the seam holds is waited for with a bound of the test's own,
/// 50 ms, and the wait returns `Overdue` -- the frame still committed and
/// in flight, counted overdue once -- rather than blocking until the
/// completion comes, which here is never. Break: the wait blocking with no
/// bound, and the test hangs until its runner's bound kills it.
#[test]
fn a_wait_past_its_deadline_returns_overdue() {
    let data = font();
    let faces = [Face::parse(&data).expect("a face")];
    let mut r = renderer(&faces);
    let mut g = Gpu::new().expect("a Metal device; this test cannot skip itself");
    let (w, h) = r.whole_size();
    let target = g.offscreen(w, h).expect("a target");
    g.inject(Inject::Hold);
    let bound = Deadline::injected(Duration::from_millis(50));
    let Some(Drawn::Frame(f)) = g.draw(&mut r, &target, Done::Within(bound)) else {
        panic!("the held frame commits");
    };
    g.inject(Inject::Pass);
    let Waited::Overdue(o) = &f.waited else {
        panic!("the wait found {}", f.waited);
    };
    assert!(o.after >= bound.get(), "returned before its bound: {o:?}");
    assert_eq!(
        g.in_flight(),
        InFlight::Busy,
        "still committed, still in flight"
    );
    let faults = g.faults();
    assert_eq!(
        (faults.overdue.value(), faults.late.value()),
        (1, 0),
        "{faults}"
    );
    assert_eq!(g.release().value(), 1, "the one completion held");
    assert_eq!(g.in_flight(), InFlight::Idle);
}

/// **A completion after its bound is counted out once, and counted late**
/// (T8): the frame waited for past its bound is still in flight; released,
/// its handler counts it late and out of flight, once, and the frame after
/// it is compared with nothing and drawn whole, since the one before was
/// for every consumer not drawn until it landed. Breaks: the waiter
/// counting the frame out of flight at its bound as well as the handler,
/// and the count wraps and reads busy for ever; the handler not counting a
/// late answer, and `late` reads 0.
#[test]
fn the_late_completion_counts_out_once() {
    let data = font();
    let faces = [Face::parse(&data).expect("a face")];
    let mut r = renderer(&faces);
    let mut g = Gpu::new().expect("a Metal device; this test cannot skip itself");
    let (w, h) = r.whole_size();
    let target = g.offscreen(w, h).expect("a target");
    let Some(Drawn::Frame(f)) = g.draw(&mut r, &target, Done::Within(Deadline::WAIT)) else {
        panic!("the first frame");
    };
    assert!(f.waited.landed().is_ok(), "{}", f.waited);

    g.inject(Inject::Hold);
    let bound = Deadline::injected(Duration::from_millis(50));
    let Some(Drawn::Frame(f)) = g.draw(&mut r, &target, Done::Within(bound)) else {
        panic!("the held frame commits");
    };
    g.inject(Inject::Pass);
    assert!(matches!(f.waited, Waited::Overdue(_)), "{}", f.waited);
    assert_eq!(
        g.in_flight(),
        InFlight::Busy,
        "in flight until it is answered"
    );
    assert_eq!(g.release().value(), 1);
    assert_eq!(g.release().value(), 0, "delivered once");
    assert_eq!(g.in_flight(), InFlight::Idle, "counted out of flight once");
    let faults = g.faults();
    assert_eq!(
        (faults.overdue.value(), faults.late.value()),
        (1, 1),
        "{faults}"
    );

    let Some(Drawn::Frame(f)) = g.draw(&mut r, &target, Done::Within(Deadline::WAIT)) else {
        panic!("the frame after");
    };
    assert!(f.waited.landed().is_ok(), "{}", f.waited);
    assert_eq!(
        f.drawn,
        Redraw::Whole,
        "the frame after a late one is drawn whole"
    );
    assert_eq!(g.faults().late.value(), 1, "a frame in time is not late");
}

/// **A notified frame's bounds, as the window's wakes meet them** (M2):
/// the frame carries which commit it is, and its wakes count only that
/// frame. Before its bound a wake counts nothing; at
/// [`Deadline::FRAME`] it is counted overdue, once however many wakes come;
/// at [`Deadline::GIVE_UP`] the GPU is given up, once for the GPU's life;
/// answered after, it is counted late, presented and woken; and a frame
/// answered in time is never counted by its wakes, at any bound. The
/// instants are the frame's own commit plus each bound, so no wall time
/// is waited. Breaks: a wake counting whatever frame is in flight rather
/// than its own, and the frame answered in time reads overdue; the frame
/// bound's count dropped, and `overdue` reads 0.
#[test]
fn a_notified_frame_is_overdue_at_its_bound_and_late_after() {
    let data = font();
    let faces = [Face::parse(&data).expect("a face")];
    let mut r = renderer(&faces);
    let mut p = presenter(&r);
    let watch = Watcher::default();

    gpu(&p).inject(Inject::Hold);
    let Some(Drawn::Frame(f)) = p.draw(watch.clone(), &mut r) else {
        panic!("the held frame commits");
    };
    gpu(&p).inject(Inject::Pass);
    let held = f.flying.expect("a committed frame carries its commit");
    assert_eq!(gpu(&p).await_held(Tally::of(1), OWED).value(), 1);

    let frame = held.due(Deadline::FRAME);
    let early = frame
        .checked_sub(Duration::from_millis(1))
        .expect("an instant");
    assert_eq!(held.frame_due(early), Came::Early);
    assert_eq!(p.faults().expect("gpu").overdue.value(), 0);
    assert_eq!(held.frame_due(frame), Came::Overdue);
    assert_eq!(held.frame_due(frame), Came::Overdue, "a second wake");
    let faults = p.faults().expect("gpu");
    assert_eq!(faults.overdue.value(), 1, "counted once: {faults}");
    assert_eq!(
        gpu(&p).gpu_use(),
        GpuUse::Live,
        "overdue alone gives nothing up"
    );
    assert_eq!(
        p.in_flight(),
        InFlight::Busy,
        "and the window holds every draw"
    );
    assert!(matches!(p.draw(watch.clone(), &mut r), Some(Drawn::Busy)));

    let give_up = held.due(Deadline::GIVE_UP);
    assert_eq!(held.give_up_due(give_up), Came::GaveUp);
    assert_eq!(held.give_up_due(give_up), Came::GaveUp, "a second wake");
    let faults = p.faults().expect("gpu");
    assert_eq!(
        (faults.overdue.value(), faults.gave_up.value()),
        (1, 1),
        "{faults}"
    );
    assert_eq!(gpu(&p).gpu_use(), GpuUse::GaveUp(GaveUp::Stuck));

    assert_eq!(gpu(&p).release().value(), 1);
    assert_eq!(p.in_flight(), InFlight::Idle);
    assert_eq!(watch.seen(), (1, 1), "the late frame is shown and woken");
    let faults = p.faults().expect("gpu");
    assert_eq!(faults.late.value(), 1, "{faults}");
    assert_eq!(held.frame_due(frame), Came::Answered);

    // Given up, the `Gpu` commits nothing more: the next draw says so.
    assert!(
        matches!(
            p.draw(watch.clone(), &mut r),
            Some(Drawn::GaveUp(GaveUp::Stuck))
        ),
        "a draw after the give-up"
    );
    let after = p.faults().expect("gpu");
    assert_eq!(
        (after.overdue, after.late, after.gave_up),
        (faults.overdue, faults.late, faults.gave_up),
        "{after}"
    );
    assert_eq!(
        gpu(&p).gpu_use(),
        GpuUse::GaveUp(GaveUp::Stuck),
        "never back"
    );
}

/// **A frame answered in time is never counted by its wakes**, at either
/// bound, and gives nothing up. Break: a wake counting whatever frame is in
/// flight rather than its own, and the frame reads overdue.
#[test]
fn a_frame_answered_in_time_is_counted_by_no_wake() {
    let data = font();
    let faces = [Face::parse(&data).expect("a face")];
    let mut r = renderer(&faces);
    let mut p = presenter(&r);
    let watch = Watcher::default();
    let Some(Drawn::Frame(f)) = p.draw(watch.clone(), &mut r) else {
        panic!("a frame");
    };
    delivered(&p, 1);
    let on_time = f.flying.expect("its commit");
    assert_eq!(
        on_time.frame_due(on_time.due(Deadline::FRAME)),
        Came::Answered
    );
    assert_eq!(
        on_time.give_up_due(on_time.due(Deadline::GIVE_UP)),
        Came::Answered
    );
    let after = p.faults().expect("gpu");
    assert_eq!(
        (after.overdue, after.late, after.gave_up),
        (Tally::ZERO, Tally::ZERO, Tally::ZERO),
        "{after}"
    );
    assert_eq!(gpu(&p).gpu_use(), GpuUse::Live);
}

/// The next `n` frames committed on `p`'s GPU report failing with `code`.
fn fail_next(p: &Presenter, code: FaultCode, n: usize) {
    gpu(p).inject(Inject::FailNext(Cause::Error(code), Tally::of(n)));
}

/// `n` frames drawn on `p`, each delivered before the next is drawn: the
/// completions so far are `from` plus these.
fn draw_delivered(p: &mut Presenter, r: &mut Renderer<'_, '_>, w: &Watcher, from: usize, n: usize) {
    for k in 1..=n {
        let Some(Drawn::Frame(_)) = p.draw(w.clone(), r) else {
            panic!("frame {} commits", from + k);
        };
        delivered(p, from + k);
    }
}

/// **Three failed frames in a row give the GPU up** (U1): the third's
/// handler latches `Failing`, counted once, and the fourth draw commits
/// nothing and says why. Break: no latch, and the fourth draw answers
/// `Drawn::Frame`.
#[test]
fn three_failed_frames_in_a_row_give_the_gpu_up() {
    let data = font();
    let faces = [Face::parse(&data).expect("a face")];
    let mut r = renderer(&faces);
    let mut p = presenter(&r);
    let watch = Watcher::default();
    fail_next(&p, FaultCode::Internal, 3);
    draw_delivered(&mut p, &mut r, &watch, 0, 3);
    let f = p.faults().expect("the GPU path");
    assert_eq!(f.frames_failed().value(), 3, "{f}");
    assert_eq!(
        (f.why, f.gave_up),
        (Some(GaveUp::Failing), Tally::of(1)),
        "{f}"
    );
    assert_eq!(gpu(&p).gpu_use(), GpuUse::GaveUp(GaveUp::Failing));
    assert!(
        matches!(
            p.draw(watch.clone(), &mut r),
            Some(Drawn::GaveUp(GaveUp::Failing))
        ),
        "the fourth draw commits nothing"
    );
    assert_eq!(p.faults().expect("gpu").frames_failed().value(), 3);
}

/// **A frame that lands ends the row** (U1b): two failed, one landed, two
/// failed is four failures and no give-up, and the next frame draws. Break:
/// the row not ended by a landing, and the fourth failure gives up.
#[test]
fn a_landed_frame_between_failures_resets_the_count() {
    let data = font();
    let faces = [Face::parse(&data).expect("a face")];
    let mut r = renderer(&faces);
    let mut p = presenter(&r);
    let watch = Watcher::default();
    fail_next(&p, FaultCode::Internal, 2);
    draw_delivered(&mut p, &mut r, &watch, 0, 2);
    draw_delivered(&mut p, &mut r, &watch, 2, 1);
    fail_next(&p, FaultCode::Internal, 2);
    draw_delivered(&mut p, &mut r, &watch, 3, 2);
    let f = p.faults().expect("the GPU path");
    assert_eq!(f.frames_failed().value(), 4, "{f}");
    assert_eq!((f.why, f.gave_up), (None, Tally::ZERO), "{f}");
    draw_delivered(&mut p, &mut r, &watch, 5, 1);
    assert_eq!(gpu(&p).gpu_use(), GpuUse::Live);
}

/// **A device that refuses the process gives the GPU up at its first
/// frame** (U2), for each of the two codes the header gives for it, as
/// `Refused` with that code. Break: the code not read, and one frame gives
/// nothing up.
#[test]
fn an_access_revoked_frame_gives_the_gpu_up_at_once() {
    let data = font();
    let faces = [Face::parse(&data).expect("a face")];
    let mut r = renderer(&faces);
    for (code, why) in [
        (FaultCode::AccessRevoked, Refusal::AccessRevoked),
        (FaultCode::NotPermitted, Refusal::NotPermitted),
    ] {
        let mut p = presenter(&r);
        let watch = Watcher::default();
        fail_next(&p, code, 1);
        draw_delivered(&mut p, &mut r, &watch, 0, 1);
        let f = p.faults().expect("the GPU path");
        assert_eq!(
            (f.why, f.gave_up),
            (Some(GaveUp::Refused(why)), Tally::of(1)),
            "{code:?}: {f}"
        );
        assert!(
            matches!(p.draw(watch.clone(), &mut r), Some(Drawn::GaveUp(_))),
            "{code:?}: the next draw commits nothing"
        );
    }
}

/// **Codes a GPU reset could give are failures in a row, not a refusal**
/// (U2b): three frames failed with a code the header does not name give
/// the GPU up as `Failing`; one `Timeout` gives nothing up. Break: every
/// code read as a refusal, and the timeout gives up.
#[test]
fn codes_a_reset_would_give_are_failing_not_refused() {
    let data = font();
    let faces = [Face::parse(&data).expect("a face")];
    let mut r = renderer(&faces);
    let mut p = presenter(&r);
    let watch = Watcher::default();
    fail_next(&p, FaultCode::Unknown(ErrorNumber(5)), 3);
    draw_delivered(&mut p, &mut r, &watch, 0, 3);
    let f = p.faults().expect("the GPU path");
    assert_eq!(f.why, Some(GaveUp::Failing), "{f}");

    let mut p = presenter(&r);
    fail_next(&p, FaultCode::Timeout, 1);
    draw_delivered(&mut p, &mut r, &watch, 0, 1);
    let f = p.faults().expect("the GPU path");
    assert_eq!((f.why, f.gave_up), (None, Tally::ZERO), "{f}");
}

/// A whole frame for `p`'s surfaces, every pixel `word`, and its stride.
fn cpu_frame(p: &Presenter, word: u32) -> (Vec<u32>, baud_font::Px) {
    let (w, h) = p.size();
    let n = usize::try_from(w.value() * h.value()).expect("a frame");
    (vec![word; n], w)
}

/// **A frame answering after the give-up is neither shown nor counted
/// twice** (U3): a held frame past its give-up bound, the presenter given
/// up and a CPU frame copied, then the held frame released. It lands in
/// the parked journal, superseded, so nothing is presented; it is counted
/// late once and overdue once, the give-up once; the new journal never
/// waits on it, before the release or after; and the CPU frame's surface
/// is not written by it. Breaks: the epoch not carried, and the late
/// frame is presented; the journal carried, and a reshape waits before the
/// release.
#[test]
fn a_late_answer_after_the_give_up_is_neither_presented_nor_counted_twice() {
    let data = font();
    let faces = [Face::parse(&data).expect("a face")];
    let mut r = renderer(&faces);
    let mut p = presenter(&r);
    let watch = Watcher::default();

    gpu(&p).inject(Inject::Hold);
    let Some(Drawn::Frame(f)) = p.draw(watch.clone(), &mut r) else {
        panic!("the held frame commits");
    };
    gpu(&p).inject(Inject::Pass);
    let held = f.flying.expect("its commit");
    assert_eq!(gpu(&p).await_held(Tally::of(1), OWED).value(), 1);
    assert_eq!(held.give_up_due(held.due(Deadline::GIVE_UP)), Came::GaveUp);

    assert_eq!(p.give_up(GaveUp::Stuck), Switched::Now);
    assert_eq!(p.give_up(GaveUp::Stuck), Switched::Already, "once");
    assert_eq!(p.reshape(), Reshape::Now, "nothing waits on the held frame");
    let (frame, stride) = cpu_frame(&p, 0xff20_4060);
    assert_eq!(
        p.copy_changed(&frame, stride, Argb(0xff00_0000), Changed::Whole),
        Presented::Yes,
        "the CPU frame is copied"
    );
    let shown = p.surface_pixels(SurfaceIx::FIRST).expect("the surface");

    assert_eq!(p.parked_release(), Some(Tally::of(1)), "the one held");
    assert_eq!(watch.seen(), (0, 1), "woken, and nothing presented");
    let (before, now) = p.parked_counts().expect("given up");
    assert_eq!((before.landed, before.superseded), (0, 0), "{before:?}");
    assert_eq!(
        (now.landed, now.superseded, now.shown),
        (1, 1, 0),
        "{now:?}"
    );
    let f = p.faults().expect("kept");
    assert_eq!(
        (f.overdue, f.late, f.gave_up, f.why),
        (
            Tally::of(1),
            Tally::of(1),
            Tally::of(1),
            Some(GaveUp::Stuck)
        ),
        "{f}"
    );
    assert_eq!(p.reshape(), Reshape::Now);
    assert_eq!(p.counts().landed, 0, "the new journal saw no landing");
    assert_eq!(
        p.surface_pixels(SurfaceIx::FIRST).expect("the surface"),
        shown,
        "the CPU frame is as copied"
    );
}

/// **A presenter that gave the GPU up never draws on it** (U4): every GPU
/// draw answers `None`, the path is the CPU's, nothing is in flight, the
/// `Gpu` is out of reach and its counts are not, and a resize keeps all of
/// it. Break: a resize building the given-up path as the CPU's, and the
/// counts are gone.
#[test]
fn a_presenter_given_up_never_draws_on_the_gpu() {
    let data = font();
    let faces = [Face::parse(&data).expect("a face")];
    let mut r = renderer(&faces);
    let mut p = presenter(&r);
    let watch = Watcher::default();
    assert_eq!(p.give_up(GaveUp::Stuck), Switched::Now);
    let check = |p: &mut Presenter, r: &mut Renderer<'_, '_>, when: &str| {
        assert!(p.draw(watch.clone(), r).is_none(), "{when}: draw");
        assert!(
            p.keep_frame(watch.clone(), r).is_none(),
            "{when}: keep_frame"
        );
        assert_eq!(p.mode(), Mode::Cpu, "{when}");
        assert_eq!(p.in_flight(), InFlight::Idle, "{when}");
        assert!(p.gpu().is_none(), "{when}: the Gpu is out of reach");
        assert!(p.faults().is_some(), "{when}: its counts are not");
        assert_eq!(p.gave_up(), Some(GaveUp::Stuck), "{when}");
    };
    check(&mut p, &mut r, "given up");
    let (w, h) = p.size();
    p.resize(
        baud_font::Px::of(w.value() + 8),
        baud_font::Px::of(h.value() + 8),
    )
    .expect("resized");
    check(&mut p, &mut r, "resized");
    assert_eq!(watch.seen(), (0, 0));
}

/// **A surface set declined at the switch keeps the given-up GPU** (K1):
/// the GPU is given up all the same, its counts kept, nothing is copied
/// into the old set -- a frame in flight may write any of it -- and the
/// next resize makes a set the CPU draws into. Break: the parked `Gpu`
/// dropped with the declined set, and the counts are gone.
#[test]
fn a_surface_set_declined_at_the_switch_keeps_the_given_up_gpu() {
    let data = font();
    let faces = [Face::parse(&data).expect("a face")];
    let r = renderer(&faces);
    let mut p = presenter(&r);
    p.refuse_next_set();
    assert_eq!(p.give_up(GaveUp::Failing), Switched::Refused);
    assert!(p.faults().is_some(), "the counts are kept");
    assert_eq!(p.gave_up(), Some(GaveUp::Failing));
    assert_eq!(p.mode(), Mode::Cpu);
    assert_eq!(p.reshape(), Reshape::Now);
    let (frame, stride) = cpu_frame(&p, 0xff20_4060);
    let clear = Argb(0xff00_0000);
    assert_eq!(
        p.copy_changed(&frame, stride, clear, Changed::Whole),
        Presented::No,
        "nothing is copied into the old set"
    );
    let (w, h) = p.size();
    p.resize(w, h).expect("a set at the next resize");
    assert_eq!(
        p.copy_changed(&frame, stride, clear, Changed::Whole),
        Presented::Yes
    );
    assert!(p.faults().is_some());
    assert_eq!(p.gave_up(), Some(GaveUp::Failing));
}

/// **What a given-up `Gpu` keeps** (a measurement, printed): its device's
/// `currentAllocatedSize` after a frame at a 200x50 grid, before the
/// give-up and after it, when everything allocated stays parked for the
/// window's life.
#[test]
fn the_given_up_gpu_keeps_what_it_allocated() {
    let data = font();
    let faces = [Face::parse(&data).expect("a face")];
    let mut r = Renderer::new(
        Stack::new(&faces).expect("a stack"),
        PxSize::of(28.0).expect("a size"),
        Theme::dark(),
        Blend::Native,
        Cols::of_index(200),
        Rows::of_index(50),
    )
    .expect("a renderer");
    let mut p = presenter(&r);
    let watch = Watcher::default();
    draw_delivered(&mut p, &mut r, &watch, 0, 1);
    let before = gpu(&p).allocated();
    assert_eq!(p.give_up(GaveUp::Stuck), Switched::Now);
    let after = p.parked_allocated().expect("parked");
    let (w, h) = p.size();
    println!(
        "allocated at {}x{} px: before the give-up {} B, after {} B",
        w.value(),
        h.value(),
        before.0,
        after.0
    );
    assert!(before.0 > 0, "the device allocated nothing");
    assert_eq!(after, before, "the give-up frees nothing it parks");
}
