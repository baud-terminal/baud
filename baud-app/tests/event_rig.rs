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
    /// The child said something else was wrong.
    Child,
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
    /// this config; the frame never driven.
    Program(Program),
}

/// A program's output, and the config it is read under.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Program {
    line: &'static str,
    config: &'static str,
}

/// What `[link.path]` makes of a printed path.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Rule {
    Reveal,
    Command,
    Copy,
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
}

/// What the row expects: these effects, each exactly once an iteration
/// and in this order, each in a turn of its own with the responder free,
/// its nested `wake:` delivered; and any number of the `paced` kinds, each
/// run so too. A paced effect is one baud holds to a pace of its own -- a
/// program's copy is announced at most once per message's length
/// (`copied.rs`) -- so how many fall in a run is not the row's to count.
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

const SITES: [Site; 10] = [
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
    // **The control.** The reveal row with `link::later` handing over
    // inline, as the reveal was handed over before it: the reveal runs
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
    // **The frame's control: the crash's own shape.** `link::later` parks
    // the hand-over and baud's `frame` makes it, as the reveal was made in
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
    // **Stage 2's rows: what a program writes**, each pending until its
    // site moves through the effect queue, its recorded failure what the
    // probe saw when the row was first run.
    Site {
        name: SiteName("OSC 52's write to the pasteboard"),
        cause: Cause::Program(Program {
            line: r"printf '\033]52;c;aGk=\007'",
            config: "",
        }),
        expect: Expect {
            each: &[EffectKind::Copy],
            paced: &[EffectKind::Announce],
        },
        state: RowState::Pending {
            stage: Stage::Two,
            recorded: Failure::InCallback(EffectKind::Copy, Nested::Never),
        },
    },
    Site {
        name: SiteName("OSC 52's clear of the pasteboard"),
        cause: Cause::Program(Program {
            line: r"printf '\033]52;c;!\007'",
            config: "",
        }),
        expect: Expect::effect(&[EffectKind::Clear]),
        state: RowState::Pending {
            stage: Stage::Two,
            recorded: Failure::InCallback(EffectKind::Clear, Nested::Never),
        },
    },
    Site {
        name: SiteName("the bell's Dock bounce, mode 1042"),
        cause: Cause::Program(Program {
            line: r"printf '\033[?1042h\007'",
            config: "",
        }),
        expect: Expect::effect(&[EffectKind::Attention]),
        state: RowState::Pending {
            stage: Stage::Two,
            recorded: Failure::InCallback(EffectKind::Attention, Nested::Never),
        },
    },
    Site {
        name: SiteName("the bell's raise, mode 1043"),
        cause: Cause::Program(Program {
            line: r"printf '\033[?1043h\007'",
            config: "",
        }),
        expect: Expect::effect(&[EffectKind::Front]),
        state: RowState::Pending {
            stage: Stage::Two,
            recorded: Failure::InCallback(EffectKind::Front, Nested::Never),
        },
    },
];

/// Iterations a row runs in its child.
const ITERATIONS: &str = "100";

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
    }
}

/// The row's cause in a child; what it came to.
fn run(site: &Site, slug: &str) -> Observed {
    let expect = site.expect;
    let dir = scratch(slug);
    let conf = dir.join("config.toml");
    let (act, arg, config, ticks) = match site.cause {
        Cause::PrintedPath(rule, ticks) => {
            let file = dir.join("clicked.txt");
            std::fs::write(&file, b"x").expect("the file clicked");
            let file = file.to_str().expect("a UTF-8 path").to_owned();
            ("path", file, config(rule), ticks)
        }
        // Undriven: a program's output is read by whichever of the
        // reader's `wake` and the frame comes first, so with the frame
        // driven a row's site would be one or the other by timing. The
        // frame reads it the same way, so the Tick arm's own guard covers
        // that entry.
        Cause::Program(p) => ("type", p.line.to_owned(), p.config, Ticks::None),
    };
    std::fs::write(&conf, config).expect("the config");
    let out = Command::new(env!("CARGO_BIN_EXE_baud"))
        .args([
            "--event-rig",
            act,
            &arg,
            ITERATIONS,
            match ticks {
                Ticks::Driven => "tick",
                Ticks::None => "no-tick",
                Ticks::DrivenInline => "tick-inline",
                Ticks::DrivenInFrame => "tick-in-frame",
            },
        ])
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
    judge(expect, &out, &dir)
}

/// What a child's output and exit say.
fn judge(expect: Expect, out: &std::process::Output, dir: &Path) -> Observed {
    let kinds = expect.each;
    use std::os::unix::process::ExitStatusExt;
    let text = String::from_utf8_lossy(&out.stdout);
    let _ = std::fs::write(dir.join("stdout"), &out.stdout);
    let _ = std::fs::write(dir.join("stderr"), &out.stderr);
    if out.status.signal().is_some() {
        return Observed::Failed(Failure::Aborted);
    }
    let lines: Vec<&str> = text.lines().collect();
    let iterations: usize = ITERATIONS.parse().expect("a count");
    let mut records = 0;
    for line in &lines {
        if *line == "none" {
            return Observed::Failed(Failure::NotRun);
        }
        let Some(r) = Record::parse(line) else {
            return Observed::Failed(Failure::Child);
        };
        let kind = match expect.paced.contains(&r.kind) {
            true => r.kind,
            false => {
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
