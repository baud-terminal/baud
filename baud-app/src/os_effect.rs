//! **What a window asks of the desktop, queued as data and run in a turn
//! of its own** (the event-driven rule, `docs/event-driven.md`).
//!
//! Inside a frame, a pump or any responder method macos-liquid-glass holds
//! the view's responder borrow, so an effect made there that spins a
//! nested run loop re-enters the view: the reveal crash. So the code that
//! decides an effect pushes an [`OsEffect`] to its window's [`OsQueue`] and
//! returns; the queue's runner makes it on a main-queue turn of its own
//! (`baud_present_macos::turn`), after the callback has returned, and the
//! outcome comes back to the window as data on its next `wake`
//! ([`OsQueue::answered`]).
//!
//! **One queue per window**, because one process runs several: an effect
//! and its outcome stay with the window that asked, and its wake goes to
//! that window's view. Each window's slot is in a main-thread registry the
//! runner walks. A window that closes leaves its slot there until what it
//! asked has run: a copy, an announcement or a notification still runs, an
//! effect on the window itself finds the window gone and is dropped
//! ([`Outcome::Gone`]),
//! and a started command's child goes to the process's own reaper, which
//! each turn of the runner reaps, so no zombie outlives the window.
//!
//! **Nothing allocates per effect.** Each slot's two buffers are reserved
//! when the window opens and swapped, never rebuilt; asking schedules one
//! turn for the whole process on the empty-to-asked edge only (`turn::soon`,
//! a plain function and no closure); the outcomes go back through a third
//! reused buffer, main thread to main thread.
//!
//! **Re-entry.** An effect can spin a nested run loop (a reveal does), and
//! inside it the main queue runs again: a second turn of the runner, and
//! the view's `wake`, which reads the outcomes. So the runner takes each
//! slot's batch out under a borrow that ends before any effect runs, runs
//! the batch with no slot borrowed, and pushes each outcome under a borrow
//! of its own. A nested turn sees only what was asked since, and a nested
//! `wake` reads the outcomes freely.

use core::cell::{Cell, RefCell};
use std::rc::Rc;

use baud_domain::{AdmittedUri, ClipText, Spoken};

/// **A link as the log shows it**: its first 80 bytes, escaped. Words
/// for a person, never an address to open.
pub(crate) struct ShownLink(pub(crate) String);

impl core::fmt::Display for ShownLink {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One thing a window asks of the desktop.
pub(crate) enum OsEffect {
    /// A link handed to the application the system has for it.
    Open {
        uri: AdmittedUri,
        scheme: crate::link::Scheme,
        shown: ShownLink,
    },
    /// A file shown in Finder; which link or path asked, for the log.
    Reveal { uri: AdmittedUri, from: Asked },
    /// A `[link.*]` command started; what asked, for the log.
    Command {
        cmd: std::process::Command,
        from: Asked,
    },
    /// A new window running a `[link.*]` rule's program in place of a
    /// shell, its terminal started in `at` (`app::open_window`), over the
    /// window the click came from.
    OpenWindow {
        run: crate::rules::Run,
        at: Option<std::path::PathBuf>,
        over: Option<Over>,
    },
    /// Text to the pasteboard, and whose copy it is.
    Copy { text: ClipText, from: CopyFrom },
    /// The pasteboard emptied: a program's `OSC 52` clear.
    Clear,
    /// The find pasteboard's query: a find field's search.
    FindText(ClipText),
    /// Words to VoiceOver.
    Announce(Spoken),
    /// The system beep: a bell's sound.
    Beep,
    /// The Dock icon asked for the user's attention by a bell, under
    /// `?1042`: informational, once, its request kept by no one, so it
    /// cancels nothing a program asked.
    BellAttention,
    /// **The Dock icon asked for the user's attention by a program**
    /// (iTerm2's `RequestAttention`): the request the window's runner kept
    /// is cancelled first, and this one's kept in its place, to cancel
    /// ([`Kept`]); the driver never holds it.
    Attention(baud_present_macos::notify::Ask),
    /// The program's kept attention request cancelled.
    Unattend,
    /// **A program's notification**, posted on the window's notification
    /// center, or kept there until permission comes. It names no window,
    /// so it outlives the one that asked: a window closed straight after
    /// the ask still posts it.
    Notice(baud_present_macos::notify::Post),
    /// A notification closed, by its identifier.
    Unnotice(String),
    /// Notification Center asked which of the app's notifications it
    /// still holds, answered later (`Center::delivered`).
    AskDelivered,
    /// **This window's bar on the Dock tile**, which shows the most urgent
    /// window's (`notify::DockBars`). A closed window's bar is taken off,
    /// whatever it asked, so none outlives its window.
    DockBar(Option<crate::notify::DockBar>),
    /// The application activated: a click on a notification.
    Activate,
    /// The asking window raised and made key: a click on its notification.
    /// It names no window, as [`OsEffect::Front`] does not.
    KeyFront,
    /// The asking window raised in front of the others without taking
    /// focus: a bell's, under `?1043`. It names no window: the runner
    /// raises the slot's own ([`OsQueue::attach`]), so a raise of another
    /// window cannot be asked.
    Front,
}

/// A window, loaded from its slot for one effect: a reference counted,
/// never a copy.
pub(crate) type Window =
    macos_liquid_glass::objc2::rc::Retained<macos_liquid_glass::objc2_app_kit::NSWindow>;

/// **The window a link's window opens over**: its frame, title bar
/// included, which the new window takes exactly; the size of its content,
/// which the new window's grid is laid out for; and its key, which the new
/// window hands the keyboard back to when it closes, if that one is still
/// open (`windows::focus_back`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Over {
    pub(crate) frame: macos_liquid_glass::objc2_foundation::NSRect,
    pub(crate) content: macos_liquid_glass::objc2_foundation::NSSize,
    pub(crate) id: crate::windows::WindowId,
}

/// Whose copy reaches the pasteboard, which says what is said of it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum CopyFrom {
    /// A `[link.*]` rule's or a printed path's `copy`.
    Link,
    /// The selection: Cmd-C, the Edit menu, copy-on-select.
    Selection,
    /// A program's `OSC 52` write.
    Program,
}

/// What asked for a reveal or a command, as the log says it.
pub(crate) enum Asked {
    /// A printed path's click.
    PrintedPath,
    /// A link in the window, as shown.
    Link(ShownLink),
}

