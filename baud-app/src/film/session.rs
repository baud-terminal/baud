//! **The README session without a window, and the lights it lights**: the
//! steps the `demo` example types into baud's window
//! (`baud-app/examples/demo/session.rs`, included here as it is), typed
//! instead into baud's own login shell on a pty against the real core, as
//! [`super::film`] takes a demo -- every program the session runs, the
//! container it ssh's into among them -- while the nine activity lights
//! are fed exactly as the macOS driver feeds them, from the same clocks
//! (`baud_render::Activity`) and the same reads:
//!
//! * **RD** each read of the program's output, and **HS** each one as long
//!   as the tty hands back (`baud_render::lights::full`, Darwin's 1024): a
//!   read here is one `read(2)` of the master, as the app's reader's is.
//! * **AA** each read that left the core an answer of its own to send.
//! * **SD** each write to the child, the keys typed and the answers.
//! * **SYN** while the program holds synchronised output open and **FAX**
//!   while a picture is placed, sampled at most once a 60 Hz frame
//!   (`own_line::SAMPLE`), as the display link samples them.
//! * **OH** and **CD** from `front::Front` over the kernel's answers
//!   (`front::Live`): a job other than the shell in front, and an ssh among
//!   it holding an established TCP connection.
//! * **CS** lit from the first turn, the line clear, and dark for the hold
//!   after a write the kernel took only part of at once (`Under::shorts`),
//!   as the app's is for a write it refused any of. The harness writes
//!   until the kernel has taken every byte, so nothing is owed at a turn,
//!   and in the session nothing is likely to darken it: the program in
//!   front reads its input throughout, and keys at a typist's pace never
//!   fill the tty's input queue -- that needs a program that stops reading
//!   while more than a kilobyte is owed it -- a paste into a program that
//!   is not reading, or a program that floods queries and stops reading
//!   their answers.
//!
//! [`Lit`] is what each light showed, when it first lit, by the step that
//! had begun then, and when it first went out after that.

use super::{Event, Logged, middle};
use crate::programs::{Answered, Shorts, Under};
use crate::seat::{Geometry, Input};
use baud_core::Term;
use baud_domain::{BytePos, HostTime};
use baud_render::{Activity, ByLed, Led, Light, Pace, Shown};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

#[allow(
    dead_code,
    reason = "the demo's session, of which this plays the steps"
)]
#[path = "../../examples/demo/session.rs"]
mod steps;

pub(crate) use steps::{SESSION, Step};

/// How long a prompt, an echo and the container's prompt may take.
const PROMPT_LIMIT: Duration = Duration::from_secs(15);
const ECHO_LIMIT: Duration = Duration::from_secs(3);

/// Whether the session is still playing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Turn {
    Going,
    Done,
}

/// Where the step at hand is.
#[derive(Clone, Debug)]
enum Doing {
    /// Not begun.
    Start,
    /// Keys to type, the next at `next`; `then` once they are typed.
    Typing {
        keys: VecDeque<u8>,
        next: Duration,
        then: Then,
    },
    /// Waiting since `since` for the echo of what was typed, then Return.
    Echo { since: Duration },
    /// Waiting for a prompt since `since`.
    Waiting { since: Duration },
    /// Holding until `until`.
    Holding { until: Duration },
}

/// What follows a step's typing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Then {
    /// The echo is awaited, then Return.
    Echo,
    /// The step is done.
    Done,
}

/// **The session's steps, typed into the shell** as the demo types them
/// into baud's window: a key every `KEY_GAP_MS`, a command's Return only
/// once its echo has come back, Escape encoded by the core as the
/// program on screen asked (`Term::key`), as the window's key goes, a
/// prompt counted by `OSC 133 ; A` marks, baud's shell's and the
/// container's, and the wheel at the grid's centre.
pub(crate) struct Player<'a> {
    steps: &'a [Step],
    at: StepAt,
    doing: Doing,
    /// The prompts waited for so far, and the marks seen.
    prompts: Marks,
    marks: Marks,
    /// How far into what was heard the marks are counted, and where the
    /// last typing began.
    counted: BytePos,
    typed_from: BytePos,
    geometry: Geometry,
}

/// A step of the session, by its place in the list.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Debug)]
pub(crate) struct StepAt(pub(crate) usize);

/// A count of baud's prompt marks.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Debug)]
struct Marks(usize);

