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
//! asked has run: a copy or an announcement still runs, an effect on the
//! window itself finds the window gone and is dropped ([`Outcome::Gone`]),
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
}

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
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Open {
    Yes,
    Closed,
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

    /// The outcomes since the last call, in the order their effects ran,
    /// moved into `into` (whose storage the caller keeps).
    pub(crate) fn answered(&self, into: &mut Vec<EffectDone>) {
        into.append(&mut self.0.borrow_mut().done);
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
fn post() {}

/// The runner, installed once at startup as the queue's turn.
pub(crate) fn install() {
    baud_present_macos::turn::install(|t| run_with(t, make));
}

/// **One turn of the runner**: every slot's batch taken out, made in order
/// by `exec` with no slot borrowed, and each outcome handed back, the
/// slot's window woken once if any came. A closed window's slot is dropped
/// once it has nothing left to run or read.
fn run_with<T>(t: &T, mut exec: impl FnMut(&T, OsEffect, Open) -> Outcome) {
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
        let open = slot.borrow().open.get();
        let ran = !batch.is_empty();
        for (id, effect) in batch.drain(..) {
            let outcome = exec(t, effect, open);
            slot.borrow_mut().done.push(EffectDone { id, outcome });
        }
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
fn make(t: &baud_present_macos::turn::Turn<'_>, effect: OsEffect, open: Open) -> Outcome {
    use baud_present_macos::link::{Ask, workspace};
    match effect {
        OsEffect::Open { uri, scheme, shown } => Outcome::Opened {
            opened: workspace(t, Ask::Open, uri.as_str()),
            scheme,
            shown,
        },
        OsEffect::Reveal { uri, from } => Outcome::Revealed {
            opened: workspace(t, Ask::Reveal, uri.as_str()),
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
            if let Some(mtm) = macos_liquid_glass::objc2::MainThreadMarker::new() {
                baud_present_macos::announce::announce(mtm, words.as_str());
            }
            Outcome::Announced
        }
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
        run_with(&Fake, |_, e, open| {
            seen.push((said(&e), open));
            Outcome::Announced
        });
        assert_eq!(seen, [(String::from("after close"), Open::Closed)]);
        assert_eq!(woke.get(), 0);
        assert_eq!(QUEUES.with_borrow(Vec::len), 0, "the slot gone");
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
