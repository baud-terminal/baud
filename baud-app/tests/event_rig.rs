//! **Every effect runs in a turn of its own, never inside the view's
//! callback**: the event rig.
//!
//! Each row is a site where baud asks the OS for something. The row's
//! cause is made to happen in a child process -- the `baud` binary built
//! with the `effect-probe` feature, `--event-rig` (`src/app/rig.rs`) -- a
//! real `DrawableView` with baud's `Driver`, a window never shown, and no
//! Dock icon. There each effect is recorded where it runs, and then makes
//! the nested call the reveal crash's stack shows
//! (`nextEventMatchingMask:` in the default mode) with a `wake:` queued.
//! Inside a responder call that call re-enters the view: on
//! macos-liquid-glass 1.0.0-beta.8 the child aborted, which the parent
//! reads as the row's failure, and since beta.9 the wake is held, and the
//! record says the effect ran inside the responder call. 100 iterations
//! a row, each a fresh click.
//!
//! **Where the click rows' red came from.** Taken at the parent commit
//! with the rig alone added, no site moved, on beta.8: the command and
//! copy rows aborted ("RefCell already borrowed"), and the no-tick row
//! ran no effect. The reveal row passed there, its site having moved a
//! commit before, so its red came from a perturbation: `link::later`
//! made to hand over inline, under which it aborted the same way. That
//! perturbation is now a standing control,
//! the two control rows in [`SITES`], one made in the callback and one in
//! the frame.
//!
//! **Under load**, once: every row at 100 iterations while the gate's
//! whole-workspace release build ran beside it, the 1-minute load
//! average 37.33 before the run and 34.59 after on this 18-core Mac;
//! every green row passed, the reveal row's 100 reveals each in a turn of
//! its own with its nested wake delivered, and the control failed as
//! recorded. The rows wait on counts, never on time, so load moves only
//! how long a row takes.
//!
//! A row is green, or **pending** at a stage: expected to fail exactly as
//! recorded until the stage that moves its site, red if it passes early,
//! fails another way, or outlives its stage ([`verdict`]).
//!
//! Run with `cargo test -p baud --features effect-probe --test event_rig`;
//! the default suite does not build the probe.
#![cfg(target_os = "macos")]

use baud_present_macos::probe::{EffectKind, Nested, Record, Responder, Where};
use baud_present_macos::rig_call::{
    EventRig, FrameRig, Iterations, RigAct, RigCall, RigFocus, RigFrameTicks, RigHolders, RigKeys,
    RigTicks, RigWindow,
};
use baud_present_macos::unwind::SiteName;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::time::{Duration, Instant};

/// The stage the rule's work has reached: a pending row names the stage
/// that must clear it. `Done` is after the last.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Stage {
    One,
    Two,
    Three,
    /// The window crate's pause in place, published and taken: closed by
    /// that bump alone, so stage 3 does not wait on its publish.
    PauseInPlace,
    Done,
}

/// The stage this commit is.
const CURRENT: Stage = Stage::One;

/// How a row fails.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Failure {
    /// The effect ran inside the view's `frame`, and what its nested call
    /// met: the window crate holds the `wake:` while the responder is busy.
    InFrame(EffectKind, Nested),
    /// It ran inside another responder method.
    InCallback(EffectKind, Nested),
    /// The child aborted.
    Aborted,
    /// No effect within the bound.
    NotRun,
    /// An iteration that ends on an event met neither its effects nor the
    /// program's sentinel before the backstop ([`Cause::ProgramUntil`]):
    /// the rig stopped waiting, which says nothing of the effect.
    TimedOut,
    /// The program's sentinel came with no effect: it ended, and what it
    /// waited for never came ([`Cause::ProgramUntil`]).
    Unanswered,
    /// The effect ran other than once in an iteration, or another effect
    /// ran.
    Times,
    /// The nested call never had its `wake:`.
    Lost(EffectKind),
    /// A paced kind never ran in the whole run.
    NeverPaced(EffectKind),
    /// The child said something else was wrong.
    Child,
    /// A frame row's kept arm: a poll still the tick's held the display
    /// link ([`HOLDERS`]).
    Held,
    /// More than one display link was made for one window's restarts.
    Remade,
}

/// A row's state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RowState {
    Green,
    Pending {
        stage: Stage,
        recorded: Failure,
    },
    /// A control: fails as recorded on every run, at every stage, so a rig
    /// that has gone blind passes it and goes red.
    Control(Failure),
}

/// What makes the row's effect happen.
/// **What ends an iteration of a program that ends on an event**
/// ([`Cause::ProgramUntil`]), by what the program writes last.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Until {
    /// Its effect or its sentinel, one or the other: a kitty reply's copy
    /// on an answer read whole, the clear on anything else.
    Either,
    /// [`Until::Either`], each reply the terminal makes on a decode's turn
    /// held `HELD_REPLY` first and written in two pieces
    /// (`RigAct::TypeHeld`).
    EitherHeld,
    /// Its sentinel, written after its effect's bytes every time: the
    /// iteration ends on it, the program's end, and its effects are
    /// checked after it, so every read the program made has been fed
    /// however far apart it wrote them (`RigAct::TypeToSentinel`).
    Sentinel,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cause {
    /// A click on a printed path, the paths worker's answer on its wake,
    /// and the frame driven by `tick:` each pass, or never.
    PrintedPath(Rule, Ticks),
    /// What a program writes: the shell runs this line, typed to it as
    /// keys are sent, and its output comes back through the pty, under
    /// this config; once green, run with the frame undriven and again
    /// driven ([`arms`]).
    Program(Program),
    /// What a program writes in two reads: its line, and the second line
    /// once the first's first effect has run, so the second is read after
    /// the first's effect is made; run as a program's row.
    ProgramThen(Program, &'static str),
    /// What a program writes, as [`Cause::Program`], each iteration ending
    /// on its effects or on the program's sentinel, a clear, and never on
    /// wall time (`RigAct::TypeUntil`): the rig's wait a backstop that,
    /// reached, reads [`Failure::TimedOut`], and the sentinel
    /// [`Failure::Unanswered`]. With [`Until::EitherHeld`], each reply the
    /// terminal makes on a decode's turn is held `HELD_REPLY` first
    /// (`RigAct::TypeHeld`), the wall time a loaded machine takes, passed
    /// on purpose; with [`Until::Sentinel`], the iteration ends on the
    /// sentinel alone. Run as a program's row.
    ProgramUntil(Program, Until),
    /// A gesture sent to the view's own methods, over what `setup`, typed
    /// once first, drew; the frame never driven.
    Gesture(Gesture),
}

impl Cause {
    /// The row's shape: a printed path's is read, [`ITERATIONS`] of it.
    fn shape(self) -> Shape {
        match self {
            Cause::Program(Program { shape, .. })
            | Cause::ProgramUntil(Program { shape, .. }, _)
            | Cause::ProgramThen(Program { shape, .. }, _)
            | Cause::Gesture(Gesture { shape, .. }) => shape,
            Cause::PrintedPath(..) => READ,
        }
    }
}

/// A gesture, the screen it is over and the config.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Gesture {
    /// The child's act: a link's click, a selection, the Edit menu's
    /// copy, or a notification's click.
    act: RigAct,
    /// The shell line that draws the screen, `{file}` the row's own
    /// scratch file; it ends with an `OSC 52` clear, the sign it was read.
    setup: &'static str,
    config: &'static str,
    /// Whether baud believes the never-key window is key (`probe::Key`):
    /// a left press in a window baud believes is not key is the click
    /// that brings it forward, and arms no link.
    focus: RigFocus,
    shape: Shape,
}

/// A program's output, and the config it is read under.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Program {
    line: &'static str,
    config: &'static str,
    shape: Shape,
}

/// **Where a row's effect is asked from, and how many iterations it
/// runs.**
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Shape {
    polled: Polled,
    iterations: Iterations,
}

/// What asks a row's effect.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Polled {
    /// The read: the reader's `wake` or a tick, whichever comes first.
    Read,
    /// The tick alone, until stage `until`: a notification's clicks are
    /// polled by the tick (`notice_turn`) until stage 3 moves that poll to
    /// a turn, so the row runs only with the frame driven; a row past its
    /// stage is red ([`no_row_polls_the_tick_past_its_stage`]).
    Tick { until: Stage },
    /// The control for `turn::after`: that door alone made inline, in the
    /// callback that asked, the frame not driven.
    AfterInline,
}

/// A row read from the program, [`ITERATIONS`] of it.
const READ: Shape = Shape {
    polled: Polled::Read,
    iterations: ITERATIONS,
};

/// **Four iterations of a row whose replies are held**: each holds its
/// reply `HELD_REPLY`, so a hundred would hold the run four minutes an
/// arm; four show every iteration ends on its copy, not on time.
const HELD: Shape = Shape {
    polled: Polled::Read,
    iterations: Iterations(4),
};

/// **Four iterations of a row whose program pauses**, a third of a
/// second each: ended on the wrong event, every second iteration reads
/// Unanswered, so four show it on every run.
const PAUSED: Shape = Shape {
    polled: Polled::Read,
    iterations: Iterations(4),
};

/// **Four iterations of a row that posts a notification each**: the
/// flood bound lets four through at once (`notify::BURST`), and the rows'
/// iterations come faster than its one a second.
const FOUR_POSTS: Shape = Shape {
    polled: Polled::Read,
    iterations: Iterations(4),
};

/// **Four iterations of a row whose reads a ReGIS delay holds half a
/// second each**: four releases, inside the run's time.
const DELAYS: Shape = Shape {
    polled: Polled::Read,
    iterations: Iterations(4),
};

/// **One iteration**: a row about the first post, which waits for
/// permission, or about the window's close, which comes once.
const ONCE: Shape = Shape {
    polled: Polled::Read,
    iterations: Iterations(1),
};

/// What `[link.path]` makes of a printed path.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Rule {
    Reveal,
    Command,
    Copy,
    Window,
}

/// Whether the frame is driven, and whether the hand-over is made inline:
/// the controls, which put a reveal back inside a responder call.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Ticks {
    Driven,
    None,
    /// Inline, in the callback that asked: the click's answer, on `wake`.
    DrivenInline,
    /// Parked, and made by the next `frame`: the crash's own shape.
    DrivenInFrame,
    /// `turn::after` alone inline, in the callback that asked, the frame
    /// not driven: the read is drained by the reader's `wake` alone, as a
    /// pending program row's is ([`arms`]).
    UndrivenAfterInline,
    /// Driven, each read left to the frame: a wake's turn takes none of it
    /// and the next frame all of it, as the tty's pieces can fall, so what
    /// the read asks is asked inside `frame` every time.
    DrivenSplit,
}

/// What the row expects: these effects, each exactly once an iteration
/// and in this order, each in a turn of its own with the responder free,
/// its nested `wake:` delivered; and the `paced` kinds, each run so too.
/// A paced effect is one baud holds to a pace of its own -- a program's
/// copy is announced at most once per message's length (`copied.rs`) --
/// so how many fall in a run is not the row's to count, but none at all
/// is a paced effect gone, and fails.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Expect {
    each: &'static [EffectKind],
    paced: &'static [EffectKind],
}

impl Expect {
    /// One effect an iteration, nothing paced.
    const fn effect(kind: &'static [EffectKind; 1]) -> Expect {
        Expect {
            each: kind,
            paced: &[],
        }
    }
}

/// One row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Site {
    name: SiteName,
    cause: Cause,
    expect: Expect,
    state: RowState,
}

/// What a row came to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Observed {
    Passed,
    Failed(Failure),
}

/// The harness's judgement of a row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Verdict {
    Pass,
    /// A green row failed.
    Broke(Failure),
    /// A pending row passed: move it to green on purpose.
    PassedWhilePending,
    /// A pending row failed, but not as recorded.
    FailedOtherwise(Failure),
    /// A pending row outlived its stage.
    PastItsStage,
    /// A pending row is left after the last stage.
    PendingAfterTheLast,
    /// A control passed: the rig no longer sees what it was built to.
    ControlPassed,
}

/// **The pending mechanism**: a row's state, what it came to, and the
/// stage the work is at.
fn verdict(state: RowState, observed: Observed, current: Stage) -> Verdict {
    match (state, observed) {
        (RowState::Green, Observed::Passed) => Verdict::Pass,
        (RowState::Green, Observed::Failed(f)) => Verdict::Broke(f),
        (RowState::Control(recorded), Observed::Failed(f)) if f == recorded => Verdict::Pass,
        (RowState::Control(_), Observed::Failed(f)) => Verdict::FailedOtherwise(f),
        (RowState::Control(_), Observed::Passed) => Verdict::ControlPassed,
        (RowState::Pending { .. }, _) if current == Stage::Done => Verdict::PendingAfterTheLast,
        (RowState::Pending { stage, .. }, _) if current > stage => Verdict::PastItsStage,
        (RowState::Pending { .. }, Observed::Passed) => Verdict::PassedWhilePending,
        (RowState::Pending { recorded, .. }, Observed::Failed(f)) if f == recorded => Verdict::Pass,
        (RowState::Pending { .. }, Observed::Failed(f)) => Verdict::FailedOtherwise(f),
    }
}

/// **The reply read to its terminator**, ST (`ESC \`), a byte at a time
/// into `r`, and the line's settings put back: a macro, so the lines below
/// can `concat!` it.
macro_rules! to_st {
    () => {
        concat!(
            r#"e=$(printf '\033'); r=; p=; "#,
            r#"while c=$(dd bs=1 count=1 2>/dev/null); [ -n "$c" ]; do "#,
            r#"r="$r$c"; [ "$p$c" = "$e\\" ] && break; p=$c; done; "#,
            r"stty echo icanon; ",
        )
    };
}

/// **A kitty transmit whose reply the shell reads to its end**: echo and
/// line editing off, a byte at a time until the last two are `ESC \`, the
/// reply's own terminator (ST), so any reply, an `OK` or an error, split
/// anywhere, is read whole and nothing of it is left for the next line
/// typed; then a copy on an `OK` followed by that ST and a clear, a
/// sentinel, on anything else, so the shell's end is an event either way
/// and a copy says the read reached the terminator. A read that returned
/// on the first bytes took half a reply when the tty handed it over in two
/// pieces, read the half as no `OK`, and left the rest to garble the next
/// line; a read of a fixed 11 bytes, an `OK`'s length, would leave a longer
/// error's tail the same way. Its wait is not the row's line: each byte is
/// waited for as long as it takes, the rig's own 10-second backstop the
/// only bound ([`Cause::ProgramUntil`] holds a reply past the rig's two
/// seconds, and writes it in two pieces, on purpose). The picture is
/// `baud-font/tests/png/g8-1x1-n.png`, a 1x1 grey PNG, whose decode is
/// deferred like any PNG's.
const KITTY_OK: &str = concat!(
    r"stty -echo -icanon min 1 time 0; ",
    r"printf '\033_Ga=T,f=100,i=7;",
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAAAAAA6fptVAAAACklEQVR42mPoAwAAkACPLN3KfgAAAABJRU5ErkJggg==",
    r"\033\\'; ",
    to_st!(),
    r#"case $r in *OK"$e\\") printf '\033]52;c;aGk=\007';; *) printf '\033]52;c;!\007';; esac"#,
);

/// **A kitty transmit the terminal refuses, its error read to its end**:
/// the 1x1 picture of [`KITTY_OK`] with its compressed data split over
/// two `IDAT` chunks and the second's CRC one bit off. The header and the
/// first `IDAT` parse, so the decode is deferred as a good picture's is
/// and fails on its step, where an `OK` would leave: the error, 42 bytes
/// against an `OK`'s 11, is held and halved as the `OK` is. Four bytes
/// that are no PNG fail at the header, in the read's own turn, an answer
/// the hold never sees. A copy on an error (`;E`, every error code
/// starting with `E`) followed by its ST, and the sentinel on anything
/// else. Each iteration ending on its copy says the error was read whole,
/// the next line clean: the error's first half already holds `;E`, so a
/// match on the code alone would copy on half a reply.
const KITTY_ERROR: &str = concat!(
    r"stty -echo -icanon min 1 time 0; ",
    r"printf '\033_Ga=T,f=100,i=8;",
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAAAAAA6fptVAAAAAklEQVR42v0bdY4AAAAISURBVGPoAwAAkACPhuNGQQAAAABJRU5ErkJggg==",
    r"\033\\'; ",
    to_st!(),
    r#"case $r in *";E"*"$e\\") printf '\033]52;c;aGk=\007';; *) printf '\033]52;c;!\007';; esac"#,
);

