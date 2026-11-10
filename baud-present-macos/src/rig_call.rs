//! **A rig's call: the one protocol a probe build's two ends speak.**
//!
//! A probe build (the test-only `effect-probe` feature) is only ever a
//! rig: baud-app's `main` parses its arguments with [`RigCall::parse`] as
//! its first statement and exits 2 on anything else, before any
//! application, window or Dock work, so no probe binary becomes a real
//! app. The parent (`baud-app/tests/event_rig.rs`) builds the same type
//! and passes [`RigCall::to_args`]. A field added on one side is a field
//! the other must name, or neither compiles -- where two hand-kept
//! argument lists once drifted by one, and the child, given five where it
//! took six, fell through to a plain launch.
//!
//! Every rig mode is a variant here, and its flag is matched here and
//! nowhere else (baud-app's `rig_door` source check).

use crate::probe::EffectKind;

/// The event rig's flag.
const EVENT_RIG: &str = "--event-rig";

/// The frame rig's flag.
const FRAME_RIG: &str = "--frame-rig";

/// A rig's call: every mode a probe build runs as.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum RigCall {
    /// The event rig (`baud-app/src/app/rig.rs`).
    Event(EventRig),
    /// The frame rig: what the display link does once a screen settles
    /// (`baud-app/src/app/rig.rs`, `run_frames`).
    Frames(FrameRig),
}

/// **The frame rig's call**: a screen drawn by `setup`, typed once and
/// read to its sign; the display link left to settle; then `act` sent, if
/// any, as `keys` says, the window told what `window` says, and the rig's
/// ticks offered `attempts` times, each delivered only while the link
/// runs. The child prints what the window's display link did over the
/// offered ticks.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FrameRig {
    /// The line that draws the screen; it ends with an `OSC 52` clear,
    /// the sign it was read.
    pub setup: String,
    /// What is sent once the link has settled; empty for none.
    pub act: String,
    /// How many ticks are offered; under [`RigFrameTicks::UntilFlips`],
    /// how many times the caret flips before the offers stop.
    pub attempts: Iterations,
    /// Whether baud believes the window is key (`probe::Key`).
    pub focus: RigFocus,
    /// Whether the polls still the tick's hold the link (`probe::Lift`).
    pub holders: RigHolders,
    /// What the window is told once the act is sent.
    pub window: RigWindow,
    /// How the act is sent.
    pub keys: RigKeys,
    /// Whether the ticks after the act reach the view.
    pub ticks: RigFrameTicks,
}

/// **Whether the polls still the tick's hold the display link**: kept,
/// as every real build has them, or lifted, the probe's ceiling arm, so
/// the pause can be seen before every poll has its event. A row reads
/// both arms' counters, and a lifted arm whose counters equal the kept
/// arm's lifted nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RigHolders {
    Kept,
    Lifted,
}

const HOLDERS: [(RigHolders, &str); 2] = [
    (RigHolders::Kept, "holders-kept"),
    (RigHolders::Lifted, "holders-lifted"),
];

