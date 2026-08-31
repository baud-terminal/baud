//! The terminal's state, and the one borrow the parser holds.

use crate::graph::Node;
use crate::storage::{MAX_COLS, MAX_ROWS, Storage};
use baud_domain::*;

/// A terminal: a cursor, a rendition, a grid, and where the parser stands.
///
/// **All of it behind one `&mut self`.** A C prototype of this loop used
/// file-scope globals and ran 16-34% slower; `__restrict` recovered only 2.5%
/// of that. The presumed mechanism is LLVM's `noalias`, and the rule that
/// falls out is that state does not migrate to statics.
/// Which screen buffer is live.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Normal,
    Alternate,
}

/// What `DECSC` saves and `DECRC` restores.
#[derive(Clone, Copy)]
pub struct Saved {
    pub(crate) cy: Row,
    pub(crate) cx: Col,
    pub(crate) style: StyleId,
    pub(crate) style_raw: StyleWord,
    pub(crate) flags: CellFlags,
    pub(crate) charset: Charset,
}

/// How many `SGR` sequences the byte-keyed table holds.
///
/// **Measured against the corpus, not chosen.** The distinct plain-`SGR`
/// sequences are: `btop.raw` **168**, `asciiquarium.raw` **13**,
/// `git-log.raw` 7, `vim-unicode.raw` 6, `less-cjk.raw` 3 -- so 256 slots
/// hold every recording's whole working set and the table is exact rather
/// than lossy in practice. It still **evicts on collision**, the same
/// contract as the style and `OSC 8` link tables, because the key is
/// attacker-supplied content and a growable table is the content-addressing
/// failure mode this repo already records.
#[cfg(feature = "sgr-table")]
pub const NSGR: usize = 256;

/// The state a control sequence changes that the write loop does not read.
#[derive(Clone, Copy)]
pub struct Cold {
    /// Repeated `SGR` sequences, answered from their bytes.
    ///
    /// **`SGR` is 63% of `btop.raw`'s control sequences, 86% of
    /// `asciiquarium.raw`'s and 100% of `git-log.raw`'s**, and a ceiling arm
    /// that makes the whole route free measures **+34% on btop and +29% on
    /// asciiquarium**. Parsing one is a pure function of its bytes, and the
    /// result is a masked replace of the rendition word, so the whole route --
    /// the parameter walk, `SgrAttr::parse`, and the fold -- collapses to one
    /// compare and one `StyleFold::apply`.
    #[cfg(feature = "sgr-table")]
    pub(crate) sgr: [SgrSlot; NSGR],
    /// DECSCA protection, kept apart from the cell flags so the charset and
    /// the protection each change without recomputing the other wrongly.
    pub(crate) protect: CellFlags,
    pub(crate) screen: Screen,
    /// The scrolling region, inclusive at both ends.
    pub(crate) top: Row,
    pub(crate) bot: Row,
    pub(crate) saved: Option<Saved>,
    pub(crate) alt_saved: Option<Saved>,
    pub(crate) modes: Modes,
}

impl Cold {
    pub(crate) fn new(rows: Rows) -> Cold {
        Cold {
            #[cfg(feature = "sgr-table")]
            sgr: [SgrSlot::EMPTY; NSGR],
            protect: CellFlags::NONE,
            screen: Screen::Normal,
            top: Row::ZERO,
            bot: Row::from_index(rows.index() - 1),
            saved: None,
            alt_saved: None,
            // **DECAWM defaults on**, which is xterm's behaviour rather than
            // the VT510 manual's; `docs/vt-coverage.md` records the choice and
            // `baud-core2` makes the same one, so the differential compares
            // like with like.
            modes: Modes::default(),
        }
    }
}