const SITES: [Site; 32] = [
    Site {
        name: SiteName("a printed path's reveal"),
        cause: Cause::PrintedPath(Rule::Reveal, Ticks::Driven),
        expect: Expect::effect(&[EffectKind::Reveal]),
        state: RowState::Green,
    },
    Site {
        name: SiteName("a printed path's [link.path] command"),
        cause: Cause::PrintedPath(Rule::Command, Ticks::Driven),
        expect: Expect::effect(&[EffectKind::Command]),
        state: RowState::Green,
    },
    Site {
        name: SiteName("a printed path's copy"),
        cause: Cause::PrintedPath(Rule::Copy, Ticks::Driven),
        // The copy, and the words VoiceOver is given for it (`say_copied`),
        // each in a turn of its own.
        expect: Expect {
            each: &[EffectKind::Copy, EffectKind::Announce],
            paced: &[],
        },
        state: RowState::Green,
    },
    Site {
        name: SiteName("a printed path's reveal, with no tick at all"),
        cause: Cause::PrintedPath(Rule::Reveal, Ticks::None),
        expect: Expect::effect(&[EffectKind::Reveal]),
        state: RowState::Green,
    },
    // **The control.** The reveal row with the effect queue's turn run
    // inline, as the reveal was handed over before it had a turn: the reveal runs
    // inside the callback the click's answer arrives in -- the worker's
    // `wake`, where f1ed2869 put it, not the frame -- with the responder
    // held, and its nested `wake:` is held until the call returns
    // (macos-liquid-glass 1.0.0-beta.9). On beta.8 the child aborted
    // here, the crash.
    Site {
        name: SiteName("a printed path's reveal, made inside the callback"),
        cause: Cause::PrintedPath(Rule::Reveal, Ticks::DrivenInline),
        expect: Expect::effect(&[EffectKind::Reveal]),
        state: RowState::Control(Failure::InCallback(EffectKind::Reveal, Nested::Never)),
    },
    // **The frame's control: the crash's own shape.** The effect queue's
    // turn is parked and baud's `frame` runs it, as the reveal was made in
    // `frame` before it had a turn: the reveal runs inside the frame with
    // the responder held, and its nested `wake:` is held until `frame`
    // returns (macos-liquid-glass 1.0.0-beta.9). On beta.8 this was the
    // abort.
    Site {
        name: SiteName("a printed path's reveal, made inside the frame"),
        cause: Cause::PrintedPath(Rule::Reveal, Ticks::DrivenInFrame),
        expect: Expect::effect(&[EffectKind::Reveal]),
        state: RowState::Control(Failure::InFrame(EffectKind::Reveal, Nested::Never)),
    },
    // **A link's window**: `[link.path] action = "window"`, the window
    // opened on the effect queue's turn with the responder free, recorded
    // by the probe and never made.
    Site {
        name: SiteName("a printed path's [link.path] window"),
        cause: Cause::PrintedPath(Rule::Window, Ticks::Driven),
        expect: Expect::effect(&[EffectKind::OpenWindow]),
        state: RowState::Green,
    },
    // **Its control**: the effect queue's turn run inline, so the window
    // would open inside the callback the click's answer arrives in -- the
    // shape the new window's taking the keyboard re-enters the view in.
    Site {
        name: SiteName("a printed path's [link.path] window, made inside the callback"),
        cause: Cause::PrintedPath(Rule::Window, Ticks::DrivenInline),
        expect: Expect::effect(&[EffectKind::OpenWindow]),
        state: RowState::Control(Failure::InCallback(EffectKind::OpenWindow, Nested::Never)),
    },
    // **Stage 2's rows: what a program writes**, each made on the effect
    // queue's turn.
    Site {
        name: SiteName("OSC 52's write to the pasteboard"),
        cause: Cause::Program(Program {
            line: r"printf '\033]52;c;aGk=\007'",
            config: "",
            shape: READ,
        }),
        expect: Expect {
            each: &[EffectKind::Copy],
            paced: &[EffectKind::Announce],
        },
        state: RowState::Green,
    },
    Site {
        name: SiteName("OSC 52's clear of the pasteboard"),
        cause: Cause::Program(Program {
            line: r"printf '\033]52;c;!\007'",
            config: "",
            shape: READ,
        }),
        expect: Expect::effect(&[EffectKind::Clear]),
        state: RowState::Green,
    },
    // **A kitty picture's `OK`, on the turn its decode ends**: a direct
    // PNG, `a=T`, is placed at once and its pixels are made a step a turn,
    // never on the display link, so its reply leaves with no tick driven,
    // as from a covered or minimised window, where none comes. The shell
    // reads the reply raw, for a second at most, and copies on an `OK`:
    // the copy is the sign the reply came. The probe build steps the
    // decode a byte of work at a time, so the picture takes several turns,
    // each owed by the step before it. Breaks: the step back in the frame,
    // or the step that leaves more owing no turn, and the undriven arm
    // never copies.
    Site {
        name: SiteName("a kitty picture's OK, with or without a tick"),
        cause: Cause::ProgramUntil(
            Program {
                line: KITTY_OK,
                config: "",
                shape: READ,
            },
            Until::Either,
        ),
        expect: Expect {
            each: &[EffectKind::Copy],
            paced: &[EffectKind::Announce],
        },
        state: RowState::Green,
    },
    // **A ReGIS delay's held reads go on at its deadline, with no tick**:
    // `S(T30)` holds what follows it half a second, and the copy after it
    // is made once the wake scheduled for the deadline releases it
    // (`Driver::pace_wake`), the frame undriven as from a covered window.
    // Break: no wake at the deadline, and the undriven arm never copies.
    Site {
        name: SiteName("a ReGIS delay's held reads, released with or without a tick"),
        cause: Cause::Program(Program {
            line: r"printf '\033PpS(T30)\033\\\033]52;c;aGk=\007'",
            config: "",
            shape: DELAYS,
        }),
        expect: Expect {
            each: &[EffectKind::Copy],
            paced: &[EffectKind::Announce],
        },
        state: RowState::Green,
    },
    Site {
        name: SiteName("the bell's Dock bounce, mode 1042"),
        cause: Cause::Program(Program {
            line: r"printf '\033[?1042h\007'",
            config: "",
            shape: READ,
        }),
        expect: Expect::effect(&[EffectKind::Attention]),
        state: RowState::Green,
    },
    // **The bell's raise, `?1043`, ended on its own event**: the bell and
    // then the sentinel, a clear, written last every time, so the
    // iteration ends on the clear and finds the raise before it, or no
    // raise (Unanswered: a bell lost); never on the rig's two seconds,
    // where a lost bell and a program late to print both read NotRun.
    Site {
        name: SiteName("the bell's raise, mode 1043"),
        cause: Cause::ProgramUntil(
            Program {
                line: r"printf '\033[?1043h\007'; printf '\033]52;c;!\007'",
                config: "",
                shape: READ,
            },
            Until::Sentinel,
        ),
        expect: Expect::effect(&[EffectKind::Front]),
        state: RowState::Green,
    },
    // **The same raise, its program late on purpose**: the shell sleeps
    // 2.5 s before the bell, past the rig's two-second bound, the wall time
    // a loaded machine can take. Break: the row ended on that bound
    // (`Cause::Program`), and every arm reads NotRun.
    Site {
        name: SiteName("the bell's raise, mode 1043, its program late"),
        cause: Cause::ProgramUntil(
            Program {
                line: r"sleep 2.5; printf '\033[?1043h\007'; printf '\033]52;c;!\007'",
                config: "",
                shape: HELD,
            },
            Until::Sentinel,
        ),
        expect: Expect::effect(&[EffectKind::Front]),
        state: RowState::Green,
    },
    // **The same raise, every other ring late**: on alternate iterations
    // the shell sleeps 80 ms before the bell, past the rig's 50-ms floor
    // between iterations, so the next bell follows that late ring at once.
    // The terminal rings bells within one frame interval once
    // (`Ringer::hear`), so the floor is counted from the iteration's
    // records. Break: the floor counted from the iteration's start, and the
    // fast bells, a few milliseconds after the late ones, are absorbed:
    // Unanswered.
    Site {
        name: SiteName("the bell's raise, mode 1043, every other ring late"),
        cause: Cause::ProgramUntil(
            Program {
                line: concat!(
                    r#"if [ -e "$HOME/late" ]; then rm "$HOME/late"; "#,
                    r#"else : > "$HOME/late"; sleep 0.08; fi; "#,
                    r"printf '\033[?1043h\007'; printf '\033]52;c;!\007'",
                ),
                config: "",
                shape: READ,
            },
            Until::Sentinel,
        ),
        expect: Expect::effect(&[EffectKind::Front]),
        state: RowState::Green,
    },
    // **The bell's sentinel written apart from its bell**: the shell pauses
    // 200 ms between the two writes, past the floor that follows an
    // iteration's records, so the bell and the clear are two reads and the
    // bell's frame comes before the clear's read exists; then 100 ms more
    // before the shell takes the next line. The iteration ends on the
    // clear, the program's end, so the pauses can be any length. Break: the
    // iteration ended on the raise (`Until::Either`), and the clear comes
    // alone in the next iteration, 100 ms before that iteration's bell:
    // Unanswered every second iteration. Without the pause after it the
    // clear would ride with the next bell, the shell taking the next line
    // as soon as the clear is written, and that break would pass.
    Site {
        name: SiteName("the bell's sentinel written apart from its bell"),
        cause: Cause::ProgramUntil(
            Program {
                line: r"printf '\033[?1043h\007'; sleep 0.2; printf '\033]52;c;!\007'; sleep 0.1",
                config: "",
                shape: PAUSED,
            },
            Until::Sentinel,
        ),
        expect: Expect::effect(&[EffectKind::Front]),
        state: RowState::Green,
    },
    // **A program's window operations, `CSI t`**: an iconify and a full
    // screen's entry, each the probe's in a turn of its own after the read
    // that asked (`window_later`); the child checks the window never
    // minimised, never in full screen and never shown.
    Site {
        name: SiteName("XTWINOPS's iconify and full screen"),
        cause: Cause::Program(Program {
            line: r"printf '\033[2t\033[10;1t'",
            config: "",
            shape: READ,
        }),
        expect: Expect {
            each: &[EffectKind::Miniaturize, EffectKind::FullScreen],
            paced: &[],
        },
        state: RowState::Green,
    },
    // **A program's notifications** (`OSC 99`), each post made on the
    // effect queue's turn. The first waits for permission, asked once
    // (`Authorize`), and the probe's grant comes on a turn of its own,
    // which sends what waited (`Center::flush`), with or without a tick.
    Site {
        name: SiteName("a program's notification, posted"),
        cause: Cause::Program(Program {
            line: r"printf '\033]99;;Build\033\\'",
            config: "",
            shape: FOUR_POSTS,
        }),
        expect: Expect {
            each: &[EffectKind::Notice],
            paced: &[EffectKind::Authorize],
        },
        state: RowState::Green,
    },
    // Closed in the read that posted it, while the post waits for the
    // grant: removed on the turn, the waiting post with it, so the grant
    // sends nothing.
    Site {
        name: SiteName("a program's notification, closed before it is allowed"),
        cause: Cause::Program(Program {
            line: r"printf '\033]99;i=r;Build\033\\\033]99;i=r:p=close;\033\\'",
            config: "",
            shape: ONCE,
        }),
        expect: Expect {
            each: &[EffectKind::Unnotice],
            paced: &[EffectKind::Authorize],
        },
        state: RowState::Green,
    },
    // Expired a second after it was asked (`w=1000`): removed on a turn
    // `turn::after` schedules, well after the grant's turn has sent it.
    Site {
        name: SiteName("a program's notification, expired"),
        cause: Cause::Program(Program {
            line: r"printf '\033]99;w=1000;Build\033\\'",
            config: "",
            shape: FOUR_POSTS,
        }),
        expect: Expect {
            each: &[EffectKind::Notice, EffectKind::Unnotice],
            paced: &[EffectKind::Authorize],
        },
        state: RowState::Green,
    },
    // **A program's progress on the Dock tile** (`OSC 9;4`), a new value
    // each iteration, so each changes the tile.
    Site {
        name: SiteName("a program's progress, on the Dock tile"),
        cause: Cause::Program(Program {
            line: r#"n=$((n+1)); printf '\033]9;4;1;%d\007' "$n""#,
            config: "",
            shape: READ,
        }),
        expect: Expect::effect(&[EffectKind::DockBar]),
        state: RowState::Green,
    },
    // **A program's critical attention request** (iTerm2's
    // `RequestAttention=yes`): each one cancels the request before it,
    // whose id the runner keeps, and asks again.
    Site {
        name: SiteName("a program's attention request, critical"),
        cause: Cause::Program(Program {
            line: r"printf '\033]1337;RequestAttention=yes\007'",
            config: "",
            shape: READ,
        }),
        expect: Expect {
            each: &[EffectKind::Attention],
            paced: &[EffectKind::Unattention],
        },
        state: RowState::Green,
    },
    // **Its control**: the expiry's `turn::after` alone made inline, in
    // the callback whose read asked for it, as the expiry was made before
    // it had a turn. The post is asked first, on the queue's turn; the
    // expiry's removal, made in the callback, spins the nested loop the
    // probe makes there, which runs that queued turn inside the callback
    // too, so the first record is the post's ask, held. With the door on a
    // turn of its own the row passes, and the control is red. The frame is
    // not driven: the tty can hand back the typed line's echo in pieces,
    // and a tick after a wake that drained only the first would drain the
    // rest, the notification with it, in a frame. Driven, the row read
    // the post's ask in a frame on some runs and in the callback on
    // others, and only with the macOS 27 SDK linked; no header, release
    // note or documentation page of Apple's says the link changes when a
    // run loop services the reader's wake or the display link
    // ([[source: apple-macos-27-release-notes]]). The ask is right in
    // either place, since feeding the terminal in a frame is not an
    // effect; the split arm holds what the driven row stopped holding
    // (`Ticks::DrivenSplit`).
    Site {
        name: SiteName("a program's notification, its expiry made inside the callback"),
        cause: Cause::Program(Program {
            line: r"printf '\033]99;w=1000;Build\033\\'",
            config: "",
            shape: Shape {
                polled: Polled::AfterInline,
                iterations: Iterations(1),
            },
        }),
        expect: Expect {
            each: &[EffectKind::Notice, EffectKind::Unnotice],
            paced: &[EffectKind::Authorize],
        },
        state: RowState::Control(Failure::InCallback(EffectKind::Authorize, Nested::Never)),
    },
    // **A program's attention request, then its cancel**, in two reads:
    // the runner keeps the request's id and the cancel finds it.
    Site {
        name: SiteName("a program's attention request, then its cancel"),
        cause: Cause::ProgramThen(
            Program {
                line: r"printf '\033]1337;RequestAttention=yes\007'",
                config: "",
                shape: READ,
            },
            r"printf '\033]1337;RequestAttention=no\007'",
        ),
        expect: Expect {
            each: &[EffectKind::Attention, EffectKind::Unattention],
            paced: &[],
        },
        state: RowState::Green,
    },
    // **A window that closes with a program's attention request kept**:
    // the close as its observer takes it (`on_close`), and the request
    // cancelled on the close's own turn, with no call to the window.
    Site {
        name: SiteName("a window's close, cancelling its kept attention request"),
        cause: Cause::Gesture(Gesture {
            act: RigAct::Close,
            setup: r"printf '\033]1337;RequestAttention=yes\007\033]52;c;!\007'",
            config: "",
            focus: RigFocus::Real,
            shape: ONCE,
        }),
        expect: Expect::effect(&[EffectKind::Unattention]),
        state: RowState::Green,
    },
    // **Stage 2's rows: what a gesture asks**, a press or a menu item sent
    // to the view's own methods. baud reads every left press in a window
    // it believes is not key as the click that brings it forward, and the
    // rig's window is never key, so a row that needs a click goes through
    // the probe's focus door (`RigFocus::Believed`).
    Site {
        name: SiteName("a selection, copied by the Edit menu"),
        cause: Cause::Gesture(Gesture {
            act: RigAct::CopyMenu,
            setup: r"printf '\033[H\033[2JWORD\033]52;c;!\007'",
            config: "[clipboard]\ncopy_on_select = \"off\"\n",
            focus: RigFocus::Believed,
            shape: READ,
        }),
        expect: Expect {
            each: &[EffectKind::Copy],
            paced: &[EffectKind::Announce],
        },
        state: RowState::Green,
    },
    // **A link's `[link.*]` command, Cmd-clicked**, through the focus door:
    // the command is asked on the press's release and started on the effect
    // queue's turn. No row turns on focus reporting (`?1004`), so the focus
    // baud believes writes no `CSI I` to the shell
    // ([`no_row_turns_on_focus_reporting`]).
    Site {
        name: SiteName("a link's [link.*] command, Cmd-clicked"),
        cause: Cause::Gesture(LINK_CLICK),
        expect: Expect::effect(&[EffectKind::Command]),
        state: RowState::Green,
    },
    // **The door's control**: the same Cmd-click with baud believing AppKit
    // alone. The press brings the window forward, no link is armed and
    // nothing is asked: the door is what lets the row's press through, and
    // the real path still reads `isKeyWindow()`.
    Site {
        name: SiteName("that Cmd-click, the window not believed key"),
        cause: Cause::Gesture(Gesture {
            focus: RigFocus::Real,
            ..LINK_CLICK
        }),
        expect: Expect::effect(&[EffectKind::Command]),
        state: RowState::Control(Failure::NotRun),
    },
    // **A notification's click**, the delegate's as the framework would
    // call it: the window brought forward on the turn, the app activated
    // and the window made key, each stopped at its probe arm, so the rig's
    // window is never key. The setup's post is sent on the grant's turn,
    // before the first iteration.
    Site {
        name: SiteName("a notification's click, focusing its window"),
        cause: Cause::Gesture(Gesture {
            act: RigAct::NoticeClick,
            setup: r"printf '\033]99;;Build\033\\\033]52;c;!\007'",
            config: "",
            focus: RigFocus::Real,
            shape: CLICKS,
        }),
        expect: Expect {
            each: &[EffectKind::Activate, EffectKind::KeyFront],
            paced: &[],
        },
        state: RowState::Green,
    },
    // **The same `OK`, sent late and in two pieces on purpose**: each reply
    // held 2.5 s (`HELD_REPLY`), scheduled on a wake and never slept, then
    // written as its first half and, 200 ms on, the rest. The wall time a
    // loaded machine took when the shell waited a second for it, and the
    // split a tty can make of one write, each read a reply as none: the row
    // went NotRun, then Unanswered, on runs that fell either way. The hold
    // is past the shell's old second and the rig's two-second bound, so
    // neither decides the row: the iteration ends on the copy or the
    // shell's sentinel, the rig's wait a backstop that says when it is
    // reached. Breaks: the shell's wait back to a second (`time 10`), or
    // its read back to the first bytes (`min 0`), and every arm reads the
    // sentinel, Unanswered; the hold past the backstop, and every arm reads
    // TimedOut, never NotRun.
    Site {
        name: SiteName("a kitty picture's OK, sent late"),
        cause: Cause::ProgramUntil(
            Program {
                line: KITTY_OK,
                config: "",
                shape: HELD,
            },
            Until::EitherHeld,
        ),
        expect: Expect {
            each: &[EffectKind::Copy],
            paced: &[EffectKind::Announce],
        },
        state: RowState::Green,
    },
    // **A kitty picture's error, sent late and in two pieces**: the reply
    // to a picture whose decode fails on its step, held and split as the
    // row above holds and splits its `OK`. An error is longer than an
    // `OK`, so a read of a fixed `OK`'s length would leave its tail to
    // garble the next line; read to its terminator it is consumed whole,
    // and each of the four iterations ends on its own copy. Break: the
    // shell reading once, at least 11 bytes (`min 11`), takes the error's
    // first half, and the row reads Unanswered.
    Site {
        name: SiteName("a kitty picture's error, sent late"),
        cause: Cause::ProgramUntil(
            Program {
                line: KITTY_ERROR,
                config: "",
                shape: HELD,
            },
            Until::EitherHeld,
        ),
        expect: Expect {
            each: &[EffectKind::Copy],
            paced: &[EffectKind::Announce],
        },
        state: RowState::Green,
    },
];

/// **A notification's clicks**, polled by the tick until stage 3, and
/// counted: under the probe the center's list is what it sent, so the
/// notification clicked stays shown however long the iterations take.
const CLICKS: Shape = Shape {
    polled: Polled::Tick {
        until: Stage::Three,
    },
    iterations: ITERATIONS,
};

