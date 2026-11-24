//! **baud filmed without a window**: a demo runs in a pty against the real
//! core, and its frames are drawn by the CPU renderer -- the Linux driver's
//! path, which `baud-present-macos/tests/cells.rs` holds to the GPU arms'
//! pixels for the trails' effects -- through the Linux driver's own timing
//! (`timed.rs`): the launch splash, the cursor trail, the scrollbar's thumb,
//! pictures, held synchronised updates. What `scripts/readme-film.sh` makes
//! the README's clip with; nothing opens a window, records the screen or
//! drives the pointer of the desktop.
//!
//! **Two passes, so every frame is sampled on the wall clock.** The take
//! runs the child in real time: each read is fed as the drivers feed it,
//! every answer the core owes goes back to the child, a kitty picture is
//! decoded a step a turn, and the pointer the demo asks for goes through
//! the seat (`Seat::input`, as `programs.rs` drives it). Each of those is
//! logged with the instant it happened. The film then plays the log into a
//! second terminal set up the same way and draws frame `k` at `k` frame
//! periods after the launch, from exactly what had arrived by then, however
//! long a frame takes to draw. The second terminal's answers must equal the
//! first's byte for byte, and its screen and cursor at the end the first's,
//! which is the check that the film shows the screen the child talked to.
//!
//! **The window it stands for** is the macOS app's on a Retina display:
//! twice the points in pixels, the default 8 pt padding around the grid,
//! the dark theme, the cursor steady (`[cursor] blink = false`, so the
//! first and last frames can match) and motion full. **The page is opaque**
//! -- `[glass] dim_dark = 1.0`, the dark theme's own colour -- because
//! glass shows the desktop behind the window, which cannot be filmed
//! without one. A film draws no title bar -- the README session composes
//! one over its frames (`chrome.rs`) -- and no pointer, and output scrolls
//! in jumps: the smooth-scroll slide is the cell path's alone, and this is
//! the CPU path.
//!
//! **A font the stack lacks is found as the drivers find it**
//! (`host::follow_gaps`), and the frame that met the gap is drawn again on
//! the new stack before it is kept: the app shows the boxes for one display
//! refresh, which a film at 25 frames a second would hold for 40 ms.
//!
//! **A config read from a file is read again when the file changes**, as
//! baud's reload reads it (`reload.rs`): the take reads the file at every
//! turn, logs its text a settle after the last change (`reload::SETTLE`),
//! and the film applies the parts that changed as the Linux driver applies
//! them (`live::between`) -- the trail and the screen transition, the only
//! parts a film draws differently; any other part fails the film rather
//! than going unshown. An empty file is no change, since a save can
//! truncate the file before it writes it, where baud's reload reads it as
//! the defaults: the film departs from it there. Its relative paths are the
//! file's, as baud reads them (`Config::beside`). The take watches by
//! reading, not through the kernel's watch: every turn rather than on an
//! event, which hears the same change at most a turn later.
//!
//! **The demo asks for the pointer through a file**, a line a gesture, named
//! by `BAUD_FILM_CUES` in its environment: `wheel ROW COL STEPS` (detents,
//! negative up the history), `drag ROW COL ROW COL`, `click ROW COL`, and
//! `done`, which ends the take. Rows and columns count from 0.
//!
//! **Nothing of the machine is in what the child wrote or was sent**: the
//! child runs with its home, configuration and temporary files in a
//! directory of its own (`programs::Scratch`), and everything it wrote,
//! and everything written to it, is searched for the user's name, the
//! host's, the home directory and the temporary directories ([`named`]);
//! a take that finds one fails. Each frame carries its text as drawn
//! ([`Frame::text`]), which the README session searches frame by frame
//! (`session.rs`); the pixels are not searched.

// The README session played on baud's login shell, and the lights it
// lights: macOS's, where OH and CD are read.
#[cfg(target_os = "macos")]
pub(crate) mod session;
// The window's title strip, composed over the session's frames.
#[cfg(target_os = "macos")]
pub(crate) mod chrome;
// The titled window's bar the strip is laid out by, under the SDK the app
// links; the window test reads the same file.
#[cfg(target_os = "macos")]
mod titled;
// The link layer on the take, written down for the README session.
#[cfg(target_os = "macos")]
pub(crate) mod tap;
// mpv's kitty video, replayed from its capture and held to what it sent.
mod mpv;

use crate::gesture::Pressed;
use crate::programs::{Heard, Scratch, Under};
use crate::seat::{Geometry, Input, Seat};
use baud_core::{Focus, Modifiers, Owned, Term};
use baud_domain::{DecodeProgress, EventMillis, WinSize};
use baud_font::Px;
use baud_render::{Damage, PxSize, Renderer, Target};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// The window's backing scale: a Retina display's two pixels a point.
const SCALE: f32 = 2.0;

/// The config's default 8 pt padding around the grid, in pixels at
/// [`SCALE`].
const PAD: Px = Px::of(16);

/// What every film is set to before its own lines: the macOS window this
/// stands for, as the module's doc says.
const BASE: &str = "theme = \"dark\"\n[glass]\ndim_dark = 1.0\n[cursor]\nblink = false\n";

/// How long the take waits on the child before it looks at the clock again:
/// the latest a gesture's step or a decode's step can be taken late.
const POLL: Duration = Duration::from_millis(5);

/// The longest a synchronised update holds the frame, as both drivers hold
/// it (`SYNC_MAX` in each).
const SYNC_MAX: Duration = Duration::from_millis(33);

/// **What a take runs on the pty.**
pub(crate) enum Child<'a> {
    /// A command under `/bin/sh -c`, given the variables of a scratch
    /// directory of its own beside the film's; its `done` cue ends it.
    Command(&'a str),
    /// **The README session**: baud's login shell started in `dir` as baud
    /// starts a window's, the film's variables as baud's environment, and
    /// the session's steps typed into it (`session::Player`); its last
    /// step ends it.
    #[cfg(target_os = "macos")]
    Session {
        steps: &'a [session::Step],
        dir: &'a Path,
        /// Where the link layer's dumps go (`tap.rs`), or none for no tap.
        links: Option<&'a Path>,
    },
}

/// **A film to make**: what runs on the pty, the variables it is given,
/// the grid, the config lines laid over [`BASE`] and the file they were
/// read from, if any, the frame period, and how long the take may wait for
/// its end.
pub(crate) struct Film<'a> {
    pub(crate) child: Child<'a>,
    pub(crate) env: Vec<(OsString, OsString)>,
    pub(crate) grid: WinSize,
    pub(crate) config: &'a str,
    /// The file `config` was read from: its relative paths are beside it,
    /// and a change to it is read again and applied (the module's doc).
    pub(crate) source: Option<&'a Path>,
    pub(crate) period: Duration,
    pub(crate) limit: Duration,
}

/// One frame, drawn.
pub(crate) struct Frame<'p> {
    /// Premultiplied `0xAARRGGBB`, flattened over the page, so every pixel
    /// is opaque.
    pub(crate) pixels: &'p [u32],
    pub(crate) size: (Px, Px),
    /// Its instant, from the launch.
    pub(crate) at: Duration,
    /// The session's step that had begun by then, by its index; none
    /// before the first and in a film of a command.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) step: Option<usize>,
    /// What the activity lights showed then, as the take's clocks had them.
    #[cfg(target_os = "macos")]
    pub(crate) lights: baud_render::Shown,
    /// The cursor's blink phase it was drawn in: [`CursorPhase::SHOWN`]
    /// for a cursor that does not blink, or one in its shown phase.
    ///
    /// [`CursorPhase::SHOWN`]: baud_render::CursorPhase::SHOWN
    pub(crate) caret: baud_render::CursorPhase,
    /// **The frame's text, as drawn**: the screen's rows and the status
    /// line's (`programs::shown`), then the badge's text when one is
    /// drawn, a line each -- of the frame shown, so a frame a synchronised
    /// update holds carries the held frame's. What the README session's
    /// search for this machine's names reads, frame by frame, rather than
    /// the pixels.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) text: &'p [String],
}

/// **A frame's text**: the screen's and the status line's rows, then the
/// badge's text, if `badge` is drawn.
fn frame_text(term: &Term<'_>, badge: Option<&baud_render::BadgeLook>) -> Vec<String> {
    let mut text = crate::programs::shown(term);
    text.extend(badge.map(|b| b.text.clone()));
    text
}

