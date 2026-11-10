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
use std::process::Command;

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
    /// A gesture sent to the view's own methods, over what `setup`, typed
    /// once first, drew; the frame never driven.
    Gesture(Gesture),
}

impl Cause {
    /// The row's shape: a printed path's is read, [`ITERATIONS`] of it.
    fn shape(self) -> Shape {
        match self {
            Cause::Program(Program { shape, .. })
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
    /// callback that asked.
    AfterInline,
}

/// A row read from the program, [`ITERATIONS`] of it.
const READ: Shape = Shape {
    polled: Polled::Read,
    iterations: ITERATIONS,
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
    /// `turn::after` alone inline, in the callback that asked.
    DrivenAfterInline,
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

/// **A kitty transmit whose reply the shell waits for**: echo and line
/// editing off, a read that returns on the first bytes or after a second
/// (under the iteration's bound), and a copy on an `OK`. The picture is
/// `baud-font/tests/png/g8-1x1-n.png`, a 1x1 grey PNG, whose decode is
/// deferred like any PNG's.
const KITTY_OK: &str = concat!(
    r"stty -echo -icanon min 0 time 10; ",
    r"printf '\033_Ga=T,f=100,i=7;",
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAAAAAA6fptVAAAACklEQVR42mPoAwAAkACPLN3KfgAAAABJRU5ErkJggg==",
    r"\033\\'; r=$(dd bs=64 count=1 2>/dev/null); stty echo icanon; ",
    r"case $r in *OK*) printf '\033]52;c;aGk=\007';; esac",
);

const SITES: [Site; 27] = [
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
        cause: Cause::Program(Program {
            line: KITTY_OK,
            config: "",
            shape: READ,
        }),
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
    Site {
        name: SiteName("the bell's raise, mode 1043"),
        cause: Cause::Program(Program {
            line: r"printf '\033[?1043h\007'",
            config: "",
            shape: READ,
        }),
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
    // turn of its own the row passes, and the control is red.
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

/// The scratch directory a row's child lives in: its home, its config and
/// the file it clicks.
fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("event_rig")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("home")).expect("the scratch home");
    dir
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
/// its recorded site would be one entry or the other by timing. A row
/// whose effect the tick alone asks ([`Polled::Tick`]) runs driven alone.
fn arms(site: &Site) -> &'static [Ticks] {
    match (site.cause, site.state, site.cause.shape().polled) {
        (Cause::PrintedPath(_, Ticks::Driven), ..) => &[Ticks::Driven],
        (Cause::PrintedPath(_, Ticks::None), ..) => &[Ticks::None],
        (Cause::PrintedPath(_, Ticks::DrivenInline), ..) => &[Ticks::DrivenInline],
        (Cause::PrintedPath(_, Ticks::DrivenInFrame), ..) => &[Ticks::DrivenInFrame],
        (Cause::PrintedPath(_, Ticks::DrivenAfterInline), ..) | (_, _, Polled::AfterInline) => {
            &[Ticks::DrivenAfterInline]
        }
        (_, _, Polled::Tick { .. }) => &[Ticks::Driven],
        (Cause::Program(_) | Cause::ProgramThen(..), RowState::Green, Polled::Read) => {
            &[Ticks::None, Ticks::Driven]
        }
        (Cause::Program(_) | Cause::ProgramThen(..) | Cause::Gesture(_), ..) => &[Ticks::None],
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
            Ticks::DrivenAfterInline => RigTicks::AfterInCallback,
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
    let mut red = Vec::new();
    for (i, site) in SITES.iter().enumerate() {
        let observed = run(site, &format!("row{i}"));
        let v = verdict(site.state, observed, CURRENT);
        println!("{:?}: {observed:?} -> {v:?}", site.name);
        if v != Verdict::Pass {
            red.push((site.name, v));
        }
    }
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
            Cause::Program(p) => format!("{} {}", p.line, p.config),
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

/// **A probe build that is not called as a rig exits 2 at once**, before
/// any application, window or Dock work (`rig_door.rs`): with no
/// arguments, a plain launch's, and with one argument too few for the
/// event rig, which is how a by-hand runner once fell through to a plain
/// launch and showed a window. Each child is given 1 s and killed past it
/// -- a launch would still be running -- and only calls the door refuses
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
        let start = std::time::Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().expect("its status") {
                break Some(status);
            }
            if start.elapsed() > std::time::Duration::from_secs(1) {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        let mut said = String::new();
        let _ =
            std::io::Read::read_to_string(&mut child.stderr.take().expect("its stderr"), &mut said);
        let status = status.unwrap_or_else(|| panic!("{args:?}: still running after 1 s"));
        assert_eq!(status.code(), Some(2), "{args:?}: {said}");
        assert!(said.starts_with("baud: "), "{args:?}: {said}");
    }
}

/// **The polls still the tick's**, as a settle names them
/// (`frames::Holder`): while any is held a real build's display link runs,
/// and the frame rows' kept arms fail as recorded. Each row that moves a
/// poll to its event takes its name out; with none left the kept arms
/// must pass.
const HOLDERS: &[&str] = &["FS-A5", "FS-A9", "FS-A13", "FS-A14", "FS-A17"];

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
    reaped: u32,
    /// Each followed setting's changes taken: scroller, flashing,
    /// animated.
    followed: [u32; 3],
    /// The hands of this machine's names to the links.
    hands: u32,
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
            title: text
                .lines()
                .find_map(|l| l.strip_prefix("title "))
                .unwrap_or_default()
                .to_owned(),
        })
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
    /// `drawn` frames drawn, at most `delivered` ticks taken for them, and
    /// at least a send for each frame, the link run and paused between
    /// them; the state at the end is the clock's.
    Blinks { drawn: u64, delivered: u32 },
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
    /// the hover read at least once, no more often than a frame was drawn,
    /// and fewer times than a turn fed the core -- which a read per feed
    /// would equal, whatever the machine's pace.
    ReadByFrame,
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
    /// **This machine's names handed with no tick**: the ticks withheld,
    /// nothing delivered, and the read's names handed to the links.
    Handed,
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
            Settles::Blinks { drawn, delivered } => {
                c.drawn >= drawn && c.delivered <= delivered && u64::from(c.sends) >= drawn
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
            Settles::ReadByFrame => {
                c.fed > c.reads && c.reads >= 1 && u64::from(c.reads) <= c.drawn
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

const FRAME_SITES: [FrameSite; 22] = [
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
    // frame before it, and pauses between.
    FrameSite {
        name: SiteName("a blinking caret runs the link at each flip"),
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
        expect: Settles::Blinks {
            drawn: 4,
            delivered: 40,
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
    // **The splash's wait holds the link** (`FS-A17`): the one row with the
    // splash on, so its kept arm names it among the polls; the window is
    // never shown, and the splash waits for ever.
    FrameSite {
        name: SiteName("a splash waiting for its window holds the link"),
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
    // than the run on either arm. The guard is free of the machine's pace:
    // fewer reads than turns that fed the core, which a read per feed
    // equals, at least one, and no more than the frames drawn.
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
        state: RowState::Pending {
            stage: Stage::Three,
            recorded: Failure::Held,
        },
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

/// **The polls a row's settle names**: [`HOLDERS`], `FS-A17` only where the
/// splash is on -- every row but one turns it off.
fn holders_of(site: &FrameSite) -> Vec<&'static str> {
    let splash_off = site.config.contains("splash = \"off\"");
    HOLDERS
        .iter()
        .copied()
        .filter(|h| !(splash_off && *h == "FS-A17"))
        .collect()
}

/// One frame row's child, the holders kept or lifted.
fn run_frames(site: &FrameSite, slug: &str, holders: RigHolders) -> Result<Counters, String> {
    let dir = scratch(slug);
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
        .current_dir(&dir)
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
    for (i, site) in FRAME_SITES.iter().enumerate() {
        let lifted = run_frames(site, &format!("frame{i}-lifted"), RigHolders::Lifted);
        println!("{:?}, lifted: {lifted:?}", site.name);
        let lifted = match lifted {
            Ok(c) => c,
            Err(e) => {
                red.push((site.name, format!("lifted arm: {e}")));
                continue;
            }
        };
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
    assert!(red.is_empty(), "{red:?}");
}