/// Iterations a row runs in its child.
///
/// **A green program row's driven arm depends on this count.** There, a
/// tick takes the read in about 70-80 of 100 iterations (counted under a
/// break that made the bell's attention inline on the tick's read: 71, 81
/// and 80), and the red fires on the first one. So at 100 iterations a run
/// with no tick read is about 0.3^100; lowering the count weakens that red.
const ITERATIONS: Iterations = Iterations(100);

/// **A link Cmd-clicked**: an `OSC 8` link over the first row's second
/// cell, which `[link.https]` hands to a command.
const LINK_CLICK: Gesture = Gesture {
    act: RigAct::Link,
    setup: r"printf '\033[H\033[2J\033]8;;https://example.com/\033\\LINK\033]8;;\033\\\033]52;c;!\007'",
    config: "[link.https]\ncommand = [\"/usr/bin/true\", \"{url}\"]\n",
    focus: RigFocus::Believed,
    shape: READ,
};

/// **The iterations a row runs**: its shape's, a printed path's
/// [`ITERATIONS`], but three for a control that runs nothing, each of
/// whose iterations waits out the child's bound for an effect that never
/// comes.
fn iterations(site: &Site) -> Iterations {
    match site.state {
        RowState::Control(Failure::NotRun) => Iterations(3),
        _ => site.cause.shape().iterations,
    }
}

/// **This run's folder**, which every row's scratch directory is made in:
/// `<CARGO_TARGET_TMPDIR>/event_rig/r<pid>-<n>`, made once a process with
/// `create_dir`, `n` the first that makes a folder no other run holds, so
/// a pid an older run's folder already carries still takes a fresh one.
/// Two runs on one target, or a run beside an older layout's
/// `event_rig/<row>`, never share a folder, and a run removes nothing it
/// did not make. It is kept after the run: each row's `stdout` and
/// `stderr` in it are what a red is read from.
fn run_root() -> &'static Path {
    static ROOT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    ROOT.get_or_init(|| {
        let rig = Path::new(env!("CARGO_TARGET_TMPDIR")).join("event_rig");
        std::fs::create_dir_all(&rig).expect("the rig's folder");
        (0..)
            .map(|n| rig.join(format!("r{}-{n}", std::process::id())))
            .find(|dir| match std::fs::create_dir(dir) {
                Ok(()) => true,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => false,
                Err(e) => panic!("this run's folder {}: {e}", dir.display()),
            })
            .expect("a name no run holds")
    })
}

/// The scratch directory a row's child lives in: its home, its config,
/// the file it clicks and the FIFO a two-stage act reads, in
/// [`run_root`]. A name this run made before is made again empty: the
/// folder it is removed from is this run's own.
fn scratch(name: &str) -> PathBuf {
    let dir = run_root().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("home")).expect("the scratch home");
    dir
}

/// The row name both runs of [`two_runs_on_one_target_keep_their_homes`]
/// make a home for.
const SHARED_ROW: &str = "shared-row";

/// The test the second run is, by its exact name.
const OTHER_RUN: &str = "the_other_run_makes_a_home_of_the_same_name";

/// **Two runs on one target keep each other's homes.** This run makes the
/// home of [`SHARED_ROW`] and marks it; a second run of this test binary,
/// on the same target, makes a home of the same row name and marks it;
/// then both marks must still be there, in two folders. With homes named
/// per target, the second run's [`scratch`] removed the first's, which is
/// how whole rigs sharing a target failed each other's rows with "the
/// scratch home: Invalid argument", "the row's screen was never drawn" and
/// "the act never read its second stage". The second run has exited
/// before this one looks, so the order is the processes', not a timer's.
/// Named break: [`run_root`] back to the target's one `event_rig` folder.
#[test]
fn two_runs_on_one_target_keep_their_homes() {
    let me = std::process::id().to_string();
    let mine = scratch(SHARED_ROW);
    std::fs::write(mine.join("home").join("run"), &me).expect("this run's mark");
    let out = Command::new(std::env::current_exe().expect("this test binary"))
        .args([OTHER_RUN, "--exact", "--ignored", "--nocapture"])
        .output()
        .expect("the other run");
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "the other run: {said}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let line = |key: &str| {
        said.lines()
            .find_map(|l| l.strip_prefix(key))
            .unwrap_or_else(|| panic!("the other run printed no {key:?}: {said}"))
            .to_owned()
    };
    let (them, theirs) = (line("run: "), PathBuf::from(line("home: ")));
    // A home's mark is the pid of the run that made it: a home removed
    // and made again by the other run has the other run's.
    let mark = |dir: &Path| std::fs::read_to_string(dir.join("home").join("run")).ok();
    assert_eq!(
        mark(&mine).as_deref(),
        Some(me.as_str()),
        "run {me} lost its home {} when run {them} made {} on the same target",
        mine.display(),
        theirs.display()
    );
    assert_eq!(
        mark(&theirs).as_deref(),
        Some(them.as_str()),
        "run {them} lost its home {}",
        theirs.display()
    );
    assert_ne!(mine, theirs, "runs {me} and {them} were given one home");
}

/// **The second run of [`two_runs_on_one_target_keep_their_homes`]**,
/// which starts it: a home of [`SHARED_ROW`], marked, and its folder and
/// process printed.
#[test]
#[ignore = "the second run of two_runs_on_one_target_keep_their_homes, which starts it"]
fn the_other_run_makes_a_home_of_the_same_name() {
    let dir = scratch(SHARED_ROW);
    std::fs::write(dir.join("home").join("run"), std::process::id().to_string())
        .expect("the other run's mark");
    println!("run: {}", std::process::id());
    println!("home: {}", dir.display());
}

fn config(rule: Rule) -> &'static str {
    match rule {
        Rule::Reveal => "[link]\npaths = \"on\"\n",
        Rule::Command => {
            "[link]\npaths = \"on\"\n\n[link.path]\ncommand = [\"/usr/bin/true\", \"{path}\"]\n"
        }
        Rule::Copy => "[link]\npaths = \"on\"\n\n[link.path]\naction = \"copy\"\n",
        Rule::Window => {
            "[link]\npaths = \"on\"\n\n[link.path]\naction = \"window\"\n\
             command = [\"/usr/bin/true\", \"{path}\"]\n"
        }
    }
}

/// **How a row's children drive the frame.** A program's output is read
/// by whichever of the reader's `wake` and the frame comes first, so a
/// green program row runs twice: undriven, where the read is the wake's,
/// and driven, where a tick can read it first. Both must make the effect
/// on its turn, so a read on the tick that still makes it inline goes
/// red. A pending program row runs undriven alone: with the frame driven,
/// its recorded site would be one entry or the other by timing -- and so
/// does the `turn::after` control, whose recorded site is the callback. A row
/// whose effect the tick alone asks ([`Polled::Tick`]) runs driven alone.
/// A green program row runs undriven, driven, and driven with each read
/// left to the frame ([`Ticks::DrivenSplit`]), as the tty's pieces can
/// fall, its effects still each on a turn of its own.
fn arms(site: &Site) -> &'static [Ticks] {
    match (site.cause, site.state, site.cause.shape().polled) {
        (Cause::PrintedPath(_, Ticks::Driven), ..) => &[Ticks::Driven],
        (Cause::PrintedPath(_, Ticks::None), ..) => &[Ticks::None],
        (Cause::PrintedPath(_, Ticks::DrivenInline), ..) => &[Ticks::DrivenInline],
        (Cause::PrintedPath(_, Ticks::DrivenInFrame), ..) => &[Ticks::DrivenInFrame],
        (Cause::PrintedPath(_, Ticks::DrivenSplit), ..) => &[Ticks::DrivenSplit],
        (Cause::PrintedPath(_, Ticks::UndrivenAfterInline), ..) | (_, _, Polled::AfterInline) => {
            &[Ticks::UndrivenAfterInline]
        }
        (_, _, Polled::Tick { .. }) => &[Ticks::Driven],
        (
            Cause::Program(_) | Cause::ProgramUntil(..) | Cause::ProgramThen(..),
            RowState::Green,
            Polled::Read,
        ) => &[Ticks::None, Ticks::Driven, Ticks::DrivenSplit],
        (
            Cause::Program(_)
            | Cause::ProgramUntil(..)
            | Cause::ProgramThen(..)
            | Cause::Gesture(_),
            ..,
        ) => &[Ticks::None],
    }
}

/// The row's cause in a child for each of its [`arms`]; what the first
/// that did not pass came to, or that all passed.
fn run(site: &Site, slug: &str) -> Observed {
    for (i, &ticks) in arms(site).iter().enumerate() {
        let observed = run_arm(site, &format!("{slug}-{i}"), ticks);
        println!("  {:?}, {ticks:?}: {observed:?}", site.name);
        if observed != Observed::Passed {
            return observed;
        }
    }
    Observed::Passed
}

/// The row's cause in one child, the frame driven as `ticks` says.
fn run_arm(site: &Site, slug: &str, ticks: Ticks) -> Observed {
    let expect = site.expect;
    let dir = scratch(slug);
    let conf = dir.join("config.toml");
    let (act, arg, config, focus) = match site.cause {
        Cause::PrintedPath(rule, _) => {
            let file = dir.join("clicked.txt");
            std::fs::write(&file, b"x").expect("the file clicked");
            let file = file.to_str().expect("a UTF-8 path").to_owned();
            (RigAct::Path, file, config(rule), RigFocus::Real)
        }
        Cause::Program(p) => (RigAct::Type, p.line.to_owned(), p.config, RigFocus::Real),
        Cause::ProgramUntil(p, until) => (
            match until {
                Until::Either => RigAct::TypeUntil,
                Until::EitherHeld => RigAct::TypeHeld,
                Until::Sentinel => RigAct::TypeToSentinel,
            },
            p.line.to_owned(),
            p.config,
            RigFocus::Real,
        ),
        Cause::ProgramThen(p, second) => (
            RigAct::TypeTwo,
            format!("{}\n{second}", p.line),
            p.config,
            RigFocus::Real,
        ),
        Cause::Gesture(g) => {
            let file = dir.join("clicked.txt");
            std::fs::write(&file, b"x").expect("the file linked");
            let file = file.to_str().expect("a UTF-8 path");
            (g.act, g.setup.replace("{file}", file), g.config, g.focus)
        }
    };
    std::fs::write(&conf, config).expect("the config");
    // The one call type the child's door parses (`rig_call.rs`).
    let call = RigCall::Event(EventRig {
        act,
        arg,
        iterations: iterations(site),
        ticks: match ticks {
            Ticks::Driven => RigTicks::Driven,
            Ticks::None => RigTicks::None,
            Ticks::DrivenInline => RigTicks::InCallback,
            Ticks::DrivenInFrame => RigTicks::InFrame,
            Ticks::UndrivenAfterInline => RigTicks::AfterInCallback,
            Ticks::DrivenSplit => RigTicks::Split,
        },
        focus,
        each: expect.each.to_vec(),
    });
    let out = Command::new(env!("CARGO_BIN_EXE_baud"))
        .args(call.to_args())
        .env_clear()
        .env("HOME", dir.join("home"))
        .env("ZDOTDIR", dir.join("home"))
        .env("SHELL", "/bin/sh")
        .env("HISTFILE", "/dev/null")
        .env("PATH", "/usr/bin:/bin")
        .env("BAUD_CONFIG", &conf)
        .env("BAUD_TYPING", "off")
        .current_dir(&dir)
        .output()
        .expect("the child");
    judge(expect, iterations(site), &out, &dir)
}

/// What a child's output and exit say.
fn judge(expect: Expect, n: Iterations, out: &std::process::Output, dir: &Path) -> Observed {
    let kinds = expect.each;
    use std::os::unix::process::ExitStatusExt;
    let text = String::from_utf8_lossy(&out.stdout);
    let _ = std::fs::write(dir.join("stdout"), &out.stdout);
    let _ = std::fs::write(dir.join("stderr"), &out.stderr);
    if out.status.signal().is_some() {
        return Observed::Failed(Failure::Aborted);
    }
    let lines: Vec<&str> = text.lines().collect();
    let iterations = usize::from(n.0);
    let mut records = 0;
    let mut paced = vec![0_usize; expect.paced.len()];
    if lines.contains(&"timed out at the backstop") {
        return Observed::Failed(Failure::TimedOut);
    }
    if lines.contains(&"unanswered") {
        return Observed::Failed(Failure::Unanswered);
    }
    for line in &lines {
        if *line == "none" {
            return Observed::Failed(Failure::NotRun);
        }
        let Some(r) = Record::parse(line) else {
            return Observed::Failed(Failure::Child);
        };
        let kind = match expect.paced.iter().position(|&k| k == r.kind) {
            Some(at) => {
                paced[at] += 1;
                r.kind
            }
            None => {
                let kind = kinds[records % kinds.len()];
                records += 1;
                if r.kind != kind {
                    return Observed::Failed(Failure::Times);
                }
                kind
            }
        };
        let want = Record {
            kind,
            at: Where::InTurn,
            responder: Responder::Free,
            nested: Nested::Delivered,
        };
        match (r.at, r.responder) {
            (Where::InFrame, _) => return Observed::Failed(Failure::InFrame(kind, r.nested)),
            (Where::InCallback, _) | (_, Responder::Held) => {
                return Observed::Failed(Failure::InCallback(kind, r.nested));
            }
            (Where::InTurn, Responder::Free) => {}
        }
        if r != want {
            return Observed::Failed(Failure::Lost(kind));
        }
    }
    if records != iterations * kinds.len() {
        return Observed::Failed(Failure::Times);
    }
    if let Some(at) = paced.iter().position(|&seen| seen == 0) {
        return Observed::Failed(Failure::NeverPaced(expect.paced[at]));
    }
    match out.status.success() {
        true => Observed::Passed,
        false => Observed::Failed(Failure::Child),
    }
}

/// **Every row, each in a child of its own**, judged by [`verdict`] at
/// [`CURRENT`]: the green rows pass, and the control fails as recorded.
#[test]
fn every_row_comes_to_its_state() {
    say_load("at the start");
    let mut red = Vec::new();
    for (i, site) in SITES.iter().enumerate() {
        let observed = run(site, &format!("row{i}"));
        let v = verdict(site.state, observed, CURRENT);
        println!("{:?}: {observed:?} -> {v:?}", site.name);
        if v != Verdict::Pass {
            red.push((site.name, v));
        }
    }
    say_load("at the end");
    assert!(red.is_empty(), "{red:?}");
}

/// **No row polls the tick past its stage**: a row that runs driven alone
/// because a poll is still the tick's names the stage that moves the
/// poll, and once the work is past it the row must run both arms. Named
/// break: `CURRENT` past a row's `until`.
#[test]
fn no_row_polls_the_tick_past_its_stage() {
    for site in SITES {
        if let Polled::Tick { until } = site.cause.shape().polled {
            assert!(
                CURRENT <= until,
                "{:?} polls the tick past {until:?}",
                site.name
            );
        }
    }
}

/// **No row turns on focus reporting** (`?1004`): with it on, the focus
/// a row's door has baud believe would write `CSI I` to the shell, input
/// no row expects. Every row's line, setup and config is read, `?1004`
/// alone or among other modes (`?1004;2004h`). Perturbation: a row's setup
/// beginning `\033[?1004h`.
#[test]
fn no_row_turns_on_focus_reporting() {
    for site in SITES {
        let text = match site.cause {
            Cause::PrintedPath(rule, _) => config(rule).to_owned(),
            Cause::Program(p) | Cause::ProgramUntil(p, _) => format!("{} {}", p.line, p.config),
            Cause::ProgramThen(p, second) => format!("{} {second} {}", p.line, p.config),
            Cause::Gesture(g) => format!("{} {}", g.setup, g.config),
        };
        let modes = text.split("[?").skip(1).filter_map(|m| {
            let end = m.find(|c: char| !(c.is_ascii_digit() || c == ';'))?;
            (m[end..].starts_with('h')).then(|| &m[..end])
        });
        for set in modes {
            assert!(
                !set.split(';').any(|m| m == "1004"),
                "{:?} turns on focus reporting",
                site.name
            );
        }
    }
}

/// **The pending mechanism, a fake row in each state.** Each state's
/// break, run once and each failing here alone: a recorded failure judged
/// `FailedOtherwise` (state 1); a pending row that passes judged `Pass`
/// (2); any failure taken as the recorded one (3); the stage check
/// dropped (4); the after-the-last check dropped (5); a control's
/// recorded failure judged `FailedOtherwise` (6).
#[test]
fn a_pending_row_is_red_unless_it_fails_as_recorded_within_its_stage() {
    let recorded = Failure::InFrame(EffectKind::Copy, Nested::Never);
    let pending = RowState::Pending {
        stage: Stage::Two,
        recorded,
    };
    // Pending and failing as recorded, within its stage: passes.
    assert_eq!(
        verdict(pending, Observed::Failed(recorded), Stage::One),
        Verdict::Pass
    );
    assert_eq!(
        verdict(pending, Observed::Failed(recorded), Stage::Two),
        Verdict::Pass
    );
    // Pending but passing: red.
    assert_eq!(
        verdict(pending, Observed::Passed, Stage::One),
        Verdict::PassedWhilePending
    );
    // Pending but failing for another reason: red.
    assert_eq!(
        verdict(pending, Observed::Failed(Failure::Aborted), Stage::One),
        Verdict::FailedOtherwise(Failure::Aborted)
    );
    // Pending past its stage: red, failing as recorded or not.
    assert_eq!(
        verdict(pending, Observed::Failed(recorded), Stage::Three),
        Verdict::PastItsStage
    );
    // Any pending row after the last stage: red, whatever its own stage.
    let last = RowState::Pending {
        stage: Stage::Three,
        recorded,
    };
    assert_eq!(
        verdict(last, Observed::Failed(recorded), Stage::Done),
        Verdict::PendingAfterTheLast
    );
    // And a green row: passes when it passes, red when it fails.
    assert_eq!(
        verdict(RowState::Green, Observed::Passed, Stage::One),
        Verdict::Pass
    );
    assert_eq!(
        verdict(RowState::Green, Observed::Failed(recorded), Stage::One),
        Verdict::Broke(recorded)
    );
    // A control, failing as recorded: passes, at every stage and after the
    // last; passing, or failing otherwise: red.
    let control = RowState::Control(recorded);
    for stage in [Stage::One, Stage::Three, Stage::Done] {
        assert_eq!(
            verdict(control, Observed::Failed(recorded), stage),
            Verdict::Pass
        );
    }
    assert_eq!(
        verdict(control, Observed::Passed, Stage::One),
        Verdict::ControlPassed
    );
    assert_eq!(
        verdict(control, Observed::Failed(Failure::Aborted), Stage::One),
        Verdict::FailedOtherwise(Failure::Aborted)
    );
}

