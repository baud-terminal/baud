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

use crate::link::{Chord, Host, Press, Spot, Under};
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
    /// The first frame a row reading `text` showed, and the first it was
    /// lit in.
    pub(crate) fn first_lit(&self, text: &str) -> Option<(FrameNo, Option<FrameNo>)> {
        self.seen
            .iter()
            .find(|(t, _)| t.contains(text))
            .map(|(_, v)| *v)
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
    _watch: Option<crate::fg::Watch>,
    master: crate::sys::Fd,
    /// The session's environment, whose `PATH` a link's window would find
    /// its program on.
    env: Vec<(std::ffi::OsString, std::ffi::OsString)>,
    /// What a dump writes in place of each machine path, longest first.
    redact: Vec<(String, &'static str)>,
    frame: FrameNo,
    lit: BTreeMap<AbsLine, (String, Cells)>,
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
        let mut probes = crate::probe::Probes::system(|| {});
        probes.set_programs(link.programs());
        let watched = crate::probe::WatchCell::default();
        let watch = crate::fg::Watch::start(None, watched.clone(), || {});
        if let Some(w) = &watch {
            w.add(pid);
        }
        std::fs::create_dir_all(dir).expect("the links' folder");
        let mut redact = Vec::new();
        if let Some(h) = &home {
            let h = String::from_utf8_lossy(h).trim_end_matches('/').to_string();
            for spelling in spellings(&h) {
                redact.push((spelling, "~"));
            }
            if let Some(root) = Path::new(&h).parent() {
                for spelling in spellings(&root.to_string_lossy()) {
                    redact.push((spelling, "<root>"));
                }
            }
        }
        redact.sort_by_key(|(s, _)| std::cmp::Reverse(s.len()));
        Tap {
            dir: dir.to_path_buf(),
            lines: crate::lined::Lines::default(),
            uris,
            paths,
            config: link,
            probes,
            sampler: crate::probe::Sampler::default(),
            watched,
            _watch: watch,
            master,
            env: env.to_vec(),
            redact,
            frame: FrameNo::default(),
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
        crate::link_layer::read(
            &mut self.sampler,
            Instant::now(),
            bytes,
            self.master,
            &self.watched,
        )
    }

    /// **After the read was fed**: its writers kept with the lines it wrote,
    /// from `began` to where the cursor is now (`link_layer::fed`).
    pub(crate) fn fed(
        &mut self,
        term: &Term<'_>,
        scrolled: Scrolled,
        began: AbsLine,
        read: (Writers, Linked),
    ) {
        let ended = scrolled.line(Rows::ZERO, term.cursor().0);
        crate::link_layer::fed(
            &mut self.probes,
            &mut self.paths,
            [read],
            crate::paths::LineSpan::of(began, ended),
        );
        if let Some(s) = self.step {
            let w = self.report.written.entry(s).or_insert((began, ended));
            w.0 = w.0.min(began);
            w.1 = w.1.max(ended);
        }
    }

    /// **One turn of the take, a frame**: the worker's answers merged, its
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
        let _ = crate::link_layer::tick(&mut self.probes, &mut layer, Instant::now(), self.master);
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

    /// **What a Command-click at `(row, col)` would take**, at step `step`:
    /// the link there and what clicking it comes to.
    pub(crate) fn hover(
        &mut self,
        term: &Term<'_>,
        scrolled: Scrolled,
        step: StepAt,
        row: Row,
        col: Col,
    ) {
        let line = scrolled.line(Rows::ZERO, row);
        let opens = self.opens(term, scrolled, row, col, line);
        self.report.hovered.push((step, opens));
    }

    /// The lit cells now, by line, against the last frame's.
    fn changes(&mut self, term: &Term<'_>, scrolled: Scrolled, at: Duration) {
        let text = crate::programs::screen(term);
        let screen = term.screen();
        let mut now: BTreeMap<AbsLine, (String, Cells)> = BTreeMap::new();
        for (r, t) in text.iter().enumerate() {
            let line = scrolled.line(Rows::ZERO, Row::from_index(r));
            let cells = lit_cells(self.lines.lines(), Row::from_index(r));
            now.insert(line, (t.clone(), cells));
            let seen = self
                .report
                .seen
                .entry(t.trim_end().to_string())
                .or_insert((self.frame, None));
            if cells > Cells(0) && seen.1.is_none() {
                seen.1 = Some(self.frame);
            }
        }
        for (line, (t, cells)) in &now {
            let was = self.lit.get(line).map_or(Cells(0), |(_, c)| *c);
            if was == *cells {
                continue;
            }
            let change = Change {
                frame: self.frame,
                step: self.step,
                at,
                line: *line,
                text: t.trim_end().to_string(),
                was,
                now: *cells,
                screen,
            };
            let same = self.lit.get(line).is_some_and(|(old, _)| old == t);
            if *cells < was && same && screen == Screen::Normal {
                self.report.darkened.push(change.clone());
            }
            self.report.changes.push(change);
        }
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
                .map(|(from, to)| (from, to, self.opens(term, scrolled, row, from, line)))
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
    fn opens(
        &self,
        term: &Term<'_>,
        scrolled: Scrolled,
        row: Row,
        col: Col,
        line: AbsLine,
    ) -> Opens {
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
        match crate::link_layer::decide(&under, line, term, &self.paths, &self.uris) {
            Decision::Path => match &under {
                Under::Path(p) => {
                    let host = Host(self.uris.host());
                    let found = match self.paths.result(p.path()) {
                        Some(crate::paths::Probe::Exists) => {
                            let asked = Asked::Printed(p.line_col().clone());
                            match crate::paths::on_found(&asked, p.path(), host, self.uris.rules())
                            {
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
                _ => Opens::Nothing,
            },
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
        let mut t = text.to_string();
        for (from, to) in &self.redact {
            t = t.replace(from.as_str(), to);
        }
        t
    }

    /// **The dumps written**: one file a step, `NN-step.txt`, each row's
    /// line, origin, text, lit runs and what each opens; `changes.txt`,
    /// every change of a row's lit cells with its frame; `hovered.txt`. The
    /// files written, for the names check.
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
        written
    }
}

/// How many cells of `row` the runs light.
fn lit_cells(set: &[LinkLine], row: Row) -> Cells {
    Cells(
        set.iter()
            .filter(|l| l.row == row)
            .map(|l| l.to.index().saturating_sub(l.from.index()) + 1)
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
    use crate::no_appkit::{Follow, Part, Walk, hits, reach};

    /// What would open or start something: a process started, an effect
    /// asked of the OS, a file shown, a URL opened.
    /// In halves, so this file does not match itself. A thread's `spawn`
    /// takes its closure, a process's nothing.
    const OPENS: [&str; 9] = [
        concat!(".spawn", "()"),
        concat!(".status", "()"),
        concat!(".output", "()"),
        concat!("posix_", "spawn"),
        concat!("exec", "vp"),
        concat!("activateFile", "Viewer"),
        concat!("open", "URL"),
        concat!("os.", "ask("),
        concat!("OsEffect", "::"),
    ];

    /// **The tap opens nothing and names no AppKit**: from `tap.rs` and
    /// `link_layer.rs`, every module they reach, comments, literals and
    /// other modules' unit tests aside, names none of `no_appkit`'s four
    /// names -- an application among them -- and starts, asks the OS for or
    /// shows nothing. The worker's question whether an app opens a scheme
    /// stays, allowed as the launch test allows it: a LaunchServices lookup
    /// that opens nothing. The control: the walk reaches the worker, the
    /// rules, the paths and the harness's screen reader, followed fn by fn as
    /// the test harness it is; the session it reads, for its step type alone,
    /// is named and not read.
    #[test]
    fn the_tap_opens_nothing() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let file = |p: &[&str]| Part::File(p.iter().map(|s| s.to_string()).collect());
        let reached = reach(&Walk {
            src: &src,
            start: vec![file(&["film", "tap"]), file(&["link_layer"])],
            // The test harness, whose screen reader alone the tap calls.
            by_fn: &[(&["programs"], Follow::AllFns)],
            // The session's step type, named and never run.
            sinks: &[&["film", "session"]],
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
        let found = hits(&reached, &OPENS, &[crate::no_appkit::LINK_WORKER]);
        assert_eq!(found, Vec::<String>::new());
    }
}