impl<'a> Player<'a> {
    pub(crate) fn new(steps: &'a [Step], geometry: Geometry) -> Player<'a> {
        Player {
            steps,
            at: StepAt(0),
            doing: Doing::Start,
            prompts: Marks(0),
            marks: Marks(0),
            counted: BytePos::ZERO,
            typed_from: BytePos::ZERO,
            geometry,
        }
    }

    /// The step at hand, by its index.
    pub(crate) fn step(&self) -> StepAt {
        self.at
    }

    /// The prompt marks in `heard`, counted as it grows.
    fn count(&mut self, heard: &[u8]) {
        let from = self
            .counted
            .index()
            .saturating_sub(steps::PROMPT_MARK.len() - 1);
        self.marks = Marks(self.marks.0 + steps::marks(&heard[from..]));
        self.counted = BytePos::from_index(heard.len());
    }

    /// **One turn of the take at `now`**: the keys due typed, the waits
    /// looked at, the wheel's inputs queued for the take to send and log,
    /// and each step's start logged.
    pub(super) fn turn(
        &mut self,
        now: Duration,
        heard: &[u8],
        u: &mut Under<'_, '_>,
        inputs: &mut VecDeque<(Duration, Input)>,
        log: &mut Vec<Logged>,
        epoch: Instant,
    ) -> Turn {
        self.count(heard);
        loop {
            let Some(&step) = self.steps.get(self.at.0) else {
                return Turn::Done;
            };
            let gap = Duration::from_secs_f64(steps::KEY_GAP_MS / 1000.0);
            if let Doing::Start = self.doing {
                log.push(Logged {
                    at: epoch.elapsed(),
                    event: Event::Step(self.at.0),
                });
            }
            let doing = std::mem::replace(&mut self.doing, Doing::Start);
            let next = match (step, doing) {
                (Step::Prompt, Doing::Start) => {
                    self.prompts = Marks(self.prompts.0 + 1);
                    Some(Doing::Waiting { since: now })
                }
                (Step::Prompt, Doing::Waiting { since }) => {
                    if self.marks >= self.prompts {
                        None
                    } else {
                        self.limit(
                            now,
                            since,
                            PROMPT_LIMIT,
                            u,
                            &format!("prompt {}", self.prompts.0),
                        );
                        Some(Doing::Waiting { since })
                    }
                }
                (Step::Run(t) | Step::Keys(t) | Step::Line(t), Doing::Start) => {
                    self.typed_from = BytePos::from_index(heard.len());
                    let mut keys: VecDeque<u8> = t.bytes().collect();
                    if matches!(step, Step::Line(_)) {
                        keys.push_back(b'\r');
                    }
                    let then = match step {
                        Step::Run(_) => Then::Echo,
                        _ => Then::Done,
                    };
                    Some(Doing::Typing {
                        keys,
                        next: now,
                        then,
                    })
                }
                (
                    _,
                    Doing::Typing {
                        mut keys,
                        next,
                        then,
                    },
                ) => {
                    if now < next {
                        self.doing = Doing::Typing { keys, next, then };
                        return Turn::Going;
                    }
                    if let Some(k) = keys.pop_front() {
                        self.press(u, log, epoch, &[k]);
                    }
                    match (keys.is_empty(), then) {
                        (false, _) => Some(Doing::Typing {
                            keys,
                            next: now + gap,
                            then,
                        }),
                        (true, Then::Echo) => Some(Doing::Echo { since: now }),
                        (true, Then::Done) => None,
                    }
                }
                (Step::Run(t), Doing::Echo { since }) => {
                    if steps::echoed(heard, self.typed_from.index(), t) && now >= since + gap {
                        self.press(u, log, epoch, b"\r");
                        None
                    } else {
                        self.limit(now, since, ECHO_LIMIT, u, &format!("the echo of `{t}`"));
                        Some(Doing::Echo { since })
                    }
                }
                (Step::Escape, Doing::Start) => {
                    let key = baud_core::KeyEvent::press(
                        baud_domain::KeyCode::Escape,
                        baud_domain::Modifiers::NONE,
                    );
                    let mut out = Vec::new();
                    crate::reads::input(u.term, |t| t.key(key), |b| out.extend_from_slice(b));
                    self.press(u, log, epoch, &out);
                    None
                }
                (Step::Hold(ms), Doing::Start) => Some(Doing::Holding {
                    until: now + Duration::from_millis(u64::from(ms)),
                }),
                (Step::Hold(_), Doing::Holding { until }) => {
                    if now >= until {
                        None
                    } else {
                        Some(Doing::Holding { until })
                    }
                }
                (Step::Wheel { turns, lines }, Doing::Start) => {
                    let g = self.geometry;
                    let centre = middle(
                        g,
                        (
                            f64::from(g.grid.1) / 2.0 - 0.5,
                            f64::from(g.grid.0) / 2.0 - 0.5,
                        ),
                    );
                    let ms = |d: u64| now + Duration::from_millis(d);
                    inputs.push_back((now, Input::Moved(centre.0, centre.1)));
                    let settle = steps::WHEEL_SETTLE_MS;
                    // A turn up the history is a negative step, as the cue
                    // file's are.
                    let one = -f64::from(lines * turns.signum());
                    let n = u64::from(turns.unsigned_abs());
                    for i in 0..n {
                        inputs.push_back((ms(settle + i * steps::WHEEL_GAP_MS), Input::Steps(one)));
                    }
                    let end = ms(settle + n * steps::WHEEL_GAP_MS);
                    inputs.push_back((end, Input::Stopped));
                    Some(Doing::Holding { until: end })
                }
                (Step::Wheel { .. }, Doing::Holding { until }) => {
                    if now >= until {
                        None
                    } else {
                        Some(Doing::Holding { until })
                    }
                }
                (s, d) => panic!(
                    "step {} ({s:?}) in a state it cannot be in: {d:?}",
                    self.at.0
                ),
            };
            match next {
                Some(d) => {
                    self.doing = d;
                    return Turn::Going;
                }
                None => {
                    self.at = StepAt(self.at.0 + 1);
                    self.doing = Doing::Start;
                }
            }
        }
    }

    /// Keys written to the child, and logged for the film's terminal.
    fn press(&mut self, u: &mut Under<'_, '_>, log: &mut Vec<Logged>, epoch: Instant, b: &[u8]) {
        u.typed(b);
        log.push(Logged {
            at: epoch.elapsed(),
            event: Event::Typed(b.to_vec()),
        });
    }

    /// Fails with the screen once `limit` has gone by since `since`.
    fn limit(
        &self,
        now: Duration,
        since: Duration,
        limit: Duration,
        u: &Under<'_, '_>,
        what: &str,
    ) {
        assert!(
            now < since + limit,
            "step {}: {what} not within {limit:?}; the screen:\n{}",
            self.at.0,
            crate::programs::screen(u.term).join("\n")
        );
    }
}

/// **What the lights showed through a take**: for each, the first moment
/// it showed lit and the step that had begun then, or none, and the first
/// moment after that it showed anything else.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) struct Lit(
    ByLed<Option<(Duration, Option<StepAt>)>>,
    ByLed<Option<(Duration, Option<StepAt>)>>,
);

