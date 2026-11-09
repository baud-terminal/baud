//! **The event rig's child** (`tests/event_rig.rs`), built only under the
//! test-only `effect-probe` feature: a real `DrawableView` with baud's
//! `Driver` as its responder, in a process that shows nothing.
//!
//! Its call is a `RigCall` (`baud_present_macos::rig_call`), parsed by
//! `main`'s door before anything else runs, which exits 2 on anything
//! else (`rig_door.rs`): an act, its argument, a count of iterations,
//! the ticks, and the effects each iteration waits for. It makes the window and never
//! orders it in, under the prohibited activation policy, so there is no
//! Dock icon and nothing takes focus; after every iteration the window is
//! checked never shown, never key, never minimised and never in full
//! screen. Each iteration does the row's act,
//! taken on the view's `wake`:
//! - `path`: clicks `arg` as a printed path, and the paths worker's real
//!   thread checks the file and wakes the view with its answer;
//! - `type`: sends `arg` and a newline to the shell as typed keys are
//!   sent, and what the shell runs writes back through the pty, as a
//!   program's output does.
//!
//! Under `tick`, each pass also posts a `wake:` from another thread,
//! joined so it is queued, and sends the view's own `tick:` with a
//! `CADisplayLink` that is never added to a run loop: the display link's
//! code path, with no vsync to wait on. The effects the act comes to are
//! recorded where they run (`baud_present_macos::probe`), and each record
//! is printed as a line for the parent; an iteration with none prints
//! `none`. `tick-inline` and `tick-in-frame` are the controls:
//! the effect queue's turn runs inline, in the callback that asked or in the
//! next frame.
//!
//! Under `focus-believed` baud believes the window is key (`probe::Key`),
//! read by `poll_focus` alone, so a left press is a click rather than the
//! one that brings the window forward; AppKit's key state is untouched,
//! and checked never key after every iteration.

use super::*;
use baud_present_macos::probe;
use baud_present_macos::rig_call as probe_call;
use macos_liquid_glass::objc2::sel;
use probe_call::Iterations;
use std::cell::RefCell;

/// Whether each pass drives the view's frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ticks {
    Driven,
    None,
}

/// What each iteration does.
#[derive(Clone)]
enum Act {
    /// Click this file as a printed path.
    Click(Vec<u8>),
    /// Type this line to the shell, as keys are sent: what it runs writes
    /// back through the pty, as a program's output does.
    Type(Vec<u8>),
    /// A pointer gesture over what this line, typed once first, drew.
    Gesture(Gesture, Vec<u8>),
    /// Type the first line, and the second once the first's first effect
    /// has run: two reads, in order.
    TypeTwo(Vec<u8>, Vec<u8>),
}

/// A gesture over the screen's first row, sent to the view's own methods
/// (`probe::press`, `probe::menu`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Gesture {
    /// Cmd-click on the second cell, a link's.
    LinkClick,
    /// A drag from the first cell to the fourth.
    Select,
    /// That drag, then the Edit menu's Copy.
    CopyMenu,
    /// A click on the notification the setup posted, as the center's
    /// delegate hears one (`notify::rig_click_the_last_post`).
    NoticeClick,
    /// The window's close, as its will-close observer takes it
    /// (`on_close`): the queue told and the window taken down on a later
    /// turn, with no call to the window itself.
    Close,
}

thread_local! {
    /// What the close gesture takes: the window's queue's closer and the
    /// window's key.
    static CLOSE: RefCell<Option<(crate::os_effect::Closer, crate::windows::WindowId)>> =
        const { RefCell::new(None) };
}

