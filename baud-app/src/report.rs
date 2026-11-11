//! **The five-second report**: what a driver's frames did, logged at
//! `info` once five seconds of its clock have passed since the last -- the
//! macOS app's from every display-link tick, the Linux driver's from every
//! turn of its loop. One shape on both, each count a number before its
//! name, so a reader that takes the line as number-and-name pairs parses
//! both; the first line names the driver's wakes, `ticks` on macOS and
//! `turns` on Linux, and its frames and what the last frame cost, then a
//! line for each count that has moved -- the clipboard's writes, the page
//! width's requests and the links activated.
//!
//! **What a driver has no source for is left out, not printed as zero.**
//! The Linux driver draws on the CPU only, so the GPU line's bind and
//! commit times and the presenter's counts are the macOS app's alone, and
//! so is the key-latency line. And it has no display link: where macOS
//! counts the ticks, which run whether or not anything changed, Linux
//! counts the turns of its loop -- each return from its one wait, the
//! report's own wake among them -- so a turn that built no frame is the
//! Linux counterpart of an idle tick.

use crate::bell::Moment;
use crate::clip::Clips;
use crate::width::Widths;
use baud_render::{Frame, Tally};
use std::time::Duration;

/// How often the report is logged.
pub const EVERY: Duration = Duration::from_secs(5);

/// Whether the report is due.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Due {
    Yes,
    No,
}

/// When the report was last logged, on the driver's clock.
pub struct Every<T> {
    last: T,
}

impl<T: Moment> Every<T> {
    /// A report first due five seconds after `now`.
    pub fn new(now: T) -> Every<T> {
        Every { last: now }
    }

    /// Whether the report is due at `now`: five seconds or more since the
    /// last. One that is due is timed again from `now`, so a late turn
    /// moves the next report with it, as a late tick does.
    pub fn due(&mut self, now: T) -> Due {
        if now.elapsed_since(self.last) < EVERY {
            return Due::No;
        }
        self.last = now;
        Due::Yes
    }

    /// How long from `now` until the report is due; zero once it is.
    pub fn until(&self, now: T) -> Duration {
        self.last.after(EVERY).elapsed_since(now)
    }
}

/// What a driver counts its wakes in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wakes {
    /// The display link's ticks, on macOS.
    #[cfg(any(target_os = "macos", test))]
    Ticks(Tally),
    /// The turns of the loop's one wait, on Linux.
    #[cfg(any(target_os = "linux", test))]
    Turns(Tally),
}

/// The first line of a frame drawn on the CPU: the wakes, the frames, and
/// what the last frame cost -- its dirty rows, its instances, the
/// conversion, the paint and the run cache.
pub fn cpu_line(wakes: Wakes, frames: Tally, last: &Frame) -> String {
    let (n, name) = match wakes {
        #[cfg(any(target_os = "macos", test))]
        Wakes::Ticks(n) => (n, "ticks"),
        #[cfg(any(target_os = "linux", test))]
        Wakes::Turns(n) => (n, "turns"),
    };
    format!(
        "{} {name}, {} frames; last: {} rows, {} instances, convert {:?}, paint {:?}, cache {:?}",
        n.value(),
        frames.value(),
        last.rows_dirty.value(),
        last.instances.value(),
        last.convert,
        last.composite,
        last.cache
    )
}

/// `OSC 8` activation, and a bare link's: links opened, revealed (in
/// Finder on macOS, sent to the file manager over the session bus on
/// Linux), and refused -- each refusal also a `warn!` naming the rule, the
/// `mouse_unencodable` shape of refusing and saying how often.
#[derive(Clone, Copy, Default, Debug)]
pub struct Links {
    pub opened: Tally,
    pub revealed: Tally,
    pub refused: Tally,
    /// What the printed paths' worker was asked and did (`probe.rs`), as the
    /// driver last copied it: counts only, never a path.
    pub checks: crate::probe::Counts,
}

impl Links {
    /// The report's line, or `None` before any link was activated.
    pub fn line(&self) -> Option<String> {
        let (o, v, r) = (
            self.opened.value(),
            self.revealed.value(),
            self.refused.value(),
        );
        (o + v + r > 0).then(|| format!("links: {o} opened, {v} revealed, {r} refused"))
    }

    /// **The printed paths' line**, once anything was asked: the paths sent
    /// to be checked, the calls the worker made for them (each `lstat`, each
    /// link read, the mount table, the program in front), the batches, those
    /// the floor after a batch that asked held, and the hangs. No path is
    /// named, as none is anywhere.
    pub fn checks_line(&self) -> Option<String> {
        let c = self.checks;
        (c.asked > 0).then(|| {
            format!(
                "paths: {} checked, {} calls, {} batches, {} held, {} hangs",
                c.asked, c.calls, c.batches, c.held, c.hangs
            )
        })
    }
}