impl Lit {
    /// When `led` first showed lit, and in which step.
    pub(crate) fn first(&self, led: Led) -> Option<(Duration, Option<StepAt>)> {
        *self.0.get(led)
    }

    /// When `led` first went out once it had lit, and in which step.
    pub(crate) fn out(&self, led: Led) -> Option<(Duration, Option<StepAt>)> {
        *self.1.get(led)
    }

    /// The lights that never showed lit.
    pub(crate) fn dark(&self) -> Vec<Led> {
        Led::ALL
            .into_iter()
            .filter(|&l| self.first(l).is_none())
            .collect()
    }
}

/// **The lights' clocks and what feeds them**, as the macOS driver's.
pub(crate) struct Lights {
    activity: Activity,
    front: crate::front::Front,
    procs: crate::front::Live,
    shell: crate::sys::Pid,
    sampled: Option<Duration>,
    /// The synchronised update at the last sampling, so one opened and
    /// closed between two turns still lights SYN.
    synced: crate::bell::Synced,
    /// How much had been written to the child at the last turn.
    wrote: BytePos,
    /// How many writes the kernel had taken only part of at the last turn.
    shorts: Shorts,
    lit: Lit,
    /// What the lights showed, at each turn it changed: what the film's
    /// title strip draws at each frame (`chrome.rs`).
    timeline: Vec<(Duration, Shown)>,
}

/// A take's instant as the host's clock, a second in so nothing is at 0.
fn host(at: Duration) -> HostTime {
    HostTime::of_nanos(u64::try_from((at + Duration::from_secs(1)).as_nanos()).unwrap_or(u64::MAX))
}

impl Lights {
    pub(crate) fn new(shell: crate::sys::Pid) -> Lights {
        Lights {
            activity: Activity::new(),
            front: crate::front::Front::new(),
            procs: crate::front::Live::new(),
            shell,
            sampled: None,
            synced: crate::bell::Synced::new(),
            wrote: BytePos::ZERO,
            shorts: Shorts::NONE,
            lit: Lit(ByLed::of(|_| None), ByLed::of(|_| None)),
            timeline: Vec::new(),
        }
    }

    /// One read of the program's output at `at`: RD, HS for one at the
    /// tty's ceiling, AA when it left the core an answer of its own.
    pub(crate) fn read(&mut self, bytes: &[u8], answered: Answered, at: Duration) {
        if bytes.is_empty() {
            return;
        }
        let now = host(at);
        self.activity.seen(Led::Rd, now);
        if baud_render::lights::full(bytes) {
            self.activity.seen(Led::Hs, now);
        }
        if answered == Answered::Bytes {
            self.activity.seen(Led::Aa, now);
        }
    }

    /// **A turn at `at`**: SD for any byte written to the child since the
    /// last, CS dark for any write of the `shorts` the kernel took only
    /// part of since the last, SYN, FAX, OH and CD sampled once a frame,
    /// and the clocks stepped, each light's first lit moment and first
    /// moment out after it kept with the step `player` is at.
    pub(crate) fn turn(
        &mut self,
        at: Duration,
        term: &Term<'_>,
        wrote: BytePos,
        shorts: Shorts,
        player: Option<&Player<'_>>,
    ) {
        let now = host(at);
        if wrote > self.wrote {
            self.wrote = wrote;
            self.activity.seen(Led::Sd, now);
        }
        if shorts > self.shorts {
            self.shorts = shorts;
            self.activity.seen(Led::Cs, now);
        }
        let due = self
            .sampled
            .is_none_or(|s| at >= s + crate::own_line::SAMPLE);
        if due {
            self.sampled = Some(at);
            if self.synced.seen(term) {
                self.activity.seen(Led::Syn, now);
            }
            if term.images().any_placed() {
                self.activity.seen(Led::Fax, now);
            }
            let (oh, cd) =
                self.front
                    .sample(&mut self.procs, self.shell, now, crate::front::RELIST);
            crate::lights::feed(&mut self.activity, Led::Oh, oh, now);
            crate::lights::feed(&mut self.activity, Led::Cd, cd, now);
        }
        let shown = self.activity.step(now, Pace::Full);
        if self.timeline.last().map_or(Shown::rest(), |t| t.1) != shown {
            self.timeline.push((at, shown));
        }
        let step = player.map(Player::step);
        for led in Led::ALL {
            let lit = *shown.get(led) == Light::Lit;
            let first = self.lit.0.get_mut(led);
            if first.is_none() && lit {
                *first = Some((at, step));
            }
            let lit_before = first.is_some();
            let out = self.lit.1.get_mut(led);
            if out.is_none() && lit_before && !lit {
                *out = Some((at, step));
            }
        }
    }

