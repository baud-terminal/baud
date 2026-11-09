//! A program's print jobs, from the core to the print panel: the policy,
//! pure, so its tests run on every platform (note AW, section 1).
//!
//! **The core is told there is a printer only where one can be offered**:
//! on macOS with `[screen] printing` on and a printer listed, where DSR
//! `?15` then answers ready. A printer that macOS stops listing makes it
//! "not ready": `?15` answers so and the jobs wait for one to be listed
//! again. Anywhere else it answers "no printer" and every print is ignored
//! and counted, as 070-7 § 7.3.2 ties the two (note AW). `[screen]
//! printing` is read once, at launch, so "off" is never a lost printer.
//! The Linux driver has no print panel, so its core never has a printer.
//!
//! **The list is read again when a program asks, off the main thread**
//! ([`Watch`]): on macOS a DSR `?15` stops the read (`PrinterHold::On`),
//! the list is read on a thread of its own, and the answer goes from what
//! it found before the rest of the read is parsed (`reads::paced`), as a
//! port answers from its printer's state when asked (070-7 § 7.3.2). It is
//! read again at a focus gain, and while the indicator line shows the
//! printer as often as 070-14's 80 ms for an indicator needs
//! ([`indicator_every`]). Nothing reads it otherwise: each read launches
//! the print system's daemon if it is not running.
//!
//! **At most one panel at a time; jobs that finish while one is up wait as
//! one**: each joins the waiting job on a new page, so however many finish
//! while a panel is up, one panel follows it, and nothing is lost below
//! [`WAITING_MAX`]; past it a job is dropped and counted. A program that
//! goes on printing gets a panel each time the one before closes.

use crate::config::Printing;
#[cfg(any(target_os = "macos", test))]
use baud_core::{PrintBudget, PrintCut};
use baud_core::{PrinterPort, Term};
#[cfg(any(target_os = "macos", test))]
use baud_render::print::{Layout, Page, PictureBudget, Pictures, Stripped};

#[cfg(any(target_os = "macos", test))]
/// **The pixels a job's pictures may hold together**, 2^25, 128 MiB of
/// picture and as much again in the panel's bitmaps: a hard copy of the
/// whole text area of a 6K display, about 20 million pixels, fits whole,
/// and a larger one is reduced to fit, which the sheet's shrinking to fit
/// would have done to it anyway.
pub const PICTURE_PIXELS: PictureBudget = PictureBudget(1 << 25);

#[cfg(any(target_os = "macos", test))]
/// The core's print area: the jobs one read finishes and the one being
/// collected, about 500 pages of text. A printer controller run past it
/// prints what fit, marked cut.
pub const BUDGET: PrintBudget = PrintBudget::of_bytes(2 << 20);

#[cfg(any(target_os = "macos", test))]
/// The most a waiting job holds: one full area, which a release build lays
/// out in 63 ms off the main thread (534 ms at 16 MiB); the text views
/// AppKit then builds on the main thread are unmeasured.
pub const WAITING_MAX: usize = 2 << 20;

#[cfg(any(target_os = "macos", test))]
/// The area the core is given: [`BUDGET`] while printing is on, none while
/// it is off, as the core then never collects a job.
pub fn budget(p: Printing) -> PrintBudget {
    match p {
        Printing::On => BUDGET,
        Printing::Off => PrintBudget::of_bytes(0),
    }
}

/// Whether the system lists a printer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Printers {
    /// Never on Linux, whose driver has no print panel.
    #[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
    Listed,
    None,
}

/// **Tell the core whether it has a printer**: only with printing on and a
/// printer listed. One function for both drivers, called after `Term::new`
/// and again whenever the listing is read anew.
pub fn set_printer(term: &mut Term<'_>, p: Printing, listed: Printers) {
    term.set_printer(match (p, listed) {
        (Printing::On, Printers::Listed) => PrinterPort::Ready,
        _ => PrinterPort::Absent,
    });
}

#[cfg(any(target_os = "macos", test))]
/// **Whether a DSR `?15` waits for the list to be read again**: with
/// printing on, where there is a list to read; off, the core answers "no
/// printer" at once.
pub fn set_hold(term: &mut Term<'_>, p: Printing) {
    term.set_printer_hold(match p {
        Printing::On => baud_core::PrinterHold::On,
        Printing::Off => baud_core::PrinterHold::Off,
    });
}