pub struct Term<'b> {
    /// Segment hashes, for the `ceiling-segment-cache` arm only.
    #[cfg(feature = "ceiling-segment-cache")]
    pub(crate) seen: [SegHash; crate::feed::SEGS],
    pub(crate) store: Storage<'b>,
    /// Where the parser stands between reads.
    pub(crate) node: Node,
    pub(crate) encoding: InputEncoding,

    pub(crate) cy: Row,
    pub(crate) cx: Col,
    pub(crate) rows: Rows,
    pub(crate) cols: Cols,
    /// The distance between rows in the grid.
    ///
    /// **The live column count, not `MAX_COLS`.** Striding by the maximum put
    /// consecutive rows 16 KB apart, and on a 128 KB eight-way L1 the set
    /// index spans exactly 16 KB -- so every row of a 200-column screen mapped
    /// to the same sets, and any operation touching several rows thrashed an
    /// eight-way set with fifty of them. 78 KB of live cells were smeared over
    /// 800 KB of address space. Measured, cells byte-identical: **+16% on an
    /// ASCII payload that scrolls and +12% on one with newlines**, and flat on
    /// one that never leaves a row -- which is exactly the split a set
    /// conflict predicts.
    pub(crate) stride: Cols,
    pub(crate) rot: RowRot,

    pub(crate) style: StyleId,
    /// The rendition being *built*. SGR folds into this and interns the
    /// result: a fold reads the previous value, and an index cannot be read
    /// back through a table that evicts on collision.
    ///
    /// **Beside `style` rather than inside `cold`.** `asciiquarium.raw` folds
    /// into it 876,840 times a rep, which is nearly once a character.
    pub(crate) style_raw: StyleWord,
    pub(crate) flags: CellFlags,
    pub(crate) charset: Charset,
    /// Everything a control sequence changes that the per-character loop does
    /// not read. **One struct so `Paint::new` does not grow an argument per
    /// operation**, which is the shape that gets two of them swapped at a call
    /// site -- and so the hot fields above stay where they are.
    pub(crate) cold: Cold,

    pub(crate) utf8: Utf8Acc,
    /// The shortest-form floor for the lead byte's length. Without it every
    /// overlong decodes to a real character -- `E0 80 AF` renders `/`.
    pub(crate) utf8_min: Utf8Min,

    /// The parameters of the `CSI` being parsed, beside the node for the same
    /// reason `utf8` is.
    pub(crate) csi: crate::write::Csi,
    /// Bytes ingested, the denominator of every amplification figure.
    pub(crate) bytes_in: usize,
    /// Cells written, the numerator. Two arms of an experiment that disagree
    /// here are not doing the same work, whatever their timings say.
    pub(crate) cells_written: usize,
    /// Run-length and encoded-length histograms; see `Paint::runs`.
    ///
    /// **The block-shaped counters went with the block walk.** `blocks`,
    /// `block_runs` and `miss_first` counted iterations per sixteen-byte
    /// block, and there are no blocks -- leaving them would have printed
    /// `NaN%` from counters nothing feeds, which is the failure this repo
    /// records as "instrumentation outlived what fed it".
    #[cfg(feature = "diag")]
    pub runs: [usize; 8],
    #[cfg(feature = "diag")]
    pub run_len: [usize; 5],
    /// **Iterations of the ingest loop.** The unit this core is designed
    /// against: `CLAUDE.md` records that the floor tracks *bytes an iteration*
    /// and not what the actions do, and nothing here reported it. `bytes_in /
    /// iters` is that quantity, measured rather than reasoned from run
    /// histograms.
    #[cfg(feature = "diag")]
    pub iters: usize,
    /// Characters served by the three-byte permutation table.
    #[cfg(feature = "diag")]
    pub table_hits: usize,
    #[cfg(feature = "diag")]
    pub hist: crate::graph::Hist,
    #[cfg(feature = "diag")]
    pub unsupported: [usize; FinalByte::COUNT],
    #[cfg(feature = "diag")]
    pub scroll_rot: usize,
    #[cfg(feature = "diag")]
    pub scroll_copy: usize,
    pub fast_try: usize,
    #[cfg(feature = "diag")]
    pub fast_hit: usize,
    #[cfg(all(feature = "diag", feature = "sgr-table"))]
    pub sgr_hit: usize,
}

impl<'b> Term<'b> {
    /// Geometry comes from a [`WinSize`], which is the one place `ws_col` and
    /// `ws_row` are destructured -- so they cannot be transposed here.
    /// **Fill the three-byte cell table**: every `(b0,b1,b2)` a character can
    /// be, and the cell it becomes. 65,536 entries, once, at construction.
    ///
    /// This is the compile step of a compiled database that happens to run at
    /// startup, because 2 MiB is more than a `const fn` should evaluate and
    /// nothing in it depends on the terminal. `the_cell_table_agrees_with_the_
    /// decoder` walks all 65,536 against the path it replaces.
    ///
    /// **An empty slice is how a caller declines it, and that is the whole
    /// switch.** The only read is in `put_run` behind `cell-table`, so a
    /// default build that passes 2 MiB here spends 262,144 property-table
    /// lookups and 2 MiB of stores at construction on a table nothing will
    /// consult. `ingest2` and `multibench` were doing exactly that. Gating
    /// *this function* on the feature instead is wrong -- the table's own
    /// equivalence test builds it without the feature and reads the slice
    /// directly, which is how that mistake was caught.
    fn fill_cells3(store: &mut Storage<'b>) {
        if store.cells3.is_empty() {
            return;
        }
        for b0 in 0xE0usize..=0xEF {
            for b1 in 0x80usize..=0xBF {
                let row = ((b0 - 0xE0) * 64 + (b1 - 0x80)) * 256;
                // Table 3-7: `E0` needs `A0..BF` and `ED` needs `80..9F`, or the
                // sequence is overlong or a surrogate.
                let ok = !(b0 == 0xE0 && b1 < 0xA0) && !(b0 == 0xED && b1 > 0x9F);
                for b2 in 0x80usize..=0xBF {
                    let raw = Codepoint::from_three_bytes(b0, b1, b2);
                    let cp = match ok {
                        true => Codepoint::decoded(raw),
                        false => Codepoint::REPLACEMENT,
                    };
                    // **The entry carries everything the write needs**: the
                    // cell, how wide it is, and how far the cursor moves. Both are
                    // bounded functions of the three bytes, so both are settled
                    // here rather than at ingest.
                    let w = Classified::of(cp).width(AmbiguousWidth::Narrow);
                    store.cells3[row + b2] =
                        PackedCell::glyph(cp, StyleId::DEFAULT, CellFlags::of_entry(w));
                }
            }
        }
    }

