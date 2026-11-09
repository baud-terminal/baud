//! **The link layer on the take, with no window**: the macOS app's printed
//! paths and links run over the README session's screen as it is played,
//! through the app's own sequences (`link_layer.rs`) -- each read sampled
//! for its writer with the process watch's verdict, the paths' worker ticked
//! and its answers merged, the lines at rest scanned every turn -- and what
//! they showed written down: each row's lit cells, and what a click on each
//! lit link would come to, decided and never acted on.
//!
//! **Lit is counted in frames, not in time**: each turn of the take is a
//! frame here, its answers merged before its lines are scanned, so a row is
//! lit by the first frame after the worker's answer for it, however long
//! the worker took. Wall time bounds nothing but the take itself.
//!
//! **What is written names no machine path**: the session's home is
//! written `~` and its scratch root `<root>`, so the dumps can sit beside
//! the stills.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use baud_core::Screen;
use baud_core::{Focus, Term};
use baud_domain::{Col, Row, Rows};
use baud_render::{AbsLine, LinkLine, Scrolled};

use crate::link::{Chord, Press, Spot};
use crate::link_layer::{Decision, Layer, Then};
use crate::paths::{Asked, Found, Origin};
use crate::probe::{Linked, Writers};

use super::session::StepAt;

/// A frame of the take: its turns, counted from the first.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub(crate) struct FrameNo(pub(crate) u64);

/// A count of a row's lit cells.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub(crate) struct Cells(pub(crate) usize);

/// One row of the screen, as the link layer showed it.
#[derive(Clone, Debug)]
pub(crate) struct RowDump {
    pub(crate) line: AbsLine,
    pub(crate) text: String,
    /// The lit runs, each `(from, to)` and what a click there comes to.
    pub(crate) lit: Vec<(Col, Col, Opens)>,
    pub(crate) struck: Vec<(Col, Col)>,
    pub(crate) origin: Origin,
}

/// **What a click on a lit link comes to**, as `link_layer::decide` and,
/// for a printed path, `paths::on_found` decide it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Opens {
    /// A printed path: its text, the worker's answer for it, and what a
    /// click on it found there would do.
    Path { text: String, found: String },
    /// A window running this argv.
    Window(Vec<String>),
    /// Anything else, in words: a command, a copy, an open, a refusal.
    Other(String),
    /// No link under the run's first cell.
    Nothing,
}

/// **A row's lit cells changing**, between two frames.
#[derive(Clone, Debug)]
pub(crate) struct Change {
    pub(crate) frame: FrameNo,
    pub(crate) step: Option<StepAt>,
    pub(crate) at: Duration,
    pub(crate) line: AbsLine,
    pub(crate) text: String,
    pub(crate) was: Cells,
    pub(crate) now: Cells,
    pub(crate) screen: Screen,
}

/// **The screen as one step left it.**
#[derive(Clone, Debug)]
pub(crate) struct Snapshot {
    pub(crate) step: StepAt,
    pub(crate) screen: Screen,
    pub(crate) rows: Vec<RowDump>,
}

/// **What the tap saw over the take.**
#[derive(Clone, Debug, Default)]
pub(crate) struct Report {
    /// The screen at the end of each step, by the step.
    pub(crate) snapshots: Vec<Snapshot>,
    /// Every change of a row's lit cells, with its frame.
    pub(crate) changes: Vec<Change>,
    /// A row that went darker while its text stayed and it stayed on the
    /// normal screen: what a prompt row going dark is.
    pub(crate) darkened: Vec<Change>,
    /// The frames whose turn merged a worker answer.
    pub(crate) answered: Vec<FrameNo>,
    /// Each row's text and lit count, by frame, for the rows a check names
    /// ([`Report::first_lit`]): the frame each text first showed, and the
    /// first it was lit in.
    pub(crate) seen: BTreeMap<String, (FrameNo, Option<FrameNo>)>,
    /// The lines written while each step ran, the first and last.
    pub(crate) written: BTreeMap<StepAt, (AbsLine, AbsLine)>,
    /// What a Command-click at each hover step's cell would take.
    pub(crate) hovered: Vec<(StepAt, Opens)>,
}

impl Report {
    /// Of the rows reading `text`, the one that showed first: the frame it
    /// showed in, and the first it was lit in.
    pub(crate) fn first_lit(&self, text: &str) -> Option<(FrameNo, Option<FrameNo>)> {
        self.seen
            .iter()
            .filter(|(t, _)| t.contains(text))
            .map(|(_, v)| *v)
            .min_by_key(|(shown, _)| *shown)
    }
}

/// **The system as the session's worker asks it**: the app's own worker,
/// `PATH` aside, which is the session's, as the app's worker has baud's:
/// a `[link.*]` rule's program is looked up along the `PATH` the shell
/// the take films was given.
///
/// **Every `Fs` method is written out**, none left to the trait's default:
/// a method `Fs` gains with a default would answer here with that default,
/// not the system's, and compile. `every_fs_method_is_the_systems` holds
/// the list to the trait's.
struct SessionFs {
    system: crate::probe::System,
    path: Option<std::ffi::OsString>,
}

/// The system as the worker of a session in `env` asks it.
fn session_fs(env: &[(std::ffi::OsString, std::ffi::OsString)]) -> SessionFs {
    SessionFs {
        system: crate::probe::System,
        path: env
            .iter()
            .find(|(k, _)| k == "PATH")
            .map(|(_, v)| v.clone()),
    }
}

impl crate::probe::Fs for SessionFs {
    fn lstat(&mut self, path: &[u8]) -> crate::probe::Stat {
        self.system.lstat(path)
    }
    fn readlink(&mut self, path: &[u8]) -> Option<Vec<u8>> {
        self.system.readlink(path)
    }
    fn mounts(&mut self) -> Option<crate::probe::Mounts> {
        self.system.mounts()
    }
    fn program(&mut self, group: crate::sys::Pid) -> Vec<String> {
        self.system.program(group)
    }
    fn folder(&mut self, group: crate::sys::Pid) -> Option<Vec<u8>> {
        self.system.folder(group)
    }
    fn started(&mut self, group: crate::sys::Pid) -> Option<crate::fg::StartTime> {
        self.system.started(group)
    }
    fn prepare(&mut self) {
        self.system.prepare();
    }
    fn handler(&mut self, address: &str) -> crate::paths::Handler {
        self.system.handler(address)
    }
    fn path_var(&mut self) -> Option<std::ffi::OsString> {
        self.path.clone()
    }
}

