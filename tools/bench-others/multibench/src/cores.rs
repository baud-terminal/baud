#[cfg(have_ghostty)]
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
    let ws = baud_ws();
    // **10,000 lines, the number every other core here is handed.** This arm
    // used `Owned::new`, which is `Depth::DEFAULT` = 2,000, while alacritty,
    // ghostty and vt100 each get 10,000 below and `MEASUREMENTS.md` states
    // that every core is configured alike. The ghostty comment beside it
    // still said "baud carries `SB = 10_000`" -- `baud-core2`'s constant,
    // which went with that crate -- so the harness had the ghostty trap
    // again with a different carrier: a dimension nobody set, in baud's own
    // favour this time. The ring measured ~7% on scrolling payloads at
    // 2,000; at 10,000 the ring is 16 MB at this pitch, and the published
    // table is re-taken with it rather than inherited.
    let mut owned = baud_owned(&ws);
    let mut t = baud_new(&mut owned, &ws);
    drive(p, total, |chunk| baud_feed(&mut t, chunk))
}

/// baud-core's geometry here: [`ROWS`] by [`COLS`].
pub(crate) fn baud_ws() -> baud_core::WinSize {
    baud_core::WinSize {
        ws_row: crate::ROWS as u16,
        ws_col: crate::COLS as u16,
    }
}

/// baud-core's storage as every rep and the screen check are given it.
pub(crate) fn baud_owned(ws: &baud_core::WinSize) -> baud_core::Owned {
    baud_owned_with(ws, IMAGE_BUDGET)
}

/// [`baud_owned`], its images given `budget`.
pub(crate) fn baud_owned_with(
    ws: &baud_core::WinSize,
    budget: baud_domain::ImageBudget,
) -> baud_core::Owned {
    use baud_core as nb;
    nb::Owned::with_history(ws, &nb::CacheGeometry::UNKNOWN, nb::Depth::of(10_000))
        .with_images(budget)
}

/// **The cell both baud-core and ghostty are given**, in pixels. A kitty
/// placement with no `c=` or `r=` covers the cells its pixels fill, so a
/// core given no cell size gives it no extent: ghostty then reads one on
/// the top row as off the screen.
pub(crate) const CELL_WIDTH_PX: u16 = 10;
/// The cell's height, as [`CELL_WIDTH_PX`] is its width.
pub(crate) const CELL_HEIGHT_PX: u16 = 20;

/// A baud-core terminal over `owned`, as every rep and the screen check
/// are given it: `ws` cells of [`CELL_WIDTH_PX`] by [`CELL_HEIGHT_PX`].
pub(crate) fn baud_new<'a>(
    owned: &'a mut baud_core::Owned,
    ws: &baud_core::WinSize,
) -> baud_core::Term<'a> {
    use baud_domain::ImagePx;
    let mut t = baud_core::Term::new(owned.storage(), ws);
    t.set_cell_size(ImagePx::of(CELL_WIDTH_PX), ImagePx::of(CELL_HEIGHT_PX));
    t
}

/// One read into baud-core, and the kitty decode it deferred run to
/// completion before the next, as the app steps it between reads: a PNG
/// or `o=z` image is placed inside the read and stored by
/// [`baud_core::Term::decode_step`], so without the steps the rep would
/// skip the decode ghostty does inside its own write of the bytes, and
/// the screen check would see no image.
pub(crate) fn baud_feed(t: &mut baud_core::Term<'_>, chunk: &[u8]) {
    use baud_domain::{DecodeBudget, DecodeProgress};
    t.write_at(baud_core::BytePos::ZERO, chunk);
    t.feed(
        baud_core::BytePos::ZERO,
        baud_core::ReadLen::from_index(chunk.len()),
    );
    while t.decode_step(DecodeBudget::ALL) == DecodeProgress::More {}
}

/// **The image budget both baud-core and ghostty are given**: the app's,
/// 95 MiB of RGBA pixels. A core that keeps images under a smaller budget
/// evicts where the other stores, so its image rows measure less work.
pub(crate) const IMAGE_BUDGET: baud_domain::ImageBudget =
    baud_domain::ImageBudget { pixels: 95 << 18 };

/// [`IMAGE_BUDGET`] in bytes, four per pixel, as ghostty's storage limit
/// counts them.
#[cfg(have_ghostty)]
const IMAGE_BUDGET_BYTES: u64 = (IMAGE_BUDGET.pixels as u64) * 4;

/// A ghostty terminal configured as every rep and the screen check are
/// given it: [`ROWS`] by [`COLS`] cells of [`CELL_WIDTH_PX`] by
/// [`CELL_HEIGHT_PX`], 10,000 lines of scrollback and [`IMAGE_BUDGET`].
/// The PNG decoder is the process's, set apart
/// ([`ghostty::set_png_decoder`]).
#[cfg(have_ghostty)]
pub(crate) fn ghostty_new() -> ghostty::Terminal {
    ghostty_with(GhosttyImages::Budget)
}

/// What ghostty's image storage is given.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum GhosttyImages {
    /// [`IMAGE_BUDGET`], as every rep has it.
    Budget,
    /// A limit of zero, which turns its image storage off: the count
    /// check's positive control.
    #[cfg(all(test, have_ghostty))]
    Off,
    /// A budget of this many pixels on both arms, four bytes each on
    /// ghostty: a quota a few small images fill.
    #[cfg(all(test, have_ghostty))]
    Small(usize),
}