/// What became of one effect.
pub(crate) enum Outcome {
    /// An open, as the OS answered it.
    Opened {
        opened: baud_present_macos::link::Opened,
        scheme: crate::link::Scheme,
        shown: ShownLink,
    },
    /// A reveal, as the OS answered it.
    Revealed {
        opened: baud_present_macos::link::Opened,
        from: Asked,
    },
    /// A command, started or not.
    Started {
        started: std::io::Result<std::process::Child>,
        from: Asked,
    },
    /// The pasteboard written with this text, or refused.
    Copied {
        written: baud_present_macos::clipboard::Written,
        text: ClipText,
        from: CopyFrom,
    },
    /// A window opened for a link's `action = "window"`.
    WindowOpened,
    /// No window opened: the program a link's window would run is not there.
    WindowRefused(std::ffi::OsString),
    /// The pasteboard emptied.
    Cleared,
    /// The find pasteboard written, or refused.
    Found(baud_present_macos::clipboard::Written),
    /// The words given to VoiceOver.
    Announced,
    /// The beep made.
    Beeped,
    /// The Dock icon asked for attention.
    Attended,
    /// A program's attention request cancelled, if one was kept.
    Unattended,
    /// A notification posted, or kept for permission.
    Noticed,
    /// A notification closed.
    Unnoticed,
    /// Notification Center asked what it holds.
    AskedDelivered,
    /// The Dock tile's bar set.
    Barred,
    /// The application activated.
    Activated,
    /// The window raised and made key.
    MadeKey,
    /// The window raised.
    Raised,
    /// Not made: the window it was for has closed.
    Gone,
}

/// Which effect an outcome is: each window counts its own, in order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct EffectId(u32);

impl EffectId {
    const FIRST: EffectId = EffectId(0);

    /// The one after, wrapping: two in flight at once never share one.
    fn next(self) -> EffectId {
        EffectId(self.0.wrapping_add(1))
    }
}

/// One effect's outcome, back to the window that asked.
pub(crate) struct EffectDone {
    /// Which effect: the window reads its outcomes in the order asked,
    /// which its tests check by this.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) id: EffectId,
    pub(crate) outcome: Outcome,
}

/// How many effects a window's buffers hold before they grow: a burst of
/// clicks, never a steady state.
const RESERVED: usize = 64;

/// A window's queue: what it asked, its outcomes, and how to wake it.
struct Slot {
    /// Asked since the last turn.
    now: Vec<(EffectId, OsEffect)>,
    /// The other buffer, empty, swapped with `now` by each turn.
    spare: Vec<(EffectId, OsEffect)>,
    /// Outcomes not yet read by the window.
    done: Vec<EffectDone>,
    /// The next effect's id.
    next: EffectId,
    /// The window's view, woken when an outcome is pushed: shared, so the
    /// runner clones it out and calls it with no borrow of the slot held.
    wake: Rc<dyn Fn()>,
    /// Whether the window still has the slot: a closed window's slot runs
    /// what it asked and is then dropped.
    open: Cell<Open>,
    /// Whether the window itself is still open: closed the moment it is
    /// about to close ([`Closer`]), a turn before its driver, and the slot
    /// with it, lets go (`app::closed`), so an effect on the window asked
    /// before then finds it gone rather than showing it again.
    window: Rc<Cell<Open>>,
    /// The window itself, the one its own effects are made on, given once
    /// it exists ([`OsQueue::attach`]); weak, so the slot keeps no closed
    /// window alive.
    own: Option<macos_liquid_glass::objc2::rc::Weak<macos_liquid_glass::objc2_app_kit::NSWindow>>,
    /// The window's key in the open windows, given once it is shown
    /// ([`OsQueue::name`]): its Dock bar is kept under it.
    id: Option<crate::windows::WindowId>,
    /// The process's notification center, given once the window has it
    /// ([`OsQueue::attach_center`]).
    center: Option<Rc<baud_present_macos::notify::Center>>,
    /// Where the answer to this window's "which are delivered" arrives,
    /// its own ([`OsQueue::take_delivered`]).
    delivered: baud_present_macos::notify::Delivered,
    /// The program's attention request, kept by the runner: shared with
    /// the window's [`Closer`], which takes it at the close.
    attention: Kept<baud_present_macos::notify::AttentionId>,
    /// Whether the window is owed a turn with no effect in it
    /// ([`OsQueue::owe_turn`]): the runner wakes it once and clears this.
    turn: Cell<Owed>,
}

/// **A request kept to be cancelled**: a program's attention request,
/// owned by the window's runner and its close, never by the driver.
pub(crate) struct Kept<I>(Rc<Cell<Option<I>>>);

impl<I: Copy> Kept<I> {
    fn new() -> Kept<I> {
        Kept(Rc::new(Cell::new(None)))
    }

    /// Another handle on the same request.
    fn share(&self) -> Kept<I> {
        Kept(Rc::clone(&self.0))
    }

    /// **A new request in place of the kept one**: the kept one cancelled
    /// first, then `ask`'s id kept; with no ask, the kept one cancelled
    /// and none kept. Pure over the two calls, so a test asks it with no
    /// Dock.
    fn attend(&self, ask: Option<impl FnOnce() -> I>, cancel: impl FnOnce(I)) {
        if let Some(id) = self.0.take() {
            cancel(id);
        }
        self.0.set(ask.map(|ask| ask()));
    }

    /// The kept request, taken: none kept after.
    fn take(&self) -> Option<I> {
        self.0.take()
    }
}

/// What the runner knows of the asking window for one effect: whether it
/// is open, and, while it is, the window itself if the slot has it; its
/// key, its notification center and its kept attention request, open or
/// not. Each a reference counted or a copy, never an allocation.
struct Its {
    open: Open,
    window: Option<Window>,
    id: Option<crate::windows::WindowId>,
    center: Option<Rc<baud_present_macos::notify::Center>>,
    delivered: baud_present_macos::notify::Delivered,
    attention: Kept<baud_present_macos::notify::AttentionId>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Open {
    Yes,
    Closed,
}

/// Whether a window is owed a turn of its own, with no effect in it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Owed {
    No,
    Yes,
}

/// Whether a turn is already posted for what has been asked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Due {
    No,
    Posted,
}

