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

/// A rig's call: every mode a probe build runs as.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum RigCall {
    /// The event rig (`baud-app/src/app/rig.rs`).
    Event(EventRig),
}

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
}

/// Whether the frame is driven, and whether a turn runs inline.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RigTicks {
    Driven,
    None,
    /// Driven, each turn run inline in the callback that asked.
    InCallback,
    /// Driven, each turn parked for the next frame.
    InFrame,
}

/// Whether baud believes the rig's window is key: AppKit's answer alone,
/// which is never, or the probe's focus door (`probe::Key::Yes`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RigFocus {
    Real,
    Believed,
}

const ACTS: [(RigAct, &str); 5] = [
    (RigAct::Path, "path"),
    (RigAct::Type, "type"),
    (RigAct::Link, "link"),
    (RigAct::Select, "select"),
    (RigAct::CopyMenu, "copy-menu"),
];

const TICKS: [(RigTicks, &str); 4] = [
    (RigTicks::Driven, "tick"),
    (RigTicks::None, "no-tick"),
    (RigTicks::InCallback, "tick-inline"),
    (RigTicks::InFrame, "tick-in-frame"),
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
        let RigCall::Event(EventRig {
            act,
            arg,
            iterations,
            ticks,
            focus,
            each,
        }) = self;
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
        for refused in [
            args(&[]),
            args(&["README.md"]),
            args(&["--event-rig"]),
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
