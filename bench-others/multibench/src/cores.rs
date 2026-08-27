use crate::ffi::ghostty;
use crate::*;
use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::term::{Config, Term as ATerm};
use alacritty_terminal::vte::ansi::Processor;

/// baud's SIMD entry point only exists on x86-64, so naming it elsewhere does
/// not compile. Same shape as baud-bench's dispatch.
#[cfg(target_arch = "x86_64")]
pub(crate) unsafe fn baud_feed(
    t: &mut baud_core::Term,
    base: baud_core::BytePos,
    n: baud_core::ByteLen,
) {
    unsafe { t.feed_avx2(base, n) }
}

#[cfg(not(target_arch = "x86_64"))]
pub(crate) unsafe fn baud_feed(
    t: &mut baud_core::Term,
    base: baud_core::BytePos,
    n: baud_core::ByteLen,
) {
    t.feed(base, n)
}

#[derive(Clone)]
pub(crate) struct Noop;

impl EventListener for Noop {
    fn send_event(&self, _: Event) {}
}

#[derive(Clone, Copy)]
pub(crate) struct Dim;

impl alacritty_terminal::grid::Dimensions for Dim {
    fn total_lines(&self) -> usize {
        ROWS
    }
    fn screen_lines(&self) -> usize {
        ROWS
    }
    fn columns(&self) -> usize {
        COLS
    }
}

#[derive(Debug)]
pub(crate) struct WzCfg;

impl tattoy_wezterm_term::TerminalConfiguration for WzCfg {
    fn color_palette(&self) -> tattoy_wezterm_term::color::ColorPalette {
        tattoy_wezterm_term::color::ColorPalette::default()
    }
}

pub(crate) struct WzSink;

impl std::io::Write for WzSink {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// One rep of baud in a given representation.
///
/// The mode is chosen once at startup, so each is a configuration a user could
/// actually run -- the same status ghostty's -Dsimd flag has. Reporting only
/// `Runs` under-reports baud on payloads where a materialised viewport wins, and
/// MEASUREMENTS.md's own baud-alone table shows the modes trade places by
/// payload: spans lead on streaming, and on TUI all three land within noise.
pub(crate) fn baud_rep(p: &[u8], total: usize, mode: baud_core::OpMode) -> f64 {
    let mut st = baud_core::Storage::boxed();
    let mut t = baud_core::Term::new(&mut st, mode);
    drive(p, total, |chunk| {
        let mut slot = t.read_slot();
        let base = slot.base();
        let fl = slot.write(baud_core::StreamByte::slice_from(chunk));
        unsafe { baud_feed(&mut t, base, fl.len()) };
        t.commit_read(fl.len());
    })
}

/// One rep of one core, with state built fresh exactly as before.
///
/// Indexed rather than a closure per core so the driver can interleave them.
/// Blocked ordering -- nine reps of one core, then nine of the next -- let run
/// position bias a core: inserting a row above baud cost it 16% while every
/// other core moved under 6%.
pub(crate) fn run_one(core: usize, p: &[u8], total: usize) -> f64 {
    match core {
        // Not a ceiling. A discarding sink makes the reader block in read(2)
        // continuously, so this pair pays more scheduler round trips per byte
        // than one whose sink does work -- cores have measured *above* it.
        0 => drive(p, total, |c| {
            std::hint::black_box(c);
        }),
        1 => baud_rep(p, total, baud_core::OpMode::Runs),
        2 => {
            let mut cfg = Config::default();
            cfg.scrolling_history = 10_000;
            let mut term = ATerm::new(cfg, &Dim, Noop);
            let mut proc = Processor::<alacritty_terminal::vte::ansi::StdSyncHandler>::default();
            drive(p, total, |chunk| proc.advance(&mut term, chunk))
        }
        3 => {
            #[cfg(have_ghostty)]
            {
                let mut term: ghostty::Terminal = std::ptr::null_mut();
                let rc = unsafe {
                    ghostty::ghostty_terminal_new(
                        std::ptr::null(),
                        &mut term,
                        COLS as u16,
                        ROWS as u16,
                    )
                };
                assert_eq!(rc, 0, "ghostty_terminal_new failed with {rc}");
                let v = drive(p, total, |chunk| unsafe {
                    ghostty::ghostty_terminal_vt_write(term, chunk.as_ptr(), chunk.len());
                });
                unsafe { ghostty::ghostty_terminal_free(term) };
                v
            }
            // NaN, not zero: a missing core must not be mistaken for a slow one.
            #[cfg(not(have_ghostty))]
            {
                f64::NAN
            }
        }
        4 => {
            let mut parser = vt100::Parser::new(ROWS as u16, COLS as u16, 10_000);
            drive(p, total, |chunk| parser.process(chunk))
        }
        5 => {
            let size = tattoy_wezterm_term::TerminalSize {
                rows: ROWS,
                cols: COLS,
                pixel_width: 0,
                pixel_height: 0,
                dpi: 0,
            };
            let mut term = tattoy_wezterm_term::Terminal::new(
                size,
                std::sync::Arc::new(WzCfg),
                "baud-bench",
                "1",
                Box::new(WzSink),
            );
            drive(p, total, |chunk| term.advance_bytes(chunk))
        }
        6 => {
            let mut parser = termwiz::escape::parser::Parser::new();
            drive(p, total, |chunk| {
                parser.parse(chunk, |a| {
                    std::hint::black_box(a);
                });
            })
        }
        7 => baud_rep(p, total, baud_core::OpMode::Computed),
        8 => baud_rep(p, total, baud_core::OpMode::Templated),
        _ => unreachable!(),
    }
}