thread_local! {
    /// Every window's slot, the closed ones until their effects have run.
    static QUEUES: RefCell<Vec<Rc<RefCell<Slot>>>> = const { RefCell::new(Vec::new()) };
    static DUE: Cell<Due> = const { Cell::new(Due::No) };
    /// How many turns of the runner are on the stack: one, or more inside
    /// an effect's nested run loop.
    static DEPTH: Cell<u32> = const { Cell::new(0) };
    /// Children whose window closed before their command was reaped.
    static ORPHANS: RefCell<crate::opener::Opener> = RefCell::new(crate::opener::Opener::default());
}

/// **A window's effect queue.** Made once per window; dropped with it.
pub(crate) struct OsQueue(Rc<RefCell<Slot>>);

impl OsQueue {
    /// The queue of a window whose view `wake` wakes.
    pub(crate) fn open(wake: impl Fn() + 'static) -> OsQueue {
        let slot = Rc::new(RefCell::new(Slot {
            now: Vec::with_capacity(RESERVED),
            spare: Vec::with_capacity(RESERVED),
            done: Vec::with_capacity(RESERVED),
            next: EffectId::FIRST,
            wake: Rc::new(wake),
            open: Cell::new(Open::Yes),
            window: Rc::new(Cell::new(Open::Yes)),
            own: None,
            id: None,
            center: None,
            delivered: baud_present_macos::notify::Delivered::default(),
            attention: Kept::new(),
            turn: Cell::new(Owed::No),
        }));
        QUEUES.with_borrow_mut(|q| q.push(Rc::clone(&slot)));
        OsQueue(slot)
    }

    /// **Ask for `effect`**, made on the next turn of the runner; nothing
    /// runs now. The first ask since the last turn posts that turn.
    pub(crate) fn ask(&self, effect: OsEffect) -> EffectId {
        let id = {
            let mut s = self.0.borrow_mut();
            let id = s.next;
            s.next = id.next();
            s.now.push((id, effect));
            id
        };
        if DUE.with(|d| d.replace(Due::Posted)) == Due::No {
            post();
        }
        id
    }

    /// **A turn owed the window with nothing to make in it**: the runner's
    /// next turn wakes the window once, as it does after a batch, and the
    /// window's pump is that turn -- for work a window does a step at a time
    /// (a kitty picture's decode, `reads::decode_step`), never on the
    /// display link. Owing it again before the turn runs is the same turn:
    /// one wake, however many asks. The first ask since the last turn posts
    /// that turn, as [`OsQueue::ask`] does, with no slot borrowed.
    pub(crate) fn owe_turn(&self) {
        self.0.borrow().turn.set(Owed::Yes);
        if DUE.with(|d| d.replace(Due::Posted)) == Due::No {
            post();
        }
    }

    /// The outcomes since the last call, in the order their effects ran,
    /// moved into `into` (whose storage the caller keeps).
    pub(crate) fn answered(&self, into: &mut Vec<EffectDone>) {
        into.append(&mut self.0.borrow_mut().done);
    }

    /// **The window this queue is the window's own for**: the one its own
    /// effects, a raise, are made on, given once the window exists.
    pub(crate) fn attach(&self, window: &macos_liquid_glass::objc2_app_kit::NSWindow) {
        self.0.borrow_mut().own = Some(macos_liquid_glass::objc2::rc::Weak::new(window));
    }

    /// **The window's key**, given when it is shown: what its Dock bar is
    /// kept under.
    pub(crate) fn name(&self, id: crate::windows::WindowId) {
        self.0.borrow_mut().id = Some(id);
    }

    /// **The window's notification center**, the one its notifications are
    /// posted on and closed through, given once it exists.
    pub(crate) fn attach_center(&self, center: Rc<baud_present_macos::notify::Center>) {
        self.0.borrow_mut().center = Some(center);
    }

    /// The answer to this window's last "which are delivered", once.
    pub(crate) fn take_delivered(&self) -> Option<Vec<String>> {
        self.0.borrow().delivered.take()
    }

    /// What tells this queue its window is closing, for the window's close
    /// observer to hold.
    pub(crate) fn closer(&self) -> Closer {
        let s = self.0.borrow();
        Closer {
            window: Rc::clone(&s.window),
            attention: s.attention.share(),
        }
    }
}

/// **A window's close, told to its queue at once**: called from the
/// window's will-close observer, before the turn that drops its driver.
/// From then on the window's own effects -- its raise, its attention --
/// are dropped ([`Outcome::Gone`]); what else it asked still runs, and the
/// slot is walked until the driver lets go. No borrow of the slot is
/// taken, so it can be called from any callback.
pub(crate) struct Closer {
    window: Rc<Cell<Open>>,
    attention: Kept<baud_present_macos::notify::AttentionId>,
}

impl Closer {
    /// The window closed, and the attention request the runner kept for
    /// it taken, for the close's own turn to cancel: none can be kept
    /// after, since a closed window's attention is never asked.
    #[must_use = "the kept request is the close's turn to cancel"]
    pub(crate) fn closed(&self) -> Option<baud_present_macos::notify::AttentionId> {
        self.window.set(Open::Closed);
        self.attention.take()
    }
}

impl Drop for OsQueue {
    fn drop(&mut self) {
        self.0.borrow().open.set(Open::Closed);
    }
}

/// The turn for what has been asked: `turn::soon`, whose function
/// [`install`] set. In this module's tests nothing is posted; they run
/// the turn themselves.
#[cfg(not(test))]
fn post() {
    baud_present_macos::turn::soon();
}

#[cfg(test)]
thread_local! {
    /// The turns this module's tests would have posted.
    static POSTS: Cell<u32> = const { Cell::new(0) };
}

/// In this module's tests nothing is posted: the post is counted.
#[cfg(test)]
fn post() {
    POSTS.with(|p| p.set(p.get() + 1));
}

/// The runner, installed once at startup as the queue's turn.
pub(crate) fn install() {
    baud_present_macos::turn::install(|t| run_with(t, make));
}