/// The lines after the first, each once its count has moved: the
/// clipboard's writes, the page width's requests, the links activated.
pub fn counts(clips: &Clips, widths: &Widths, links: &Links) -> impl Iterator<Item = String> {
    [
        clips.report(),
        widths.report(),
        links.line(),
        links.checks_line(),
    ]
    .into_iter()
    .flatten()
}

/// Whether the log shows the report: `info` or more, read once at launch
/// from `BAUD_LOG`, so it holds for the session.
#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Logged {
    Yes,
    No,
}

#[cfg(target_os = "linux")]
impl Logged {
    /// What the log's threshold says of `info`.
    pub fn now() -> Logged {
        match crate::log::enabled(crate::log::Level::Info) {
            true => Logged::Yes,
            false => Logged::No,
        }
    }
}

/// **The report of a driver that waits on a clock**, the Linux loop: its
/// turns, the frames committed to the compositor, what the last frame cost
/// and the links activated, beside the report's clock.
///
/// **The report's deadline bounds the wait while the log shows it**
/// ([`Polled::wait`]), so an idle window, whose wait has no other bound,
/// still reports, and nothing waits but the one wait; at the default
/// level, `warn`, the report schedules nothing and the idle loop's wakes
/// are what they were without it.
#[cfg(any(target_os = "linux", test))]
pub struct Polled<T> {
    every: Every<T>,
    logged: Logged,
    turns: Tally,
    frames: Tally,
    last: Frame,
    pub links: Links,
}

#[cfg(any(target_os = "linux", test))]
impl<T: Moment> Polled<T> {
    /// Nothing counted, the first report due five seconds after `now`.
    pub fn new(now: T, logged: Logged) -> Polled<T> {
        Polled {
            every: Every::new(now),
            logged,
            turns: Tally::default(),
            frames: Tally::default(),
            last: Frame::default(),
            links: Links::default(),
        }
    }

    /// A frame drawn, and what it cost: the last one drawn is the
    /// report's, as on macOS's CPU path.
    pub fn drawn(&mut self, frame: Frame) {
        self.last = frame;
    }

    /// A frame committed to the compositor (`Presented::Yes`), as macOS
    /// counts a frame committed.
    pub fn presented(&mut self) {
        self.frames.bump();
    }

    /// How long the loop's wait may last for the report: until it is due,
    /// zero once it is -- the turn it wakes takes it, so a due report
    /// never spins the wait -- and no bound while the log would not show
    /// it.
    pub fn wait(&self, now: T) -> Option<Duration> {
        (self.logged == Logged::Yes).then(|| self.every.until(now))
    }

