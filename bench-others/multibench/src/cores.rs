use crate::ffi::ghostty;
use crate::*;
use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::term::{Config, Term as ATerm};
use alacritty_terminal::vte::ansi::Processor;

/// baud's SIMD entry point only exists on x86-64, so naming it elsewhere does
/// not compile. Same shape as baud-bench's dispatch.
#[cfg(target_arch = "x86_64")]
pub(crate) unsafe fn baud_feed(
    t: &mut baud_core2::Term,
    base: baud_core2::BytePos,
    n: baud_core2::ByteLen,
) {
    unsafe { t.feed_avx2(base, n) }
}

#[cfg(not(target_arch = "x86_64"))]
pub(crate) unsafe fn baud_feed(
    t: &mut baud_core2::Term,
    base: baud_core2::BytePos,
    n: baud_core2::ByteLen,
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
pub(crate) fn baud_rep(p: &[u8], total: usize, mode: baud_core2::OpMode) -> f64 {
    let mut st = baud_core2::Storage::boxed();
    let mut t = baud_core2::Term::new(&mut st, mode);
    drive(p, total, |chunk| {
        let mut slot = t.read_slot();
        let base = slot.base();
        let fl = slot.write(baud_core2::StreamByte::slice_from(chunk));
        unsafe { baud_feed(&mut t, base, fl.len()) };
        t.commit_read(fl.len());
    })
}

/// The new core: caller-provided storage, no allocator of its own.
///
/// **Every buffer is the harness's**, which is the contract the crate is built
/// to -- it is `no_std`, `alloc`-free and borrows all of it. Built fresh per
/// rep like every other core here.
pub(crate) fn baud_new_rep(p: &[u8], total: usize) -> f64 {
    use baud_core as nb;
    let ws = nb::WinSize {
        ws_row: crate::ROWS as u16,
        ws_col: crate::COLS as u16,
    };
    let (_, rows) = ws.geometry();
    let span = nb::RowBlock::leading(rows, nb::Cols::from_stride(nb::MAX_COLS));
    let mut arena = vec![0u8; nb::ARENA];
    let mut vp = vec![nb::PackedCell::EMPTY; span.len()];
    let mut used = vec![nb::RowExtent::ZERO; rows.index()];
    // Empty declines the three-byte cell table, whose only read is behind
    // `cell-table`; filling it costs 2 MiB of stores per core construction.
    let mut cells3: Vec<nb::PackedCell> = Vec::new();
    let mut tmpl = vec![None; nb::NSTYLE];
    let mut tabs = vec![false; nb::MAX_COLS];
    let mut reply = vec![0u8; nb::REPLY];
    let mut alt = vec![nb::PackedCell::EMPTY; span.len()];
    let mut alt_used = vec![nb::RowExtent::ZERO; rows.index()];
    let mut clusters = vec![nb::ClusterSlot::EMPTY; nb::NCLUSTER];
    let mut t = nb::Term::new(
        nb::Storage::new(
            &mut arena,
            &mut vp,
            &mut used,
            &mut cells3,
            &mut tmpl,
            &mut tabs,
            &mut reply,
            &mut alt,
            &mut alt_used,
            &mut clusters,
        ),
        &ws,
    );
    drive(p, total, |chunk| {
        t.write_at(nb::BytePos::ZERO, chunk);
        t.feed(nb::BytePos::ZERO, nb::ReadLen::from_index(chunk.len()));
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
        1 => baud_rep(p, total, baud_core2::OpMode::Runs),
        9 => baud_new_rep(p, total),
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
                // **Match the scrollback every other core is given.** ghostty's
                // defaults are unlimited lines and 10,000 *bytes*, which at its
                // documented ~400 KB page granularity keeps no history at all.
                // alacritty and vt100 get 10,000 lines above and baud carries
                // `SB = 10_000`; without this ghostty was the only core not
                // retaining what it scrolled.
                const OPT_SCROLLBACK_MAX_BYTES: i32 = 27;
                const OPT_SCROLLBACK_MAX_LINES: i32 = 28;
                let lines: usize = 10_000;
                let rs = unsafe {
                    ghostty::ghostty_terminal_set(
                        term,
                        OPT_SCROLLBACK_MAX_LINES,
                        (&raw const lines).cast(),
                    )
                };
                assert_eq!(rs, 0, "setting ghostty scrollback lines failed with {rs}");
                // NULL removes the byte limit, so lines are the only bound --
                // the same shape as the other three.
                let rb = unsafe {
                    ghostty::ghostty_terminal_set(term, OPT_SCROLLBACK_MAX_BYTES, std::ptr::null())
                };
                assert_eq!(
                    rb, 0,
                    "clearing ghostty scrollback byte limit failed with {rb}"
                );
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
        7 => baud_rep(p, total, baud_core2::OpMode::Computed),
        8 => baud_rep(p, total, baud_core2::OpMode::Templated),
        _ => unreachable!(),
    }
}