    pub(crate) fn lit(&self) -> Lit {
        self.lit
    }

    /// What the lights showed, at each turn it changed, from all dark.
    pub(crate) fn timeline(&mut self) -> Vec<(Duration, Shown)> {
        std::mem::take(&mut self.timeline)
    }
}

/// **What the lights showed at `at`**: the last change at or before it in
/// `timeline`, every light at rest before the first, CS lit
/// (`Shown::rest`), as the clocks start; `from` is where the last look
/// left off, as a film's frames come in order.
pub(crate) fn shown_at(timeline: &[(Duration, Shown)], from: &mut usize, at: Duration) -> Shown {
    while timeline.get(*from).is_some_and(|t| t.0 <= at) {
        *from += 1;
    }
    from.checked_sub(1)
        .and_then(|i| timeline.get(i))
        .map_or(Shown::rest(), |t| t.1)
}

/// **`steps` without the command typed as `run`**: its `Run`, and every
/// step after it through the `Prompt` it comes back to and the hold after
/// that -- a command taken out of the session whole, for a perturbation.
/// `ssh` comes back to baud's shell only after the `exit` typed in the
/// container, so taking it out takes out everything typed there.
pub(crate) fn without(steps: &[Step], run: &str) -> Vec<Step> {
    let Some(from) = steps
        .iter()
        .position(|s| matches!(s, Step::Run(t) if *t == run))
    else {
        panic!("no step runs `{run}`");
    };
    let back = match run.starts_with("ssh ") {
        true => {
            from + steps[from..]
                .iter()
                .position(|s| *s == Step::Run("exit"))
                .expect("an exit from the container")
        }
        false => from,
    };
    let prompt = back
        + steps[back..]
            .iter()
            .position(|s| *s == Step::Prompt)
            .expect("a prompt after the command");
    let to = match steps.get(prompt + 1) {
        Some(Step::Hold(_)) => prompt + 2,
        _ => prompt + 1,
    };
    let mut v = steps[..from].to_vec();
    v.extend_from_slice(&steps[to..]);
    v
}

/// The scratch directory's `env`, a `KEY=value` a line, as the demo writes
/// it for this test.
fn env_of(text: &str) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    text.lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.into(), v.into()))
        .collect()
}

/// **The lights the app's feeders light, fed as the app feeds them**: a
/// read of the tty's 1024 lights RD and HS, a shorter one RD alone, a read
/// the core answered AA, a write SD, `?2026` held open SYN and a placed
/// picture FAX -- each from the core's own state -- and a turn with
/// nothing new lights nothing more; an update opened and closed between
/// two turns lights SYN too. CS is lit from the first turn, the line
/// clear, and a write the kernel took only part of darkens it for the
/// hold, then it lights again, its first moment out kept. The controls: a
/// read of 1023 bytes does not light HS, a turn reads OH and CD of a
/// shell that is not there as dark, two turns with no update between them
/// leave SYN dark, and no short write leaves CS lit throughout.
#[test]
fn the_lights_light_from_what_the_app_reads() {
    let grid = baud_domain::WinSize {
        ws_col: 20,
        ws_row: 5,
    };
    let mut owned = baud_core::Owned::new(&grid);
    let mut term = Term::new(owned.storage(), &grid);
    let gone = crate::sys::Pid::MAX;
    let mut l = Lights::new(gone);
    let ms = Duration::from_millis;
    l.read(&[b'a'; 1023], Answered::Nothing, ms(10));
    l.turn(ms(10), &term, BytePos::ZERO, Shorts::NONE, None);
    assert_eq!(l.lit().first(Led::Rd), Some((ms(10), None)));
    assert_eq!(l.lit().first(Led::Cs), Some((ms(10), None)), "CS, clear");
    assert_eq!(
        l.lit().first(Led::Hs),
        None,
        "1023 bytes is not the ceiling"
    );
    l.read(&[b'a'; 1024], Answered::Bytes, ms(40));
    l.turn(ms(40), &term, BytePos::from_index(3), Shorts::NONE, None);
    for led in [Led::Hs, Led::Aa, Led::Sd] {
        assert_eq!(l.lit().first(led), Some((ms(40), None)), "{led:?}");
    }
    crate::reads::feed(
        &mut term,
        [&b"\x1b[?2026h"[..]],
        |_| {},
        |_| {},
        &mut crate::hostlog::NoLog,
    );
    assert!(term.synchronized_output());
    l.turn(ms(70), &term, BytePos::from_index(3), Shorts::NONE, None);
    assert_eq!(l.lit().first(Led::Syn), Some((ms(70), None)));
    assert_eq!(
        l.lit().dark(),
        [Led::Cd, Led::Oh, Led::Fax],
        "nothing else lit"
    );
    assert_eq!(
        l.lit().out(Led::Cs),
        None,
        "no short write, CS lit throughout"
    );
    let one = Shorts::NONE.next();
    l.turn(ms(100), &term, BytePos::from_index(9), one, None);
    assert_eq!(l.lit().out(Led::Cs), Some((ms(100), None)), "a short write");
    l.turn(ms(299), &term, BytePos::from_index(9), one, None);
    assert_eq!(l.activity.shown().cs, Light::Dark, "the hold");
    l.turn(ms(300), &term, BytePos::from_index(9), one, None);
    assert_eq!(l.activity.shown().cs, Light::Lit, "clear again");
    // An update opened and closed between two turns, as btop's through
    // ssh, lights SYN; two turns with none between leave it dark.
    let mut owned = baud_core::Owned::new(&grid);
    let mut term = Term::new(owned.storage(), &grid);
    let mut l = Lights::new(gone);
    l.turn(ms(10), &term, BytePos::ZERO, Shorts::NONE, None);
    l.turn(ms(40), &term, BytePos::ZERO, Shorts::NONE, None);
    assert_eq!(l.lit().first(Led::Syn), None, "no update");
    crate::reads::feed(
        &mut term,
        [&b"\x1b[?2026hframe\x1b[?2026l"[..]],
        |_| {},
        |_| {},
        &mut crate::hostlog::NoLog,
    );
    assert!(!term.synchronized_output());
    l.turn(ms(70), &term, BytePos::ZERO, Shorts::NONE, None);
    assert_eq!(l.lit().first(Led::Syn), Some((ms(70), None)));
}