#[cfg(any(target_os = "macos", test))]
/// "The interval between the change in the state of a feature and the
/// update of the appropriate indicator shall not exceed 80 milliseconds"
/// (`[[source: dec-std-070]]` 070-14 p. 14-6).
const INDICATOR_BOUND: std::time::Duration = std::time::Duration::from_millis(80);

#[cfg(any(target_os = "macos", test))]
/// A `printerNames` with the print daemon running: 0.30-0.83 ms wall
/// measured, so a millisecond.
const READ_WARM: std::time::Duration = std::time::Duration::from_millis(1);

#[cfg(any(target_os = "macos", test))]
/// **How long after one read the indicator's next is due, on a display
/// that ticks every `tick`**: 070-14's 80 ms less what follows a read
/// being due -- up to a tick until the pump sends it, the read twice (a
/// change can land just after it looks, and its answer comes back after
/// the next one's), up to a tick until the frame that draws it, and that
/// frame's vsync. 28 ms at 60 Hz and 53 ms at 120; below 38.5 Hz nothing
/// is left, the list is read every tick and the bound is not met.
/// The bound assumes the ticks arrive on time: a tick that runs late
/// sends its read that much later, against a margin of about 11 ms
/// at 60 Hz and about 3 ms at 120.
/// The read is warm because these reads keep the daemon running; the one
/// that starts it, 40-78 ms, is not inside the bound.
pub fn indicator_every(tick: std::time::Duration) -> std::time::Duration {
    INDICATOR_BOUND.saturating_sub(tick * 3 + READ_WARM * 2)
}

#[cfg(any(target_os = "macos", test))]
/// Why the list would be read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ask {
    /// A held DSR `?15`, or a focus gain: now.
    Now,
    /// The indicator line is shown, on a display that ticks every `tick`:
    /// once [`indicator_every`] has passed.
    Indicator { tick: std::time::Duration },
}

#[cfg(any(target_os = "macos", test))]
/// **When the driver reads the printer list again**: never two reads at
/// once -- a question asked while one is out is answered by it, a read
/// that began before the question: under a millisecond with the print
/// daemon running, and tens of milliseconds when the read had to start
/// it -- and for the indicator no more often than [`indicator_every`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Watch {
    out: Out,
    last: Option<std::time::Instant>,
}

#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Out {
    Idle,
    Reading,
}

#[cfg(any(target_os = "macos", test))]
impl Watch {
    pub const fn new() -> Watch {
        Watch {
            out: Out::Idle,
            last: None,
        }
    }

    /// Whether to send a read now, for `why`; if so, one is out.
    pub fn ask(&mut self, now: std::time::Instant, why: Ask) -> bool {
        let due = self.out == Out::Idle
            && match why {
                Ask::Now => true,
                Ask::Indicator { tick } => {
                    self.last.is_none_or(|l| now >= l + indicator_every(tick))
                }
            };
        if due {
            self.out = Out::Reading;
            self.last = Some(now);
        }
        due
    }

    /// **When the indicator line's next read is due**, on a display that
    /// ticks every `tick`: none while one is out, whose landing wakes the
    /// window, and at once before the first.
    #[cfg(target_os = "macos")]
    pub fn next(
        &self,
        now: std::time::Instant,
        tick: std::time::Duration,
    ) -> Option<std::time::Instant> {
        match self.out {
            Out::Reading => None,
            Out::Idle => Some(self.last.map_or(now, |l| l + indicator_every(tick))),
        }
    }

    /// The read came back.
    pub fn landed(&mut self) {
        self.out = Out::Idle;
    }
}

#[cfg(any(target_os = "macos", test))]
/// A job for the panel: 070-7's print stream, and whether any of it was
/// cut at the core's bound or dropped here.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Job {
    pub bytes: Vec<u8>,
    pub cut: PrintCut,
}

#[cfg(any(target_os = "macos", test))]
/// Jobs dropped because the waiting job was full.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Dropped(usize);

#[cfg(any(target_os = "macos", test))]
impl std::fmt::Display for Dropped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(any(target_os = "macos", test))]
/// The jobs taken from the core and not yet on a panel.
pub struct Prints {
    waiting: Option<Job>,
    pub dropped: Dropped,
}

#[cfg(any(target_os = "macos", test))]
impl Prints {
    pub fn new() -> Prints {
        Prints {
            waiting: None,
            dropped: Dropped::default(),
        }
    }