/// **How long a door child is given to exit before the rig ends it.** The
/// door answers before any application work, so a child still running
/// past this is a launch that fell through. The bound only has to
/// outlast a process's start on a loaded machine: it was 1 s, killed at
/// once, which a suite at load can spend before the door runs, and which
/// would have ended a launch that got as far as its Metal device with
/// `SIGKILL` mid-work. 10 s is the bound the repository's other
/// process-ending checks give a loaded machine (`xtask/tests/bounded.rs`,
/// `assert_gone`).
const DOOR_BOUND: Duration = Duration::from_secs(10);

/// **How long a child the rig ends is given to act on `SIGTERM`** before
/// `SIGKILL`. A probe build is a Metal client once it is past its door,
/// and one ended mid-submit by `SIGKILL` alone never finishes the command
/// buffer it was handing over; `SIGTERM` lets the app end its own way.
/// 2 s is the time `scripts/bounded` gives the programs it ends, for the
/// same reason (`GRACE`, `scripts/bounded.rs`).
const CHILD_GRACE: Duration = Duration::from_secs(2);

unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
    fn getloadavg(loadavg: *mut f64, nelem: i32) -> i32;
}

/// **The 1-minute load average, said at a whole rig's start and its end**:
/// a row's pace is the machine's, so a run, a load proof's above all, says
/// what load it ran under. Printed, never judged.
fn say_load(when: &str) {
    let mut avg = [0.0_f64; 1];
    // SAFETY: `getloadavg` writes at most `nelem` (1) samples into `avg`,
    // which holds one.
    let n = unsafe { getloadavg(avg.as_mut_ptr(), 1) };
    match n {
        1 => println!("load average, 1 minute, {when}: {:.2}", avg[0]),
        _ => println!("load average, 1 minute, {when}: not read"),
    }
}

/// `SIGTERM` (`<sys/signal.h>`).
const SIGTERM: i32 = 15;

/// **`child`'s status once it has exited, or `None` if it was still running
/// at `bound` and the rig ended it** ([`end`]).
fn ended_within(child: &mut Child, bound: Duration) -> Option<ExitStatus> {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().expect("its status") {
            return Some(status);
        }
        if start.elapsed() > bound {
            end(child);
            return None;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// **A child ended as the rig ends one: `SIGTERM`, then [`CHILD_GRACE`] to
/// act on it, then `SIGKILL`**, never a stop. Its status.
fn end(child: &mut Child) -> ExitStatus {
    let pid = i32::try_from(child.id()).expect("a pid");
    // SAFETY: a signal to the rig's own child, not yet reaped, so the pid
    // is still its.
    unsafe { kill(pid, SIGTERM) };
    let asked = Instant::now();
    while asked.elapsed() < CHILD_GRACE {
        if let Some(status) = child.try_wait().expect("its status") {
            return status;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let _ = child.kill();
    child.wait().expect("its status")
}

/// **A child the rig ends is asked first** ([`end`]): a stand-in that
/// traps `SIGTERM`, notes it and exits 7 ends by its own exit, not by
/// `SIGKILL`. Named break: `end` calling `child.kill()` first; the note is
/// never written and the status is signal 9.
#[test]
fn a_child_the_rig_ends_gets_sigterm_and_its_grace() {
    use std::os::unix::process::ExitStatusExt;
    let dir = scratch("grace");
    let note = dir.join("termed");
    let ready = dir.join("ready");
    let mut child = Command::new("/bin/sh")
        .arg("-c")
        .arg("trap 'echo TERM > \"$1\"; exit 7' TERM; : > \"$2\"; while :; do sleep 0.01; done")
        .arg("stand-in")
        .arg(&note)
        .arg(&ready)
        .spawn()
        .expect("the stand-in");
    let start = Instant::now();
    while !ready.exists() {
        assert!(
            start.elapsed() < DOOR_BOUND,
            "the stand-in never set its trap"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let status = end(&mut child);
    assert_eq!(
        (status.code(), status.signal()),
        (Some(7), None),
        "the stand-in was not left to act on SIGTERM: {status:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&note).expect("the stand-in's note"),
        "TERM\n"
    );
}

/// **A probe build that is not called as a rig exits 2 at once**, before
/// any application, window or Dock work (`rig_door.rs`): with no
/// arguments, a plain launch's, and with one argument too few for the
/// event rig, which is how a by-hand runner once fell through to a plain
/// launch and showed a window. Each child is given [`DOOR_BOUND`] and ended
/// past it ([`end`]) -- a launch would still be running -- and only calls the door refuses
/// are made, so no window can come of this test. Perturbation: the door
/// exits 3, and both fail on the code.
#[test]
fn a_probe_build_called_as_no_rig_exits_2_at_once() {
    let short = RigCall::Event(EventRig {
        act: RigAct::Path,
        arg: "/nothing".to_owned(),
        iterations: Iterations(1),
        ticks: RigTicks::Driven,
        focus: RigFocus::Real,
        each: vec![EffectKind::Reveal],
    })
    .to_args();
    let short = &short[..short.len() - 1];
    for args in [&[][..], short] {
        let dir = scratch(&format!("door-{}", args.len()));
        let mut child = Command::new(env!("CARGO_BIN_EXE_baud"))
            .args(args)
            .env_clear()
            .env("HOME", dir.join("home"))
            .env("PATH", "/usr/bin:/bin")
            .env("BAUD_CONFIG", dir.join("config.toml"))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("the child");
        let status = ended_within(&mut child, DOOR_BOUND);
        let mut said = String::new();
        let _ =
            std::io::Read::read_to_string(&mut child.stderr.take().expect("its stderr"), &mut said);
        let status =
            status.unwrap_or_else(|| panic!("{args:?}: still running after {DOOR_BOUND:?}"));
        assert_eq!(status.code(), Some(2), "{args:?}: {said}");
        assert!(said.starts_with("baud: "), "{args:?}: {said}");
    }
}

/// **The polls still the tick's**, as a settle names them
/// (`frames::Holder`): while any is held a real build's display link runs,
/// and the frame rows' kept arms fail as recorded. Each row that moves a
/// poll to its event takes its name out; with none left the kept arms
/// must pass.
const HOLDERS: &[&str] = &["FS-A5", "FS-A9"];

/// The focus baud last set, as a frame rig's child printed it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FocusSeen {
    Unset,
    Gained,
    Lost,
}

/// What the last read found under the pointer, as a frame rig's child
/// printed it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PreviewSeen {
    Unset,
    Link,
    Nothing,
}

/// The display link's state as a frame rig's child printed it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum LinkSeen {
    Run,
    Pause,
}

/// **What a frame rig's child printed**: the link's state at the end, the
/// ticks delivered, the door's sends and the frames drawn over the offered
/// ticks, the links made in all, the polls still the tick's, what the last
/// settle found owed, the palettes applied, the focus baud last set, the
/// display options' changes taken, and what the last read found under
/// the pointer.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Counters {
    link: LinkSeen,
    delivered: u32,
    sends: u32,
    made: u32,
    drawn: u64,
    holders: Vec<String>,
    owed: String,
    styled: u32,
    focus: FocusSeen,
    motion: u32,
    display: u32,
    preview: PreviewSeen,
    reads: u32,
    /// The turns that fed the core after the settle.
    fed: u32,
    /// The window's title at the end, from its own line.
    title: String,
    /// A listing row's measurement, from its own line
    /// (`RigFrameTicks::UntilMarked`): the setup's paths asked about, and
    /// the text's and the marks' frames' times; empty for any other row.
    marks: String,
    reaped: u32,
    /// Each followed setting's changes taken: scroller, flashing,
    /// animated.
    followed: [u32; 3],
    /// The hands of this machine's names to the links.
    hands: u32,
    /// The launch splash's starts.
    greeted: u32,
    /// The printed paths' asked about after the settle.
    asked: u32,
    /// The batches that asked about a path the floor after the last that
    /// did held, after the settle.
    held: u32,
    /// The batches sent to the printed paths' worker after the settle.
    batches: u32,
    /// The link lines at rest at the end.
    marked: u32,
    /// The caret's flips after the settle, on its own clock.
    flips: u32,
    /// The splash's starts on a window shown before its first frame, before
    /// anything was typed.
    first: u32,
    /// The ticks the rig offered after the settle, or after a frozen
    /// clock's counts were taken again.
    offered: u32,
    /// What the GPU's completions had counted at the last settle, for the
    /// window's life: frames failed, the slowest commit-to-handler time,
    /// frames past their bound, answered late, and the GPU given up.
    gpu_failed: u64,
    gpu_slowest_us: u64,
    gpu_overdue: u64,
    gpu_late: u64,
    gpu_gave_up: u64,
    /// The GPU frames drawn whole, for the window's life.
    gpu_whole: u64,
    /// What the window's first frame was drawn with, when the rig drew it
    /// as the window's opening does.
    opened: OpenedSeen,
    /// Whether the window still draws on the GPU, or why it gave it up.
    gpu_use: UseSeen,
    /// The frames drawn since the window gave the GPU up, the first on the
    /// CPU among them; none while it has not.
    switched: Option<u64>,
    /// What last set the layer's contents.
    layer: LayerSeen,
    /// The `Gpu`s the child made: one, the window's, unless something made
    /// the device again.
    gpu_made: u64,
    /// The completions the fault seam held at the end.
    gpu_held: u64,
    /// **The printed paths' batches the act's second stage sent, by why
    /// each was due**, to its marks' frame or the end, from their own
    /// line: none for a row that releases no second stage
    /// (`RigWindow::Released`).
    by: Option<SentBy>,
}

/// Whether a frame rig's window still drew on the GPU, or why it gave it
/// up.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum UseSeen {
    Live,
    Stuck,
    Failing,
    Revoked,
    NotPermitted,
}

/// What last set a frame rig's window's layer contents.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum LayerSeen {
    Unset,
    Cpu,
    Gpu,
}

/// What a frame rig's child said its window opened with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum OpenedSeen {
    /// The rig drew no opening frame.
    Unset,
    Gpu,
    Cpu,
    /// Neither path drew it: a window that would open on a surface nothing
    /// drew.
    Undrawn,
}

/// The printed paths' batches by why each was due (baud-app's `By`), and
/// those the floor held whose own deadline the shell's `D` set, and the
/// paths they asked about.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct SentBy {
    ended: u32,
    quiet: u32,
    ceiling: u32,
    floor: u32,
    held_granted: u32,
    /// The paths the batches asked about.
    asked: u32,
}

impl SentBy {
    fn parse(line: &str) -> Option<SentBy> {
        let count = |name: &str| -> Option<u32> {
            line.split(' ')
                .find_map(|kv| kv.strip_prefix(name)?.strip_prefix('='))?
                .parse()
                .ok()
        };
        Some(SentBy {
            ended: count("ended")?,
            quiet: count("quiet")?,
            ceiling: count("ceiling")?,
            floor: count("floor")?,
            held_granted: count("held_granted")?,
            asked: count("asked")?,
        })
    }
}

impl Counters {
    /// **No frame failed, none was past its bound or late, and the GPU was
    /// not given up**: what every row that injects nothing must read, on a
    /// loaded machine as on an idle one.
    fn gpu_clean(&self) -> bool {
        self.gpu_failed == 0
            && self.gpu_overdue == 0
            && self.gpu_late == 0
            && self.gpu_gave_up == 0
            && self.gpu_use == UseSeen::Live
            && self.switched.is_none()
    }
}

impl Counters {
    fn parse(text: &str) -> Option<Counters> {
        let line = text.lines().find_map(|l| l.strip_prefix("frames "))?;
        let field = |name: &str| {
            line.split(' ')
                .find_map(|kv| kv.strip_prefix(name)?.strip_prefix('='))
        };
        let count = |name: &str| field(name)?.parse().ok();
        Some(Counters {
            link: match field("link")? {
                "run" => LinkSeen::Run,
                "pause" => LinkSeen::Pause,
                _ => return None,
            },
            delivered: count("delivered")?,
            sends: count("sends")?,
            made: count("made")?,
            drawn: field("drawn")?.parse().ok()?,
            holders: match field("holders")? {
                "-" => Vec::new(),
                list => list.split(',').map(str::to_owned).collect(),
            },
            owed: field("owed")?.to_owned(),
            styled: count("styled")?,
            focus: match field("focus")? {
                "unset" => FocusSeen::Unset,
                "gained" => FocusSeen::Gained,
                "lost" => FocusSeen::Lost,
                _ => return None,
            },
            motion: count("motion")?,
            display: count("display")?,
            preview: match field("preview")? {
                "unset" => PreviewSeen::Unset,
                "link" => PreviewSeen::Link,
                "none" => PreviewSeen::Nothing,
                _ => return None,
            },
            reads: count("reads")?,
            fed: count("fed")?,
            reaped: count("reaped")?,
            followed: {
                let mut n = field("followed")?.split(',').map(str::parse);
                let mut next = || n.next()?.ok();
                [next()?, next()?, next()?]
            },
            hands: count("hands")?,
            greeted: count("greeted")?,
            asked: count("asked")?,
            held: count("held")?,
            batches: count("batches")?,
            marked: count("marked")?,
            flips: count("flips")?,
            first: count("first")?,
            offered: count("offered")?,
            gpu_failed: field("gpu_failed")?.parse().ok()?,
            gpu_slowest_us: field("gpu_slowest_us")?.parse().ok()?,
            gpu_overdue: field("gpu_overdue")?.parse().ok()?,
            gpu_late: field("gpu_late")?.parse().ok()?,
            gpu_gave_up: field("gpu_gave_up")?.parse().ok()?,
            gpu_whole: field("gpu_whole")?.parse().ok()?,
            opened: match field("opened")? {
                "unset" => OpenedSeen::Unset,
                "gpu" => OpenedSeen::Gpu,
                "cpu" => OpenedSeen::Cpu,
                "undrawn" => OpenedSeen::Undrawn,
                _ => return None,
            },
            gpu_use: match field("gpu_use")? {
                "live" => UseSeen::Live,
                "stuck" => UseSeen::Stuck,
                "failing" => UseSeen::Failing,
                "revoked" => UseSeen::Revoked,
                "not-permitted" => UseSeen::NotPermitted,
                _ => return None,
            },
            switched: match field("switched")? {
                "none" => None,
                n => Some(n.parse().ok()?),
            },
            layer: match field("layer")? {
                "unset" => LayerSeen::Unset,
                "cpu" => LayerSeen::Cpu,
                "gpu" => LayerSeen::Gpu,
                _ => return None,
            },
            gpu_made: field("gpu_made")?.parse().ok()?,
            gpu_held: field("gpu_held")?.parse().ok()?,
            title: text
                .lines()
                .find_map(|l| l.strip_prefix("title "))
                .unwrap_or_default()
                .to_owned(),
            marks: text
                .lines()
                .find_map(|l| l.strip_prefix("marks "))
                .unwrap_or_default()
                .to_owned(),
            by: text
                .lines()
                .find_map(|l| l.strip_prefix("by "))
                .and_then(SentBy::parse),
        })
    }

    /// The paths the setup asked about, from a listing row's own line.
    fn setup_asked(&self) -> Option<u32> {
        self.marks
            .split(' ')
            .find_map(|kv| kv.strip_prefix("setup_asked="))?
            .parse()
            .ok()
    }
}