/// **A frame's lights are the last change at or before it**: at rest,
/// CS lit, before the first, the change itself at its instant, and the one before
/// a later change until it comes, looked up in the frames' order; and a
/// turn records a change only when the lights moved. The control: the
/// frame a millisecond before the second change still shows the first.
/// Perturbation: take the first change after the frame rather than the
/// last before it and the 50 ms frame shows RD dark.
#[test]
fn a_frames_lights_are_the_last_change_before_it() {
    let ms = Duration::from_millis;
    let rd = ByLed {
        rd: Light::Lit,
        ..Shown::rest()
    };
    let timeline = [(ms(10), rd), (ms(80), Shown::rest())];
    let mut from = 0;
    let at = |t: u64, from: &mut usize| shown_at(&timeline, from, ms(t));
    assert_eq!(at(0, &mut from), Shown::rest(), "at rest, CS lit");
    assert_eq!(at(10, &mut from), rd);
    assert_eq!(at(50, &mut from), rd);
    assert_eq!(at(79, &mut from), rd, "the control");
    assert_eq!(at(80, &mut from), Shown::rest());
    assert_eq!(at(500, &mut from), Shown::rest());
    let grid = baud_domain::WinSize {
        ws_col: 20,
        ws_row: 5,
    };
    let mut owned = baud_core::Owned::new(&grid);
    let term = Term::new(owned.storage(), &grid);
    let mut l = Lights::new(crate::sys::Pid::MAX);
    l.turn(ms(5), &term, BytePos::ZERO, Shorts::NONE, None);
    l.read(b"a", Answered::Nothing, ms(10));
    l.turn(ms(10), &term, BytePos::ZERO, Shorts::NONE, None);
    l.turn(ms(11), &term, BytePos::ZERO, Shorts::NONE, None);
    let t = l.timeline();
    assert_eq!(t.len(), 1, "one change, RD lit: {t:?}");
    assert_eq!((t[0].0, *t[0].1.get(Led::Rd)), (ms(10), Light::Lit));
}

/// **`without` takes a command out whole**: a command at baud's shell
/// through the prompt it comes back to and the hold after it, and `ssh`
/// through the `exit` typed in the container, since everything between is
/// typed there. The control: without `fastfetch`, btop and what follows it
/// stay.
#[test]
fn without_takes_a_command_out_whole() {
    let s = SESSION;
    let cut = without(s, "ssh demo");
    assert!(!cut.contains(&Step::Run("ssh demo")));
    for gone in ["fastfetch", "btop", "exit", steps::FIREWORKS] {
        assert!(!cut.contains(&Step::Run(gone)), "{gone}");
    }
    assert!(
        cut.contains(&Step::Run("clear")),
        "baud's own after it stays"
    );
    let ff = without(s, "fastfetch");
    assert_eq!(
        ff.len(),
        s.len() - 3,
        "the command, its prompt and its hold"
    );
    assert!(ff.contains(&Step::Run("btop")));
}

