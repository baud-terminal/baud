use crate::ffi::ghostty;
use crate::*;
use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::term::{Config, Term as ATerm};
use alacritty_terminal::vte::ansi::Processor;

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

/// `baud-core`: caller-provided storage, no allocator of its own.
///
/// **Every buffer is the harness's**, which is the contract the crate is built
/// to -- it is `no_std`, `alloc`-free and borrows all of it. Built fresh per rep
/// like every other core here.
///
/// **This arm had not compiled since `StrBuf` was added to `Storage::new`.** It
/// spelled all eleven borrows out by hand, so every field the struct gained was
/// a build error here -- and `multibench` is outside the workspace, so
/// `cargo test --workspace` never compiled it and nothing went red. That is the
/// same rot as `docs.rs`'s `ROOTS` list and the CI step pointing at a package
/// with no such target: not a gate failing, a gate absent. It matters more than
/// most, because `multibench` is the harness that can resolve a change where
/// `baud-bench` could not.
///
/// `Owned` is the fix rather than an eleventh argument: it is the same
/// convenience the tests use, it declines the three-byte cell table the way a
/// default caller should, and a twelfth field cannot break it.
pub(crate) fn baud_rep(p: &[u8], total: usize) -> f64 {
    use baud_core as nb;
    let ws = nb::WinSize {
        ws_row: crate::ROWS as u16,
        ws_col: crate::COLS as u16,
    };
    // **10,000 lines, the number every other core here is handed.** This arm
    // used `Owned::new`, which is `Depth::DEFAULT` = 2,000, while alacritty,
    // ghostty and vt100 each get 10,000 below and `MEASUREMENTS.md` states
    // that every core is configured alike. The ghostty comment beside it
    // still said "baud carries `SB = 10_000`" -- `baud-core2`'s constant,
    // which went with that crate -- so the harness had the ghostty trap
    // again with a different carrier: a dimension nobody set, in baud's own
    // favour this time. `CLAUDE.md` measures the ring at ~7% on scrolling
    // payloads at 2,000; at 10,000 the ring is 16 MB at this pitch, and the
    // published table is re-taken with it rather than inherited.
    let mut owned =
        nb::Owned::with_history(&ws, &nb::CacheGeometry::UNKNOWN, nb::Depth::of(10_000));
    let mut t = nb::Term::new(owned.storage(), &ws);
    drive(p, total, |chunk| {
        t.write_at(nb::BytePos::ZERO, chunk);
        t.feed(nb::BytePos::ZERO, nb::ReadLen::from_index(chunk.len()));
    })
}

/// **sfte is measured at 2,000 lines of scrollback, and every other core at
/// 10,000, and that is stated rather than hidden.** sfte's depth is a
/// compile-time macro in the oracle's shim, where 2,000 is what makes the
/// differential like-for-like with `baud-core`'s `Depth::DEFAULT`; this
/// harness cannot change it per arm without a second shim, which would be a
/// second description of how sfte is driven. The unfairness runs toward sfte
/// on the scroll-heavy rows -- a smaller ring stays closer to cache -- and
/// the table says so in the row label.
pub(crate) const SFTE_SCROLLBACK: usize = 2_000;

/// The admissibility gate for the sfte row: after one rep of the same replay,
/// do baud-core and sfte hold the same screen? sfte has no `cells_written`,
/// so a screen digest is the only cross-implementation counter there is,
/// and a throughput row whose arms drew different screens is the failure
/// this repository has recorded three times. Returns the two digests.
pub(crate) fn sfte_admissible(p: &[u8], total: usize) -> (u64, u64) {
    use baud_core as nb;
    use baud_sfte_difftest as sd;
    let mut owned = nb::Owned::with_history(
        &sd::WS,
        &nb::CacheGeometry::UNKNOWN,
        nb::Depth::of(SFTE_SCROLLBACK),
    );
    let mut ours = sd::Ours::new(&mut owned);
    let mut theirs = sd::Sfte::new();
    let mut norm = sd::Normalised::new();
    drive(p, total, |chunk| {
        ours.feed(chunk);
        theirs.feed(chunk);
    });
    (ours.screen().digest(), theirs.screen(&mut norm).digest())
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
        1 => baud_rep(p, total),
        2 => {
            let cfg = Config {
                scrolling_history: 10_000,
                ..Default::default()
            };
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
                // alacritty and vt100 get 10,000 lines and so does baud, through
                // `Owned::with_history`; without this ghostty was the only core
                // not retaining what it scrolled.
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
        // sfte, exactly as `baud-sfte-difftest` drives it: 200x50 at compile
        // time (its pixel resize divides by a fontless cell width of zero),
        // the locale asserted to be `C.UTF-8` (its width is libc `wcwidth`,
        // and a headless build never sets one), scrollback 2,000 -- see
        // `SFTE_SCROLLBACK` below -- and the BEL-terminated-OSC patch applied
        // by that crate's build.rs. `sfte_parse` is byte-at-a-time and writes
        // one 36-byte cell per character; the comparison is what those cost
        // against a run path, and the shim adds one call per chunk to it.
        7 => {
            let mut t = baud_sfte_difftest::Sfte::new();
            drive(p, total, |chunk| t.feed(chunk))
        }
        _ => unreachable!(),
    }
}