/// **One turn of the runner**: every slot's batch taken out, made in order
/// by `exec` with no slot borrowed, and each outcome handed back, the
/// slot's window woken once if any came. A closed window's slot is dropped
/// once it has nothing left to run or read.
fn run_with<T>(t: &T, mut exec: impl FnMut(&T, OsEffect, Its) -> Outcome) {
    DUE.with(|d| d.set(Due::No));
    DEPTH.with(|d| d.set(d.get() + 1));
    let mut i = 0;
    while let Some(slot) = QUEUES.with_borrow(|q| q.get(i).cloned()) {
        i += 1;
        // The batch out, and `now` given the empty buffer: the borrow ends
        // before anything runs.
        let mut batch = {
            let mut s = slot.borrow_mut();
            let mut batch = core::mem::take(&mut s.spare);
            core::mem::swap(&mut s.now, &mut batch);
            batch
        };
        // A turn the window was owed with nothing in it wakes it as a
        // batch does (`OsQueue::owe_turn`).
        let owed = slot.borrow().turn.replace(Owed::No) == Owed::Yes;
        let ran = !batch.is_empty() || owed;
        for (id, effect) in batch.drain(..) {
            // Read for each effect: an effect before it can spin a loop the
            // window closes in.
            let its = {
                let s = slot.borrow();
                let (open, window) = match (s.open.get(), s.window.get()) {
                    (Open::Yes, Open::Yes) => (
                        Open::Yes,
                        s.own
                            .as_ref()
                            .and_then(macos_liquid_glass::objc2::rc::Weak::load),
                    ),
                    _ => (Open::Closed, None),
                };
                Its {
                    open,
                    window,
                    id: s.id,
                    center: s.center.clone(),
                    delivered: s.delivered.clone(),
                    attention: s.attention.share(),
                }
            };
            let outcome = exec(t, effect, its);
            slot.borrow_mut().done.push(EffectDone { id, outcome });
        }
        let open = slot.borrow().open.get();
        // The buffer back, for the next turn's swap.
        {
            let mut s = slot.borrow_mut();
            if s.spare.capacity() < batch.capacity() {
                s.spare = batch;
            }
        }
        // The wake cloned out and called after the borrow ends: a wake that
        // ran the pump in place reads the outcomes, which borrows the slot.
        if ran && open == Open::Yes {
            let wake = Rc::clone(&slot.borrow().wake);
            wake();
        }
    }
    // A closed window's slot, run out: its outcomes have no reader, and a
    // started command's child went to the process's reaper. Only by the
    // outermost turn, so a nested one never moves a slot under the walk.
    if DEPTH.with(|d| d.replace(d.get() - 1)) == 1 {
        QUEUES.with_borrow_mut(|q| {
            q.retain(|slot| {
                let s = slot.borrow();
                s.open.get() == Open::Yes || !s.now.is_empty()
            })
        });
        ORPHANS.with_borrow_mut(crate::opener::Opener::reap);
    }
}

/// **One effect, made**: the desktop call each kind names, with the turn
/// that proves no responder borrow is held. A closed window's command
/// starts and its child is the process's to reap.
fn make(t: &baud_present_macos::turn::Turn<'_>, effect: OsEffect, its: Its) -> Outcome {
    let open = its.open;
    use baud_present_macos::link::workspace;
    use baud_present_macos::notify::Ask;
    match effect {
        OsEffect::Open { uri, scheme, shown } => Outcome::Opened {
            opened: workspace(t, baud_present_macos::link::Ask::Open, uri.as_str()),
            scheme,
            shown,
        },
        OsEffect::Reveal { uri, from } => Outcome::Revealed {
            opened: workspace(t, baud_present_macos::link::Ask::Reveal, uri.as_str()),
            from,
        },
        OsEffect::Command { cmd, from } => match (crate::app::start_command(cmd), open) {
            (Ok(child), Open::Closed) => {
                ORPHANS.with_borrow_mut(|o| o.keep(child));
                Outcome::Started {
                    started: Err(std::io::Error::other("its window closed")),
                    from,
                }
            }
            (started, _) => Outcome::Started { started, from },
        },
        // In the turn, with no responder borrowed: the new window takes the
        // keyboard, which inside a callback would re-enter the view. It is
        // the click's, not the asking window's, so it opens whether or not
        // that window has closed since.
        // Looked for first, on the PATH the window's terminal would run it
        // under; then opened in the turn, with no responder borrowed: the
        // new window takes the keyboard, which inside a callback would
        // re-enter the view. It is the click's, not the asking window's, so
        // it opens whether or not that window has closed since.
        OsEffect::OpenWindow { run, at, over } => window_or_refused(
            run,
            at,
            over,
            std::env::var_os("PATH").as_deref(),
            |at, run, over| {
                baud_present_macos::window::open_running(t, move || {
                    crate::app::open_window(at, Some(run), over);
                });
            },
        ),
        OsEffect::Copy { text, from } => Outcome::Copied {
            written: baud_present_macos::clipboard::write_text(t, text.as_str()),
            text,
            from,
        },
        OsEffect::Clear => {
            baud_present_macos::clipboard::clear(t);
            Outcome::Cleared
        }
        OsEffect::FindText(text) => {
            Outcome::Found(baud_present_macos::clipboard::write_find(t, text.as_str()))
        }
        OsEffect::Announce(words) => {
            baud_present_macos::announce::announce(t.main_thread(), words.as_str());
            Outcome::Announced
        }
        OsEffect::Beep => {
            baud_present_macos::bell::beep(t);
            Outcome::Beeped
        }
        OsEffect::BellAttention => on_its_window(open, Some(()), |()| {
            let _ = baud_present_macos::notify::attention(t, Ask::Once);
            Outcome::Attended
        }),
        // The kept request cancelled and this one kept: a Cancel asked
        // behind its Critical in one batch runs after it and finds its id.
        OsEffect::Attention(ask) => on_its_window(open, Some(()), |()| {
            its.attention.attend(
                Some(|| baud_present_macos::notify::attention(t, ask)),
                |id| baud_present_macos::notify::cancel_attention(t, id),
            );
            Outcome::Attended
        }),
        OsEffect::Unattend => {
            its.attention.attend(
                None::<fn() -> baud_present_macos::notify::AttentionId>,
                |id| baud_present_macos::notify::cancel_attention(t, id),
            );
            Outcome::Unattended
        }
        // Open or closed: a notification goes to the user, and the slot's
        // center lives until it is posted.
        OsEffect::Notice(post) => match its.center {
            Some(center) => {
                center.post(t, post);
                Outcome::Noticed
            }
            None => Outcome::Gone,
        },
        OsEffect::Unnotice(id) => match its.center {
            Some(center) => {
                center.remove(t, &id);
                Outcome::Unnoticed
            }
            None => Outcome::Gone,
        },
        OsEffect::AskDelivered => on_its_window(open, its.center, |center| {
            center.ask_delivered(t, &its.delivered);
            Outcome::AskedDelivered
        }),
        OsEffect::DockBar(bar) => match its.id {
            Some(id) => {
                crate::app::dock(t, id, if open == Open::Yes { bar } else { None });
                Outcome::Barred
            }
            None => Outcome::Gone,
        },
        OsEffect::Activate => on_its_window(open, Some(()), |()| {
            baud_present_macos::window::activate(t);
            Outcome::Activated
        }),
        OsEffect::KeyFront => on_its_window(open, its.window, |window| {
            baud_present_macos::window::make_key_and_order_front(t, &window);
            Outcome::MadeKey
        }),
        OsEffect::Front => on_its_window(open, its.window, |window| {
            baud_present_macos::window::order_front(t, &window);
            Outcome::Raised
        }),
    }
}