    /// **The core's finished jobs, taken** after a read: each joins the
    /// waiting job after a form feed, or starts it. Call after every part of
    /// a read, as the core keeps [`baud_core::PRINT_QUEUE`].
    pub fn take(&mut self, t: &mut Term<'_>) {
        for j in t.print_jobs() {
            let held = self.waiting.as_ref().map_or(0, |w| w.bytes.len() + 1);
            let room = WAITING_MAX.saturating_sub(held);
            if j.bytes.len() > room {
                self.dropped.0 += 1;
                if let Some(w) = self.waiting.as_mut() {
                    w.cut = PrintCut::Cut;
                }
                continue;
            }
            match self.waiting.as_mut() {
                Some(w) => {
                    w.bytes.push(0x0c);
                    w.bytes.extend_from_slice(j.bytes);
                    if j.cut == PrintCut::Cut {
                        w.cut = PrintCut::Cut;
                    }
                }
                None => {
                    self.waiting = Some(Job {
                        bytes: j.bytes.to_vec(),
                        cut: j.cut,
                    })
                }
            }
        }
        t.clear_print_jobs();
    }

    /// The waiting job, handed to a panel while the core has a printer;
    /// the next ones wait anew. With the printer lost the core is "not
    /// ready" and queues its prints (070-7 § 7.3.2), so the job waits for
    /// the focus gain that finds one listed again.
    pub fn next(&mut self, port: PrinterPort) -> Option<Job> {
        match port {
            PrinterPort::Ready => self.waiting.take(),
            PrinterPort::Absent => None,
        }
    }
}

#[cfg(any(target_os = "macos", test))]
/// **A job laid out for the panel's paper**: read as baud's printer reads
/// it, the type sized to its widest line, on sheets of the lines that fit
/// (`baud_render::print`). `width` and `height` are the sheet's printable
/// area and `advance` and `leading` the font's, per point.
pub fn pages(
    job: &[u8],
    width: f64,
    height: f64,
    advance: f64,
    leading: f64,
) -> (Vec<Page>, Layout, Stripped) {
    let (forms, stripped) = baud_render::print::read(job, PICTURE_PIXELS);
    let layout = Layout::fit(
        width,
        height,
        advance,
        leading,
        baud_render::print::widest(&forms),
    );
    (
        baud_render::print::paginate(&forms, layout),
        layout,
        stripped,
    )
}