/// What a take gave besides its frames.
pub(crate) struct Take {
    /// The trail the film drew, from its first frame and at each change,
    /// with the session's step the frame was in.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) trails: Vec<(Option<usize>, crate::config::CursorTrail)>,
    /// The frames an effect's trail lit an ignition in (`Timed::next_seed`
    /// moved): the session's step the frame was in, the trail and the
    /// frame's instant.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(crate) ignitions: Vec<(Option<usize>, crate::config::CursorTrail, Duration)>,
    /// Everything the child wrote, in order.
    pub(crate) heard: Vec<u8>,
    /// Everything written to the child, which the film's terminal answered
    /// the same.
    pub(crate) answers: Vec<u8>,
    /// The frames drawn.
    pub(crate) frames: Frames,
    /// The cell, in pixels.
    pub(crate) cell: (Px, Px),
    /// What the activity lights showed through the take.
    #[cfg(target_os = "macos")]
    pub(crate) lit: session::Lit,
    /// What the link layer showed through the take, and the dumps it wrote,
    /// when the session asked for them (`tap.rs`).
    #[cfg(target_os = "macos")]
    pub(crate) links: Option<(tap::Report, Vec<PathBuf>)>,
}

/// How many frames a film drew.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Frames(usize);

/// What the take logged, and when.
enum Event {
    /// A read from the child, fed.
    Read(Vec<u8>),
    /// One step of a picture's decode.
    Decode,
    /// The pointer, through the seat.
    Input(Input),
    /// Keys typed: their bytes, written to the child.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Typed(Vec<u8>),
    /// The session's step by its index, begun.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Step(usize),
    /// The config file read again, its text, a settle after it changed.
    Reload(String),
}

struct Logged {
    at: Duration,
    event: Event,
}

/// **The config, the fonts and the renderer**, made once: the take and the
/// film each set a terminal up from them.
struct Stage {
    cfg: crate::config::Config,
    /// The config as read, before its family names are resolved: what a
    /// reload is compared with (`live::between`).
    read: crate::config::Config,
    fonts: crate::fonts::Fonts,
    renderer: Renderer<'static, 'static>,
    px: PxSize,
}

impl Stage {
    fn new(f: &Film<'_>) -> Stage {
        let mut cfg = configured(f.config, f.source, &f.env);
        let read = cfg.clone();
        crate::host::resolved(&mut cfg);
        let (data, mut held) = crate::host::map_faces(&cfg);
        let faces = crate::host::parse_faces(&cfg, &data, &mut held);
        let fonts = crate::fonts::Fonts::new(&cfg, data, faces, held);
        let halves = crate::palette::Halves {
            selection: cfg.selection,
            cursor: cfg.cursor,
            ..crate::palette::Halves::default()
        };
        let theme =
            crate::palette::half_theme(crate::palette::Half::Dark, &cfg.glass, cfg.floor, &halves);
        let px = PxSize::of(cfg.font_size * SCALE).expect("a font size");
        let mut renderer = Renderer::new(
            crate::host::stack(&fonts),
            px,
            theme,
            baud_render::Blend::Native,
            baud_domain::Cols::of_index(usize::from(f.grid.ws_col)),
            baud_domain::Rows::of_index(usize::from(f.grid.ws_row)),
        )
        .expect("cell metrics");
        crate::host::set_up_renderer(&mut renderer, &cfg, &crate::live::Notice::of(&[]));
        renderer.set_cursor_look(crate::caret::look(Focus::Gained, cfg.unfocused_cursor));
        Stage {
            cfg,
            read,
            fonts,
            renderer,
            px,
        }
    }

    /// **A terminal as the Linux driver sets one up**, at [`SCALE`]: what
    /// `OSC 1337 ; ReportCellSize` divides by.
    fn term<'s>(&self, term: Term<'s>) -> Term<'s> {
        let mut term = crate::host::set_up_term(term, &self.cfg, &self.renderer, &self.fonts);
        term.set_kitty_files(Box::leak(Box::new(crate::kitty_files::LocalFiles::new())));
        term.set_regis_font(crate::regis_font::glyph);
        crate::reads::hold_replies(&mut term);
        term.set_backing_scale(baud_domain::BackingScale::of(f64::from(SCALE)));
        // The window has the keyboard, as a window just launched does.
        crate::reads::input(&mut term, |t| t.focus(Focus::Gained), |_| {});
        term
    }

    /// Where the grid starts in the frame, and its cells: what a point is
    /// mapped through.
    fn geometry(&self, grid: &WinSize) -> Geometry {
        let m = self.renderer.metrics();
        Geometry {
            pad: (Px::of(PAD.value() + self.renderer.grid_left().value()), PAD),
            cell: (f64::from(m.cell_w.value()), f64::from(m.cell_h.value())),
            grid: crate::width::Grid {
                cols: grid.ws_col,
                rows: grid.ws_row,
            },
            scale: 1.0,
        }
    }
}

/// **A film's config**: [`BASE`], then `text` over it, its relative paths
/// beside `source` when it was read from a file. It is read under the
/// `HOME` and `PATH` of `env`, the film's baud's, as that baud would read
/// it, and this process's where `env` names neither: a session's `~/`
/// program is in the session's home, and its bare names on its `PATH`.
fn configured(
    text: &str,
    source: Option<&Path>,
    env: &[(OsString, OsString)],
) -> crate::config::Config {
    let under = crate::config::ConfigEnv::of(|k| {
        env.iter()
            .rev()
            .find(|(n, _)| n == k)
            .map(|(_, v)| v.clone())
            .or_else(|| std::env::var_os(k))
    });
    let mut cfg = crate::config::Config::defaults();
    // Two reads, so the film's lines start outside BASE's last table.
    let mut said = cfg.apply_in(BASE, &under);
    said.extend(cfg.apply_in(text, &under));
    assert!(said.is_empty(), "the film's config: {said:?}");
    if let Some(p) = source {
        cfg.beside(p);
    }
    cfg
}

/// **The config file, read as baud's reload reads it**: its bytes at every
/// turn, and its text once they have stood a settle unchanged and differ
/// from the last text given.
struct Watch<'a> {
    path: &'a Path,
    given: Vec<u8>,
    seen: Vec<u8>,
    since: Option<Duration>,
}

impl<'a> Watch<'a> {
    fn new(path: &'a Path) -> Watch<'a> {
        let now = std::fs::read(path).unwrap_or_default();
        Watch {
            path,
            given: now.clone(),
            seen: now,
            since: None,
        }
    }

    /// The file's text at `now`, when it has changed and settled. An empty
    /// file is no change: a save can truncate the file before it writes
    /// it, and one headless run of the README session failed on helix's
    /// save as a change of the fonts first, as the file read empty for a
    /// settle gives it -- a config of no lines. baud's reload reads that
    /// empty file as the defaults and the write after it as a second
    /// change (`reload.rs`); the film, which cannot draw the fonts' change
    /// between, departs from it there.
    fn turn(&mut self, now: Duration) -> Option<String> {
        if let Ok(b) = std::fs::read(self.path) {
            if b != self.seen && !b.is_empty() {
                self.seen = b;
                self.since = Some(now);
            }
        }
        let settled = self.since? + crate::reload::SETTLE <= now;
        if !settled {
            return None;
        }
        self.since = None;
        if self.seen == self.given {
            return None;
        }
        self.given.clone_from(&self.seen);
        Some(String::from_utf8_lossy(&self.given).into_owned())
    }
}

/// **A demo's gesture**, read off its cue file.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Cue {
    Wheel { at: (f64, f64), steps: f64 },
    Drag { from: (f64, f64), to: (f64, f64) },
    Click { at: (f64, f64) },
    Done,
}

/// The middle of the cell at row `r` and column `c`, in the frame's
/// pixels.
fn middle(g: Geometry, (r, c): (f64, f64)) -> (f64, f64) {
    (
        f64::from(g.pad.0.value()) + (c + 0.5) * g.cell.0,
        f64::from(g.pad.1.value()) + (r + 0.5) * g.cell.1,
    )
}

impl Cue {
    /// One line of the cue file, or why it is not one.
    fn parse(line: &str, g: Geometry) -> Result<Cue, String> {
        let words: Vec<&str> = line.split_whitespace().collect();
        let n = |i: usize| -> Result<f64, String> {
            words
                .get(i)
                .and_then(|w| w.parse::<f64>().ok())
                .ok_or_else(|| format!("cue {line:?}: word {i} is not a number"))
        };
        match words.first().copied() {
            Some("wheel") => Ok(Cue::Wheel {
                at: middle(g, (n(1)?, n(2)?)),
                steps: n(3)?,
            }),
            Some("drag") => Ok(Cue::Drag {
                from: middle(g, (n(1)?, n(2)?)),
                to: middle(g, (n(3)?, n(4)?)),
            }),
            Some("click") => Ok(Cue::Click {
                at: middle(g, (n(1)?, n(2)?)),
            }),
            Some("done") => Ok(Cue::Done),
            _ => Err(format!("cue {line:?}: not wheel, drag, click or done")),
        }
    }