/// **The README session lights every light**, played without a
/// window by `cargo run --example demo -- --headless DIR`, which makes the
/// scratch directory `BAUD_SESSION_ROOT` names, starts the container and
/// runs this: the session's steps (`SESSION`) typed into baud's own login
/// shell started as baud starts it, with that directory's `env` as baud's
/// environment, the config where baud finds it with that environment --
/// read again when helix saves it, as baud reads it -- and its 80x24 grid.
/// Each light's first lit moment is printed with the step it came in, and
/// every one must light, CS from the first turn; when CS first went out,
/// if it did, is printed too (the module's doc says why it rarely can). The
/// trail the first `ge` draws must be plasma and the ones `gg` and the
/// second `ge` draw liquid, after the edit saved between them, and `gg` or
/// its hold must light a liquid: each ignition the film's trail lit is
/// printed with its step. FAX must first light in the photo's shot,
/// kitten's picture, before fastfetch's.
///
/// `BAUD_SESSION_WITHOUT` takes a command out ([`without`]), for a
/// perturbation: without `ssh demo` CD stays dark, and without the photo
/// FAX first lights in fastfetch, and the test fails.
/// `BAUD_SESSION_POINTS` draws at another size, written into the config
/// file first, and with `BAUD_SESSION_STILLS` and `BAUD_FILM_FFMPEG` the
/// last frame of every hold is written there as a PNG, through baud's CPU
/// renderer on the opaque page, the window's title strip composed above
/// it (`chrome.rs`): the stills of the session at the size it is filmed
/// at. With `BAUD_SESSION_FILM` every frame, composed so, goes into that
/// file, lossless, at `BAUD_SESSION_PERIOD_MS` a frame (40 without it),
/// and beside it `<file>.shots` names the frames of the trails' shot, the
/// photo's, the fireworks' and the clear's, for the flash census
/// (`baud-bench/tests/session_flashes.rs`), and `<file>.steps` each step's
/// first frame.
#[test]
#[ignore = "a tool: `cargo run --example demo -- --headless DIR` runs it"]
fn the_readme_session_lights_every_light() {
    use crate::config::CursorTrail;
    use std::io::Write as _;
    let var = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("{k} is not set"));
    let root = std::path::PathBuf::from(var("BAUD_SESSION_ROOT"));
    let read = |name: &str| {
        std::fs::read_to_string(root.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
    };
    let env = env_of(&read("env"));
    let get = |k: &str| {
        env.iter()
            .find(|(key, _)| key == k)
            .and_then(|(_, v)| v.to_str())
            .unwrap_or_else(|| panic!("{k} in the env"))
            .to_string()
    };
    let grid = super::grid_of(&get("BAUD_GRID"));
    // Where baud looks with no `BAUD_CONFIG` and no `XDG_CONFIG_HOME`,
    // which the env holds neither of.
    assert!(
        env.iter()
            .all(|(k, _)| k != "BAUD_CONFIG" && k != "XDG_CONFIG_HOME")
    );
    let home = std::path::PathBuf::from(get("HOME"));
    let source = crate::config::config_paths(None, None, Some(home))
        .pop()
        .expect("the home's config");
    let mut config =
        std::fs::read_to_string(&source).unwrap_or_else(|e| panic!("{}: {e}", source.display()));
    if let Ok(p) = std::env::var("BAUD_SESSION_POINTS") {
        let line = config.lines().next().unwrap_or("").to_string();
        assert!(line.starts_with("font_size = "), "{line}");
        config = config.replacen(&line, &format!("font_size = {p}"), 1);
        std::fs::write(&source, &config).expect("the config at the size asked for");
    }
    let steps = match std::env::var("BAUD_SESSION_WITHOUT") {
        Ok(run) => without(SESSION, &run),
        Err(_) => SESSION.to_vec(),
    };
    let period = std::env::var("BAUD_SESSION_PERIOD_MS")
        .map(|v| Duration::from_millis(v.parse().expect("BAUD_SESSION_PERIOD_MS")))
        .unwrap_or(Duration::from_millis(40));
    let dir = root.join("home/baud");
    let f = super::Film {
        child: super::Child::Session {
            steps: &steps,
            dir: &dir,
        },
        env,
        grid,
        config: &config,
        source: Some(&source),
        period,
        limit: Duration::from_secs(180),
    };
    let stills = std::env::var_os("BAUD_SESSION_STILLS").map(std::path::PathBuf::from);
    let ffmpeg = std::env::var_os("BAUD_FILM_FFMPEG");
    let film_to = std::env::var_os("BAUD_SESSION_FILM").map(std::path::PathBuf::from);
    let mut encoder: Option<std::process::Child> = None;
    let mut frame_steps: Vec<Option<usize>> = Vec::new();
    // Each frame's step and whether its cursor was drawn short of whole.
    let mut blinked: Vec<(Option<usize>, bool)> = Vec::new();
    let mut size = (0, 0);
    let mut last: Option<Shot> = None;
    let mut written = 0;
    // The last frame of the hold `i`, named for the step it follows.
    let mut still = |i: usize, px: &[u32], size: (u32, u32)| {
        let (Some(dir), Step::Hold(_)) = (&stills, steps[i]) else {
            return;
        };
        let what = steps[..i]
            .iter()
            .rev()
            .find_map(|s| match s {
                Step::Run(t) | Step::Keys(t) | Step::Line(t) => Some(*t),
                _ => None,
            })
            .unwrap_or("launch");
        let name: String = what
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let to = dir.join(format!("{i:02}-{name}.png"));
        png(ffmpeg.as_ref().expect("BAUD_FILM_FFMPEG"), px, size, &to);
        written += 1;
    };
    // The window's title strip over every frame, its lights as the take's
    // clocks showed them then (`chrome.rs`): composed, not filmed.
    let mut strip: Option<super::chrome::Strip> = None;
    let mut window: Vec<u32> = Vec::new();
    let take = super::film(&f, |fr| {
        blinked.push((fr.step, fr.caret != baud_render::CursorPhase::SHOWN));
        let s = strip
            .get_or_insert_with(|| super::chrome::Strip::new(fr.size.0, f64::from(super::SCALE)));
        s.compose(fr.lights, fr.pixels, &mut window);
        let px = (fr.size.0.value(), fr.size.1.value() + s.height().value());
        if let Some(to) = &film_to {
            let e = encoder.get_or_insert_with(|| {
                size = px;
                lossless(ffmpeg.as_ref().expect("BAUD_FILM_FFMPEG"), px, period, to)
            });
            let bytes: Vec<u8> = window.iter().flat_map(|p| p.to_le_bytes()).collect();
            e.stdin
                .as_mut()
                .expect("ffmpeg's input")
                .write_all(&bytes)
                .expect("a frame to ffmpeg");
            frame_steps.push(fr.step);
        }
        if stills.is_none() {
            return;
        }
        if let Some((Some(i), px, size)) = &last {
            if fr.step != Some(*i) {
                still(*i, px, *size);
            }
        }
        last = Some((fr.step, window.clone(), px));
    });
    if let Some((Some(i), px, size)) = &last {
        still(*i, px, *size);
    }
    if let (Some(mut e), Some(to)) = (encoder, &film_to) {
        drop(e.stdin.take());
        assert!(
            e.wait().expect("ffmpeg").success(),
            "ffmpeg failed on the film"
        );
        let shots = shots(&steps, &frame_steps, size, period);
        let mut list = to.clone().into_os_string();
        list.push(".shots");
        std::fs::write(&list, shots).expect("the shots' list");
        // Each step's first frame, for cutting a still from the film.
        let mut firsts = String::new();
        for (i, step) in steps.iter().enumerate() {
            if let Some(f) = frame_steps.iter().position(|s| *s == Some(i)) {
                firsts.push_str(&format!("{i} {f} {step:?}\n"));
            }
        }
        let mut list = to.clone().into_os_string();
        list.push(".steps");
        std::fs::write(&list, firsts).expect("the steps' list");
        println!("film:    {} frames, {}", frame_steps.len(), to.display());
    }
    for led in Led::ALL {
        match take.lit.first(led) {
            Some((at, step)) => println!(
                "lights:  {:<3} lit at {:>6.2} s, in step {}",
                led.letters(),
                at.as_secs_f64(),
                step.map_or("-".to_string(), |StepAt(i)| format!("{i} {:?}", steps[i]))
            ),
            None => println!("lights:  {:<3} never lit", led.letters()),
        }
    }
    match take.lit.out(Led::Cs) {
        Some((at, step)) => println!(
            "lights:  CS  out at {:>6.2} s, in step {}",
            at.as_secs_f64(),
            step.map_or("-".to_string(), |StepAt(i)| format!("{i} {:?}", steps[i]))
        ),
        None => println!("lights:  CS  never out"),
    }
    for (step, trail) in &take.trails {
        println!(
            "trail:   {trail:?} from step {}",
            step.map_or("-".to_string(), |i| format!("{i} {:?}", steps[i]))
        );
    }
    for (step, trail, at) in &take.ignitions {
        println!(
            "ignited: {trail:?} at {:>6.2} s, in step {}",
            at.as_secs_f64(),
            step.map_or("-".to_string(), |i| format!("{i} {:?}", steps[i]))
        );
    }
    if stills.is_some() {
        println!("stills:  {written} written");
    }
    // What the programs printed and what was sent them, for the demo's
    // search of what must never be filmed, and searched here for the
    // machine's names as a film's are.
    let bytes = [&take.heard[..], &take.answers[..]].concat();
    std::fs::write(root.join("heard.log"), &bytes).expect("heard.log");
    let names = super::machine_names(&[]);
    assert_eq!(
        super::named(&bytes, &names),
        Vec::<String>::new(),
        "the session names this machine"
    );
    // The trail as each jump began: plasma down to the trail's line, liquid
    // back up once the edit was saved, and down again.
    let at = |want: Step| steps.iter().position(|s| *s == want);
    let again = steps.iter().rposition(|s| *s == Step::Keys("ge"));
    if let (Some(down), Some(up)) = (at(Step::Keys("ge")), at(Step::Keys("gg"))) {
        assert_eq!(
            trail_as(&take.trails, StepAt(down)),
            CursorTrail::Plasma,
            "at `ge`"
        );
        assert_eq!(
            trail_as(&take.trails, StepAt(up)),
            CursorTrail::Liquid,
            "at `gg`"
        );
        if let Some(again) = again.filter(|&a| a > up) {
            assert_eq!(
                trail_as(&take.trails, StepAt(again)),
                CursorTrail::Liquid,
                "at the second `ge`"
            );
        }
        // And the jump up lights a liquid: an ignition in `gg` or the hold
        // after it, which REIGNITE refuses within 334 ms of the last.
        let jump = |s: &Option<usize>| s.is_some_and(|s| s == up || s == up + 1);
        assert!(
            take.ignitions
                .iter()
                .any(|(s, t, _)| jump(s) && *t == CursorTrail::Liquid),
            "no liquid ignition at `gg`"
        );
    }
    // **The cursor blinks at every prompt the session holds at, baud's
    // shell's and the server's over ssh, and is steady while helix asks for
    // a steady block**: a prompt's hold draws the cursor both whole and
    // not, and helix's holds only whole -- helix sends `CSI 2 SP q` as it
    // draws, and its exit puts the shell's blinking cursor back
    // (`DECSCUSR` 0, and the alternate screen's end). Each hold is the one
    // after a step's prompt: the launch, `cat`, the photo, fastfetch's
    // in the container, the fireworks' there, and the clear's at the end.
    let hold_after = |want: Step| {
        let from = at(want)?;
        steps[from..]
            .iter()
            .position(|s| matches!(s, Step::Hold(_)))
            .map(|h| from + h)
    };
    let drawn = |i: usize, hidden: bool| {
        blinked
            .iter()
            .filter(|(s, h)| *s == Some(i) && *h == hidden)
            .count()
    };
    let prompts = [
        Some(1),
        hold_after(Step::Run("cat hello.txt")),
        hold_after(Step::Run(steps::ICAT)),
        hold_after(Step::Run("fastfetch")),
        hold_after(Step::Run(steps::FIREWORKS)),
        hold_after(Step::Run("clear")),
    ];
    for i in prompts.into_iter().flatten() {
        assert!(
            drawn(i, true) > 0 && drawn(i, false) > 0,
            "the cursor does not blink at the prompt held in step {i} {:?}: {} frames hidden, \
             {} whole",
            steps[i],
            drawn(i, true),
            drawn(i, false)
        );
    }
    if let (Some(open), Some(quit)) = (
        at(Step::Run("hx ~/.config/baud/config.toml")),
        at(Step::Line(":q")),
    ) {
        for i in (open + 1..quit).filter(|&i| matches!(steps[i], Step::Hold(_))) {
            assert_eq!(
                drawn(i, true),
                0,
                "helix asked for a steady cursor, and step {i} {:?} drew it blinking",
                steps[i]
            );
        }
    }
    // The photo lights FAX: first in its shot, kitten typed to the hold
    // after its prompt, before fastfetch's picture.
    let first = take.lit.first(Led::Fax).and_then(|(_, s)| s);
    let icat = at(Step::Run(steps::ICAT));
    assert!(
        icat.is_some_and(|icat| first.is_some_and(|StepAt(i)| (icat..=icat + 2).contains(&i))),
        "FAX first lit in step {first:?}, not in the photo's shot, from step {icat:?}"
    );
    assert_eq!(
        take.lit.dark(),
        Vec::<Led>::new(),
        "every light lights in the session"
    );
}

