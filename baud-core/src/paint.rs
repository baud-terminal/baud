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
pub(crate) struct Paint<'a, 'b> {
    pub(crate) arena: &'a [u8],
    /// The three-byte cell table; empty when the core is computing instead.
    ///
    /// **Compiling it out when the feature is off is worth nothing**, which is
    /// the boundary of the placement rule this file records five wins from. A
    /// field nothing *reads* costs nothing -- LLVM has already removed it. What
    /// costs is a field the hot path *writes* (`last_cp` at 16%, `style_raw` at
    /// 3.5%) or a large struct *copied* through the loop (`Cold` was 120 bytes and
    /// 4% when measured; it is 192 now and a reference, see below). Two dead words are free: btop -1%, less-pager -2%, the rest flat.
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
    pub(crate) cluster: ClusterBreak,
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
    /// **A reference, and it used to be by value.** `Cold` is **192 bytes** (120 when
    /// this was measured, grown since via charset slots and input modes) --
    /// half of it two `Option<Saved>` at 48 each, for `DECSC` and the alternate screen's
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
    /// How much of `title` and `icon` is live. Lengths rather than a
    /// terminator, because a title may legitimately contain any byte.
    pub(crate) title_len: ByteLen,
    pub(crate) icon_len: ByteLen,
    /// The link every cell written from here carries. `LinkId::NONE` between
    /// `OSC 8` runs, which is what an empty URI restores.
    pub(crate) link: LinkId,
    /// Bodies clipped at `OSC_MAX`, strings recognised and not acted on, and
    /// the two halves of `OSC 52`. **Refused is not the same as unnoticed**:
    /// silence is the right answer to a clipboard read, and being unable to
    /// tell that one arrived is a different thing.
    /// VT52 commands this core does not act on: the six print functions
    /// (`ESC ^ _ W X ] V`) and any byte that is not a VT52 command at all.
    /// **Counted rather than silent**, because `unsupported` is indexed by
    /// `FinalByte::slot()` and a VT52 command byte is not a `FinalByte`.
    pub(crate) vt52_unsupported: usize,
    pub(crate) bells: usize,
    /// Character-set designations this core does not name.
    ///
    /// **Counted rather than recorded**, which is the distinction the eleven
    /// named sets exist to make: a name means the designation was understood,
    /// so recording an unlisted final as a set would make it look designated.
    /// `Charset::Other` is what the slot holds, and this is how often.
    pub(crate) charset_unsupported: usize,
    /// Control sequences recognised by the grammar and not acted on.
    ///
    /// **Always on, where the per-final histogram is behind `diag`.** The
    /// conformance suites ask one question per terminfo capability -- "was
    /// anything left unsupported after this?" -- and a counter that only
    /// exists in a diagnostic build cannot answer it in a normal test run.
    /// Incremented in an arm that is already `#[cold]`, so the ingest path
    /// pays nothing.
    pub(crate) unsupported_total: usize,
    /// Mode parameters this core does not recognise, and `U+FFFD`s written for
    /// bytes that could not be decoded. **Counted rather than silent** for the
    /// reason every counter here is: an embedder cannot tell a mode that did
    /// nothing from one that was never sent, and a replacement is the parser
    /// saying it lost data.
    pub(crate) unsupported_param: usize,
    pub(crate) replacements: usize,
    pub(crate) string_truncations: usize,
    pub(crate) strings_ignored: usize,
    pub(crate) clipboard_reads_refused: usize,
    pub(crate) semantic_marks: usize,
    pub(crate) clipboard_writes_dropped: usize,
    /// The screen that is not live: the other surface's cells, marks, ring
    /// and rotation, reached **once per alt-screen transition** and never by
    /// the per-character loop, when `?1049` swaps it with the view's.
    ///
    /// Four words, carried by value, and measured rather than assumed: the
    /// span attempt and the row map each cost 5-10% for two more words
    /// through this struct, which is inlined into the per-byte loop. The
    /// swap is what buys it back -- two live surfaces mean no park and no
    /// restore, so entering the alternate screen copies nothing.
    pub(crate) other: Surface<'a>,
    /// The primary ring's bookkeeping: how many rows behind the window are
    /// lines, how many after it were cut by a shorter window. One word.
    pub(crate) book: &'a mut crate::storage::Book,
    /// Where a cluster's continuations go. Reached once per zero-width
    /// codepoint, which ASCII never is.
    ///
    /// **One word, behind a pointer, and that is measured rather than tidy.**
    /// Carried as a slice plus a counter -- three words -- it cost **4.2% on
    /// asciiquarium and 2.9% on cava**, neither of which contains a single
    /// zero-width codepoint, so neither can reach the code at all. `Paint` is
    /// inlined into the per-byte loop and this file records that class twice
    /// before: the span attempt and the row map, both 5-10% for two more words,
    /// and both recovered by a thin reference. `AltBuf` was grouped for the
    /// same reason one change earlier.
    pub(crate) clusters: &'a mut ClusterStore<'b>,
    /// Everything a control string writes. **One word, not six slices.**
    ///
    /// `Paint` is inlined into the per-byte loop, and this file records the
    /// class twice at 3.5-10%: carrying `Cold` by value cost 4% of less-pager
    /// because 96 of its then-120 bytes were two `Option<Saved>` no payload
    /// touches. Six slices is twelve words of exactly that. A reference is one
    /// word and costs one load on a path taken once per control string.
    pub(crate) strs: &'a mut crate::storage::StrBuf<'b>,
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
    /// **How often each stride's *vector* scan is entered**, indexed by encoded
    /// length.
    ///
    /// Separate from `run_len`, which counts runs the scan *produced*. This
    /// counts entries into the bulk path, because those are different numbers
    /// and the gap between them is where a missing bulk path hides: a run of
    /// four-byte characters is found either way, by the vector scan or by the
    /// scalar loop under it, and only an entry counter can tell which.
    ///
    /// This repo already states the rule -- "do not read a bulk path as
    /// covering a character class, count how often it is entered" -- and then
    /// had no counter for this one.
    #[cfg(feature = "diag")]
    pub(crate) stride_scan: [core::cell::Cell<usize>; 5],
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
    pub(crate) unsupported_by_final: [usize; FinalByte::COUNT],
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

impl<'a, 'b> Paint<'a, 'b> {
    /// **One destructure, one view.** `arena` and `vp` are different fields of
    /// the same `Storage`, so splitting the single `&mut` is what lets a run be
    /// blitted straight out of the arena with no copy in between.
    ///
    /// One borrow per field it needs, and a config struct would defeat the
    /// purpose: this file measures `Cold` carried by value against `Cold`
    /// carried by reference at 1.5-4%, because `Paint` is inlined into the
    /// per-character loop and what that loop carries is the cost. A grouping
    /// type would put a pointer where a borrow is. The audit gates the *types*
    /// of these, which is the check with teeth.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        store: &'a mut Storage<'b>,
        cy: Row,
        cx: Col,
        rows: Rows,
        cols: Cols,
        rot: RowRot,
        style: StyleId,
        style_raw: StyleWord,
        flags: CellFlags,
        charset: Charset,
        cluster: ClusterBreak,
        last_cp: Option<(Codepoint, Width)>,
        cold: &'a mut crate::term::Cold,
        reply_len: ByteLen,
        title_len: ByteLen,
        icon_len: ByteLen,
        link: LinkId,
    ) -> Paint<'a, 'b> {
        let Storage {
            arena,
            stride,
            depth,
            vp,
            used,
            cells3,
            tmpl,
            tabs,
            reply,
            alt,
            book,
            clusters,
            strs,
            ..
        } = store;
        // **The live surface by which screen is up.** The primary is the
        // ring -- the screen's capacity plus the scrollback -- and the
        // alternate is a ring of the capacity alone, so a scroll there
        // rotates blanks into blanks as it always did. Whichever is not live
        // rides as `other`, ready to be swapped in.
        let ring = Rows::of_index(used.len());
        let cap = Rows::of_index(used.len() - depth.lines());
        let (vp, used): (&'a mut [PackedCell], &'a mut [RowExtent]) = (vp, used);
        let (acells, aused, arot): (&'a mut [PackedCell], &'a mut [RowExtent], RowRot) =
            (alt.cells, alt.used, alt.rot);
        let (view, other) = match cold.screen {
            crate::term::Screen::Normal => (
                View::new(vp, used, *stride, cols, rows, ring, rot),
                Surface {
                    cells: acells,
                    used: aused,
                    ring: cap,
                    rot: arot,
                },
            ),
            crate::term::Screen::Alternate => (
                View::new(acells, aused, *stride, cols, rows, cap, arot),
                Surface {
                    cells: vp,
                    used,
                    ring,
                    rot,
                },
            ),
        };
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
            title_len,
            icon_len,
            link,
            vt52_unsupported: 0,
            bells: 0,
            charset_unsupported: 0,
            unsupported_total: 0,
            unsupported_param: 0,
            replacements: 0,
            string_truncations: 0,
            strings_ignored: 0,
            clipboard_reads_refused: 0,
            semantic_marks: 0,
            clipboard_writes_dropped: 0,
            other,
            book,
            clusters,
            strs,
            cold,
            view,
            cy,
            cx,
            rows,
            cols,
            style,
            flags,
            charset,
            cluster,
            style_raw,
            last_cp,
            cells: Cells::ZERO,
            row_written: Col::ZERO,
            #[cfg(feature = "diag")]
            runs: [0; 8],
            #[cfg(feature = "diag")]
            #[cfg(feature = "diag")]
            stride_scan: [const { core::cell::Cell::new(0) }; 5],
            #[cfg(feature = "diag")]
            run_len: [0; 5],
            #[cfg(feature = "diag")]
            table_hits: 0,
            #[cfg(feature = "diag")]
            hist: crate::graph::Hist::ZERO,
            #[cfg(feature = "diag")]
            unsupported_by_final: [0; FinalByte::COUNT],
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
        match n.index() <= self.room().index() {
            true => self.piece(from, n),
            false => self.wrapped(from, n),
        }
    }