    /// **The pointer inputs a gesture is**, each at its instant from `now`:
    /// a detent each 40 ms, a drag across 0.7 s in 20 moves, a click's press
    /// and release 80 ms apart. Empty for `done`.
    fn inputs(self, now: Duration) -> Vec<(Duration, Input)> {
        let ms = |d: u64| now + Duration::from_millis(d);
        let stamp =
            |d: Duration| EventMillis::of_millis(u32::try_from(d.as_millis()).unwrap_or(u32::MAX));
        let press = |d: Duration| Input::Pressed {
            button: Pressed::Left,
            time: stamp(d),
        };
        let release = |d: Duration| Input::Released {
            button: Pressed::Left,
            time: stamp(d),
        };
        match self {
            Cue::Wheel { at, steps } => {
                let one = steps.signum();
                let count = steps.abs().round();
                let mut v = vec![(now, Input::Moved(at.0, at.1))];
                let mut d = 40;
                let mut left = count;
                while left > 0.0 {
                    v.push((ms(d), Input::Steps(one)));
                    d += 40;
                    left -= 1.0;
                }
                v.push((ms(d), Input::Stopped));
                v
            }
            Cue::Drag { from, to } => {
                let mut v = vec![(now, Input::Moved(from.0, from.1)), (ms(80), press(ms(80)))];
                for i in 1..=20u32 {
                    let p = f64::from(i) / 20.0;
                    let at = (from.0 + (to.0 - from.0) * p, from.1 + (to.1 - from.1) * p);
                    v.push((ms(80 + 35 * u64::from(i)), Input::Moved(at.0, at.1)));
                }
                v.push((ms(860), release(ms(860))));
                v
            }
            Cue::Click { at } => vec![
                (now, Input::Moved(at.0, at.1)),
                (ms(40), press(ms(40))),
                (ms(120), release(ms(120))),
            ],
            Cue::Done => Vec::new(),
        }
    }
}

/// **The cue file**: what the demo has appended since the last look, by
/// whole lines; a line still being written waits in `partial`.
struct Cues {
    file: std::fs::File,
    partial: Vec<u8>,
}

impl Cues {
    fn new(path: &Path) -> Cues {
        std::fs::write(path, b"").expect("the cue file");
        Cues {
            file: std::fs::File::open(path).expect("the cue file"),
            partial: Vec::new(),
        }
    }

    fn take(&mut self) -> Vec<String> {
        let _ = std::io::Read::read_to_end(&mut self.file, &mut self.partial);
        let Some(end) = self.partial.iter().rposition(|&b| b == b'\n') else {
            return Vec::new();
        };
        let rest = self.partial.split_off(end + 1);
        let lines = String::from_utf8_lossy(&self.partial)
            .lines()
            .map(str::to_string)
            .collect();
        self.partial = rest;
        lines
    }
}

/// What a take recorded.
struct Recorded {
    log: Vec<Logged>,
    /// Everything the child wrote, and everything written to it.
    heard: Vec<u8>,
    wrote: Vec<u8>,
    /// When it ended, from the launch.
    end: Duration,
    /// The screen's text and the cursor then.
    screen: Vec<String>,
    cursor: (baud_domain::Row, baud_domain::Col),
    /// What the lights showed: when each first lit, and every change.
    #[cfg(target_os = "macos")]
    lit: session::Lit,
    #[cfg(target_os = "macos")]
    timeline: Vec<(Duration, baud_render::Shown)>,
    #[cfg(target_os = "macos")]
    links: Option<(tap::Report, Vec<PathBuf>)>,
}

/// **The take**: the child in real time against a terminal of the stage's,
/// every read, decode step and pointer input logged with its instant.
fn record(f: &Film<'_>, stage: &Stage) -> Recorded {
    // A directory of its own for each take, so takes in parallel tests do
    // not share a cue file, whose `done` would end the other take.
    static TAKES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = TAKES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let scratch = Scratch::new(&format!("film{n}"));
    let cues = scratch.path().join("cues");
    let mut cue_file = Cues::new(&cues);
    let cues_os = cues.into_os_string();
    let mut env = scratch.env(&[]);
    env.extend(f.env.iter().cloned());
    env.push(("BAUD_FILM_CUES".into(), cues_os));
    let mut owned = Owned::new(&f.grid).with_images(stage.cfg.images);
    let mut term = stage.term(Term::new(owned.storage(), &f.grid));
    let m = stage.renderer.metrics();
    let ws = crate::pty::text_area(&f.grid, m.cell_w, m.cell_h);
    let g = stage.geometry(&f.grid);
    let epoch = Instant::now();
    let mut u = match f.child {
        Child::Command(cmd) => Under::start_on(&mut term, cmd, env, ws, g),
        #[cfg(target_os = "macos")]
        Child::Session { dir, .. } => Under::start_shell(&mut term, f.env.clone(), dir, ws, g),
    };
    let mut log = Vec::new();
    let mut heard = Vec::new();
    let mut steps: std::collections::VecDeque<(Duration, Input)> = Default::default();
    let mut decoding = DecodeProgress::Idle;
    #[cfg(target_os = "macos")]
    let mut player = match f.child {
        Child::Session { steps, .. } => Some(session::Player::new(steps, g)),
        Child::Command(_) => None,
    };
    #[cfg(target_os = "macos")]
    let mut lights = session::Lights::new(u.pid());
    // The link layer on the take, as the app runs it, when asked for.
    #[cfg(target_os = "macos")]
    let (mut tap, hovers) = match f.child {
        Child::Session {
            links: Some(dir),
            steps,
            ..
        } => (
            Some(tap::Tap::new(dir, &stage.cfg, &f.env, u.master(), u.pid())),
            steps,
        ),
        _ => (None, &[][..]),
    };
    let mut watch = f.source.map(Watch::new);
    loop {
        let t = epoch.elapsed();
        assert!(
            t < f.limit,
            "no `done` from the demo by {:?}; the screen:\n{}",
            f.limit,
            crate::programs::screen(u.term).join("\n")
        );
        while let Some(&(at, e)) = steps.front() {
            if at > t {
                break;
            }
            steps.pop_front();
            u.input(e, Modifiers::NONE);
            log.push(Logged {
                at: epoch.elapsed(),
                event: Event::Input(e),
            });
        }
        if decoding == DecodeProgress::More {
            decoding = u.decode();
            log.push(Logged {
                at: epoch.elapsed(),
                event: Event::Decode,
            });
        }
        let wait = match decoding {
            DecodeProgress::More => Duration::ZERO,
            DecodeProgress::Idle => steps.front().map_or(POLL, |&(at, _)| {
                at.saturating_sub(epoch.elapsed()).min(POLL)
            }),
        };
        match u.read(wait) {
            Heard::Bytes(b) => {
                // Who wrote it, at the read, and the line it began on.
                #[cfg(target_os = "macos")]
                let taken = tap.as_mut().map(|t| {
                    let began = u
                        .seat
                        .scrolled()
                        .line(baud_domain::Rows::ZERO, u.term.cursor().0);
                    let began = (u.term.screen(), began);
                    (began, t.read(&b))
                });
                // Fed in the parts the app feeds it in, each under its own
                // writers (`reads::paced`), when the tap files them.
                #[cfg(target_os = "macos")]
                let (_answered, parts) = match taken {
                    Some((_, read)) => u.take_parts(&b, read),
                    None => (u.take(&b), Vec::new()),
                };
                #[cfg(not(target_os = "macos"))]
                let _answered = u.take(&b);
                #[cfg(target_os = "macos")]
                if let (Some(t), Some((began, _))) = (tap.as_mut(), taken) {
                    t.fed(
                        u.term,
                        u.seat.scrolled(),
                        began,
                        u.seat.counts().core_lines(u.term),
                        &parts,
                    );
                }
                #[cfg(target_os = "macos")]
                lights.read(&b, _answered, epoch.elapsed());
                heard.extend_from_slice(&b);
                log.push(Logged {
                    at: epoch.elapsed(),
                    event: Event::Read(b),
                });
                // A read may start a picture's decode: the next turn steps
                // it, as the Linux loop steps one each turn.
                decoding = DecodeProgress::More;
            }
            Heard::Quiet => {}
            Heard::Closed => panic!(
                "the demo exited before its end; the screen:\n{}",
                crate::programs::screen(u.term).join("\n")
            ),
        }
        if let Some(text) = watch.as_mut().and_then(|w| w.turn(epoch.elapsed())) {
            log.push(Logged {
                at: epoch.elapsed(),
                event: Event::Reload(text),
            });
        }
        #[cfg(target_os = "macos")]
        let ended = match &mut player {
            Some(p) => p.turn(epoch.elapsed(), &heard, &mut u, &mut steps, &mut log, epoch),
            None => session::Turn::Going,
        };
        // The tap's frame: answers merged, the clock ticked, the lines
        // scanned; and what a Command-click would take on the hovered
        // link, read as its step ends.
        #[cfg(target_os = "macos")]
        if let (Some(t), Some(p)) = (tap.as_mut(), player.as_ref()) {
            let step = p.step();
            let left = t.step().filter(|&s| s != step);
            t.turn(u.term, u.seat.scrolled(), Some(step), epoch.elapsed());
            if let Some(s) = left {
                // The link's cell is found on the screen by its text, where
                // this grid shows it, whatever the step's cell.
                if let Some(session::Step::Hover(_)) = hovers.get(s.0) {
                    t.hover(u.term, u.seat.scrolled(), s, session::LINK_URL);
                }
            }
        }
        #[cfg(target_os = "macos")]
        lights.turn(
            epoch.elapsed(),
            u.term,
            baud_domain::BytePos::from_index(u.wrote.len()),
            u.shorts,
            player.as_ref(),
        );
        #[cfg(target_os = "macos")]
        let mut done = ended == session::Turn::Done;
        #[cfg(not(target_os = "macos"))]
        let mut done = false;
        for line in cue_file.take() {
            match Cue::parse(&line, g).unwrap_or_else(|e| panic!("{e}")) {
                Cue::Done => done = true,
                c => steps.extend(c.inputs(epoch.elapsed())),
            }
        }
        if done {
            let wrote = std::mem::take(&mut u.wrote);
            #[cfg(target_os = "macos")]
            let links = tap.take().map(|mut t| {
                let files = t.write(u.term, u.seat.scrolled());
                (t.report, files)
            });
            return Recorded {
                log,
                heard,
                wrote,
                end: epoch.elapsed(),
                screen: crate::programs::screen(u.term),
                cursor: u.term.cursor(),
                #[cfg(target_os = "macos")]
                lit: lights.lit(),
                #[cfg(target_os = "macos")]
                timeline: lights.timeline(),
                #[cfg(target_os = "macos")]
                links,
            };
        }
    }
}

