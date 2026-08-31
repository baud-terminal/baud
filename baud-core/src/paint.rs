//! One borrow of the grid, held for a whole read.
//!
//! **Why this type exists.** `ceiling-no-write` prices the write path at 49%
//! of this core, and most of that is not the stores -- it is building a
//! `View`. A `View` is six fields and a bounds re-derivation, and the block
//! walk was building one per *run*: on `cat-cjk.raw` a run is a handful of
//! bytes, so the setup was paid thousands of times where the stores it guards
//! are eight cells each.
//!
//! This repo already records the same failure one layer down -- "a view/borrow
//! helper was constructed inside a per-cell loop ... bulk operations belong on
//! the view, not per cell". This is that rule applied per *read* rather than
//! per run.
//!
//! The cursor lives here too, because it moves on every write and reading it
//! back through `Term` each time would put the borrow back.

use crate::storage::Storage;
use baud_domain::*;

/// A text run at or below this length goes a character at a time.
///
/// Two, re-measured under the one-line loop rather than inherited from the
/// block walk that first set it: btop **1077-1085 at two against 995-1018 at
/// four**, asciiquarium 511-522 against 462-464, cat-cjk flat between one and
/// two and down at three. The blitter's setup -- row resolution, orphan
/// repair, extent -- is what a one- or two-character run cannot amortise.
const SHORT_TEXT: usize = 2;