/// What a frame row expects of the link over the offered ticks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Settles {
    /// Nothing owed: paused all through, no tick delivered, nothing sent,
    /// nothing drawn.
    Still,
    /// Run again by what was owed -- the act's frame or a clock's -- and
    /// paused again: at least `drawn` frames drawn, at most `delivered`
    /// ticks taken for them, at least a run and a pause sent, paused at the
    /// end. A frame owed on a wake may draw on that wake, with no tick.
    Restarts { drawn: u64, delivered: u32 },
    /// A clock that keeps restarting it, a blinking caret's: at least
    /// `flips` flips of the caret, a frame drawn for each but the last at
    /// least and for one more at most, at most `per_flip` ticks taken for
    /// each flip ([`TICKS_A_FLIP`]), and at least a send for each frame,
    /// the link run and paused between them; the state at the end is the
    /// clock's. Counted against the flips, on the caret's own clock, and
    /// not against the frames: a link drawing every tick has drawn four
    /// frames in four ticks, before any flip.
    Blinks { flips: u32, per_flip: u64 },
    /// Every run resumes the one link: at most one made in all.
    OneLink,
    /// **Run by a frame no one who runs the link owes, until it is paid**:
    /// a latency record a key opened and no echo closed runs the link, and
    /// its expiry, which only a tick reads, lets it pause -- at least a
    /// tick delivered, a run and a pause sent, paused at the end with
    /// nothing owed.
    Expires,
    /// **A light/dark flip drawn while the link was paused**: a palette
    /// applied, a frame drawn, and paused again at the end.
    Restyled,
    /// **The window's key change heard with the link paused**: the focus
    /// baud set is this, and the link paused at the end.
    Focused(FocusSeen),
    /// **The display options' change heard with the link paused**: the
    /// motion preference and the display settings each taken, and the link
    /// paused at the end.
    Accessed,
    /// **A link moved under a resting pointer, read by the frame the read
    /// owes**: that frame's read found the link's preview, and the link
    /// paused at the end, so the preview rode the one frame the output owed
    /// and nothing held the link after it.
    Previewed,
    /// **Output on rows away from a resting pointer, and no read of it**:
    /// the link under the pointer previewed by the pointer's own event, at
    /// least `drawn` frames of the output drawn -- one, the act waited for
    /// to its end, guards against a run that drew nothing -- and not one
    /// read of the link under the pointer after the settle.
    Unread { drawn: u64 },
    /// **Scrolling output under a resting pointer, read for the frames**:
    /// the hover read at least once, and never more often than a frame was
    /// drawn -- the tick's and the event frames a fed turn draws (D14)
    /// alike, which the count includes, `present` counting every frame it
    /// draws -- or a turn fed the core. Bounds that hold at any pace; that
    /// a frame takes many fed turns in one read is
    /// [`Settles::ReadOnce`]'s row.
    ReadByFrame,
    /// **Two fed turns and one frame, one read**
    /// (`RigFrameTicks::OneAfterFeeds`): the hover read exactly once, two
    /// turns or more having fed the core and a frame drawn on a tick
    /// offered after them, never more often than a frame was drawn or a
    /// turn fed. A read per feed reads once a turn, twice at least.
    ReadOnce,
    /// **Its positive control, a frame a fed turn** (`EventFrames`): two
    /// frames or more drawn on fed turns with no tick offered, the hover
    /// read in each that drew, at least twice, and never more often than a
    /// frame was drawn or a turn fed.
    ReadByEvents,
    /// **A job started with no tick, in the window's title**: the ticks
    /// withheld, nothing delivered, and the derived title names the job.
    Titled(&'static str),
    /// **A command's child reaped with no tick**: the ticks withheld,
    /// nothing delivered, and the child handed over reaped once it ended.
    Reaped,
    /// **The followed settings heard with no tick**: the ticks withheld,
    /// nothing delivered, and each of the three settings' changes taken.
    Followed,
    /// **A link under the resting pointer previewed with no tick**, judged
    /// on the lifted arm alone (as [`Settles::OneLink`] is): the preview a
    /// link and the link paused there. A miss is recorded as
    /// [`Failure::Held`], a poll the tick makes still holding it.
    PreviewedUnticked,
    /// **The launch splash started with no tick**: the ticks withheld,
    /// nothing delivered, and the splash started once.
    Greeted,
    /// **The splash started on the first frame of a window shown before
    /// it**, with nothing typed: once.
    GreetedFirst,
    /// **A held `p=alive` answered at its bound with no tick**: the ticks
    /// withheld, nothing delivered; the act's clear says the answer came
    /// whole and named what is alive.
    AliveAtBound,
    /// **This machine's names handed with no tick**: the ticks withheld,
    /// nothing delivered, and the read's names handed to the links.
    Handed,
    /// **Output's printed paths asked about with no tick**: the ticks
    /// withheld, nothing delivered, and the path the act's output filed
    /// asked about.
    Filed,
    /// **A listing's printed paths drawn, the batch that asked about them
    /// held by no floor** (`RigFrameTicks::UntilMarked`): at least these
    /// many link lines at rest and exactly these many paths asked about --
    /// another, the echo's, would set a floor of its own -- in two batches at
    /// least -- the one that named the listing's writer and the one its
    /// answer made the rows readable for -- and none of them held by the
    /// floor after a batch that asked about a path.
    Marked(u16),
    /// **A frame held past its bound, counted with no tick, and late
    /// after** (`BAUD_RIG_GPU=hold`): the ticks withheld and none
    /// delivered, the held frame counted overdue once and late once, the
    /// GPU not given up and no frame failed, and a frame drawn after it --
    /// the child checks that one was drawn whole.
    GpuHeld,
    /// **A failed frame, and the window going on** (`BAUD_RIG_GPU=fail`):
    /// a palette applied, the frame it owed failed, none past its bound,
    /// and a frame drawn after it -- the child checks that one was drawn
    /// whole.
    GpuFailed,
    /// **The launch's first frame past its bound, drawn on the CPU**
    /// (`BAUD_RIG_GPU=hold-launch`): the window opened on the CPU's frame,
    /// the launch's GPU frame counted overdue once and late once, the GPU
    /// not given up, and no frame failed.
    OpenedOnCpu,
    /// **A frame the GPU never answers gives the window to the CPU, which
    /// draws every frame after** (`BAUD_RIG_GPU=hold-forever`): the ticks
    /// withheld, the frame counted overdue once and the GPU given up once,
    /// as stuck, and never late; at least four frames drawn from the switch
    /// on -- the first on the CPU and one an act, the child checking each --
    /// the layer last set from the CPU, one `Gpu` made, and the one
    /// completion held at the end, so no frame was committed to the GPU
    /// after it.
    GaveUpStuck,
    /// **The launch with the GPU stuck from its first frame**
    /// (`BAUD_RIG_GPU=hold-launch-forever`): opened on the CPU's frame, the
    /// launch's frame counted overdue once and the GPU given up once, as
    /// stuck, and never late; at least three frames from the switch on, the
    /// layer last set from the CPU, and one `Gpu` made.
    LaunchGaveUp,
    /// **Every frame failing gives the GPU up at the third**
    /// (`BAUD_RIG_GPU=fail-forever`): exactly three failed, as no frame is
    /// committed after the give-up, which says failing; frames drawn on
    /// the CPU after it, the layer last set from the CPU.
    GaveUpFailing,
    /// **Two failed frames in a row are not three**
    /// (`BAUD_RIG_GPU=fail-twice`): exactly two failed, the GPU not given
    /// up and no switch, and the window on the GPU -- the child checks a
    /// whole frame was drawn after the second.
    FailedTwice,
    /// **A device that revoked the process's access gives the GPU up at
    /// once** (`BAUD_RIG_GPU=revoked`): one frame failed, the give-up says
    /// revoked, and frames drawn on the CPU after it.
    GaveUpRevoked,
    /// **A command's printed path drawn on the turns its shell's `D` started**
    /// (`RigFrameTicks::UntilMarked`, `RigWindow::Released`): asked about,
    /// a link line at rest, and every batch the act's second stage sent to
    /// its marks' frame due by the `D` -- one at least, and none by the
    /// quiet, the ceiling or the floor. Counted, not timed.
    MarkedOnD,
    /// **A command's printed path drawn on the turns its shell's `D`
    /// started, past the floor a command just before laid**
    /// (`RigFrameTicks::UntilMarked`, `RigWindow::Released`): two link lines
    /// at rest, the two commands', and every batch the second stage sent to
    /// its marks' frame due by a `D` -- none by the quiet, the ceiling or the
    /// floor, none the floor held under the grant (`held_granted`, the
    /// cap's witness) -- asking about this many paths: two where the second
    /// command's path is new, one where its answer is kept from the first's.
    /// Counted, not timed.
    PastFloor(u32),
    /// **A remote command's printed path dark under its shell's `D`**
    /// (`RigWindow::Released`): a batch the `D` made due went, and still
    /// nothing was asked about and no link line is at rest.
    DarkOnD,
}

impl Settles {
    fn holds(self, c: &Counters) -> bool {
        match self {
            Settles::Still => {
                c.link == LinkSeen::Pause && c.delivered == 0 && c.sends == 0 && c.drawn == 0
            }
            Settles::Restarts { drawn, delivered } => {
                c.link == LinkSeen::Pause
                    && c.drawn >= drawn
                    && c.delivered <= delivered
                    && c.sends >= 2
            }
            Settles::Blinks { flips, per_flip } => {
                let flipped = u64::from(c.flips);
                c.flips >= flips
                    && c.drawn.saturating_add(1) >= flipped
                    && c.drawn <= flipped.saturating_add(1)
                    && u64::from(c.delivered) <= per_flip.saturating_mul(flipped)
                    && u64::from(c.sends) >= c.drawn
            }
            Settles::OneLink => c.made <= 1,
            Settles::Expires => {
                c.link == LinkSeen::Pause && c.delivered >= 1 && c.sends >= 2 && c.owed == "Nothing"
            }
            Settles::Restyled => c.link == LinkSeen::Pause && c.styled >= 1 && c.drawn >= 1,
            Settles::Focused(focus) => c.focus == focus && c.link == LinkSeen::Pause,
            Settles::Accessed => c.link == LinkSeen::Pause && c.motion >= 1 && c.display >= 1,
            Settles::Previewed | Settles::PreviewedUnticked => {
                c.preview == PreviewSeen::Link && c.link == LinkSeen::Pause
            }
            Settles::Titled(job) => c.delivered == 0 && c.title.contains(job),
            Settles::Reaped => c.delivered == 0 && c.reaped >= 1,
            Settles::Followed => c.delivered == 0 && c.followed.iter().all(|&n| n >= 1),
            Settles::Handed => c.delivered == 0 && c.hands >= 1,
            Settles::Filed => c.delivered == 0 && c.asked >= 1,
            Settles::Marked(n) => {
                let n = u32::from(n);
                c.held == 0 && c.marked >= n && c.asked == n && c.batches >= 2
            }
            Settles::GpuHeld => {
                c.delivered == 0
                    && c.gpu_overdue == 1
                    && c.gpu_late == 1
                    && c.gpu_gave_up == 0
                    && c.gpu_failed == 0
                    && c.drawn >= 2
            }
            Settles::GpuFailed => {
                c.gpu_failed == 1
                    && c.gpu_overdue == 0
                    && c.gpu_gave_up == 0
                    && c.styled >= 1
                    && c.drawn >= 2
            }
            Settles::GaveUpStuck => {
                c.gpu_overdue == 1
                    && c.gpu_gave_up == 1
                    && c.gpu_use == UseSeen::Stuck
                    && c.gpu_late == 0
                    && c.gpu_failed == 0
                    && c.switched.is_some_and(|n| n >= 4)
                    && c.layer == LayerSeen::Cpu
                    && c.gpu_made == 1
                    && c.gpu_held == 1
            }
            Settles::LaunchGaveUp => {
                c.opened == OpenedSeen::Cpu
                    && c.gpu_overdue == 1
                    && c.gpu_gave_up == 1
                    && c.gpu_use == UseSeen::Stuck
                    && c.gpu_late == 0
                    && c.gpu_failed == 0
                    && c.switched.is_some_and(|n| n >= 3)
                    && c.layer == LayerSeen::Cpu
                    && c.gpu_made == 1
            }
            Settles::GaveUpFailing => {
                c.gpu_failed == 3
                    && c.gpu_gave_up == 1
                    && c.gpu_use == UseSeen::Failing
                    && c.gpu_overdue == 0
                    && c.switched.is_some_and(|n| n >= 1)
                    && c.layer == LayerSeen::Cpu
                    && c.gpu_made == 1
            }
            Settles::FailedTwice => {
                c.gpu_failed == 2
                    && c.gpu_gave_up == 0
                    && c.gpu_use == UseSeen::Live
                    && c.gpu_overdue == 0
                    && c.switched.is_none()
                    && c.styled >= 1
            }
            Settles::GaveUpRevoked => {
                c.gpu_failed == 1
                    && c.gpu_gave_up == 1
                    && c.gpu_use == UseSeen::Revoked
                    && c.gpu_overdue == 0
                    && c.switched.is_some_and(|n| n >= 1)
                    && c.layer == LayerSeen::Cpu
                    && c.gpu_made == 1
            }
            Settles::OpenedOnCpu => {
                c.opened == OpenedSeen::Cpu
                    && c.gpu_overdue == 1
                    && c.gpu_late == 1
                    && c.gpu_gave_up == 0
                    && c.gpu_failed == 0
            }
            Settles::MarkedOnD => c.by.is_some_and(|by| {
                by.ended >= 1
                    && by.quiet + by.ceiling + by.floor == 0
                    && c.asked >= 1
                    && c.marked >= 1
            }),
            Settles::PastFloor(asks) => c.by.is_some_and(|by| {
                by.ended >= 1
                    && by.quiet + by.ceiling + by.floor == 0
                    && by.held_granted == 0
                    && by.asked == asks
                    && c.marked >= 2
            }),
            Settles::DarkOnD => {
                c.by.is_some_and(|by| by.ended >= 1 && c.asked == 0 && c.marked == 0)
            }
            Settles::Greeted => c.delivered == 0 && c.greeted == 1,
            Settles::GreetedFirst => c.first == 1,
            Settles::AliveAtBound => c.delivered == 0,
            Settles::ReadByFrame => {
                c.reads >= 1 && u64::from(c.reads) <= c.drawn && c.reads <= c.fed
            }
            Settles::ReadOnce => {
                c.reads == 1
                    && c.fed >= 2
                    && c.drawn >= 1
                    && c.offered >= 1
                    && u64::from(c.reads) <= c.drawn
                    && c.reads <= c.fed
            }
            Settles::ReadByEvents => {
                c.reads >= 2
                    && c.drawn >= 2
                    && c.fed >= 2
                    && c.offered == 0
                    && u64::from(c.reads) <= c.drawn
                    && c.reads <= c.fed
            }
            Settles::Unread { drawn } => {
                c.reads == 0 && c.preview == PreviewSeen::Link && c.drawn >= drawn
            }
        }
    }
}

/// One frame row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct FrameSite {
    name: SiteName,
    /// The screen's line, ending with the `OSC 52` clear that signs it.
    setup: &'static str,
    /// What is sent once the link has settled; empty for none.
    act: &'static str,
    /// How the act is sent.
    keys: RigKeys,
    /// Whether the ticks after the act reach the view.
    ticks: RigFrameTicks,
    /// What the window is told after the act.
    window: RigWindow,
    /// The child's environment beyond the rig's own.
    env: &'static [(&'static str, &'static str)],
    config: &'static str,
    focus: RigFocus,
    attempts: Iterations,
    expect: Settles,
    /// The kept arm's state; `Pending` on [`Failure::Held`] while
    /// [`HOLDERS`] is not empty.
    state: RowState,
}

/// **No launch splash and no cursor trail**: the rig's window is never
/// shown, so a splash would wait for it for ever and its wait (`FS-A17`)
/// hold the link; a trail owes every frame while it burns, motion of its
/// own that these rows are not about.
const NO_SPLASH: &str = "[cursor]\ntrail = \"off\"\n[launch]\nsplash = \"off\"\n";

/// **[`NO_SPLASH`], the caret steady**: a focused window's caret blinks by
/// default, and its flips would run the link, a tick that polls what
/// these rows hear.
const STEADY: &str = "[cursor]\nblink = false\ntrail = \"off\"\n[launch]\nsplash = \"off\"\n";

/// A screen of text and the clear that signs it.
const STILL: &str = r"printf 'baud\033]52;c;!\007'";

/// **[`STILL`] on a full screen**, the prompt on its last row: every line
/// the act writes, its echo's newline the first, scrolls the line under a
/// pointer resting on the first row, so every turn that feeds the core
/// moves what the hover's last read covered. On [`STILL`]'s screen the
/// act's echo and its first lines are written below the line read, and a
/// turn that feeds them owes the hover nothing.
const FULL: &str = r"seq 1 300; printf 'baud\033]52;c;!\007'";

/// **The ticks a blinking caret's link takes for each flip**, at
/// most. The flip's deadline runs the link a frame early: a deadline less
/// than a frame away is a frame owed (`frames::owes`), so the tick that
/// restarts it draws the flip or finds it a frame ahead, the next draws
/// it, and the link pauses on the deadline after, a quarter of a second
/// out -- two at most, and one more for a tick that lands across the
/// deadline itself. A link that never paused takes one every period, some
/// thirty a flip at 250 ms.
const TICKS_A_FLIP: u64 = 3;