/// **What the frame rig's window is told**, after the act and before the
/// ticks, as AppKit tells it; the window is never shown, and never made
/// key: a key change is AppKit's notification posted to the window's
/// observers with the probe's focus door set to agree (`closing::rig_post`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RigWindow {
    /// Nothing.
    Untold,
    /// The appearance it does not have, dark for light or light for dark,
    /// as a light/dark flip gives it: AppKit tells the view, and the view
    /// its responder.
    Restyled,
    /// That it became the key window, the focus door set key.
    BecameKey,
    /// That it resigned the key window, the focus door set not key.
    ResignedKey,
    /// Its view made first responder again, resigned and taken back, as a
    /// field that takes the keys and hands them back does: the view's own
    /// first-responder hook, heard after the responder is set.
    Refocused,
    /// That the accessibility display options changed: the workspace's
    /// notification posted with the probe's access door turned from both
    /// off to both on (`settings::rig_post_display_options`).
    DisplayOptions,
    /// The pointer moved over the first row's second cell, no button held,
    /// sent to the view's own `mouseMoved:` (`probe::press`).
    PointerOver,
    /// A command's child, handed to the window at opening as a click's
    /// `[link.*]` command is (baud-app's `Driver::reap_command`) with its
    /// input held open, then that input closed, so the child ends: the probe
    /// build starts no command of a click's own.
    CommandExits,
    /// That the settings the window follows changed: "Show scroll bars",
    /// Dim Flashing Lights and Animated Images, each notification posted on
    /// the default centre, as the system posts it
    /// (`settings::rig_post_settings`).
    Settings,
    /// That the reverse lookup the frame rig stalls answered with a name of
    /// its own (`probe::HostName`), so the window's read of every name
    /// ends, adding one.
    HostNamed,
    /// That it is shown: the probe's door set to on screen
    /// (`probe::Shown`), then the occlusion change AppKit would post. The
    /// window is not ordered front.
    Shown,
    /// The act's second stage released once its first has settled -- its
    /// pasteboard clear signed, if it has one, its frame drawn and the
    /// printed paths it owed answered, read through the probe's doors
    /// within the bound -- by a newline written to the FIFO `go` in the
    /// row's folder, which the act reads: no key, nothing of the window's,
    /// so what the act does next comes on the pty's own read and after no
    /// wait.
    Released,
    /// That it is shown, as [`RigWindow::Shown`] tells it, before anything
    /// is typed and before its first frame: the frame rig then waits for
    /// that frame, the shell's prompt, and for the splash to start, with
    /// nothing sent, counts the start (`first=`) and plays a started
    /// splash to its end on offered ticks. After the act, nothing.
    ShownFirst,
}

const WINDOW: [(RigWindow, &str); 13] = [
    (RigWindow::Untold, "untold"),
    (RigWindow::Restyled, "restyled"),
    (RigWindow::BecameKey, "became-key"),
    (RigWindow::ResignedKey, "resigned-key"),
    (RigWindow::Refocused, "refocused"),
    (RigWindow::DisplayOptions, "display-options"),
    (RigWindow::PointerOver, "pointer-over"),
    (RigWindow::CommandExits, "command-exits"),
    (RigWindow::Settings, "settings"),
    (RigWindow::HostNamed, "host-named"),
    (RigWindow::Shown, "shown"),
    (RigWindow::Released, "released"),
    (RigWindow::ShownFirst, "shown-first"),
];

/// **How the frame rig's act is sent**: as a line to the shell, a newline
/// after it, or as key presses through the driver's own key path, one a
/// character and no newline, each opening what a key opens.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RigKeys {
    Line,
    Pressed,
}

const KEYS: [(RigKeys, &str); 2] = [(RigKeys::Line, "line"), (RigKeys::Pressed, "pressed")];

/// **Whether the frame rig's ticks reach the view after the act**: offered,
/// each delivered while the link runs, or withheld, the run loop run with
/// no tick at all -- what drives the state is then the act's own turns --
/// or offered until a count of frames is drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RigFrameTicks {
    Offered,
    Withheld,
    /// Offered until the caret has flipped `attempts` times since the act,
    /// on its own clock (`probe::flips`), for a row about a clock's frames:
    /// the run ends on the flips it is about -- not on a count of passes
    /// whose length the run loop decides, nor on the frames drawn, which a
    /// link that drew every tick would reach before any flip -- and
    /// [`SENTINEL_BACKSTOP`] is a backstop that, reached, says so.
    UntilFlips,
    /// **One tick after the act fed the core twice, the frame gate's clock
    /// frozen** (`probe::GateClock`): frozen once the act has fed, a frame
    /// drawn after it, so the last commit is no time ago and a fed turn
    /// draws no frame for its age; the ticks withheld until two turns have
    /// fed the core; then ticks offered until one draws, and no more,
    /// whatever `attempts` says.
    OneAfterFeeds,
    /// **[`RigFrameTicks::OneAfterFeeds`]'s positive control**: frozen the
    /// same way and then advanced an hour, so every commit is an hour old
    /// and a fed turn draws a frame of its own; the ticks withheld until
    /// two frames have drawn.
    EventFrames,
}

const FRAME_TICKS: [(RigFrameTicks, &str); 5] = [
    (RigFrameTicks::Offered, "offered"),
    (RigFrameTicks::Withheld, "withheld"),
    (RigFrameTicks::UntilFlips, "until-flips"),
    (RigFrameTicks::OneAfterFeeds, "one-after-feeds"),
    (RigFrameTicks::EventFrames, "event-frames"),
];