/// The grid, the bytes, and the cursor, borrowed together for one read.
pub(crate) struct Paint<'a> {
    pub(crate) arena: &'a [u8],
    /// The three-byte cell table; empty when the core is computing instead.
    ///
    /// **Compiling it out when the feature is off is worth nothing**, which is
    /// the boundary of the placement rule this file records five wins from. A
    /// field nothing *reads* costs nothing -- LLVM has already removed it. What
    /// costs is a field the hot path *writes* (`last_cp` at 16%, `style_raw` at
    /// 3.5%) or a large struct *copied* through the loop (`Cold` at 120 bytes,
    /// 4%). Two dead words are free: btop -1%, less-pager -2%, the rest flat.
    #[cfg(feature = "cell-table")]
    pub(crate) cells3: &'a [PackedCell],
    /// The rendition table, written by SGR and read by a renderer.
    pub(crate) tmpl: &'a mut [Option<StyleWord>],
    /// Tab stops.
    pub(crate) tabs: &'a mut [bool],
    pub(crate) view: View<'a>,
    pub(crate) cy: Row,
    pub(crate) cx: Col,
    pub(crate) rows: Rows,
    pub(crate) cols: Cols,
    pub(crate) style: StyleId,
    pub(crate) flags: CellFlags,
    pub(crate) charset: Charset,
    /// The rendition being built, which `SGR` folds into.
    ///
    /// **A flat field and not part of `cold`, for the reason `last_cp` is.**
    /// `asciiquarium.raw` sends **876,840 SGRs a rep** against about a million
    /// characters, so this is written nearly as often as a cell is -- and a
    /// twenty-byte read-modify-write inside the struct that also carries the
    /// region, the modes and the saved cursor keeps that struct out of
    /// registers for the whole loop.
    pub(crate) style_raw: StyleWord,
    /// What `REP` repeats. **A flat field and not part of `cold`**: it is
    /// written once per character, and a per-character store into the cold
    /// struct is what keeps that struct out of registers for the whole loop.
    pub(crate) last_cp: Option<(Codepoint, Width)>,
    /// The state a control sequence changes and the write loop never reads.
    ///
    /// **A reference, and it used to be by value.** `Cold` is **120 bytes** --
    /// two `Option<Saved>` at 48 each, for `DECSC` and the alternate screen's
    /// save, which no payload touches more than a handful of times -- and
    /// `Paint` is inlined into the per-character loop. Carrying it meant
    /// carrying all of that; an indirection costs one load on a path taken
    /// once a control sequence.
    /// Bytes owed to the host, and how many are pending.
    ///
    /// **A slice rather than a report queued for later.** A single read can
    /// carry several queries -- an application probing on startup sends `DA`
    /// and `DSR` together -- so there is nothing to hold them in but the
    /// buffer itself.
    pub(crate) reply: &'a mut [u8],
    pub(crate) reply_len: ByteLen,
    pub(crate) reply_overflows: usize,
    pub(crate) cold: &'a mut crate::term::Cold,
    /// Cells written this read, the numerator of the amplification ratio.
    /// Two arms of an experiment that disagree here are not doing the same
    /// work, whatever their timings say.
    pub(crate) cells: Cells,
    /// How far the *current* row is known to be written, kept in a register.
    ///
    /// **This is what makes the orphan probe skippable.** `put_glyph` reads
    /// the cells either side of a write to repair a split wide character;
    /// past the row's used extent there is nothing to repair, because a blank
    /// is not a `WIDE_TAIL`. Asking `View` for the extent instead is a load
    /// from a second array -- a different cache line -- and that is exactly
    /// how the same idea measured *slower* in the previous core. Held here it
    /// is a compare against a value already in flight.
    ///
    /// **Zero on a row change means the *fresh* path, not the probing one**,
    /// and an earlier version of this comment claimed the opposite -- a zero
    /// makes `cx >= row_written` true, which is the branch that skips the
    /// repair. It is safe here only because of what this core can do: the
    /// cursor moves between rows by `LF`/`VT`/`FF` and by wrapping, never
    /// upward and never by absolute address, so the row it lands on is either
    /// one nothing has written or one the scroll has just blanked.
    ///
    /// **`CUP` breaks that.** The first operation that moves the cursor to an
    /// arbitrary row lands it on text, and the first write there will orphan
    /// whatever wide character it half-covers. Whoever adds cursor addressing
    /// has to make this a real per-row extent -- `View::extent` already holds
    /// one -- rather than a per-read high-water mark.
    row_written: Col,
    /// How many runs of each character count the loop took, and of each
    /// encoded length. **Do not read a bulk path as covering a character class
    /// -- count how often it is entered**, which is the mistake this repo
    /// records against a collector that took 0.0 characters an entry.
    #[cfg(feature = "diag")]
    pub(crate) runs: [usize; 8],
    #[cfg(feature = "diag")]
    pub(crate) run_len: [usize; 5],
    /// Characters whose cell came from the permutation table rather than from
    /// a decode. **Coverage is the question a throughput number cannot
    /// answer**: the table serves three-byte characters that arrive alone, and
    /// counting the entries is the only way to say what share of a recording
    /// that is.
    #[cfg(feature = "diag")]
    pub(crate) table_hits: usize,
    /// Iterations that started in each node, and the action each took.
    /// **Which node the bytes are in decides what a wider key must cover** --
    /// a k-stride table over `CsiParam` is a different object from one over
    /// `Ground`, and nothing here reported the split.
    #[cfg(feature = "diag")]
    pub(crate) hist: crate::graph::Hist,
    /// Final bytes that arrived and were not acted on, by final byte.
    /// **The work list, counted rather than read off the enum** -- 54 of the
    /// 63 finals are unimplemented and the corpus sends a handful of them.
    #[cfg(feature = "diag")]
    pub(crate) unsupported: [usize; FinalByte::COUNT],
    /// Accelerated `CSI` attempts, and the ones that committed. **A bulk path
    /// that declines still pays its scan**, which is a rule this repo already
    /// records; the only way to know which is happening is to count entries.
    /// Scrolls that rotated an index, and scrolls that copied the rows.
    /// **The difference between them is 45%** by this repo's own measurement,
    /// and nothing counted which one was happening.
    #[cfg(feature = "diag")]
    pub(crate) scroll_rot: usize,
    #[cfg(feature = "diag")]
    pub(crate) scroll_copy: usize,
    #[cfg(feature = "diag")]
    pub(crate) fast_try: usize,
    #[cfg(feature = "diag")]
    pub(crate) fast_hit: usize,
    #[cfg(all(feature = "diag", feature = "sgr-table"))]
    pub(crate) sgr_hit: usize,
}