/// Every pixel over the opaque page `page`, its alpha folded in: what the
/// window shows with nothing behind its page.
fn flatten(pixels: &mut [u32], page: baud_render::Argb) {
    let page = page.0;
    let ch = |p: u32, shift: u32| (p >> shift) & 0xff;
    for p in pixels.iter_mut() {
        let a = ch(*p, 24);
        if a == 0xff {
            continue;
        }
        let mix = |shift: u32| (ch(*p, shift) + (0xff - a) * ch(page, shift) / 0xff).min(0xff);
        *p = 0xff00_0000 | (mix(16) << 16) | (mix(8) << 8) | mix(0);
    }
}

/// **A film made**: the take, then every frame drawn from its log at its
/// instant and handed to `keep`.
pub(crate) fn film(f: &Film<'_>, keep: impl FnMut(Frame<'_>)) -> Take {
    let mut stage = Stage::new(f);
    let recorded = record(f, &stage);
    play(f, &mut stage, recorded, keep)
}

/// **The take's log played**: every frame drawn at its instant and handed
/// to `keep`, and the checks that the film's terminal answered and ended
/// as the take's did.
fn play(
    f: &Film<'_>,
    stage: &mut Stage,
    recorded: Recorded,
    mut keep: impl FnMut(Frame<'_>),
) -> Take {
    let Recorded {
        log,
        heard,
        wrote,
        end,
        screen,
        cursor,
        #[cfg(target_os = "macos")]
        lit,
        #[cfg(target_os = "macos")]
        timeline,
        #[cfg(target_os = "macos")]
        links,
    } = recorded;
    #[cfg(target_os = "macos")]
    let mut seen = 0;
    let mut step = None;
    let g = stage.geometry(&f.grid);
    let mut owned = Owned::new(&f.grid).with_images(stage.cfg.images);
    let mut term = stage.term(Term::new(owned.storage(), &f.grid));
    let mut seat = Seat::new();
    seat.set_word_wrap(stage.cfg.wrap_words);
    seat.focused(Focus::Gained);
    let epoch = Instant::now();
    let cfg = &stage.cfg;
    let mut timed = crate::timed::Timed::new(epoch, cfg.cursor_trail, cfg.screen_transition)
        .with_trail_scroll(cfg.trail_scroll)
        .with_splash(cfg.launch_splash);
    timed.set_image_animation(cfg.image_animation);
    let _ = timed.set_motion(crate::config::SystemMotion::Full, epoch);
    let mut caret = crate::caret::Caret::new(
        epoch,
        crate::caret::Cycle::of_config(cfg.blink_interval),
        crate::caret::Gate::of(cfg.cursor_blink, crate::caret::NonBlinking::Off),
        cfg.cursor_stop,
        cfg.blink_animation,
    );
    let mut updates = crate::bell::UpdateClock::new(SYNC_MAX);
    // The phase the last drawn frame showed, for a frame a synchronised
    // update holds.
    let mut drawn_caret = baud_render::CursorPhase::SHOWN;
    let (guide, badge) = (cfg.cursor_guide, cfg.badge);
    let session = crate::effects::Session::here(None);
    let (gw, gh) = stage.renderer.whole_size();
    let (w, h) = (
        Px::of(gw.value() + 2 * PAD.value()),
        Px::of(gh.value() + 2 * PAD.value()),
    );
    let slots = |p: Px| usize::try_from(p.value()).expect("a size");
    let mut frame = vec![0u32; slots(w) * slots(h)];
    // The grid's first pixel, past the padding, and its rows.
    let offset = slots(PAD) * slots(w) + slots(PAD);
    let rows = slots(PAD) * slots(w)..(slots(PAD) + slots(gh)) * slots(w);
    let mut shown = frame.clone();
    // The text of the frame in `shown`.
    let mut shown_text: Vec<String> = Vec::new();
    let mut kept: Vec<u32> = Vec::new();
    let mut composed: Vec<u32> = Vec::new();
    let mut margin = None;
    let mut damage = Damage::Full;
    let mut replies = Vec::new();
    let mut events = log.into_iter().peekable();
    let mut read = stage.read.clone();
    let mut trails = vec![(None, timed.trail())];
    let mut ignitions = Vec::new();
    let mut seed = timed.next_seed();
    let mut k = 0u32;
    loop {
        let at = f.period * k;
        // Past the end, what came after the last frame is still fed, so the
        // two terminals can be compared at the take's end.
        let upto = at.min(end);
        let now = epoch + at;
        while let Some(e) = events.next_if(|e| e.at <= upto) {
            let when = epoch + e.at;
            match e.event {
                Event::Read(b) => {
                    let before = seat.before_read(&mut term);
                    crate::reads::feed(
                        &mut term,
                        [&b[..]],
                        |o| replies.extend_from_slice(o),
                        |_| {},
                        &mut crate::hostlog::NoLog,
                    );
                    let rec = term.take_record();
                    let after = seat.after_read(&term, before, Some(&rec));
                    timed.scrolled(&term, before);
                    if after.surface == crate::seat::Surface::Switched {
                        let form = timed.switched(when, term.screen());
                        if let Some(crate::effects::SwitchForm::Slide(_)) = form {
                            let mut target = Target::new(&mut frame[offset..], w, gw, gh)
                                .expect("the frame holds the grid");
                            stage.renderer.render_hidden(&term, &mut target);
                            kept.clone_from(&frame);
                        }
                    }
                    if after.thumb == crate::seat::Thumb::Reveal {
                        timed.bar.reveal.reveal(timed.clock.host(when));
                    }
                }
                Event::Decode => {
                    let _ = crate::reads::decode_step(&mut term);
                    crate::reads::owed(&mut term, &mut |o: &[u8]| replies.extend_from_slice(o));
                }
                Event::Typed(b) => {
                    // The film draws a frame at every period, so what the
                    // input owes is drawn by the next.
                    let _ = caret.input(when, crate::caret::Input::Key);
                    replies.extend_from_slice(&b);
                }
                Event::Step(i) => step = Some(i),
                Event::Reload(text) => {
                    use crate::live::Part;
                    let new = configured(&text, f.source, &f.env);
                    let parts = crate::live::between(&read, &new, crate::live::FaceFiles::Same);
                    for part in parts {
                        match part {
                            // Always listed: a theme file may have changed.
                            // The film's theme is BASE's, which no file
                            // changes.
                            Part::Palette => {}
                            Part::Trail | Part::Transition => {
                                timed.set_trail_scroll(new.trail_scroll);
                                let _ =
                                    timed.set_config(new.cursor_trail, new.screen_transition, when);
                            }
                            other => panic!(
                                "the config changed its {other:?}, which a film does not \
                                 apply; it applies the trail and the transition"
                            ),
                        }
                    }
                    read = new;
                }
                Event::Input(i) => {
                    if matches!(
                        i,
                        Input::Pressed { .. } | Input::Steps(_) | Input::Distance(_)
                    ) {
                        caret.quiet(when);
                    }
                    let back = term.primary_window_back();
                    let mut out = Vec::new();
                    seat.input(&mut term, i, Modifiers::NONE, g, |o| {
                        out.extend_from_slice(o)
                    });
                    replies.extend_from_slice(&out);
                    crate::reads::owed(&mut term, &mut |o: &[u8]| replies.extend_from_slice(o));
                    if term.primary_window_back() != back {
                        timed.bar.reveal.reveal(timed.clock.host(when));
                    }
                }
            }
        }
        if at > end {
            break;
        }
        if trails.last().map(|t| t.1) != Some(timed.trail()) {
            trails.push((step, timed.trail()));
        }
        if timed.turn(now, &mut term) == crate::timed::Owes::Full {
            damage = Damage::Full;
        }
        // A synchronised update younger than its bound holds the frame:
        // the last one stands, as on screen.
        let update = updates.observe(&term, now);
        #[cfg(target_os = "macos")]
        let lights = session::shown_at(&timeline, &mut seen, at);
        if k > 0 && update == crate::bell::Update::Honoured {
            keep(Frame {
                pixels: &shown,
                size: (w, h),
                at,
                step,
                #[cfg(target_os = "macos")]
                lights,
                caret: drawn_caret,
                text: &shown_text,
            });
            k += 1;
            continue;
        }
        let r = &mut stage.renderer;
        let visible =
            term.cursor_visible() || term.status_line().is_some_and(|l| l.cursor.is_some());
        let blinks =
            crate::caret::blinks(term.cursor_style(), Focus::Gained, visible.then_some(()));
        // The cursor this frame presents restarts the cycle if it is not
        // the one the last frame did (`Caret::present`).
        caret.present(now, crate::caret::PresentedCursor::of(&term));
        let phase = caret.plan(now, blinks);
        r.set_cursor_blink(phase);
        r.set_highlight(seat.highlight(&term));
        let guided = term.cursor_guide().unwrap_or(guide);
        r.set_cursor_guide(crate::effects::guide_look(guided, f64::from(SCALE)));
        let selection = || seat.selected_text(&term);
        let page = r.background_of(&term);
        let badged =
            crate::effects::badge_look(&term, badge, &session, &selection, page, f64::from(SCALE));
        let text = frame_text(&term, badged.as_ref());
        r.set_badge(badged);
        let m = r.metrics();
        let side = |p: Px| f32::from(u16::try_from(p.value()).unwrap_or(u16::MAX));
        let cell = crate::timed::Cell {
            px: (side(m.cell_w), side(m.cell_h)),
            height: baud_render::Points(side(m.cell_h) / SCALE),
        };
        let widths = crate::reveal::widths(crate::reveal::Scale(f64::from(SCALE)));
        let plan = timed.plan(
            now,
            &term,
            |row, col, style| r.cursor_rect(row, col, style),
            cell,
            widths,
        );
        if timed.next_seed() != seed {
            seed = timed.next_seed();
            ignitions.push((step, timed.trail(), at));
        }
        r.set_scrollbar(plan.scroll);
        r.set_screen_fade(plan.fade);
        r.set_blink(plan.blink);
        r.set_trail(plan.trail);
        r.set_splash(plan.splash);
        r.splash_yields_to(&term, Px::of(0));
        let shows: Vec<_> = plan.fireworks.iter().flatten().copied().collect();
        r.set_fireworks(&shows);
        let clear = r.clear_color_of(&term);
        if margin != Some(clear) {
            margin = Some(clear);
            frame.fill(clear.0);
            damage = Damage::Full;
        }
        let back = term.window_back();
        r.place_marks(&term, back, baud_render::Leaving::None, Px::of(0));
        {
            let mut target =
                Target::new(&mut frame[offset..], w, gw, gh).expect("the frame holds the grid");
            r.render_at(&mut term, back, &mut target, damage);
        }
        // A face the frame needed and the stack lacked, found as the
        // drivers find it, and the frame drawn again with it.
        if let Some(Ok(_)) = crate::host::follow_gaps(r, &mut stage.fonts, stage.px) {
            let r = &mut stage.renderer;
            r.set_splash(plan.splash);
            r.set_trail(plan.trail);
            r.splash_yields_to(&term, Px::of(0));
            r.place_marks(&term, back, baud_render::Leaving::None, Px::of(0));
            let mut target =
                Target::new(&mut frame[offset..], w, gw, gh).expect("the frame holds the grid");
            r.render_at(&mut term, back, &mut target, Damage::Full);
        }
        let r = &mut stage.renderer;
        seat.drawn(r.bidi());
        timed.drawn(r.blink_seen(), term.regis_blink());
        caret.commit(phase);
        drawn_caret = phase;
        damage = Damage::Incremental;
        // A sliding frame: the grid's rows composed from the live frame and
        // the one kept at the switch.
        let slid = match plan.slide {
            Some(s) if kept.len() == frame.len() => {
                composed.clone_from(&frame);
                let o = baud_render::slide_offsets(s, gh);
                let (live, old) = (&frame[rows.clone()], &kept[rows.clone()]);
                match baud_render::compose_slide(live, old, &mut composed[rows.clone()], w, gh, o) {
                    true => crate::effects::Composed::Yes,
                    false => crate::effects::Composed::No,
                }
            }
            _ => {
                if plan.slide.is_none() {
                    kept.clear();
                    composed.clear();
                }
                crate::effects::Composed::No
            }
        };
        shown.clone_from(match slid {
            crate::effects::Composed::Yes => &composed,
            crate::effects::Composed::No => &frame,
        });
        flatten(&mut shown, clear);
        shown_text = text;
        keep(Frame {
            pixels: &shown,
            size: (w, h),
            at,
            step,
            #[cfg(target_os = "macos")]
            lights,
            caret: phase,
            text: &shown_text,
        });
        // The window is shown once its first frame is: the splash's clock
        // starts on the next (`Timed::shown`).
        if k == 0 {
            let _ = timed.shown();
        }
        k += 1;
    }
    assert!(
        replies == wrote,
        "the film's terminal answered {} bytes where the take's answered {}: \
         the film would not show the screen the child talked to",
        replies.len(),
        wrote.len()
    );
    assert_eq!(
        (crate::programs::screen(&term), term.cursor()),
        (screen, cursor),
        "the film's terminal ended on another screen than the take's"
    );
    let m = stage.renderer.metrics();
    Take {
        trails,
        ignitions,
        heard,
        answers: wrote,
        frames: Frames(usize::try_from(k).unwrap_or(usize::MAX)),
        cell: (m.cell_w, m.cell_h),
        #[cfg(target_os = "macos")]
        lit,
        #[cfg(target_os = "macos")]
        links,
    }
}

/// **The machine's names**: the user's, the host's, the home directory, the
/// temporary directory -- `TMPDIR`, and `std::env::temp_dir()`, which
/// `Scratch` roots the child's home under and which is `/tmp` on Linux
/// with `TMPDIR` unset -- and the roots user and temporary paths start
/// with, and any `extra` the caller knows (a Mac's computer name).
pub(crate) fn machine_names(extra: &[String]) -> Vec<String> {
    let mut names: Vec<String> = ["USER", "LOGNAME", "HOME", "TMPDIR"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .chain(std::env::temp_dir().to_str().map(str::to_string))
        .map(|v| v.trim_end_matches('/').to_string())
        .collect();
    let host = std::process::Command::new("hostname")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    names.extend(host.split('.').next().map(str::to_string));
    names.push(host);
    names.extend(
        ["/Users/", "/home/", "/private/", "/var/folders"]
            .iter()
            .map(|s| s.to_string()),
    );
    names.extend(extra.iter().cloned());
    names.retain(|n| n.len() >= 3);
    names.sort();
    names.dedup();
    names
}

/// **Which of `names` the bytes hold**, as fixed strings.
pub(crate) fn named(bytes: &[u8], names: &[String]) -> Vec<String> {
    names
        .iter()
        .filter(|n| bytes.windows(n.len()).any(|w| w == n.as_bytes()))
        .cloned()
        .collect()
}

/// A `COLSxROWS` grid.
pub(crate) fn grid_of(s: &str) -> WinSize {
    let (c, r) = s.split_once('x').expect("a grid is COLSxROWS");
    WinSize {
        ws_col: c.parse().expect("columns"),
        ws_row: r.parse().expect("rows"),
    }
}

/// **A film of about a second of a trivial child opens on the splash**, as a
/// launch does, and the child names nothing of the machine: the first
/// frame is the empty page (the window is shown once it is drawn); a fifth
/// of a second in, the lockup fills the middle of the frame; by 0.8 s it
/// is gone and the middle is the page again, and the prompt the child
/// printed before its `done` is drawn at the top left of the last frame;
/// each of those is the replay's clock, so a slow machine reads the same;
/// the child's `DA` was answered, and the film's terminal answered what the
/// take's did and ended on its screen and cursor (asserted in `film`); and
/// what the child wrote, and what was written to it, holds none of the
/// machine's names, while each of them, written, is found -- the probe can
/// fire. Perturbations: skip `timed.shown()`, or start the splash off --
/// the frame at 0.2 s has an empty middle; print `$HOME` from the child --
/// the names check fails; drop the film's answers to a read, or the last
/// byte of each read it feeds -- `film`'s checks that both terminals
/// answered alike, and ended alike, fail.
#[test]
fn a_film_opens_on_the_splash_and_names_nothing_of_the_machine() {
    let period = Duration::from_millis(40);
    let f = Film {
        child: Child::Command(
            "stty -echo; printf '\\033[c\\033[1;36mbaud\\033[0m \\033[35m\\342\\235\\257\\033[0m '; \
              sleep 0.9; echo done >>\"$BAUD_FILM_CUES\"; exec sleep 30",
        ),
        env: Vec::new(),
        grid: WinSize {
            ws_col: 40,
            ws_row: 12,
        },
        config: "font_size = 10\n",
        source: None,
        period,
        limit: Duration::from_secs(30),
    };
    let mut frames: Vec<(Duration, Vec<u32>, (Px, Px))> = Vec::new();
    let take = film(&f, |fr| frames.push((fr.at, fr.pixels.to_vec(), fr.size)));
    assert_eq!(take.frames, Frames(frames.len()));
    assert!(
        frames.len() >= 23,
        "a frame each 40 ms to the `done`, 0.9 s on"
    );
    assert_eq!(frames[5].0, Duration::from_millis(200));
    let n = |p: Px| usize::try_from(p.value()).unwrap();
    let (w, h) = (n(frames[0].2.0), n(frames[0].2.1));
    // The pixels of frame `i` in the rectangle from `(x0, y0)` to `(x1,
    // y1)` that are not the page's colour, which the padding shows.
    let inked = |i: usize, (x0, y0): (usize, usize), (x1, y1): (usize, usize)| {
        let px = &frames[i].1;
        (y0..y1)
            .flat_map(|y| (x0..x1).map(move |x| y * w + x))
            .filter(|&at| px[at] != px[0])
            .count()
    };
    // The middle third, below the prompt's row and above the last.
    let middle = |i: usize| inked(i, (w / 3, h / 3), (2 * w / 3, 2 * h / 3));
    let area = (w / 3) * (h / 3);
    assert_eq!(middle(0), 0, "the first frame, before the window is shown");
    assert!(
        middle(5) * 50 > area,
        "the lockup at 0.2 s: {} of {area} pixels inked",
        middle(5)
    );
    assert_eq!(middle(20), 0, "the splash gone by 0.8 s");
    let (pad, (cw, ch)) = (n(PAD), (n(take.cell.0), n(take.cell.1)));
    let last = frames.len() - 1;
    let prompt = inked(last, (pad, pad), (pad + 8 * cw, pad + ch));
    assert!(prompt > 0, "the prompt drawn at the top left");
    assert!(take.answers.starts_with(b"\x1b[?"), "the DA answer");
    let names = machine_names(&[]);
    assert!(!names.is_empty());
    assert_eq!(named(&take.heard, &names), Vec::<String>::new());
    assert_eq!(named(&take.answers, &names), Vec::<String>::new());
    for n in &names {
        let written = format!("a {n} b");
        assert!(named(written.as_bytes(), &names).contains(n), "{n}");
    }
}

/// **A config file changed during the take is read again, and its trail
/// drawn from then on**, as baud's reload draws it: the film's config read
/// from a file with the plasma trail, which the child rewrites to fire
/// 0.3 s in. The film draws plasma, then fire from a frame after the
/// rewrite and its settle, and nothing else changed; a file rewritten with
/// the same text is no change. The rewrite truncates the file and writes
/// it 0.1 s later, as a save under load can, and the empty file between is
/// no change either. The control is the first film's first frame, plasma.
/// Perturbations: drop the take's watch, or the film's `Reload` arm, and
/// the trail never changes; read the empty file as a config and the film
/// fails on a part it does not apply.
#[test]
fn a_config_file_changed_in_the_take_changes_the_trail() {
    use crate::config::CursorTrail;
    let dir = std::env::temp_dir().join(format!("baud-film-reload-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a directory");
    let conf = dir.join("config.toml");
    let text = |trail: &str| format!("font_size = 10\n[cursor]\ntrail = \"{trail}\"\n");
    let run = |to: &str| {
        let initial = text("plasma");
        std::fs::write(&conf, &initial).expect("the config");
        let cmd = format!(
            "sleep 0.3; : > \"$CONF\"; sleep 0.1; printf '%s' '{}' > \"$CONF\"; sleep 0.4; \
             echo done >>\"$BAUD_FILM_CUES\"; exec sleep 30",
            text(to)
        );
        let f = Film {
            child: Child::Command(&cmd),
            env: vec![("CONF".into(), conf.clone().into_os_string())],
            grid: WinSize {
                ws_col: 20,
                ws_row: 5,
            },
            config: &initial,
            source: Some(&conf),
            period: Duration::from_millis(40),
            limit: Duration::from_secs(30),
        };
        let mut at = Vec::new();
        let take = film(&f, |fr| at.push(fr.at));
        (take.trails, at)
    };
    let (trails, _) = run("fire");
    assert_eq!(
        trails.iter().map(|t| t.1).collect::<Vec<_>>(),
        [CursorTrail::Plasma, CursorTrail::Fire],
        "plasma, then fire once the file says so"
    );
    let (same, _) = run("plasma");
    assert_eq!(
        same,
        [(None, CursorTrail::Plasma)],
        "the same text is no change"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// **A film's config is read under its baud's `HOME` and `PATH`**, as the
/// README session's baud reads its own: a `[link.path]` program under `~/`
/// in the film's home and a `[link.https]` program named bare on the
/// film's `PATH`, neither of them in this process's. Read under this
/// process's, both are refused and `configured` fails on what it said.
#[test]
fn a_films_config_is_read_under_its_own_home_and_path() {
    use crate::rules::{Act, PathAct};
    use std::os::unix::fs::PermissionsExt as _;
    let dir = std::env::temp_dir().join(format!("baud-film-env-{}", std::process::id()));
    let home = dir.join("home");
    let path = dir.join("path");
    for (folder, program) in [(home.join("bin"), "opener"), (path.clone(), "viewer")] {
        std::fs::create_dir_all(&folder).expect("a folder");
        let p = folder.join(program);
        std::fs::write(&p, "#!/bin/sh\n").expect("a program");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).expect("its mode");
    }
    let text = "[link.path]\naction = \"window\"\ncommand = [\"~/bin/opener\", \"{path}\"]\n\
                [link.https]\naction = \"window\"\ncommand = [\"viewer\", \"{url}\"]\n";
    let env = [
        (OsString::from("HOME"), home.clone().into_os_string()),
        (OsString::from("PATH"), path.into_os_string()),
    ];
    let cfg = configured(text, None, &env);
    let opener = home.join("bin/opener");
    let rules = &cfg.link.rules;
    match rules.path_act() {
        PathAct::Window(a) => assert_eq!(Some(a.program()), opener.to_str()),
        other => panic!("the path rule is {other:?}"),
    }
    match rules.rule_for("https").map(|(_, r)| r.act()) {
        Some(Act::Window(a)) => assert_eq!(a.program(), "viewer"),
        other => panic!("the https rule is {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// **The README's clip**, made by `scripts/readme-film.sh`, which pins the
/// programs and the photo and encodes what this writes: `BAUD_FILM_CMD` run
/// at `BAUD_FILM_GRID` (`COLSxROWS`) with `font_size = BAUD_FILM_POINTS`
/// and `BAUD_FILM_CONFIG`'s lines (the frame is laid out at the default
/// 8 pt padding, [`PAD`], whatever a `padding` line among them says), at
/// `BAUD_FILM_FPS` frames a second until the demo's `done` or
/// `BAUD_FILM_SECS`; the variables `BAUD_FILM_ENV` names are passed to the
/// demo; every frame goes as a PNG into `BAUD_FILM_OUT` through
/// `BAUD_FILM_FFMPEG`; and the take fails if the demo wrote a name of the
/// machine, `BAUD_FILM_FORBID`'s lines among them.
#[test]
#[ignore = "a tool: scripts/readme-film.sh runs it"]
fn film_the_readme_clip() {
    use std::io::Write as _;
    let var = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("{k} is not set"));
    let cmd = var("BAUD_FILM_CMD");
    let out = PathBuf::from(var("BAUD_FILM_OUT"));
    let ffmpeg = var("BAUD_FILM_FFMPEG");
    let fps: u32 = var("BAUD_FILM_FPS").parse().expect("BAUD_FILM_FPS");
    let secs: u64 = var("BAUD_FILM_SECS").parse().expect("BAUD_FILM_SECS");
    let config = format!(
        "font_size = {}\n{}",
        var("BAUD_FILM_POINTS"),
        std::env::var("BAUD_FILM_CONFIG").unwrap_or_default()
    );
    let env: Vec<(OsString, OsString)> = std::env::var("BAUD_FILM_ENV")
        .unwrap_or_default()
        .split_whitespace()
        .filter_map(|k| std::env::var_os(k).map(|v| (OsString::from(k), v)))
        .collect();
    let forbid: Vec<String> = std::env::var("BAUD_FILM_FORBID")
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect();
    let f = Film {
        child: Child::Command(&cmd),
        env,
        grid: grid_of(&var("BAUD_FILM_GRID")),
        config: &config,
        source: None,
        period: Duration::from_secs(1) / fps,
        limit: Duration::from_secs(secs),
    };
    std::fs::create_dir_all(&out).expect("the frames' directory");
    let mut encoder: Option<std::process::Child> = None;
    let started = Instant::now();
    let take = film(&f, |fr| {
        let e = encoder.get_or_insert_with(|| {
            let size = format!("{}x{}", fr.size.0.value(), fr.size.1.value());
            std::process::Command::new(&ffmpeg)
                .args(["-v", "error", "-f", "rawvideo", "-pix_fmt", "bgra"])
                .args(["-video_size", &size, "-framerate", &fps.to_string()])
                .args(["-i", "-"])
                .arg(out.join("%05d.png"))
                .stdin(std::process::Stdio::piped())
                .spawn()
                .expect("ffmpeg")
        });
        let bytes: Vec<u8> = fr.pixels.iter().flat_map(|p| p.to_le_bytes()).collect();
        e.stdin
            .as_mut()
            .expect("ffmpeg's input")
            .write_all(&bytes)
            .expect("a frame to ffmpeg");
    });
    let mut e = encoder.expect("a frame");
    drop(e.stdin.take());
    assert!(e.wait().expect("ffmpeg").success(), "ffmpeg failed");
    let names = machine_names(&forbid);
    let (found, sent) = (named(&take.heard, &names), named(&take.answers, &names));
    assert!(
        found.is_empty() && sent.is_empty(),
        "the machine's names: {found:?} written by the demo, {sent:?} to it"
    );
    println!(
        "film: {} frames at {fps} a second, {:?} to make; {} bytes of answers \
         and the screen at the end, the same from both terminals",
        take.frames.0,
        started.elapsed(),
        take.answers.len()
    );
}

/// **A take of `log` with no child**: the instants and bytes a child would
/// have logged, and what the take's terminal would have answered and ended
/// on, from a terminal of the stage's fed the same reads -- so a film can
/// play bytes a test wrote, at the instants it chose.
fn written(f: &Film<'_>, stage: &Stage, log: Vec<Logged>, end: Duration) -> Recorded {
    let mut owned = Owned::new(&f.grid).with_images(stage.cfg.images);
    let mut term = stage.term(Term::new(owned.storage(), &f.grid));
    let (mut heard, mut wrote) = (Vec::new(), Vec::new());
    for e in &log {
        match &e.event {
            Event::Read(b) => {
                heard.extend_from_slice(b);
                crate::reads::feed(
                    &mut term,
                    [&b[..]],
                    |o| wrote.extend_from_slice(o),
                    |_| {},
                    &mut crate::hostlog::NoLog,
                );
            }
            Event::Typed(b) => wrote.extend_from_slice(b),
            _ => panic!("a written take reads and types, and nothing else"),
        }
    }
    Recorded {
        log,
        heard,
        wrote,
        end,
        screen: crate::programs::screen(&term),
        cursor: term.cursor(),
        #[cfg(target_os = "macos")]
        lit: session::Lit::default(),
        #[cfg(target_os = "macos")]
        timeline: Vec::new(),
        #[cfg(target_os = "macos")]
        links: None,
    }
}

/// The cursor blinking, at the default rate, with no trail and no splash to
/// draw over the frames, and a frame every 10 ms.
fn blink_film() -> Film<'static> {
    Film {
        child: Child::Command("true"),
        env: Vec::new(),
        grid: WinSize {
            ws_col: 84,
            ws_row: 10,
        },
        config: "[cursor]\nblink = true\ntrail = \"off\"\n[launch]\nsplash = \"off\"\n",
        source: None,
        period: Duration::from_millis(10),
        limit: Duration::from_secs(10),
    }
}

/// A drawn frame of a blink film: its instant, its cursor's phase and a
/// digest of its pixels.
type Drawn = (Duration, baud_render::CursorPhase, u64);

/// **`log` filmed** until `end`: every frame drawn.
fn blink_frames(log: Vec<(Duration, Event)>, end: Duration) -> Vec<Drawn> {
    use std::hash::{Hash as _, Hasher as _};
    let f = blink_film();
    let mut stage = Stage::new(&f);
    let log = log
        .into_iter()
        .map(|(at, event)| Logged { at, event })
        .collect();
    let recorded = written(&f, &stage, log, end);
    let mut drawn = Vec::new();
    play(&f, &mut stage, recorded, |fr| {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        fr.pixels.hash(&mut h);
        drawn.push((fr.at, fr.caret, h.finish()));
    });
    drawn
}

/// Whether the other pane's spinner turns from frame to frame, or holds
/// one glyph so every frame draws the same pixels.
#[derive(Clone, Copy, PartialEq)]
enum Spin {
    Turning,
    Still,
}

/// Whether every frame of the stream is one read, or every other one is
/// cut across two.
#[derive(Clone, Copy, PartialEq)]
enum Cut {
    Whole,
    EveryOther,
}

/// **A multiplexer redrawing a pane the cursor is not in**, its frames in
/// turn, in the shape herdr 0.9.5 sends with a spinner running in an
/// unfocused split: a synchronised update that hides the cursor, closes any
/// link, redraws a spinner and its counter, puts the cursor back on the
/// focused pane's cell (row 8, column 30) and shows it, and after the update
/// the same cursor position and show again. The redraws' bytes are the
/// recording's, the spinner turning through its four glyphs.
fn other_pane(spin: Spin) -> impl Iterator<Item = Vec<u8>> {
    (0..).map(move |n: usize| {
        let glyph = match spin {
            Spin::Turning => ["\\", "|", "/", "-"][n / 2 % 4],
            Spin::Still => "|",
        };
        let body = match (n % 2, spin) {
            (0, _) | (_, Spin::Still) => {
                format!("\x1b[7;37H\x1b[0;39;49m{glyph}\x1b[7;40H1\x1b[0m")
            }
            _ => format!(
                "\x1b[9;76H\x1b[0;39;49m{}\x1b[9;80H{}\x1b[0m",
                100 + n % 900,
                n % 10
            ),
        };
        format!(
            "\x1b[?2026h\x1b[?25l\x1b]8;;\x1b\\{body}\x1b[8;30H\x1b[?25h\x1b[?2026l\x1b[8;30H\x1b[?25h"
        )
        .into_bytes()
    })
}

/// The multiplexer's stream until `until`: the cursor put on its cell at
/// 5 ms, then a frame every 108 ms from 100 ms -- 9.3 a second, the rate
/// measured -- each in one read, or under [`Cut::EveryOther`] every other one
/// in two reads 15 ms apart, cut after the hide, so a frame instant falls
/// inside the update.
fn stream(until: Duration, cut: Cut, spin: Spin) -> Vec<(Duration, Event)> {
    let ms = Duration::from_millis;
    let mut log = vec![(ms(5), Event::Read(b"\x1b[8;30H".to_vec()))];
    let instants = (0u32..)
        .map(|k| ms(100) + ms(108) * k)
        .take_while(|&t| t < until);
    for (n, (at, frame)) in instants.zip(other_pane(spin)).enumerate() {
        if cut == Cut::EveryOther && n % 2 == 1 {
            let opened = "\x1b[?2026h\x1b[?25l".len();
            log.push((at, Event::Read(frame[..opened].to_vec())));
            log.push((at + ms(15), Event::Read(frame[opened..].to_vec())));
        } else {
            log.push((at, Event::Read(frame)));
        }
    }
    log
}

/// The default cycle's two phases.
fn default_phases() -> (Duration, Duration) {
    match crate::caret::Cycle::DEFAULT {
        crate::caret::Cycle::Blinks { on, off } => (on.get(), off.get()),
        crate::caret::Cycle::Steady => unreachable!("the default blinks"),
    }
}

/// The instants the cursor went from shown to hidden among `drawn`'s
/// frames from `from` for `over`.
fn hides(drawn: &[Drawn], from: Duration, over: Duration) -> Vec<Duration> {
    let shown = baud_render::CursorPhase::SHOWN;
    drawn
        .windows(2)
        .filter(|w| w[1].0 >= from && w[1].0 < from + over)
        .filter(|w| w[0].1 == shown && w[1].1 != shown)
        .map(|w| w[1].0)
        .collect()
}

/// The frame drawn at `at`.
fn drawn_at(drawn: &[Drawn], at: Duration) -> Drawn {
    *drawn
        .iter()
        .find(|d| d.0 == at)
        .unwrap_or_else(|| panic!("a frame at {at:?}"))
}

/// The spinner's stream, with `event` at `at`.
fn stream_with(event: Event, at: Duration) -> Vec<(Duration, Event)> {
    let mut log = stream(Duration::from_secs(3), Cut::Whole, Spin::Turning);
    log.push((at, event));
    log.sort_by_key(|e| e.0);
    log
}

/// **A spinner in another pane does not hold the cursor steady**: herdr's
/// frames, 9.3 a second, each hiding the cursor inside a synchronised
/// update and showing it again on the cell it left, half of them cut across
/// two reads with a frame instant inside the update. The cursor presented
/// never changes after it is put on its cell, so over 3 s it hides as often
/// as an idle cursor does: once each cycle of [`Cycle::DEFAULT`] whose hide
/// falls inside the 3 s -- six at 250/250 -- counted from the frame that first
/// drew it on its cell, and the same as the control with no stream after the
/// cursor is placed. Perturbations: restart the cycle on every frame drawn
/// (`Caret::present` ignoring the last cursor) -- which is restarting on every
/// read, each read owing a frame -- and it never hides; compare the cursor at
/// each read rather than at a frame drawn, and the half-drawn reads restart it.
///
/// [`Cycle::DEFAULT`]: crate::caret::Cycle::DEFAULT
#[test]
fn a_spinner_in_another_pane_does_not_hold_the_cursor_steady() {
    let ms = Duration::from_millis;
    let (on, off) = default_phases();
    let over = Duration::from_secs(3);
    let want = (0u32..).take_while(|&k| on + (on + off) * k < over).count();
    // The first frame after the cursor is put on its cell, at 5 ms.
    let from = ms(10);
    let drawn = blink_frames(stream(ms(3200), Cut::EveryOther, Spin::Turning), ms(3200));
    let control = blink_frames(stream(ms(100), Cut::Whole, Spin::Turning), ms(3200));
    let (seen, quiet) = (hides(&drawn, from, over), hides(&control, from, over));
    assert_eq!(quiet.len(), want, "the control, no stream: {quiet:?}");
    assert_eq!(seen.len(), want, "the spinner's stream: {seen:?}");
}

/// **A key restarts the cycle, shown, at once**: in the spinner's stream, a
/// key typed at 1400 ms, in the third hidden phase, and not echoed. The
/// frame before it hides the cursor; the frame at it shows it, and so does
/// every frame for a whole shown phase after; the next hides it.
/// Perturbation: drop the film's `caret.input` and the frame at the key is
/// still hidden.
#[test]
fn a_key_shows_the_cursor_at_once() {
    let ms = Duration::from_millis;
    let (on, _) = default_phases();
    let drawn = blink_frames(stream_with(Event::Typed(b"x".to_vec()), ms(1400)), ms(3000));
    let shown = baud_render::CursorPhase::SHOWN;
    assert_ne!(drawn_at(&drawn, ms(1390)).1, shown, "hidden before the key");
    assert_eq!(drawn_at(&drawn, ms(1400)).1, shown, "shown at the key");
    let until = ms(1400) + on;
    assert_eq!(
        drawn_at(&drawn, until - ms(10)).1,
        shown,
        "a whole shown phase"
    );
    assert_ne!(drawn_at(&drawn, until).1, shown, "then hidden");
}

/// **A program moving the cursor to another cell restarts the cycle**,
/// shown: at 1400 ms, in a hidden phase, a read that moves it one column on.
/// Perturbation: leave the cell out of what `Caret::present` compares, and
/// the frame at the move is hidden.
#[test]
fn a_cursor_moved_to_another_cell_is_shown() {
    let ms = Duration::from_millis;
    let moved = Event::Read(b"\x1b[8;31H".to_vec());
    let drawn = blink_frames(stream_with(moved, ms(1400)), ms(3000));
    let shown = baud_render::CursorPhase::SHOWN;
    assert_ne!(
        drawn_at(&drawn, ms(1390)).1,
        shown,
        "hidden before the move"
    );
    assert_eq!(drawn_at(&drawn, ms(1400)).1, shown, "shown at the move");
}

/// **A change of shape restarts the cycle, shown**: `DECSCUSR 2`, a steady
/// block, blinking under `?12h` (`Term::cursor_style` folds it in), then
/// `DECSCUSR 6`, a steady bar, blinking the same, at 1400 ms in a hidden
/// phase. The control is the hide before it: the steady shapes blink.
/// Perturbation: leave the style out of what `Caret::present` compares, and
/// the frame at the change is hidden.
#[test]
fn a_change_of_shape_is_shown() {
    let ms = Duration::from_millis;
    let mut log = stream_with(Event::Read(b"\x1b[6 q".to_vec()), ms(1400));
    log.insert(0, (ms(2), Event::Read(b"\x1b[?12h\x1b[2 q".to_vec())));
    let drawn = blink_frames(log, ms(3000));
    let shown = baud_render::CursorPhase::SHOWN;
    assert_ne!(
        drawn_at(&drawn, ms(1390)).1,
        shown,
        "hidden before the change"
    );
    assert_eq!(drawn_at(&drawn, ms(1400)).1, shown, "shown at the change");
}

/// **Output that leaves the cursor hidden keeps it hidden**: a frame at
/// 1000 ms hides it, and every frame after shows it hidden, the blink
/// showing nothing -- each the same pixels as a frame from the hidden phase
/// before (300 ms), and none the pixels of a shown one (600 ms), which the
/// cursor makes differ: the control that the digest sees the cursor. The
/// stream draws the same spinner glyph each frame, so only the cursor can
/// change the pixels; the hiding frames are the multiplexer's without the
/// show at their end. Perturbation: the CPU path drawing the cursor
/// whatever `DECTCEM` says, and a frame after the hide is not the hidden
/// phase's pixels.
#[test]
fn output_that_hides_the_cursor_keeps_it_hidden() {
    let ms = Duration::from_millis;
    let mut log = stream(ms(1000), Cut::Whole, Spin::Still);
    let hide =
        b"\x1b[?2026h\x1b[?25l\x1b[7;37H\x1b[0;39;49m|\x1b[7;40H1\x1b[0m\x1b[8;30H\x1b[?2026l";
    let hiding = (0u32..)
        .map(|k| ms(1000) + ms(108) * k)
        .take_while(|&t| t < ms(3000));
    log.extend(hiding.map(|at| (at, Event::Read(hide.to_vec()))));
    let drawn = blink_frames(log, ms(3000));
    let (hidden, shown) = (drawn_at(&drawn, ms(300)), drawn_at(&drawn, ms(600)));
    assert_ne!(hidden.1, baud_render::CursorPhase::SHOWN, "300 ms hides it");
    assert_eq!(shown.1, baud_render::CursorPhase::SHOWN, "600 ms shows it");
    assert_ne!(hidden.2, shown.2, "the control: the digest sees the cursor");
    for d in drawn.iter().filter(|d| d.0 >= ms(1010)) {
        assert_eq!(d.2, hidden.2, "the cursor drawn at {:?}", d.0);
    }
}

/// **A shell printing at its prompt holds the cursor shown while it moves**,
/// as before: a letter every 100 ms for 3 s, each moving the cursor a column
/// on, restarts the cycle each time, so no frame hides it; after the last,
/// it blinks again within a cycle. Perturbation: leave the cell out of what
/// `Caret::present` compares, and it hides while the text is written.
#[test]
fn a_shell_printing_at_its_prompt_shows_the_cursor_as_it_moves() {
    let ms = Duration::from_millis;
    let (on, off) = default_phases();
    let mut log = vec![(ms(5), Event::Read(b"$ ".to_vec()))];
    let letters = (1u32..=30).map(|k| ms(100) * k);
    log.extend(letters.map(|at| (at, Event::Read(b"a".to_vec()))));
    let drawn = blink_frames(log, ms(3000) + on + off + ms(100));
    let shown = baud_render::CursorPhase::SHOWN;
    let writing = ms(100)..ms(3000) + on;
    assert!(
        drawn
            .iter()
            .filter(|d| writing.contains(&d.0))
            .all(|d| d.1 == shown),
        "a hide while the text is written"
    );
    assert!(
        drawn.iter().any(|d| d.0 >= writing.end && d.1 != shown),
        "the control: it blinks once the text stops"
    );
}
