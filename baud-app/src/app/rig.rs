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
//! next frame. `tick-split` drives the frame and splits each read
//! ([`split`]): a wake's turn takes none of a read and the next frame all
//! of it, so a green row's effects are asked inside `frame` and must still
//! each be made on a turn of its own.
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

/// Whether the rig splits each read: the `tick-split` arm.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Splits {
    No,
    Yes,
}

thread_local! {
    /// Whether reads are split, set once by the call.
    static SPLITS: core::cell::Cell<Splits> = const { core::cell::Cell::new(Splits::No) };
    /// What a wake's turn left of its reads, for the next frame.
    static TAIL: RefCell<Vec<crate::pty::Read>> = const { RefCell::new(Vec::new()) };
    /// How many frames have fed a read, what a wake left or what the frame
    /// drained itself: one at least each iteration, or the arm left nothing
    /// to a frame.
    static TAILS: core::cell::Cell<u32> = const { core::cell::Cell::new(0) };
}

/// **A read left to the frame, the way the event loop allows**, under
/// `tick-split`: a wake's turn takes none of what the reader sent, and the
/// next frame takes all of it, which `app` is owed so the link runs. The
/// tty hands one write back in pieces, and a wake that took
/// the first leaves the rest to whichever turn comes next, the display
/// link's whenever it runs; the prefix a wake takes is left empty here
/// because any other can end a sequence -- a BEL, the `\` of an ST, a CSI
/// final, each the first byte of a later piece -- and what that sequence
/// asks would then be asked in the callback, by timing. So whatever a read
/// asks is asked inside `frame`, every time, and every effect must still be
/// made on a turn of its own. Every other arm: the reads as drained.
pub(super) fn split(
    mut drained: crate::pty::Drained,
    trigger: Trigger,
    app: &mut App,
) -> crate::pty::Drained {
    if SPLITS.with(core::cell::Cell::get) == Splits::No {
        return drained;
    }
    match trigger {
        Trigger::Tick => {
            let tail = TAIL.with_borrow_mut(core::mem::take);
            drained.chunks.splice(0..0, tail);
            if !drained.chunks.is_empty() {
                TAILS.with(|t| t.set(t.get().saturating_add(1)));
            }
            drained
        }
        Trigger::Wake | Trigger::Opening => {
            TAIL.with_borrow_mut(|t| t.append(&mut drained.chunks));
            if TAIL.with_borrow(|t| !t.is_empty()) {
                app.owe_tick();
            }
            drained
        }
    }
}

thread_local! {
    /// Whether a decode's replies are held, set once by the call
    /// (`RigAct::TypeHeld`).
    static HOLDS: core::cell::Cell<Holds> = const { core::cell::Cell::new(Holds::No) };
    /// The reply held, when what is left of it may go, and whether its
    /// first half has gone.
    static HELD: RefCell<Option<(Vec<u8>, Instant, Halves)>> = const { RefCell::new(None) };
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Holds {
    No,
    Yes,
}

/// Whether the held reply's first half has gone.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Halves {
    Whole,
    Rest,
}

/// **What ends an iteration**: its effects, or [`BOUND`] passing; or its
/// effects or the program's sentinel, a clear, with
/// [`probe_call::SENTINEL_BACKSTOP`] a backstop that says it was reached
/// (`RigAct::TypeUntil`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ends {
    Bound,
    Sentinel,
}

thread_local! {
    /// What ends this call's iterations, set once by the call.
    static ENDS: core::cell::Cell<Ends> = const { core::cell::Cell::new(Ends::Bound) };
}

/// **The held arm's reply, taken from the terminal on a decode's turn**,
/// to go [`probe_call::HELD_REPLY`] from now: wall time passed on purpose,
/// as a loaded machine passes it, scheduled and never slept. When it may
/// go, for a wake then; `None` with no hold, or with one already waiting,
/// which this reply joins.
pub(super) fn hold_reply(app: &mut App) -> Option<Instant> {
    if HOLDS.with(core::cell::Cell::get) == Holds::No || app.term.reply().is_empty() {
        return None;
    }
    let bytes = app.term.reply().to_vec();
    app.term.clear_reply();
    HELD.with(|h| match &mut *h.borrow_mut() {
        Some((held, ..)) => {
            held.extend_from_slice(&bytes);
            None
        }
        slot @ None => {
            let at = Instant::now() + probe_call::HELD_REPLY;
            *slot = Some((bytes, at, Halves::Whole));
            Some(at)
        }
    })
}