const FRAME_SITES: [FrameSite; 37] = [
    // **R1, the done-condition's row**: a static screen's link pauses and
    // no tick reaches the view. Stage 3 closes on this row's kept arm,
    // never on its lifted one.
    FrameSite {
        name: SiteName("a static screen pauses the display link"),
        setup: STILL,
        act: "",
        keys: RigKeys::Line,
        ticks: RigFrameTicks::Offered,
        window: RigWindow::Untold,
        env: &[],
        config: NO_SPLASH,
        focus: RigFocus::Real,
        attempts: Iterations(100),
        expect: Settles::Still,
        state: RowState::Pending {
            stage: Stage::Three,
            recorded: Failure::Held,
        },
    },
    // **R2**: a blinking caret runs the link at each flip, scheduled a
    // frame before it, and pauses between. Its flips come on wall time, so
    // its run is the caret's flips (`RigFrameTicks::UntilFlips`), four: a
    // fixed count of offered ticks ran out before the fourth flip when the
    // run loop's passes returned early, and a count of frames would end a
    // link drawing every tick after four ticks. Breaks: each offer's pass
    // at zero with the fixed count, and the lifted arm draws none (red);
    // the caret's blink off, and the run times out at the backstop; a
    // frame owed every tick, and more frames are drawn than flips (red);
    // the link never paused, and more ticks are taken than flips allow.
    FrameSite {
        name: SiteName("a blinking caret runs the link at each flip"),
        setup: STILL,
        act: "",
        keys: RigKeys::Line,
        ticks: RigFrameTicks::UntilFlips,
        window: RigWindow::Untold,
        env: &[],
        config: concat!(
            "[cursor]\nblink = \"true\"\ntrail = \"off\"\n",
            "[launch]\nsplash = \"off\"\n"
        ),
        focus: RigFocus::Believed,
        attempts: Iterations(4),
        expect: Settles::Blinks {
            flips: 4,
            per_flip: TICKS_A_FLIP,
        },
        state: RowState::Pending {
            stage: Stage::Three,
            recorded: Failure::Held,
        },
    },
    // **R3, eager start**: a read's echo owes a frame, and owing it runs
    // the link (`App::owe_frame`), which no settle starts; once drawn it
    // pauses again.
    FrameSite {
        name: SiteName("an echo runs the link, and it pauses again"),
        setup: STILL,
        act: "printf y",
        keys: RigKeys::Line,
        ticks: RigFrameTicks::Offered,
        window: RigWindow::Untold,
        env: &[],
        config: NO_SPLASH,
        focus: RigFocus::Real,
        attempts: Iterations(100),
        expect: Settles::Restarts {
            drawn: 1,
            delivered: 30,
        },
        state: RowState::Pending {
            stage: Stage::Three,
            recorded: Failure::Held,
        },
    },
    // **R4**: a visual bell runs the link to its end, then it pauses.
    FrameSite {
        name: SiteName("a flash runs the link to its end, then it pauses"),
        setup: STILL,
        act: r"printf '\007'",
        keys: RigKeys::Line,
        ticks: RigFrameTicks::Offered,
        window: RigWindow::Untold,
        env: &[],
        config: concat!(
            "bell = \"flash\"\n",
            "[cursor]\ntrail = \"off\"\n[launch]\nsplash = \"off\"\n"
        ),
        focus: RigFocus::Real,
        attempts: Iterations(100),
        expect: Settles::Restarts {
            drawn: 2,
            delivered: 60,
        },
        state: RowState::Pending {
            stage: Stage::Three,
            recorded: Failure::Held,
        },
    },
    // **A frame no one who runs the link owes**: a key the shell does not
    // echo opens a latency record (`BAUD_KEYLAT`), and the record, owed
    // the tick its expiry is read on, runs the link until it expires, then
    // it pauses. The screen reads with echo off and draws nothing.
    FrameSite {
        name: SiteName("a key with no echo runs the link until its record expires"),
        setup: r"stty -echo; printf 'baud\033]52;c;!\007'; cat >/dev/null",
        act: "x",
        keys: RigKeys::Pressed,
        ticks: RigFrameTicks::Offered,
        window: RigWindow::Untold,
        env: &[("BAUD_KEYLAT", "1")],
        config: NO_SPLASH,
        focus: RigFocus::Real,
        attempts: Iterations(400),
        expect: Settles::Expires,
        state: RowState::Pending {
            stage: Stage::Three,
            recorded: Failure::Held,
        },
    },
    // **A light/dark flip while the link is paused** is drawn then, not at
    // the next event: the appearance owes the frame whose pump applies it.
    FrameSite {
        name: SiteName("a flip of light and dark while paused is drawn"),
        setup: STILL,
        act: "",
        keys: RigKeys::Line,
        ticks: RigFrameTicks::Offered,
        window: RigWindow::Restyled,
        env: &[],
        config: NO_SPLASH,
        focus: RigFocus::Real,
        attempts: Iterations(100),
        expect: Settles::Restyled,
        state: RowState::Pending {
            stage: Stage::Three,
            recorded: Failure::Held,
        },
    },
    // **The splash's wait holds no link** (`FS-A17`): the splash on, the
    // window never shown, so the splash waits for ever, and on the lifted
    // arm the link pauses: what it waits for wakes the window
    // (`splash_check`), and no tick polls for it. The kept arm's other
    // holders still run the link.
    FrameSite {
        name: SiteName("a splash waiting for its window holds no link"),
        setup: STILL,
        act: "",
        keys: RigKeys::Line,
        ticks: RigFrameTicks::Offered,
        window: RigWindow::Untold,
        env: &[],
        config: "[cursor]\ntrail = \"off\"\n",
        focus: RigFocus::Real,
        attempts: Iterations(100),
        expect: Settles::Still,
        state: RowState::Pending {
            stage: Stage::Three,
            recorded: Failure::Held,
        },
    },
    // **FS-A17**: the splash starts on the turn its window is shown, the
    // ticks withheld: the occlusion change AppKit posts wakes the window,
    // whose turn finds a frame on the layer and the window on screen (the
    // probe's door, the window never ordered front) and starts the splash,
    // owing its first frame; no tick polls for it.
    FrameSite {
        name: SiteName("the splash starts when its window is shown, with no tick"),
        setup: STILL,
        act: "",
        keys: RigKeys::Line,
        ticks: RigFrameTicks::Withheld,
        window: RigWindow::Shown,
        env: &[],
        config: "[cursor]\nblink = false\ntrail = \"off\"\n",
        focus: RigFocus::Believed,
        attempts: Iterations(100),
        expect: Settles::Greeted,
        state: RowState::Green,
    },
    // **The splash starts on the first CPU frame of a window shown before
    // it, with no tick**: the window told it is shown before anything is
    // typed or drawn (`RigWindow::ShownFirst`), so the splash waits for a
    // frame on the layer; the first, the shell's prompt, is drawn on the
    // CPU path, whose present has no completion, and the present itself
    // wakes the window while the splash waits (`layer_presented`), whose
    // turn starts it. Nothing is typed until then, the ticks are
    // withheld, and the printed paths and links are off, so no batch's
    // answer wakes the window instead.
    FrameSite {
        name: SiteName("the splash starts on the first CPU frame of a window shown before it"),
        setup: STILL,
        act: "",
        keys: RigKeys::Line,
        ticks: RigFrameTicks::Withheld,
        window: RigWindow::ShownFirst,
        env: &[("BAUD_GPU", "cpu")],
        config: concat!(
            "[cursor]\nblink = false\ntrail = \"off\"\n",
            "[link]\nurls = \"off\"\npaths = \"off\"\n",
            "[screen]\nprinting = \"off\"\n"
        ),
        focus: RigFocus::Believed,
        attempts: Iterations(100),
        expect: Settles::GreetedFirst,
        state: RowState::Green,
    },
    // **A held `p=alive` is answered at its bound** (`reads::NOTICE_WAIT`)
    // when Notification Center never answers: the read stops at the
    // question (`NoticeHold::On`), the list is asked for, and the reply
    // leaves on the wake the read's hold scheduled for its deadline
    // (`pace_wake`), the ticks withheld, so nothing else brings that turn.
    // The setup's post is the one alive, and the grant it asks for comes
    // first. The act reads the answer to its `ST` and clears the pasteboard
    // only on `p=alive;k`; anything else copies, so the act never signs.
    FrameSite {
        name: SiteName("a held p=alive is answered at its bound with no tick"),
        setup: r"printf '\033]99;i=k;one\033\\\033]52;c;!\007'",
        act: concat!(
            r"stty -echo -icanon min 1 time 0; ",
            r"printf '\033]99;i=m:p=alive;\033\\'; ",
            to_st!(),
            r#"case $r in *"p=alive;k$e\\") printf '\033]52;c;!\007';; *) printf '\033]52;c;aGk=\007';; esac"#,
        ),
        keys: RigKeys::Line,
        ticks: RigFrameTicks::Withheld,
        window: RigWindow::Untold,
        env: &[("BAUD_RIG_DELIVERED", "stalled")],
        config: concat!(
            "[cursor]\nblink = false\ntrail = \"off\"\n",
            "[link]\nurls = \"off\"\npaths = \"off\"\n",
            "[screen]\nprinting = \"off\"\n"
        ),
        focus: RigFocus::Believed,
        attempts: Iterations(100),
        expect: Settles::AliveAtBound,
        state: RowState::Green,
    },
    // **FS-A2**: the window made key is heard with the link paused, from
    // its notification, and no tick polls for it.
    FrameSite {
        name: SiteName("a window made key is heard with the link paused"),
        setup: STILL,
        act: "",
        keys: RigKeys::Line,
        ticks: RigFrameTicks::Offered,
        window: RigWindow::BecameKey,
        env: &[],
        config: STEADY,
        focus: RigFocus::Real,
        attempts: Iterations(100),
        expect: Settles::Focused(FocusSeen::Gained),
        state: RowState::Pending {
            stage: Stage::Three,
            recorded: Failure::Held,
        },
    },
    // **FS-A2**: and the key window resigned, the other notification.
    FrameSite {
        name: SiteName("a window that resigns key is heard with the link paused"),
        setup: STILL,
        act: "",
        keys: RigKeys::Line,
        ticks: RigFrameTicks::Offered,
        window: RigWindow::ResignedKey,
        env: &[],
        config: STEADY,
        focus: RigFocus::Believed,
        attempts: Iterations(100),
        expect: Settles::Focused(FocusSeen::Lost),
        state: RowState::Pending {
            stage: Stage::Three,
            recorded: Failure::Held,
        },
    },
    // **FS-A2, the opening's reading**: the frame rig opens as `show` does,
    // the view made first responder before its responder is set, so the
    // focus a window not key opens with is the opening pump's reading of
    // the window, which `KeyHeard` starts marked for.
    FrameSite {
        name: SiteName("a window opened not key starts with its focus lost"),
        setup: STILL,
        act: "",
        keys: RigKeys::Line,
        ticks: RigFrameTicks::Offered,
        window: RigWindow::Untold,
        env: &[],
        config: STEADY,
        focus: RigFocus::Real,
        attempts: Iterations(100),
        expect: Settles::Focused(FocusSeen::Lost),
        state: RowState::Pending {
            stage: Stage::Three,
            recorded: Failure::Held,
        },
    },
    // **FS-A2, the view's hook**: a first-responder change heard once the
    // responder is set reads the window, not the hook's word, so a window
    // not key keeps its focus lost.
    FrameSite {
        name: SiteName("a first-responder change in a window not key leaves its focus lost"),
        setup: STILL,
        act: "",
        keys: RigKeys::Line,
        ticks: RigFrameTicks::Offered,
        window: RigWindow::Refocused,
        env: &[],
        config: STEADY,
        focus: RigFocus::Real,
        attempts: Iterations(100),
        expect: Settles::Focused(FocusSeen::Lost),
        state: RowState::Pending {
            stage: Stage::Three,
            recorded: Failure::Held,
        },
    },
    // **FS-A4**: Reduce Motion and Reduce Transparency turned on are heard
    // with the link paused, from the workspace's notification, and no tick
    // polls for them.
    FrameSite {
        name: SiteName("the display options changed are heard with the link paused"),
        setup: STILL,
        act: "",
        keys: RigKeys::Line,
        ticks: RigFrameTicks::Offered,
        window: RigWindow::DisplayOptions,
        env: &[],
        config: STEADY,
        focus: RigFocus::Real,
        attempts: Iterations(100),
        expect: Settles::Accessed,
        state: RowState::Pending {
            stage: Stage::Three,
            recorded: Failure::Held,
        },
    },
    // **FS-A3**: a link printed under a resting pointer is previewed by the
    // frame its read owes: the read runs the link for that frame, whose
    // compare finds the line under the pointer written and reads it. The
    // link pauses after it, the caret steady, so the preview rode that one
    // frame. The pointer moves before the link's bytes arrive, so only that
    // compare can find it; no tick polls for it (the frame rows above and
    // below hold that output elsewhere reads nothing, the source check that
    // no tick reads it).
    FrameSite {
        name: SiteName("a link printed under a resting pointer is previewed by the frame it owes"),
        setup: STILL,
        act: r"printf '\033[H\033]8;;https://example.com/\033\\LINK\033]8;;\033\\'",
        keys: RigKeys::Line,
        ticks: RigFrameTicks::Offered,
        window: RigWindow::PointerOver,
        env: &[],
        config: concat!(
            "[link.https]\ncommand = [\"/usr/bin/true\", \"{url}\"]\n",
            "[cursor]\nblink = false\ntrail = \"off\"\n[launch]\nsplash = \"off\"\n"
        ),
        focus: RigFocus::Believed,
        attempts: Iterations(100),
        expect: Settles::Previewed,
        // The kept arm's holders keep the link running, so it pauses only
        // on the lifted arm until stage 3 takes them out.
        state: RowState::Pending {
            stage: Stage::Three,
            recorded: Failure::Held,
        },
    },
    // **FS-A3, the other side**: output on rows away from a resting pointer
    // reads nothing under it. The pointer rests on a URI on the first row;
    // a loop of the shell's own builtins writes rows nine to fourteen by
    // cursor address, so no row scrolls, the ticks offered, and ends by
    // clearing the pasteboard, which the run waits for, so the whole act is
    // fed and drawn whatever the machine's pace. Not one read of the link
    // under the pointer follows the settle: a turn's output owes the hover a
    // read only where it wrote what the last read went over. The loop starts
    // no program and the printed paths are off: a paths worker answer that
    // moves them owes the hover a read, an event this row is not about.
    FrameSite {
        name: SiteName("output away from a resting pointer reads nothing under it"),
        setup: r"printf '\033[2J\033[Hhttps://example.com/\033[5H\033]52;c;!\007'",
        act: r"i=0; while [ $i -lt 400 ]; do printf '\033[%d;1Hout %d' $((i%6+9)) $i; i=$((i+1)); done; printf '\033]52;c;!\007'",
        keys: RigKeys::Line,
        ticks: RigFrameTicks::Offered,
        window: RigWindow::PointerOver,
        env: &[],
        config: concat!(
            "[link]\npaths = \"off\"\nmissing = \"off\"\n",
            "[link.https]\ncommand = [\"/usr/bin/true\", \"{url}\"]\n",
            "[cursor]\nblink = false\ntrail = \"off\"\n[launch]\nsplash = \"off\"\n"
        ),
        focus: RigFocus::Believed,
        attempts: Iterations(200),
        expect: Settles::Unread { drawn: 1 },
        state: RowState::Green,
    },
    // **FS-A3's M1**: scrolling output under a resting pointer moves the
    // line under it on every read, and the hover is read for the frames
    // that draw it, not for every read: the frame compares what the last
    // read covered and reads, and the pump reads only what an event marked.
    // `seq` scrolls the screen over many reads, the ticks offered, longer
    // than the run on either arm. Here only bounds that hold at any pace:
    // read at least once, and never more often than a frame was drawn --
    // the event frames a fed turn draws (D14) counted with the ticks' -- or
    // a turn fed the core. A count of reads against feeds over offered
    // ticks measured the machine's pace: where the output comes no faster
    // than frames, each feed has a frame of its own and its read. That a
    // frame takes many feeds in one read is the next row's.
    FrameSite {
        name: SiteName("scrolling output under a resting pointer is read once a frame"),
        setup: STILL,
        act: "seq 1 200000",
        keys: RigKeys::Line,
        ticks: RigFrameTicks::Offered,
        window: RigWindow::PointerOver,
        env: &[],
        config: concat!(
            "[link.https]\ncommand = [\"/usr/bin/true\", \"{url}\"]\n",
            "[cursor]\nblink = false\ntrail = \"off\"\n[launch]\nsplash = \"off\"\n"
        ),
        focus: RigFocus::Believed,
        attempts: Iterations(200),
        expect: Settles::ReadByFrame,
        state: RowState::Green,
    },
    // **M1, two feeds to a frame**: `seq` scrolls a full screen ([`FULL`])
    // under the resting pointer, every fed turn moving the line it rests on,
    // with the frame gate's clock frozen (`RigFrameTicks::OneAfterFeeds`), so
    // no fed turn draws a frame for the time since the last; the act is sent
    // once the clock is frozen, a frame owed before it drawn and the counts
    // taken again, so all it feeds is counted; two turns feed the core with no
    // tick and no frame, and one tick then draws a frame that reads the hover
    // once. No other path owes a frame in the window: the act is written to
    // the shell as a line, not pressed, so no key's echo or prediction is owed
    // and no composition marked; its output is digits, so no printed path is
    // asked about and no worker's answer owes the hover a read; the caret is
    // steady (`blink = false`), the trail and the splash off, and the output
    // jumps rather than slides (`[scroll] mode = "normal"`), so no slide's
    // motion owes a frame.
    // Named break: read the hover on every turn that feeds the core --
    // read twice before the tick.
    FrameSite {
        name: SiteName(
            "two fed turns under a resting pointer are read once by the frame after them",
        ),
        setup: FULL,
        act: "seq 1 200000",
        keys: RigKeys::Line,
        ticks: RigFrameTicks::OneAfterFeeds,
        window: RigWindow::PointerOver,
        env: &[],
        config: concat!(
            "[link.https]\ncommand = [\"/usr/bin/true\", \"{url}\"]\n",
            "[cursor]\nblink = false\ntrail = \"off\"\n[launch]\nsplash = \"off\"\n",
            "[scroll]\nmode = \"normal\"\n"
        ),
        focus: RigFocus::Believed,
        attempts: Iterations(1),
        expect: Settles::ReadOnce,
        state: RowState::Green,
    },
    // **Its positive control**: the same output with the frame gate's
    // clock moved on an hour (`RigFrameTicks::EventFrames`), so every
    // commit is an hour old and each fed turn draws a frame of its own
    // with no tick (D14), and each of those frames reads the hover: two
    // frames on two fed turns, two reads at least, no tick offered, on
    // the same full screen, where every fed turn moves the line read. The
    // slide is off as in the row above: a slide owes frames of its own,
    // which any turn draws once the clock has moved on, fed or not. It
    // shows the frozen row's one read is the clock's doing, not a read the
    // frames never make.
    FrameSite {
        name: SiteName("fed turns that each draw a frame are read by each"),
        setup: FULL,
        act: "seq 1 200000",
        keys: RigKeys::Line,
        ticks: RigFrameTicks::EventFrames,
        window: RigWindow::PointerOver,
        env: &[],
        config: concat!(
            "[link.https]\ncommand = [\"/usr/bin/true\", \"{url}\"]\n",
            "[cursor]\nblink = false\ntrail = \"off\"\n[launch]\nsplash = \"off\"\n",
            "[scroll]\nmode = \"normal\"\n"
        ),
        focus: RigFocus::Believed,
        attempts: Iterations(1),
        expect: Settles::ReadByEvents,
        state: RowState::Green,
    },
    // **FS-A12**: a job started at the prompt is in the derived title, the
    // ticks withheld: the read of its echo and the shell's fork and exec,
    // each a turn, owe the check, which the turn makes, and no tick polls
    // for it. `sleep` prints nothing and outlasts the run, so only those
    // turns can name it.
    FrameSite {
        name: SiteName("a job started with no tick is in the title"),
        setup: STILL,
        act: "sleep 30",
        keys: RigKeys::Line,
        ticks: RigFrameTicks::Withheld,
        window: RigWindow::Untold,
        env: &[],
        config: STEADY,
        focus: RigFocus::Believed,
        attempts: Iterations(100),
        expect: Settles::Titled("sleep"),
        state: RowState::Green,
    },
    // **FS-A15**: a command's child, handed to the window as a click's
    // `[link.*]` command is, is reaped at its exit, the ticks withheld: the
    // process watch's thread hears its `NOTE_EXIT`, or the `SIGCHLD` after
    // it, and reaps it, and no tick polls for it. The child is `read` in a
    // shell, held open until the act closes its input; the probe build
    // starts no command of a click's own.
    FrameSite {
        name: SiteName("a command's child is reaped at its exit with no tick"),
        setup: STILL,
        act: "",
        keys: RigKeys::Line,
        ticks: RigFrameTicks::Withheld,
        window: RigWindow::CommandExits,
        env: &[],
        config: STEADY,
        focus: RigFocus::Believed,
        attempts: Iterations(100),
        expect: Settles::Reaped,
        state: RowState::Green,
    },
    // **FS-A16**: the settings the window follows -- "Show scroll bars",
    // Dim Flashing Lights, Animated Images -- are taken on the turn their
    // notification wakes, the ticks withheld: each observer marks its
    // setting and wakes the window, and no tick polls for the marks.
    FrameSite {
        name: SiteName("the followed settings are taken with no tick"),
        setup: STILL,
        act: "",
        keys: RigKeys::Line,
        ticks: RigFrameTicks::Withheld,
        window: RigWindow::Settings,
        env: &[],
        config: STEADY,
        focus: RigFocus::Believed,
        attempts: Iterations(100),
        expect: Settles::Followed,
        state: RowState::Green,
    },
    // **A shell's report naming this machine is judged on `gethostname`'s
    // name alone**, the reverse lookup behind the rest stalled (the rig's
    // seam): an `OSC 7` naming this machine as `/bin/hostname` prints it --
    // the name a shell carries -- is judged at once, so the derived title
    // is the report's folder, the ticks withheld. Before, a report naming
    // a host waited for every name, which the lookup held as long as the
    // network did, and the title stayed the last one sent.
    FrameSite {
        name: SiteName("a report naming this machine titles the window, the lookup stalled"),
        setup: r#"mkdir -p sub && printf '\033]7;file://%s%s/sub\007\033]52;c;!\007' "$(hostname)" "$PWD""#,
        act: "",
        keys: RigKeys::Line,
        ticks: RigFrameTicks::Withheld,
        window: RigWindow::Untold,
        env: &[],
        config: STEADY,
        focus: RigFocus::Believed,
        attempts: Iterations(100),
        expect: Settles::Titled("/sub"),
        state: RowState::Green,
    },
    // **The read of every name hands them with no tick** (B5): the rig's
    // stalled reverse lookup answered with a name `gethostname` does not
    // give, the ticks withheld. The read's thread stores the names and
    // wakes the window, whose turn hands them to the links, a paused
    // window's included; no tick polls for them.
    FrameSite {
        name: SiteName("the names read late are handed with no tick"),
        setup: STILL,
        act: "",
        keys: RigKeys::Line,
        ticks: RigFrameTicks::Withheld,
        window: RigWindow::HostNamed,
        env: &[],
        config: STEADY,
        focus: RigFocus::Believed,
        attempts: Iterations(100),
        expect: Settles::Handed,
        state: RowState::Green,
    },
    // **A title waiting on this machine's names is set when they land,
    // with no tick** (FS-A13): a report naming the host the rig's stalled
    // lookup will answer with, which `gethostname` does not give, waits
    // for every name; the lookup answered, the read's thread stores the
    // names and wakes the window, whose turn judges the report by them
    // and titles the window with its folder, the ticks withheld.
    FrameSite {
        name: SiteName("a title waiting on the names is set when they land, with no tick"),
        setup: r#"mkdir -p named && printf '\033]7;file://baud-rig.invalid%s/named\007\033]52;c;!\007' "$PWD""#,
        act: "",
        keys: RigKeys::Line,
        ticks: RigFrameTicks::Withheld,
        window: RigWindow::HostNamed,
        env: &[],
        config: STEADY,
        focus: RigFocus::Believed,
        attempts: Iterations(100),
        expect: Settles::Titled("/named"),
        state: RowState::Green,
    },
    // **A file link under a resting pointer previews with no tick**: a
    // `file:///tmp` link, which names no host, under the pointer the window
    // is told of, with the link paused. Pending, and judged on the lifted
    // arm, where it fails today: the preview waits on the printed paths'
    // worker's answer to a batch only the tick sends (FS-A14's
    // `paths_turn`), which the kept arm's running link delivers. It goes
    // green when that producer has a wake of its own.
    FrameSite {
        name: SiteName("a file link under a resting pointer previews with no tick"),
        setup: r"printf '\033[2J\033[Hfile:///tmp\033]52;c;!\007'",
        act: "",
        keys: RigKeys::Line,
        ticks: RigFrameTicks::Offered,
        window: RigWindow::PointerOver,
        env: &[],
        config: STEADY,
        focus: RigFocus::Believed,
        attempts: Iterations(100),
        expect: Settles::PreviewedUnticked,
        state: RowState::Green,
    },
    // **Output's printed paths are asked about with no tick** (FS-A14): on
    // the CPU path, whose present has no completion to wake the window
    // again. The act signs its clear and waits in the shell's own `read`
    // on the FIFO `go` the setup made; once the clear is signed, its frame
    // drawn and the batch its echo owes answered, the rig writes the
    // newline it waits on (`RigWindow::Released`, through the probe's
    // doors), and the act prints the path `/usr/bin` in a read of its own,
    // with no effect in it (spelled in octal, so the echo of the typed line
    // names no path), and waits on `read` again. No key, no prompt, later
    // output, effect or process event gives the window another turn after
    // the reader's wake whose turn drains and draws the path, and no wait
    // decides which: a key's own turn drains what arrives within it, and
    // the reader's wake for that read then runs a turn of its own. That
    // turn's frame files the path, and the same turn, after its present,
    // schedules the worker's wake for the batch's deadline, a paused
    // window's included; the ticks withheld and no pointer moved. The run
    // is two seconds, past the batch's deadline: 150 ms after the path's
    // read, the echo's batch having asked about no path to hold it a
    // second after.
    FrameSite {
        name: SiteName("output's printed paths are asked about with no tick"),
        setup: r"mkfifo go; printf 'baud\033]52;c;!\007'",
        act: r"printf '\033]52;c;!\007'; read x <go; printf '\033[2J\033[H\057usr\057bin'; read y",
        keys: RigKeys::Line,
        ticks: RigFrameTicks::Withheld,
        window: RigWindow::Released,
        env: &[("BAUD_GPU", "cpu")],
        config: STEADY,
        focus: RigFocus::Believed,
        attempts: Iterations(200),
        expect: Settles::Filed,
        state: RowState::Green,
    },
    // **A frame the GPU never answers is counted at its bound, with no
    // tick** (`BAUD_RIG_GPU=hold`): the act's frame's completion held by
    // the fault seam, so the frame stays in flight and the window holds
    // every draw. The ticks withheld, nothing but the wake posted at the
    // frame's commit runs at its bound, and it counts the frame overdue;
    // released then, the frame is counted late and the window draws a
    // whole frame after it, on the completion's wake. Red with no wake at
    // the bound: the held frame is never counted, and the window stays
    // held with nothing said. Break: the bound checked on the tick instead
    // of the wake.
    FrameSite {
        name: SiteName("a frame held past its bound is counted overdue with no tick"),
        setup: STILL,
        act: "printf y",
        keys: RigKeys::Line,
        ticks: RigFrameTicks::Withheld,
        window: RigWindow::Untold,
        env: &[("BAUD_RIG_GPU", "hold")],
        config: STEADY,
        focus: RigFocus::Real,
        attempts: Iterations(10),
        expect: Settles::GpuHeld,
        state: RowState::Green,
    },
    // **A failed frame is not trusted, and the window goes on**
    // (`BAUD_RIG_GPU=fail`): the frame a light/dark flip owes, reported
    // failed by the fault seam, is counted, and the window draws a whole
    // frame after it, which nothing but the failure owes. Break: the
    // failure owing no frame, and none is drawn after it.
    FrameSite {
        name: SiteName("a failed frame is counted and the window draws whole after it"),
        setup: STILL,
        act: "",
        keys: RigKeys::Line,
        ticks: RigFrameTicks::Offered,
        window: RigWindow::Restyled,
        env: &[("BAUD_RIG_GPU", "fail")],
        config: STEADY,
        focus: RigFocus::Real,
        attempts: Iterations(30),
        expect: Settles::GpuFailed,
        state: RowState::Green,
    },
    // **The launch's first frame past its bound opens drawn on the CPU**
    // (`BAUD_RIG_GPU=hold-launch`): drawn as the window's opening draws
    // it, its completion held past the launch's bound, so the wait returns
    // and the window opens on the same frame drawn on the CPU, never on a
    // surface nothing drew; released, it is counted late. Red with the
    // wait unbounded: the launch never returns. Break: no CPU frame, and
    // the window opens undrawn.
    FrameSite {
        name: SiteName("a launch frame past its bound opens drawn on the CPU"),
        setup: STILL,
        act: "",
        keys: RigKeys::Line,
        ticks: RigFrameTicks::Offered,
        window: RigWindow::Untold,
        env: &[("BAUD_RIG_GPU", "hold-launch")],
        config: STEADY,
        focus: RigFocus::Real,
        attempts: Iterations(30),
        expect: Settles::OpenedOnCpu,
        state: RowState::Green,
    },
    // **A frame the GPU never answers gives the window to the CPU, which
    // draws every frame after** (`BAUD_RIG_GPU=hold-forever`): the act's
    // frame's completion held and never released, the ticks withheld, so
    // only events move the window. A second act is fed while it is held;
    // the frame's bound wake counts it overdue and its give-up wake gives
    // the GPU up, and the window draws on the CPU from then on: the first
    // frame there holds the second act's output, three acts after it are
    // each drawn, and a zoom and a wider view each come into force, drawn.
    // Red with the give-up only counted, as it was: every draw stays owed
    // to the frame in flight, and no frame is drawn after the give-up.
    // Breaks: the journal carried across the switch, and the zoom waits
    // for a landing that never comes; the wanted path left as it was, and
    // the wider view makes a second `Gpu`.
    FrameSite {
        name: SiteName("a frame the GPU never answers gives the window to the CPU"),
        setup: STILL,
        act: "printf y",
        keys: RigKeys::Line,
        ticks: RigFrameTicks::Withheld,
        window: RigWindow::Untold,
        env: &[("BAUD_RIG_GPU", "hold-forever")],
        config: STEADY,
        focus: RigFocus::Real,
        attempts: Iterations(10),
        expect: Settles::GaveUpStuck,
        state: RowState::Green,
    },
    // **A launch whose first frame the GPU never answers opens on the CPU
    // and goes on there** (`BAUD_RIG_GPU=hold-launch-forever`): the launch's
    // frame held from before the window's `Gpu` is made and never
    // released, so the window opens on its CPU frame, and the give-up wake
    // its commit posted gives the GPU up; three acts after are each drawn.
    // Red with the give-up only counted: the window draws nothing after its
    // opening. Break: the switch not reached from the give-up's wake.
    FrameSite {
        name: SiteName("a launch the GPU never answers opens and goes on on the CPU"),
        setup: STILL,
        act: "",
        keys: RigKeys::Line,
        ticks: RigFrameTicks::Offered,
        window: RigWindow::Untold,
        env: &[("BAUD_RIG_GPU", "hold-launch-forever")],
        config: STEADY,
        focus: RigFocus::Real,
        attempts: Iterations(30),
        expect: Settles::LaunchGaveUp,
        state: RowState::Green,
    },
    // **Every frame failing gives the GPU up at the third**
    // (`BAUD_RIG_GPU=fail-forever`): from the light/dark flip's frame on,
    // every frame is reported failed, each owing the next; the third in a
    // row gives the GPU up, no frame is committed to it after, and the
    // window draws on the CPU. Red with no threshold: the failures never
    // stop. Break: the `Gpu`'s own refusal and the switch at the head of
    // `present` both removed, and frames go on failing past three.
    FrameSite {
        name: SiteName("every frame failing gives the GPU up at the third"),
        setup: STILL,
        act: "",
        keys: RigKeys::Line,
        ticks: RigFrameTicks::Offered,
        window: RigWindow::Restyled,
        env: &[("BAUD_RIG_GPU", "fail-forever")],
        config: STEADY,
        focus: RigFocus::Real,
        attempts: Iterations(30),
        expect: Settles::GaveUpFailing,
        state: RowState::Green,
    },
    // **Two failed frames in a row do not give the GPU up**
    // (`BAUD_RIG_GPU=fail-twice`): the control at the threshold, the frame
    // after the second drawn whole on the GPU. Break: a threshold of two.
    FrameSite {
        name: SiteName("two failed frames in a row do not give the GPU up"),
        setup: STILL,
        act: "",
        keys: RigKeys::Line,
        ticks: RigFrameTicks::Offered,
        window: RigWindow::Restyled,
        env: &[("BAUD_RIG_GPU", "fail-twice")],
        config: STEADY,
        focus: RigFocus::Real,
        attempts: Iterations(30),
        expect: Settles::FailedTwice,
        state: RowState::Green,
    },
    // **A device that revoked the process's access gives the GPU up at
    // once** (`BAUD_RIG_GPU=revoked`): one frame reported `AccessRevoked`,
    // which the header calls the client's for too many timeouts or hangs,
    // and the window draws on the CPU from the next. Break: the code not
    // read, and the window goes back to GPU frames that land.
    FrameSite {
        name: SiteName("a revoked device gives the GPU up at once"),
        setup: STILL,
        act: "",
        keys: RigKeys::Line,
        ticks: RigFrameTicks::Offered,
        window: RigWindow::Restyled,
        env: &[("BAUD_RIG_GPU", "revoked")],
        config: STEADY,
        focus: RigFocus::Real,
        attempts: Iterations(30),
        expect: Settles::GaveUpRevoked,
        state: RowState::Green,
    },
    // **One link, paused and resumed, never made again per restart**: red
    // while the door's backend starts a new link each run, until
    // macos-liquid-glass's pause in place is published and baud
    // moves to it. Lifted arm only. Its stage is that bump's
    // (`Stage::PauseInPlace`), so stage 3 closes without it.
    FrameSite {
        name: SiteName("one display link, paused, not made again per restart"),
        setup: STILL,
        act: "",
        keys: RigKeys::Line,
        ticks: RigFrameTicks::Offered,
        window: RigWindow::Untold,
        env: &[],
        config: concat!(
            "[cursor]\nblink = \"true\"\ntrail = \"off\"\n",
            "[launch]\nsplash = \"off\"\n"
        ),
        focus: RigFocus::Believed,
        attempts: Iterations(150),
        expect: Settles::OneLink,
        state: RowState::Pending {
            stage: Stage::PauseInPlace,
            recorded: Failure::Remade,
        },
    },
];

