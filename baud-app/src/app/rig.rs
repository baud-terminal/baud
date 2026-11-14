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

use super::census_judge::{Bill, Counted, Fed, Pumping, Witnessed, Woke};
use super::quiet_judge::Scope;
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
/// (`probe::press`, `probe::menu`, `probe::key_down`).
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
    /// Return pressed, sent to the view's own `keyDown:`, outside any
    /// turn the rig drives: a key that ends a one-shot ReGIS input reaches
    /// the core through the key's own funnel (`App::send`), with no pump
    /// of the rig's to feed what the input held ([`After::Alone`]).
    Key,
    /// The left button pressed and let go on the first cell, sent to the
    /// view's own `mouseDown:` and `mouseUp:`, outside any turn the rig
    /// drives: a press that ends a one-shot ReGIS input reaches the core
    /// through the button's own funnel (`Driver::locator_owed`), with no
    /// pump of the rig's to feed what the input held ([`After::Alone`]).
    Button,
}

/// **Whether the rig posts a wake of its own after an act** ([`post`]).
/// Its turn pumps, and a pump feeds whatever a one-shot ReGIS input held
/// once the input has ended (`reads::paced`), so after an act that ends
/// one it would feed the held reads whether or not the product asked a
/// wake for them: a row of [`Gesture::Key`] or [`Gesture::Button`] proves
/// the product's own wake (`App::input_released`) only with none.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum After {
    /// The rig's wake posted after the act, so its turn runs what the act
    /// left: every act but those below.
    Posted,
    /// Nothing of the rig's after the act: what it owes runs on the
    /// product's own wakes or not at all.
    Alone,
}