/// **The trail as step `i` began**: the last the film changed to in a step
/// before it, a change during `i` itself coming after its start.
fn trail_as(
    trails: &[(Option<usize>, crate::config::CursorTrail)],
    StepAt(i): StepAt,
) -> crate::config::CursorTrail {
    trails
        .iter()
        .take_while(|(s, _)| s.is_none_or(|s| s < i))
        .last()
        .map(|t| t.1)
        .expect("the first frame's trail")
}

/// **The shots the flash census counts**, by their frames: the trails'
/// (helix opened to the prompt after it), the photo's (kitten typed to
/// the `ssh` after it), the fireworks' (their request typed to the `exit`
/// after it) and the clear's (to the end), a `name first last` line each
/// after the film's `size W H` and `rate R`.
fn shots(steps: &[Step], frames: &[Option<usize>], (w, h): (u32, u32), period: Duration) -> String {
    let at = |want: Step| steps.iter().position(|s| *s == want);
    let ranges = [
        (
            "trails",
            at(Step::Run("hx ~/.config/baud/config.toml")),
            at(Step::Run("cat hello.txt")),
        ),
        (
            "photo",
            at(Step::Run(steps::ICAT)),
            at(Step::Run("ssh demo")),
        ),
        (
            "fireworks",
            at(Step::Run(steps::FIREWORKS)),
            at(Step::Run("exit")),
        ),
        ("clear", at(Step::Run("clear")), Some(steps.len())),
    ];
    let rate = Duration::from_secs(1).as_millis() / period.as_millis();
    let mut out = format!("size {w} {h}\nrate {rate}\n");
    for (name, from, to) in ranges {
        let (Some(from), Some(to)) = (from, to) else {
            continue;
        };
        let inside = |f: &Option<usize>| f.is_some_and(|i| (from..to).contains(&i));
        let first = frames.iter().position(inside);
        let last = frames.iter().rposition(inside);
        if let (Some(a), Some(b)) = (first, last) {
            out.push_str(&format!("{name} {a} {b}\n"));
        }
    }
    out
}