/// **The polls a row's settle names**: [`HOLDERS`], whatever the row
/// turns on or off.
fn holders_of(_site: &FrameSite) -> Vec<&'static str> {
    HOLDERS.to_vec()
}

/// One frame row's child, the holders kept or lifted.
fn run_frames(site: &FrameSite, slug: &str, holders: RigHolders) -> Result<Counters, String> {
    run_frames_in(site, &scratch(slug), holders)
}

/// One frame row's child, in its scratch directory `dir`.
fn run_frames_in(site: &FrameSite, dir: &Path, holders: RigHolders) -> Result<Counters, String> {
    let conf = dir.join("config.toml");
    std::fs::write(&conf, site.config).expect("the config");
    let call = RigCall::Frames(FrameRig {
        setup: site.setup.to_owned(),
        act: site.act.to_owned(),
        attempts: site.attempts,
        focus: site.focus,
        holders,
        window: site.window,
        keys: site.keys,
        ticks: site.ticks,
    });
    let out = Command::new(env!("CARGO_BIN_EXE_baud"))
        .args(call.to_args())
        .env_clear()
        .env("HOME", dir.join("home"))
        .env("ZDOTDIR", dir.join("home"))
        .env("SHELL", "/bin/sh")
        .env("HISTFILE", "/dev/null")
        .env("PATH", "/usr/bin:/bin")
        .env("BAUD_CONFIG", &conf)
        .env("BAUD_TYPING", "off")
        .envs(site.env.iter().copied())
        .current_dir(dir)
        .output()
        .expect("the child");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let _ = std::fs::write(dir.join("stdout"), &out.stdout);
    let _ = std::fs::write(dir.join("stderr"), &out.stderr);
    match (out.status.success(), Counters::parse(&text)) {
        (true, Some(c)) => Ok(c),
        _ => Err(format!("{:?}: {}", out.status, text.trim())),
    }
}

/// **The frame rows a run runs, and the ones it skips**: every row, or
/// with `BAUD_RIG_ROWS` set only those whose names hold one of its parts,
/// `|` between them -- a row's own runs, many at a time, under load. The
/// run says how many of the rows it ran and names each it skipped
/// ([`Picked`]'s `Display`), so a filtered run never reads as the whole
/// rig, and a filter that names no row is refused rather than passing with
/// none run. The gate never runs filtered: `test:unit` refuses with it set.
#[derive(Debug, PartialEq, Eq)]
struct Picked {
    /// The rows to run, as indices into the rows given.
    run: Vec<usize>,
    /// The names of the rows skipped.
    skipped: Vec<&'static str>,
    /// How many rows there are.
    of: usize,
}

impl Picked {
    /// The rows of `names` that `only` picks: all of them when it is
    /// unset; otherwise those holding one of its parts, refused when one of
    /// its parts is empty, which every name holds, or when it picks none.
    fn of(names: &[&'static str], only: Option<&str>) -> Result<Picked, String> {
        let Some(only) = only else {
            return Ok(Picked {
                run: (0..names.len()).collect(),
                skipped: Vec::new(),
                of: names.len(),
            });
        };
        let parts: Vec<&str> = only.split('|').collect();
        if parts.iter().any(|p| p.is_empty()) {
            return Err(format!(
                "BAUD_RIG_ROWS={only:?} has an empty part, which names every row"
            ));
        }
        let (run, skipped): (Vec<usize>, Vec<usize>) =
            (0..names.len()).partition(|&i| parts.iter().any(|p| names[i].contains(p)));
        let picked = Picked {
            run,
            skipped: skipped.into_iter().map(|i| names[i]).collect(),
            of: names.len(),
        };
        if picked.run.is_empty() {
            return Err(format!("BAUD_RIG_ROWS={only:?} names no row: {picked}"));
        }
        Ok(picked)
    }
}

impl core::fmt::Display for Picked {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "rows run {} of {}, skipped: ", self.run.len(), self.of)?;
        match self.skipped.is_empty() {
            true => f.write_str("none"),
            false => f.write_str(&self.skipped.join("; ")),
        }
    }
}

/// **A row filter names what it skipped, and one that names no row is
/// red**: unset, every row runs and none is skipped; a part picks the rows
/// whose names hold it and names the rest; a filter that picks none, or
/// has an empty part, is refused. Red with the filter as it was, a skip
/// by `continue` that passed with no row run.
#[test]
fn a_row_filter_names_what_it_skipped_and_one_naming_no_row_is_red() {
    let names = ["a held frame", "a failed frame", "a still screen"];
    let whole = Picked::of(&names, None).expect("unset runs every row");
    assert_eq!(whole.run, [0, 1, 2]);
    assert_eq!(whole.to_string(), "rows run 3 of 3, skipped: none");
    let some = Picked::of(&names, Some("frame|nothing")).expect("two rows");
    assert_eq!(some.run, [0, 1]);
    assert_eq!(some.to_string(), "rows run 2 of 3, skipped: a still screen");
    assert!(Picked::of(&names, Some("no such row")).is_err());
    assert!(Picked::of(&names, Some("")).is_err());
    assert!(Picked::of(&names, Some("frame|")).is_err());
    // The rig's own rows: each one's name picks it.
    let rows: Vec<&'static str> = FRAME_SITES.iter().map(|s| s.name.0).collect();
    for (i, name) in rows.iter().enumerate() {
        let one = Picked::of(&rows, Some(name)).expect("its own name");
        assert!(one.run.contains(&i), "{name}: {one}");
    }
}

