//! **The tour without a window, and the lights it lights**: the
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

/// How long a prompt, an echo and the container's prompt may take, and a
/// program's first picture.
const PROMPT_LIMIT: Duration = Duration::from_secs(15);
const ECHO_LIMIT: Duration = Duration::from_secs(3);
const PICTURE_LIMIT: Duration = Duration::from_secs(60);

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
/// once its echo has come back, Escape and Ctrl-d encoded by the core as
/// the program on screen asked (`Term::key`), as the window's key goes, a
/// prompt counted by `OSC 133 ; A` marks, baud's shell's and the
/// container's, and the wheel at the grid's centre. Command-C takes the
/// seat's selection, which must read as the step says, and Command-V sends
/// it as the app's paste does (`paste::bytes`), bracketed when the program
/// asked; a peek's Command is held over nothing here, as the film draws no
/// address bar, and is a hold.
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
    /// What the last Command-C copied.
    copied: Option<String>,
}

/// A step of the session, by its place in the list.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
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
            copied: None,
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
                (Step::Scene(_), Doing::Start) => None,
                (Step::Escape | Step::Ctrl(_) | Step::Arrow(_), Doing::Start) => {
                    let key = steps::key_event(step).expect("a step that presses a key");
                    let mut out = Vec::new();
                    crate::reads::input(u.term, |t| t.key(key), |b| out.extend_from_slice(b));
                    self.press(u, log, epoch, &out);
                    None
                }
                (Step::Copy(want), Doing::Start) => {
                    let got = u.seat.selected_text(u.term);
                    assert_eq!(
                        got.as_deref(),
                        Some(want),
                        "step {}: Command-C copied {got:?}, not `{want}`; the screen:\n{}",
                        self.at.0,
                        crate::programs::screen(u.term).join("\n")
                    );
                    self.copied = got;
                    None
                }
                (Step::Paste, Doing::Start) => {
                    let text = self.copied.clone().expect("a paste after a copy");
                    let form = crate::paste::form(
                        u.term,
                        || crate::readline::Quote::LNEXT,
                        crate::paste::Specials::default,
                    );
                    self.typed_from = BytePos::from_index(heard.len());
                    self.press(u, log, epoch, &crate::paste::bytes(&text, form));
                    None
                }
                (Step::Picture, Doing::Start) => Some(Doing::Waiting { since: now }),
                (Step::Picture, Doing::Waiting { since }) => {
                    if steps::pictured(heard, self.typed_from.index()) {
                        None
                    } else {
                        self.limit(now, since, PICTURE_LIMIT, u, "the program's first picture");
                        Some(Doing::Waiting { since })
                    }
                }
                (Step::Hold(ms) | Step::Peek(ms), Doing::Start) => Some(Doing::Holding {
                    until: now + Duration::from_millis(u64::from(ms)),
                }),
                (Step::Hold(_) | Step::Peek(_), Doing::Holding { until }) => {
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
                (Step::Hover(cell), Doing::Start) => {
                    let at = place(self.geometry, cell, steps::MIDDLE);
                    inputs.push_back((now, Input::Moved(at.0, at.1)));
                    let settle = Duration::from_millis(steps::WHEEL_SETTLE_MS);
                    Some(Doing::Holding {
                        until: now + settle,
                    })
                }
                (Step::Drag { from, to }, Doing::Start) => {
                    let g = self.geometry;
                    let (press, release) = steps::drag_ends();
                    let moves = super::Cue::Drag {
                        from: place(g, from, press),
                        to: place(g, to, release),
                    }
                    .inputs(now);
                    let end = moves.last().map_or(now, |m| m.0);
                    inputs.extend(moves);
                    Some(Doing::Holding { until: end })
                }
                (
                    Step::Wheel { .. } | Step::Hover(_) | Step::Drag { .. },
                    Doing::Holding { until },
                ) => {
                    if now >= until {
                        None
                    } else {
                        Some(Doing::Holding { until })
                    }
                }
                (Step::CmdClick(_), _) => panic!(
                    "step {}: a Command-click opens a link's window, which a session with \
                     no window has none of (`windowless` takes it out)",
                    self.at.0
                ),
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

/// **A point inside a cell of the grid**, `within` of the cell from its
/// top left (`steps::MIDDLE` its middle), in the frame's pixels.
fn place(g: Geometry, cell: steps::Cell, within: (f64, f64)) -> (f64, f64) {
    middle(
        g,
        (
            f64::from(cell.row) + within.1 - 0.5,
            f64::from(cell.col) + within.0 - 0.5,
        ),
    )
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

/// **`steps` as a session with no window plays them**: each Command-click
/// taken out with what is done in the window it opens -- the picture
/// waited for, the wheel turned and the `q` that closes it -- through that
/// `q` and the hold after it. A link's window is a new baud window running
/// the program the config names for the link, which a session played
/// without a window cannot open.
pub(crate) fn windowless(steps: &[Step]) -> Vec<Step> {
    let mut v = Vec::with_capacity(steps.len());
    let mut i = 0;
    while i < steps.len() {
        if !matches!(steps[i], Step::CmdClick(_)) {
            v.push(steps[i]);
            i += 1;
            continue;
        }
        let close = steps[i..]
            .iter()
            .position(|s| *s == Step::Keys("q"))
            .map(|q| i + q)
            .expect("a `q` that closes the link's window");
        i = match steps.get(close + 1) {
            Some(Step::Hold(_)) => close + 2,
            _ => close + 1,
        };
    }
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

/// **The demo's link rules read as baud reads them, each in a window**:
/// `examples/demo/links.toml`, which the demo writes into its config. Read
/// by baud's own config reader its keys are all known, a web link opens
/// `cha` with its pictures on in a window, and a file path
/// `~/bin/demo-open` in a window, a relative one such as a README's
/// `assets/readme/logo-dark.gif` among the paths read, beside the three
/// prefixes read by default. It is read against a home holding
/// `bin/demo-open` and a `PATH` holding `cha`, as the demo gives baud, and
/// not this machine's, and nothing is said; its sections are read again
/// by the rules' own reader against the same. The control: the text with a key the rules do not
/// have is refused whole, and says so. Perturbation: write `action =
/// "windows"` in the file and the https rule is refused.
#[test]
fn the_demos_link_rules_open_their_windows() {
    use crate::rules::{Act, PathAct, Section};
    use std::os::unix::fs::PermissionsExt as _;
    let text = include_str!("../../examples/demo/links.toml");
    let dir = std::env::temp_dir().join(format!("demo-links-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (home, bin) = (dir.join("home"), dir.join("path"));
    for exe in [home.join("bin/demo-open"), bin.join("cha")] {
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let env = crate::config::ConfigEnv {
        home: Some(home.clone()),
        path: Some(bin.clone().into_os_string()),
    };
    let mut c = crate::config::Config::defaults();
    let said = c.apply_in(text, &env);
    assert_eq!(said, Vec::<String>::new());
    let r = &c.link.rules;
    match r.rule_for("https").map(|(_, x)| x.act()) {
        Some(Act::Window(argv)) => {
            assert_eq!(argv.program(), "cha");
            assert_eq!(argv.written(), ["-o", "buffer.images=true", "{url}"]);
        }
        other => panic!("https opens {other:?}"),
    }
    match r.path_act() {
        PathAct::Window(argv) => {
            assert!(argv.program().ends_with("/bin/demo-open"), "{argv:?}");
            assert!(argv.written().iter().any(|e| e == "{path}"), "{argv:?}");
        }
        other => panic!("a path opens {other:?}"),
    }
    use crate::paths::Turned::{Off, On};
    let f = r.forms();
    assert_eq!(
        [f.absolute, f.home, f.dot, f.parent, f.bare],
        [On, On, On, Off, On],
        "`/`, `~/`, `./` and relative paths are read, `../` is not"
    );
    // The same sections against the home and the PATH the demo gives baud.
    let mut sections: Vec<Section> = Vec::new();
    for line in text.lines() {
        match line
            .strip_prefix("[link.")
            .and_then(|l| l.strip_suffix(']'))
        {
            Some(name) => sections.push(Section {
                name: name.to_string(),
                fields: Vec::new(),
            }),
            None => {
                let (k, v) = line.split_once(" = ").expect("a key and its value");
                let s = sections.last_mut().expect("a section");
                s.fields.push((k.to_string(), v.to_string()));
            }
        }
    }
    let (rules, said) = Section::read_all(&sections, Some(&home), Some(bin.as_os_str()));
    assert_eq!(said, Vec::<String>::new());
    assert!(matches!(
        rules.rule_for("https").map(|(_, x)| x.act()),
        Some(Act::Window(_))
    ));
    match rules.path_act() {
        PathAct::Window(argv) => assert_eq!(
            std::path::Path::new(argv.program()),
            home.join("bin/demo-open"),
            "~/ is the home given"
        ),
        other => panic!("a path opens {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&dir);
    let mut refused = crate::config::Config::defaults();
    let said =
        refused.apply(&text.replace("[link.https]\n", "[link.https]\nconfirm = \"always\"\n"));
    assert!(
        said.iter()
            .any(|s| s.contains("[link.https]") && s.contains("confirm")),
        "the control: {said:?}"
    );
    assert_eq!(refused.link.rules.rule_for("https"), None, "the control");
}

/// **A session with no window plays the tour without the link's window**:
/// the Command-click, the picture, the wheel and Chawan's `q` with its
/// hold are gone, the hover before them and helix's `:q` after them stay,
/// and nothing else moves. The control: btop's `q`, which closes no
/// window of a link, stays. Perturbation: cut through the next `Prompt`
/// rather than the `q` and helix's `:q` goes with it.
#[test]
fn a_windowless_session_skips_the_links_window() {
    let s = SESSION;
    let w = windowless(s);
    let click = s
        .iter()
        .position(|t| matches!(t, Step::CmdClick(_)))
        .unwrap();
    assert!(w.iter().all(|t| !matches!(t, Step::CmdClick(_))));
    assert!(w.iter().all(|t| !matches!(t, Step::Wheel { .. })));
    assert_eq!(w[..click], s[..click], "everything before stays");
    assert_eq!(
        w[click..click + 2],
        [Step::Line(":q"), Step::Prompt],
        "helix's `:q` follows"
    );
    // CmdClick, Picture, Hold, Wheel, Hold, `q` and its hold.
    assert_eq!(w.len(), s.len() - 7);
    assert!(w.contains(&Step::Keys("q")), "the control: btop's `q`");
}

/// **The tour lights every light**, played without a
/// window by `cargo run --example demo -- --headless DIR`, which makes the
/// scratch directory `BAUD_SESSION_ROOT` names, starts the container and
/// runs this: the session's steps (`SESSION`), the link's window taken out
/// ([`windowless`]), typed into baud's own login shell started as baud
/// starts it, with that directory's `env` as baud's environment and
/// `LOGNAME` and `USER` the account `BAUD_SESSION_ACCOUNT` names, the
/// config where baud finds it with that environment -- read again when the
/// finale's `sed` changes it, as baud reads it -- and its 80x24 grid.
/// Each light's first lit moment is printed with the step it came in, and
/// every one must light, CS from the first turn; when CS first went out,
/// if it did, is printed too (the module's doc says why it rarely can).
/// Every move of the README's scene, from `hx README.md` to `:q`, must
/// begin in the plasma trail, and each jump from `/character` to `gg` must
/// light a plasma in its step or the hold after it; Ctrl-d is no jump, as
/// it moves the page and the cursor together and leaves the cursor on its
/// row, so its three are only printed; each ignition the film's trail lit
/// is printed with its step. FAX must first light in fastfetch's shot, its
/// mark.
///
/// `BAUD_SESSION_WITHOUT` takes a command out ([`without`]), for a
/// perturbation: without `ssh demo` CD stays dark, and without `fastfetch`
/// FAX does not light in its shot, and the test fails.
/// `BAUD_SESSION_POINTS` draws at another size, written into the config
/// file first, and with `BAUD_SESSION_STILLS` and `BAUD_FILM_FFMPEG` the
/// last frame of every hold is written there as a PNG, through baud's CPU
/// renderer on the opaque page, the window's title strip composed above
/// it (`chrome.rs`): the stills of the session at the size it is filmed
/// at. With `BAUD_SESSION_FILM` every frame, composed so, goes into that
/// file, lossless, at `BAUD_SESSION_PERIOD_MS` a frame (40 without it),
/// and beside it `<file>.shots` names the frames of the trails' shot, the
/// card's, the fireworks' and the clear's, for the flash census
/// (`baud-bench/tests/session_flashes.rs`), and `<file>.steps` each step's
/// first frame.
#[test]
#[ignore = "a tool: `cargo run --example demo -- --headless DIR` runs it"]
fn the_readme_session_lights_every_light() {
    // No window: AppKit's main-thread marker is made only on the main
    // thread, and this test is never on it (`no_appkit.rs`).
    assert!(!crate::sys::on_main_thread(), "a test off the main thread");
    use crate::config::CursorTrail;
    use std::io::Write as _;
    let var = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("{k} is not set"));
    let root = std::path::PathBuf::from(var("BAUD_SESSION_ROOT"));
    let read = |name: &str| {
        std::fs::read_to_string(root.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
    };
    let mut env = env_of(&read("env"));
    // Who the shell runs as, as launchd tells a Dock-started baud: the demo
    // gives the account's name to this test alone and writes it nowhere, so
    // the shell's zsh does not set `LOGNAME` from `getlogin()`, which in a
    // test started under another login session reads that session's name,
    // and starship prints no user line.
    let account = var("BAUD_SESSION_ACCOUNT");
    env.extend(["LOGNAME", "USER"].map(|k| (k.into(), account.clone().into())));
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
        Ok(run) => without(&windowless(SESSION), &run),
        Err(_) => windowless(SESSION),
    };
    let period = std::env::var("BAUD_SESSION_PERIOD_MS")
        .map(|v| Duration::from_millis(v.parse().expect("BAUD_SESSION_PERIOD_MS")))
        .unwrap_or(Duration::from_millis(40));
    let dir = root.join("home/baud");
    // The link layer's dumps, under the stills (`tap.rs`).
    let links = std::env::var_os("BAUD_SESSION_LINKS").map(std::path::PathBuf::from);
    let f = super::Film {
        child: super::Child::Session {
            steps: &steps,
            dir: &dir,
            links: links.as_deref(),
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
        let s = strip.get_or_insert_with(|| {
            super::chrome::Strip::new(fr.size.0, f64::from(super::SCALE), crate::version::BUILD)
        });
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
    // **The link layer's three checks**, on what the tap saw (`tap.rs`),
    // and its dumps naming nothing of the machine: printed here, held after
    // the session's own names check, so that one failing stops neither.
    let links_failed = take.links.as_ref().map(|(report, files)| {
        let cfg = {
            let mut c = crate::config::Config::defaults();
            let env = crate::config::ConfigEnv {
                home: Some(root.join("home")),
                path: env_path(&f.env),
            };
            let said = c.apply_in(&config, &env);
            assert_eq!(said, Vec::<String>::new(), "the session's config");
            c
        };
        let mut failed = link_checks(report, &steps, &cfg, &f.env);
        for f in files {
            println!("links:   {}", f.display());
        }
        let names = super::machine_names(&[]);
        for file in files {
            let bytes = std::fs::read(file).expect("a dump");
            let named = super::named(&bytes, &names);
            if !named.is_empty() {
                failed.push(format!("{} names this machine: {named:?}", file.display()));
            }
        }
        for f in &failed {
            println!("links:   failed: {f}");
        }
        failed
    });
    // What the programs printed and what was sent them, for the demo's
    // search of what must never be filmed, written as it came; and searched
    // here for the machine's names as a film's are, each shell folder
    // report as the title shows its folder (`undrawn.rs`): the core draws
    // none of it, and the title never shows the host it names.
    let bytes = [&take.heard[..], &take.answers[..]].concat();
    std::fs::write(root.join("heard.log"), &bytes).expect("heard.log");
    let home = std::fs::canonicalize(root.join("home")).expect("the session's home");
    let shown = [
        &crate::undrawn::shown_bytes(
            &take.heard,
            Some(std::os::unix::ffi::OsStrExt::as_bytes(home.as_os_str())),
        )[..],
        &take.answers[..],
    ]
    .concat();
    let names = super::machine_names(&[]);
    assert_eq!(
        super::named(&shown, &names),
        Vec::<String>::new(),
        "the session names this machine"
    );
    if let Some(failed) = links_failed {
        assert_eq!(failed, Vec::<String>::new(), "the link checks");
    }
    // The README's scene, in the container's helix: the trail plasma at
    // every step of it, nothing changing it before the finale, and each
    // jump's own plasma, from the search to `gg`, in its step or the hold
    // after it; Ctrl-d is no jump, as it moves the page and the cursor
    // together and leaves the cursor on its row.
    let at = |want: Step| steps.iter().position(|s| *s == want);
    if let (Some(open), Some(quit)) = (at(Step::Run("hx README.md")), at(Step::Line(":q"))) {
        for i in (open..=quit).filter(|&i| !matches!(steps[i], Step::Hold(_))) {
            assert_eq!(
                trail_as(&take.trails, StepAt(i)),
                CursorTrail::Plasma,
                "at step {i} {:?}",
                steps[i]
            );
        }
    }
    if let (Some(search), Some(top)) = (at(Step::Line("/character")), at(Step::Keys("gg"))) {
        for i in (search..=top).filter(|&i| !matches!(steps[i], Step::Hold(_))) {
            let lit = take.ignitions.iter().any(|(s, t, _)| {
                s.is_some_and(|s| s == i || s == i + 1) && *t == CursorTrail::Plasma
            });
            println!("scene:   step {i} {:?}: plasma lit: {lit}", steps[i]);
            assert!(
                lit || matches!(steps[i], Step::Ctrl(_)),
                "no plasma ignition at step {i} {:?}",
                steps[i]
            );
        }
    }
    // **The cursor blinks at every prompt the session holds at, baud's
    // shell's and the server's over ssh, and is steady while helix asks for
    // a steady block**: a prompt's hold draws the cursor both whole and
    // not, and helix's holds only whole -- helix sends `CSI 2 SP q` as it
    // draws, and its exit puts the shell's blinking cursor back
    // (`DECSCUSR` 0, and the alternate screen's end). Each hold is the one
    // after a step's prompt: the launch, fastfetch's and `cat`'s in the
    // container, the fireworks' there, and the clear's and the closing
    // line's at the end.
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
        hold_after(Step::Prompt),
        hold_after(Step::Run("fastfetch")),
        hold_after(Step::Run("cat hello.txt")),
        hold_after(Step::Run(steps::FIREWORKS)),
        hold_after(Step::Run("clear")),
        hold_after(Step::Run(steps::CLOSING)),
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
    if let (Some(open), Some(quit)) = (at(Step::Run("hx README.md")), at(Step::Line(":q"))) {
        for i in (open + 1..quit).filter(|&i| matches!(steps[i], Step::Hold(_))) {
            assert_eq!(
                drawn(i, true),
                0,
                "helix asked for a steady cursor, and step {i} {:?} drew it blinking",
                steps[i]
            );
        }
    }
    // **The move down and the move up each light a plasma**: one from `4j`
    // to `4k`, and one from `4k` to the search, the trail plasma then
    // (`Show::ArrowTrail`). Not in the key's own step: the key goes over ssh
    // and comes back as helix's redraw during the hold after it. Both are
    // required in the tour, so a tour without them fails here.
    let down = at(Step::Keys(steps::DOWN));
    let up = at(Step::Keys(steps::UP));
    let search = at(Step::Line("/character"));
    for (from, to) in [(down, up), (up, search)] {
        let (Some(from), Some(to)) = (from, to) else {
            panic!(
                "the tour has no {} and {} before its search",
                steps::DOWN,
                steps::UP
            );
        };
        let lit = take.ignitions.iter().any(|(s, t, _)| {
            s.is_some_and(|s| (from..to).contains(&s)) && *t == CursorTrail::Plasma
        });
        assert!(
            lit,
            "no plasma ignition from step {from} {:?} to step {to} {:?}",
            steps[from], steps[to]
        );
    }
    // **The finale's `sed` turns the trail to the saucer before `clear`**,
    // by baud's reading of the file again, and `clear` lights it.
    if let Some(clear) = at(Step::Run("clear")) {
        assert_eq!(
            trail_as(&take.trails, StepAt(clear)),
            CursorTrail::Ufo,
            "at the finale's `clear`"
        );
        let flew = take.ignitions.iter().any(|(s, t, _)| {
            s.is_some_and(|s| (clear..clear + 3).contains(&s)) && *t == CursorTrail::Ufo
        });
        assert!(flew, "no saucer at the finale's `clear`");
    }
    // fastfetch's mark lights FAX: first in its shot, fastfetch typed to
    // the hold after its prompt. The Nix link's page opens in a window of
    // its own, which a session with no window does not open.
    let first = take.lit.first(Led::Fax).and_then(|(_, s)| s);
    let card = at(Step::Run("fastfetch"));
    assert!(
        card.is_some_and(|c| first.is_some_and(|StepAt(i)| (c..=c + 2).contains(&i))),
        "FAX first lit in step {first:?}, not in fastfetch's shot, from step {card:?}"
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
/// (helix opened to `cat hello.txt`), the card's (fastfetch typed to helix),
/// the fireworks' (their request typed to the `exit` after it) and the
/// clear's, the saucer's flight among it (to the end), a `name first last`
/// line each after the film's `size W H` and `rate R`.
fn shots(steps: &[Step], frames: &[Option<usize>], (w, h): (u32, u32), period: Duration) -> String {
    let at = |want: Step| steps.iter().position(|s| *s == want);
    let ranges = [
        (
            "trails",
            at(Step::Run("hx README.md")),
            at(Step::Run("cat hello.txt")),
        ),
        (
            "pictures",
            at(Step::Run("fastfetch")),
            at(Step::Run("hx README.md")),
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

/// **A cell lands in its place in the grid**: its middle, and its corners
/// at its edges. Perturbation: swap the row and the column in `place`.
#[test]
fn a_cell_is_in_its_place_in_the_grid() {
    use steps::Cell;
    let g = Geometry {
        pad: (4, 2),
        cell: (10.0, 20.0),
        grid: (80, 24),
        scale: 1.0,
    };
    assert_eq!(
        place(g, Cell { row: 0, col: 0 }, steps::MIDDLE),
        (9.0, 12.0)
    );
    assert_eq!(place(g, Cell { row: 0, col: 0 }, (0.0, 0.0)), (4.0, 2.0));
    assert_eq!(
        place(g, Cell { row: 23, col: 79 }, (1.0, 1.0)),
        (804.0, 482.0)
    );
    assert_eq!(
        place(g, Cell { row: 1, col: 3 }, steps::MIDDLE),
        (39.0, 32.0)
    );
}

/// **The demo's window around its grid is the app's**: the title strip's
/// height is `chrome::INSET` and the padding the config's default, which
/// the demo's live take places the pointer by (`steps::in_window`).
/// Perturbation: change `steps::TITLE_PT` and the first assertion fails.
#[test]
fn the_window_geometry_is_the_apps() {
    assert_eq!(steps::TITLE_PT, super::chrome::INSET);
    assert_eq!(
        steps::PAD_PT,
        f64::from(crate::config::Config::defaults().padding)
    );
}

/// The terminal programs the session runs that take the keyboard, and
/// what ends each: helix's `:q` line, btop's `q` key.
const TAKES_KEYS: [(&str, Step); 2] = [("hx", Step::Line(":q")), ("btop", Step::Keys("q"))];

/// **The command lines a session types, as the shell would read them**:
/// keys, a paste of what was copied and a line's own text, joined, at each
/// Return. Each one's first word, `#` for a comment. While a program of
/// [`TAKES_KEYS`] has the keyboard, its keys build no shell line, and each
/// line it is sent is read on its own, until its ending.
fn submitted(steps: &[Step]) -> Vec<String> {
    let (mut out, mut line, mut copied) = (Vec::new(), String::new(), "");
    let mut holder: Option<Step> = None;
    let first = |l: &str| {
        let w = l.split_whitespace().next().unwrap_or("");
        match w.starts_with('#') {
            true => "#".to_string(),
            false => w.to_string(),
        }
    };
    for s in steps {
        if let Some(end) = holder {
            if let Step::Line(t) = *s {
                out.push(first(t));
            }
            if *s == end {
                holder = None;
            }
            continue;
        }
        match *s {
            Step::Keys(t) => line.push_str(t),
            Step::Copy(t) => copied = t,
            Step::Paste => line.push_str(copied),
            Step::Run(t) | Step::Line(t) => {
                line.push_str(t);
                let w = first(&line);
                holder = TAKES_KEYS.iter().find(|(p, _)| *p == w).map(|(_, e)| *e);
                out.push(w);
                line.clear();
            }
            _ => {}
        }
    }
    out
}

/// What the headless session may type and submit: comments, ssh, the
/// container's terminal programs, helix's own commands, and the shell's
/// `printf`, `sed`, `clear` and `exit`. None of them opens a window.
const SUBMITTED_ALLOWED: [&str; 14] = [
    "#",
    "ssh",
    "fastfetch",
    "hx",
    "cat",
    "btop",
    "printf",
    "exit",
    "sed",
    "clear",
    ":q",
    "/character",
    "/Nix",
    "",
];

/// **The headless session starts no program that opens a window**: every
/// command line it submits, played as the headless run plays it
/// ([`windowless`], which takes every Command-click out with its window's
/// steps), begins with a word of [`SUBMITTED_ALLOWED`]. The control: an
/// `mpv` line, and an `open` spelled across keys and a paste, are caught.
#[test]
fn the_headless_session_submits_only_programs_that_open_no_window() {
    let steps = windowless(SESSION);
    assert!(!steps.iter().any(|s| matches!(s, Step::CmdClick(_))));
    let lines = submitted(&steps);
    assert!(
        lines.len() >= 20,
        "the control: the lines are read: {lines:?}"
    );
    for l in &lines {
        assert!(SUBMITTED_ALLOWED.contains(&l.as_str()), "{l:?} submitted");
    }
    let mpv = [Step::Run("mpv https://example.com/x.mp4")];
    assert_eq!(submitted(&mpv), ["mpv"], "the control: mpv is read");
    let spelled = [
        Step::Copy("en"),
        Step::Keys("op"),
        Step::Paste,
        Step::Line(" README.md"),
    ];
    assert_eq!(
        submitted(&spelled),
        ["open"],
        "the control: spelled across steps"
    );
}

/// **The two tests the headless run executes in baud hold themselves off
/// the main thread**, and so off every AppKit main-thread marker: libtest
/// runs a test on a thread of its own, with any `--test-threads`, and each
/// asserts it first thing. The control: this test is off it too.
#[test]
fn the_headless_tests_hold_off_the_main_thread() {
    assert!(
        !crate::sys::on_main_thread(),
        "the control: a test is off it"
    );
    let guard = r#"assert!(!crate::sys::on_main_thread(), "a test off the main thread");"#;
    let first = |src: &str, test: &str| {
        let body = src
            .split_once(&format!("fn {test}() {{"))
            .map(|(_, b)| b)
            .expect("the test");
        let code: Vec<&str> = body
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with("//") && !l.starts_with("#["))
            .take(1)
            .collect();
        code.first().copied().unwrap_or("").to_string()
    };
    assert_eq!(
        first(
            include_str!("session.rs"),
            "the_readme_session_lights_every_light"
        ),
        guard,
        "the session's first statement"
    );
    assert_eq!(
        first(
            include_str!("../programs.rs"),
            "chawan_draws_a_pages_picture_by_kittys_protocol"
        ),
        guard,
        "Chawan's first statement"
    );
}

/// **A block put on the main queue never runs in a test**: libtest's main
/// thread waits on its tests and drains no queue, so code baud dispatches
/// to the main queue -- where it would be on the main thread, with a
/// marker -- never runs in the headless run's tests. The control: a block
/// on a queue of its own runs.
#[test]
fn a_block_on_the_main_queue_never_runs_in_a_test() {
    let (tx, rx) = std::sync::mpsc::channel();
    let other = tx.clone();
    dispatch2::DispatchQueue::main().exec_async(move || {
        let _ = tx.send("main");
    });
    dispatch2::DispatchQueue::new("baud.no-appkit", None).exec_async(move || {
        let _ = other.send("own");
    });
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(10)),
        Ok("own"),
        "the control: a queue of its own runs"
    );
    assert!(
        rx.recv_timeout(Duration::from_millis(500)).is_err(),
        "the main queue's block ran"
    );
}

/// **Nothing in baud takes a main-thread marker by hand**: objc2 builds an
/// application or a window only with a marker, and a marker is made only on
/// the main thread, unless code forces one (`new_unchecked`), turns the
/// process into an application from C (`TransformProcessType`,
/// `NSApplicationLoad`), sends to the class without the safe API, or looks
/// a class up by name. No code in baud-app or baud-present-macos does any
/// of these, and every class it looks up by name is named in a C string
/// that is no application, window or panel: comments and literals aside,
/// a literal being source for another program, not a call. The controls:
/// a forced marker is caught and the same words in a string are not; a
/// lookup of a notification class passes, and one of the application
/// class, or of a name not written, is caught.
#[test]
fn nothing_takes_a_main_thread_marker_by_hand() {
    use crate::no_appkit::bare_code;
    const FORBIDDEN_APP: &str = crate::no_appkit::FORBIDDEN[1];
    let bypasses = [
        concat!("MainThreadMarker::new", "_unchecked"),
        concat!("TransformProcess", "Type"),
        concat!("class!(NSApp", "lication)"),
        concat!("msg_send![NSApp", "lication"),
        concat!("NSApplication", "Load"),
        concat!("objc_get", "Class"),
        concat!("objc_lookUp", "Class"),
        concat!("NSClassFrom", "String"),
        concat!("objc_msg", "Send"),
    ];
    // A class looked up by name is named in a literal, which `bare_code`
    // blanks: each lookup's name is read from the code with literals kept,
    // and must be a C string that is no application, window or panel.
    let lookups = [concat!("AnyClass::", "get("), "new_of("];
    let windowing = ["NSApp", "NSWindow", "NSPanel"];
    // `new_of`'s own lookup, by its argument: its callers are lookups too.
    let by_argument = concat!("let cls = AnyClass::", "get(name)?;");
    let caught = |text: &str| -> Vec<String> {
        let bare = bare_code(text);
        let kept = crate::no_appkit::code(text);
        let mut out: Vec<String> = bare
            .lines()
            .filter(|l| bypasses.iter().any(|b| l.contains(b)))
            .map(|l| l.trim().to_string())
            .collect();
        for (b, k) in bare.lines().zip(kept.lines()) {
            for l in lookups {
                if !b.contains(l) || b.contains(&format!("fn {l}")) || k.trim() == by_argument {
                    continue;
                }
                let named = k
                    .split_once(l)
                    .and_then(|(_, r)| r.strip_prefix("c\""))
                    .and_then(|r| r.split_once('"'))
                    .map(|(n, _)| n);
                match named {
                    Some(n) if !windowing.iter().any(|w| n.starts_with(w)) => {}
                    _ => out.push(k.trim().to_string()),
                }
            }
        }
        out
    };
    let forced = format!("let m = unsafe {{ {}() }};\n", bypasses[0]);
    assert_eq!(caught(&forced).len(), 1, "the control: code is caught");
    assert!(
        caught(&format!("let s = {forced:?};\n")).is_empty(),
        "a string is not"
    );
    let lookup = |n: &str| format!("let c = {}c\"{n}\");\n", lookups[0]);
    assert!(
        caught(&lookup("UNNotificationSound")).is_empty(),
        "the control: a notification class passes"
    );
    assert_eq!(
        caught(&lookup(FORBIDDEN_APP)).len(),
        1,
        "the control: the application class is caught"
    );
    assert_eq!(
        caught(&format!("let c = {}x);\n", lookups[0])).len(),
        1,
        "a name not written is caught"
    );
    let mut found = Vec::new();
    for root in ["src", "../baud-present-macos/src"] {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(root);
        let mut todo = vec![dir];
        while let Some(d) = todo.pop() {
            for e in std::fs::read_dir(&d).expect("a source folder") {
                let p = e.expect("an entry").path();
                if p.is_dir() {
                    todo.push(p);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    let text = std::fs::read_to_string(&p).expect("a source file");
                    for l in caught(&text) {
                        found.push(format!("{}: {l}", p.display()));
                    }
                }
            }
        }
    }
    assert_eq!(found, Vec::<String>::new(), "a marker taken by hand");
}

/// The `PATH` in a film's environment, as its config reads a command.
fn env_path(env: &[(std::ffi::OsString, std::ffi::OsString)]) -> Option<std::ffi::OsString> {
    env.iter()
        .find(|(k, _)| k == "PATH")
        .map(|(_, v)| v.clone())
}

/// The URL the storyline's link names: scene 9, "The Nix link opens
/// nixos.org in Chawan (F1, on the Mac)", hovered at [`steps::NIX_AT`]
/// and found by the tap on its screen by this text.
pub(super) const NIX_URL: &str = "https://nixos.org";

/// **The link layer's three checks on what the tap saw**, each from the
/// storyline (the designer's `commands.md`); the failures, in words.
///
/// 1. Scene 9's link lights the frame it shows, as a URL waits on no
///    answer of the worker's, and a Command-click on it, at the
///    `Hover(`[`steps::NIX_AT`]`)` step, takes a
///    window running the argv the session's `[link.https]` rule builds for
///    it on the session's `PATH`.
/// 2. No row goes darker while its text stays and it stays on the normal
///    screen: what a typed prompt line going dark after a quick command is.
/// 3. "Its helix runs over ssh, where no path is looked up": no printed path
///    lit on a row written while `ssh demo` ran, below the line it was
///    typed on, nor on helix's screen there, and no such row is taken as
///    written on this machine.
fn link_checks(
    report: &super::tap::Report,
    steps: &[Step],
    cfg: &crate::config::Config,
    env: &[(std::ffi::OsString, std::ffi::OsString)],
) -> Vec<String> {
    use super::tap::Opens;
    let mut failed = Vec::new();
    // 1. The rule's argv for the link, its program found on the session's
    // `PATH` as a window finds it.
    let rules = &cfg.link.rules;
    let want: Option<Vec<String>> = rules.rule_for("https").and_then(|(ix, _)| {
        rules
            .window(ix, NIX_URL)
            .and_then(Result::ok)
            .map(|run| super::tap::window_argv(&run, env))
    });
    let hover = steps.iter().position(|s| *s == Step::Hover(steps::NIX_AT));
    match (hover, &want) {
        (Some(h), Some(want)) => match report.hovered.iter().find(|(s, _)| *s == StepAt(h)) {
            Some((_, Opens::Window(argv))) if argv == want => {}
            other => failed.push(format!(
                "1: a Command-click at the Nix link takes {other:?}, not a window running {want:?}"
            )),
        },
        _ => failed.push(format!(
            "1: no hover over the Nix link ({hover:?}) or no window rule for it ({want:?})"
        )),
    }
    if let Some(argv) = &want {
        if !argv.first().is_some_and(|p| p.starts_with('/')) {
            failed.push(format!("1: the rule's program is not absolute: {argv:?}"));
        }
    }
    // A URL lights while its program is unknown, and an answer can only
    // strike it: it waits on no answer, so it lights the frame it shows.
    match report.first_lit(NIX_URL) {
        Some((shown, Some(lit))) if lit == shown => {}
        Some((shown, Some(lit))) => failed.push(format!(
            "1: the Nix link showed at frame {shown:?} and lit at {lit:?}: a URL waits on no \
             answer and lights the frame it shows"
        )),
        other => failed.push(format!("1: the Nix link never lit: {other:?}")),
    }
    // 2.
    for d in &report.darkened {
        failed.push(format!(
            "2: {:?} step {:?}: {:?} went from {:?} lit cells to {:?}, its text unchanged: |{}|",
            d.frame, d.step, d.line, d.was, d.now, d.text
        ));
    }
    // 3.
    let at = |want: Step| steps.iter().position(|s| *s == want);
    match (at(Step::Run("ssh demo")), at(Step::Run("exit"))) {
        (Some(ssh), Some(exit)) => {
            let spans: Vec<_> = (ssh..exit)
                .filter_map(|s| report.written.get(&StepAt(s)).copied())
                .collect();
            let hi = spans.iter().map(|s| s.1).max();
            // The rows below the line `ssh demo` was typed on: the line
            // itself, and any above it the step's span reaches, are this
            // machine's prompt.
            let typed = report
                .snapshots
                .iter()
                .filter(|s| {
                    (ssh..exit).contains(&s.step.0) && s.screen == baud_core::Screen::Normal
                })
                .flat_map(|s| &s.rows)
                .find(|r| r.text.trim_end().ends_with("ssh demo"))
                .map(|r| r.line);
            if typed.is_none() {
                failed.push("3: the line `ssh demo` was typed on is on no screen read".to_string());
            }
            let lo = spans
                .iter()
                .map(|s| s.0)
                .min()
                .max(typed.map(baud_render::AbsLine::below));
            let mut rows = 0;
            for snap in report
                .snapshots
                .iter()
                .filter(|s| (ssh..exit).contains(&s.step.0))
            {
                for r in &snap.rows {
                    let over_ssh = match snap.screen {
                        baud_core::Screen::Alternate => true,
                        baud_core::Screen::Normal => {
                            typed.is_some()
                                && lo
                                    .zip(hi)
                                    .is_some_and(|(lo, hi)| lo <= r.line && r.line <= hi)
                        }
                    };
                    if !over_ssh || r.text.is_empty() {
                        continue;
                    }
                    rows += 1;
                    for (from, to, opens) in &r.lit {
                        if let Opens::Path { .. } = opens {
                            failed.push(format!(
                                "3: {:?} {:?}: a path lit at {}..{} on a row written over \
                                 ssh: |{}|",
                                snap.step,
                                r.line,
                                from.index(),
                                to.index(),
                                r.text
                            ));
                        }
                    }
                    if snap.screen == baud_core::Screen::Normal
                        && r.origin == crate::paths::Origin::Local
                    {
                        failed.push(format!(
                            "3: {:?} {:?} written over ssh is taken as this machine's: |{}|",
                            snap.step, r.line, r.text
                        ));
                    }
                }
            }
            if rows == 0 {
                failed.push("3: the control: no row written over ssh was read".to_string());
            }
        }
        other => failed.push(format!("3: no ssh in the session: {other:?}")),
    }
    failed
}

/// The demo's link rules read against a home holding `bin/demo-open` and a
/// `PATH` holding `cha`, in a folder of the test's own for the caller to
/// remove; and the environment naming that `PATH`.
fn demo_link_config(
    tag: &str,
) -> (
    crate::config::Config,
    std::path::PathBuf,
    Vec<(std::ffi::OsString, std::ffi::OsString)>,
) {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = std::env::temp_dir().join(format!("demo-links-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (home, bin) = (dir.join("home"), dir.join("path"));
    for exe in [home.join("bin/demo-open"), bin.join("cha")] {
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let vars = vec![("PATH".into(), bin.clone().into_os_string())];
    let env = crate::config::ConfigEnv {
        home: Some(home),
        path: Some(bin.into_os_string()),
    };
    let mut c = crate::config::Config::defaults();
    let said = c.apply_in(include_str!("../../examples/demo/links.toml"), &env);
    assert_eq!(said, Vec::<String>::new());
    (c, dir, vars)
}

/// **The link checks fail on what each guards**, on a report the tap could
/// have made: one that holds all three passes; a Command-click at the Nix
/// link that opens anything but the rule's window, or a link lit a frame
/// after it showed, an answer merged between the two among them, fails
/// check 1; a row gone dark
/// with its text unchanged fails check 2; a path lit on a row written over
/// ssh, or such a row taken as this machine's, or no line `ssh demo` was
/// typed on, fails check 3. The controls: the passing report reads a row
/// written over ssh, and its step's span reaches this machine's prompt
/// rows, a path lit on one, above and on the line `ssh demo` was typed on.
#[test]
fn the_link_checks_catch_what_each_guards() {
    use super::tap::{Cells, Change, FrameNo, Opens, Report, RowDump, Snapshot};
    use baud_domain::{Col, Rows};
    use baud_render::AbsLine;
    let (cfg, dir, env) = demo_link_config("checks");
    let steps = windowless(SESSION);
    let at = |s: Step| steps.iter().position(|t| *t == s).expect("the step");
    let (hover, ssh) = (at(Step::Hover(steps::NIX_AT)), at(Step::Run("ssh demo")));
    let rules = &cfg.link.rules;
    let (ix, _) = rules.rule_for("https").expect("the https rule");
    let run = rules.window(ix, NIX_URL).expect("a window").expect("built");
    let want = super::tap::window_argv(&run, &env);
    assert!(
        want[0].starts_with('/') && want[0].ends_with("/cha"),
        "{want:?}"
    );
    let line = |n: usize| AbsLine::at(Rows::of_count(n));
    let row = |n: usize, text: &str| RowDump {
        line: line(n),
        text: text.to_string(),
        lit: Vec::new(),
        struck: Vec::new(),
        origin: crate::paths::Origin::Remote,
    };
    let mut good = Report::default();
    good.hovered
        .push((StepAt(hover), Opens::Window(want.clone())));
    good.seen.insert(
        format!("29 [Nix]({NIX_URL}) (with flakes)"),
        (FrameNo(10), Some(FrameNo(10))),
    );
    good.answered.push(FrameNo(4));
    // The step's span reaches the prompt above the line `ssh demo` was
    // typed on, as a take's does: those rows are this machine's.
    good.written.insert(StepAt(ssh), (line(0), line(9)));
    let local = |n: usize, text: &str| RowDump {
        origin: crate::paths::Origin::Local,
        ..row(n, text)
    };
    let mut prompt = local(0, "~/baud ❯ ls README.md");
    prompt.lit.push((
        Col::at_index(12),
        Col::at_index(20),
        Opens::Path {
            text: "README.md".to_string(),
            found: "Exists".to_string(),
        },
    ));
    good.snapshots.push(Snapshot {
        step: StepAt(ssh),
        screen: baud_core::Screen::Normal,
        rows: vec![
            prompt,
            local(1, "~/baud ❯ ssh demo"),
            row(6, "~ ❯ cat hello.txt"),
        ],
    });
    assert_eq!(
        link_checks(&good, &steps, &cfg, &env),
        Vec::<String>::new(),
        "the control"
    );
    let fails = |r: &Report, check: &str| {
        let f = link_checks(r, &steps, &cfg, &env);
        assert!(
            !f.is_empty() && f.iter().all(|s| s.starts_with(check)),
            "{check} {f:?}"
        );
    };
    let mut r = good.clone();
    r.hovered[0].1 = Opens::Other("Open Https".to_string());
    fails(&r, "1:");
    let mut r = good.clone();
    r.seen.values_mut().for_each(|v| v.1 = Some(FrameNo(11)));
    fails(&r, "1:");
    // An answer merged after the link showed is no reason for it to wait.
    r.answered.push(FrameNo(12));
    fails(&r, "1:");
    let mut r = good.clone();
    r.darkened.push(Change {
        frame: FrameNo(30),
        step: Some(StepAt(ssh)),
        at: Duration::from_secs(3),
        line: line(1),
        text: "~/baud ❯ sed -i '' s/a/b/ ~/.config/baud/config.toml".to_string(),
        was: Cells(31),
        now: Cells(0),
        screen: baud_core::Screen::Normal,
    });
    fails(&r, "2:");
    let mut r = good.clone();
    r.snapshots[0].rows[2].lit.push((
        Col::at_index(9),
        Col::at_index(17),
        Opens::Path {
            text: "hello.txt".to_string(),
            found: "Exists".to_string(),
        },
    ));
    fails(&r, "3:");
    let mut r = good.clone();
    r.snapshots[0].rows[2].origin = crate::paths::Origin::Local;
    fails(&r, "3:");
    let mut r = good;
    r.snapshots[0].rows.remove(1);
    fails(&r, "3:");
    let _ = std::fs::remove_dir_all(&dir);
}