impl GhosttyImages {
    /// The image budget baud-core is given beside it: [`IMAGE_BUDGET`],
    /// or the [`GhosttyImages::Small`] budget.
    #[cfg(have_ghostty)]
    pub(crate) fn baud(self) -> baud_domain::ImageBudget {
        match self {
            GhosttyImages::Budget => IMAGE_BUDGET,
            #[cfg(test)]
            GhosttyImages::Off => IMAGE_BUDGET,
            #[cfg(test)]
            GhosttyImages::Small(px) => baud_domain::ImageBudget::of(px),
        }
    }
}

/// A delete fed to ghostty alone before read `read` of the screen check
/// (`equal::check_reads`), outside the recording: `a=d,d=I` of image
/// `id`, which takes its placements with it.
#[cfg(all(test, have_ghostty))]
#[derive(Clone, Copy, Debug)]
pub(crate) struct Plant {
    pub(crate) read: usize,
    pub(crate) id: u32,
}

/// [`ghostty_new`], its image storage given `images`.
#[cfg(have_ghostty)]
pub(crate) fn ghostty_with(images: GhosttyImages) -> ghostty::Terminal {
    let mut term: ghostty::Terminal = std::ptr::null_mut();
    let rc = unsafe {
        ghostty::ghostty_terminal_new(std::ptr::null(), &mut term, COLS as u16, ROWS as u16)
    };
    assert_eq!(rc, 0, "ghostty_terminal_new failed with {rc}");
    // The same grid again, now with the cell's pixels.
    let rz = unsafe {
        ghostty::ghostty_terminal_resize(
            term,
            COLS as u16,
            ROWS as u16,
            u32::from(CELL_WIDTH_PX),
            u32::from(CELL_HEIGHT_PX),
        )
    };
    assert_eq!(rz, 0, "setting ghostty's cell size failed with {rz}");
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
        ghostty::ghostty_terminal_set(term, OPT_SCROLLBACK_MAX_LINES, (&raw const lines).cast())
    };
    assert_eq!(rs, 0, "setting ghostty scrollback lines failed with {rs}");
    // NULL removes the byte limit, so lines are the only bound --
    // the same shape as the other three.
    let rb =
        unsafe { ghostty::ghostty_terminal_set(term, OPT_SCROLLBACK_MAX_BYTES, std::ptr::null()) };
    assert_eq!(
        rb, 0,
        "clearing ghostty scrollback byte limit failed with {rb}"
    );
    // **Images under the budget baud-core is given.** The library's
    // default is 10 MB per screen.
    const OPT_KITTY_IMAGE_STORAGE_LIMIT: i32 = 15;
    let limit: u64 = match images {
        GhosttyImages::Budget => IMAGE_BUDGET_BYTES,
        #[cfg(all(test, have_ghostty))]
        GhosttyImages::Off => 0,
        #[cfg(all(test, have_ghostty))]
        GhosttyImages::Small(px) => (px as u64) * 4,
    };
    let ri = unsafe {
        ghostty::ghostty_terminal_set(
            term,
            OPT_KITTY_IMAGE_STORAGE_LIMIT,
            (&raw const limit).cast(),
        )
    };
    assert_eq!(
        ri, 0,
        "setting ghostty's image storage limit failed with {ri}"
    );
    term
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
                // **A PNG is decoded, as baud-core decodes it.** With no decoder
                // registered, the library refuses an `f=100` transmit up front
                // and does no image work at all.
                ghostty::set_png_decoder(ghostty::PngDecoder::Registered);
                let term = ghostty_new();
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
            let size = |n: usize| std::num::NonZero::new(n as u16).expect("the grid has cells");
            let mut parser = vt100::Parser::new(size(ROWS), size(COLS), 10_000);
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

#[cfg(test)]
mod tests {
    /// **A PNG is stored by the read that ends it.** The placeholder
    /// recording's `a=T,f=100,U=1` image, fed through [`super::baud_feed`]
    /// in the chunks every rep reads, leaves no decode to step, and its
    /// virtual placement shows the 60x40 image: a rep's figure includes the
    /// decode, and the screen check reads the image the recording draws.
    #[test]
    fn a_png_is_stored_by_the_read_that_ends_it() {
        use baud_domain::{DecodeBudget, DecodeProgress, KittyId, KittyPlacement};
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/recordings/kitty/grad-80x24-placeholder.raw"
        );
        let p = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let ws = super::baud_ws();
        let mut owned = super::baud_owned(&ws);
        let mut t = super::baud_new(&mut owned, &ws);
        crate::transport::through_memory(&p, p.len(), |chunk| super::baud_feed(&mut t, chunk));
        assert_eq!(
            t.decode_step(DecodeBudget::of(1)),
            DecodeProgress::Idle,
            "no decode is left for a later step"
        );
        let v = t
            .kitty_virtual(KittyId::of(3_148_373_296), KittyPlacement::NONE)
            .expect("the recording's virtual placement");
        let image = t.images().image(v.image).expect("its image, stored");
        assert_eq!((image.width.index(), image.height.index()), (60, 40));
    }
}