#[cfg(any(target_os = "macos", test))]
/// What the panel names the job: the window's title, so it says which
/// window printed, and a word for each way some of it was lost: the job
/// cut short, pictures reduced to fit [`PICTURE_PIXELS`], pictures left
/// out with none of it left.
pub fn job_title(window: &str, cut: PrintCut, stripped: Stripped) -> String {
    let mut lost = Vec::new();
    if cut == PrintCut::Cut {
        lost.push("cut short");
    }
    if stripped.reduced() > Pictures::NONE {
        lost.push("pictures reduced");
    }
    if stripped.left_out() > Pictures::NONE {
        lost.push("pictures left out");
    }
    match lost.is_empty() {
        true => format!("{window} \u{2014} print"),
        false => format!("{window} \u{2014} print ({})", lost.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_core::{Owned, PrintRun};
    use baud_domain::{BytePos, ReadLen, WinSize};

    const WS: WinSize = WinSize {
        ws_row: 4,
        ws_col: 10,
    };

    fn feed(t: &mut Term<'_>, b: &[u8]) {
        t.write_at(BytePos::ZERO, b);
        t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
    }

    /// **The core has a printer only with printing on and one listed**, and
    /// DSR `?15` says which.
    #[test]
    fn the_core_has_a_printer_only_with_printing_on_and_one_listed() {
        for (p, listed, want, reply) in [
            (
                Printing::On,
                Printers::Listed,
                PrinterPort::Ready,
                "\x1b[?10n",
            ),
            (
                Printing::On,
                Printers::None,
                PrinterPort::Absent,
                "\x1b[?13n",
            ),
            (
                Printing::Off,
                Printers::Listed,
                PrinterPort::Absent,
                "\x1b[?13n",
            ),
        ] {
            let mut s = Owned::new(&WS).with_printer(budget(p));
            let mut t = Term::new(s.storage(), &WS);
            set_printer(&mut t, p, listed);
            assert_eq!(t.printer(), want, "{p:?} {listed:?}");
            feed(&mut t, b"\x1b[?15n");
            assert_eq!(t.reply(), reply.as_bytes(), "{p:?} {listed:?}");
        }
        assert_eq!(budget(Printing::Off), PrintBudget::of_bytes(0));
    }

    /// **Jobs taken while a panel is up wait as one**, each on a new page,
    /// and the next panel takes them all.
    #[test]
    fn jobs_wait_as_one_each_on_a_new_page() {
        let mut s = Owned::new(&WS).with_printer(BUDGET);
        let mut t = Term::new(s.storage(), &WS);
        set_printer(&mut t, Printing::On, Printers::Listed);
        let mut prints = Prints::new();
        feed(&mut t, b"a\x1b[?1i");
        prints.take(&mut t);
        feed(&mut t, b"\x1b[5ib\x1b[4i");
        prints.take(&mut t);
        assert_eq!(t.print_jobs().count(), 0, "taken from the core");
        let job = prints.next(t.printer()).expect("a waiting job");
        assert_eq!(job.bytes, b"\x1b[0m\x1b#5a\r\n\x1b[0m\x0cb");
        assert_eq!(job.cut, PrintCut::Whole);
        assert_eq!(prints.next(t.printer()), None);
        assert_eq!(t.print_run(), PrintRun::Idle);
    }

    /// The indicator's ask on a display that ticks every `tick`.
    fn indicator(tick: std::time::Duration) -> Ask {
        Ask::Indicator { tick }
    }

    /// **The list is read when a question needs it, one read at a time,
    /// and for the indicator at its pace**: a held `?15` or a focus gain
    /// sends a read at once unless one is out, and the indicator's reads
    /// wait [`indicator_every`] from the last.
    #[test]
    fn the_list_is_read_once_at_a_time_and_paced_for_the_indicator() {
        use std::time::{Duration, Instant};
        let t0 = Instant::now();
        let tick = Duration::from_secs_f64(1.0 / 60.0);
        let every = indicator_every(tick);
        let mut w = Watch::new();
        assert!(w.ask(t0, indicator(tick)), "the first look");
        assert!(!w.ask(t0, Ask::Now), "one is out");
        w.landed();
        assert!(w.ask(t0, Ask::Now), "a question reads at once");
        w.landed();
        let soon = t0 + every - Duration::from_millis(1);
        assert!(!w.ask(soon, indicator(tick)));
        assert!(w.ask(t0 + every, indicator(tick)));
        assert!(!w.ask(t0 + 2 * every, indicator(tick)), "one is out");
    }

    /// **A printer's change reaches the glass within 070-14's 80 ms** at
    /// 60 and 120 Hz: driven a tick at a time, the longest gap between two
    /// of the indicator's reads, a change landing just after one read
    /// looked, plus the warm read twice (it looks partway in, and comes
    /// back), the wait for the next tick to draw and that frame's vsync.
    #[test]
    fn a_printer_change_reaches_the_indicator_within_80_ms() {
        use std::time::{Duration, Instant};
        for hz in [60.0, 120.0] {
            let tick = Duration::from_secs_f64(1.0 / hz);
            let t0 = Instant::now();
            let mut w = Watch::new();
            let (mut sent, mut gap) = (None::<Instant>, Duration::ZERO);
            for k in 0..600u32 {
                let now = t0 + tick * k;
                if w.ask(now, indicator(tick)) {
                    if let Some(s) = sent {
                        gap = gap.max(now - s);
                    }
                    sent = Some(now);
                    w.landed();
                }
            }
            let worst = gap + 2 * READ_WARM + 2 * tick;
            assert!(
                worst <= Duration::from_millis(80),
                "{hz} Hz: {worst:?}, reads {gap:?} apart"
            );
        }
    }

    /// **A held `?15` only while printing is on**: off, the core answers "no
    /// printer" at once, and with it on the question waits for the list.
    #[test]
    fn a_question_waits_for_the_list_only_with_printing_on() {
        for (p, want) in [(Printing::On, &b""[..]), (Printing::Off, b"\x1b[?13n")] {
            let mut s = Owned::new(&WS).with_printer(budget(p));
            let mut t = Term::new(s.storage(), &WS);
            set_hold(&mut t, p);
            feed(&mut t, b"\x1b[?15n");
            assert_eq!(t.reply(), want, "{p:?}");
        }
    }

    /// **A lost printer holds the jobs until one is back**: the core
    /// answers "not ready" and queues its prints (070-7 § 7.3.2), so no
    /// panel shows until macOS lists a printer again.
    #[test]
    fn a_lost_printer_holds_the_jobs_until_one_is_back() {
        let mut s = Owned::new(&WS).with_printer(BUDGET);
        let mut t = Term::new(s.storage(), &WS);
        set_printer(&mut t, Printing::On, Printers::Listed);
        set_printer(&mut t, Printing::On, Printers::None);
        let mut prints = Prints::new();
        feed(&mut t, b"\x1b[5ib\x1b[4i\x1b[?15n");
        assert_eq!(t.reply(), b"\x1b[?11n");
        prints.take(&mut t);
        assert_eq!(prints.next(t.printer()), None, "held while not ready");
        set_printer(&mut t, Printing::On, Printers::Listed);
        let job = prints.next(t.printer()).expect("the held job");
        assert_eq!(job.bytes, b"b");
    }

    /// **A job that would pass `WAITING_MAX` is dropped and counted**, and
    /// the waiting job says some was lost.
    #[test]
    fn a_job_past_the_waiting_bound_is_dropped_and_counted() {
        let mut s = Owned::new(&WS).with_printer(PrintBudget::of_bytes(WAITING_MAX));
        let mut t = Term::new(s.storage(), &WS);
        set_printer(&mut t, Printing::On, Printers::Listed);
        let mut prints = Prints::new();
        let big = vec![b'x'; WAITING_MAX - 4];
        feed(&mut t, b"\x1b[5i");
        for chunk in big.chunks(4096) {
            feed(&mut t, chunk);
        }
        feed(&mut t, b"\x1b[4i");
        prints.take(&mut t);
        feed(&mut t, b"\x1b[5i12345678\x1b[4i");
        prints.take(&mut t);
        assert_eq!(prints.dropped, Dropped(1));
        let job = prints.next(t.printer()).expect("the first job");
        assert_eq!(job.bytes.len(), WAITING_MAX - 4);
        assert_eq!(job.cut, PrintCut::Cut);
    }

    /// **A job reaches the panel laid out**: a print page's lines on one
    /// sheet at the size its widest line fits.
    #[test]
    fn a_job_is_laid_out_for_the_sheet() {
        let mut s = Owned::new(&WS).with_printer(BUDGET);
        let mut t = Term::new(s.storage(), &WS);
        set_printer(&mut t, Printing::On, Printers::Listed);
        feed(&mut t, b"\x1b[1mhello\x1b[0m\r\nworld\x1b[?19h\x1b[i");
        let job: Vec<u8> = t.print_jobs().next().expect("a job").bytes.to_vec();
        let (pages, layout, _) = pages(&job, 480.0, 720.0, 0.6, 1.2);
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].lines.len(), 4);
        assert_eq!(
            pages[0].lines[0].runs(),
            [baud_render::print::Run::new(
                "hello",
                baud_render::print::Look::BOLD
            )]
        );
        assert_eq!(layout.size, 10.0);
    }

    /// **A ReGIS hard copy reaches the panel as a picture**: the core's
    /// dump of the screen, its text drawn by the embedded face, read by
    /// the printer into a picture the page's size at six decipoints a
    /// pixel, the "H" inked inside its cell and nowhere past the "i".
    #[test]
    fn a_hard_copy_reaches_the_panel_as_a_picture() {
        let mut s = Owned::new(&WS)
            .with_images(baud_domain::ImageBudget::of(1 << 20))
            .with_printer(BUDGET);
        let mut t = Term::new(s.storage(), &WS);
        t.set_cell_size(baud_domain::ImagePx::of(10), baud_domain::ImagePx::of(20));
        t.set_regis_font(crate::regis_font::glyph);
        set_printer(&mut t, Printing::On, Printers::Listed);
        let mut prints = Prints::new();
        feed(&mut t, b"Hi\x1bPpS(H)\x1b\\");
        prints.take(&mut t);
        let job = prints.next(t.printer()).expect("the hard copy");
        let (pages, layout, stripped) = pages(&job.bytes, 480.0, 720.0, 0.6, 1.2);
        assert_eq!(stripped, Stripped::default());
        let p = pages[0].lines[0].picture.as_ref().expect("a picture");
        assert_eq!((p.width.raw(), p.height.raw()), (100, 80));
        let (w, h) = p.size(layout);
        assert!((w.0 - 60.0).abs() < 1e-9 && (h.0 - 48.0).abs() < 1e-9);
        let lit: Vec<(usize, usize)> = p
            .pixels
            .iter()
            .enumerate()
            .filter(|(_, px)| px.alpha().get() != 0)
            .map(|(i, _)| (i % 100, i / 100))
            .collect();
        assert!(lit.iter().any(|&(x, y)| x < 10 && y < 20), "the H");
        assert!(
            lit.iter().any(|&(x, y)| (10..20).contains(&x) && y < 20),
            "the i"
        );
        assert!(
            lit.iter().all(|&(x, y)| x < 20 && y < 20),
            "nothing past them"
        );
    }

    /// **The panel always shows, as a sheet**: the AppKit side sets
    /// `showsPrintPanel` and runs the operation modal for the window, which
    /// returns at once, and never runs it in place, which would print with
    /// whatever the operation was told or hold the main thread. The panel is
    /// never shown in a test; this reads the source that shows it.
    #[test]
    fn the_print_panel_is_always_shown_as_a_sheet() {
        let src = include_str!("../../baud-present-macos/src/print.rs");
        let code: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(code.contains("op.setShowsPrintPanel(true);"));
        assert!(!code.contains("setShowsPrintPanel(false)"));
        assert!(code.contains("runOperationModalForWindow_delegate_didRunSelector_contextInfo("));
        assert!(!code.contains(".runOperation()"), "never run in place");
        let app = include_str!("app.rs");
        assert_eq!(
            app.matches("baud_present_macos::print::show_panel(")
                .count(),
            1
        );
        assert!(app.contains("dispatch2::DispatchQueue::main().exec_async(move || {"));
    }

    /// **Both drivers tell the core**, and the Linux one that it has no
    /// printer, having no print panel.
    #[test]
    fn both_drivers_tell_the_core_and_linux_has_no_printer() {
        let mac = include_str!("app.rs");
        assert!(mac.contains("crate::print::set_printer(&mut term, printing, printers_listed());"));
        assert!(mac.contains(".with_printer(crate::print::budget(printing))"));
        let linux = &crate::host::linux_driver_src();
        assert!(linux.contains(
            "crate::print::set_printer(&mut term, cfg.printing, crate::print::Printers::None);"
        ));
        assert!(
            !linux.contains("with_printer("),
            "no area: no job is ever collected"
        );
    }

    #[test]
    fn the_panel_names_the_window_that_printed() {
        let none = Stripped::default();
        assert_eq!(
            job_title("vim", PrintCut::Whole, none),
            "vim \u{2014} print"
        );
        assert_eq!(
            job_title("vim", PrintCut::Cut, none),
            "vim \u{2014} print (cut short)"
        );
    }

    /// **A job naming a picture past the budget is laid out within it**,
    /// and the panel says so: a 29-byte dump naming 65535 by 65535 pixels
    /// holds at most [`PICTURE_PIXELS`]; after a picture of all of it, the
    /// next is left out.
    #[test]
    fn a_picture_past_the_budget_is_reduced_and_the_title_says_so() {
        let huge = b"\x1bP0;1;6q\"1;1;65535;65535#1~\x1b\\";
        let (pages_of, _, stripped) = pages(huge, 480.0, 720.0, 0.6, 1.2);
        let p = pages_of[0].lines[0].picture.as_ref().expect("a picture");
        assert!(p.pixels.len() <= 1 << 25, "{}", p.pixels.len());
        assert_eq!(
            (stripped.reduced(), stripped.left_out()),
            (Pictures(1), Pictures::NONE)
        );
        assert_eq!(
            job_title("vim", PrintCut::Whole, stripped),
            "vim \u{2014} print (pictures reduced)"
        );
        let mut job = b"\x1bP0;1;6q\"1;1;8192;4096#1~\x1b\\".to_vec();
        job.extend_from_slice(huge);
        let (pages_of, _, stripped) = pages(&job, 480.0, 720.0, 0.6, 1.2);
        assert_eq!(pages_of.iter().map(|p| p.lines.len()).sum::<usize>(), 1);
        assert_eq!(
            (stripped.reduced(), stripped.left_out()),
            (Pictures::NONE, Pictures(1))
        );
        assert_eq!(
            job_title("vim", PrintCut::Cut, stripped),
            "vim \u{2014} print (cut short, pictures left out)"
        );
    }
}