thread_local! {
    /// The path the next `wake` clicks.
    static CLICK: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
    /// The line the next `wake` sends the shell.
    static TYPED: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
    /// The characters the next `wake` sends as key presses.
    static PRESSED: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// The click a `wake` takes, if the rig queued one.
pub(super) fn take_click() -> Option<Vec<u8>> {
    CLICK.with(|c| c.borrow_mut().take())
}

/// The line a `wake` sends the shell, if the rig queued one.
pub(super) fn take_typed() -> Option<Vec<u8>> {
    TYPED.with(|c| c.borrow_mut().take())
}

/// The characters a `wake` sends as key presses, if the rig queued them.
pub(super) fn take_pressed() -> Option<String> {
    PRESSED.with(|c| c.borrow_mut().take())
}

/// The bound on one iteration: reaching it is that iteration's failure.
const BOUND: Duration = Duration::from_secs(2);

/// **An iteration's shortest length**, its tail spent running the run
/// loop: longer than the frame interval within which bells ring once
/// (`bell.rs`, `Ringer::hear`). An undriven view keeps the interval it
/// starts with, 8.3 ms, and the bell's rows are undriven, so a bell each
/// iteration is a ring each iteration. A typed line's rows only. A floor on time passed, never a
/// wait for something to happen: what every iteration waits for is its
/// records, under [`BOUND`].
const SPACING: Duration = Duration::from_millis(50);

/// **The rig, as `main`'s door admitted it** (`rig_door.rs`): the call
/// already parsed into `RigCall`, the one type the parent builds too.
pub(crate) fn run_call(call: probe_call::RigCall) -> std::process::ExitCode {
    use probe_call::{RigAct, RigCall, RigFocus, RigTicks};
    let probe_call::EventRig {
        act,
        arg,
        iterations,
        ticks,
        focus,
        each,
    } = match call {
        RigCall::Event(e) => e,
        RigCall::Frames(f) => return run_frames(f),
    };
    // The focus door: baud believes its never-key window is key, or not.
    probe::set_key(match focus {
        RigFocus::Real => probe::Key::No,
        RigFocus::Believed => probe::Key::Yes,
    });
    let line = || [arg.as_bytes(), b"\n"].concat();
    let act = match act {
        RigAct::Path => Act::Click(arg.as_bytes().to_vec()),
        RigAct::Type => Act::Type(line()),
        RigAct::Link => Act::Gesture(Gesture::LinkClick, line()),
        RigAct::Select => Act::Gesture(Gesture::Select, line()),
        RigAct::CopyMenu => Act::Gesture(Gesture::CopyMenu, line()),
        RigAct::NoticeClick => Act::Gesture(Gesture::NoticeClick, line()),
        RigAct::Close => Act::Gesture(Gesture::Close, line()),
        RigAct::TypeTwo => {
            let (first, second) = arg.split_once('\n').unwrap_or((arg.as_str(), ""));
            Act::TypeTwo(
                [first.as_bytes(), b"\n"].concat(),
                [second.as_bytes(), b"\n"].concat(),
            )
        }
    };
    let ticks = match ticks {
        RigTicks::Driven => Ticks::Driven,
        RigTicks::None => Ticks::None,
        // The controls: the hand-over made inline, in the callback, or in
        // the next frame.
        RigTicks::InCallback => {
            probe::set_inline(probe::Inline::InCallback);
            Ticks::Driven
        }
        RigTicks::InFrame => {
            probe::set_inline(probe::Inline::InFrame);
            Ticks::Driven
        }
        RigTicks::AfterInCallback => {
            probe::set_inline(probe::Inline::AfterInCallback);
            Ticks::Driven
        }
    };
    run(act, iterations, ticks, &each)
}

/// A printed path's setup: a clear of the pasteboard, read by the shell.
const READY: &[u8] = b"printf '\\033]52;c;!\\007'\n";

/// Which iteration a gesture is.
#[derive(Clone, Copy)]
struct Iteration(u16);

/// `g` sent to `view`, over the first row's first four cells. A
/// selection's drag ends on the fourth cell one iteration and the third
/// the next, so each makes a selection the last did not.
fn gesture(
    view: &DrawableView,
    g: Gesture,
    [first, second, third, fourth]: [NSPoint; 4],
    at: Iteration,
) {
    use probe::{Chord, Press, press};
    match g {
        Gesture::LinkClick => {
            press(view, Press::Down, second, Chord::Command);
            press(view, Press::Up, second, Chord::Command);
        }
        Gesture::Select => drag(view, first, [fourth, third][usize::from(at.0 % 2)]),
        // The selection made once, by the first iteration's drag, and
        // copied by each iteration's Copy.
        Gesture::CopyMenu => {
            if at.0 == 0 {
                drag(view, first, fourth);
            }
            probe::menu(view, sel!(copy:));
        }
        Gesture::NoticeClick => {
            if !baud_present_macos::notify::rig_click_the_last_post() {
                println!("no notification to click");
            }
        }
        Gesture::Close => CLOSE.with_borrow(|c| {
            if let Some((closer, id)) = c {
                super::on_close(closer, *id);
            }
        }),
    }
}

/// A left-button drag from `from` to `to`.
fn drag(view: &DrawableView, from: NSPoint, to: NSPoint) {
    use probe::{Chord, Press, press};
    press(view, Press::Down, from, Chord::Plain);
    press(view, Press::Dragged, to, Chord::Plain);
    press(view, Press::Up, to, Chord::Plain);
}

/// A `wake:` posted from another thread and joined: certainly queued.
fn post(waker: Waker) {
    let _ = std::thread::spawn(move || waker.wake()).join();
}

/// Whether `got` holds each of `each`, as many times as `each` names it.
fn holds(got: &[probe::Record], each: &[probe::EffectKind]) -> bool {
    each.iter().all(|k| {
        let want = each.iter().filter(|e| *e == k).count();
        got.iter().filter(|r| r.kind == *k).count() >= want
    })
}

fn run(
    act: Act,
    iterations: Iterations,
    ticks: Ticks,
    each: &[probe::EffectKind],
) -> std::process::ExitCode {
    let Some(mtm) = MainThreadMarker::new() else {
        return std::process::ExitCode::from(3);
    };
    let ns_app = NSApplication::sharedApplication(mtm);
    ns_app.setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
    crate::os_effect::install();
    let size = WINDOW_SIZE;
    let scale = 2.0;
    let inset = predicted_inset(mtm, size);
    let shell = Shell::start(scale, size, inset, None, None);
    let Opening {
        mut driver,
        view,
        window,
        ..
    } = Opening::new(mtm, size, scale, inset, shell, None);
    // **A key in the open windows' numbering**, as `show` gives every
    // window it shows: a window's notifications are named by it, and its
    // Dock bar and its clicks are found by it. Never entered into the open
    // windows themselves, which hold what `show` makes.
    let id = crate::windows::Windows::<()>::new().open(());
    driver.window_id = Some(id);
    driver.app.os.name(id);
    CLOSE.with_borrow_mut(|c| *c = Some((driver.app.os.closer(), id)));
    // The display link run for the first frame, as `show` runs it, so the
    // rig's ticks are delivered (`probe::Ticker`).
    driver.app.owe_tick();
    driver.pump(Trigger::Opening);
    let cells = [0, 1, 2, 3].map(|col| driver.rig_point(Row::ZERO, Col::at_index(col)));
    view.set_responder(Box::new(baud_present_macos::unwind::Guarded(driver)));
    // The view the window's first responder, which tells baud it has the
    // keys and the pointer (`becomeFirstResponder`, `Focus::Gained`): a
    // link's Cmd-click is armed only then. The window is not made key,
    // nor shown, by it; the checks below say so every iteration.
    let _ = window.ns_window().makeFirstResponder(Some(&view));
    probe::set_waker(view.waker());
    // A gesture's screen, drawn once: the line typed, and its last effect,
    // a clear of the pasteboard (`OSC 52`) printed after what the gesture
    // is over, is the sign it has been read -- a clear, which says nothing,
    // so no announcement of the setup's is left to fall in an iteration. A
    // count, never a time; the bound is a failure.
    //
    // A printed path's click has the clear alone: the shell read it, so
    // the shell is the terminal's foreground program, which the paths
    // worker asks of the first click. A click before then -- the shell not
    // yet started, under load -- is answered unchecked, and said so.
    let ticker = probe::Ticker::new(&view);
    let setup = match &act {
        Act::Gesture(_, setup) => Some(setup.clone()),
        Act::Click(_) => Some(READY.to_vec()),
        Act::Type(_) | Act::TypeTwo(..) => None,
    };
    if let Some(setup) = setup {
        TYPED.with(|c| *c.borrow_mut() = Some(setup));
        post(view.waker());
        let start = Instant::now();
        let mut ready = false;
        while !ready && start.elapsed() < BOUND {
            // A control's turn parked for the frame runs on a tick.
            if ticks == Ticks::Driven {
                ticker.tick();
            }
            probe::pass(0.01);
            ready = probe::take()
                .iter()
                .any(|r| r.kind == probe::EffectKind::Clear);
        }
        if !ready {
            println!("the row's screen was never drawn");
            return std::process::ExitCode::from(1);
        }
        // What the setup's read asked on a later turn of its own -- the
        // permission's answer, which sends the post that waited -- run
        // here, already queued, so none falls in the first iteration.
        for _ in 0..64 {
            probe::pass(0.0);
        }
        let _ = probe::take();
    }
    let mut failed = false;
    for i in 0..iterations.0 {
        // Kept, never dropped: whatever an iteration's records miss is the
        // parent's to see.
        let mut got = probe::take();
        match &act {
            Act::Click(file) => CLICK.with(|c| *c.borrow_mut() = Some(file.clone())),
            Act::Type(line) => TYPED.with(|c| *c.borrow_mut() = Some(line.clone())),
            Act::Gesture(g, _) => gesture(&view, *g, cells, Iteration(i)),
            Act::TypeTwo(first, _) => TYPED.with(|c| *c.borrow_mut() = Some(first.clone())),
        }
        post(view.waker());
        let start = Instant::now();
        // **The second line once the first's first effect has run**, so
        // the two are two reads, the shell writing the second only after
        // baud has made the first's effect.
        if let Act::TypeTwo(_, second) = &act {
            while !holds(&got, &each[..1]) && start.elapsed() < BOUND {
                if ticks == Ticks::Driven {
                    ticker.tick();
                }
                probe::pass(0.01);
                got.extend(probe::take());
            }
            TYPED.with(|c| *c.borrow_mut() = Some(second.clone()));
            post(view.waker());
        }
        // **Every effect the iteration expects, not the first record**: an
        // effect asked by another's outcome -- a copy's words, on the wake
        // after the copy's turn -- runs a turn later, and a load can put
        // that turn after the passes below, in the next iteration, where
        // the first record ended the wait before that iteration's own
        // effect had run.
        while !holds(&got, each) && start.elapsed() < BOUND {
            if ticks == Ticks::Driven {
                // Queued, and shown not yet delivered: nothing runs the
                // run loop between the post and the frame.
                let before = probe::wakes();
                post(view.waker());
                if probe::wakes() != before {
                    println!("wake delivered before the frame");
                    failed = true;
                }
                ticker.tick();
            }
            probe::pass(0.01);
            got.extend(probe::take());
        }
        // Whatever else is ready runs, so a second effect would be seen.
        for _ in 0..64 {
            probe::pass(0.0);
        }
        got.extend(probe::take());
        // A program's rows only: a click rings no bell.
        while let (Act::Type(_) | Act::TypeTwo(..) | Act::Gesture(..), Some(left)) =
            (&act, SPACING.checked_sub(start.elapsed()))
        {
            probe::pass(left.as_secs_f64());
        }
        got.extend(probe::take());
        if got.is_empty() {
            println!("none");
        }
        for r in &got {
            println!("{}", r.line());
        }
        // Nothing reaches the screen: the window is never shown, never
        // key, never in the Dock and never in full screen, after every
        // iteration -- the last two what a program's `CSI t` asks.
        let w = window.ns_window();
        if w.isVisible() {
            println!("the window was shown");
            failed = true;
        }
        if w.isKeyWindow() {
            println!("the window was made key");
            failed = true;
        }
        if w.isMiniaturized() {
            println!("the window was minimised");
            failed = true;
        }
        if w.styleMask().contains(NSWindowStyleMask::FullScreen) {
            println!("the window was put in full screen");
            failed = true;
        }
    }
    // No panic stopped anywhere: a borrow taken twice in a nested turn
    // (the effect queue's re-entry) would be one, swallowed by its guard.
    if baud_present_macos::unwind::caught() != baud_present_macos::unwind::Caught::NONE {
        println!("a panic was caught");
        failed = true;
    }
    match failed {
        true => std::process::ExitCode::from(1),
        false => std::process::ExitCode::SUCCESS,
    }
}

/// **The frame rig** (`rig_call::FrameRig`): the window made as the event
/// rig makes it and never shown; `setup` typed and read to its sign, the
/// clear of the pasteboard it ends with; the display link left to settle;
/// then `act` typed, if any, and the rig's ticks offered `attempts`
/// times, each delivered only while the door's state is `Run`
/// (`probe::Ticker`). Printed: the link's state at the end, the ticks
/// delivered and the door's sends over the offered ticks, the links made
/// in all, and the polls the last settle found still the tick's, and the
/// palettes applied over them.
/// **The window given the appearance it does not have**: dark for light,
/// light for dark, as a light/dark flip gives it. AppKit tells the view,
/// which tells its responder (`Responder::appearance`).
fn restyle(window: &NSWindow) {
    use macos_liquid_glass::objc2_app_kit::NSAppearance;
    let to = match macos_liquid_glass::is_dark(&window.effectiveAppearance()) {
        true => "NSAppearanceNameAqua",
        false => "NSAppearanceNameDarkAqua",
    };
    let named = NSAppearance::appearanceNamed(&NSString::from_str(to));
    window.setAppearance(named.as_deref());
}

fn run_frames(rig: probe_call::FrameRig) -> std::process::ExitCode {
    use probe_call::{RigFocus, RigHolders, RigKeys, RigWindow};
    let Some(mtm) = MainThreadMarker::new() else {
        return std::process::ExitCode::from(3);
    };
    probe::set_key(match rig.focus {
        RigFocus::Real => probe::Key::No,
        RigFocus::Believed => probe::Key::Yes,
    });
    probe::set_lift(match rig.holders {
        RigHolders::Kept => probe::Lift::Kept,
        RigHolders::Lifted => probe::Lift::Lifted,
    });
    let ns_app = NSApplication::sharedApplication(mtm);
    ns_app.setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
    crate::os_effect::install();
    let size = WINDOW_SIZE;
    let scale = 2.0;
    let inset = predicted_inset(mtm, size);
    let shell = Shell::start(scale, size, inset, None, None);
    let Opening {
        mut driver,
        view,
        window,
        ..
    } = Opening::new(mtm, size, scale, inset, shell, None);
    let id = crate::windows::Windows::<()>::new().open(());
    driver.window_id = Some(id);
    driver.app.os.name(id);
    // The first frame owed, as `show` owes it.
    driver.app.owe_tick();
    driver.pump(Trigger::Opening);
    view.set_responder(Box::new(baud_present_macos::unwind::Guarded(driver)));
    let _ = window.ns_window().makeFirstResponder(Some(&view));
    probe::set_waker(view.waker());
    let ticker = probe::Ticker::new(&view);
    // Each offered tick due a frame after now, on the display link's clock
    // (`probe::set_target`), as a real link's would be.
    let offer = |n: u16| {
        for _ in 0..n {
            let now = host_now().since(HostTime::ZERO).unwrap_or_default();
            probe::set_target((now + Duration::from_micros(8333)).as_secs_f64());
            ticker.tick();
            probe::pass(0.01);
        }
    };
    TYPED.with(|c| *c.borrow_mut() = Some([rig.setup.as_bytes(), b"\n"].concat()));
    post(view.waker());
    let start = Instant::now();
    let mut ready = false;
    while !ready && start.elapsed() < BOUND {
        offer(1);
        ready = probe::take()
            .iter()
            .any(|r| r.kind == probe::EffectKind::Clear);
    }
    if !ready {
        println!("the row's screen was never drawn");
        return std::process::ExitCode::from(1);
    }
    // Settled: what the setup owed drawn, and the link left as it then is.
    offer(30);
    let (_, sends) = probe::link();
    let delivered = probe::delivered();
    let drawn = probe::frames();
    let styled = probe::styled();
    match (rig.act.is_empty(), rig.keys) {
        (true, _) => {}
        (false, RigKeys::Line) => {
            TYPED.with(|c| *c.borrow_mut() = Some([rig.act.as_bytes(), b"\n"].concat()));
            post(view.waker());
        }
        (false, RigKeys::Pressed) => {
            PRESSED.with(|c| *c.borrow_mut() = Some(rig.act.clone()));
            post(view.waker());
        }
    }
    // The window told, after the act.
    match rig.window {
        RigWindow::Untold => {}
        RigWindow::Restyled => restyle(window.ns_window()),
    }
    offer(rig.attempts.0);
    let (link, sends_after) = probe::link();
    let holders = probe::holders();
    println!(
        "frames link={} delivered={} sends={} made={} drawn={} holders={} owed={} styled={}",
        match link {
            baud_present_macos::display_link::LinkState::Run => "run",
            baud_present_macos::display_link::LinkState::Pause => "pause",
        },
        probe::delivered().0.saturating_sub(delivered.0),
        sends_after.0.saturating_sub(sends.0),
        probe::made().0,
        probe::frames().value().saturating_sub(drawn.value()),
        match holders.is_empty() {
            true => "-".to_owned(),
            false => holders.join(","),
        },
        probe::owes().replace(' ', ""),
        probe::styled().0.saturating_sub(styled.0),
    );
    let _ = probe::take();
    let w = window.ns_window();
    let mut failed = false;
    if w.isVisible() || w.isKeyWindow() || w.isMiniaturized() {
        println!("the window was shown, made key or minimised");
        failed = true;
    }
    if baud_present_macos::unwind::caught() != baud_present_macos::unwind::Caught::NONE {
        println!("a panic was caught");
        failed = true;
    }
    match failed {
        true => std::process::ExitCode::from(1),
        false => std::process::ExitCode::SUCCESS,
    }
}