/// The event rig's call.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EventRig {
    /// What each iteration does.
    pub act: RigAct,
    /// What it does it with: the path clicked, the line typed, the setup
    /// line a gesture is over.
    pub arg: String,
    /// How many iterations.
    pub iterations: Iterations,
    /// Whether the frame is driven, and whether a turn runs inline.
    pub ticks: RigTicks,
    /// Whether baud believes the window is key (`probe::Key`).
    pub focus: RigFocus,
    /// The effects each iteration waits for.
    pub each: Vec<EffectKind>,
}

/// A number of iterations.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Iterations(pub u16);

/// What each iteration does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RigAct {
    /// Click `arg` as a printed path.
    Path,
    /// Type `arg` to the shell.
    Type,
    /// Cmd-click the first row's second cell, over `arg`'s screen.
    Link,
    /// Drag over the first row, over `arg`'s screen.
    Select,
    /// That drag once, then the Edit menu's Copy.
    CopyMenu,
    /// A click on the notification posted last, as the center's delegate
    /// hears one, over `arg`'s screen.
    NoticeClick,
    /// The window's close, as its will-close observer takes it, with no
    /// call to the window itself, over `arg`'s screen.
    Close,
    /// `arg`'s first line typed to the shell, and its second once the
    /// first's first effect has run: two reads, in order.
    TypeTwo,
    /// Type `arg` to the shell, each iteration ending on its effects or on
    /// the program's sentinel, a clear: never on wall time, with
    /// [`SENTINEL_BACKSTOP`] a backstop that, reached, says so.
    TypeUntil,
    /// [`RigAct::TypeUntil`], each reply the terminal makes on a decode's
    /// turn held [`HELD_REPLY`] and then written in two pieces: wall time
    /// passed and a write split on purpose, as a loaded machine and a tty
    /// make them.
    TypeHeld,
    /// Type `arg` to the shell, each iteration ending on the program's
    /// sentinel alone, a clear the program writes after its effect's bytes
    /// every time, its effects checked after it; [`SENTINEL_BACKSTOP`] the
    /// backstop, as for [`RigAct::TypeUntil`].
    TypeToSentinel,
}

/// **How long [`RigAct::TypeHeld`] holds a reply**: past the second a
/// shell that waited a second for it gave, and past the rig's two-second
/// bound on an iteration that ends on time, so neither line can decide
/// the row. Then it is written in two pieces, its first half and the
/// rest, as a tty may hand them to a raw read.
pub const HELD_REPLY: std::time::Duration = std::time::Duration::from_millis(2500);

/// **The backstop on an iteration that ends on an event**
/// ([`RigAct::TypeUntil`]): no pass or fail line, only the point past
/// which the rig stops waiting and says it timed out, never that the
/// effect did not run.
pub const SENTINEL_BACKSTOP: std::time::Duration = std::time::Duration::from_secs(10);

/// Whether the frame is driven, and whether a turn runs inline.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RigTicks {
    Driven,
    None,
    /// Driven, each turn run inline in the callback that asked.
    InCallback,
    /// Driven, each turn parked for the next frame.
    InFrame,
    /// Undriven, `turn::after` alone run inline in the callback that asked:
    /// with no tick only a callback can drain the read that asks it.
    AfterInCallback,
    /// Driven, each read left to the frame: a wake's turn takes none of it
    /// and the next frame all of it, so what a read asks is asked inside
    /// `frame`.
    Split,
}

/// Whether baud believes the rig's window is key: AppKit's answer alone,
/// which is never, or the probe's focus door (`probe::Key::Yes`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RigFocus {
    Real,
    Believed,
}

const ACTS: [(RigAct, &str); 11] = [
    (RigAct::Path, "path"),
    (RigAct::Type, "type"),
    (RigAct::Link, "link"),
    (RigAct::Select, "select"),
    (RigAct::CopyMenu, "copy-menu"),
    (RigAct::NoticeClick, "notice-click"),
    (RigAct::Close, "close"),
    (RigAct::TypeTwo, "type-two"),
    (RigAct::TypeUntil, "type-until"),
    (RigAct::TypeHeld, "type-held"),
    (RigAct::TypeToSentinel, "type-to-sentinel"),
];