    /// IRM: open a gap of `n` cells at the cursor before writing into it.
    ///
    /// **`CSI 4 h` was recorded and never read.** `apply_ansi_mode` stored
    /// `ModeBit::InsertReplace` and nothing in the write path consulted it, so
    /// insert mode overwrote the tail of the line exactly as replace mode
    /// does -- the "a bit that is set and never read is the same failure as a
    /// sequence parsed and dropped" rule, and five ported tests found it.
    ///
    /// The shift is `insert_cells`, which `ICH` already uses, so there is one
    /// spelling of "move the tail right and blank what it left". Called **once
    /// per run** rather than once per cell, which is where the mode read
    /// belongs: this file prices a runtime flag inside a per-cell loop at 5%,
    /// and a run-level branch at nothing.
    #[cold]
    #[inline(never)]
    fn open_gap(&mut self, n: Count) {
        let (r, at, b) = (self.cy, self.cx, self.blank());
        let to = Col::at_index(self.cols.index());
        self.view.insert_cells(r, at, n, to, b);
        self.cells = self.cells.plus(self.cols.index() - at.index());
        self.row_changed_to(r);
    }

    /// Whether `CSI 4 h` is in force.
    #[inline(always)]
    fn inserting(&self) -> bool {
        self.cold.modes.is_set(ModeBit::InsertReplace)
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
        // **Printable ASCII is blitted and never reaches the cluster machine,
        // so it has to be told.** Every ASCII character is class `Other` with
        // no lookback, so a run of them ends any cluster in progress -- and a
        // machine left mid-cluster would have the *next* non-ASCII character
        // wrongly continue a base that text has already scrolled past.
        //
        // One store per **run**, not per byte, so it is off the per-byte path
        // this file is careful about. `new` rather than `after_ascii_run`: the
        // truthful state is `prev = Some(Other)` and the two are
        // observationally equivalent here, which is the argument
        // `ClusterBreak::after_ascii_run` exists to carry.
        self.cluster = ClusterBreak::new();
        let k = n.index();
        // **No gap here.** `piece` is a router: it hands the run to `glyph`
        // per character or to `blit` whole, and both open their own. Opening
        // one here as well shifted the tail twice -- `abcd` + insert `XY` gave
        // `XY..abcd` instead of `XYabcd`, a gap of four for a write of two,
        // which is what the doubled shift looks like from the screen.
        #[cfg(feature = "ceiling-no-text")]
        {
            let _ = from;
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
        // **What `REP` repeats, recorded once a run rather than once a byte.**
        // `blit` writes eight cells to a store and never touched `last_cp`, so
        // `hello` then `CSI 3 b` repeated **nothing** -- the preceding graphic
        // character existed on the screen and the terminal could not name it.
        // Only `glyph` set it, which is the short-run path above, so whether
        // `REP` worked depended on how long the run before it was.
        //
        // One store per run is the same placement the `ClusterBreak` reset
        // above already has, and for the same reason: this is a router, and
        // per-byte is what this file is careful about.
        let last = StreamByte::from_stream(self.arena[from.index() + n.index() - 1]);
        self.last_cp = Some((PackedCell::translate(last, self.charset), Width::NARROW));
        self.blit(from, n);
    }

    /// A run that runs off the row: row-piece by row-piece, wrapping between.
    #[inline(never)]
    fn wrapped(&mut self, from: BytePos, n: ByteLen) {
        let (mut at, mut left) = (from.index(), n.index());
        while left > 0 {
            if self.cx.index() > self.cold.right.index() {
                self.wrap();
            }
            let k = left.min(self.room().index());
            self.piece(BytePos::from_index(at), ByteLen::from_index(k));
            at += k;
            left -= k;
        }
    }

    /// The vectored form, for a run long enough to fill a store.
    #[inline(always)]
    fn blit(&mut self, from: BytePos, n: ByteLen) {
        // Told for the same reason `piece` is, and once per run.
        self.cluster = ClusterBreak::new();
        // No clip: `text` sized the piece to the row before calling. Clipping
        // here is what silently dropped the tail of a long line.
        let k = n.index();
        if self.inserting() {
            self.open_gap(Count::of(k));
        }
        let src = &self.arena[from.index()..from.index() + k];
        self.view
            .put_ascii_run(self.cy, self.cx, src, self.style, self.flags, self.charset);
        self.cells = self.cells.plus(k);
        self.cx = Col::at_index(self.cx.index() + k);
        self.row_written = self.row_written.max(self.cx);
    }

    /// Advance past a character just written, leaving the pending-wrap
    /// sentinel only where `DECAWM` can consume it.
    ///
    /// **With `DECAWM` off there is no sentinel**, because nothing will ever
    /// wrap: the cursor stays *on* the last writable column and further
    /// characters overwrite it. Advancing to `right + 1` there left every
    /// later operation addressing off the row -- `ICH` inserted its blank
    /// outside the margin, so the test that found this asserts on `ICH` rather
    /// than on the cursor: an operation is the only thing that can tell where
    /// the cursor *really* is.
    ///
    /// `Term::cursor` clamps the same value for the same reason, and `CPR` now
    /// does too. Three places, one sentinel, and this is the one that stops it
    /// being created.
    #[inline(always)]
    fn advance(&mut self, need: Count) {
        let to = self.cx.index() + need.index();
        self.cx = match self.cold.modes.is_set(ModeBit::AutoWrap) {
            true => Col::at_index(to),
            false => Col::at_index(to.min(self.cold.right.index())),
        };
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
        if self.cx.index() + need > self.cold.right.index() + 1 {
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
        self.advance(Count::of(need));
        self.row_written = self.row_written.max(self.cx);
    }

    /// One glyph under the cursor, wrapping first if it will not fit.
    #[inline(always)]
    pub(crate) fn glyph(&mut self, cp: Codepoint, w: Width) {
        if cp == Codepoint::REPLACEMENT {
            self.replacements += 1;
        }
        // **A zero-width codepoint gets no cell, and that needs no cluster
        // machine.** A combining mark, a ZWJ or a variation selector took a
        // full cell here and advanced the cursor a column, so everything after
        // it on the row sat one place right of where `baud-core2` puts it --
        // which is what 84% of the pinned glyph divergence is.
        //
        // `baud-core2` reaches the same answer through a *second* arm that
        // fires whatever the break decision was (`put_cp_classified`, and
        // `ClusterBreak::after_ascii_run`'s doc says so in as many words), so
        // this reproduces its cell output exactly rather than approximately,
        // for every input, with no state and no 24 KB transition table.
        //
        // What it does not reproduce is *storing* the codepoint. core2
        // attaches it to the base cell; this drops it, and says so.
        if w == Width::ZERO {
            self.attach(cp);
            return;
        }
        let need = 1 + usize::from(w == Width::WIDE);
        if self.cx.index() + need > self.cold.right.index() + 1 {
            self.wrap();
        }
        // After the wrap, because the gap belongs in the row the character
        // lands in rather than the one it was pushed off.
        if self.inserting() {
            self.open_gap(Count::of(need));
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
        self.advance(Count::of(need));
        self.row_written = self.row_written.max(self.cx);
    }

    /// Attach a zero-width codepoint to the cell that owns the column before
    /// the cursor.
    ///
    /// **Ask the grid which cell that is; do not compute it.** The cursor sits
    /// after the character it just wrote, so one column back is that character
    /// -- *unless it was double-width*, where one column back is its
    /// `WIDE_TAIL`. `ViewRef::lead` answers with a [`LeadCol`], a type only it
    /// can build, so "one column back" and "the cell that owns this position"
    /// cannot be confused at a call site. `baud-core2` attached every ZWJ
    /// sequence to the tail before that type existed, and no counter
    /// disagreed: the marks were all stored, on the wrong cell.
    ///
    /// The cluster is **re-interned on every continuation**, because a cluster
    /// is not known until the codepoint after it arrives. Six continuations of
    /// a family cost six interns of a growing list, against the six hashes and
    /// six probes a position key costs for the same family -- so this is not
    /// more hashing than the design it replaces.
    /// **`#[cold]` and out of line, for the reason `want_reply` is.** `glyph`
    /// is inlined into the per-byte loop, so a body added here is paid by
    /// every payload whether or not it runs -- this file records
    /// `for w in [WIDE, NARROW]` costing 3.5% on pure ASCII that could not
    /// execute it. Inlined, this cost **3.1% on asciiquarium and 4.1% on
    /// cava**, neither of which contains a zero-width codepoint, and 15% on
    /// the two CJK recordings. The gate stays in `glyph`; the work does not.
    #[cold]
    #[inline(never)]
    pub(crate) fn attach(&mut self, cp: Codepoint) {
        // A continuation with nothing before it on the line has no base.
        // Dropping it is correct: `[[source: uax29]]` GB1 puts a boundary at
        // start of text, so it is its own cluster and there is no cell for it.
        if self.cx == Col::ZERO {
            self.clusters.refused += 1;
            return;
        }
        // **Priced by doing it twice, discarding the first.** The lead
        // resolution is a grid read and `intern` is idempotent on the same
        // cluster -- it finds the slot it just filled and returns the same id
        // -- so the state and every counter come out unchanged and the delta
        // is the work rather than its consequences.
        #[cfg(feature = "probe-double-attach")]
        {
            let o = self.view.lead(self.cy, self.cx.before());
            let c = self.view.cell_at(self.cy, o);
            core::hint::black_box(self.clusters.get(c.flags.cluster(), c.content).is_some());
        }
        let (r, owner) = (self.cy, self.view.lead(self.cy, self.cx.before()));
        let cell = self.view.cell_at(r, owner);
        if cell.content.is_empty() {
            self.clusters.refused += 1;
            return;
        }
        let mut cps = [0u32; ClusterSlot::CAPACITY];
        let mut n = 0usize;
        if let Some(s) = self.clusters.get(cell.flags.cluster(), cell.content) {
            for c in s.marks() {
                if n < cps.len() {
                    cps[n] = c;
                    n += 1;
                }
            }
        }
        if n == cps.len() {
            // A cluster past the table's capacity keeps what it has rather
            // than losing its head, and says one was lost.
            self.clusters.refused += 1;
            return;
        }
        cps[n] = cp.value();
        n += 1;
        let id = self.clusters.intern(cell.content, &cps[..n]);
        match id.is_none() {
            true => {}
            false => self.view.set_cluster(r, owner, id),
        }
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
        n.index() * (1 + usize::from(w == Width::WIDE)) <= self.room().index()
    }

    /// Columns left before the **right margin**, which is the screen edge
    /// until `DECSLRM` says otherwise.
    ///
    /// One field read on a path taken once a *run*, not once a character --
    /// the same placement `charset` has, and the reason `Cold` holds the
    /// margins rather than `Paint` resolving them per cell.
    #[inline(always)]
    pub(crate) fn room(&self) -> Count {
        Count::of(self.cold.right.index() + 1 - self.cx.index())
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
        let bytes = r.encode(self.cold.c1);
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

    /// Move to the start of the next line, scrolling if there is no next line.
    ///
    /// **DECAWM off means the cursor stays put and the character overwrites**,
    /// which is what a status line drawn to the last column depends on. This
    /// comment was stranded on `want_reply` by an insertion -- the fifth time
    /// in this repo that an edit landed between a doc and the function it was
    /// written for, and the first found by reading rather than by a compiler
    /// warning about a duplicated attribute.
    pub(crate) fn wrap(&mut self) {
        // **The margin, not the edge.** With `DECAWM` off the cursor pins to
        // the last *writable* column, and with it on the next row starts at
        // the **left** margin -- `[[source: vt510-decslrm]]` makes the region
        // a rectangle, so text confined to it stays confined across the wrap.
        if !self.cold.modes.is_set(ModeBit::AutoWrap) {
            self.cx = self.cold.right;
            return;
        }
        self.cx = self.cold.left;
        // The row being left continues onto the next: the fact a reflow
        // needs, kept in the mark the write already touched.
        self.view.set_wrapped(self.cy);
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
    /// `LF`, `VT` and `FF`: down one line, and a carriage return **only**
    /// when LNM is set.
    ///
    /// **The mode was recorded and never read**, which is the failure
    /// `docs/vt-coverage.md` rates worse than not implementing it: `CSI 20 h`
    /// set a bit `apply_ansi_mode` stored and nothing consulted, so `LF` moved
    /// down and left the column alone whatever the application asked for.
    /// `baud-core2` has always read it here, and the ported
    /// `lf_returns_to_column_one_when_lnm_is_set` is what found the gap.
    #[inline(always)]
    pub(crate) fn line_feed(&mut self) {
        // A newline ends the row it leaves, however it was written before.
        self.view.set_ended(self.cy);
        self.index();
        if self.cold.modes.is_set(ModeBit::LineFeedNewLine) {
            self.cx = Col::ZERO;
        }
    }

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
        self.retire();
        let rot = self.view.advance_rot(1);
        #[cfg(feature = "probe-warm-scroll")]
        self.warm_incoming();
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