    pub fn new(store: Storage<'b>, ws: &WinSize) -> Term<'b> {
        let mut store = store;
        Term::fill_cells3(&mut store);
        // **Every eighth column, and not column zero.** A tab stop at 0 makes
        // `CBT` from column 3 land where the cursor already was.
        for (i, stop) in store.tabs.iter_mut().enumerate() {
            *stop = i % 8 == 0 && i != 0;
        }
        let (cols, rows) = ws.geometry();
        let (cols, rows) = (cols.clamp(MAX_COLS), rows.clamp(MAX_ROWS));
        Term {
            #[cfg(feature = "ceiling-segment-cache")]
            seen: [SegHash::ZERO; crate::feed::SEGS],
            store,
            node: Node::Ground,
            encoding: InputEncoding::Utf8,
            cy: Row::ZERO,
            cx: Col::ZERO,
            rows,
            cols,
            stride: Cols::from_stride(cols.index()),
            rot: RowRot::NONE,
            style: StyleId::DEFAULT,
            style_raw: StyleWord::DEFAULT,
            flags: CellFlags::NONE,
            charset: Charset::Ascii,
            cold: Cold::new(rows),
            utf8: Utf8Acc::lead(0),
            utf8_min: Utf8Min::Two,
            csi: crate::write::Csi::NEW,
            bytes_in: 0,
            cells_written: 0,
            #[cfg(feature = "diag")]
            runs: [0; 8],
            #[cfg(feature = "diag")]
            run_len: [0; 5],
            #[cfg(feature = "diag")]
            iters: 0,
            #[cfg(feature = "diag")]
            table_hits: 0,
            #[cfg(feature = "diag")]
            hist: crate::graph::Hist::ZERO,
            #[cfg(feature = "diag")]
            unsupported: [0; FinalByte::COUNT],
            #[cfg(feature = "diag")]
            scroll_rot: 0,
            #[cfg(feature = "diag")]
            scroll_copy: 0,
            fast_try: 0,
            #[cfg(feature = "diag")]
            fast_hit: 0,
            #[cfg(all(feature = "diag", feature = "sgr-table"))]
            sgr_hit: 0,
        }
    }

    /// Bytes ingested and cells written, for the amplification ratio rule #1
    /// is about.
    pub fn amplification(&self) -> (usize, usize) {
        (self.bytes_in, self.cells_written)
    }

    /// The materialised viewport, un-rotated so a renderer gets a contiguous
    /// slice in screen order.
    ///
    /// **`&mut` is not a wart.** The rotation is undone here, once a *frame*,
    /// rather than on every scroll -- which is where the laziness that makes a
    /// scroll O(1) is paid for.
    /// The rendition a cell's [`StyleId`] stands for.
    ///
    /// **Read the slot back rather than trusting the id.** The table evicts on
    /// collision, so an id can outlive the rendition it was minted for; a
    /// renderer that trusted it would paint a stale colour.
    /// The distance between rows in the slice [`Term::viewport`] returns.
    ///
    /// **A renderer indexes by this, not by `MAX_COLS`.** The grid is
    /// contiguous at the live width, which is what keeps consecutive rows off
    /// the same L1 sets.
    pub fn stride(&self) -> Cols {
        self.stride
    }

    /// The rendition a cell's [`StyleId`] stands for.
    ///
    /// **Read the slot back rather than trusting the id.** The table evicts on
    /// collision, so an id can outlive the rendition it was minted for; a
    /// renderer that trusted it would paint a stale colour.
    pub fn style_of(&self, cell: PackedCell) -> StyleWord {
        self.store.tmpl[cell.style.index()].unwrap_or(StyleWord::DEFAULT)
    }

    pub fn viewport(&mut self) -> &[PackedCell] {
        let stride = self.stride;
        let live = RowBlock::leading(self.rows, stride);
        if self.rot.index() != 0 {
            // Un-rotated in place, once a *frame*, where the scroll that made
            // the rotation happens once a *scroll*.
            let turned = RowBlock::leading(Rows::of_index(self.rot.index()), stride);
            live.rotate_left_by(self.store.vp, turned);
            self.store.used[..self.rows.index()].rotate_left(self.rot.index());
            self.rot = RowRot::NONE;
        }
        live.of(self.store.vp)
    }

    /// Place bytes in the arena at `base`, the way a `read(2)` would.
    ///
    /// The core never copies on the parse path: a read writes here and the
    /// parser reads here. This is the seam, and it exists so a caller can hand
    /// the kernel `&mut arena[base..]` directly.
    pub fn write_at(&mut self, base: BytePos, src: &[u8]) {
        let from = base.index();
        self.store.arena[from..from + src.len()].copy_from_slice(src);
    }

    pub fn cursor(&self) -> (Row, Col) {
        (self.cy, self.cx)
    }
}