    /// **A turn of the loop, ending at `now`**: counted, and the report's
    /// lines when five seconds have passed since the last and the log
    /// shows them.
    pub fn turn(&mut self, now: T, clips: &Clips, widths: &Widths) -> Option<Vec<String>> {
        self.turns.bump();
        if self.logged == Logged::No || self.every.due(now) == Due::No {
            return None;
        }
        let first = cpu_line(Wakes::Turns(self.turns), self.frames, &self.last);
        Some(
            std::iter::once(first)
                .chain(counts(clips, widths, &self.links))
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Osc52;
    use baud_core::{BytePos, ClipboardBudget, Owned, ReadLen, Term, WinSize};
    use std::time::Instant;

    const WS: WinSize = WinSize {
        ws_row: 24,
        ws_col: 80,
    };

    fn feed(t: &mut Term<'_>, b: &[u8]) {
        t.write_at(BytePos::ZERO, b);
        t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
    }

    /// A frame of `rows` dirty rows and `instances` instances, converted
    /// in `convert` and painted in 900 us.
    fn frame(rows: Tally, instances: Tally, convert: Duration) -> Frame {
        Frame {
            rows_dirty: rows,
            instances,
            convert,
            composite: Duration::from_micros(900),
            ..Frame::default()
        }
    }

    /// **The first line on the CPU path, as macOS has printed it**: the
    /// ticks, the frames, and the last frame's rows, instances, conversion,
    /// paint and cache. Linux's names its turns where macOS names its
    /// ticks, and is otherwise the same text.
    #[test]
    fn the_cpu_line_is_the_macos_line_with_the_wakes_named() {
        let f = frame(Tally::of(7), Tally::of(120), Duration::from_micros(250));
        assert_eq!(
            cpu_line(Wakes::Ticks(Tally::of(601)), Tally::of(0), &f),
            format!(
                "601 ticks, 0 frames; last: 7 rows, 120 instances, convert 250µs, paint 900µs, cache {:?}",
                f.cache
            )
        );
        assert_eq!(
            cpu_line(Wakes::Turns(Tally::of(3)), Tally::of(2), &f),
            format!(
                "3 turns, 2 frames; last: 7 rows, 120 instances, convert 250µs, paint 900µs, cache {:?}",
                f.cache
            )
        );
    }

    /// **The report is due at five seconds of the driver's clock and not
    /// before**, timed again from each report, a late one included.
    #[test]
    fn the_report_is_due_at_five_seconds_and_timed_from_each_report() {
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let mut e = Every::new(t0);
        assert_eq!(e.until(t0), EVERY);
        assert_eq!(e.due(at(4999)), Due::No, "not before five seconds");
        assert_eq!(e.until(at(4999)), Duration::from_millis(1));
        assert_eq!(e.due(at(5000)), Due::Yes, "at five seconds");
        assert_eq!(e.due(at(5000)), Due::No, "once");
        assert_eq!(e.until(at(5000)), EVERY, "timed again from the report");
        assert_eq!(e.due(at(9999)), Due::No);
        assert_eq!(e.due(at(17_000)), Due::Yes, "a late turn takes it");
        assert_eq!(e.until(at(17_000)), EVERY, "and the next is timed from it");
        assert_eq!(e.due(at(21_999)), Due::No);
        assert_eq!(e.due(at(22_000)), Due::Yes);
    }

    /// **The Linux driver's report fires at five seconds of its clock and
    /// not before, with the counters it reads moving**: the turns, the
    /// frames committed to the compositor, the last frame's cost, and the
    /// clipboard's, the page width's and the links' counts, each line once
    /// its count has moved -- and its deadline bounds the wait.
    #[test]
    fn the_polled_report_fires_at_five_seconds_with_its_counters_moving() {
        let mut s = Owned::new(&WS).with_clipboard(ClipboardBudget::of_text(64));
        let mut t = Term::new(s.storage(), &WS);
        let mut clips = Clips::for_wayland(&t);
        let mut widths = Widths::new(&t);
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let mut r = Polled::new(t0, Logged::Yes);
        assert_eq!(r.wait(t0), Some(EVERY), "the wait is bounded by it");
        r.drawn(frame(
            Tally::of(24),
            Tally::of(300),
            Duration::from_micros(400),
        ));
        r.presented();
        assert_eq!(r.turn(at(1000), &clips, &widths), None);
        r.drawn(frame(
            Tally::of(3),
            Tally::of(40),
            Duration::from_micros(100),
        ));
        r.presented();
        assert_eq!(r.wait(at(1000)), Some(Duration::from_secs(4)));
        assert_eq!(r.turn(at(4999), &clips, &widths), None, "not before");
        assert_eq!(r.wait(at(4999)), Some(Duration::from_millis(1)));
        let first = r.turn(at(5000), &clips, &widths).expect("at five seconds");
        assert_eq!(
            first,
            [cpu_line(
                Wakes::Turns(Tally::of(3)),
                Tally::of(2),
                &frame(Tally::of(3), Tally::of(40), Duration::from_micros(100))
            )],
            "three turns, two frames, the last frame's cost, and no count moved"
        );
        assert!(first[0].starts_with("3 turns, 2 frames; last: 3 rows, 40 instances, "));
        assert_eq!(r.wait(at(5000)), Some(EVERY), "timed again from it");
        // The counts move: a frame, a link opened and two refused, a copy
        // taken, and a page-width request this driver refuses.
        r.drawn(frame(Tally::of(1), Tally::of(9), Duration::from_micros(50)));
        r.presented();
        r.links.opened.bump();
        r.links.refused.bump();
        r.links.refused.bump();
        feed(&mut t, b"\x1b]52;c;aGk=\x07");
        assert!(clips.poll(&t, Osc52::Write).is_some(), "the copy taken");
        feed(&mut t, b"\x1b[132$|");
        assert!(
            widths.poll(&t, crate::width::Host::NoResize).is_some(),
            "the request refused"
        );
        assert_eq!(r.turn(at(9999), &clips, &widths), None, "not before");
        let second = r.turn(at(10_000), &clips, &widths).expect("five after");
        assert_eq!(second.len(), 4, "{second:?}");
        assert!(
            second[0].starts_with("5 turns, 3 frames; last: 1 rows, 9 instances, convert 50µs, "),
            "{}",
            second[0]
        );
        assert_eq!(second[1], "clipboard: 1 written, 0 cleared, 0 refused");
        assert_eq!(second[2], "page width: 0 resized, 1 refused");
        assert_eq!(second[3], "links: 1 opened, 0 revealed, 2 refused");
    }

    /// **While the log would not show it, the report schedules nothing**:
    /// no bound on the wait and no lines, at any time, while the turns are
    /// still counted.
    #[test]
    fn a_report_the_log_hides_bounds_no_wait() {
        let mut s = Owned::new(&WS);
        let t = Term::new(s.storage(), &WS);
        let (clips, widths) = (Clips::for_wayland(&t), Widths::new(&t));
        let t0 = Instant::now();
        let mut r = Polled::new(t0, Logged::No);
        for secs in [0, 5, 10, 60] {
            let now = t0 + Duration::from_secs(secs);
            assert_eq!(r.wait(now), None);
            assert_eq!(r.turn(now, &clips, &widths), None);
        }
        assert_eq!(r.turns.value(), 4);
    }

    /// **The Linux driver keeps the report as the macOS app does**, read
    /// off its source, which a macOS build compiles only for the Linux
    /// target: the report's deadline is among the bounds of the loop's one
    /// wait; every turn reaches the report after its present, the loop
    /// having no `continue`; the frame `render_at` returns is kept and a
    /// frame committed to the compositor is counted; and every link
    /// activation is counted as the line it logs says.
    #[test]
    fn the_linux_driver_counts_and_reports_every_turn() {
        let src = include_str!("linux.rs");
        let bare = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
        let body = src
            .split_once("'run: loop {")
            .and_then(|(_, rest)| rest.split_once("// **The session's end, however it came**"))
            .map(|(b, _)| b)
            .expect("the loop");
        assert!(
            bare(body).contains("letreport=d.report.wait(now);"),
            "the report's bound"
        );
        let bounds = body
            .split_once("let timeout = [")
            .and_then(|(_, rest)| rest.split_once(']'))
            .map(|(b, _)| bare(b))
            .expect("the wait's bounds");
        assert!(bounds.split(',').any(|b| b == "report"), "{bounds}");
        assert!(!body.contains("continue"), "every turn reaches the report");
        let present = body.find("d.present();").expect("the present");
        let turn = body
            .find("d.report.turn(Instant::now(), &d.clips, &d.widths)")
            .expect("the report's turn");
        assert!(present < turn, "after the present");
        assert_eq!(body.matches("d.report.turn(").count(), 1, "once a turn");
        let drawn = src
            .split_once(".render_at(&mut self.term, back, &mut target, self.damage);")
            .map(|(_, rest)| bare(rest))
            .expect("the paint");
        assert!(drawn.starts_with("self.report.drawn(painted);"), "kept");
        let taken = src
            .split_once("== Presented::Yes")
            .and_then(|(_, rest)| rest.split_once("\n        }\n"))
            .map(|(b, _)| bare(b))
            .expect("the present's arm");
        assert!(taken.contains("self.report.presented();"), "counted");
        let link = src
            .split_once(
                "fn open_link(&mut self, link: &crate::link::Under, line: baud_render::AbsLine) {",
            )
            .and_then(|(_, rest)| rest.split_once("\n    }\n"))
            .map(|(b, _)| bare(b))
            .expect("the link's activation");
        let said = |line: &str, counted: &str| {
            let n = link.matches(line).count();
            assert!(n > 0, "{line}");
            assert_eq!(
                link.matches(&format!("{counted}{line}")).count(),
                n,
                "{line}"
            );
            n
        };
        let refusals = said("crate::warn!(\"link", "self.report.links.refused.bump();");
        assert_eq!(link.matches("refused.bump()").count(), refusals);
        said(
            "crate::info!(\"linkopened",
            "self.report.links.opened.bump();",
        );
        said(
            "crate::info!(\"linksent",
            "self.report.links.revealed.bump();",
        );
        // A `[link.*]` command's run is an open, under a word of its own so
        // a reveal's line is not mistaken for it.
        said(
            "crate::info!(\"linkhanded",
            "self.report.links.opened.bump();",
        );
        assert_eq!(link.matches(".bump()").count(), refusals + 3);
    }

    /// **The printed paths' line counts what was asked and done, and names
    /// nothing**: none before anything is asked, then numbers before their
    /// names, as the report's lines are parsed. Perturbation: print the line
    /// with nothing asked.
    #[test]
    fn the_paths_line_is_counts_only() {
        let mut l = Links::default();
        assert_eq!(l.checks_line(), None);
        l.checks = crate::probe::Counts {
            asked: 6,
            calls: 19,
            batches: 2,
            held: 1,
            hangs: 0,
        };
        assert_eq!(
            l.checks_line().as_deref(),
            Some("paths: 6 checked, 19 calls, 2 batches, 1 held, 0 hangs")
        );
    }
}