/// What [`release_reply`] let go.
pub(super) enum Released {
    /// Nothing: no reply held, or its time not come.
    Nothing,
    /// The reply's first half, the rest to go at this time, for a wake
    /// then: a reply written in two pieces, as a tty may hand a raw read
    /// one before the other.
    Half(Instant),
    /// The rest, or the whole of a reply too short to halve.
    Rest,
}

/// **How long the held reply's second half waits on its first**: long
/// enough that a read woken by the first returns with it alone.
const HALVES_APART: Duration = Duration::from_millis(200);

/// **The held reply, once its time has come**, put where the next flush
/// writes it: its first half, then the rest [`HALVES_APART`] after.
pub(super) fn release_reply(app: &mut App) -> Released {
    HELD.with(|h| {
        let mut h = h.borrow_mut();
        let now = Instant::now();
        let Some((bytes, at, halves)) = h.as_mut() else {
            return Released::Nothing;
        };
        if now < *at {
            return Released::Nothing;
        }
        if *halves == Halves::Whole && bytes.len() >= 2 {
            let rest = bytes.split_off(bytes.len() / 2);
            app.outbox.push(bytes);
            *bytes = rest;
            *at = now + HALVES_APART;
            *halves = Halves::Rest;
            return Released::Half(*at);
        }
        app.outbox.push(bytes);
        *h = None;
        Released::Rest
    })
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

/// The bound on the setup's printed paths being answered: past the batch's
/// one-second ceiling (`probe::CEILING`) and a worker's answer.
const PATHS_BOUND: Duration = Duration::from_secs(5);

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
        RigAct::TypeUntil => {
            ENDS.with(|e| e.set(Ends::Sentinel));
            Act::Type(line())
        }
        RigAct::TypeHeld => {
            ENDS.with(|e| e.set(Ends::Sentinel));
            HOLDS.with(|h| h.set(Holds::Yes));
            Act::Type(line())
        }
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
        // With no tick, so the read that asks the expiry is drained by
        // the reader's wake, a callback, and never by a frame: a tick could
        // take what a wake left of a read the tty split, and the recorded
        // site would be the frame by timing.
        RigTicks::AfterInCallback => {
            probe::set_inline(probe::Inline::AfterInCallback);
            Ticks::None
        }
        RigTicks::Split => {
            SPLITS.with(|s| s.set(Splits::Yes));
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
        // A read the setup left to a frame is not this iteration's.
        TAILS.with(|t| t.set(0));
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
        // A row that ends on an event ends on its effects or the program's
        // sentinel, a clear, and its bound is a backstop that says so.
        let (bound, ended) = match ENDS.with(core::cell::Cell::get) {
            Ends::Bound => (BOUND, None),
            Ends::Sentinel => (
                probe_call::SENTINEL_BACKSTOP,
                Some(probe::EffectKind::Clear),
            ),
        };
        let over = |got: &[probe::Record]| {
            holds(got, each) || ended.is_some_and(|k| got.iter().any(|r| r.kind == k))
        };
        while !over(&got) && start.elapsed() < bound {
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
        // The sentinel says the program ended; it is no effect of the
        // row's, so it is not printed. An iteration it ended without the
        // effects is unanswered, and one nothing ended timed out.
        let sentinel = ended.is_some_and(|k| got.iter().any(|r| r.kind == k));
        if let Some(k) = ended {
            got.retain(|r| r.kind != k);
        }
        match (ended, holds(&got, each), sentinel) {
            (Some(_), false, true) => println!("unanswered"),
            (Some(_), false, false) => println!("timed out at the backstop"),
            _ if got.is_empty() => println!("none"),
            _ => {}
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
        // The split arm's own control: a frame fed a read this iteration.
        if SPLITS.with(core::cell::Cell::get) == Splits::Yes {
            let tails = TAILS.with(core::cell::Cell::get);
            if tails == 0 {
                println!("no read was fed in a frame");
                failed = true;
            }
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

/// **The frame rig** (`rig_call::FrameRig`): the window made as the event
/// rig makes it and never shown; `setup` typed and read to its sign, the
/// clear of the pasteboard it ends with; the display link left to settle;
/// then `act` sent, if any, as `keys` says, the window told what `window`
/// says, and the rig's ticks offered `attempts` times, each delivered
/// only while the door's state is `Run` (`probe::Ticker`). Printed: the link's state at the end, the ticks
/// delivered and the door's sends over the offered ticks, the links made
/// in all, the polls the last settle found still the tick's, what it
/// found owed, the palettes applied over the ticks, the focus baud last
/// set, the display options' changes taken, and what the last read found
/// under the pointer.
fn run_frames(rig: probe_call::FrameRig) -> std::process::ExitCode {
    use probe_call::{RigFocus, RigFrameTicks, RigHolders, RigKeys, RigWindow};
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
    // Reduce Motion and Reduce Transparency off, whatever this Mac is set
    // to: every row starts from the rig's display options, not the
    // machine's (`probe::Access`).
    probe::set_access(probe::Access::Set {
        motion: probe::Turned::Off,
        transparency: probe::Turned::Off,
    });
    let ns_app = NSApplication::sharedApplication(mtm);
    ns_app.setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
    crate::os_effect::install();
    let size = WINDOW_SIZE;
    let scale = 2.0;
    let inset = predicted_inset(mtm, size);
    // **The reverse lookup stalled** (the rig's seam, `probe::HostName`):
    // the window's own read of every name (`Hosts`) runs on its thread, and
    // its `NSProcessInfo.hostName`, whose pace is the network's, is held
    // until a row answers it (`RigWindow::HostNamed`). Every other row runs
    // on `gethostname`'s name, given at opening, alone: none waits on the
    // network, and no hand of the read's names lands after a settle to owe
    // the hover a read.
    probe::set_host_name(probe::HostName::Stalled);
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
    // Where `PointerOver` moves the pointer: the first row's second cell.
    let over = driver.rig_point(Row::ZERO, Col::at_index(1));
    // `CommandExits`'s child, handed over as a click's command is, its
    // input kept here until the act closes it.
    let mut command_input = match rig.window {
        RigWindow::CommandExits => {
            let mut sh = std::process::Command::new("/bin/sh");
            sh.args(["-c", "read x"])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            match sh.spawn() {
                Ok(mut child) => {
                    let input = child.stdin.take();
                    driver.reap_command(child);
                    input
                }
                Err(_) => {
                    println!("the command's child would not start");
                    return std::process::ExitCode::from(1);
                }
            }
        }
        _ => None,
    };
    // The view made first responder before its responder is set, as
    // `show` does: the view's hook then tells no one, and the focus the
    // window opens with is the opening pump's reading (`KeyHeard`).
    let _ = window.ns_window().makeFirstResponder(Some(&view));
    view.set_responder(Box::new(baud_present_macos::unwind::Guarded(driver)));
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
    // **The setup's printed paths answered**: its batch goes at its
    // deadline, as late as a second after the last, and its answer may owe
    // a frame, both the setup's work and not the act's; waited for, within
    // the bound, on the run loop.
    let start = Instant::now();
    while probe::paths_due() == probe::PathsDue::Owed && start.elapsed() < PATHS_BOUND {
        offer(1);
    }
    if probe::paths_due() == probe::PathsDue::Owed {
        println!("the setup's paths were never answered");
        return std::process::ExitCode::from(1);
    }
    // Settled: what the setup owed drawn, and the link left as it then is.
    offer(30);
    let (_, sends) = probe::link();
    let delivered = probe::delivered();
    let drawn = probe::frames();
    let styled = probe::styled();
    let reads = probe::hover_reads();
    let fed = probe::fed_turns();
    let hands = probe::hands();
    let greetings = probe::greetings();
    let batches = probe::paths_batches();
    let followed = || {
        [
            probe::Followed::Scroller,
            probe::Followed::Flashing,
            probe::Followed::Animated,
        ]
        .map(|s| probe::followed(s).0)
    };
    let followed_before = followed();
    let (motions, displays) = (
        probe::taken(probe::DisplayOption::Motion),
        probe::taken(probe::DisplayOption::Display),
    );
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
    // The window told, after the act: a key change with the focus door
    // set to agree, then the notification AppKit would post.
    match rig.window {
        RigWindow::Untold => {}
        RigWindow::Restyled => restyle(window.ns_window()),
        RigWindow::BecameKey => probe::set_key(probe::Key::Yes),
        RigWindow::ResignedKey => probe::set_key(probe::Key::No),
        RigWindow::Refocused => {
            let w = window.ns_window();
            let _ = w.makeFirstResponder(None);
            let _ = w.makeFirstResponder(Some(&view));
        }
        RigWindow::DisplayOptions => {
            probe::set_access(probe::Access::Set {
                motion: probe::Turned::On,
                transparency: probe::Turned::On,
            });
            baud_present_macos::settings::rig_post_display_options();
        }
        RigWindow::PointerOver => {
            probe::press(&view, probe::Press::Moved, over, probe::Chord::Plain)
        }
        // Its input closed, `read` ends and the shell with it.
        RigWindow::CommandExits => drop(command_input.take()),
        RigWindow::Settings => baud_present_macos::settings::rig_post_settings(),
        RigWindow::Shown => probe::set_shown(probe::Shown::Yes),
        // The stalled lookup answered with a name `gethostname` does not
        // give, so the read adds one and its names are owed to the links.
        RigWindow::HostNamed => {
            probe::set_host_name(probe::HostName::Answered("baud-rig.invalid".to_owned()))
        }
    }
    baud_present_macos::closing::rig_post(window.ns_window(), rig.window);
    // The run: `attempts` ticks offered, or the run loop run as long with
    // no tick, the act's own turns.
    let run = |n: u16| match rig.ticks {
        RigFrameTicks::Offered => offer(n),
        RigFrameTicks::Withheld => {
            for _ in 0..n {
                probe::pass(0.01);
            }
        }
    };
    run(rig.attempts.0);
    // **An act that ends by clearing the pasteboard is waited for**, as the
    // setup is: the run goes on until that clear, within the bound, and
    // then as long as the setup's settle, so what the act wrote is all fed
    // and drawn whatever the machine's pace, and the row's counts are the
    // whole act's.
    if rig.act.contains("]52;c;!") {
        let start = Instant::now();
        let mut signed = probe::take()
            .iter()
            .any(|r| r.kind == probe::EffectKind::Clear);
        while !signed && start.elapsed() < BOUND {
            run(1);
            signed = probe::take()
                .iter()
                .any(|r| r.kind == probe::EffectKind::Clear);
        }
        if !signed {
            println!("the act never finished");
            return std::process::ExitCode::from(1);
        }
        run(30);
    }
    let (link, sends_after) = probe::link();
    let holders = probe::holders();
    println!(
        "frames link={} delivered={} sends={} made={} drawn={} holders={} owed={} styled={} focus={} motion={} display={} preview={} reads={} fed={} reaped={} followed={} hands={} greeted={} batches={}",
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
        match probe::focus() {
            probe::Focused::Unset => "unset",
            probe::Focused::Gained => "gained",
            probe::Focused::Lost => "lost",
        },
        probe::taken(probe::DisplayOption::Motion)
            .0
            .saturating_sub(motions.0),
        probe::taken(probe::DisplayOption::Display)
            .0
            .saturating_sub(displays.0),
        match probe::preview() {
            probe::Previewed::Unset => "unset",
            probe::Previewed::Link => "link",
            probe::Previewed::Nothing => "none",
        },
        probe::hover_reads().0.saturating_sub(reads.0),
        probe::fed_turns().0.saturating_sub(fed.0),
        // Marked by a watch's thread; no command is handed over before
        // the act, so any reap is the act's.
        match crate::fg::a_command_reaped() {
            crate::fg::Reaped::Now => 1,
            crate::fg::Reaped::Not => 0,
        },
        // Each followed setting's takes since the settle: scroller,
        // flashing, animated.
        {
            let now = followed();
            format!(
                "{},{},{}",
                now[0].saturating_sub(followed_before[0]),
                now[1].saturating_sub(followed_before[1]),
                now[2].saturating_sub(followed_before[2]),
            )
        },
        probe::hands().0.saturating_sub(hands.0),
        probe::greetings().0.saturating_sub(greetings.0),
        probe::paths_batches().0.saturating_sub(batches.0),
    );
    let _ = probe::take();
    let w = window.ns_window();
    // The window's title, on a line of its own: it may hold spaces.
    println!("title {}", w.title());
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