/// **Each frame row, its kept arm and its lifted arm**, read like a
/// ceiling arm: both arms' counters printed and compared, and a lifted arm
/// whose counters equal the kept arm's is red, the lift having done
/// nothing. The lifted arm's polls must be the row's [`holders_of`], the kept
/// arm's recorded list, so the lift hides no poll off it. The lifted arm must
/// meet the row; the kept arm is judged by [`verdict`] at [`CURRENT`], a
/// pending row failing as [`Failure::Held`] while any poll holds the link.
#[test]
fn every_frame_row_comes_to_its_state() {
    let mut red = Vec::new();
    // **The rows this run runs** ([`Picked`]): said first, so a filtered
    // run's log never reads as the whole rig.
    let names: Vec<&'static str> = FRAME_SITES.iter().map(|s| s.name.0).collect();
    let picked = Picked::of(&names, std::env::var("BAUD_RIG_ROWS").ok().as_deref())
        .unwrap_or_else(|e| panic!("{e}"));
    println!("{picked}");
    say_load("at the start");
    // **A row that injects nothing reads the GPU clean** on both arms: no
    // frame failed, none past its bound, the GPU not given up.
    let injects = |site: &FrameSite| site.env.iter().any(|(k, _)| *k == "BAUD_RIG_GPU");
    for &i in &picked.run {
        let site = &FRAME_SITES[i];
        let lifted = run_frames(site, &format!("frame{i}-lifted"), RigHolders::Lifted);
        println!("{:?}, lifted: {lifted:?}", site.name);
        let lifted = match lifted {
            Ok(c) => c,
            Err(e) => {
                red.push((site.name, format!("lifted arm: {e}")));
                continue;
            }
        };
        println!(
            "{:?}: gpu slowest {} us, lifted",
            site.name, lifted.gpu_slowest_us
        );
        if !injects(site) && !lifted.gpu_clean() {
            red.push((site.name, format!("lifted arm's GPU: {lifted:?}")));
        }
        if lifted.holders != holders_of(site) {
            red.push((site.name, format!("the lift hid {:?}", lifted.holders)));
        }
        if matches!(site.expect, Settles::OneLink | Settles::PreviewedUnticked) {
            let observed = match (site.expect.holds(&lifted), site.expect) {
                (true, _) => Observed::Passed,
                (false, Settles::OneLink) => Observed::Failed(Failure::Remade),
                (false, _) => Observed::Failed(Failure::Held),
            };
            let v = verdict(site.state, observed, CURRENT);
            println!("{:?}: {observed:?} -> {v:?}", site.name);
            if v != Verdict::Pass {
                red.push((site.name, format!("{v:?}")));
            }
            continue;
        }
        if !site.expect.holds(&lifted) {
            red.push((site.name, format!("lifted arm missed the row: {lifted:?}")));
        }
        let kept = run_frames(site, &format!("frame{i}-kept"), RigHolders::Kept);
        println!("{:?}, kept: {kept:?}", site.name);
        let kept = match kept {
            Ok(c) => c,
            Err(e) => {
                red.push((site.name, format!("kept arm: {e}")));
                continue;
            }
        };
        println!(
            "{:?}: gpu slowest {} us, kept",
            site.name, kept.gpu_slowest_us
        );
        if !injects(site) && !kept.gpu_clean() {
            red.push((site.name, format!("kept arm's GPU: {kept:?}")));
        }
        if kept.holders != lifted.holders {
            red.push((site.name, format!("kept {:?}", kept.holders)));
        }
        let seen = |c: &Counters| (c.link, c.delivered, c.sends, c.drawn);
        if seen(&kept) == seen(&lifted) {
            red.push((site.name, "the lift did nothing".to_owned()));
        }
        let observed = match (site.expect.holds(&kept), kept.holders.is_empty()) {
            (true, _) => Observed::Passed,
            (false, false) => Observed::Failed(Failure::Held),
            (false, true) => Observed::Failed(Failure::Child),
        };
        let v = verdict(site.state, observed, CURRENT);
        println!("{:?}: {observed:?} -> {v:?}", site.name);
        if v != Verdict::Pass {
            red.push((site.name, format!("{v:?}")));
        }
    }
    say_load("at the end");
    assert!(red.is_empty(), "{red:?}");
}

/// **The names a listing row's folder holds**: fifty, each a single name
/// with a `.` not at its end, the form `ls` prints and a scan reads against
/// the reported folder.
const LISTED: u16 = 50;

/// **A listing's paths, the row the link lag was found on**: the setup
/// moves into a folder of [`LISTED`] files made before the child starts,
/// sets a prompt with no path in it, and clears the screen before it
/// reports the folder, so no row the setup leaves holds a path and no batch
/// it sends asks about one -- a floor after one would hold the act's batch
/// by how soon the act came, which the row then reads. Its echo spells `/`
/// and `f` in octal for the same reason. The act lists the folder in a
/// subshell, a job of its own, that signs the end of what it wrote with a
/// clear and stays in front, reading, after a pause of a second that
/// decides no verdict. In the runs tried, the listing then took three
/// batches -- the echo's, one naming its writer and one asking about its
/// paths -- and without the pause, a bare `ls` or the subshell alone, one
/// batch with no naming batch before it, which no floor can hold. The
/// pause is a whole second because `0.2` in the echo is a name with a `.`,
/// asked about, whose batch's floor then held the listing's. The listing's
/// rows are read once the batch that names their writer is answered, and
/// the batch that asks about their paths then goes at its own deadline:
/// the row ends on the frame that draws the fifty marks
/// (`RigFrameTicks::UntilMarked`), and is judged on the counts alone
/// (`Settles::Marked`). The times of the text's frame and the marks' are
/// printed, a measurement it is not judged on.
const LISTING: FrameSite = FrameSite {
    name: SiteName("a listing's paths are not held by the batch that named its writer"),
    setup: concat!(
        r"cd list; PS1='$ '; printf '\033[2J\033[H\033]7;\146ile:\057\057%s%s\007",
        r#"baud\033]52;c;!\007' "$(hostname)" "$PWD""#,
    ),
    act: r"(sleep 1; ls; printf '\033]52;c;!\007'; read x)",
    keys: RigKeys::Line,
    ticks: RigFrameTicks::UntilMarked,
    window: RigWindow::Untold,
    env: &[],
    config: STEADY,
    focus: RigFocus::Believed,
    attempts: Iterations(LISTED),
    expect: Settles::Marked(LISTED),
    state: RowState::Green,
};

/// **A listing's printed paths are drawn at their own deadline, not held a
/// second by the batch that named their writer**, on the lifted arm alone:
/// the row is about the printed paths' pace, not the polls a settle names.
/// Red with the floor armed by any batch (`Pace::sent` setting the last
/// batch whatever it asked, or the floor set before the tick knows what
/// goes): the batch that asks about the listing's paths is then held to a
/// second after the batch that named `ls`, and counted held. The setup must
/// have asked about no path, or a floor after it could hold the act's
/// batch by timing, and that is the harness's failure, not the row's.
#[test]
fn a_listings_paths_are_not_held_by_the_batch_that_named_its_writer() {
    let dir = scratch("listing");
    let list = dir.join("list");
    std::fs::create_dir(&list).expect("the listed folder");
    for n in 10..10 + LISTED {
        std::fs::write(list.join(format!("f{n}.txt")), b"").expect("a listed file");
    }
    let c = run_frames_in(&LISTING, &dir, RigHolders::Lifted)
        .unwrap_or_else(|e| panic!("{:?}: {e}", LISTING.name));
    println!("{:?}: {c:?}", LISTING.name);
    assert_eq!(
        c.setup_asked(),
        Some(0),
        "the setup asked about a path, and a floor after it could hold the act's batch: {c:?}"
    );
    assert!(LISTING.expect.holds(&c), "{:?}: {c:?}", LISTING.name);
}

/// **The act a shell's `D` row sends**, its second stage one `printf`, so
/// one `write(2)` ([`shells_d_command!`]) naming `/usr/bin`, then a line
/// feed and a clear that signs the second stage. With `before`, the
/// second stage starts with another such command naming that path, then
/// a tenth of a second's pause: within a second of the batch that asked
/// about it, so under the floor that batch laid; each of the two is
/// `printf` run by `env`, not the shell's own, so its rows are a program's the
/// shell runs, named by a batch before they are read, as a listing's are. `/bin/sh` is handed the
/// token by the probe build's door (`BAUD_RIG_SHELL_TOKEN`), since the
/// rig's shell is none baud writes a script for.
macro_rules! shells_d_act {
    ($cmd:literal) => {
        concat!(
            r"printf '\033]52;c;!\007'; read x <go; ",
            shells_d_command!($cmd, r"\057usr\057bin", r"\n\033]52;c;!\007"),
            "; read y",
        )
    };
    (before: $path:literal, $cmd:literal) => {
        concat!(
            r"printf '\033]52;c;!\007'; read x <go; env ",
            shells_d_command!($cmd, $path, r"\n"),
            "; sleep 0.1; env ",
            shells_d_command!($cmd, r"\057usr\057bin", r"\n\033]52;c;!\007"),
            "; read y",
        )
    };
}

/// **One command a shell's `D` row's act runs, in one `printf`**: the
/// window's shell's `C` with its token naming `cmd`, `path` (spelled in
/// octal, so the echo of the typed line names no path), a move back up
/// onto its row, which refuses the read's cut at the `D`, so the `D`'s own
/// part names that row; the shell's `D` with its token, its `A`, and
/// `tail`.
macro_rules! shells_d_command {
    ($cmd:literal, $path:literal, $tail:literal) => {
        concat!(
            r"printf '\033]133;C;baud_token=%s;cmdline=",
            $cmd,
            r"\007",
            $path,
            r"\n\033[A\033]133;D;0;aid=1;baud_token=%s\007\033]133;A;aid=1\007",
            $tail,
            r#"' "$BAUD_SHELL_TOKEN" "$BAUD_SHELL_TOKEN""#,
        )
    };
}

/// **A command's printed path is asked about on the turns its shell's `D`
/// starts**, not 150 ms after its read (baud-app's `probe::SETTLE`):
/// FS-A14's shape, the act's first stage signed and its echo's batch
/// answered before its second is released (`RigWindow::Released`), which
/// is the shell's `C` naming `printf`, `/usr/bin`, and the shell's `D`, in
/// one read. The row ends on the frame that draws the mark
/// (`RigFrameTicks::UntilMarked`, one link line) and is judged on the
/// counts: every batch the second stage sent to that frame was due by the
/// `D` (`By::Ended`), none by the quiet, the ceiling or the floor. No
/// clock decides it.
const SHELLS_D: FrameSite = FrameSite {
    name: SiteName("a command's printed path is asked about on its shell's D"),
    setup: r"mkfifo go; printf 'baud\033]52;c;!\007'",
    act: shells_d_act!("printf"),
    keys: RigKeys::Line,
    ticks: RigFrameTicks::UntilMarked,
    window: RigWindow::Released,
    env: &[("BAUD_GPU", "cpu"), ("BAUD_RIG_SHELL_TOKEN", "1")],
    config: STEADY,
    focus: RigFocus::Believed,
    attempts: Iterations(1),
    expect: Settles::MarkedOnD,
    state: RowState::Green,
};

/// **[`SHELLS_D`]'s remote control**: the same act with the shell's `C`
/// naming `ssh`, run for two seconds with no tick: the `D` made a batch
/// due, and the row it filed remote at its read stays dark, its path never
/// asked about.
const SHELLS_D_REMOTE: FrameSite = FrameSite {
    name: SiteName("a remote command's printed path stays dark under its shell's D"),
    setup: r"mkfifo go; printf 'baud\033]52;c;!\007'",
    act: shells_d_act!("ssh"),
    keys: RigKeys::Line,
    ticks: RigFrameTicks::Withheld,
    window: RigWindow::Released,
    env: &[("BAUD_GPU", "cpu"), ("BAUD_RIG_SHELL_TOKEN", "1")],
    config: STEADY,
    focus: RigFocus::Believed,
    attempts: Iterations(200),
    expect: Settles::DarkOnD,
    state: RowState::Green,
};

/// **A command's printed path is asked about on the turns its shell's `D`
/// starts** ([`SHELLS_D`]), on the lifted arm alone: the row is about the
/// printed paths' pace. Red with no proof minted from the `D`
/// (`vouch::Latch::step`): the path is asked about by the quiet. Red with
/// the probe's door handing the shell another token than the window's
/// marks are read against: no `D` is the shell's, and the same. The setup
/// must have asked about no path, or a floor after it could hold the act's
/// batch, and that is the harness's failure, not the row's. The time from
/// the text's frame to the marks' is printed, not judged.
#[test]
fn a_commands_printed_path_is_asked_about_on_its_shells_d() {
    let c = run_frames(&SHELLS_D, "shells-d", RigHolders::Lifted)
        .unwrap_or_else(|e| panic!("{:?}: {e}", SHELLS_D.name));
    println!("{:?}: {c:?}", SHELLS_D.name);
    assert_eq!(
        c.setup_asked(),
        Some(0),
        "the setup asked about a path, and a floor after it could hold the act's batch: {c:?}"
    );
    assert!(SHELLS_D.expect.holds(&c), "{:?}: {c:?}", SHELLS_D.name);
}

/// **A remote command's printed path stays dark under its shell's `D`**
/// ([`SHELLS_D_REMOTE`]), on the lifted arm alone. Red with the drain where
/// the shell ended its command filed as the shell's (`drain_recut` raising
/// every part of it): the path's row is then asked about and marked.
#[test]
fn a_remote_commands_printed_path_stays_dark_under_its_shells_d() {
    let c = run_frames(&SHELLS_D_REMOTE, "shells-d-remote", RigHolders::Lifted)
        .unwrap_or_else(|e| panic!("{:?}: {e}", SHELLS_D_REMOTE.name));
    println!("{:?}: {c:?}", SHELLS_D_REMOTE.name);
    assert!(
        SHELLS_D_REMOTE.expect.holds(&c),
        "{:?}: {c:?}",
        SHELLS_D_REMOTE.name
    );
}

/// **A command printing a path, then within a second another printing
/// another, each ended by its shell's `D`**: [`SHELLS_D`]'s act with a
/// command printing `/usr/lib` before it, a tenth of a second earlier, in
/// the same second stage: the batches its `D` sends ask about `/usr/lib`
/// and lay the floor, and the second command's `/usr/bin`, a path not yet
/// asked about, and its `D` come under it -- the paused typed listing's
/// shape, the path asked about within a second before the listing. The
/// row ends on the frame that draws both marks
/// (`RigFrameTicks::UntilMarked`, two link lines).
const SHELLS_D_FLOORED: FrameSite = FrameSite {
    name: SiteName("a path printed within a second of an asking batch is asked about on its D"),
    setup: r"mkfifo go; printf 'baud\033]52;c;!\007'",
    act: shells_d_act!(before: r"\057usr\057lib", "printf"),
    keys: RigKeys::Line,
    ticks: RigFrameTicks::UntilMarked,
    window: RigWindow::Released,
    env: &[("BAUD_GPU", "cpu"), ("BAUD_RIG_SHELL_TOKEN", "1")],
    config: STEADY,
    focus: RigFocus::Believed,
    attempts: Iterations(2),
    expect: Settles::PastFloor(2),
    state: RowState::Green,
};

/// **The same path printed again on its shell's `D`, within a second of
/// the batch that asked about it**: [`SHELLS_D_FLOORED`]'s shape with the
/// first command printing `/usr/bin` too, so the second command's has its
/// answer kept from the first's batch, which laid the floor: the second
/// `D`'s one batch only names its writer, and that makes the kept answer
/// drawable -- a listing run again on the first one's marks. The row ends
/// on the frame that draws both marks.
const SHELLS_D_WARM: FrameSite = FrameSite {
    name: SiteName("a path printed again within a second is drawn on its D"),
    setup: r"mkfifo go; printf 'baud\033]52;c;!\007'",
    act: shells_d_act!(before: r"\057usr\057bin", "printf"),
    keys: RigKeys::Line,
    ticks: RigFrameTicks::UntilMarked,
    window: RigWindow::Released,
    env: &[("BAUD_GPU", "cpu"), ("BAUD_RIG_SHELL_TOKEN", "1")],
    config: STEADY,
    focus: RigFocus::Believed,
    attempts: Iterations(2),
    expect: Settles::PastFloor(1),
    state: RowState::Green,
};

/// **A command's printed path within a second of a batch that asked is
/// drawn on the turns its shell's `D` starts** ([`SHELLS_D_FLOORED`]), on
/// the lifted arm alone: the second `D` passes the floor the first
/// command's asking batch laid, for the batch naming the command's writer
/// and the one asking about its path, and none is held under the grant.
/// Red with no lift (the floor laid over a grant's deadline as over any
/// other) and on the base's pace: the floor holds the second command's
/// batches to a second after the first's, counted held under the grant,
/// or past the row's backstop; and with the floor passed only by a batch
/// that asks: the batch naming the second command is never sent, and its
/// mark is not drawn. The setup must have asked about no
/// path, or its floor would be in the row. The time from the text's frame
/// to the marks' is printed, not judged.
#[test]
fn a_path_printed_within_a_second_of_an_asking_batch_is_drawn_on_its_d() {
    let c = run_frames(&SHELLS_D_FLOORED, "shells-d-floored", RigHolders::Lifted)
        .unwrap_or_else(|e| panic!("{:?}: {e}", SHELLS_D_FLOORED.name));
    println!("{:?}: {c:?}", SHELLS_D_FLOORED.name);
    assert_eq!(c.setup_asked(), Some(0), "the setup asked: {c:?}");
    assert!(
        SHELLS_D_FLOORED.expect.holds(&c),
        "{:?}: {c:?}",
        SHELLS_D_FLOORED.name
    );
}

/// **The same path printed again within a second is drawn on the turns
/// its shell's `D` starts** ([`SHELLS_D_WARM`]), on the lifted arm alone:
/// the batch naming the writer passes the floor and asks about nothing,
/// and none is held under the grant. Red with no lift and on the base's
/// pace: the naming batch goes by the floor, a second after the first
/// command's, counted held under the grant; and with the floor passed only
/// by a batch that asks: it is never sent, and the mark is not drawn. The time from the text's frame to the
/// marks' is printed, not judged.
#[test]
fn the_same_path_printed_again_within_a_second_is_drawn_on_its_d() {
    let c = run_frames(&SHELLS_D_WARM, "shells-d-warm", RigHolders::Lifted)
        .unwrap_or_else(|e| panic!("{:?}: {e}", SHELLS_D_WARM.name));
    println!("{:?}: {c:?}", SHELLS_D_WARM.name);
    assert_eq!(c.setup_asked(), Some(0), "the setup asked: {c:?}");
    assert!(
        SHELLS_D_WARM.expect.holds(&c),
        "{:?}: {c:?}",
        SHELLS_D_WARM.name
    );
}