/// An encoder of every frame, lossless, into `to`.
fn lossless(
    ffmpeg: &std::ffi::OsString,
    (w, h): (u32, u32),
    period: Duration,
    to: &std::path::Path,
) -> std::process::Child {
    let rate = Duration::from_secs(1).as_millis() / period.as_millis();
    std::process::Command::new(ffmpeg)
        .args(["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "bgra"])
        .args(["-video_size", &format!("{w}x{h}")])
        .args(["-framerate", &rate.to_string(), "-i", "-"])
        .args(["-c:v", "ffv1", "-pix_fmt", "bgra"])
        .arg(to)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("ffmpeg")
}

/// A frame kept for a still: the step it was in, its pixels and its size.
type Shot = (Option<usize>, Vec<u32>, (u32, u32));

/// One frame of premultiplied `0xAARRGGBB`, opaque, as a PNG at `to`.
fn png(ffmpeg: &std::ffi::OsString, px: &[u32], (w, h): (u32, u32), to: &std::path::Path) {
    use std::io::Write as _;
    let mut e = std::process::Command::new(ffmpeg)
        .args(["-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", "bgra"])
        .args([
            "-video_size",
            &format!("{w}x{h}"),
            "-i",
            "-",
            "-frames:v",
            "1",
        ])
        .arg(to)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("ffmpeg");
    let bytes: Vec<u8> = px.iter().flat_map(|p| p.to_le_bytes()).collect();
    e.stdin
        .as_mut()
        .expect("ffmpeg's input")
        .write_all(&bytes)
        .expect("a frame to ffmpeg");
    drop(e.stdin.take());
    assert!(
        e.wait().expect("ffmpeg").success(),
        "ffmpeg failed on {}",
        to.display()
    );
}
