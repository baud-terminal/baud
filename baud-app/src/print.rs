//! A program's print jobs, from the core to the print panel: the policy,
//! pure, so its tests run on every platform (the owner's call of 2026-10-20,
//! "printing in through the macOS print panel"; note AW, section 1).
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
//! **At most one panel at a time; jobs that finish while one is up wait as
//! one** (Jev's lean, MERGE, 0.02/0.06 then 0.48/0.30): each joins the
//! waiting job on a new page, so however many finish while a panel is up,
//! one panel follows it, and nothing is lost below [`WAITING_MAX`]; past
//! it a job is dropped and counted. A program that goes on printing gets a
//! panel each time the one before closes.

use crate::config::Printing;
#[cfg(any(target_os = "macos", test))]
use baud_core::{PrintBudget, PrintCut};
use baud_core::{PrinterPort, Term};
#[cfg(any(target_os = "macos", test))]
use baud_render::print::{Layout, Page, Stripped};

#[cfg(any(target_os = "macos", test))]
/// The core's print area: the jobs one read finishes and the one being
/// collected, about 500 pages of text. A printer controller run past it
/// prints what fit, marked cut.
pub const BUDGET: PrintBudget = PrintBudget::of_bytes(2 << 20);

#[cfg(any(target_os = "macos", test))]
/// The most a waiting job holds: one full area, which a release build lays
/// out in 63 ms off the main thread (534 ms at 16 MiB), the figures
/// docs/record.md gives; the text views AppKit then builds on the main
/// thread are unmeasured.
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
    let (forms, stripped) = baud_render::print::read(job);
    let layout = Layout::fit(
        width,
        height,
        advance,
        leading,
        baud_render::print::widest(&forms),
    );
    (
        baud_render::print::paginate(&forms, layout.lines),
        layout,
        stripped,
    )
}

#[cfg(any(target_os = "macos", test))]
/// What the panel names the job: the window's title, so it says which
/// window printed, and a word when some of it was lost.
pub fn job_title(window: &str, cut: PrintCut) -> String {
    match cut {
        PrintCut::Whole => format!("{window} \u{2014} print"),
        PrintCut::Cut => format!("{window} \u{2014} print (cut short)"),
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
            [("hello".to_string(), baud_render::print::Look::BOLD)]
        );
        assert_eq!(layout.size, 10.0);
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
        let linux = include_str!("linux.rs");
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
        assert_eq!(job_title("vim", PrintCut::Whole), "vim \u{2014} print");
        assert_eq!(
            job_title("vim", PrintCut::Cut),
            "vim \u{2014} print (cut short)"
        );
    }
}