impl Act {
    /// What the rig does after the act ([`After`]).
    fn after(&self) -> After {
        match self {
            Act::Gesture(Gesture::Key | Gesture::Button, _) => After::Alone,
            Act::Click(_) | Act::Type(_) | Act::Gesture(..) | Act::TypeTwo(..) => After::Posted,
        }
    }
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

/// **The link run while a read waits for the frame** ([`split`]), at
/// every settle: the turn that left it fed nothing and owes no frame, so
/// `frames::owes` finds nothing the read is owed, and the settle would
/// pause the link its `owe_tick` ran -- `Ticker::tick` then sends nothing,
/// and a row with no deadline of its own (the caret steady, the lights
/// off) leaves the read there for good, "a read left to a frame no tick
/// fed". Every other arm leaves nothing here, and the settle as it was.
pub(super) fn held_for_split(act: crate::frames::Act) -> crate::frames::Act {
    match TAIL.with_borrow(Vec::is_empty) {
        true => act,
        false => crate::frames::Act::Run,
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

/// **What ends an iteration**: its effects, or [`BOUND`] passing; its
/// effects or the program's sentinel, a clear (`RigAct::TypeUntil`); or
/// the sentinel alone, its effects checked after it
/// (`RigAct::TypeToSentinel`). Ending on an event,
/// [`probe_call::SENTINEL_BACKSTOP`] is a backstop that says it was
/// reached.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ends {
    Bound,
    EffectsOrSentinel,
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

/// The line a `wake` sends the shell, if the rig queued one: heard for the
/// census as the bytes its turn writes ([`Carrier::Typed`]).
pub(super) fn take_typed() -> Option<Vec<u8>> {
    let line = TYPED.with(|c| c.borrow_mut().take());
    if line.is_some() {
        heard(Carrier::Typed);
    }
    line
}

/// The characters a `wake` sends as key presses, if the rig queued them.
pub(super) fn take_pressed() -> Option<String> {
    PRESSED.with(|c| c.borrow_mut().take())
}

/// **A geometry change the frame rig asks of the window**, taken by the
/// next `wake` as the View menu's zoom and a window resize are: a zoom in
/// one point (`Driver::zoom`), or the view made wider by [`WIDER`] points
/// (`Driver::relayout`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Reshape {
    ZoomIn,
    Wider,
}

/// How much wider [`Reshape::Wider`] makes the view, in points.
pub(super) const WIDER: f64 = 40.0;

thread_local! {
    /// The geometry change the next `wake` makes.
    static RESHAPE: core::cell::Cell<Option<Reshape>> = const { core::cell::Cell::new(None) };
    /// The window's font size and view at its last settle, as the frame
    /// rig reads them back.
    static SAW: core::cell::Cell<Option<(f32, (Px, Px))>> = const { core::cell::Cell::new(None) };
    /// The screen's text when the window gave the GPU up: what its first
    /// frame on the CPU drew.
    static SWITCH_SCREEN: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// The geometry change a `wake` makes, if the rig asked for one.
pub(super) fn take_reshape() -> Option<Reshape> {
    RESHAPE.with(core::cell::Cell::take)
}

/// The window's font size in points and its view in pixels, as a settle
/// found them.
pub(super) fn saw(points: f32, view: (Px, Px)) {
    SAW.with(|s| s.set(Some((points, view))));
}

/// The screen's text as the window gave the GPU up.
pub(super) fn switch_screen(text: String) {
    SWITCH_SCREEN.with(|s| *s.borrow_mut() = Some(text));
}

/// **The window's text**, a line a row, each cell's base read through the
/// cluster store and a cell with none a space.
pub(super) fn screen_text(t: &Term<'_>) -> String {
    let v = t.window();
    (0..v.rows().index())
        .map(|y| {
            v.row(Row::from_index(y))
                .iter()
                .map(|c| {
                    t.base_of(*c)
                        .as_codepoint()
                        .and_then(|p| char::from_u32(p.value()))
                        .unwrap_or(' ')
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The bound on one iteration: reaching it is that iteration's failure.
const BOUND: Duration = Duration::from_secs(2);

/// The bound on the setup's printed paths being answered and its lights
/// settling: past the batch's one-second ceiling (`probe::CEILING`) and a
/// worker's answer, and far past a light's hold and dwell.
const PATHS_BOUND: Duration = Duration::from_secs(5);

/// The bound on a row's first stage settling before its second is
/// released (`RigWindow::Released`): past the paths' bound and a hang's
/// five seconds (`probe::HANG`), whose wake, scheduled for a batch since
/// answered, still turns once.
const RELEASE_BOUND: Duration = Duration::from_secs(15);

/// **The run loop's time between one iteration's records and the next
/// iteration**: longer than the frame interval within which bells ring
/// once (`bell.rs`, `Ringer::hear`), so a bell each iteration is a ring
/// each iteration. The view keeps the interval it starts with, 8.333 ms,
/// on every arm: the hand-driven link's target never advances, so the
/// tick clock measures no interval of its own. Counted from the records,
/// not from the iteration's start: a ring later than this floor ended its
/// iteration with nothing left of it, the next line went out at once, and
/// its bell fell inside the frame of the last ring and was absorbed, a
/// raise lost to the rig and not to the terminal. A typed line's rows
/// only. A floor on time passed, never a wait for something to happen:
/// what every iteration waits for is its records, under [`BOUND`].
const SPACING: Duration = Duration::from_millis(50);

/// **A frame row's second stage** (`RigWindow::Released`): none asked
/// for; due at a time, and written on the first pass after it that finds
/// `go` open -- before the run when due at once, on the run's passes when
/// late; written, with when, and the turns fed, the paths asked about, the
/// batches sent and those by why each was due just before it, from which
/// the run counts what the act wrote after it; or never read, `go` never
/// opened within [`PATHS_BOUND`] of its time.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Second {
    Unasked,
    Due(Instant),
    Written {
        at: Instant,
        fed: probe::FedTurns,
        asked: probe::Taken,
        batches: probe::Taken,
        by: probe::PathsBy,
    },
    Unread,
}

/// The longest pass of the floor's run loop between two ticks on a driven
/// arm, as long as the wait's own passes.
const TICK: Duration = Duration::from_millis(10);

/// A 60 Hz display's frame, the longest a display link of the screens
/// baud is drawn on steps by: the floor is past it whatever the view's
/// interval, and each read's own is checked against it besides
/// ([`heard_under`]).
const _: () = assert!(SPACING.as_micros() > 16_667);

thread_local! {
    /// The longest frame interval the ringer heard a read's bells under
    /// this iteration (`Ringer::hear`'s `frame`), for the check against
    /// [`SPACING`].
    static HEARD: core::cell::Cell<Duration> = const { core::cell::Cell::new(Duration::ZERO) };
}

/// **The frame interval a read's bells are heard under**, the very value
/// the driver hands `Ringer::hear`, the longest kept for the iteration: a
/// bell within it of the last ring rings with it once, so an interval at
/// or past the floor between iterations would let one iteration's bell be
/// absorbed by the last's.
pub(super) fn heard_under(frame: Duration) {
    HEARD.with(|h| h.set(h.get().max(frame)));
}

thread_local! {
    /// How many of the run's writes to the child the kernel took in part
    /// or not at all (`Wrote::Part`, `Wrote::Nothing`).
    static SHORT: core::cell::Cell<u32> = const { core::cell::Cell::new(0) };
    /// How many turns the census counted ([`carried`]) with the master's
    /// room among what woke them ([`Carrier::Room`]): turns that wrote
    /// owed bytes on the room's event.
    static ROOM: core::cell::Cell<u32> = const { core::cell::Cell::new(0) };
}

/// **A write the kernel took in part or not at all**, as `flush_reply`
/// met it: the witness that a row's replies filled the child's input
/// queue. Printed once, at the run's end, as `short writes <n>`, beside
/// the turns the master's room woke to write the rest (`room turns <n>`,
/// [`carried`]): with both, an undriven arm's pass says the room
/// (`pty::OutboxWatch`) carried bytes, however its turns fell in time; a
/// driven arm's tick may carry them first, and its room count is a record.
pub(super) fn short_write() {
    SHORT.with(|s| s.set(s.get().saturating_add(1)));
}

/// **What woke a turn that writes bytes it fed no read for**: the
/// census of what carries owed bytes on (`carried`), printed at the run's
/// end beside the short writes. Each is heard where it wakes the window,
/// on the main thread, and taken by the next turn ([`turn`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Carrier {
    /// The master's room, heard by `pty::OutboxWatch`.
    Room,
    /// A frame's completion: its surface presented, its wake after.
    Landed,
    /// A wake posted for a moment (`wake_holding`), by where it was asked.
    Scheduled(&'static core::panic::Location<'static>),
    /// The rig's own wake ([`post`]).
    Posted,
    /// The rig's line to the shell, written on the turn that takes it
    /// ([`take_typed`]).
    Typed,
    /// The wake an ended one-shot ReGIS input asks for
    /// (`App::input_released`), heard by its type where its callback lets
    /// it go (`InputEnded`), beside where it was asked: what a row whose
    /// act the rig leaves alone is judged by ([`released`]).
    Released,
}

impl Carrier {
    fn name(self) -> String {
        match self {
            Carrier::Room => "room".to_owned(),
            Carrier::Landed => "landed".to_owned(),
            Carrier::Scheduled(at) => format!("scheduled at {}:{}", at.file(), at.line()),
            Carrier::Posted => "posted".to_owned(),
            Carrier::Typed => "typed".to_owned(),
            Carrier::Released => "released".to_owned(),
        }
    }

    /// What the release's witness reads of a carrier: its type, never
    /// where it was asked (`census_judge::released_by_the_act`).
    fn woke(self) -> Woke {
        match self {
            Carrier::Released => Woke::Released,
            Carrier::Room
            | Carrier::Landed
            | Carrier::Scheduled(_)
            | Carrier::Posted
            | Carrier::Typed => Woke::Other,
        }
    }
}

/// The turn running: what woke it, whether it fed a read, and whether it
/// is counted.
struct Turn {
    woke: Vec<Carrier>,
    fed: Fed,
    counted: Counted,
}

thread_local! {
    /// What was heard since the last turn began.
    static WOKE: RefCell<Vec<Carrier>> = const { RefCell::new(Vec::new()) };
    /// The turn running.
    static TURN: RefCell<Turn> = const {
        RefCell::new(Turn {
            woke: Vec::new(),
            fed: Fed::No,
            counted: Counted::Yes,
        })
    };
    /// Each counted turn's carriers, named and joined, the run's census.
    static CARRIED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// Whether a pump's census turn runs: from [`turn`] to [`turn_end`].
    static IN_PUMP: core::cell::Cell<Pumping> = const { core::cell::Cell::new(Pumping::No) };
    /// What woke each turn that fed held reads and no read of its own
    /// ([`released`]), the release census.
    static RELEASED: RefCell<Vec<Vec<Carrier>>> = const { RefCell::new(Vec::new()) };
    /// **What woke the window while the rig waited before an act it leaves
    /// alone** ([`still_before_the_act`]), none outside that wait: named on
    /// the line of a wait that never ends ([`waited`]), so a wake that
    /// renews itself says where it was asked.
    static WAITED: RefCell<Option<Vec<Carrier>>> = const { RefCell::new(None) };
}

/// **A wake's source, heard** as it wakes the window ([`Carrier`]), and
/// kept for the wait before an act left alone while it runs ([`WAITED`]).
pub(super) fn heard(by: Carrier) {
    WOKE.with_borrow_mut(|w| w.push(by));
    WAITED.with_borrow_mut(|w| {
        if let Some(waited) = w {
            waited.push(by);
        }
    });
}

/// **A turn begins its feed** (`pump`): what woke it, taken, and whether
/// it feeds a read -- a read's turn writes the replies it fed, and is no
/// carrier.
pub(super) fn turn(drained: &crate::pty::Drained) {
    let woke = WOKE.with_borrow_mut(core::mem::take);
    let fed = match drained.chunks.is_empty() {
        true => Fed::No,
        false => Fed::Yes,
    };
    TURN.with_borrow_mut(|t| {
        *t = Turn {
            woke,
            fed,
            counted: Counted::No,
        }
    });
    IN_PUMP.with(|p| p.set(Pumping::Yes));
}

/// **A pump's census turn ends** (`pump`'s last call): a write from here
/// to the next [`turn`] is made outside any pump, by a responder method
/// ([`carried`]).
pub(super) fn turn_end() {
    IN_PUMP.with(|p| p.set(Pumping::No));
}

/// **A write of bytes no read on this turn fed** (`flush_reply`), billed
/// by `census_judge::bill`: inside a pump, to the turn's carriers, once a
/// turn, and the turn counted in `ROOM` as well when the master's room is
/// among them; outside one, to `responder`, every time, the running turn
/// left as it was -- an act's own report, written in the responder method
/// that ended a one-shot ReGIS input (`App::send`, `Driver::locator_owed`),
/// was billed to whichever pump ran last before it, and read as a turn
/// that carried it. A write a pump makes before its turn begins (a focus
/// report, a zoom's size) is billed to `responder` too.
pub(super) fn carried() {
    let pumping = IN_PUMP.with(core::cell::Cell::get);
    TURN.with_borrow_mut(
        |t| match super::census_judge::bill(pumping, t.fed, t.counted) {
            Bill::Nothing => {}
            Bill::Responder => CARRIED.with_borrow_mut(|c| c.push("responder".to_owned())),
            Bill::Turn => {
                t.counted = Counted::Yes;
                CARRIED.with_borrow_mut(|c| c.push(named(&t.woke)));
                if t.woke.contains(&Carrier::Room) {
                    ROOM.with(|r| r.set(r.get().saturating_add(1)));
                }
            }
        },
    );
}

/// **A turn that fed held reads and no read of its own** (`pump`, after
/// `reads::paced`): a ReGIS delay's deadline come, or a one-shot input an
/// event ended. What woke it is kept for the release census, printed at
/// the run's end as `released by`, and judged for a row whose act the rig
/// leaves alone ([`After::Alone`]). A release on a read's own turn is not
/// counted: neither row that judges it reads anything after its act.
pub(super) fn released() {
    let woke = TURN.with_borrow(|t| t.woke.clone());
    RELEASED.with_borrow_mut(|r| r.push(woke));
}

/// A turn's carriers by their names, sorted and joined with `+`;
/// "nothing heard" when no source the census hears woke it.
fn named(woke: &[Carrier]) -> String {
    let mut names: Vec<String> = woke.iter().copied().map(Carrier::name).collect();
    names.sort();
    names.dedup();
    match names.is_empty() {
        true => "nothing heard".to_owned(),
        false => names.join("+"),
    }
}

/// **A census, one line**: the turns in it, then each set of carriers
/// with how many turns it woke, in the order first met.
fn tally(all: &[String]) -> String {
    format!("{} turns: {}", all.len(), each(all))
}

/// Each name in `all` with how many times it is there, in the order first
/// met, joined with `, `.
fn each(all: &[String]) -> String {
    let mut kinds: Vec<&String> = Vec::new();
    for k in all {
        if !kinds.contains(&k) {
            kinds.push(k);
        }
    }
    let counted: Vec<String> = kinds
        .iter()
        .map(|k| format!("{k} {}", all.iter().filter(|n| n == k).count()))
        .collect();
    counted.join(", ")
}

/// **What woke the window while the rig waited before an act it leaves
/// alone** ([`WAITED`]), the wait over and the record dropped: each source
/// by name with how many times it was heard, in the order first heard, or
/// "nothing heard".
fn waited() -> String {
    let heard: Vec<String> = WAITED
        .with_borrow_mut(Option::take)
        .unwrap_or_default()
        .into_iter()
        .map(Carrier::name)
        .collect();
    match heard.is_empty() {
        true => "nothing heard".to_owned(),
        false => each(&heard),
    }
}

/// The census of what carried bytes no read fed ([`carried`]).
fn census() -> String {
    CARRIED.with_borrow(|all| tally(all.as_slice()))
}

/// The release census: what woke each turn that fed held reads alone
/// ([`released`]).
fn release_census() -> String {
    RELEASED.with_borrow(|turns| {
        tally(
            &turns
                .iter()
                .map(|w| named(w.as_slice()))
                .collect::<Vec<_>>(),
        )
    })
}

/// **Whether the held reads went on on the act's own wake**: every turn
/// that fed held reads alone was woken by `App::input_released`'s, read
/// by the carrier's type (`census_judge::released_by_the_act`).
fn released_by_the_act() -> Witnessed {
    let turns: Vec<Vec<Woke>> = RELEASED.with_borrow(|turns| {
        turns
            .iter()
            .map(|w| w.iter().copied().map(Carrier::woke).collect())
            .collect()
    });
    super::census_judge::released_by_the_act(&turns)
}

/// **The wakes posted for a moment and not yet run** (`wake_holding`):
/// one more as each is posted, one fewer in its callback before what it
/// holds is let go, or at once for a post the main queue refused. A wake
/// whose callback posts another (`FrameDue` early, or overdue and so owing
/// its give-up) is counted through, the second post after the first's
/// count is taken off. What goes through `wake_holding` -- a frame's
/// bounds, the printed paths' worker's deadline and hang, a notice's
/// expiry wake, a held read's deadline, a title's pace, a paused link's
/// restart, the config's settle -- is none of it still to come while this
/// reads zero -- but for the `wake:` a callback's turn is brought by: the
/// callback posts it to the main run loop as it returns, after its count
/// is off, so zero says only that it is posted. [`quiet_at`]'s barrier
/// waits for that. A block posted through `baud-present-macos`'s
/// `turn::after` or `turn::later` is not counted here ([`quiet`] names
/// them). Atomic, though every post and callback found is the main
/// thread's, so the count holds wherever one is posted from; a callback
/// with no post wraps it past any count a run reaches, and quiet never
/// comes.
static WAKES_IN_FLIGHT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// **The wakes' callbacks run, ever** ([`wake_done`]): a barrier posted at
/// zero ([`quiet_at`]) stands only while this has not moved, since a
/// callback run after it posts its `wake:` behind the barrier.
static WAKES_RUN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// **The wakes posted for a moment, ever** ([`wake_posted`]): read before
/// and after an act the rig leaves alone, the difference the wakes the act
/// asked (`asked by the act`).
static WAKES_POSTED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// A wake posted for a moment ([`WAKES_IN_FLIGHT`], [`WAKES_POSTED`]).
pub(super) fn wake_posted() {
    WAKES_IN_FLIGHT.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    WAKES_POSTED.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
}

/// A wake posted for a moment run, or never posted ([`WAKES_IN_FLIGHT`],
/// [`WAKES_RUN`]).
pub(super) fn wake_done() {
    WAKES_IN_FLIGHT.fetch_sub(1, std::sync::atomic::Ordering::AcqRel);
    WAKES_RUN.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
}

/// **What quiet still waits for** ([`quiet`]), the first part not yet
/// met, named on the backstop's line: `quiet_judge`'s, over the row's
/// event and the printed paths' worker as the probe reads them.
type Unquiet = super::quiet_judge::Unquiet<probe_call::RigEvent, probe::PathsDue>;

/// **Whether the act ends by clearing the pasteboard** (`]52;c;!`, as
/// [`READY`] does): read once from the act, for every step that waits for
/// that clear.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Clears {
    Yes,
    No,
}

impl Clears {
    fn of(act: &str) -> Clears {
        match act.contains("]52;c;!") {
            true => Clears::Yes,
            false => Clears::No,
        }
    }
}

/// **Whether the act's clear has been read**, or it has none to read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Signed {
    Yes,
    No,
}

/// **Quiet: nothing more can happen without an outside event**
/// (`probe_call::RigEnd`), the row's `event` seen first: the link paused;
/// no frame owed and the last settle owing nothing, not even a restart at
/// a deadline; no wake posted for a moment still to run
/// ([`WAKES_IN_FLIGHT`]); the printed paths' worker idle, no batch due,
/// out or with its wake to come; and the act's clear read, if it has one
/// ([`Signed`]), so the shell's output is over. An act with no clear -- none
/// at all, or one that leaves a job running -- has nothing of its own to
/// wait for here: the run's quiet is then the window's. Every part is read
/// here and judged by `quiet_judge::judge`, whose tests every test build
/// runs.
///
/// **The producers that wake the window directly**, with no wake posted
/// for a moment, and what of theirs quiet counts:
/// - the printed paths' worker (`Probes::system`): a batch due or out is
///   [`probe::PathsDue::Owed`], the worker's answer to it among what is
///   out, and its scheduled wakes `Waking`;
/// - a wake posted on the main thread inside a turn -- a frame on the layer
///   while a splash waits (`layer_presented`), a gone link the pointer
///   came over (`recheck_entered`), the effect queue's runner done -- is a
///   `wake:` posted before any barrier [`quiet_at`] posts after it, and is
///   performed first;
/// - **no in-flight count**, each a finding to add before a row that
///   reaches it after its event ends on quiet: from a thread of its own,
///   the pty's reader (`Pty::spawn`'s wake), the process watch
///   (`pty.watch`, which the derived title and the reaping hear), the read
///   of this machine's names (`Hosts`, which the rig stalls but on
///   `RigWindow::HostNamed`), the plasma's build (`build_plasma`, the GPU's
///   alone, once the window is shown), the printer list, the attachments'
///   checker (`notify::Checker`) and a GPU frame's completion; as main-queue
///   blocks, `turn::soon` (the effect queue's runner), `turn::after` (a
///   notice's removal at its expiry) and `turn::later`, and the outbox's
///   room and the config's dispatch sources; and the host log's writer at
///   a close, the notification list's answer (`notify::listen`), the
///   launch's bound (`baud-present-macos`), and the hover's, the settings'
///   and the window's own observers, which AppKit calls.
///
/// A frame's completion first: a frame row with no `BAUD_GPU` draws on
/// the GPU where there is one, whose completion counts the frame out of
/// flight before it posts the surface and the `wake:` from the GPU's
/// thread, and a barrier posted from the main thread is ordered after
/// neither. So no such row ends on quiet until a frame in flight is a
/// part of it; a row on the CPU path (`BAUD_GPU=cpu`) has no completion.
///
/// **Before an act the rig leaves alone** ([`Scope::BeforeAct`],
/// [`still_before_the_act`]) the same reads are judged for what could pump
/// after the act instead: the wakes in flight and the printed paths'
/// worker, as above; the effect queue's turn not posted
/// (`os_effect::due`), a main-queue block with no count; and no frame ever
/// drawn on the GPU (`probe::gpu_committed`), the row on the CPU path, so
/// no completion can wake the window. The link and a frame owed are not
/// judged there: neither pumps.
fn quiet(event: probe_call::RigEvent, signed: Signed, scope: Scope) -> Result<(), Unquiet> {
    let seen = match event {
        probe_call::RigEvent::Settled => true,
        probe_call::RigEvent::AliveAnswered => {
            probe::alive_answered() != probe::AliveAnswered::Unanswered
        }
    };
    // A part of quiet, as read: met or not.
    let met = |yes: bool| match yes {
        true => super::quiet_judge::Met::Yes,
        false => super::quiet_judge::Met::No,
    };
    super::quiet_judge::judge(super::quiet_judge::Parts {
        scope,
        event: match seen {
            true => Ok(()),
            false => Err(event),
        },
        paused: met(probe::link().0 == baud_present_macos::display_link::LinkState::Pause),
        owed_nothing: met(probe::frame_owed() == probe::FrameOwed::No && probe::owes() == "Nothing"),
        on_cpu: met(probe::gpu_committed() == baud_render::Tally::ZERO),
        effects: met(crate::os_effect::due() == crate::os_effect::Due::No),
        wakes: super::quiet_judge::InFlight(
            WAKES_IN_FLIGHT.load(std::sync::atomic::Ordering::Acquire),
        ),
        paths: match probe::paths_due() {
            probe::PathsDue::Idle => Ok(()),
            due @ (probe::PathsDue::Waking | probe::PathsDue::Owed) => Err(due),
        },
        signed: met(signed == Signed::Yes),
    })
}

/// **Quiet, closed by a barrier** ([`quiet`]): every part read and judged
/// again on each call, then the wakes' callbacks run, both handed to
/// `quiet_judge::closed`, whose rules post a barrier through the main
/// thread's performs ([`probe::barrier`]) at zero and come to quiet only
/// once it is performed ([`probe::reached`]) with no wake's callback run
/// since it was posted -- or, a responder busy when the callback's
/// `wake:` landed, after the call it folded into returned.
fn quiet_at(
    event: probe_call::RigEvent,
    signed: Signed,
    scope: Scope,
    at: &mut Option<super::quiet_judge::AtQuiet<probe::Barrier>>,
) -> Result<(), Unquiet> {
    super::quiet_judge::closed(
        quiet(event, signed, scope),
        super::quiet_judge::WakesRun(WAKES_RUN.load(std::sync::atomic::Ordering::Acquire)),
        at,
        |barrier| match probe::reached(barrier) {
            probe::Reached::Yes => super::quiet_judge::Reached::Yes,
            probe::Reached::No => super::quiet_judge::Reached::No,
        },
        probe::barrier,
    )
}

/// **Still before an act the rig leaves alone** ([`After::Alone`]):
/// passes of the run loop until nothing that could pump after the act is
/// still to come ([`quiet`] under [`Scope::BeforeAct`], closed by its
/// barrier), so a turn after the act is one the act asked for. A title's
/// pace, a paused link's restart, a frame's bound and a printed path's
/// hang are wakes posted for a moment, and wait here until they have run;
/// a frame's completion cannot come, the row on the CPU path; and the
/// steady cursor the row's setup asks leaves no blink to restart for. A
/// wake that renews itself never stops being to come, and is no wait's to
/// pass: the job in front listed again every second while the activity
/// lights read it (`front_turn`, `Front::next_listing`), whose rows turn
/// the lights off. Bounded by [`RELEASE_BOUND`], past a printed path's
/// hang: reaching it says which part never came and what woke the window
/// while it waited ([`waited`]), on a line of its own, and the row fails.
fn still_before_the_act() -> Result<(), Unquiet> {
    let (event, signed, scope) = (probe_call::RigEvent::Settled, Signed::Yes, Scope::BeforeAct);
    let mut at_still = None;
    WAITED.with_borrow_mut(|w| *w = Some(Vec::new()));
    let start = Instant::now();
    loop {
        let still = quiet_at(event, signed, scope, &mut at_still);
        if still.is_ok() || start.elapsed() >= RELEASE_BOUND {
            return still;
        }
        probe::pass(0.01);
    }
}

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
            ENDS.with(|e| e.set(Ends::EffectsOrSentinel));
            Act::Type(line())
        }
        RigAct::TypeToSentinel => {
            ENDS.with(|e| e.set(Ends::Sentinel));
            Act::Type(line())
        }
        RigAct::TypeHeld => {
            ENDS.with(|e| e.set(Ends::EffectsOrSentinel));
            HOLDS.with(|h| h.set(Holds::Yes));
            Act::Type(line())
        }
        RigAct::Link => Act::Gesture(Gesture::LinkClick, line()),
        RigAct::Select => Act::Gesture(Gesture::Select, line()),
        RigAct::CopyMenu => Act::Gesture(Gesture::CopyMenu, line()),
        RigAct::NoticeClick => Act::Gesture(Gesture::NoticeClick, line()),
        RigAct::Close => Act::Gesture(Gesture::Close, line()),
        RigAct::Key => Act::Gesture(Gesture::Key, line()),
        RigAct::Button => Act::Gesture(Gesture::Button, line()),
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
        Gesture::Key => probe::key_down(view, probe::ViewKey::Return),
        Gesture::Button => {
            press(view, Press::Down, first, Chord::Plain);
            press(view, Press::Up, first, Chord::Plain);
        }
    }
}

/// A left-button drag from `from` to `to`.
fn drag(view: &DrawableView, from: NSPoint, to: NSPoint) {
    use probe::{Chord, Press, press};
    press(view, Press::Down, from, Chord::Plain);
    press(view, Press::Dragged, to, Chord::Plain);
    press(view, Press::Up, to, Chord::Plain);
}

/// A `wake:` posted from another thread and joined: certainly queued, and
/// heard for the census ([`Carrier::Posted`]).
fn post(waker: Waker) {
    heard(Carrier::Posted);
    let _ = std::thread::spawn(move || waker.wake()).join();
}

/// Effect kinds by their words, `,` between, `-` for none.
fn words(kinds: impl Iterator<Item = probe::EffectKind>) -> String {
    let said: Vec<&str> = kinds.map(probe::EffectKind::word).collect();
    match said.is_empty() {
        true => "-".to_owned(),
        false => said.join(","),
    }
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
    driver.given_key(id);
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
        HEARD.with(|h| h.set(Duration::ZERO));
        // Kept, never dropped: whatever an iteration's records miss is the
        // parent's to see.
        let mut got = probe::take();
        // **A sentinel found before this iteration's line went out** is the
        // last iteration's, its program's end left to a frame that iteration
        // never had: named here, the other end of the strand the check below
        // names where it is left.
        if ENDS.with(core::cell::Cell::get) != Ends::Bound
            && got.iter().any(|r| r.kind == probe::EffectKind::Clear)
        {
            println!("a stale sentinel from the last iteration");
            failed = true;
        }
        // **An act left alone starts still** ([`still_before_the_act`]),
        // or the row stops here, what woke the window while the rig waited
        // and this iteration's records printed.
        if act.after() == After::Alone {
            let still = still_before_the_act();
            let heard = waited();
            if let Err(unmet) = still {
                println!("never still before the act: {unmet}; heard while it waited: {heard}");
                for r in &got {
                    println!("{}", r.line());
                }
                failed = true;
                break;
            }
        }
        let posted = WAKES_POSTED.load(std::sync::atomic::Ordering::Acquire);
        match &act {
            Act::Click(file) => CLICK.with(|c| *c.borrow_mut() = Some(file.clone())),
            Act::Type(line) => TYPED.with(|c| *c.borrow_mut() = Some(line.clone())),
            Act::Gesture(g, _) => gesture(&view, *g, cells, Iteration(i)),
            Act::TypeTwo(first, _) => TYPED.with(|c| *c.borrow_mut() = Some(first.clone())),
        }
        match act.after() {
            After::Posted => post(view.waker()),
            // **What the act asked**: the wakes posted for a moment from
            // inside its responder call and the settle after it, which ran
            // with no pass of the run loop since the count was read -- the
            // ended input's own wake among them, and nothing else on a row
            // whose wait above found nothing to come.
            After::Alone => println!(
                "asked by the act {}",
                WAKES_POSTED
                    .load(std::sync::atomic::Ordering::Acquire)
                    .saturating_sub(posted)
            ),
        }
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
        // sentinel, a clear, or on the sentinel alone when the program
        // writes it last every time; its bound is a backstop that says so.
        let ends = ENDS.with(core::cell::Cell::get);
        let (bound, ended) = match ends {
            Ends::Bound => (BOUND, None),
            Ends::EffectsOrSentinel | Ends::Sentinel => (
                probe_call::SENTINEL_BACKSTOP,
                Some(probe::EffectKind::Clear),
            ),
        };
        let over = |got: &[probe::Record]| {
            let sentinel = ended.is_some_and(|k| got.iter().any(|r| r.kind == k));
            match ends {
                Ends::Sentinel => sentinel,
                Ends::Bound | Ends::EffectsOrSentinel => holds(got, each) || sentinel,
            }
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
        let answered = Instant::now();
        // Whatever else is ready runs, so a second effect would be seen; on
        // a driven arm a frame is ticked on every pass, as a display link
        // ticks while the link is on (`Ticker::tick` sends nothing while it
        // is paused), the passes bounding how many: a read left to a frame
        // -- every read, under `tick-split` -- is fed in this iteration, not
        // by the next one's first tick.
        for _ in 0..64 {
            if ticks == Ticks::Driven {
                ticker.tick();
            }
            probe::pass(0.0);
        }
        got.extend(probe::take());
        // A program's rows only: a click rings no bell.
        while let (Act::Type(_) | Act::TypeTwo(..) | Act::Gesture(..), Some(left)) =
            (&act, SPACING.checked_sub(answered.elapsed()))
        {
            if ticks == Ticks::Driven {
                ticker.tick();
            }
            probe::pass(left.min(TICK).as_secs_f64());
        }
        got.extend(probe::take());
        // **Nothing left to a frame no tick fed**: an iteration ends with no
        // read waiting for a frame, or what it left would be fed, and its
        // records found, by the next iteration's first tick. A read still
        // waiting is ticked for until it is fed, under the backstop, so the
        // check fails only on frames given that never fed it, never on a
        // read that came late. Only `tick-split` leaves reads to a frame,
        // so off it this always holds. The link paused at the iteration's
        // end is the other half of the same check, and it waits on the
        // polls that still keep the link running while they exist
        // (`held`): with them, the link never pauses, and that half would
        // fail every driven iteration.
        let waiting = Instant::now();
        while TAIL.with_borrow(|t| !t.is_empty())
            && waiting.elapsed() < probe_call::SENTINEL_BACKSTOP
        {
            if ticks == Ticks::Driven {
                ticker.tick();
            }
            probe::pass(TICK.as_secs_f64());
        }
        // Checked straight after the pass that found it empty, with no
        // pass between that could take a read in.
        got.extend(probe::take());
        if TAIL.with_borrow(|t| !t.is_empty()) {
            // Every later iteration would find this one's reads: the row
            // stops here.
            println!("a read left to a frame no tick fed");
            failed = true;
            break;
        }
        // The floor is longer than every frame this iteration's bells were
        // heard under, or the next iteration's bell may ring with this one.
        let heard = HEARD.with(core::cell::Cell::get);
        if heard >= SPACING {
            println!(
                "bells heard under a {heard:?} frame, the floor between iterations {SPACING:?}"
            );
            failed = true;
        }
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
            // **[`BOUND`] passed with an effect still owed** (`Ends::Bound`,
            // no sentinel): said on a line of its own, with what the
            // iteration owed and what ran, so a lost effect reads as the
            // bound it passed and never as an iteration with nothing to say.
            (None, false, _) => println!(
                "bound passed before its effects: owed {}, ran {}",
                words(each.iter().copied()),
                words(got.iter().map(|r| r.kind)),
            ),
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
    // The run's short writes, for a row that asks its witness
    // (`short_write`).
    println!("short writes {}", SHORT.with(core::cell::Cell::get));
    // And what carried the bytes no read fed on, by the turns' sources
    // (`carried`).
    println!("carried {}", census());
    // And how many of those turns the master's room woke, the field a row
    // that asks its witness reads (`ROOM`).
    println!("room turns {}", ROOM.with(core::cell::Cell::get));
    // And what woke each turn that fed held reads alone (`released`).
    println!("released by {}", release_census());
    // **An act left alone, released on its own wake**: the witness its row
    // passes only with, judged here by the carrier's type.
    if act.after() == After::Alone && released_by_the_act() == Witnessed::Yes {
        println!("{}", probe_call::RELEASED_ON_THE_ACTS_WAKE);
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

/// **The display link's period the frame rig's ticks claim**: each offered
/// tick's target is this far ahead (`probe::set_target`), and an
/// [`RigFrameTicks::UntilFlips`](probe_call::RigFrameTicks::UntilFlips) run
/// offers no tick before the last one's target has come.
const FRAME_PERIOD: Duration = Duration::from_micros(8333);

/// **What a frame row asks of the GPU's fault seam** (`BAUD_RIG_GPU`, in
/// the row's environment): nothing; the act's frame's completion held
/// past its bound with the ticks withheld (`hold`), or held and never
/// released (`hold-forever`); the window's next frame reported failed
/// (`fail`), its next two (`fail-twice`), every frame from it on
/// (`fail-forever`), or its next reported from a device that revoked the
/// process's access (`revoked`); or the launch's first frame held past
/// its bound, drawn as the window's opening draws it (`hold-launch`), or
/// held and never released (`hold-launch-forever`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum GpuRow {
    Untouched,
    Hold,
    HoldForever,
    Fail,
    FailTwice,
    FailForever,
    Revoked,
    HoldLaunch,
    HoldLaunchForever,
}

impl GpuRow {
    /// The row's, from its environment; `None` for a value no row spells.
    fn of(asked: Option<&str>) -> Option<GpuRow> {
        match asked {
            None => Some(GpuRow::Untouched),
            Some("hold") => Some(GpuRow::Hold),
            Some("hold-forever") => Some(GpuRow::HoldForever),
            Some("fail") => Some(GpuRow::Fail),
            Some("fail-twice") => Some(GpuRow::FailTwice),
            Some("fail-forever") => Some(GpuRow::FailForever),
            Some("revoked") => Some(GpuRow::Revoked),
            Some("hold-launch") => Some(GpuRow::HoldLaunch),
            Some("hold-launch-forever") => Some(GpuRow::HoldLaunchForever),
            Some(_) => None,
        }
    }

    /// The launch's first frame held (`hold-launch`, `hold-launch-forever`).
    fn holds_launch(self) -> bool {
        matches!(self, GpuRow::HoldLaunch | GpuRow::HoldLaunchForever)
    }

    /// The window's next frames reported failed, armed before what the
    /// window is told: how many, and how (`fail`, `fail-twice`,
    /// `fail-forever`, `revoked`).
    fn fails(self) -> Option<probe::GpuSeam> {
        match self {
            GpuRow::Fail => Some(probe::GpuSeam::FailNext(baud_render::Tally::of(1))),
            GpuRow::FailTwice => Some(probe::GpuSeam::FailNext(baud_render::Tally::of(2))),
            GpuRow::FailForever => Some(probe::GpuSeam::Fail),
            GpuRow::Revoked => Some(probe::GpuSeam::Revoked),
            GpuRow::Untouched
            | GpuRow::Hold
            | GpuRow::HoldForever
            | GpuRow::HoldLaunch
            | GpuRow::HoldLaunchForever => None,
        }
    }
}

/// The acts a row sends after the GPU is given up, each drawn.
const AFTER_GIVE_UP: u32 = 3;

/// **The act fed while a frame is held** (`GpuRow::HoldForever`), and what
/// it writes: its echo spells the dash as `\055`, so only its output
/// holds [`HELD_FED`].
const HELD_FED_ACT: &[u8] = br"printf 'held\055fed'";
const HELD_FED: &str = "held-fed";

/// How the frame rig spells why the GPU was given up.
fn gave_up_word(why: baud_present_macos::GaveUp) -> &'static str {
    match why {
        baud_present_macos::GaveUp::Stuck => "stuck",
        baud_present_macos::GaveUp::Failing => "failing",
        baud_present_macos::GaveUp::Refused(baud_present_macos::Refusal::AccessRevoked) => {
            "revoked"
        }
        baud_present_macos::GaveUp::Refused(baud_present_macos::Refusal::NotPermitted) => {
            "not-permitted"
        }
    }
}

/// **The launch's frame drawn on the CPU, read back from the surface it was
/// shown on** (`probe::cpu_shown_on`), so `opened=cpu` means the window
/// opened on the launch's screen and not only that the copy returned: the
/// surface's frame area, where the copy centres it, holds the CPU frame
/// premultiplied pixel for pixel, and some of it is not the clear colour.
/// Red on a copy that filled the surface with the clear colour alone, and
/// on a CPU frame that was never rendered and holds only the clear colour.
/// What it read is printed.
fn read_back_opening(app: &App) -> Result<(), String> {
    let at = probe::cpu_shown_on().ok_or("no frame was shown from the CPU")?;
    let p = app.presenter.as_ref().ok_or("no presenter")?;
    let words = p
        .surface_pixels(at)
        .ok_or("the shown surface could not be read")?;
    let (sw, sh) = p.size();
    let (sw, sh) = (slots(sw), slots(sh));
    let (fw, fh) = (slots(app.frame_w), slots(app.frame_h));
    if fw == 0 || fw > sw || fh > sh || words.len() < sw * sh || app.frame.len() < fw * fh {
        return Err(format!(
            "the shown surface ({sw}x{sh}, {} words) does not hold the frame ({fw}x{fh})",
            words.len()
        ));
    }
    let pitch = words.len() / sh;
    let (pad_x, pad_y) = ((sw - fw) / 2, (sh - fh) / 2);
    let clear = baud_present_macos::premultiply(app.renderer.clear_color()).0;
    let (mut unlike, mut drawn) = (0_usize, 0_usize);
    for y in 0..fh {
        for x in 0..fw {
            let want = baud_present_macos::premultiply(baud_render::Argb(app.frame[y * fw + x])).0;
            let got = words[(y + pad_y) * pitch + pad_x + x];
            unlike += usize::from(got != want);
            drawn += usize::from(got != clear);
        }
    }
    println!(
        "opening read back: {unlike} of {} pixels unlike the CPU frame, {drawn} not the clear colour",
        fw * fh
    );
    match (unlike, drawn) {
        (0, 0) => Err("the shown surface holds only the clear colour".to_owned()),
        (0, _) => Ok(()),
        _ => Err(format!(
            "the shown surface is not the CPU frame: {unlike} of {} pixels differ",
            fw * fh
        )),
    }
}

/// **The frame rig** (`rig_call::FrameRig`): the window made as the event
/// rig makes it and never shown; `setup` typed and read to its sign, the
/// clear of the pasteboard it ends with; the display link left to settle;
/// then `act` sent, if any, as `keys` says, the window told what `window`
/// says, and the rig's ticks offered `attempts` times, or until the caret
/// flips `attempts` times (`RigFrameTicks::UntilFlips`), each delivered only
/// while the door's state is `Run` (`probe::Ticker`). Printed: the link's
/// state at the end, the ticks delivered and the door's sends over the
/// offered ticks, the links made in all, the polls the last settle found
/// still the tick's, what it found owed, the palettes applied over the
/// ticks, the focus baud last set, the display options' changes taken, and
/// what the last read found under the pointer.
fn run_frames(rig: probe_call::FrameRig) -> std::process::ExitCode {
    use probe_call::{RigEnd, RigFocus, RigFrameTicks, RigHolders, RigKeys, RigWindow};
    // Whether the act ends on the pasteboard's clear: every wait for it
    // below reads this.
    let clears = Clears::of(&rig.act);
    // **The run's end and its ticks agree** (`RigEnd::Ticks`): ticks with
    // a run of their own end on it and on nothing else, and ticks offered
    // or withheld have none, so their row ends on a count or on quiet. A
    // call that pairs them otherwise would end twice or not at all.
    let ticks_end = match rig.ticks {
        RigFrameTicks::Offered | RigFrameTicks::Withheld => RigEnd::Passes,
        RigFrameTicks::UntilFlips
        | RigFrameTicks::OneAfterFeeds
        | RigFrameTicks::EventFrames
        | RigFrameTicks::UntilMarked
        | RigFrameTicks::UntilAsked => RigEnd::Ticks,
    };
    if (ticks_end == RigEnd::Ticks) != (rig.end == RigEnd::Ticks) {
        println!(
            "the row's end ({:?}) and its ticks ({:?}) disagree",
            rig.end, rig.ticks
        );
        return std::process::ExitCode::from(1);
    }
    // Before the app is made, and so before its every commit: the instant
    // a frozen frame gate's clock stops at (`OneAfterFeeds`, `EventFrames`).
    let opened = Instant::now();
    let Some(mtm) = MainThreadMarker::new() else {
        return std::process::ExitCode::from(3);
    };
    // **The fault seam, armed where the row asks** (`GpuRow`); a row that
    // asks for it in a build without it fails rather than passing with
    // nothing injected.
    let Some(gpu_row) = GpuRow::of(std::env::var("BAUD_RIG_GPU").ok().as_deref()) else {
        println!("BAUD_RIG_GPU names no row");
        return std::process::ExitCode::from(1);
    };
    if gpu_row != GpuRow::Untouched {
        let arm = match gpu_row {
            r if r.holds_launch() => probe::GpuSeam::Hold,
            _ => probe::GpuSeam::Pass,
        };
        if probe::gpu_seam(arm) == probe::Seamed::NotBuilt {
            println!("the GPU's fault seam is not built in");
            return std::process::ExitCode::from(1);
        }
    }
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
    // **A Notification Center that never answers**, where the row asks for
    // it (`BAUD_RIG_DELIVERED=stalled`): a held `p=alive` is then answered
    // at its bound, on the wake the read's hold scheduled for it.
    probe::set_notice_list(
        match std::env::var_os("BAUD_RIG_DELIVERED").is_some_and(|v| v == "stalled") {
            true => probe::NoticeList::Stalled,
            false => probe::NoticeList::Answers,
        },
    );
    let shell = Shell::start(scale, size, inset, None, None);
    let Opening {
        mut driver,
        view,
        window,
        ..
    } = Opening::new(mtm, size, scale, inset, shell, None);
    let id = crate::windows::Windows::<()>::new().open(());
    driver.given_key(id);
    driver.app.os.name(id);
    // **The launch's first frame, held** (`GpuRow::HoldLaunch`): drawn now
    // and waited for as `show` draws it, with its completion held past
    // the launch's bound, so the window opens with the frame drawn on the
    // CPU; then the seam let go, and what it held delivered, late. Held
    // for ever (`GpuRow::HoldLaunchForever`), the seam stays armed and
    // nothing is released: the window goes on from its CPU opening.
    if gpu_row.holds_launch() {
        let v = driver.view.clone();
        driver.app.present(&v, Show::Now);
        if let Err(why) = read_back_opening(&driver.app) {
            println!("{why}");
            return std::process::ExitCode::from(1);
        }
        if probe::gpu_held() == baud_render::Tally::ZERO {
            println!("the launch's frame was never held");
            return std::process::ExitCode::from(1);
        }
        if gpu_row == GpuRow::HoldLaunch {
            let _ = probe::gpu_seam(probe::GpuSeam::Pass);
            let _ = probe::gpu_release();
        }
    }
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
    // The ticks offered, the rig's own count: what a withheld window is
    // held to (`RigFrameTicks::OneAfterFeeds`).
    let offered = core::cell::Cell::new(0_u32);
    // Each offered tick due a frame after now, on the display link's clock
    // (`probe::set_target`), as a real link's would be.
    let offer = |n: u16| {
        for _ in 0..n {
            offered.set(offered.get().saturating_add(1));
            let now = host_now().since(HostTime::ZERO).unwrap_or_default();
            probe::set_target((now + FRAME_PERIOD).as_secs_f64());
            ticker.tick();
            probe::pass(0.01);
        }
    };
    // **Shown before its first frame** (`RigWindow::ShownFirst`): the door
    // set on screen and the occlusion change posted before anything is
    // typed, then the first frame, the shell's prompt, waited for, and
    // then the splash's start, with nothing sent: only what that frame
    // wakes can start it, and then the splash played out. Each within the
    // bound.
    let first = match rig.window {
        RigWindow::ShownFirst => {
            probe::set_shown(probe::Shown::Yes);
            baud_present_macos::closing::rig_post(window.ns_window(), RigWindow::Shown);
            let start = Instant::now();
            while probe::frames().value() == 0 && start.elapsed() < BOUND {
                probe::pass(0.01);
            }
            if probe::frames().value() == 0 {
                println!("the first frame was never drawn");
                return std::process::ExitCode::from(1);
            }
            let start = Instant::now();
            while probe::greetings().0 == 0 && start.elapsed() < BOUND {
                probe::pass(0.01);
            }
            let first = probe::greetings().0;
            // A splash started is played to its end on offered ticks, as
            // the setup's settle offers them, so the row ends with no motion
            // owed: the lifted arm's link pauses and the held polls run the
            // kept arm's. One never started owes nothing.
            let start = Instant::now();
            while probe::splash() == probe::Splash::Moving && start.elapsed() < BOUND {
                offer(1);
            }
            if probe::splash() == probe::Splash::Moving {
                println!("the splash never ended");
                return std::process::ExitCode::from(1);
            }
            first
        }
        _ => 0,
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
    // **The setup's printed paths answered and its lights settled**: its
    // batch goes at its deadline, as late as a second after the last, and
    // its answer may owe a frame; what its bytes lit goes dark a hold and a
    // dwell after (`Deadline::Lights`). Both are the setup's work and not
    // the act's; waited for, within the bound, on the run loop.
    let start = Instant::now();
    let owed = || {
        (
            probe::paths_due() == probe::PathsDue::Owed,
            probe::lamps_due() == probe::LampsDue::Owed,
        )
    };
    while owed() != (false, false) && start.elapsed() < PATHS_BOUND {
        offer(1);
    }
    match owed() {
        (true, _) => {
            println!("the setup's paths were never answered");
            return std::process::ExitCode::from(1);
        }
        (_, true) => {
            println!("the setup's lights never settled");
            return std::process::ExitCode::from(1);
        }
        (false, false) => {}
    }
    // Settled: what the setup owed drawn, and the link left as it then is.
    offer(30);
    let (_, sends) = probe::link();
    let delivered = probe::delivered();
    let mut drawn = probe::frames();
    let flips = probe::flips();
    let styled = probe::styled();
    let mut reads = probe::hover_reads();
    let mut fed = probe::fed_turns();
    let hands = probe::hands();
    let greetings = probe::greetings();
    let asked = probe::paths_asked();
    // The paths asked about before the act, whose texts are not its.
    let asked_before = probe::paths_asked_texts().len();
    let (held, batches) = (probe::paths_held(), probe::paths_batches());
    let expired = probe::expirations();
    let notice_turns = probe::notice_turns();
    let notice_reads = probe::notice_state_reads();
    let tips = probe::lights_reads();
    let (looks, edges) = (probe::looks_made(), probe::edges_reads());
    let (facts, reported) = (probe::window_reads(), probe::window_reports());
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
    // **The row's counts, on one line**, each since the counts were last
    // taken (`reads`, `fed`, `drawn` and the ticks offered may be taken
    // again after a freeze): printed at the end, and before each labelled
    // exit of a frozen-clock window, so every red shows what it read.
    let counters = |reads: probe::HoverReads,
                    fed: probe::FedTurns,
                    drawn: baud_render::Tally,
                    offered_at: u32| {
        let (link, sends_after) = probe::link();
        let holders = probe::holders();
        println!(
            "frames link={} delivered={} sends={} made={} drawn={} holders={} owed={} styled={} focus={} motion={} display={} preview={} reads={} fed={} reaped={} followed={} hands={} greeted={} asked={} held={} batches={} marked={} flips={} first={} offered={} gpu_failed={} gpu_slowest_us={} gpu_overdue={} gpu_late={} gpu_gave_up={} gpu_whole={} opened={} gpu_use={} switched={} layer={} gpu_made={} gpu_held={} lit={} expired={} notices={} alive={} notice_reads={} tips={} looks={} edges={} facts={} reported={}",
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
            probe::paths_asked().0.saturating_sub(asked.0),
            probe::paths_held().0.saturating_sub(held.0),
            probe::paths_batches().0.saturating_sub(batches.0),
            probe::lines_marked().0,
            probe::flips().0.saturating_sub(flips.0),
            first,
            offered.get().saturating_sub(offered_at),
            probe::gpu_faults().frames_failed().value(),
            probe::gpu_faults().slowest.as_micros(),
            probe::gpu_faults().overdue.value(),
            probe::gpu_faults().late.value(),
            probe::gpu_faults().gave_up.value(),
            probe::gpu_whole().value(),
            match probe::opening() {
                probe::Opened::Unset => "unset",
                probe::Opened::Gpu => "gpu",
                probe::Opened::Cpu => "cpu",
                probe::Opened::Undrawn => "undrawn",
            },
            match probe::gpu_faults().gpu_use() {
                baud_present_macos::GpuUse::Live => "live",
                baud_present_macos::GpuUse::GaveUp(why) => gave_up_word(why),
            },
            // The frames drawn since the window gave the GPU up, the first
            // on the CPU among them.
            match probe::switched_at() {
                None => "none".to_owned(),
                Some(at) => probe::frames()
                    .value()
                    .saturating_sub(at.value())
                    .to_string(),
            },
            match probe::layer_by() {
                probe::Layer::Unset => "unset",
                probe::Layer::Cpu => "cpu",
                probe::Layer::Gpu => "gpu",
            },
            probe::gpu_made().value(),
            probe::gpu_held().value(),
            {
                let lit = probe::lit();
                match lit.is_empty() {
                    true => "-".to_owned(),
                    false => lit.join(","),
                }
            },
            probe::expirations().0.saturating_sub(expired.0),
            probe::notice_turns().0.saturating_sub(notice_turns.0),
            match probe::alive_answered() {
                probe::AliveAnswered::Unanswered => "unanswered",
                probe::AliveAnswered::List => "list",
                probe::AliveAnswered::Bound => "bound",
            },
            probe::notice_state_reads().0.saturating_sub(notice_reads.0),
            probe::lights_reads().0.saturating_sub(tips.0),
            probe::looks_made().0.saturating_sub(looks.0),
            probe::edges_reads().0.saturating_sub(edges.0),
            probe::window_reads().0.saturating_sub(facts.0),
            probe::window_reports().0.saturating_sub(reported.0),
        );
        // Each path asked about since the settle, on a line of its own: a
        // row about one path reads which, not only how many.
        for path in probe::paths_asked_texts().iter().skip(asked_before) {
            println!("asked {path}");
        }
    };
    let mut offered_at = offered.get();
    // A line of the rig's own to the shell, as typed keys are sent: a
    // row's later acts, each its own text.
    let type_line = |line: &[u8]| {
        TYPED.with(|c| *c.borrow_mut() = Some([line, b"\n"].concat()));
        post(view.waker());
    };
    let send_act = || match (rig.act.is_empty(), rig.keys) {
        (true, _) => {}
        (false, RigKeys::Line) => {
            TYPED.with(|c| *c.borrow_mut() = Some([rig.act.as_bytes(), b"\n"].concat()));
            post(view.waker());
        }
        (false, RigKeys::Pressed) => {
            PRESSED.with(|c| *c.borrow_mut() = Some(rig.act.clone()));
            post(view.waker());
        }
    };
    // **The act sent now, or after the frame gate's clock is frozen**
    // (`OneAfterFeeds`, `EventFrames`): there it is sent once a frame has
    // drawn after the freeze and the counts are taken again, so none of
    // what it feeds can land before them, whatever the machine's pace. A
    // finite act sent before the freeze could be fed whole before the
    // counts were taken again, leaving nothing to count.
    let frozen = matches!(
        rig.ticks,
        RigFrameTicks::OneAfterFeeds | RigFrameTicks::EventFrames
    );
    // The act's second stage, once the window's step has made it due
    // (`RigWindow::Released`).
    let second = core::cell::Cell::new(Second::Unasked);
    // **The second stage written once it is due**, on the run loop's
    // passes, so a run that ends on its own event waits for it however
    // late it comes: once the act has opened `go` to read it, since
    // opening the FIFO to write without blocking fails until then, and
    // never read once [`PATHS_BOUND`] past its time has passed without.
    let release = || {
        let Second::Due(due) = second.get() else {
            return;
        };
        let now = Instant::now();
        if now < due {
            return;
        }
        use std::io::Write as _;
        use std::os::unix::fs::OpenOptionsExt as _;
        let fifo = std::fs::OpenOptions::new()
            .write(true)
            .custom_flags(crate::sys::O_NONBLOCK)
            .open("go");
        // Taken before the newline goes: nothing the act writes after it
        // is fed before this pass returns to the run loop.
        let (fed, asked, batches, by) = (
            probe::fed_turns(),
            probe::paths_asked(),
            probe::paths_batches(),
            probe::paths_by(),
        );
        match fifo {
            Ok(mut f) => second.set(match f.write_all(b"\n") {
                Ok(()) => Second::Written {
                    at: now,
                    fed,
                    asked,
                    batches,
                    by,
                },
                Err(_) => Second::Unread,
            }),
            Err(_) if now >= due + PATHS_BOUND => second.set(Second::Unread),
            Err(_) => {}
        }
    };
    // The run: `attempts` ticks offered, or the run loop run as long with
    // no tick, the act's own turns; a second stage due written on the way.
    let run = |n: u16| match rig.ticks {
        RigFrameTicks::Offered | RigFrameTicks::UntilFlips | RigFrameTicks::UntilMarked => {
            release();
            offer(n);
        }
        RigFrameTicks::Withheld
        | RigFrameTicks::OneAfterFeeds
        | RigFrameTicks::EventFrames
        | RigFrameTicks::UntilAsked => {
            for _ in 0..n {
                release();
                probe::pass(0.01);
            }
        }
    };
    // Until `done`, a pass at a time, within `bound`: whether it came.
    let until = |done: &dyn Fn() -> bool, bound: Duration| {
        let start = Instant::now();
        while !done() && start.elapsed() < bound {
            run(1);
        }
        done()
    };
    let fail = |why: &str| {
        counters(reads, fed, drawn, offered_at);
        println!("{why}");
        std::process::ExitCode::from(1)
    };
    // The GPU frames committed, and those drawn whole, as the seam was
    // armed to fail the window's next frames.
    let (mut committed, mut whole) = (baud_render::Tally::ZERO, baud_render::Tally::ZERO);
    match gpu_row {
        // **The act's frame held past its bound** (`GpuRow::Hold`), the
        // ticks withheld: its completion held as it came, so the frame is
        // in flight and the window holds every draw; then only the wake
        // posted at its commit can count it, at its bound, and the window
        // goes on holding. Released once counted, the frame lands late,
        // and the window draws a whole frame after it on that completion's
        // wake. Each within the bound, which is under the give-up's.
        GpuRow::Hold => {
            let _ = probe::gpu_seam(probe::GpuSeam::Hold);
            send_act();
            let held = until(&|| probe::gpu_held() != baud_render::Tally::ZERO, BOUND);
            let _ = probe::gpu_seam(probe::GpuSeam::Pass);
            if !held {
                return fail("the act's frame was never held");
            }
            let overdue = || probe::gpu_faults().overdue != baud_render::Tally::ZERO;
            if !until(&overdue, BOUND) {
                let _ = probe::gpu_release();
                return fail("the held frame was never counted overdue");
            }
            let whole = probe::gpu_whole();
            if probe::gpu_release() == baud_render::Tally::ZERO {
                return fail("nothing was held at the release");
            }
            if !until(&|| probe::gpu_whole() != whole, BOUND) {
                return fail("no whole frame was drawn after the late one");
            }
        }
        // **The act's frame held and never released**
        // (`GpuRow::HoldForever`), the ticks withheld: its completion held
        // as it came, the seam left armed, so a frame committed to the GPU
        // after it would be held too. A second act is sent and fed while
        // the frame is held; what comes after, the give-up and the frames
        // on the CPU, is the window's to do on events alone (below).
        GpuRow::HoldForever => {
            let _ = probe::gpu_seam(probe::GpuSeam::Hold);
            send_act();
            if !until(&|| probe::gpu_held() != baud_render::Tally::ZERO, BOUND) {
                return fail("the act's frame was never held");
            }
            let before = probe::fed_turns();
            type_line(HELD_FED_ACT);
            if !until(&|| probe::fed_turns().0 > before.0, BOUND) {
                return fail("the second act was never fed while the frame was held");
            }
        }
        // **The window's next frames failed** (`GpuRow::fails`): armed
        // before what the window is told, whose frames fail below.
        GpuRow::Fail | GpuRow::FailTwice | GpuRow::FailForever | GpuRow::Revoked => {
            (committed, whole) = (probe::gpu_committed(), probe::gpu_whole());
            if let Some(seam) = gpu_row.fails() {
                let _ = probe::gpu_seam(seam);
            }
            send_act();
        }
        GpuRow::Untouched | GpuRow::HoldLaunch | GpuRow::HoldLaunchForever => {
            if !frozen {
                send_act();
            }
        }
    }
    // Whether the act's clear was waited for by the window's step.
    let mut act_signed = false;
    // The printed paths' batches by why each was due as the act's marks
    // were drawn (`RigFrameTicks::UntilMarked`), counted from those as its
    // second stage was written (`Second::Written`): what the act's output
    // alone sent, the first stage's echo's batches left out.
    // And the paths asked about, at the same two moments.
    let mut marked_by: Option<probe::PathsBy> = None;
    let mut marked_asked: Option<probe::Taken> = None;
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
        RigWindow::ShownFirst => {}
        // **Moved and not resized**: AppKit sends the move's notification
        // only "when the window that moved didn’t also change size"
        // (`[[source: apple-nswindow-didmovenotification]]`), so the frame's
        // origin alone is set, which keeps the size, and the row ends if it
        // did not. Nothing is posted for it below (`window_facts::rig_posts`):
        // what the row hears is AppKit's own notice of this move, the window
        // never shown.
        RigWindow::Moved => {
            let w = window.ns_window();
            let was = w.frame();
            w.setFrameOrigin(NSPoint::new(was.origin.x + 40.0, was.origin.y - 30.0));
            let now = w.frame();
            if now.size != was.size || now.origin == was.origin {
                println!("the move did not move the window alone");
                return std::process::ExitCode::from(1);
            }
        }
        // The rest change nothing of the window: their notification alone.
        RigWindow::Resized
        | RigWindow::ChangedScreen
        | RigWindow::ChangedBacking
        | RigWindow::Miniaturized
        | RigWindow::Deminiaturized
        | RigWindow::ScreenParameters => {}
        // The act's notice expired, its `w` passed, on a turn of the
        // window's own: waited for through the probe's door, within the
        // bound.
        RigWindow::Expired => {
            let start = Instant::now();
            while probe::expirations() == expired && start.elapsed() < BOUND {
                probe::pass(0.01);
            }
            if probe::expirations() == expired {
                println!("the notice never expired");
                return std::process::ExitCode::from(1);
            }
        }
        // The stalled lookup answered with a name `gethostname` does not
        // give, so the read adds one and its names are owed to the links.
        RigWindow::HostNamed => {
            probe::set_host_name(probe::HostName::Answered("baud-rig.invalid".to_owned()))
        }
        // **The act's second stage, on an event**: its clear signed, if it
        // has one, its frame drawn and the paths it owed answered, within
        // the bound; then the newline it reads from `go` made due, now or
        // [`probe_call::LATE_RELEASE`] from now, and written on the run's
        // passes (`release`).
        RigWindow::Released | RigWindow::ReleasedLate => {
            let start = Instant::now();
            let mut signed = clears == Clears::No;
            loop {
                signed |= probe::take()
                    .iter()
                    .any(|r| r.kind == probe::EffectKind::Clear);
                // Answered, and no wake the worker scheduled still to come:
                // one turns once, whatever comes before it.
                let answered = probe::paths_due() == probe::PathsDue::Idle;
                // Drawn: the frame that files what the act wrote is not
                // still owed, so its paths are owed before they are read.
                let drawn = probe::frame_owed() == probe::FrameOwed::No;
                if (signed && answered && drawn) || start.elapsed() >= RELEASE_BOUND {
                    break;
                }
                run(1);
            }
            if !signed || probe::paths_due() != probe::PathsDue::Idle {
                println!("the act's first stage never settled");
                return std::process::ExitCode::from(1);
            }
            act_signed = clears == Clears::Yes;
            let hold = match rig.window {
                RigWindow::ReleasedLate => probe_call::LATE_RELEASE,
                _ => Duration::ZERO,
            };
            second.set(Second::Due(Instant::now() + hold));
            // Due now, it is written before the window is told and the row
            // run: once the act has opened `go`, or never read. Only a late
            // release is left to the run's passes.
            if hold.is_zero() {
                loop {
                    release();
                    match second.get() {
                        Second::Written { .. } => break,
                        Second::Unread => {
                            println!("the act never read its second stage");
                            return std::process::ExitCode::from(1);
                        }
                        Second::Unasked | Second::Due(_) => run(1),
                    }
                }
            }
        }
    }
    baud_present_macos::closing::rig_post(window.ns_window(), rig.window);
    // **The frame the window was told to draw failed** (`GpuRow::Fail`):
    // that frame and no other reported failed (`Inject::FailNext`), at its
    // commit. The row owes nothing else -- no act, no output -- so a
    // frame after it is drawn only because the failure owed one; the
    // window draws it whole, within the bound. Two failed (`fail-twice`),
    // the same after the second.
    match gpu_row {
        GpuRow::Fail | GpuRow::FailTwice => {
            let armed = match gpu_row {
                GpuRow::Fail => 1,
                _ => 2,
            };
            let failed = || probe::gpu_faults().frames_failed().value() >= armed;
            if !until(&failed, BOUND) {
                return fail("fewer frames failed than were armed");
            }
            // A frame committed after the last that failed -- the armed
            // ones and one more since the seam was armed -- and each frame
            // after a failure drawn whole: counted from the arming, not
            // from when the failure was seen, which the frame after it can
            // come before on a loaded machine.
            let after = || {
                probe::gpu_committed().value() > committed.value() + armed
                    && probe::gpu_whole().value() >= whole.value() + armed
            };
            if !until(&after, BOUND) {
                return fail("no whole frame was drawn after the failed ones");
            }
        }
        // **The GPU given up, and the window going on** (`hold-forever`,
        // `hold-launch-forever`, `fail-forever`, `revoked`): the give-up
        // counted within its bound and the bound after it, and then
        // [`AFTER_GIVE_UP`] more acts, each its own text, each drawn
        // within the bound, on whatever the row's ticks are -- withheld,
        // the frames come on the events alone. The seam is still armed: a
        // frame committed to the GPU after the give-up would be held, or
        // fail, and the held count is the same at the end as at the
        // give-up. After a held frame (`hold-forever`), the screen the
        // first frame on the CPU drew holds the act fed while the frame was
        // held; and a zoom and a wider view after the give-up each come
        // into force, drawn.
        GpuRow::HoldForever | GpuRow::HoldLaunchForever | GpuRow::FailForever | GpuRow::Revoked => {
            let gave_up = || probe::gpu_faults().gave_up != baud_render::Tally::ZERO;
            let bound = baud_present_macos::Deadline::GIVE_UP.get() + BOUND;
            if !until(&gave_up, bound) {
                return fail("the GPU was never given up");
            }
            let held = probe::gpu_held();
            for k in 0..AFTER_GIVE_UP {
                let frames = probe::frames();
                type_line(format!("printf 'after\\055{k}'").as_bytes());
                if !until(&|| probe::frames() != frames, BOUND) {
                    return fail("no frame was drawn after the give-up");
                }
            }
            if gpu_row == GpuRow::HoldForever {
                let screen = SWITCH_SCREEN.with(|s| s.borrow().clone());
                if !screen.is_some_and(|s| s.contains(HELD_FED)) {
                    return fail("the first frame on the CPU did not hold what was fed while held");
                }
                let saw = || SAW.with(core::cell::Cell::get);
                let before = saw();
                RESHAPE.with(|r| r.set(Some(Reshape::ZoomIn)));
                post(view.waker());
                let zoomed = || saw().map(|s| s.0) != before.map(|s| s.0);
                if !until(&zoomed, BOUND) {
                    return fail("a zoom after the give-up never came into force");
                }
                let (before, frames) = (saw(), probe::frames());
                RESHAPE.with(|r| r.set(Some(Reshape::Wider)));
                post(view.waker());
                let wider = || saw().map(|s| s.1) != before.map(|s| s.1);
                if !until(&|| wider() && probe::frames() != frames, BOUND) {
                    return fail("a wider view after the give-up was never drawn");
                }
            }
            if probe::gpu_held() != held {
                return fail("a frame was held after the give-up: one was committed to the GPU");
            }
        }
        GpuRow::Untouched | GpuRow::Hold | GpuRow::HoldLaunch => {}
    }
    // The bound a run that ends on its event reached, if it did: which
    // event never came (`RigFrameTicks::UntilAsked`).
    let mut bound: Option<&str> = None;
    match rig.ticks {
        // **Until the caret has flipped**: a clock's frames come on wall
        // time, and a pass returns after the first source it handles, so a
        // count of offers is no length of time; offered until the caret
        // has flipped `attempts` times since the act, on its own clock,
        // under the backstop -- not until a count of frames, which a link
        // drawing every tick reaches before any flip. Each tick is offered
        // once the last one's target has come, the display link's own
        // period, which a real link never ticks faster than: the ticks the
        // link takes are then a count of the frames it ran for, not of how
        // fast the run loop's passes returned.
        RigFrameTicks::UntilFlips => {
            let wanted = u32::from(rig.attempts.0);
            let since = || probe::flips().0.saturating_sub(flips.0);
            let start = Instant::now();
            let mut due = host_now();
            let mut offered: u32 = 0;
            while since() < wanted && start.elapsed() < probe_call::SENTINEL_BACKSTOP {
                let now = host_now();
                match due.since(now) {
                    Some(left) if !left.is_zero() => probe::pass(left.as_secs_f64()),
                    _ => {
                        due = now.plus(FRAME_PERIOD);
                        offer(1);
                        offered = offered.saturating_add(1);
                    }
                }
            }
            // **A clock that kept its period**: at least a tick offered for
            // every second period passed, or a loaded machine has starved
            // the rig's link, and a count of ticks taken -- the row's bound
            // -- would read low for a reason that is not the app's.
            let periods = start.elapsed().as_micros() / FRAME_PERIOD.as_micros();
            if u128::from(offered).saturating_mul(2) < periods {
                println!(
                    "the rig's clock fell behind its period: {offered} ticks offered in {periods} periods"
                );
                return std::process::ExitCode::from(1);
            }
            if since() < wanted {
                println!("timed out at the backstop");
                return std::process::ExitCode::from(1);
            }
        }
        // **The frame gate's clock frozen** (`OneAfterFeeds`,
        // `EventFrames`), and then the act sent: the clock stopped at an
        // instant before the app was made, so every commit is no time ago
        // by it and no fed turn draws a frame for its age (D14), whatever
        // the machine's pace -- with no frame needed after the freeze,
        // which an idle screen would never owe. A frame the window's step
        // owed is drawn on offered ticks first, and the counts the row is
        // judged on are taken again there; only then is the act sent, so
        // every turn it feeds is counted and none lands before the counts.
        // Then the ticks withheld: until two turns have fed the core, and
        // one tick drawn after -- or, the clock moved on an hour first so
        // every fed turn draws a frame of its own, until two frames have
        // drawn, two turns fed and two reads made. In between, no tick
        // offered by the rig's own count. Each labelled exit prints the
        // counts first.
        RigFrameTicks::OneAfterFeeds | RigFrameTicks::EventFrames => {
            let fail = |why: &str, reads, fed, drawn, offered_at| {
                counters(reads, fed, drawn, offered_at);
                println!("{why}");
                std::process::ExitCode::from(1)
            };
            probe::freeze_gate(opened);
            let start = Instant::now();
            while probe::frame_owed() == probe::FrameOwed::Yes
                && start.elapsed() < probe_call::SENTINEL_BACKSTOP
            {
                offer(1);
            }
            if probe::frame_owed() == probe::FrameOwed::Yes {
                let why = "the frame owed before the act was never drawn";
                return fail(why, reads, fed, drawn, offered_at);
            }
            (reads, fed, drawn) = (probe::hover_reads(), probe::fed_turns(), probe::frames());
            offered_at = offered.get();
            let fed_since = || probe::fed_turns().0.saturating_sub(fed.0);
            let drawn_since = || probe::frames().value().saturating_sub(drawn.value());
            if rig.ticks == RigFrameTicks::EventFrames {
                probe::advance_gate(Duration::from_secs(3600));
            }
            send_act();
            let start = Instant::now();
            while fed_since() == 0 && start.elapsed() < probe_call::SENTINEL_BACKSTOP {
                probe::pass(0.01);
            }
            if fed_since() == 0 {
                return fail("the act never fed the core", reads, fed, drawn, offered_at);
            }
            match rig.ticks {
                // Until two frames have drawn and two turns fed, and the
                // hover has been read twice: the frames are the reads' to
                // make, and a frame drawn for anything but a fed turn's
                // output reads nothing.
                RigFrameTicks::EventFrames => {
                    let read_since = || probe::hover_reads().0.saturating_sub(reads.0);
                    let done = || drawn_since() >= 2 && fed_since() >= 2 && read_since() >= 2;
                    while !done() && start.elapsed() < probe_call::SENTINEL_BACKSTOP {
                        probe::pass(0.01);
                    }
                    if !done() {
                        let why = format!(
                            "fed turns never drew two frames that read: {} drawn, {} fed, {} read",
                            drawn_since(),
                            fed_since(),
                            read_since()
                        );
                        return fail(&why, reads, fed, drawn, offered_at);
                    }
                }
                _ => {
                    while fed_since() < 2 && start.elapsed() < probe_call::SENTINEL_BACKSTOP {
                        probe::pass(0.01);
                    }
                    if fed_since() < 2 {
                        let why = "the act fed fewer than two turns with the ticks withheld";
                        return fail(why, reads, fed, drawn, offered_at);
                    }
                    if drawn_since() != 0 {
                        return fail(
                            "a frame drew in the frozen window",
                            reads,
                            fed,
                            drawn,
                            offered_at,
                        );
                    }
                }
            }
            if offered.get() != offered_at {
                let why = "a tick was offered with the ticks withheld";
                return fail(why, reads, fed, drawn, offered_at);
            }
            if rig.ticks == RigFrameTicks::OneAfterFeeds {
                let start = Instant::now();
                while drawn_since() == 0 && start.elapsed() < probe_call::SENTINEL_BACKSTOP {
                    offer(1);
                }
                if drawn_since() == 0 {
                    let why = "no tick drew after the frozen window";
                    return fail(why, reads, fed, drawn, offered_at);
                }
            }
            probe::unfreeze_gate();
        }
        // **Until the act's printed paths are drawn**: a tick offered once
        // the last one's target has come, the display link's own period, as
        // `UntilFlips` offers them, so the frames between are a real link's.
        // The text's frame is the first with the act's clear read and no
        // frame owed, which has drawn everything written before the clear;
        // the marks' frame the first after it with `attempts` link lines at
        // rest and no frame owed. The run ends on the second, never on a
        // count of passes; the backstop, reached, says it never came. The
        // two frames' times since the act, and the frames drawn between,
        // are printed on a line of their own: a measurement, not the row's
        // judgement, which is the counts'.
        RigFrameTicks::UntilMarked => {
            let wanted = u32::from(rig.attempts.0);
            let start = Instant::now();
            let mut due = host_now();
            let mut text: Option<(Instant, baud_render::Tally)> = None;
            let mut marks: Option<(Instant, baud_render::Tally)> = None;
            let mut signed = false;
            while marks.is_none() && start.elapsed() < probe_call::SENTINEL_BACKSTOP {
                let now = host_now();
                match due.since(now) {
                    Some(left) if !left.is_zero() => probe::pass(left.as_secs_f64()),
                    _ => {
                        due = now.plus(FRAME_PERIOD);
                        offer(1);
                    }
                }
                signed |= probe::take()
                    .iter()
                    .any(|r| r.kind == probe::EffectKind::Clear);
                let drawn = probe::frame_owed() == probe::FrameOwed::No;
                if text.is_none() && signed && drawn {
                    text = Some((Instant::now(), probe::frames()));
                } else if text.is_some() && drawn && probe::lines_marked().0 >= wanted {
                    marks = Some((Instant::now(), probe::frames()));
                    marked_by = Some(probe::paths_by());
                    marked_asked = Some(probe::paths_asked());
                }
            }
            act_signed = signed;
            let (Some((text_at, text_frames)), Some((marks_at, marks_frames))) = (text, marks)
            else {
                counters(reads, fed, drawn, offered_at);
                println!("the act's marks were never drawn");
                return std::process::ExitCode::from(1);
            };
            let ms = |d: Duration| d.as_secs_f64() * 1000.0;
            let between = marks_at.saturating_duration_since(text_at);
            println!(
                "marks setup_asked={} text_ms={:.1} marks_ms={:.1} text_to_marks_ms={:.1} periods={:.1} frames={}",
                asked.0,
                ms(text_at.saturating_duration_since(start)),
                ms(marks_at.saturating_duration_since(start)),
                ms(between),
                between.as_secs_f64() / FRAME_PERIOD.as_secs_f64(),
                marks_frames.value().saturating_sub(text_frames.value()),
            );
        }
        // **Until the act's second stage is asked about**, the ticks
        // withheld: a turn fed after the newline that releases it, and then
        // a batch that asked about a path, the path's, the first stage
        // having left nothing owed and the second writing nothing else. The
        // run ends on the batch, never on a count of passes; past
        // [`probe_call::SENTINEL_BACKSTOP`] from the newline with no feed,
        // or from the feed with no batch, a bound that says which never
        // came, on a line of its own after the counts, which the row reads
        // as a failure of its own.
        RigFrameTicks::UntilAsked => {
            let mut path_fed: Option<Instant> = None;
            if second.get() == Second::Unasked {
                println!("the act has no second stage to release");
                return std::process::ExitCode::from(1);
            }
            bound = loop {
                run(1);
                let (at, fed_then, asked_then) = match second.get() {
                    Second::Unasked | Second::Unread => break None,
                    Second::Due(_) => continue,
                    Second::Written { at, fed, asked, .. } => (at, fed, asked),
                };
                if path_fed.is_none() && probe::fed_turns().0 > fed_then.0 {
                    path_fed = Some(Instant::now());
                }
                let Some(fed_at) = path_fed else {
                    if at.elapsed() >= probe_call::SENTINEL_BACKSTOP {
                        break Some("bound passed before the path was fed");
                    }
                    continue;
                };
                if probe::paths_asked().0 > asked_then.0 {
                    break None;
                }
                if fed_at.elapsed() >= probe_call::SENTINEL_BACKSTOP {
                    break Some("bound passed before the path's batch");
                }
            };
        }
        RigFrameTicks::Offered | RigFrameTicks::Withheld => {}
    }
    // **The run's end** (`RigEnd`): a count of passes; the ticks' own run,
    // ended above; or the row's event and quiet closed by its barrier
    // ([`quiet_at`]), a pass at a time -- a tick offered on each where the
    // ticks are, none where they are withheld -- with the act's clear read
    // on the way, then judged there, where nothing is left to come without
    // an outside event.
    // Quiet that never comes is a bound that says which part of it did
    // not, after the counts. On [`RigEnd::QuietOffered`] the rig's ticker
    // then offers [`probe_call::QUIET_OFFERS`] ticks, and a link that
    // takes one ran with nothing owed.
    match rig.end {
        RigEnd::Passes => run(rig.attempts.0),
        RigEnd::Ticks => {}
        RigEnd::Quiet(_) | RigEnd::QuietOffered | RigEnd::QuietCounted => {
            let event = match rig.end {
                RigEnd::Quiet(event) => event,
                RigEnd::Passes | RigEnd::Ticks | RigEnd::QuietOffered | RigEnd::QuietCounted => {
                    probe_call::RigEvent::Settled
                }
            };
            let mut signed = match (act_signed, clears) {
                (true, _) | (false, Clears::No) => Signed::Yes,
                (false, Clears::Yes) => Signed::No,
            };
            // Quiet reached, a pass at a time, its barrier posted afresh:
            // once after the act, and on `QuietCounted` again after the
            // wakes it posts there.
            let mut reach = || {
                let mut at_quiet = None;
                let start = Instant::now();
                let mut unmet = quiet_at(event, signed, Scope::End, &mut at_quiet).err();
                while unmet.is_some() && start.elapsed() < probe_call::SENTINEL_BACKSTOP {
                    run(1);
                    if probe::take()
                        .iter()
                        .any(|r| r.kind == probe::EffectKind::Clear)
                    {
                        signed = Signed::Yes;
                    }
                    unmet = quiet_at(event, signed, Scope::End, &mut at_quiet).err();
                }
                unmet
            };
            if let Some(unmet) = reach() {
                counters(reads, fed, drawn, offered_at);
                println!(
                    "bound passed before quiet: {unmet}, owes {}",
                    probe::owes().replace(' ', "")
                );
                return std::process::ExitCode::from(1);
            }
            act_signed = true;
            // **What baud's own work cost, counted** (`RigEnd::QuietCounted`):
            // at quiet, the main thread's allocations inside a responder
            // method or a turn (`heap::allocations`) and the AppKit seams'
            // sends with the display link door's (`probe::sends_counted`,
            // `probe::link`), read before the ticks, after them, and after
            // the bare wakes and quiet again.
            let counted_at = || {
                (
                    crate::heap::allocations(),
                    probe::sends_counted(),
                    probe::link().1,
                )
            };
            let at_quiet_counts = counted_at();
            if matches!(rig.end, RigEnd::QuietOffered | RigEnd::QuietCounted) {
                let before = probe::delivered();
                offer(probe_call::QUIET_OFFERS);
                let took = probe::delivered().0.saturating_sub(before.0);
                if took != 0 {
                    counters(reads, fed, drawn, offered_at);
                    println!(
                        "the link took {took} of the {} ticks offered at quiet",
                        probe_call::QUIET_OFFERS
                    );
                    return std::process::ExitCode::from(1);
                }
            }
            if rig.end == RigEnd::QuietCounted {
                let offered_counts = counted_at();
                let woke_before = probe::wakes();
                let mut posted = 0_u16;
                for _ in 0..probe_call::QUIET_WAKES {
                    if probe::wake_bare() == probe::Woken::Posted {
                        posted += 1;
                    }
                }
                if let Some(unmet) = reach() {
                    counters(reads, fed, drawn, offered_at);
                    println!(
                        "bound passed before quiet after the wakes: {unmet}, owes {}",
                        probe::owes().replace(' ', "")
                    );
                    return std::process::ExitCode::from(1);
                }
                let woke_counts = counted_at();
                // The seams' sends and the display link door's, from one
                // read to another.
                let appkit =
                    |from: (crate::heap::Calls, probe::Sent, probe::Sends),
                     to: (crate::heap::Calls, probe::Sent, probe::Sends)| {
                        let door = to.2.0.saturating_sub(from.2.0);
                        to.1.since(from.1).total().0.saturating_add(door)
                    };
                let kinds = woke_counts.1.since(offered_counts.1);
                let named: Vec<String> = baud_present_macos::sends::Kind::ALL
                    .iter()
                    .zip(kinds.0)
                    .filter(|(_, n)| n.0 > 0)
                    .map(|(k, n)| format!("{}:{}", k.name(), n.0))
                    .collect();
                println!(
                    "counted allocs={} appkit={} posted={posted} woke={} offered_allocs={} offered_appkit={} setup_allocs={} setup_seams={} kinds={}",
                    woke_counts.0.since(offered_counts.0).0,
                    appkit(offered_counts, woke_counts),
                    probe::wakes().since(woke_before),
                    offered_counts.0.since(at_quiet_counts.0).0,
                    appkit(at_quiet_counts, offered_counts),
                    at_quiet_counts.0.0,
                    at_quiet_counts.1.total().0,
                    match named.is_empty() {
                        true => "-".to_owned(),
                        false => named.join(","),
                    },
                );
            }
        }
    }
    // A late second stage still due here was never written before the run
    // ended: the row reads that from its counts, as a path it missed.
    if second.get() == Second::Unread {
        println!("the act never read its second stage");
        return std::process::ExitCode::from(1);
    }
    // **An act that ends by clearing the pasteboard is waited for**, as the
    // setup is: the run goes on until that clear, within the bound, and
    // then as long as the setup's settle, so what the act wrote is all fed
    // and drawn whatever the machine's pace, and the row's counts are the
    // whole act's.
    if clears == Clears::Yes && !act_signed {
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
    counters(reads, fed, drawn, offered_at);
    // The batches the act's second stage sent, by why each was due, and the
    // paths they asked about, to its marks' frame or the end: a line of
    // their own, for the rows that release one.
    if let Second::Written {
        by: from,
        asked: asked_from,
        ..
    } = second.get()
    {
        let by = marked_by.unwrap_or_else(probe::paths_by).since(from);
        let asked = marked_asked
            .unwrap_or_else(probe::paths_asked)
            .0
            .saturating_sub(asked_from.0);
        println!(
            "by ended={} quiet={} ceiling={} floor={} held_granted={} asked={asked}",
            by.ended.0, by.quiet.0, by.ceiling.0, by.floor.0, by.held_granted.0
        );
    }
    // The bound a run that ends on its event reached, after the counts:
    // the row's own failure, said apart from a count it missed, with what
    // the worker and the frame owed then and the batches sent and the paths
    // asked since the newline, so a red says where it stood.
    if let Some(why) = bound {
        let (batches, asked) = match second.get() {
            Second::Written { batches, asked, .. } => (
                probe::paths_batches().0.saturating_sub(batches.0),
                probe::paths_asked().0.saturating_sub(asked.0),
            ),
            _ => (0, 0),
        };
        println!(
            "{why}: paths {:?}, frame owed {:?}, owes {}, batches since the newline {batches}, paths asked since {asked}",
            probe::paths_due(),
            probe::frame_owed(),
            probe::owes().replace(' ', ""),
        );
    }
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