/// **The link layer of one take.**
pub(crate) struct Tap {
    dir: PathBuf,
    lines: crate::lined::Lines,
    uris: crate::lined::Uris,
    paths: crate::paths::Paths,
    config: crate::config::Link,
    probes: crate::probe::Probes<Instant>,
    sampler: crate::probe::Sampler,
    watched: crate::probe::WatchCell,
    /// The process watch on the child, whose verdict each read takes and
    /// whose moves ask what is struck again, as the app's pump does.
    watch: Option<crate::fg::Watch>,
    master: crate::sys::Fd,
    /// The session's environment, whose `PATH` a link's window would find
    /// its program on.
    env: Vec<(std::ffi::OsString, std::ffi::OsString)>,
    /// What a dump writes in place of each machine path, longest first.
    redact: Vec<(String, &'static str)>,
    /// What `reads.txt` writes in place of each machine path and of this
    /// machine's name, longest first.
    reads_redact: Vec<(String, &'static str)>,
    frame: FrameNo,
    /// Every read of the take, as it arrived.
    reads: Vec<ReadAt>,
    lit: BTreeMap<AbsLine, (String, Spans)>,
    step: Option<StepAt>,
    pub(crate) report: Report,
}

impl Tap {
    /// **A tap on the take whose child is `pid` on `master`**, its config
    /// the session's and its home the session's `HOME`, writing into `dir`.
    /// Set up as the app sets a window's up: the paths' rules and switches,
    /// this machine's name, the worker with the rules' programs, and a
    /// process watch on the child.
    pub(crate) fn new(
        dir: &Path,
        cfg: &crate::config::Config,
        env: &[(std::ffi::OsString, std::ffi::OsString)],
        master: crate::sys::Fd,
        pid: crate::sys::Pid,
    ) -> Tap {
        use std::os::unix::ffi::OsStrExt;
        let home = env
            .iter()
            .find(|(k, _)| k == "HOME")
            .map(|(_, v)| v.as_bytes().to_vec());
        let link = cfg.link.clone();
        let mut paths = crate::paths::Paths::new(home.clone(), link.rules.forms());
        paths.set_rules(link.rules.clone());
        paths.set(link.paths_read());
        let _ = paths.set_missing(link.missing_read());
        let _ = paths.set_join(link.join_read());
        // `gethostname`'s, which the shell's `OSC 7` names: the app adds
        // the process info's `hostName` where it differs, which is the
        // presenter's to read, and no film reads the presenter.
        let names: Vec<String> = crate::hostlog::machine_name().into_iter().collect();
        paths.set_names(names.clone());
        let mut uris = crate::lined::Uris::default();
        uris.set_rules(link.rules.clone());
        if let Some(host) = names.last() {
            uris.set_host(host.clone());
        }
        let session = env.to_vec();
        let mut probes = crate::probe::Probes::new(Box::new(move || {
            crate::probe::Worker::start(Box::new(session_fs(&session)), Box::new(|| {}))
        }));
        probes.set_programs(link.programs());
        let watched = crate::probe::WatchCell::default();
        let watch = crate::fg::Watch::start(None, watched.clone(), || {});
        if let Some(w) = &watch {
            w.add(pid);
        }
        std::fs::create_dir_all(dir).expect("the links' folder");
        clear_dumps(dir);
        let redact = home.as_deref().map(redactions).unwrap_or_default();
        let reads_redact = with_hosts(&redact, &names);
        Tap {
            dir: dir.to_path_buf(),
            lines: crate::lined::Lines::default(),
            uris,
            paths,
            config: link,
            probes,
            sampler: crate::probe::Sampler::default(),
            watched,
            watch,
            master,
            env: env.to_vec(),
            redact,
            reads_redact,
            frame: FrameNo::default(),
            reads: Vec::new(),
            lit: BTreeMap::new(),
            step: None,
            report: Report::default(),
        }
    }

    /// The session's step at the last turn.
    pub(crate) fn step(&self) -> Option<StepAt> {
        self.step
    }

    /// **At a read, before it is fed**: who wrote it, as the app's reader
    /// thread asks (`link_layer::read`).
    pub(crate) fn read(&mut self, bytes: &[u8]) -> (Writers, Linked) {
        heard(&mut self.reads, (self.frame, self.step), bytes);
        crate::link_layer::read(
            &mut self.sampler,
            Instant::now(),
            bytes,
            self.master,
            &self.watched,
        )
    }

    /// **After the read was fed**: each part of it, under the writers it
    /// was fed with, kept with the lines it wrote, from `began` to where the
    /// cursor is now (`link_layer::fed`).
    pub(crate) fn fed(
        &mut self,
        term: &Term<'_>,
        scrolled: Scrolled,
        began: AbsLine,
        parts: &[(Writers, Linked)],
    ) {
        let ended = scrolled.line(Rows::ZERO, term.cursor().0);
        crate::link_layer::fed(
            &mut self.probes,
            &mut self.paths,
            term,
            parts.iter().copied(),
            began,
            ended,
        );
        if let Some(s) = self.step {
            let w = self.report.written.entry(s).or_insert((began, ended));
            w.0 = w.0.min(began);
            w.1 = w.1.max(ended);
        }
    }

    /// **One turn of the take, a frame**: the worker's answers merged, what
    /// is struck asked about again when the process watch saw a move, its
    /// clock ticked, the lines at rest scanned, as the app does each; then
    /// what changed written down. `step` is the session's step now, and a
    /// change of it keeps the screen as the step before it left it.
    pub(crate) fn turn(
        &mut self,
        term: &Term<'_>,
        scrolled: Scrolled,
        step: Option<StepAt>,
        at: Duration,
    ) {
        let mut layer = Layer {
            term,
            scrolled,
            lines: &mut self.lines,
            uris: &mut self.uris,
            paths: &mut self.paths,
            config: &self.config,
        };
        let answered = crate::link_layer::answers(&mut self.probes, &mut layer);
        if self.watch.as_ref().map(crate::fg::Watch::take) == Some(crate::fg::Moved::Moved) {
            crate::link_layer::moved(&mut self.probes, layer.paths);
        }
        let _ = crate::link_layer::tick(
            &mut self.probes,
            &mut layer,
            Instant::now(),
            Some(self.master),
        );
        let _ = crate::link_layer::frame(&mut layer);
        self.frame = FrameNo(self.frame.0 + 1);
        if answered.frame == crate::paths::Owed::Yes {
            self.report.answered.push(self.frame);
        }
        if step != self.step {
            if let Some(prev) = self.step {
                self.keep(term, scrolled, prev);
            }
            self.step = step;
        }
        self.changes(term, scrolled, at);
    }

    /// **What a Command-click on `text` would take**, at step `step`: the
    /// link at its middle cell, found on the screen as it is now
    /// ([`cell_of`]), and what clicking it comes to; where the screen does
    /// not show it, that, in words.
    pub(crate) fn hover(&mut self, term: &Term<'_>, scrolled: Scrolled, step: StepAt, text: &str) {
        let opens = match cell_of(&cells(term), text) {
            Some((row, col)) => self.opens(term, scrolled, row, col),
            None => Opens::Other(format!("{text:?} is not on the screen")),
        };
        self.report.hovered.push((step, opens));
    }

    /// The lit cells now, by line, against the last frame's.
    fn changes(&mut self, term: &Term<'_>, scrolled: Scrolled, at: Duration) {
        let text = crate::programs::screen(term);
        let screen = term.screen();
        let mut now: BTreeMap<AbsLine, (String, Spans)> = BTreeMap::new();
        for (r, t) in text.iter().enumerate() {
            let line = scrolled.line(Rows::ZERO, Row::from_index(r));
            let spans = lit_spans(self.lines.lines(), Row::from_index(r));
            let seen = self
                .report
                .seen
                .entry(t.trim_end().to_string())
                .or_insert((self.frame, None));
            if !spans.is_empty() && seen.1.is_none() {
                seen.1 = Some(self.frame);
            }
            now.insert(line, (t.clone(), spans));
        }
        let (changes, darkened) = lit_changes(&self.lit, &now, screen, (self.frame, self.step, at));
        self.report.changes.extend(changes);
        self.report.darkened.extend(darkened);
        self.lit = now;
    }

    /// The screen as step `step` left it, kept.
    fn keep(&mut self, term: &Term<'_>, scrolled: Scrolled, step: StepAt) {
        let rows = self.rows(term, scrolled);
        self.report.snapshots.push(Snapshot {
            step,
            screen: term.screen(),
            rows,
        });
    }

    /// Every row of the screen, as the link layer shows it now.
    fn rows(&self, term: &Term<'_>, scrolled: Scrolled) -> Vec<RowDump> {
        let text = crate::programs::screen(term);
        let mut out = Vec::new();
        for (r, t) in text.iter().enumerate() {
            let row = Row::from_index(r);
            let line = scrolled.line(Rows::ZERO, row);
            let runs = |set: &[LinkLine]| -> Vec<(Col, Col)> {
                set.iter()
                    .filter(|l| l.row == row)
                    .map(|l| (l.from, l.to))
                    .collect()
            };
            let lit = runs(self.lines.lines())
                .into_iter()
                .map(|(from, to)| (from, to, self.opens(term, scrolled, row, from)))
                .collect();
            out.push(RowDump {
                line,
                text: t.trim_end().to_string(),
                lit,
                struck: runs(self.lines.struck()),
                origin: self.paths.origin(line),
            });
        }
        out
    }

    /// What a click on the link at `(row, col)` comes to, decided.
    fn opens(&self, term: &Term<'_>, scrolled: Scrolled, row: Row, col: Col) -> Opens {
        let spot = Spot {
            scrolled,
            back: term.window_back(),
            row,
            col,
        };
        let Some(under) = crate::link::under_with(
            term,
            spot,
            Press::Down,
            Chord::Cmd,
            Focus::Gained,
            None,
            &self.paths,
        ) else {
            return Opens::Nothing;
        };
        match crate::link_layer::decide(&under, term, &self.paths, &self.uris) {
            Decision::Path(p) => {
                let found = match self.paths.result(p.path()) {
                    Some(crate::paths::Probe::Exists) => {
                        let asked = Asked::Printed(p.line_col().clone());
                        match crate::link_layer::decide_found(&asked, p.path(), &self.uris) {
                            Found::Window(Ok(run)) => {
                                format!("window {:?}", window_argv(&run, &self.env))
                            }
                            Found::Window(Err(why)) => format!("refused {why:?}"),
                            Found::Command(Ok(c)) => format!("command {:?}", c.get_program()),
                            Found::Command(Err(why)) => format!("refused {why:?}"),
                            Found::Copy => "copy".to_string(),
                            Found::Reveal => "reveal".to_string(),
                        }
                    }
                    other => format!("{other:?}"),
                };
                Opens::Path {
                    text: self.redacted(&String::from_utf8_lossy(p.path())),
                    found: self.redacted(&found),
                }
            }
            Decision::Gone => Opens::Other("gone".to_string()),
            Decision::Unreadable(why, _) => Opens::Other(format!("unreadable {why:?}")),
            Decision::Read { then, .. } => match then {
                Then::Window(Some(Ok(run))) => Opens::Window(
                    window_argv(&run, &self.env)
                        .iter()
                        .map(|a| self.redacted(a))
                        .collect(),
                ),
                Then::Window(other) => Opens::Other(format!("window {:?}", other.map(|r| r.err()))),
                Then::Command(built) => Opens::Other(match built {
                    Some(Ok(c)) => format!(
                        "command {}",
                        self.redacted(&c.get_program().to_string_lossy())
                    ),
                    Some(Err(why)) => format!("refused {why:?}"),
                    None => "no command".to_string(),
                }),
                Then::Refused(why) => Opens::Other(format!("refused {why:?}")),
                Then::Copy(_) => Opens::Other("copy".to_string()),
                Then::Again(_) => Opens::Other("asked again".to_string()),
                Then::OpenOrReveal(t) => Opens::Other(format!("{:?} {:?}", t.action(), t.scheme())),
            },
        }
    }

    /// `text` with each machine path written as the dumps write it.
    fn redacted(&self, text: &str) -> String {
        redact(text, &self.redact)
    }

    /// **The dumps written**: one file a step, `NN-step.txt`, each row's
    /// line, origin, text, lit runs and what each opens; `changes.txt`,
    /// every change of a row's lit cells with its frame; `hovered.txt`;
    /// `reads.txt`, where the reads split ([`read_lines`]). The files
    /// written, for the names check.
    pub(crate) fn write(&mut self, term: &Term<'_>, scrolled: Scrolled) -> Vec<PathBuf> {
        if let Some(prev) = self.step {
            self.keep(term, scrolled, prev);
        }
        let mut written = Vec::new();
        for snap in &self.report.snapshots {
            let mut s = format!("step {} on the {:?} screen\n", snap.step.0, snap.screen);
            for r in &snap.rows {
                let _ = writeln!(
                    s,
                    "{:>14} {:<7} |{}|",
                    format!("{:?}", r.line),
                    format!("{:?}", r.origin),
                    self.redacted(&r.text)
                );
                for (from, to, opens) in &r.lit {
                    let _ = writeln!(
                        s,
                        "{:>14}   lit {}..{}: {opens:?}",
                        "",
                        from.index(),
                        to.index()
                    );
                }
                for (from, to) in &r.struck {
                    let _ = writeln!(s, "{:>14}   struck {}..{}", "", from.index(), to.index());
                }
            }
            let name = format!("{:02}-step.txt", snap.step.0);
            let to = self.dir.join(name);
            std::fs::write(&to, s).expect("a step's dump");
            written.push(to);
        }
        let mut c = String::new();
        for ch in &self.report.changes {
            let _ = writeln!(
                c,
                "frame {:>5} step {:>3} {:>8.3}s {:?} {:?} lit {} -> {} |{}|",
                ch.frame.0,
                ch.step.map_or("-".to_string(), |s| s.0.to_string()),
                ch.at.as_secs_f64(),
                ch.line,
                ch.screen,
                ch.was.0,
                ch.now.0,
                self.redacted(&ch.text)
            );
        }
        let _ = writeln!(c, "answers merged at frames: {:?}", self.report.answered);
        let to = self.dir.join("changes.txt");
        std::fs::write(&to, c).expect("the changes");
        written.push(to);
        let mut h = String::new();
        for (step, opens) in &self.report.hovered {
            let _ = writeln!(h, "step {}: {opens:?}", step.0);
        }
        let to = self.dir.join("hovered.txt");
        std::fs::write(&to, h).expect("the hovers");
        written.push(to);
        let to = self.dir.join("reads.txt");
        std::fs::write(&to, read_lines(&self.reads, &self.reads_redact)).expect("the reads");
        written.push(to);
        written
    }
}

/// **One read of the take, as it arrived**: the frame and the step it
/// came after, and its bytes, so a dump can show where the reads split.
#[derive(Clone, Debug)]
pub(crate) struct ReadAt {
    frame: FrameNo,
    step: Option<StepAt>,
    bytes: Vec<u8>,
}

/// How many bytes of each end of a long read `reads.txt` shows.
const EDGE: usize = 64;

/// **A read kept as its own**, after frame `at.0` in step `at.1`: never
/// joined to the one before it, as the boundary between two reads is what
/// the dump is for.
fn heard(reads: &mut Vec<ReadAt>, (frame, step): (FrameNo, Option<StepAt>), bytes: &[u8]) {
    reads.push(ReadAt {
        frame,
        step,
        bytes: bytes.to_vec(),
    });
}

/// **`reads.txt`**: a line a read, its frame, step and length, and its
/// bytes escaped -- all of them for a read of up to twice [`EDGE`], else
/// its first and last [`EDGE`] -- each machine path written as the other
/// dumps write it, before the read is cut.
fn read_lines(reads: &[ReadAt], redactions: &[(String, &'static str)]) -> String {
    let mut out = String::new();
    for r in reads {
        let b = redact_bytes(&r.bytes, redactions);
        let shown = match b.len() > 2 * EDGE {
            true => format!(
                "|{}| .. |{}|",
                escaped(&b[..EDGE]),
                escaped(&b[b.len() - EDGE..])
            ),
            false => format!("|{}|", escaped(&b)),
        };
        let _ = writeln!(
            out,
            "frame {:>5} step {:>3} len {:>5} {shown}",
            r.frame.0,
            r.step.map_or("-".to_string(), |s| s.0.to_string()),
            r.bytes.len()
        );
    }
    out
}

/// `bytes` with each machine path written as [`redactions`] says, the
/// bytes around them as they are.
fn redact_bytes(bytes: &[u8], redactions: &[(String, &'static str)]) -> Vec<u8> {
    let mut b = bytes.to_vec();
    for (from, to) in redactions {
        let from = from.as_bytes();
        if from.is_empty() {
            continue;
        }
        let mut out = Vec::with_capacity(b.len());
        let mut i = 0;
        while i < b.len() {
            if b[i..].starts_with(from) {
                out.extend_from_slice(to.as_bytes());
                i += from.len();
            } else {
                out.push(b[i]);
                i += 1;
            }
        }
        b = out;
    }
    b
}

/// A read's bytes as text: printable ASCII as it is, `\`, `|` and the
/// controls a session sends by name, every other byte as `\xNN`.
fn escaped(bytes: &[u8]) -> String {
    let mut s = String::new();
    for &b in bytes {
        match b {
            b'\\' => s.push_str("\\\\"),
            b'|' => s.push_str("\\|"),
            b'\n' => s.push_str("\\n"),
            b'\r' => s.push_str("\\r"),
            b'\t' => s.push_str("\\t"),
            0x07 => s.push_str("\\a"),
            0x1b => s.push_str("\\e"),
            0x20..=0x7e => s.push(char::from(b)),
            _ => {
                let _ = write!(s, "\\x{b:02x}");
            }
        }
    }
    s
}

/// A row's lit runs, each `(from, to)`, by their first column.
type Spans = Vec<(Col, Col)>;

/// **A row's lit runs, against the frame before**, each row by its line:
/// every change of a row's runs, and those that are a row going darker --
/// a run lit before and not now -- while its text stays and it stays on
/// the normal screen, however many cells other runs lit in the same frame.
/// A row that left the window, or whose text changed, or the alternate
/// screen's, is no darkening. Each change carries the frame, the step and
/// the take's time it was seen at, and the row's lit cells before and now.
fn lit_changes(
    before: &BTreeMap<AbsLine, (String, Spans)>,
    now: &BTreeMap<AbsLine, (String, Spans)>,
    screen: Screen,
    (frame, step, at): (FrameNo, Option<StepAt>, Duration),
) -> (Vec<Change>, Vec<Change>) {
    let (mut changes, mut darkened) = (Vec::new(), Vec::new());
    for (line, (t, spans)) in now {
        let was = before.get(line).map_or(&[][..], |(_, s)| s.as_slice());
        if was == spans.as_slice() {
            continue;
        }
        let change = Change {
            frame,
            step,
            at,
            line: *line,
            text: t.trim_end().to_string(),
            was: lit_count(was),
            now: lit_count(spans),
            screen,
        };
        let same = before.get(line).is_some_and(|(old, _)| old == t);
        let lost = was.iter().any(|s| !spans.contains(s));
        if lost && same && screen == Screen::Normal {
            darkened.push(change.clone());
        }
        changes.push(change);
    }
    (changes, darkened)
}

/// The screen's characters, one to a cell: each cell's base, a cell with
/// none, a wide character's tail among them, a space. What
/// [`crate::programs::screen`] reads, its marks aside, so that a column
/// here is a column on the grid.
fn cells(term: &Term<'_>) -> Vec<Vec<char>> {
    let v = term.window();
    (0..v.rows().index())
        .map(|y| {
            v.row(Row::from_index(y))
                .iter()
                .map(|c| {
                    let base = term.base_of(*c).as_codepoint().map(|cp| cp.value());
                    base.and_then(char::from_u32).unwrap_or(' ')
                })
                .collect()
        })
        .collect()
}

/// **Where `text` is on the screen, `rows` one character to a cell**: the
/// first row holding it and the column of its middle cell, which a click
/// on it would take; none where no row holds it.
fn cell_of(rows: &[Vec<char>], text: &str) -> Option<(Row, Col)> {
    let want: Vec<char> = text.chars().collect();
    if want.is_empty() {
        return None;
    }
    rows.iter().enumerate().find_map(|(r, row)| {
        row.windows(want.len())
            .position(|w| w == want.as_slice())
            .map(|c| (Row::from_index(r), Col::at_index(c + want.len() / 2)))
    })
}

/// The runs of `row` the lines light, by their first column.
fn lit_spans(set: &[LinkLine], row: Row) -> Spans {
    let mut spans: Spans = set
        .iter()
        .filter(|l| l.row == row)
        .map(|l| (l.from, l.to))
        .collect();
    spans.sort();
    spans
}

/// How many cells the runs light, each `(from, to)` inclusive.
fn lit_count(spans: &[(Col, Col)]) -> Cells {
    Cells(
        spans
            .iter()
            .map(|(from, to)| to.index().saturating_sub(from.index()) + 1)
            .sum(),
    )
}

/// **A link's window's argv, as it would run**: the program found as a
/// window finds it on `env`'s `PATH` (`pty::window_program`), `<not
/// found>` when it is not there, and the rest as the rule built them.
pub(crate) fn window_argv(
    run: &crate::rules::Run,
    env: &[(std::ffi::OsString, std::ffi::OsString)],
) -> Vec<String> {
    let program = crate::pty::window_program(run, env).unwrap_or_else(|| "<not found>".to_string());
    std::iter::once(program)
        .chain(
            run.argv()
                .iter()
                .skip(1)
                .map(|a| a.to_string_lossy().into_owned()),
        )
        .collect()
}

/// **What a dump writes in place of each machine path**, a session's home
/// `home`: each spelling of the home `~` and of its parent, the scratch
/// root, `<root>`, longest first, so a resolved home is not read as a
/// spelling of the root with a home after it.
fn redactions(home: &[u8]) -> Vec<(String, &'static str)> {
    let h = String::from_utf8_lossy(home)
        .trim_end_matches('/')
        .to_string();
    let mut out: Vec<(String, &'static str)> =
        spellings(&h).into_iter().map(|s| (s, "~")).collect();
    if let Some(root) = Path::new(&h).parent() {
        out.extend(
            spellings(&root.to_string_lossy())
                .into_iter()
                .map(|s| (s, "<root>")),
        );
    }
    out.sort_by_key(|(s, _)| std::cmp::Reverse(s.len()));
    out
}

/// **What `reads.txt` writes in place of each machine path and this
/// machine's name**: `paths`, and each of `names` with the part of it
/// before its first `.`, of three bytes or more, as `<host>`, longest
/// first. The raw reads carry the name in every `OSC 7` the shell sends
/// (`file://<host>/...`), which the screen never shows.
fn with_hosts(paths: &[(String, &'static str)], names: &[String]) -> Vec<(String, &'static str)> {
    let mut out = paths.to_vec();
    for n in names {
        let short = n.split('.').next().unwrap_or(n);
        for s in [n.as_str(), short] {
            if s.len() >= 3 && !out.iter().any(|(f, _)| f == s) {
                out.push((s.to_string(), "<host>"));
            }
        }
    }
    out.sort_by_key(|(s, _)| std::cmp::Reverse(s.len()));
    out
}

/// `text` with each machine path written as [`redactions`] says.
fn redact(text: &str, redactions: &[(String, &'static str)]) -> String {
    let mut t = text.to_string();
    for (from, to) in redactions {
        t = t.replace(from.as_str(), to);
    }
    t
}

/// Whether a file of the links' folder is one of the dumps: a step's, the
/// changes or the hovers.
fn is_dump(name: &str) -> bool {
    name.ends_with("-step.txt")
        || name == "changes.txt"
        || name == "hovered.txt"
        || name == "reads.txt"
}

/// **The dumps an earlier take left in `dir`, removed**, so a step this
/// take never reached is not read as its own; any other file stays.
fn clear_dumps(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        if e.file_name().to_str().is_some_and(is_dump) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Every spelling of a folder: as given and as the file system resolves it.
fn spellings(dir: &str) -> Vec<String> {
    let mut out = vec![dir.to_string()];
    if let Ok(real) = std::fs::canonicalize(dir) {
        let real = real.to_string_lossy().trim_end_matches('/').to_string();
        if real != dir {
            out.push(real);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use crate::no_appkit::{Follow, Marked, Part, Walk, hits, reach};

    /// What would open or start something: a process started, an effect
    /// asked of the OS, a file shown, a URL opened.
    /// In halves, so this file does not match itself. A thread's `spawn`
    /// takes its closure, a process's nothing. The calls that start a
    /// process: std's, the system's, and the terminal's own, from `pty.rs`.
    const OPENS: [&str; 18] = [
        concat!(".spawn", "()"),
        concat!(".status", "()"),
        concat!(".output", "()"),
        concat!("posix_", "spawn"),
        concat!("exec", "vp"),
        concat!("exec", "ve("),
        concat!("fork", "("),
        concat!("fork", "pty("),
        concat!("fork_", "with("),
        concat!("fork_", "child("),
        concat!("fork_", "shell("),
        concat!("Pty::", "spawn"),
        concat!("spawn_", "given("),
        concat!("spawn_", "polled("),
        concat!("activateFile", "Viewer"),
        concat!("open", "URL"),
        concat!("os.", "ask("),
        concat!("OsEffect", "::"),
    ];

    /// **The tap's worker looks a rule's program up along the session's
    /// `PATH`**, not the process's: a program only the session's `PATH`
    /// holds is found, as a window started with that `PATH` would find it.
    /// The control: along the process's `PATH` it is not found.
    #[test]
    fn the_tap_looks_programs_up_on_the_sessions_path() {
        use crate::paths::Handler;
        use crate::probe::Fs as _;
        use std::os::unix::fs::PermissionsExt as _;
        let dir = std::env::temp_dir().join(format!("tap-path-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dir = std::fs::canonicalize(&dir).unwrap();
        let prog = "baud-tap-stand-in";
        let exe = dir.join(prog);
        std::fs::write(&exe, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let env = vec![("PATH".into(), dir.clone().into_os_string())];
        let mut fs = crate::film::tap::session_fs(&env);
        let session = fs.path_var();
        let mounts = fs.mounts().expect("the mount table");
        let mut look = |path: Option<&std::ffi::OsStr>| {
            crate::probe::command(&mut fs, &mounts, (prog, path), &mut 0, &|| {})
        };
        let found = look(session.as_deref());
        let process = look(std::env::var_os("PATH").as_deref());
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(session.as_deref(), Some(dir.as_os_str()));
        assert_eq!(found, Handler::Found);
        assert_ne!(process, Handler::Found, "the control");
    }

    /// **The hovered link is found by its text, in cells**: on the row that
    /// holds it, at its middle cell, counted one to a cell past a prompt's
    /// three-byte `❯` and a wide character's two cells; none where no row
    /// holds it.
    #[test]
    fn a_hovered_link_is_found_by_its_text() {
        use baud_domain::{Col, Row};
        let rows: Vec<Vec<char>> = ["", "~/baud ❯ 界  https://nixos.org (Nix)"]
            .iter()
            .map(|r| r.chars().collect())
            .collect();
        // `界` is a cell and its tail a space: the URL starts at column 12.
        assert_eq!(
            crate::film::tap::cell_of(&rows, "https://nixos.org"),
            Some((Row::from_index(1), Col::at_index(12 + 8)))
        );
        assert_eq!(
            crate::film::tap::cell_of(&rows, "https://example.org"),
            None
        );
    }

    /// **The dumps name no machine path, and are this take's alone**: a
    /// session's home and its scratch root, each as given and as the file
    /// system resolves it, are written `~` and `<root>`, the resolved home
    /// as `~`, not as the root with a home after it; and an earlier take's
    /// dumps are removed while another file of the folder stays.
    #[test]
    fn the_dumps_are_redacted_and_cleared() {
        let root = std::env::temp_dir().join(format!("tap-dumps-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let home = root.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let real = std::fs::canonicalize(&home).unwrap();
        let (home_s, real_s) = (home.to_string_lossy(), real.to_string_lossy());
        let given = root.to_string_lossy();
        let redactions = crate::film::tap::redactions(home_s.as_bytes());
        let text = format!("{real_s}/bin/demo-open {home_s}/x {given}/stills");
        assert_eq!(
            crate::film::tap::redact(&text, &redactions),
            "~/bin/demo-open ~/x <root>/stills"
        );
        let links = root.join("links");
        std::fs::create_dir_all(&links).unwrap();
        for f in ["99-step.txt", "changes.txt", "hovered.txt", "notes.md"] {
            std::fs::write(links.join(f), "earlier").unwrap();
        }
        crate::film::tap::clear_dumps(&links);
        let mut left: Vec<String> = std::fs::read_dir(&links)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(left, ["notes.md"]);
    }

    /// **`reads.txt` holds each read's boundary**: two reads fed in one
    /// frame, an ssh's `logout` and the shell's `D` mark with the prompt
    /// after it, are two lines, each with its own length and bytes, the
    /// controls escaped; a long read shows its two ends, a machine path
    /// written `~` before it is cut. Named break: a read joined to the one
    /// before it in the same frame.
    #[test]
    fn the_reads_dump_holds_each_boundary() {
        use crate::film::session::StepAt;
        use crate::film::tap::{FrameNo, heard, read_lines};
        let mut reads = Vec::new();
        let at = (FrameNo(3), Some(StepAt(7)));
        heard(&mut reads, at, b"logout\r\n");
        heard(
            &mut reads,
            at,
            "\x1b]133;D;0\x07~/baud \u{276f} ".as_bytes(),
        );
        let home = "/scratch/demo/home";
        let long = format!("{home}/README.md{}|end", "x".repeat(200));
        heard(&mut reads, (FrameNo(4), None), long.as_bytes());
        let redactions = vec![(home.to_string(), "~")];
        let lines = read_lines(&reads, &redactions);
        let lines: Vec<&str> = lines.lines().collect();
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert_eq!(lines[0], "frame     3 step   7 len     8 |logout\\r\\n|");
        assert_eq!(
            lines[1],
            "frame     3 step   7 len    21 |\\e]133;D;0\\a~/baud \\xe2\\x9d\\xaf |"
        );
        let tail = format!("{}\\|end", "x".repeat(60));
        assert_eq!(
            lines[2],
            format!(
                "frame     4 step   - len   {} |~/README.md{}| .. |{tail}|",
                long.len(),
                "x".repeat(53)
            )
        );
    }

    /// **`reads.txt` names no host**: a read holding the shell's `OSC 7`,
    /// `file://` this machine's name, is written with the name and its
    /// first label as `<host>`, and passes the names check over both,
    /// while its length stays the read's own. Named break: no host
    /// written in place.
    #[test]
    fn the_reads_dump_names_no_host() {
        use crate::film::tap::{FrameNo, heard, read_lines, with_hosts};
        let host = "studio-mac.example.net";
        let read = format!("\x1b]7;file://{host}/scratch/demo/home\x1b\\");
        let mut reads = Vec::new();
        heard(&mut reads, (FrameNo(9), None), read.as_bytes());
        let paths = vec![("/scratch/demo/home".to_string(), "~")];
        let lines = read_lines(&reads, &with_hosts(&paths, &[host.to_string()]));
        // The names check's search (`film::named`), as fixed strings; named
        // here, as the tap's walk would read the film's module whole.
        for name in [host, "studio-mac"] {
            assert!(!lines.contains(name), "{name} in {lines}");
        }
        assert_eq!(
            lines,
            format!(
                "frame     9 step   - len {:>5} |\\e]7;file://<host>~\\e\\\\|\n",
                read.len()
            )
        );
    }

    /// **The row a link check reads is the first to show it**, not the
    /// first by its text: of two rows holding the link, the one shown at
    /// frame 5 is read, though the other sorts first.
    #[test]
    fn the_first_row_to_show_a_link_is_read() {
        use crate::film::tap::{FrameNo, Report};
        let mut r = Report::default();
        r.seen.insert(
            "a [Nix](https://nixos.org)".to_string(),
            (FrameNo(20), Some(FrameNo(20))),
        );
        r.seen.insert(
            "b [Nix](https://nixos.org)".to_string(),
            (FrameNo(5), Some(FrameNo(7))),
        );
        r.seen
            .insert("c".to_string(), (FrameNo(1), Some(FrameNo(1))));
        assert_eq!(
            r.first_lit("https://nixos.org"),
            Some((FrameNo(5), Some(FrameNo(7))))
        );
        assert_eq!(r.first_lit("https://example.org"), None);
    }

    /// **The darkening detector, over two frames**: a prompt row that
    /// keeps its text and loses its lit run is a row gone dark, and so is
    /// one whose run went dark while another lit the same number of cells
    /// in the same frame. The controls: the same row with its text changed,
    /// the same on the alternate screen, and a scroll that takes a lit row
    /// out of the window and moves another, its text and runs kept, are
    /// not; a row lighting, and a second run lighting beside a kept one,
    /// are changes and no darkening.
    #[test]
    fn a_row_that_keeps_its_text_and_loses_a_run_went_dark() {
        use crate::film::tap::{FrameNo, Spans, lit_changes};
        use baud_core::Screen;
        use baud_domain::{Col, Rows};
        use baud_render::AbsLine;
        use std::collections::BTreeMap;
        use std::time::Duration;
        type Frame = BTreeMap<AbsLine, (String, Spans)>;
        // A row: its line, its text and its lit runs, `(from, to)`.
        type Shown<'a> = (usize, &'a str, &'a [(usize, usize)]);
        let line = |n: usize| AbsLine::at(Rows::of_count(n));
        let frame = |rows: &[Shown<'_>]| -> Frame {
            rows.iter()
                .map(|&(n, t, s)| {
                    let spans = s
                        .iter()
                        .map(|&(a, b)| (Col::at_index(a), Col::at_index(b)))
                        .collect();
                    (line(n), (t.to_string(), spans))
                })
                .collect()
        };
        let when = (FrameNo(2), None, Duration::from_millis(40));
        let prompt = "~/baud ❯ cat ~/README.md ~/notes.md";
        let ls = "~/baud ❯ ls README.md";
        let (readme, notes) = (&[(13, 23)][..], &[(25, 34)][..]);
        let before = frame(&[(0, ls, &[(12, 20)]), (1, prompt, readme)]);
        let dark = |after: &Frame, screen: Screen| {
            let (changes, darkened) = lit_changes(&before, after, screen, when);
            (
                changes.len(),
                darkened.iter().map(|c| c.line).collect::<Vec<_>>(),
            )
        };
        let kept = (0, ls, &[(12, 20)][..]);
        assert_eq!(
            dark(&frame(&[kept, (1, prompt, &[])]), Screen::Normal),
            (1, vec![line(1)]),
            "a prompt row gone dark"
        );
        assert_eq!(
            dark(&frame(&[kept, (1, prompt, &[(0, 10)])]), Screen::Normal),
            (1, vec![line(1)]),
            "a run gone dark while another lit as many cells"
        );
        assert_eq!(
            dark(&frame(&[kept, (1, "~/baud ❯ ", &[])]), Screen::Normal),
            (1, vec![]),
            "the control: its text changed"
        );
        assert_eq!(
            dark(&frame(&[kept, (1, prompt, &[])]), Screen::Alternate),
            (1, vec![]),
            "the control: the alternate screen"
        );
        assert_eq!(
            dark(
                &frame(&[(1, prompt, readme), (2, "~/baud ❯ ", &[])]),
                Screen::Normal
            ),
            (0, vec![]),
            "the control: a scroll"
        );
        assert_eq!(
            dark(
                &frame(&[kept, (1, prompt, readme), (2, "~/baud ❯ ls", &[(9, 10)])]),
                Screen::Normal
            ),
            (1, vec![]),
            "the control: a row lighting"
        );
        assert_eq!(
            dark(
                &frame(&[kept, (1, prompt, &[readme[0], notes[0]])]),
                Screen::Normal
            ),
            (1, vec![]),
            "the control: a second run lighting"
        );
    }

    /// **Every method of the worker's `Fs` is the session's own**: each
    /// `fn` the trait declares (`probe.rs`) is written out in `SessionFs`'s
    /// impl, as one the trait gains with a default would compile there and
    /// answer with the default. The control: the trait's list holds the
    /// methods with defaults, `handler` and `path_var` among them.
    #[test]
    fn every_fs_method_is_the_systems() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let read = |f: &str| std::fs::read_to_string(src.join(f)).expect("a module");
        // The `fn` names of the block that opens at `head`, to its closing
        // brace at the start of a line.
        let fns = |text: &str, head: &str| -> Vec<String> {
            let at = text.find(head).unwrap_or_else(|| panic!("{head}"));
            let block = &text[at..];
            let block = &block[..block.find("\n}").expect("the block's end")];
            let mut names: Vec<String> = block
                .lines()
                .filter_map(|l| l.trim_start().strip_prefix("fn "))
                .map(|l| l[..l.find(['(', '<']).expect("a fn's name")].to_string())
                .collect();
            names.sort();
            names
        };
        let fs = fns(&read("probe.rs"), concat!("pub trait ", "Fs: Send {"));
        for default in ["handler", "path_var", "folder", "started", "prepare"] {
            assert!(fs.iter().any(|f| f == default), "the control: {default}");
        }
        let session = fns(
            &read("film/tap.rs"),
            concat!("impl crate::probe::Fs ", "for SessionFs {"),
        );
        assert_eq!(session, fs);
    }

    /// **The tap opens nothing and names no AppKit**: from `tap.rs` and
    /// `link_layer.rs`, every module they reach, comments, literals and
    /// other modules' unit tests aside, names none of `no_appkit`'s four
    /// names -- an application among them -- and starts, asks the OS for or
    /// shows nothing: no process started, by std, by the system's `fork`
    /// and `execve` or by the terminal's own entry points. The items
    /// `#[cfg(test)]` marks are walked, as the tap is test code and calls
    /// them. The worker's question whether an app opens a scheme stays,
    /// allowed as the launch test allows it: a LaunchServices lookup that
    /// opens nothing. The controls: the walk reaches the worker, the rules,
    /// the paths with their test-only items, the harness's screen reader,
    /// the window's program and the system's calls, each module of the last
    /// three followed fn by fn; the session it reads, for its step type
    /// alone, is named and not read.
    #[test]
    fn the_tap_opens_nothing() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let file = |p: &[&str]| Part::File(p.iter().map(|s| s.to_string()).collect());
        let reached = reach(&Walk {
            src: &src,
            start: vec![file(&["film", "tap"]), file(&["link_layer"])],
            // Fn by fn: the test harness, whose screen reader alone the
            // tap calls, by its free fns, as its methods' names (`start`,
            // `rows`, `read`) are other modules' too; the terminal's
            // process code, whose window program alone it calls; and the
            // system's calls, a test's C probe among them.
            by_fn: &[
                (&["programs"], Follow::FreeFns),
                (&["pty"], Follow::AllFns),
                (&["sys"], Follow::AllFns),
            ],
            // The session's step type, named and never run.
            sinks: &[&["film", "session"]],
            // The tap is test code and calls test-only items.
            marked: Marked::Walked,
        });
        let named: Vec<&str> = reached
            .sink_items
            .iter()
            .map(|(_, item, _)| item.as_str())
            .collect();
        assert!(
            named.iter().all(|i| *i == "StepAt"),
            "the session is named for {named:?}"
        );
        assert!(
            reached.parts.contains_key(&Part::Fn(
                vec!["programs".to_string()],
                "screen".to_string()
            )),
            "the control: the harness's screen is read"
        );
        for m in [&["probe"][..], &["rules"], &["paths"], &["link"]] {
            assert!(
                reached.parts.contains_key(&file(m)),
                "the control: {m:?} is reached"
            );
        }
        let of = |m: &str, f: &str| Part::Fn(vec![m.to_string()], f.to_string());
        assert!(
            reached.parts.contains_key(&of("pty", "window_program")),
            "the control: the window's program is read"
        );
        assert!(
            reached.parts.contains_key(&of("sys", "mounted")),
            "the control: the system's calls are followed"
        );
        assert!(
            reached.parts[&file(&["paths"])].contains(concat!("fn res", "ult(")),
            "the control: the test-only items the tap calls are walked"
        );
        let found = hits(&reached, &OPENS, &[crate::no_appkit::LINK_WORKER]);
        assert_eq!(found, Vec::<String>::new());
    }
}