const TICKS: [(RigTicks, &str); 6] = [
    (RigTicks::Driven, "tick"),
    (RigTicks::None, "no-tick"),
    (RigTicks::InCallback, "tick-inline"),
    (RigTicks::InFrame, "tick-in-frame"),
    (RigTicks::AfterInCallback, "tick-after-inline"),
    (RigTicks::Split, "tick-split"),
];

const FOCUS: [(RigFocus, &str); 2] = [
    (RigFocus::Real, "focus-real"),
    (RigFocus::Believed, "focus-believed"),
];

/// Why a call is refused, as the child says it before it exits 2.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Refused(pub &'static str);

fn word<T: PartialEq + Copy>(table: &[(T, &'static str)], t: T) -> &'static str {
    table.iter().find(|(k, _)| *k == t).map_or("?", |(_, w)| w)
}

fn of<T: Copy>(table: &[(T, &'static str)], w: &str) -> Option<T> {
    table.iter().find(|(_, v)| *v == w).map(|(k, _)| *k)
}

impl RigCall {
    /// The arguments that make this call, the program's name not among
    /// them.
    pub fn to_args(&self) -> Vec<String> {
        let event = match self {
            RigCall::Event(e) => e,
            RigCall::Frames(FrameRig {
                setup,
                act,
                attempts,
                focus,
                holders,
                window,
                keys,
                ticks,
            }) => {
                return vec![
                    FRAME_RIG.to_owned(),
                    setup.clone(),
                    act.clone(),
                    attempts.0.to_string(),
                    word(&FOCUS, *focus).to_owned(),
                    word(&HOLDERS, *holders).to_owned(),
                    word(&WINDOW, *window).to_owned(),
                    word(&KEYS, *keys).to_owned(),
                    word(&FRAME_TICKS, *ticks).to_owned(),
                ];
            }
        };
        let EventRig {
            act,
            arg,
            iterations,
            ticks,
            focus,
            each,
        } = event;
        vec![
            EVENT_RIG.to_owned(),
            word(&ACTS, *act).to_owned(),
            arg.clone(),
            iterations.0.to_string(),
            word(&TICKS, *ticks).to_owned(),
            word(&FOCUS, *focus).to_owned(),
            each.iter().map(|k| k.word()).collect::<Vec<_>>().join(","),
        ]
    }

    /// The call `args` make, the program's name not among them, or why
    /// they make none: anything but a well-formed rig call is refused.
    pub fn parse(args: &[String]) -> Result<RigCall, Refused> {
        if args.first().is_some_and(|f| f == FRAME_RIG) {
            let [_, setup, act, attempts, focus, holders, window, keys, ticks] = args else {
                return Err(Refused("--frame-rig takes eight arguments"));
            };
            return Ok(RigCall::Frames(FrameRig {
                setup: setup.clone(),
                act: act.clone(),
                attempts: attempts
                    .parse()
                    .map(Iterations)
                    .map_err(|_| Refused("--frame-rig: attempts are a count"))?,
                focus: of(&FOCUS, focus).ok_or(Refused("--frame-rig: no such focus"))?,
                holders: of(&HOLDERS, holders).ok_or(Refused("--frame-rig: no such holders"))?,
                window: of(&WINDOW, window).ok_or(Refused("--frame-rig: no such window event"))?,
                keys: of(&KEYS, keys).ok_or(Refused("--frame-rig: no such keys"))?,
                ticks: of(&FRAME_TICKS, ticks).ok_or(Refused("--frame-rig: no such ticks"))?,
            }));
        }
        let [flag, act, arg, iterations, ticks, focus, each] = args else {
            return Err(match args.first() {
                Some(f) if f == EVENT_RIG => Refused("--event-rig takes six arguments"),
                _ => Refused("a probe build runs only as a rig"),
            });
        };
        if flag != EVENT_RIG {
            return Err(Refused("a probe build runs only as a rig"));
        }
        let act = of(&ACTS, act).ok_or(Refused("--event-rig: no such act"))?;
        let ticks = of(&TICKS, ticks).ok_or(Refused("--event-rig: no such ticks"))?;
        let focus = of(&FOCUS, focus).ok_or(Refused("--event-rig: no such focus"))?;
        let iterations = iterations
            .parse()
            .map(Iterations)
            .map_err(|_| Refused("--event-rig: iterations are a count"))?;
        let each = each
            .split(',')
            .map(EffectKind::of)
            .collect::<Option<Vec<_>>>()
            .ok_or(Refused("--event-rig: no such effect"))?;
        Ok(RigCall::Event(EventRig {
            act,
            arg: arg.clone(),
            iterations,
            ticks,
            focus,
            each,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| (*s).to_owned()).collect()
    }

    /// **A call parses back to itself, and nothing else parses**: no
    /// arguments, a plain launch's, one too few or too many, and a word
    /// out of its table -- an act, a focus, an effect -- are each refused. Perturbation: accept a short
    /// call.
    #[test]
    fn a_call_round_trips_and_anything_else_is_refused() {
        let call = RigCall::Event(EventRig {
            act: RigAct::CopyMenu,
            arg: "printf x".to_owned(),
            iterations: Iterations(100),
            ticks: RigTicks::InFrame,
            focus: RigFocus::Believed,
            each: vec![EffectKind::Copy, EffectKind::Announce],
        });
        assert_eq!(RigCall::parse(&call.to_args()), Ok(call));
        let frames = RigCall::Frames(FrameRig {
            setup: "printf x".to_owned(),
            act: String::new(),
            attempts: Iterations(60),
            focus: RigFocus::Real,
            holders: RigHolders::Lifted,
            window: RigWindow::Restyled,
            keys: RigKeys::Pressed,
            ticks: RigFrameTicks::Withheld,
        });
        assert_eq!(RigCall::parse(&frames.to_args()), Ok(frames));
        for ticks in [RigFrameTicks::OneAfterFeeds, RigFrameTicks::EventFrames] {
            let frozen = RigCall::Frames(FrameRig {
                setup: "printf x".to_owned(),
                act: "seq 1 9".to_owned(),
                attempts: Iterations(1),
                focus: RigFocus::Believed,
                holders: RigHolders::Kept,
                window: RigWindow::PointerOver,
                keys: RigKeys::Line,
                ticks,
            });
            assert_eq!(RigCall::parse(&frozen.to_args()), Ok(frozen));
        }
        for refused in [
            args(&[]),
            args(&["README.md"]),
            args(&["--event-rig"]),
            args(&[
                "--frame-rig",
                "printf x",
                "",
                "60",
                "focus-real",
                "holders-kept",
                "untold",
            ]),
            args(&[
                "--frame-rig",
                "printf x",
                "",
                "60",
                "focus-real",
                "holders-gone",
                "untold",
                "line",
                "offered",
            ]),
            args(&[
                "--frame-rig",
                "printf x",
                "",
                "60",
                "focus-real",
                "holders-kept",
                "made-key",
                "line",
                "offered",
            ]),
            args(&[
                "--frame-rig",
                "printf x",
                "",
                "60",
                "focus-real",
                "holders-kept",
                "untold",
                "typed",
                "offered",
            ]),
            args(&[
                "--frame-rig",
                "printf x",
                "",
                "-1",
                "focus-real",
                "holders-kept",
                "untold",
                "line",
                "offered",
            ]),
            args(&[
                "--frame-rig",
                "printf x",
                "",
                "60",
                "focus-real",
                "holders-kept",
                "untold",
                "line",
                "dropped",
            ]),
            args(&["--event-rig", "path", "/a", "3", "tick", "copy"]),
            args(&[
                "--event-rig",
                "path",
                "/a",
                "3",
                "tick",
                "focus-real",
                "copy",
                "x",
            ]),
            args(&[
                "--event-rig",
                "walk",
                "/a",
                "3",
                "tick",
                "focus-real",
                "copy",
            ]),
            args(&[
                "--event-rig",
                "path",
                "/a",
                "-1",
                "tick",
                "focus-real",
                "copy",
            ]),
            args(&[
                "--event-rig",
                "path",
                "/a",
                "3",
                "tick",
                "focus-key",
                "copy",
            ]),
            args(&[
                "--event-rig",
                "path",
                "/a",
                "3",
                "tick",
                "focus-real",
                "copy,nothing",
            ]),
        ] {
            assert!(RigCall::parse(&refused).is_err(), "{refused:?}");
        }
    }
}