/// **An effect on the asking window itself, made only while it is open**:
/// a closed window's raise, or its attention, finds the window gone and is
/// dropped, never made for a window no one can see; so is a raise for a
/// window the slot was never given. `window` is what the effect is made
/// on: the slot's own window, or `()` for the attention, which is the
/// application's Dock icon but asked for this window. Pure over `makes`,
/// so a test asks it with no window and no turn.
fn on_its_window<W>(open: Open, window: Option<W>, makes: impl FnOnce(W) -> Outcome) -> Outcome {
    match (open, window) {
        (Open::Yes, Some(w)) => makes(w),
        _ => Outcome::Gone,
    }
}

/// **A link's window, or why none**: `run`'s program looked for on
/// `path_var`'s folders, and `opens` given the window to open only when it
/// is there, so a program that is gone opens no window that would exit at
/// once. Pure over `opens`, so a test asks it with no window and no turn.
fn window_or_refused(
    run: crate::rules::Run,
    at: Option<std::path::PathBuf>,
    over: Option<Over>,
    path_var: Option<&std::ffi::OsStr>,
    opens: impl FnOnce(Option<std::path::PathBuf>, crate::rules::Run, Option<Over>),
) -> Outcome {
    match run.found(path_var) {
        crate::rules::Program::Missing => Outcome::WindowRefused(run.into_argv().swap_remove(0)),
        crate::rules::Program::Found => {
            opens(at, run, over);
            Outcome::WindowOpened
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tests' stand-in for a turn: the runner takes any token.
    struct Fake;

    fn spoken(s: &str) -> OsEffect {
        OsEffect::Announce(Spoken::of(s.to_owned()))
    }

    fn said(e: &OsEffect) -> String {
        match e {
            OsEffect::Announce(w) => w.as_str().to_owned(),
            _ => String::from("?"),
        }
    }

    /// Each test starts with no slot left by another on this thread.
    fn fresh() {
        QUEUES.with_borrow_mut(Vec::clear);
        DUE.with(|d| d.set(Due::No));
        DEPTH.with(|d| d.set(0));
        POSTS.with(|p| p.set(0));
    }

    /// **Nothing runs when asked; each runs once, on the turn, in order**,
    /// and the window hears the outcomes once.
    #[test]
    fn an_effect_runs_once_on_the_turn_in_the_order_asked() {
        fresh();
        let woke = Rc::new(Cell::new(0));
        let w = Rc::clone(&woke);
        let q = OsQueue::open(move || w.set(w.get() + 1));
        let a = q.ask(spoken("a"));
        let b = q.ask(spoken("b"));
        let mut ran = Vec::new();
        assert!(ran.is_empty(), "nothing when asked");
        run_with(&Fake, |_, e, _| {
            ran.push(said(&e));
            Outcome::Announced
        });
        assert_eq!(ran, ["a", "b"]);
        assert_eq!(woke.get(), 1, "woken once for the batch");
        let mut done = Vec::new();
        q.answered(&mut done);
        assert_eq!(done.iter().map(|d| d.id).collect::<Vec<_>>(), [a, b]);
        run_with(&Fake, |_, _, _| panic!("nothing again"));
        assert_eq!(woke.get(), 1);
    }

    /// **A turn owed with no effect wakes its window once**: two asks
    /// before the turn are one wake, the turn makes nothing, and a turn
    /// after it, with nothing owed, wakes no one. Break: the runner waking
    /// only after a batch (`ran` without `owed`), and no wake comes.
    #[test]
    fn a_turn_owed_with_no_effect_wakes_its_window_once() {
        fresh();
        let woke = Rc::new(Cell::new(0));
        let w = Rc::clone(&woke);
        let q = OsQueue::open(move || w.set(w.get() + 1));
        q.owe_turn();
        q.owe_turn();
        assert_eq!(woke.get(), 0, "nothing when owed");
        run_with(&Fake, |_, _, _| panic!("nothing to make"));
        assert_eq!(woke.get(), 1, "one wake for both");
        run_with(&Fake, |_, _, _| panic!("nothing again"));
        assert_eq!(woke.get(), 1, "none with nothing owed");
        let mut done = Vec::new();
        q.answered(&mut done);
        assert!(done.is_empty(), "no outcome");
    }

    /// **A turn owed from inside the wake is the next turn, posted once**:
    /// the window's pump runs in the wake the runner gives it, and a step
    /// there that leaves more owes the next turn. That posts one turn, an
    /// ask before it runs posts none, the next run wakes the window once,
    /// and the run after wakes no one. Breaks: `DUE` cleared after the
    /// wakes, and the second ask posts again; the wake's owed turn left
    /// unread by the runner, and the second run wakes no one.
    #[test]
    fn a_turn_owed_inside_the_wake_is_the_next_turn_posted_once() {
        fresh();
        let woke = Rc::new(Cell::new(0));
        let me: Rc<RefCell<Option<Rc<OsQueue>>>> = Rc::new(RefCell::new(None));
        let (w, m) = (Rc::clone(&woke), Rc::clone(&me));
        let q = Rc::new(OsQueue::open(move || {
            w.set(w.get() + 1);
            if w.get() == 1 {
                if let Some(q) = m.borrow().as_ref() {
                    q.owe_turn();
                }
            }
        }));
        *me.borrow_mut() = Some(Rc::clone(&q));
        q.owe_turn();
        assert_eq!(POSTS.with(Cell::get), 1, "the first ask posts");
        run_with(&Fake, |_, _, _| panic!("nothing to make"));
        assert_eq!(woke.get(), 1, "the first turn's wake");
        assert_eq!(
            POSTS.with(Cell::get),
            2,
            "owed inside it: the next turn posted"
        );
        q.owe_turn();
        assert_eq!(POSTS.with(Cell::get), 2, "an ask before it runs posts none");
        run_with(&Fake, |_, _, _| panic!("nothing to make"));
        assert_eq!(woke.get(), 2, "the turn owed inside the wake wakes it");
        run_with(&Fake, |_, _, _| panic!("nothing again"));
        assert_eq!(woke.get(), 2, "and nothing owed after");
        *me.borrow_mut() = None;
    }

    /// **R1, re-entry**: the first effect's own nested run loop runs a
    /// second turn of the runner and the window's `wake`, which reads the
    /// outcomes. Neither panics on a borrow; the second turn makes only
    /// what was asked since, and each effect runs once.
    /// Perturbation: run the batch inside `slot.borrow_mut()` and the
    /// nested turn panics, already borrowed.
    #[test]
    fn a_turn_nested_in_an_effect_runs_only_what_was_asked_since() {
        fresh();
        let q = Rc::new(OsQueue::open(|| {}));
        q.ask(spoken("first"));
        let mut order = Vec::new();
        let inner = Rc::clone(&q);
        run_with(&Fake, |t, e, _| {
            let word = said(&e);
            order.push(word.clone());
            if word == "first" {
                // The nested loop: an ask, a turn and a `wake`'s read.
                inner.ask(spoken("second"));
                let mut nested = Vec::new();
                run_with(t, |_, e, _| {
                    nested.push(said(&e));
                    Outcome::Announced
                });
                assert_eq!(nested, ["second"]);
                let mut done = Vec::new();
                inner.answered(&mut done);
                assert_eq!(done.len(), 1, "the nested outcome, read in the nest");
            }
            Outcome::Announced
        });
        assert_eq!(order, ["first"]);
        let mut done = Vec::new();
        q.answered(&mut done);
        assert_eq!(done.len(), 1, "the first's outcome");
    }

    /// **A wake that reads at once reads**: the window's `wake` is called
    /// after the slot's borrow has ended, so a wake that ran the pump in
    /// place -- reading the outcomes, which borrows the slot mutably --
    /// reads them, rather than panicking already borrowed (a panic the
    /// turn's guard would swallow). Perturbation: call the wake inside
    /// `slot.borrow()` and it panics.
    #[test]
    fn a_wake_that_reads_the_outcomes_at_once_reads_them() {
        fresh();
        let me = Rc::new(RefCell::new(std::rc::Weak::<OsQueue>::new()));
        let read = Rc::new(Cell::new(0));
        let (m, r) = (Rc::clone(&me), Rc::clone(&read));
        let q = Rc::new(OsQueue::open(move || {
            if let Some(q) = m.borrow().upgrade() {
                let mut done = Vec::new();
                q.answered(&mut done);
                r.set(r.get() + done.len());
            }
        }));
        *me.borrow_mut() = Rc::downgrade(&q);
        q.ask(spoken("a"));
        run_with(&Fake, |_, _, _| Outcome::Announced);
        assert_eq!(read.get(), 1, "the outcome, read in the wake");
    }

    /// **Two windows keep apart**: A's outcome is A's, and only A's view
    /// is woken.
    #[test]
    fn each_window_hears_only_its_own() {
        fresh();
        let (wa, wb) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
        let (ca, cb) = (Rc::clone(&wa), Rc::clone(&wb));
        let a = OsQueue::open(move || ca.set(ca.get() + 1));
        let b = OsQueue::open(move || cb.set(cb.get() + 1));
        a.ask(spoken("a"));
        run_with(&Fake, |_, _, _| Outcome::Announced);
        let (mut da, mut db) = (Vec::new(), Vec::new());
        a.answered(&mut da);
        b.answered(&mut db);
        assert_eq!((da.len(), db.len()), (1, 0));
        assert_eq!((wa.get(), wb.get()), (1, 0));
    }

    /// **R5, a closed window**: what it asked still runs, told that its
    /// window is gone, nothing is woken, and its slot goes once run out.
    #[test]
    fn a_closed_windows_effects_run_and_its_slot_goes() {
        fresh();
        let woke = Rc::new(Cell::new(0));
        let w = Rc::clone(&woke);
        let q = OsQueue::open(move || w.set(w.get() + 1));
        q.ask(spoken("after close"));
        drop(q);
        let mut seen = Vec::new();
        run_with(&Fake, |_, e, its| {
            seen.push((said(&e), its.open));
            Outcome::Announced
        });
        assert_eq!(seen, [(String::from("after close"), Open::Closed)]);
        assert_eq!(woke.get(), 0);
        assert_eq!(QUEUES.with_borrow(Vec::len), 0, "the slot gone");
    }

    /// **A closed window's raise and attention are dropped; an open one's
    /// are made, on the slot's own window**: `on_its_window` calls `makes`
    /// only while the window is open and the slot has it, and with that
    /// window. Named break: call `makes` whatever `open` says, and the
    /// closed window's raise is made.
    #[test]
    fn an_effect_on_a_closed_window_is_gone() {
        let made = core::cell::RefCell::new(Vec::new());
        let make = |w: &'static str| {
            made.borrow_mut().push(w);
            Outcome::Raised
        };
        assert!(matches!(
            on_its_window(Open::Closed, Some("own"), make),
            Outcome::Gone
        ));
        assert!(made.borrow().is_empty(), "nothing made for a closed window");
        assert!(matches!(
            on_its_window(Open::Yes, None, make),
            Outcome::Gone
        ));
        assert!(
            made.borrow().is_empty(),
            "nor for a window the slot was never given"
        );
        assert!(matches!(
            on_its_window(Open::Yes, Some("own"), make),
            Outcome::Raised
        ));
        assert_eq!(
            *made.borrow(),
            ["own"],
            "the control: made once, on its own window"
        );
    }

    /// **A raise names no window, nor does any effect on the asking window
    /// or the app**: `Front`, `KeyFront`, `Activate`, `Unattend`,
    /// `AskDelivered` and `BellAttention` carry no field, so the runner
    /// makes each on the slot's own window, request or center, and one for
    /// another window cannot be asked. Held from the source, since a
    /// compile-fail test cannot run in a binary crate. Named break:
    /// `KeyFront(Window)`.
    #[test]
    fn a_raise_names_no_window() {
        let src = include_str!("os_effect.rs");
        let effects = src
            .split_once(concat!("pub(crate) enum ", "OsEffect {"))
            .and_then(|(_, rest)| rest.split_once("\n}\n"))
            .map(|(body, _)| body)
            .expect("the effects");
        for unit in [
            "Front",
            "KeyFront",
            "Activate",
            "Unattend",
            "AskDelivered",
            "BellAttention",
        ] {
            let found: Vec<&str> = effects
                .lines()
                .map(str::trim)
                .filter(|l| {
                    l.strip_prefix(unit)
                        .is_some_and(|rest| !rest.starts_with(char::is_alphanumeric))
                })
                .collect();
            assert_eq!(found, [format!("{unit},")], "{unit}: a unit variant");
        }
        let made = src
            .split_once(concat!("fn ", "make("))
            .map(|(_, rest)| rest)
            .expect("the runner's make");
        assert!(
            made.contains("OsEffect::Front => on_its_window(open, its.window,"),
            "raised on the slot's own window"
        );
    }

    /// **A closed window's notification still posts; its raise, its key,
    /// the app's activation, its attention and its delivered list do not**:
    /// in the runner's `make`, `Notice`, `Unnotice`, `Unattend` and
    /// `DockBar` are made whatever the window, and each of the others only
    /// through `on_its_window`, which drops it for a closed window
    /// ([`an_effect_on_a_closed_window_is_gone`]). A notification goes to
    /// the user and outlives its window: `make; notify done; exit` closes
    /// the window straight after the ask. Held from the source, since the
    /// arms make desktop calls. Named breaks: `Notice` through
    /// `on_its_window` -- the notification is lost with its window --
    /// and `KeyFront` made directly.
    #[test]
    fn a_closed_windows_notice_posts_and_its_key_is_gone() {
        let src = include_str!("os_effect.rs");
        let made = src
            .split_once(concat!("fn ", "make("))
            .and_then(|(_, rest)| rest.split_once("\n}\n"))
            .map(|(body, _)| body)
            .expect("the runner's make");
        let arm = |head: &str| {
            let at = made.find(head).unwrap_or_else(|| panic!("{head}"));
            let rest = &made[at + head.len()..];
            let end = rest.find("\n        OsEffect::").unwrap_or(rest.len());
            rest[..end].trim_start()
        };
        for whatever in [
            "OsEffect::Notice(post) =>",
            "OsEffect::Unnotice(id) =>",
            "OsEffect::Unattend =>",
            "OsEffect::DockBar(bar) =>",
        ] {
            assert!(
                !arm(whatever).contains("on_its_window"),
                "{whatever} is made whatever the window"
            );
        }
        for (own, on) in [
            ("OsEffect::KeyFront =>", "on_its_window(open, its.window,"),
            ("OsEffect::Front =>", "on_its_window(open, its.window,"),
            ("OsEffect::Activate =>", "on_its_window(open, Some(()),"),
            (
                "OsEffect::Attention(ask) =>",
                "on_its_window(open, Some(()),",
            ),
            (
                "OsEffect::BellAttention =>",
                "on_its_window(open, Some(()),",
            ),
            (
                "OsEffect::AskDelivered =>",
                "on_its_window(open, its.center,",
            ),
        ] {
            assert!(
                arm(own).starts_with(on),
                "{own} only while open: {}",
                arm(own)
            );
        }
        // A closed window's bar is taken off rather than set.
        assert!(
            arm("OsEffect::DockBar(bar) =>").contains("if open == Open::Yes { bar } else { None }")
        );
    }

    /// **A program's attention request is the runner's to keep**: each new
    /// one cancels the kept one first and is kept in its place, so a
    /// Cancel asked behind its Critical in one batch finds the Critical's
    /// id; a Cancel with none kept cancels nothing; and the window's close
    /// takes the kept one, from the same cell the runner's effects see, for
    /// its own turn to cancel. Named break: keep the new id without
    /// cancelling the old, and the first Critical is never cancelled.
    #[test]
    fn the_kept_attention_request_is_cancelled_before_the_next() {
        let kept: Kept<u32> = Kept::new();
        let cancelled = RefCell::new(Vec::new());
        let cancel = |id| cancelled.borrow_mut().push(id);
        kept.attend(Some(|| 1), cancel);
        kept.attend(Some(|| 2), cancel);
        kept.attend(None::<fn() -> u32>, cancel);
        kept.attend(None::<fn() -> u32>, cancel);
        assert_eq!(*cancelled.borrow(), [1, 2], "each cancelled once, in turn");
        kept.attend(Some(|| 3), cancel);
        let shared = kept.share();
        assert_eq!(shared.take(), Some(3), "the close takes what was kept");
        assert_eq!(kept.take(), None, "and none is kept after");
        // The closer and the runner's view of a window share the slot's.
        fresh();
        let q = OsQueue::open(|| {});
        let closer = q.closer();
        q.ask(spoken("a"));
        let mut same = false;
        run_with(&Fake, |_, _, its| {
            same = Rc::ptr_eq(&its.attention.0, &closer.attention.0);
            Outcome::Announced
        });
        assert!(same, "one cell");
        assert_eq!(closer.closed(), None, "nothing kept, nothing taken");
    }

    /// **A closed window's post waiting for permission is sent on the
    /// grant's turn**: the window asks its first notification and closes
    /// at once (`make; notify done; exit`); the runner makes the post on
    /// the process's center for the closed slot, the slot is dropped once
    /// run out, and the answer's own turn sends the post, with no window
    /// left to read its news. The runner is the real one; the post is made
    /// as the `Notice` arm makes it, without a turn, which a test off the
    /// main thread cannot have. Named break: the answer's turn leaves the
    /// waiting posts (no flush), and nothing is sent.
    #[cfg(feature = "effect-probe")]
    #[test]
    fn a_closed_windows_waiting_post_is_sent_on_the_grants_turn() {
        use baud_present_macos::notify::{Center, Level, Permission, Post, PostSound};
        use baud_present_macos::probe::{EffectKind, take};
        fresh();
        let _ = take();
        let center = Rc::new(Center::unshared().expect("a center under the probe"));
        let q = OsQueue::open(|| {});
        q.attach_center(Rc::clone(&center));
        q.ask(OsEffect::Notice(Post {
            id: "baud-1-w0-1".into(),
            title: "make".into(),
            subtitle: String::new(),
            body: "done".into(),
            sound: PostSound::Silent,
            level: Level::Passive,
            buttons: Vec::new(),
            image: None,
        }));
        let closer = q.closer();
        assert_eq!(closer.closed(), None, "no attention kept");
        drop(q);
        run_with(&Fake, |_, e, its| match (e, its.center) {
            (OsEffect::Notice(post), Some(c)) => {
                c.post_off_turn(post);
                Outcome::Noticed
            }
            _ => Outcome::Gone,
        });
        assert_eq!(QUEUES.with_borrow(Vec::len), 0, "the slot gone");
        let kinds = || take().iter().map(|r| r.kind).collect::<Vec<_>>();
        assert_eq!(
            kinds(),
            [EffectKind::Authorize],
            "waiting, permission asked"
        );
        center.answered_off_turn(Permission::Granted);
        assert_eq!(kinds(), [EffectKind::Notice], "sent on the grant's turn");
    }

    /// **A window told it is closing finds its own effects gone at once**,
    /// a turn before its driver drops the queue: each effect runs told the
    /// window is closed, the window's view is still woken with the
    /// outcomes, and the slot is still walked, so what it asks next still
    /// runs. Named break: the runner reads only the queue's `open`, and
    /// the closing window's effect runs told it is open.
    #[test]
    fn a_closing_window_finds_its_effects_gone_before_its_driver_drops() {
        fresh();
        let woke = Rc::new(Cell::new(0));
        let w = Rc::clone(&woke);
        let q = OsQueue::open(move || w.set(w.get() + 1));
        let closer = q.closer();
        q.ask(spoken("raise"));
        assert_eq!(closer.closed(), None, "nothing kept");
        let mut seen = Vec::new();
        run_with(&Fake, |_, e, its| {
            seen.push((said(&e), its.open));
            Outcome::Announced
        });
        q.ask(spoken("after"));
        run_with(&Fake, |_, e, its| {
            seen.push((said(&e), its.open));
            Outcome::Announced
        });
        assert_eq!(
            seen,
            [
                (String::from("raise"), Open::Closed),
                (String::from("after"), Open::Closed)
            ]
        );
        assert_eq!(woke.get(), 2, "its view, still there, hears each");
        assert_eq!(QUEUES.with_borrow(Vec::len), 1, "the slot walked still");
        drop(q);
        run_with(&Fake, |_, _, _| Outcome::Announced);
        assert_eq!(
            QUEUES.with_borrow(Vec::len),
            0,
            "gone once the driver lets go"
        );
    }

    /// **Asking posts one turn for a batch**, on the empty-to-asked edge,
    /// and the turn clears it first.
    #[test]
    fn one_turn_is_due_for_a_batch() {
        fresh();
        let q = OsQueue::open(|| {});
        assert_eq!(DUE.with(Cell::get), Due::No);
        q.ask(spoken("a"));
        q.ask(spoken("b"));
        assert_eq!(DUE.with(Cell::get), Due::Posted);
        run_with(&Fake, |_, _, _| {
            assert_eq!(DUE.with(Cell::get), Due::No, "cleared before the effects");
            Outcome::Announced
        });
    }

    /// **The buffers are kept**: a turn swaps them, so the next asks reuse
    /// storage reserved when the window opened.
    #[test]
    fn a_turn_keeps_both_buffers() {
        fresh();
        let q = OsQueue::open(|| {});
        let caps = |q: &OsQueue| {
            let s = q.0.borrow();
            (s.now.capacity(), s.spare.capacity())
        };
        assert_eq!(caps(&q), (RESERVED, RESERVED));
        q.ask(spoken("a"));
        run_with(&Fake, |_, _, _| Outcome::Announced);
        assert_eq!(caps(&q), (RESERVED, RESERVED));
    }

    /// **A link's window opens only when its program is there, over the
    /// window the click came from**: a program that is gone, by its path or
    /// as a name on no folder of `PATH`, is refused and named, and the
    /// opener is never called; one that is there is opened once, with the
    /// folder and the argv it was asked with, moved, and the clicked
    /// window's frame, content and key as they were read. Named breaks: call
    /// `opens` before looking -- the missing editor opens a window that
    /// exits at once; open at the default place -- the frame is lost.
    #[test]
    fn a_links_window_opens_only_when_its_program_is_there() {
        use macos_liquid_glass::objc2_foundation::{NSPoint, NSRect, NSSize};
        use std::ffi::{OsStr, OsString};
        use std::path::PathBuf;
        let run = |p: &str| {
            crate::rules::Run::given(vec![OsString::from(p), OsString::from("/tmp/x")])
                .expect("a program")
        };
        let path_var = Some(OsStr::new("/usr/bin:/bin"));
        let clicked = Over {
            frame: NSRect::new(NSPoint::new(120.0, 80.0), NSSize::new(1013.0, 655.0)),
            content: NSSize::new(1013.0, 655.0),
            id: crate::windows::Windows::<()>::new().open(()),
        };
        let mut opened: Vec<(Option<PathBuf>, Vec<OsString>, Option<Over>)> = Vec::new();
        for gone in ["/no/such/editor", "no-such-editor"] {
            let out = window_or_refused(run(gone), None, Some(clicked), path_var, |at, r, o| {
                opened.push((at, r.into_argv(), o));
            });
            assert!(
                matches!(&out, Outcome::WindowRefused(p) if p == gone),
                "{gone}: refused and named"
            );
        }
        assert!(opened.is_empty(), "nothing opened: {opened:?}");
        let at = Some(PathBuf::from("/tmp"));
        let out = window_or_refused(
            run("sh"),
            at.clone(),
            Some(clicked),
            path_var,
            |at, r, o| {
                opened.push((at, r.into_argv(), o));
            },
        );
        assert!(matches!(out, Outcome::WindowOpened), "the control");
        assert_eq!(
            opened,
            [(
                at,
                vec![OsString::from("sh"), OsString::from("/tmp/x")],
                Some(clicked)
            )]
        );
    }
}