impl<'a> Paint<'a> {
    /// **One destructure, one view.** `arena` and `vp` are different fields of
    /// the same `Storage`, so splitting the single `&mut` is what lets a run be
    /// blitted straight out of the arena with no copy in between.
    pub(crate) fn new(
        store: &'a mut Storage<'_>,
        cy: Row,
        cx: Col,
        rows: Rows,
        cols: Cols,
        rot: RowRot,
        stride: Cols,
        style: StyleId,
        style_raw: StyleWord,
        flags: CellFlags,
        charset: Charset,
        cold: &'a mut crate::term::Cold,
        reply_len: ByteLen,
    ) -> Paint<'a> {
        let Storage {
            arena,
            vp,
            used,
            cells3,
            tmpl,
            tabs,
            reply,
        } = store;
        // The three-byte cell table is read only by `put_run` under
        // `cell-table`; the default build must not carry a borrow of it.
        #[cfg(not(feature = "cell-table"))]
        let _ = cells3;
        Paint {
            arena,
            #[cfg(feature = "cell-table")]
            cells3,
            tmpl,
            tabs,
            reply,
            reply_len,
            reply_overflows: 0,
            cold,
            view: View::new(vp, used, stride, cols, rows, rot),
            cy,
            cx,
            rows,
            cols,
            style,
            flags,
            charset,
            style_raw,
            last_cp: None,
            cells: Cells::ZERO,
            row_written: Col::ZERO,
            #[cfg(feature = "diag")]
            runs: [0; 8],
            #[cfg(feature = "diag")]
            run_len: [0; 5],
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
            #[cfg(feature = "diag")]
            fast_try: 0,
            #[cfg(feature = "diag")]
            fast_hit: 0,
            #[cfg(all(feature = "diag", feature = "sgr-table"))]
            sgr_hit: 0,
        }
    }

    /// A run of printable ASCII, blitted eight cells to a store.
    ///
    /// **A short run does not want the blitter.** `put_ascii_run` resolves the
    /// row, repairs whatever the run orphans and then runs a vectored loop
    /// eight cells at a time; for one or two characters that is all setup and
    /// no vector. `cat-cjk.raw` is 39% ASCII by character and interleaved, so
    /// its text runs are short -- the same shape that made a dedicated
    /// single-character path worth 12% on the multi-byte side.
    /// **A run wraps, and it wraps in the same place a single character
    /// does.** `blit` clipped to the row and returned; `glyph` wraps. While
    /// runs were capped at a block that difference was invisible, because a
    /// run reaching the margin was rare and the next one found no room and was
    /// dropped in silence. The graph loop hands over runs as long as the text
    /// is, so a 300-byte line arrives as one call and the whole tail past the
    /// margin disappeared -- 3.6M cells of `btop.raw`, and the screen simply
    /// stopped accepting text.
    ///
    /// **Chunk invariance did not catch it and could not.** The drop was
    /// driven by the screen being full, not by where the read fell, so every
    /// chunk size agreed with every other on the same wrong answer.
    ///
    /// The run that fits is the whole path here, and the wrapping case is
    /// outlined behind it: writing the fits-case through the general loop cost
    /// **7% of `cat-cjk.raw` with the cells byte-identical**, which is a
    /// payload that wraps on almost nothing.
    #[inline(always)]
    pub(crate) fn text(&mut self, from: BytePos, n: ByteLen) {
        match n.index() <= self.cols.index() - self.cx.index() {
            true => self.piece(from, n),
            false => self.wrapped(from, n),
        }
    }

    /// As much of a run as the row has room for, by whichever writer fits.
    ///
    /// **A short run does not want the blitter.** `put_ascii_run` resolves the
    /// row, repairs whatever the run orphans and then runs a vectored loop
    /// eight cells at a time; for one or two characters that is all setup and
    /// no vector. `cat-cjk.raw` is 39% ASCII by character and interleaved, so
    /// its text runs are short.
    #[inline(always)]
    fn piece(&mut self, from: BytePos, n: ByteLen) {
        let k = n.index();
        #[cfg(feature = "ceiling-no-text")]
        {
            self.cells = self.cells.plus(k);
            self.cx = Col::at_index(self.cx.index() + k);
            self.row_written = self.row_written.max(self.cx);
            return;
        }
        #[allow(unreachable_code)]
        if k <= SHORT_TEXT {
            for i in 0..k {
                let b = StreamByte::from_stream(self.arena[from.index() + i]);
                self.glyph(PackedCell::translate(b, self.charset), Width::NARROW);
            }
            return;
        }
        self.blit(from, n);
    }

    /// A run that runs off the row: row-piece by row-piece, wrapping between.
    #[inline(never)]
    fn wrapped(&mut self, from: BytePos, n: ByteLen) {
        let (mut at, mut left) = (from.index(), n.index());
        while left > 0 {
            if self.cx.index() >= self.cols.index() {
                self.wrap();
            }
            let k = left.min(self.cols.index() - self.cx.index());
            self.piece(BytePos::from_index(at), ByteLen::from_index(k));
            at += k;
            left -= k;
        }
    }

    /// The vectored form, for a run long enough to fill a store.
    #[inline(always)]
    fn blit(&mut self, from: BytePos, n: ByteLen) {
        // No clip: `text` sized the piece to the row before calling. Clipping
        // here is what silently dropped the tail of a long line.
        let k = n.index();
        let src = &self.arena[from.index()..from.index() + k];
        self.view
            .put_ascii_run(self.cy, self.cx, src, self.style, self.flags, self.charset);
        self.cells = self.cells.plus(k);
        self.cx = Col::at_index(self.cx.index() + k);
        self.row_written = self.row_written.max(self.cx);
    }

    /// A cell that was **looked up, not computed** -- content and width came
    /// from one load into the three-byte table, so nothing here decodes,
    /// classifies or constructs a codepoint.
    ///
    /// The style and flags are the terminal's current ones; the entry's are a
    /// carrier for the width and are replaced here.
    #[inline(always)]
    #[cfg(feature = "cell-table")]
    pub(crate) fn indexed(&mut self, e: PackedCell) {
        let need = e.flags.advance_of();
        if self.cx.index() + need > self.cols.index() {
            self.wrap();
        }
        let (style, flags) = (self.style, self.flags);
        let lead = PackedCell {
            content: e.content,
            style,
            flags,
        };
        let tail = match e.flags.is_wide() {
            true => Some(PackedCell::wide_tail(style, flags)),
            false => None,
        };
        match self.cx >= self.row_written {
            true => self.view.put_glyph_fresh(self.cy, self.cx, lead, tail),
            false => {
                self.view
                    .put_glyph(self.cy, self.cx, lead, tail, PackedCell::empty_with(style))
            }
        }
        self.cells = self.cells.plus(need);
        self.cx = Col::at_index(self.cx.index() + need);
        self.row_written = self.row_written.max(self.cx);
    }

    /// One glyph under the cursor, wrapping first if it will not fit.
    #[inline(always)]
    pub(crate) fn glyph(&mut self, cp: Codepoint, w: Width) {
        let need = 1 + usize::from(w == Width::WIDE);
        if self.cx.index() + need > self.cols.index() {
            self.wrap();
        }
        let (style, flags) = (self.style, self.flags);
        // **Making the wide tail implicit is worth 3%, not the 23% the store
        // accounting implied.** A wide character occupies two cells and writes
        // both, which is two stores on the path 69% of runs take, so a width
        // bit on the lead instead of a `WIDE_TAIL` cell looked like halving
        // the dominant store category. Priced with a ceiling arm that simply
        // does not write the tail -- incorrect by construction, since nothing
        // then marks the covered column -- it measures **712-745 against
        // 712-723**.
        //
        // The reason is locality: the tail lands on the cache line the lead
        // has just dirtied, so the second store is nearly free, and the runs
        // long enough to matter already go through `put_wide_run` eight cells
        // at a time. This is the same shape as blanking, which is 46% of every
        // cell written and 3% of the clock. **Cell traffic is not the cost
        // here, and a store-count argument is not a timing argument.**
        //
        // Worth knowing because the change it argued for -- a different grid
        // representation, with readers checking the preceding cell -- is
        // invasive and shared with `baud-core2`, and it would have bought 3%.
        #[cfg(feature = "ceiling-implicit-tail")]
        let tail: Option<PackedCell> = None;
        #[cfg(not(feature = "ceiling-implicit-tail"))]
        let tail = match w == Width::WIDE {
            true => Some(PackedCell::wide_tail(style, flags)),
            false => None,
        };
        let lead = PackedCell::glyph(cp, style, flags);
        // What `REP` repeats, recorded where the character is written.
        self.last_cp = Some((cp, w));
        // **The whole per-character write, priced by doing it twice.**
        // `put_glyph_fresh` is idempotent -- same row, same column, same cell,
        // same extent -- so the second call changes nothing and costs exactly
        // one addressing plus one pair of stores. This is what `ceiling-no-
        // write` could not price, because stopping the write also stops the
        // cursor and with it every wrap and every scroll.
        #[cfg(feature = "probe-double-put")]
        if self.cx >= self.row_written {
            self.view.put_glyph_fresh(self.cy, self.cx, lead, tail);
        }
        match self.cx >= self.row_written {
            true => self.view.put_glyph_fresh(self.cy, self.cx, lead, tail),
            false => {
                self.view
                    .put_glyph(self.cy, self.cx, lead, tail, PackedCell::empty_with(style))
            }
        }
        self.cells = self.cells.plus(need);
        self.cx = Col::at_index(self.cx.index() + need);
        self.row_written = self.row_written.max(self.cx);
    }

    /// A run of one width, eight cells to a store.
    ///
    /// **One body for both writers**, because they differ only in how many
    /// cells a character takes: `put_wide_run` swizzles a lead and its tail
    /// four characters at a time, `put_narrow_run` writes eight characters.
    /// The width test is once a *run*, not once a character, so it does not
    /// re-introduce the runtime-flag cost that folding the character arms did.
    ///
    /// **Held across blocks was tried and lost.** Accumulating a run into a
    /// 32-character buffer and flushing on a cursor-observing operation
    /// measured **465-480 against 508-521**, and `cells_written` rose 8.33M to
    /// 8.85M -- it also dropped characters at the row edge, which the counters
    /// showed and the tests did not.
    #[inline(always)]
    pub(crate) fn run(&mut self, cps: &[Codepoint], w: Width) {
        let (style, flags) = (self.style, self.flags);
        match w == Width::WIDE {
            true => self.view.put_wide_run(self.cy, self.cx, cps, style, flags),
            false => self
                .view
                .put_narrow_run(self.cy, self.cx, cps, style, flags),
        };
        let n = cps.len() * (1 + usize::from(w == Width::WIDE));
        self.cells = self.cells.plus(n);
        self.cx = Col::at_index(self.cx.index() + n);
        self.row_written = self.row_written.max(self.cx);
    }

    /// Does a run of `n` characters of width `w` fit the rest of the row?
    ///
    /// A question rather than a number, so no count crosses the boundary and
    /// gets converted. Returning `Count` -- which is a `u16` VT parameter
    /// count, not a column count -- put a narrowing cast on each side and
    /// measured **983 against 1016 on `cat`**, a payload that never reaches
    /// this function at all: the cost was the conversions, not the work.
    ///
    /// It has to be asked, not assumed: `put_wide_run` and `put_narrow_run`
    /// both **silently truncate** to the row and return an extent that
    /// `Paint::run` discards.
    #[inline(always)]
    pub(crate) fn fits(&self, n: Count, w: Width) -> bool {
        n.index() * (1 + usize::from(w == Width::WIDE)) <= self.cols.index() - self.cx.index()
    }

    /// Move the cursor anywhere on the screen.
    ///
    /// **`row_written` is re-read from the grid here, and that is the whole
    /// reason this function is not two assignments.** It is a per-*read* high
    /// water mark that selects the probe-free write path, and it was safe only
    /// while the cursor could not move upward or by absolute address. `CUP`
    /// breaks exactly that: it lands on a row holding text, and a first write
    /// there would take `put_glyph_fresh` and orphan whatever wide character
    /// it half-covers. `View::extent` already holds the real per-row mark.
    pub(crate) fn goto(&mut self, r: Row, c: Col) {
        let r = Row::from_index(r.index().min(self.rows.index() - 1));
        self.cy = r;
        self.cx = Col::at_index(c.index().min(self.cols.index() - 1));
        self.deferred_reset();
        self.row_written = Col::at_index(self.view.extent(r).index());
    }

    /// Erase within the cursor's row. 0 to the end, 1 from the start, 2 all.
    pub(crate) fn erase_line(&mut self, e: EraseExtent) {
        let (cy, cols) = (self.cy, self.cols);
        let (a, b) = match e {
            EraseExtent::ToStart => (Col::ZERO, Col::at_index(self.cx.index() + 1)),
            EraseExtent::All => (Col::ZERO, Col::at_index(cols.index())),
            EraseExtent::ToEnd => (self.cx, Col::at_index(cols.index())),
        };
        let blank = PackedCell::empty_with(self.style);
        let n = self.view.fill(cy, a, b, blank);
        self.cells = self.cells.plus(n);
    }

    /// Erase the screen. 0 below the cursor, 1 above it, 2 all of it.
    pub(crate) fn erase_display(&mut self, e: EraseExtent) {
        let (rows, cols) = (self.rows.index(), Col::at_index(self.cols.index()));
        let blank = PackedCell::empty_with(self.style);
        let (from, to) = match e {
            EraseExtent::ToStart => (0, self.cy.index()),
            EraseExtent::All => (0, rows),
            EraseExtent::ToEnd => (self.cy.index() + 1, rows),
        };
        for y in from..to {
            let n = self.view.fill(Row::from_index(y), Col::ZERO, cols, blank);
            self.cells = self.cells.plus(n);
        }
        // The cursor's own row is a partial erase either side of it.
        match e {
            EraseExtent::All => {}
            e => self.erase_line(e),
        }
    }

    /// **DECAWM off means the cursor stays put and the character overwrites**,
    /// which is what a status line drawn to the last column depends on.
    /// Queue a report for the host.
    ///
    /// **The only way bytes reach the reply buffer**, so the shape is chosen
    /// by the [`Report`] variant rather than by the call site: a `DSR` cannot
    /// be answered with a `DA`'s bytes, which `docs/vt-coverage.md` calls the
    /// one thing a terminal must never do.
    ///
    /// **Seven-bit introducers, always.** This core implements neither `S7C1T`
    /// nor `S8C1T`, so it has never been told to move to the 8-bit C1 forms,
    /// and passing a mode it does not track would be recording a bit nothing
    /// sets.
    #[cold]
    #[inline(never)]
    pub(crate) fn want_reply(&mut self, r: Report) {
        let bytes = r.encode(C1Transmit::SevenBit);
        let b = bytes.as_bytes();
        let at = self.reply_len.index();
        match at + b.len() <= self.reply.len() {
            true => {
                self.reply[at..at + b.len()].copy_from_slice(b);
                self.reply_len = ByteLen::from_index(at + b.len());
            }
            // A truncated reply is a wrong answer, so it is dropped whole and
            // counted rather than half-sent.
            false => self.reply_overflows += 1,
        }
    }

    pub(crate) fn wrap(&mut self) {
        if !self.cold.modes.is_set(ModeBit::AutoWrap) {
            self.cx = Col::at_index(self.cols.index() - 1);
            return;
        }
        self.cx = Col::ZERO;
        self.index();
    }

    /// One row up, scrolling the region when the cursor is on its first row.
    pub(crate) fn reverse_index(&mut self) {
        if self.cy == self.cold.top {
            self.row_changed();
            self.scroll_region_down(Count::ONE);
            return;
        }
        if self.cy.index() > 0 {
            self.row_changed();
            self.cy = Row::from_index(self.cy.index() - 1);
        }
    }

    /// **Caching the row's resolved address was tried and lost.** `phys` is a
    /// conditional subtract and the stride is a multiply, both paid per glyph
    /// for an answer that changes only when the cursor changes rows -- so a
    /// `RowBase` held here should have moved them to once a *row*. Lazily,
    /// behind an `Option`, it measured flat: the test per glyph cost the
    /// saving. Eagerly, resolved whenever the row changes, it measured
    /// **608-618 against 621-632** -- because `cat-cjk.raw` has a newline
    /// every 78 bytes, so "once a row" is not rare, and a rotation invalidates
    /// every row's address at once. LLVM keeps the inline form cheap and there
    /// was nothing to hoist.
    ///
    /// A new row: nothing is known about it yet, so the probe comes back.
    #[inline(always)]
    fn deferred_reset(&mut self) {}

    fn row_changed(&mut self) {
        self.row_written = Col::ZERO;
    }

    /// The cursor now sits on `r`, whose contents an operation has just
    /// changed. **`row_written` is a shortcut, not a fact**: past it the
    /// orphan probe is skipped because the cells are blank, and an operation
    /// that moves text within a row invalidates that. Re-reading the view's
    /// real extent is what keeps a wide character from being split in half by
    /// the next write.
    pub(crate) fn row_changed_to(&mut self, r: Row) {
        self.row_written = Col::at_index(self.view.extent(r).index());
    }

    /// Down one row, scrolling when that would leave the screen.
    ///
    /// The scroll is a rotation: the view advances its own and the only cells
    /// written are the incoming row's, which stop at its used extent.
    pub(crate) fn index(&mut self) -> RowRot {
        // **Ceiling arm: scrolling, priced whole.** The profile puts `index`
        // at 7.6% of cat-cjk and it is the one named frame that survived
        // inlining, so it is worth an arm rather than an argument. Incorrect
        // by construction -- the screen stops moving -- and `cells_written`
        // falls by exactly the blanking, which is how the pair is checked.
        #[cfg(feature = "ceiling-no-scroll")]
        return self.view.rot();
        #[allow(unreachable_code)]
        let last = self.rows.index() - 1;
        // **The bottom of the *region*, not of the screen.** A pager sets
        // `DECSTBM` and then relies on `LF` at the region's last row scrolling
        // only inside it; testing the screen's last row instead lets a status
        // line scroll away. A full-screen region falls through to the rotation
        // below, which is where the 45% this repo measured lives.
        if self.cy == self.cold.bot && self.cold.bot.index() != last {
            self.row_changed();
            self.scroll_region_up(Count::ONE);
            return self.view.rot();
        }
        self.row_changed();
        if self.cy.index() < last {
            self.cy = Row::from_index(self.cy.index() + 1);
            return self.view.rot();
        }
        let rot = self.view.advance_rot(1);
        // **Ceiling arm: the blank behind a scroll.** The ring still advances
        // and the row is still counted, so both arms attribute the same cells
        // and only the stores differ -- which is what the previous core's
        // blanking arm got wrong, reporting a full row on one side and the
        // used extent on the other.
        #[cfg(not(feature = "ceiling-no-blank"))]
        let n = self
            .view
            .fill_row(Row::from_index(last), PackedCell::empty_with(self.style));
        #[cfg(feature = "ceiling-no-blank")]
        let n = self.view.extent(Row::from_index(last));
        self.cells = self.cells.plus(n.index());
        rot
    }
}
