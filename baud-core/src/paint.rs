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

/// Which pen a glyph is written with: [`Paint::flags`] for a character
/// received as a GL code, [`Paint::decoded_pen`] for one that arrived
/// decoded. A constant at every call but REP's, so the match on it folds
/// away where text is written.
#[derive(Clone, Copy)]
pub(crate) enum Pen {
    Gl,
    Decoded,
    /// REP's copy of a GL character written before GL's set changed: the
    /// decoded pen with the set it was received under ([`Paint::invoke`]).
    /// One byte with the other two, in the niche the set leaves, so
    /// `last_cp` keeps its size.
    Kept(Option<Received>),
}

/// **What a character's write does before its cells**, after the wrap:
/// the text path's [`Plain`], or SAPV's [`Formed`]. A type and not a flag,
/// so the text path is compiled with no test of SAPV's state at all; the
/// graph sends text to SAPV's writer while a form waits or was just taken
/// ([`crate::graph::FormRead`]).
pub(crate) trait Writer {
    /// Whether this writer reads DECRLM ([`ModeBit::CursorRightToLeft`]):
    /// SAPV's, which the one-by-one graph sends every character to while
    /// it is set; never the text path's, which then never tests it.
    const LEFTWARD: bool = false;
    fn before(p: &mut Paint<'_, '_>, cp: Codepoint, need: Count);
}

/// What [`Paint::overflow`] leaves room for: the whole character, or a
/// wide character's lead alone on the line's last column.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Fit {
    Whole,
    LeadAlone,
}

/// The text path's writer: under IRM the gap opens.
pub(crate) struct Plain;

impl Writer for Plain {
    #[inline(always)]
    fn before(p: &mut Paint<'_, '_>, _: Codepoint, need: Count) {
        if p.inserting() {
            p.open_gap(need);
        }
    }
}

/// SAPV's writer, out of line: the form leaves, is taken or is spent, and
/// then the gap opens under IRM (`Paint::write_slowly`).
pub(crate) struct Formed;

impl Writer for Formed {
    const LEFTWARD: bool = true;
    #[inline(always)]
    fn before(p: &mut Paint<'_, '_>, cp: Codepoint, need: Count) {
        p.write_slowly(cp, need);
    }
}

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
    /// The cursor's column: the pending-wrap sentinel, and a move's tag
    /// one past the right margin ([`CursorCol`]).
    pub(crate) cx: CursorCol,
    pub(crate) rows: Rows,
    pub(crate) cols: Cols,
    /// The cell height in device pixels, or [`ImagePx::ZERO`] when unknown --
    /// what [`Paint::sixel_end`] divides an image's pixel height by to advance
    /// the cursor. Read-only during a feed, so it is passed by value and never
    /// written back.
    pub(crate) cell_height: ImagePx,
    /// The cell width in device pixels, paired with `cell_height` for freeing a
    /// Sixel a write overwrites. Zero (a headless caller) frees nothing.
    pub(crate) cell_width: ImagePx,
    pub(crate) style: StyleId,
    /// The pen for a character received as a GL code: the set GL holds is
    /// in it, as [`CellFlags::GRAPHICS`] or a national tag.
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
    /// What `REP` repeats, and which pen wrote it. **A flat field and not part of `cold`**: it is
    /// written once per character, and a per-character store into the cold
    /// struct is what keeps that struct out of registers for the whole loop.
    pub(crate) last_cp: Option<(Codepoint, Width, Pen)>,
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
    /// VT52 commands this core does not act on: the six print functions
    /// (`ESC ^ _ W X ] V`) and any byte that is not a VT52 command at all.
    /// **Counted rather than silent**, because `unsupported` is indexed by
    /// `FinalByte::slot()` and a VT52 command byte is not a `FinalByte`.
    pub(crate) vt52_unsupported: usize,
    pub(crate) bells: usize,
    /// The margin bell's rings this read (`margin.rs`).
    pub(crate) margin_bells: usize,
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
    /// Mode parameters this core does not recognise. **Counted rather than
    /// silent** for the reason every counter here is: an embedder cannot tell
    /// a mode that did nothing from one that was never sent.
    pub(crate) unsupported_param: usize,
    /// `U+FFFD`s written for bytes that could not be decoded, the parser
    /// saying it lost data: one a maximal subpart, whether the U+FFFD takes a
    /// cell or joins the cluster before it, and none for a literal U+FFFD,
    /// which was decoded. Counted in `write::replace`, before the cluster
    /// machine decides; it was counted here in `glyph`, by value, so a
    /// substitute that attached went uncounted and a literal that took a cell
    /// was counted (`a_replacement_that_attaches_is_counted`).
    pub(crate) replacements: usize,
    /// Bodies clipped at `OSC_MAX` -- this -- and, below, strings recognised
    /// and not acted on (`strings_ignored`) and the `OSC 52` reads and the
    /// writes refused (`clipboard_reads_refused`, `clipboard_writes_refused`).
    /// **Refused is not the same as unnoticed**: silence is the right answer
    /// to a clipboard read, and being unable to tell that one arrived is a
    /// different thing.
    pub(crate) string_truncations: usize,
    pub(crate) links_refused: usize,
    pub(crate) links_clipped: usize,
    pub(crate) strings_ignored: usize,
    pub(crate) clipboard_reads_refused: usize,
    pub(crate) semantic_marks: usize,
    /// Redraw marks that erased the block they replace; see `osc.rs`.
    pub(crate) prompt_erases: usize,
    pub(crate) clipboard_writes_refused: usize,
    /// DECRQCRA requests the embedder's setting left unanswered.
    pub(crate) checksums_refused: usize,
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
    /// **The surface not live of the main display's and the status line's**:
    /// the line's row while the main display is written, and the main
    /// display's while the line is (`status.rs`, `Paint::swap_display`).
    pub(crate) status: Surface<'a>,
    /// The primary ring's bookkeeping: how many rows behind the window are
    /// lines, how many after it were cut by a shorter window. One word.
    pub(crate) book: &'a mut crate::storage::Book<'b>,
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
    /// The rendition table's write log, reached on a table miss and on a
    /// colour change -- never on the hit an `SGR` usually is. One word, the
    /// `clusters` shape.
    pub(crate) style_log: &'a mut crate::storage::StyleLog<'b>,
    /// Stored images and their placements. A completed Sixel decodes into this
    /// and places at the cursor; see [`ImageStore`].
    pub(crate) images: &'a mut ImageStore<'b>,
    /// Everything a control string writes. **One word, not six slices.**
    ///
    /// `Paint` is inlined into the per-byte loop, and this file records the
    /// class twice at 3.5-10%: carrying `Cold` by value cost 4% of less-pager
    /// because 96 of its then-120 bytes were two `Option<Saved>` no payload
    /// touches. Six slices is twelve words of exactly that. A reference is one
    /// word and costs one load on a path taken once per control string.
    pub(crate) strs: &'a mut crate::storage::StrBuf<'b>,
    /// The state a control sequence changes and the write loop never reads.
    ///
    /// **A reference, and it used to be by value.** `Cold` is **192 bytes** (120 when
    /// this was measured, grown since via charset slots and input modes) --
    /// half of it two `Option<Saved>` at 48 each, for `DECSC` and the alternate screen's
    /// save, which no payload touches more than a handful of times -- and
    /// `Paint` is inlined into the per-character loop. Carrying it meant
    /// carrying all of that; an indirection costs one load on a path taken
    /// once a control sequence.
    pub(crate) cold: &'a mut crate::term::Cold,
    /// **The parse graph a step reads**, [`crate::graph::graph_of`] the
    /// encoding, the Unicode version and whether a SAPV form waits: chosen
    /// here and by [`Paint::reselect`] when any changes, so no step asks
    /// them.
    pub(crate) graph: &'static crate::graph::Graph,
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
    /// **It must never be below the row's real extent**, because a zero makes
    /// `cx >= row_written` true, which is the branch that skips the repair.
    /// Above it is only slower: the probing path on cells that are blank.
    ///
    /// So it is the cursor row's `View::extent`, read wherever the cursor
    /// lands on a row or a row moves under it: on every move to another row
    /// ([`Paint::set_row`]), after a scroll, a screen swap or a `DECIC`. It is
    /// zero only where the row has just been blanked -- the rotation at the
    /// bottom and a region scrolled up under its last row. It used to be zero
    /// at every read and every row change, on the argument that the cursor
    /// reached a row only by `LF` or a wrap onto one nothing had written,
    /// which stopped being true with the first `CUP` and was never true of a
    /// read starting where the last one stopped: a letter after `U+1100 CR`
    /// in a read of its own kept the tail (`tests/wide_halves.rs`).
    ///
    /// **At the start of a read it is past every column, `MAX_COLS`, not the
    /// row's extent**: too high is only slower, so the writes on the read's
    /// first row take the repairing path until the cursor changes rows. The
    /// extent, read in `Term::feed` and passed in, was the same screen and
    /// cost `cat` 4.4% at the same instructions retired; a constant, as the
    /// zero was, reads 1-2% below the parent there with the ranges
    /// overlapping.
    pub(crate) row_written: Col,
    /// Whether a write of text has more to do than its cells
    /// ([`Paint::overwrote`]): set while an image is placed, which the
    /// write may free, or a watch on a selection's rows is armed, which it
    /// may reach. **One byte in place of the gate the image check had**, so
    /// a stream with neither pays the one compare a character it paid
    /// before the watch: the watch's own compare a character in `glyph` was
    /// about 1.0% of `cat-cjk`'s instructions and 0.9% of `vim`'s. Read
    /// from both in [`Paint::new`]; set again after each place a read can
    /// place an image, a Sixel's end and each kitty call; and read again by
    /// the hooked path, so it clears once the image goes or the watch is
    /// reached. Too high is only slower; too low loses an overwrite or a
    /// dropped selection, which a debug build asserts.
    hooks: WriteHooks,
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
    /// Scrolls that rotated an index, and scrolls that copied the rows.
    /// **The difference between them is 45%** by this repo's own measurement,
    /// and nothing counted which one was happening.
    #[cfg(feature = "diag")]
    pub(crate) scroll_rot: usize,
    #[cfg(feature = "diag")]
    pub(crate) scroll_copy: usize,
    /// Accelerated `CSI` attempts, and the ones that committed. **A bulk path
    /// that declines still pays its scan**, which is a rule this repo already
    /// records; the only way to know which is happening is to count entries.
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
        cx: CursorCol,
        rows: Rows,
        cols: Cols,
        cell_height: ImagePx,
        cell_width: ImagePx,
        rot: RowRot,
        style: StyleId,
        style_raw: StyleWord,
        flags: CellFlags,
        charset: Charset,
        cluster: ClusterBreak,
        last_cp: Option<(Codepoint, Width, Pen)>,
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
            style_log,
            tabs,
            reply,
            alt,
            status,
            book,
            clusters,
            strs,
            images,
            ..
        } = store;
        let hooks = WriteHooks::of(images, cold.watched, cold.bell.armed);
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
        let status = Surface {
            cells: status.cells,
            used: status.used,
            ring: Rows::of_count(1),
            rot: RowRot::NONE,
        };
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
        // The three-byte cell table is read only by `put_one` under
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
            margin_bells: 0,
            charset_unsupported: 0,
            unsupported_total: 0,
            unsupported_param: 0,
            replacements: 0,
            string_truncations: 0,
            links_refused: 0,
            links_clipped: 0,
            strings_ignored: 0,
            clipboard_reads_refused: 0,
            semantic_marks: 0,
            prompt_erases: 0,
            clipboard_writes_refused: 0,
            checksums_refused: 0,
            other,
            status,
            book,
            clusters,
            style_log,
            strs,
            images,
            graph: crate::graph::graph_of(
                cold.encoding,
                cold.widths.widths(),
                crate::variants::form_read(cold.modes),
            ),
            cold,
            view,
            cy,
            cx,
            rows,
            cols,
            cell_height,
            cell_width,
            style,
            flags,
            charset,
            cluster,
            style_raw,
            last_cp,
            cells: Cells::ZERO,
            // Past every column, so the read's first row repairs until the
            // cursor leaves it: see the field.
            row_written: Col::at_index(crate::storage::MAX_COLS),
            hooks,
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
    pub(crate) fn open_gap(&mut self, n: Count) {
        // After the fit test, so never a tagged value.
        let (r, at, b) = (self.cy, self.cx.raw(), self.blank());
        // The line's end: the gap pushes off a double line's last column,
        // not into the half past it (`[[source: dec-std-070]]` p. 5-140,
        // `margin = end_of_line(line)`).
        let to = Col::at_index(self.line_cols().index());
        self.view.insert_cells(r, at, n, to, b);
        self.erase_images(r, at, to);
        self.cells = self.cells.plus(to.index() - at.index());
        self.row_changed_to(r);
    }

    /// Whether `CSI 4 h` is in force.
    #[inline(always)]
    pub(crate) fn inserting(&self) -> bool {
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
        // this file is careful about. **The state the run leaves, not
        // `ClusterBreak::new`**, which says nothing came before: GB9 and GB9a
        // join a continuation to the letter only if the machine knows a
        // letter is there, and under `new` GB1 broke first, so `a U+0903` put
        // the visarga in a cell of its own. `ClusterBreak::after_ascii_run`'s
        // doc has the cases.
        self.cluster = ClusterBreak::after_ascii_run();
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
            self.cx = CursorCol::written(Col::at_index(self.cx.raw().index() + k));
            self.row_written = self.row_written.max(self.cx.raw());
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
        // One store per run is the same placement the `ClusterBreak` store
        // above already has, and for the same reason: this is a router, and
        // per-byte is what this file is careful about.
        let last = StreamByte::from_stream(self.arena[from.index() + n.index() - 1]);
        self.last_cp = Some((
            PackedCell::translate(last, self.charset),
            Width::NARROW,
            Pen::Gl,
        ));
        self.blit(from, n);
    }

    /// A run that runs off the row: row-piece by row-piece, wrapping between.
    ///
    /// **Right of the fence a piece runs to the screen's edge**, as
    /// [`Paint::past_margin`] has one character do, and is sized to it so
    /// the blitter never clips: a cursor past the margin by a move, or
    /// beyond it after a piece, gets a piece of at least one.
    #[inline(never)]
    fn wrapped(&mut self, from: BytePos, n: ByteLen) {
        let (mut at, mut left) = (from.index(), n.index());
        while left > 0 {
            // The fence as the cursor's line has it, which the fit test that
            // sent the run here may have read low, and which a wrap onto
            // another line leaves as the last line's (`Paint::refresh_fence`).
            self.refresh_fence();
            let k = match self.cx.raw().index() > self.cold.fence.index() + 1 {
                true => self.past_margin_room().index(),
                false => {
                    if self.cx.raw() > self.cold.fence {
                        self.wrap();
                    }
                    self.room().index()
                }
            };
            let k = left.min(k);
            self.piece(BytePos::from_index(at), ByteLen::from_index(k));
            at += k;
            left -= k;
        }
    }

    /// The room right of the fence for a piece of a run: to the screen's
    /// edge, after the sentinel there wraps, or with `DECAWM` reset goes to
    /// the last column.
    ///
    /// **One character from a placed cursor**, which `glyph` writes where
    /// the tag says: untagged here, one past the margin would read as the
    /// sentinel when `glyph` tested it, and wrap.
    #[cold]
    #[inline(never)]
    fn past_margin_room(&mut self) -> Count {
        if self.cx.is_placed() {
            return Count::ONE;
        }
        self.past_margin(Count::ONE);
        match self.cx.raw().index() > self.cold.fence.index() {
            true => Count::of(self.cols.index() - self.cx.raw().index()),
            false => self.room(),
        }
    }

    /// **Before character `cp` is written by SAPV's writer, `need` cells
    /// of it**, out of line: a form the last character took leaves the
    /// rendition; a form waiting is this character's, kept in the
    /// rendition for its write if it is a letter that takes a form and
    /// spent on it unstored if it is not ([`baud_domain::takes_form`]); and
    /// under IRM the gap opens.
    #[cold]
    #[inline(never)]
    pub(crate) fn write_slowly(&mut self, cp: Codepoint, need: Count) {
        self.form_step(cp);
        // A wrap waiting at SLL's line limit, for the reports
        // (`crate::bounds`).
        if self.cold.modes.is_set(ModeBit::LineBounded) {
            self.limit_step(need);
        }
        // A string laid out against an aligned stop (`crate::aligned`).
        if self.cold.modes.is_set(ModeBit::Aligning) {
            self.align_step(cp, need);
        }
        if self.inserting() {
            // Under an extent SEE chose other than the line, along it
            // (`crate::extent`).
            match self.extent_chosen() {
                false => self.open_gap(need),
                true => self.open_gap_extent(need),
            }
        }
    }

    /// **The form's part of [`Paint::write_slowly`]**, which the leftward
    /// writer takes too, before it reads the rendition: the gap it opens
    /// itself, toward the left margin (`crate::leftward`).
    #[inline(always)]
    pub(crate) fn form_step(&mut self, cp: Codepoint) {
        self.form_ends();
        if self.cold.modes.is_set(ModeBit::FormPending) {
            match takes_form(cp.value()) {
                true => self.form_taken(),
                false => self.form_written(),
            }
        }
    }

    /// The vectored form, for a run long enough to fill a store.
    #[inline(always)]
    fn blit(&mut self, from: BytePos, n: ByteLen) {
        // Told for the same reason `piece` is, and once per run.
        self.cluster = ClusterBreak::after_ascii_run();
        // No clip: `text` sized the piece to the row before calling. Clipping
        // here is what silently dropped the tail of a long line.
        let k = n.index();
        if self.inserting() {
            self.open_gap(Count::of(k));
        }
        let src = &self.arena[from.index()..from.index() + k];
        self.view.put_ascii_run(
            self.cy,
            self.cx.raw(),
            src,
            self.style,
            self.flags,
            self.charset,
        );
        self.cells = self.cells.plus(k);
        let start = self.cx.raw();
        let end = Col::at_index(start.index() + k);
        self.cx = self.end_at(end);
        self.row_written = self.row_written.max(end);
        self.overwrote(start, end);
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
        let to = Col::at_index(self.cx.raw().index() + need.index());
        self.cx = self.end_at(to);
    }

    /// Where the cursor goes after a write that ended before column `to`:
    /// `to` itself, or with `DECAWM` reset the last column if `to` is past
    /// it, recording which ([`crate::term::Pinned`]).
    ///
    /// **One rule for a character and for a run.** `blit` and `run` set the
    /// cursor to the run's end, so a run ending in the last column put it on
    /// the sentinel this exists to refuse, where the same characters one at a
    /// time left it on the column: `ICH` then inserted nowhere
    /// (`tests/margin_pin.rs`). The mode test is once a write, and the clamp
    /// and the store are out of line, in [`Paint::pin`].
    #[inline(always)]
    fn end_at(&mut self, to: Col) -> CursorCol {
        match self.cold.modes.is_set(ModeBit::AutoWrap) {
            true => CursorCol::written(to),
            false => self.pin(to),
        }
    }

    /// `end_at` with `DECAWM` reset: the last column if `to` is past it, and
    /// which of the two in `Cold::pinned`.
    ///
    /// **Out of line because `blit` and `run` pay for it otherwise.** Inline,
    /// the clamp and the store sat in every run's write on the path `cat`
    /// takes with autowrap set, and `cat` read 11.8% below the previous
    /// commit at 0.7% more instructions, with more mispredicts.
    ///
    /// **Right of the fence the last column is the line's**, for a write
    /// that started there: `[[source: dec-std-070]]` p. 5-139, "the Active
    /// Position will not advance beyond the Right Margin or last column of
    /// the Active Line", the right margin only from at or inside it (p.
    /// 5-140), and on a double line that column is its half's, as xterm's
    /// last column is `LineMaxCol` (`[[source: xterm-reset]]`, charproc.c
    /// 7049-7058; `[[source: xterm-ptyx-h]]` 1701-1704). The screen's
    /// left a write past SLL's limit there right of the half's sentinel,
    /// until a read beginning on the status line pulled it back. `cx` is
    /// still where the write started.
    #[cold]
    #[inline(never)]
    fn pin(&mut self, to: Col) -> CursorCol {
        self.refresh_fence();
        let right = match self.cx.raw() > self.cold.fence {
            true => Col::at_index(self.line_cols().index() - 1),
            false => self.cold.fence,
        };
        self.cold.pinned = match to > right {
            true => crate::term::Pinned::OnLast,
            false => crate::term::Pinned::Before,
        };
        CursorCol::written(to.min(right))
    }

    /// A character of `need` columns that does not fit before the right
    /// margin: one past the margin is the pending wrap and wraps, as does a
    /// wide character on the margin; **right of it is out of line**
    /// ([`Paint::past_margin`]). Only the not-fits branch calls it.
    ///
    /// **It says whether the character now fits**, or only its lead: a wide
    /// character with `DECAWM` reset on the line's last column, or on a
    /// line one position wide. There the lead alone is written, on that
    /// column ([`Paint::lead_alone`]), and no tail past it: past the right
    /// margin, a double line's half or SLL's limit is a position no write
    /// may reach ("will not advance beyond the Right Margin or last column
    /// of the Active Line", `[[source: dec-std-070]]` p. 5-139; § 5.6.1, p.
    /// 5-95), and at the screen's edge the grid had always dropped the
    /// tail. No text gives a character two positions there (ECMA-48 6.1.6's
    /// NOTE leaves the move to the implementation); The tail went one past
    /// each of the first three, and a double line showed it whole once
    /// `DECSWL` made it single.
    ///
    /// **Out of line, the wrap with it.** Inline beside the wrap in every
    /// copy of `glyph`, the test read `cat-cjk` 8.6 M instructions over
    /// 200 reps above this call, which reads level with `glyph` calling
    /// `wrap` itself, on a recording that never wraps.
    #[cold]
    #[inline(never)]
    pub(crate) fn overflow(&mut self, need: Count) -> Fit {
        // A fence read low after the cursor left a sized line: the
        // character fits after all, and goes where the cursor is.
        self.refresh_fence();
        if self.cx.raw().index() + need.index() <= self.cold.fence.index() + 1 {
            return Fit::Whole;
        }
        match self.cx.raw().index() > self.cold.fence.index() + 1 {
            false => self.wrap_bounded(),
            true => self.past_margin(need),
        }
        // A narrow character fits wherever the move left the cursor.
        if need.index() == 1 {
            return Fit::Whole;
        }
        // The last column a write from here reaches: the fence from at or
        // inside it, the line's from right of it.
        let last = match self.cx.raw() > self.cold.fence {
            true => self.line_cols().index() - 1,
            false => self.cold.fence.index(),
        };
        match self.cx.raw().index() + need.index() <= last + 1 {
            true => Fit::Whole,
            false => Fit::LeadAlone,
        }
    }

    /// **A wide character's lead alone, on the line's last column**, where
    /// [`Paint::overflow`] found no column for its tail: one cell, the
    /// cursor staying on it, and `REP` repeating the character as received.
    /// Out of line, a copy of the write in [`Paint::glyph_by`] for one
    /// column: the column count passed into that write read `cat-cjk` and
    /// `vim` 1.4-1.6% more instructions on recordings that never reach here.
    #[cold]
    #[inline(never)]
    fn lead_alone<W: Writer>(&mut self, cp: Codepoint, w: Width, pen: Pen) {
        W::before(self, cp, Count::ONE);
        let style = self.style;
        let flags = match pen {
            Pen::Gl => self.flags,
            Pen::Decoded => self.decoded_pen(),
            Pen::Kept(set) => self.flags.with_received(set),
        };
        let lead = PackedCell::glyph(cp, style, flags);
        self.last_cp = Some((cp, w, pen));
        match self.cx.raw() >= self.row_written {
            true => self
                .view
                .put_glyph_fresh(self.cy, self.cx.raw(), lead, None),
            false => self.view.put_glyph(
                self.cy,
                self.cx.raw(),
                lead,
                None,
                PackedCell::empty_with(style),
            ),
        }
        self.cells = self.cells.plus(1);
        let start = self.cx.raw();
        let end = Col::at_index(start.index() + 1);
        self.overwrote(start, end);
        self.advance(Count::ONE);
        self.row_written = self.row_written.max(end);
    }

    /// **A write from right of the right margin writes on to the screen's
    /// last column.** `[[source: dec-std-070]]` p. 5-140 bounds a write by
    /// the right margin only `if (active_position.column <= right_margin)`,
    /// else by `end_of_line`; xterm's `dotext` takes `last_col` while
    /// `cur_col > rmargin` (`[[source: xterm-reset]]`, charproc.c
    /// 7055-7058), "the right-margin is ineffective until the text (may)
    /// wrap and get within the margins" (6991-6995). Here the cursor is a
    /// move's ([`CursorCol::placed`] one past the margin, or any column
    /// beyond it) or the sentinel past the last column that a write right of
    /// the fence leaves.
    ///
    /// What fits before the screen's edge is written where the cursor is.
    /// **On a double line that edge is its half**, which a cursor reaches
    /// right of the fence only past SLL's line limit, side margins and a
    /// sized line never meeting: it wrote on to the screen's last column.
    /// What does not -- the sentinel, or a wide character on the last
    /// column (charproc.c 7130-7138, `force_wrap`) -- wraps with `DECAWM`
    /// set, to the left margin of the next line as [`Paint::wrap`] goes and
    /// as xterm's `WrapLine` does (6983); with it reset the cursor goes to
    /// the last column, as [`Paint::wrap`] puts it on the right margin at
    /// full width, where the two are one column.
    #[cold]
    #[inline(never)]
    fn past_margin(&mut self, need: Count) {
        let c = self.cx.col();
        let end = self.line_cols().index();
        let fits = c.index() + need.index() <= end;
        self.cx = CursorCol::moved(c);
        if fits {
            return;
        }
        // With `DECAWM` reset the last column, where a wide character that
        // does not fit writes its lead alone ([`Paint::overflow`]).
        match self.cold.modes.is_set(ModeBit::AutoWrap) {
            true => self.wrap_bounded(),
            false => self.cx = CursorCol::written(Col::at_index(end - 1)),
        }
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
        if self.cx.raw().index() + need > self.cold.fence.index() + 1
            && self.overflow(Count::of(need)) == Fit::LeadAlone
        {
            // A lead alone is the text path's, under the decoded pen.
            if let Some(cp) = e.content.as_codepoint() {
                return self.lead_alone::<Plain>(cp, e.flags.width_of(), Pen::Decoded);
            }
        }
        let (style, flags) = (self.style, self.decoded_pen());
        let lead = PackedCell {
            content: e.content,
            style,
            flags,
        };
        let tail = match e.flags.is_wide() {
            true => Some(PackedCell::wide_tail(style, flags)),
            false => None,
        };
        // The field at each use, as in `glyph`.
        match self.cx.raw() >= self.row_written {
            true => self
                .view
                .put_glyph_fresh(self.cy, self.cx.raw(), lead, tail),
            false => self.view.put_glyph(
                self.cy,
                self.cx.raw(),
                lead,
                tail,
                PackedCell::empty_with(style),
            ),
        }
        self.cells = self.cells.plus(need);
        // The images under the write and the selection's watch, as `glyph`
        // tells them: a character written inside an image's cells frees it.
        let start = self.cx.raw();
        // Where the write ended, not where the cursor stops: see `glyph`.
        let end = Col::at_index(start.index() + need);
        self.overwrote(start, end);
        self.advance(Count::of(need));
        self.row_written = self.row_written.max(end);
        // What `REP` repeats, as `glyph` records it.
        if let Some(cp) = e.content.as_codepoint() {
            self.last_cp = Some((cp, e.flags.width_of(), Pen::Decoded));
        }
    }

    /// **A zero-width codepoint the cluster machine broke before**.
    ///
    /// An Extend or a ZWJ follows a break only after a control, or with
    /// nothing before it: `[[source: uax29]]` GB9 joins it to anything else,
    /// and GB4 and GB5 break after Control, CR and LF, which introduce every
    /// control function. That is a cluster of its own, a defective combining
    /// character sequence (TUS D57, "a sequence of combining characters
    /// appears at the start of a string or follows a control or format
    /// character"), and it went to the cell before the cursor until
    /// 2026-10-23: onto a letter the program did not put it on, or dropped
    /// where that cell was empty (note AB, Q24 item 1).
    ///
    /// * **A combining mark takes the cursor's cell**, stored alone as a
    ///   cluster with [`Content::DEFECTIVE`] for its base, so a copy returns
    ///   what was written and a renderer draws it on a no-break space (TUS
    ///   5.13). Copies of it by `REP` come back here, so each is one too.
    /// * **A default-ignorable one draws nothing and has no cell to take**
    ///   (TUS 5.21, for 17.0): it is dropped and counted, so it cannot change
    ///   the character before it, as `U+2764 ESC[m U+FE0F` turned the heart
    ///   into its emoji. Keeping the join would depart from GB4/GB5.
    ///
    /// Any other zero-width codepoint, a Prepend or one of class Control
    /// such as U+200B, joins the cell before the cursor as it did.
    #[cold]
    #[inline(never)]
    fn zero_width<W: Writer>(&mut self, cp: Codepoint, pen: Pen) {
        let class = Classified::of(cp).class;
        if !matches!(class, GraphemeClass::Extend | GraphemeClass::Zwj) {
            return self.attach(cp);
        }
        if is_default_ignorable(cp) {
            // Nothing is left for what follows to join, as after a control:
            // a mark next starts a cluster of its own too.
            self.clusters.refused += 1;
            self.cluster = ClusterBreak::after_control();
            return;
        }
        self.glyph_by::<W>(cp, Width::NARROW, pen);
        self.last_cp = Some((cp, Width::ZERO, pen));
        let (r, owner) = (
            self.cy,
            self.view.lead(self.cy, self.after_last_write().before()),
        );
        let id = match self
            .clusters
            .try_intern_cold(Content::DEFECTIVE, &[cp.value()])
        {
            Ok(id) => id,
            Err(why) => self.refused(why, Content::DEFECTIVE, &[cp.value()]),
        };
        if !id.is_none() {
            self.view.set_cluster(r, owner, id);
        }
        self.marked(r);
    }

    /// The pen for a character that arrived decoded: [`Paint::flags`] with
    /// the national tag cleared, since no national set replaced such a
    /// character (`[[source: dec-std-070]]` p. 5-140 stores a code above 126
    /// with the GR set, never with GL). Cleared at each write rather than
    /// kept in a field beside `flags`, which counted more instructions on
    /// `cat-cjk`.
    #[inline(always)]
    pub(crate) fn decoded_pen(&self) -> CellFlags {
        self.flags.with_received(None)
    }

    /// **The graph again, after the encoding or the Unicode version
    /// changed**: `ESC %`, DECRSTS, `UnicodeVersion`, and a restore of the
    /// saved cursor (DECRC, SCORC, `?1048`, `?1049`), which carries the
    /// version DECSC saved; and whenever a SAPV form starts or stops
    /// waiting (`crate::variants`). `step` asserts the two agree in debug
    /// builds.
    pub(crate) fn reselect(&mut self) {
        self.graph = crate::graph::graph_of(
            self.cold.encoding,
            self.cold.widths.widths(),
            crate::variants::form_read(self.cold.modes),
        );
    }

    /// One glyph received as a GL code, under the pen that names GL's set.
    #[inline(always)]
    pub(crate) fn glyph(&mut self, cp: Codepoint, w: Width) {
        self.glyph_as(cp, w, Pen::Gl);
    }

    /// One glyph under the cursor and the pen `pen` names, wrapping first if
    /// it will not fit.
    #[inline(always)]
    pub(crate) fn glyph_as(&mut self, cp: Codepoint, w: Width, pen: Pen) {
        self.glyph_by::<Plain>(cp, w, pen);
    }

    /// [`Paint::glyph_as`] by the writer `W`: the text path's, or SAPV's
    /// while a form waits or was just taken ([`Formed`]).
    #[inline(always)]
    pub(crate) fn glyph_by<W: Writer>(&mut self, cp: Codepoint, w: Width, pen: Pen) {
        // **A zero-width codepoint gets no cell of its own unless it starts a
        // cluster** (`Paint::zero_width`: after a control, a combining mark
        // takes one as a defective sequence, since 2026-10-23). The joined
        // case never reaches here. A combining mark, a ZWJ or a variation
        // selector took a
        // full cell here and advanced the cursor a column, so everything after
        // it on the row sat one place right of where `baud-core2` put it --
        // which was about a third of the pinned glyph divergence.
        //
        // `baud-core2` reached the same answer through a *second* arm that
        // fired whatever the break decision was (its `put_cp_classified`, and
        // `ClusterBreak::after_ascii_run`'s doc says so in as many words), so
        // this reproduced its cell output exactly rather than approximately,
        // for every input, with no state and no 24 KB transition table.
        //
        // It stores the codepoint too, since the cluster store landed:
        // `attach` puts it in `ClusterStore`, keyed on content, not position.
        if w == Width::ZERO {
            self.zero_width::<W>(cp, pen);
            return;
        }
        // **Under DECRLM, written leftward** (`crate::leftward`), by SAPV's
        // writer alone: the text path's never reads the bit.
        if W::LEFTWARD && self.cold.modes.is_set(ModeBit::CursorRightToLeft) {
            return self.glyph_leftward(cp, w, pen);
        }
        // **Under IRM, shifted by HEM or as drawn** (`crate::editing`),
        // by SAPV's writer alone, as DECRLM's.
        if W::LEFTWARD && self.cold.modes.is_set(ModeBit::InsertShifted) {
            return self.glyph_shifted(cp, w, pen);
        }
        let need = 1 + usize::from(w == Width::WIDE);
        if self.cx.raw().index() + need > self.cold.fence.index() + 1
            && self.overflow(Count::of(need)) == Fit::LeadAlone
        {
            return self.lead_alone::<W>(cp, w, pen);
        }
        // After the wrap, because the gap belongs in the row the character
        // lands in rather than the one it was pushed off; and **a SAPV form
        // waiting for the following graphic character, or taken by the last
        // one**, there too under SAPV's writer (`crate::variants`).
        W::before(self, cp, Count::of(need));
        let style = self.style;
        let flags = match pen {
            Pen::Gl => self.flags,
            Pen::Decoded => self.decoded_pen(),
            Pen::Kept(set) => self.flags.with_received(set),
        };
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
        // invasive, and it would have bought 3%. The readers are outside this
        // crate too: `baud-domain`'s grid and reflow, `baud-render`'s cell
        // map, overlay and selection, and both difftests find a wide
        // character's second column by its `WIDE_TAIL` cell.
        #[cfg(feature = "ceiling-implicit-tail")]
        let tail: Option<PackedCell> = None;
        #[cfg(not(feature = "ceiling-implicit-tail"))]
        let tail = match w == Width::WIDE {
            true => Some(PackedCell::wide_tail(style, flags)),
            false => None,
        };
        let lead = PackedCell::glyph(cp, style, flags);
        // What `REP` repeats, recorded where the character is written.
        self.last_cp = Some((cp, w, pen));
        // **The whole per-character write, priced by doing it twice.**
        // `put_glyph_fresh` is idempotent -- same row, same column, same cell,
        // same extent -- so the second call changes nothing and costs exactly
        // one addressing plus one pair of stores. This is what `ceiling-no-
        // write` could not price, because stopping the write also stops the
        // cursor and with it every wrap and every scroll.
        #[cfg(feature = "probe-double-put")]
        if self.cx.raw() >= self.row_written {
            self.view
                .put_glyph_fresh(self.cy, self.cx.raw(), lead, tail);
        }
        // **The column is read from the field at each use, not once into a
        // local.** Held in a local, it stayed in a register across the
        // store's call and the loop's allocation around it moved: 6 M
        // instructions over 200 reps of `cat-cjk`.
        match self.cx.raw() >= self.row_written {
            true => self
                .view
                .put_glyph_fresh(self.cy, self.cx.raw(), lead, tail),
            false => self.view.put_glyph(
                self.cy,
                self.cx.raw(),
                lead,
                tail,
                PackedCell::empty_with(style),
            ),
        }
        self.cells = self.cells.plus(need);
        let start = self.cx.raw();
        let end = Col::at_index(start.index() + need);
        self.overwrote(start, end);
        self.advance(Count::of(need));
        // **Where the write ended, not where the cursor stops.** With
        // `DECAWM` reset the cursor pins to the last column, one short of a
        // wide character written into the last two, and a mark at the cursor
        // put the next write there -- onto the tail -- on the fresh path.
        self.row_written = self.row_written.max(end);
    }

    /// Start the cluster machine again at column 0, from the state a control
    /// leaves, and put `cl` to it: the machine said `cl` continued a cluster,
    /// and at column 0 there is nothing on the row for it to continue, so
    /// `glyph` writes it in a cell of its own.
    ///
    /// **This is what a carriage return leaves the machine**, without a store
    /// in the `CR` arm: after `CR`, and after the `CR LF` the arm folds, the
    /// cursor is at column 0, and every character written there either breaks
    /// -- whose successor forgets the state before it,
    /// `a_break_forgets_the_state_before_it` -- or comes here. Without it,
    /// `U+1F1E6 CR U+1F1E7 U+1F1E8` paired the `B` with the `A` it overwrote
    /// and left the `C` alone
    /// (`a_carriage_return_ends_a_regional_indicator_pair`). `#[cold]` like
    /// `attach`, beside which `glyph` calls it.
    #[cold]
    #[inline(never)]
    // `glyph`'s classified arm is the one caller, and `ceiling-no-classify`
    // compiles that arm out.
    #[cfg(not(feature = "ceiling-no-classify"))]
    pub(crate) fn restart_cluster(&mut self, cl: Classified) {
        self.cluster = ClusterBreak::after_control();
        let _ = self.cluster.is_break_classified(cl.class, cl.lookback);
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
    fn attach_before(&mut self, cp: Codepoint, after: Col) {
        // A continuation with nothing before it on the line has no base.
        // Dropping it is correct: `[[source: uax29]]` GB1 puts a boundary at
        // start of text, so it is its own cluster and there is no cell for it.
        if after == Col::ZERO {
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
            let o = self.view.lead(self.cy, after.before());
            let c = self.view.cell_at(self.cy, o);
            core::hint::black_box(self.clusters.get(c.content).is_some());
        }
        let (r, owner) = (self.cy, self.view.lead(self.cy, after.before()));
        let cell = self.view.cell_at(r, owner);
        if cell.content.is_empty() {
            self.clusters.refused += 1;
            return;
        }
        if cell.content.is_placeholder_base() {
            return self.attach_placeholder(r, owner, cell.content, cp);
        }
        // The cell's base: its own content, or its slot's when it names one,
        // since a cell with continuations holds its slot's word instead.
        let mut cps = [0u32; ClusterSlot::CAPACITY];
        let mut n = 0usize;
        let base = match self.clusters.get(cell.content) {
            Some(s) => {
                for c in s.marks() {
                    if n < cps.len() {
                        cps[n] = c;
                        n += 1;
                    }
                }
                s.base
            }
            None => cell.content,
        };
        if n == cps.len() {
            // A cluster past the table's capacity keeps what it has rather
            // than losing its head, and says one was lost; its row counts
            // as written, as xterm's does.
            self.clusters.refused += 1;
            return self.marked(r);
        }
        cps[n] = cp.value();
        n += 1;
        let id = match self.clusters.try_intern(base, &cps[..n]) {
            Ok(id) => id,
            Err(why) => self.refused(why, base, &cps[..n]),
        };
        match id.is_none() {
            true => {}
            false => self.view.set_cluster(r, owner, id),
        }
        self.widen_for(r, owner, cp, base, &cps[..n]);
        self.marked(r);
    }

    /// What a continuation `cp` asks of the cell at `(r, owner)` once it
    /// holds the cluster of `base` and `marks`, `cp` the last of them: a
    /// `U+FE0F` may make it two cells, and under Unicode 8 a flag is.
    #[inline(always)]
    fn widen_for(&mut self, r: Row, owner: LeadCol, cp: Codepoint, base: Content, marks: &[u32]) {
        let n = marks.len();
        // **A `U+FE0F` after a listed base makes it two cells** (`vs16.rs`).
        // The codepoint before it decides, as ghostty's `graphemeWidthEffect`
        // does: `U+2620 U+FE0F` inside a ZWJ family widens nothing, because
        // the family's base is wide already, and a mark between the base and
        // the selector means the selector is not the base's.
        if cp.value() == 0xFE0F && self.cold.vs16 == Vs16Width::Two {
            let before = match n {
                1 => base.as_codepoint(),
                _ => Codepoint::new(marks[n - 2]),
            };
            if before.is_some_and(vs16::widens) {
                self.widen(r, owner.col());
            }
        }
        // **Under Unicode 8 a flag is widened where its two indicators
        // join**: East Asian Width 8.0 gives a regional indicator one column
        // (`[[source: eaw-data-8]]`), and iTerm2 draws a flag in two under
        // every version while a lone indicator keeps its version's width
        // (`[[source: iterm2-964a75d-screenchar]]`). The current tables make
        // the first indicator wide already, so the version is asked only
        // after the codepoint is one.
        if cp.is_regional_indicator()
            && n == 1
            && self.cold.widths.widths() == UnicodeWidths::Eight
            && base
                .as_codepoint()
                .is_some_and(Codepoint::is_regional_indicator)
        {
            self.widen(r, owner.col());
        }
    }

    /// **A mark joined a cell on row `r`, so its row is written**: told to
    /// the watch on a selection's lines behind the byte the text writers
    /// test (`Paint::hooks`), on while a watch is armed or an image placed.
    /// Called last, where nothing `attach_before` holds must live across
    /// it: at the function's head, where the row was found, a scratch arm
    /// of this test cost `cat-cjk` 0.53% of its instructions over an arm
    /// without it, in the registers the rest of the function lost to it;
    /// here, before the capacity branch called it, it cost 0.12% over that
    /// arm, and the change as a whole equals the base before the watch
    /// (287e328d) on `cat-cjk`. A mark past a cluster's capacity changes no
    /// cell and is told too, as xterm's row is written whether or not a
    /// combining slot was free.
    #[inline(always)]
    fn marked(&mut self, r: Row) {
        if self.hooks == WriteHooks::On {
            self.wrote_rows(r, r);
        }
    }

    /// **A mark after a kitty Unicode placeholder** (`[[source:
    /// kitty-graphics]]`, "Unicode placeholders"): a listed diacritic, up
    /// to three, goes into the cell's own word ([`Content::placeholder`]),
    /// and anything else sends the placeholder and its marks to the cluster
    /// store as any base's would go. `kitten icat` writes three diacritics
    /// on every cell, each cell a cluster of its own, and the store dropped
    /// 2366 of an 80x24 picture's 5760 marks
    /// (`every_diacritic_of_a_kitten_picture_is_kept`).
    ///
    /// `#[cold]` and out of line, behind one compare in `attach_before`, so
    /// a mark on any other base runs what it ran.
    #[cold]
    #[inline(never)]
    fn attach_placeholder(&mut self, r: Row, owner: LeadCol, content: Content, cp: Codepoint) {
        self.marked(r);
        if let Some(word) = content.with_diacritic(cp) {
            self.view.set_placeholder(r, owner, word);
            return;
        }
        let mut cps = [0u32; ClusterSlot::CAPACITY];
        let mut n = 0usize;
        for (d, m) in cps.iter_mut().zip(self.clusters.marks_of(content)) {
            *d = m;
            n += 1;
        }
        cps[n] = cp.value();
        n += 1;
        let base = self.clusters.base_of(content);
        let id = match self.clusters.try_intern_cold(base, &cps[..n]) {
            Ok(id) => id,
            Err(why) => self.refused(why, base, &cps[..n]),
        };
        if !id.is_none() {
            self.view.set_cluster(r, owner, id);
        }
    }

    /// A cluster the store refused: swept for, when the store is full and a
    /// sweep is due ([`ClusterStore::sweep_due`]), else counted.
    ///
    /// **Out of line, and that is measured.** Inline in `attach_before`,
    /// the full-store test cost the hit path a few instructions an intern
    /// in register choices and block order: `cat-cjk` +0.62%, Genesis 1:1-5
    /// +3.86% and Pango's Genesis 1 +7.63% against 0c0c30a8, where out of
    /// line they are +0.30%, +3.11% and +6.84%.
    #[cold]
    #[inline(never)]
    fn refused(&mut self, why: Refused, base: Content, cps: &[u32]) -> ClusterId {
        match why {
            Refused::Full if self.clusters.sweep_due() => self.sweep_and_intern(base, cps),
            Refused::Full | Refused::Shape => {
                self.clusters.refused += 1;
                ClusterId::NONE
            }
        }
    }

    /// **The sweep**: every slot no cell names is freed, and `base` plus `cps`
    /// probes again. Sweep when full: when all slots are used, walk the live
    /// cells once and free unreferenced slots. It runs when a cluster finds
    /// every slot it may take holding another and every slot of the store
    /// holds a cluster, at most once every `SLOTS_MAX / 16` interns that
    /// wanted a slot ([`ClusterStore::sweep_due`]).
    ///
    /// **Every cell that can name a slot is marked**: the live surface's
    /// whole buffer -- its screen, the primary's history and the rows held
    /// past a narrower edge -- and the other surface's. A cell this misses
    /// would lose its base with its marks, since the slot holds both. The
    /// cell being attached to still names its prefix, so the prefix is live
    /// until the new cluster replaces it.
    ///
    /// `#[cold]` and out of line like `attach_before`: one sweep walks a
    /// 200x50 grid's two surfaces and 2,000 lines of history, about 437,000
    /// cells, in about 122 µs on its first build.
    #[cold]
    #[inline(never)]
    fn sweep_and_intern(&mut self, base: Content, cps: &[u32]) -> ClusterId {
        let mut live = ClusterLive::new();
        live.mark(self.view.ring_cells());
        live.mark(self.other.cells);
        // The status line's row, or the main display's surface while the
        // line is written (`status.rs`).
        live.mark(self.status.cells);
        self.clusters.sweep(&live);
        self.clusters.intern(base, cps)
    }

    /// A zero-width codepoint the cluster machine did not join to anything:
    /// it goes to the character the last write put down, the policy `glyph`
    /// documents, found as [`Paint::attach_continuing`] finds it -- before the
    /// cursor, **or under it** while a write in the last column with `DECAWM`
    /// reset has it pinned there. Reading `cx - 1` here put a mark after an
    /// `SGR`, a `BEL` or an `OSC`, and a `U+200B`, which the machine breaks
    /// before with nothing between, on the letter left of that character with
    /// autowrap reset and on the character with it set
    /// (`tests/margin_pin.rs`).
    ///
    /// **Here a control can have come between**, so the pin is kept fresh by
    /// what moves the cursor: every horizontal move goes through
    /// [`Paint::set_col`], which ends it, and a `?7h` or `?7l` ends it too, as
    /// does a resize that clamps the cursor. A vertical move keeps it, as it
    /// keeps the pending-wrap column with `DECAWM` set, so the mark goes to the
    /// last column of the row the cursor is on under both modes. Where a mark
    /// after a control belongs at all is note AB's Q24: GB4 and GB5 end the
    /// cluster there, a defective combining character sequence
    /// (`[[source: unicode18-ch3]]` D57).
    #[cold]
    #[inline(never)]
    pub(crate) fn attach(&mut self, cp: Codepoint) {
        let after = self.after_last_write();
        match self.clusters.may_name(cp) {
            true => self.attach_named(cp, after),
            false => self.attach_before(cp, after),
        }
    }

    /// The column after the character the last write put down: the cursor's,
    /// or one past it where that write ended in the last column with `DECAWM`
    /// reset and nothing has moved the cursor across since
    /// ([`crate::term::Pinned`]).
    fn after_last_write(&self) -> Col {
        if self
            .cold
            .modes
            .either_set(ModeBit::CursorRightToLeft, ModeBit::InsertShifted)
        {
            return self.after_moded_write();
        }
        self.after_plain_write()
    }

    /// [`Paint::after_last_write`] under DECRLM, and under IRM with HEM's
    /// PRECEDING, where the cursor stays on the character it wrote
    /// (`crate::editing`), out of line.
    #[cold]
    #[inline(never)]
    fn after_moded_write(&self) -> Col {
        let m = self.cold.modes;
        match (
            m.is_set(ModeBit::CursorRightToLeft),
            m.is_set(ModeBit::EditPreceding),
        ) {
            (true, _) => self.after_leftward_write(),
            (false, true) => self.cx.col().after(),
            (false, false) => self.after_plain_write(),
        }
    }

    /// [`Paint::after_last_write`] for a cursor the write moved on.
    #[inline(always)]
    fn after_plain_write(&self) -> Col {
        let on_last = !self.cold.modes.is_set(ModeBit::AutoWrap)
            && self.cold.pinned == crate::term::Pinned::OnLast;
        // The column, placed or not: a mark after a move to one past the
        // margin goes before it, as after a move anywhere.
        let c = self.cx.col();
        match on_last {
            true => Col::at_index(c.index() + 1),
            false => c,
        }
    }

    /// Move the cursor to column `c` without writing there, which ends a pin:
    /// the character the last write put down is no longer under the cursor
    /// or before it ([`crate::term::Pinned`], [`Paint::attach`]). Every
    /// horizontal move but a write's comes here; a write sets the pin itself,
    /// in [`Paint::end_at`].
    ///
    /// **The pin is cleared only where it is set**, which is only ever with
    /// `DECAWM` reset, so with autowrap set a move writes no byte for it
    /// (rule 1). Paired by round it counts 1.7-4.2 instructions a move above
    /// the parent, and a plain store 1.5-2.2; these are medians whose ranges
    /// touch the parent's, so neither is a result; the store form read `cat`
    /// below the parent in every round paired and this form level, a lead
    /// and not a result, since the ranges touch.
    ///
    /// **A [`CursorCol`], so each move says whether it can land one past the
    /// right margin**: [`CursorCol::moved`] where it cannot,
    /// [`Paint::place`] where it can.
    #[inline(always)]
    pub(crate) fn set_col(&mut self, c: CursorCol) {
        self.cx = c;
        if self.cold.pinned != crate::term::Pinned::Before {
            self.cold.pinned = crate::term::Pinned::Before;
        }
    }

    /// A codepoint the cluster machine joined to the character before it:
    /// the character the last write put down, which is before the cursor,
    /// **or under it** where `DECAWM` is reset and that write ended in the
    /// last column ([`crate::term::Pinned`]). Reading `cx - 1` there put
    /// `e U+0301` in the last column on the letter left of the `e`, and a
    /// `U+FE0F` after `☠ a` widened the `☠` over the `a`
    /// (`tests/margin_pin.rs`).
    ///
    /// The pin is fresh here: a continuation means no control came between,
    /// so the last write set it, and `DECAWM` cannot have changed since. A
    /// resize of the primary carries the cursor with the character it is on,
    /// wider or reflowed narrower, so the pin still names the last write
    /// (`tests/margin_pin.rs`); the alternate screen's clamp on a narrower
    /// resize is not examined.
    #[cold]
    #[inline(never)]
    // `glyph`'s classified arm is the one caller, and `ceiling-no-classify`
    // compiles that arm out.
    #[cfg(not(feature = "ceiling-no-classify"))]
    pub(crate) fn attach_continuing(&mut self, cp: Codepoint) {
        let after = self.after_last_write();
        match self.clusters.may_name(cp) {
            true => self.attach_named(cp, after),
            false => self.attach_before(cp, after),
        }
    }

    /// **A continuation that may make a sequence the Unicode Standard
    /// names** (`ClusterStore::named`): one step of a trie built from the
    /// pinned files, and the cell names the node it reaches -- no slot is
    /// hashed for, probed or written. Every emoji cluster in the
    /// emoji-dense recordings is one; their Devanagari signs and kana marks
    /// still go to the store. Anything else is `attach_before`'s, which
    /// interns the cluster whole, a node's base and continuations read from
    /// its slot as a store slot's are.
    ///
    /// **Ahead of `attach_before`, not in it**, behind one bit for `cp`'s
    /// block: with the step inside it, the kana-and-mark shape, whose mark
    /// never takes it, ran 37 to 74 instructions a pair more in every arm
    /// tried, one of them because `try_intern` became a call.
    #[inline(never)]
    fn attach_named(&mut self, cp: Codepoint, after: Col) {
        if after != Col::ZERO {
            let (r, owner) = (self.cy, self.view.lead(self.cy, after.before()));
            let cell = self.view.cell_at(r, owner);
            if let Some((id, s)) = self.clusters.named(cell.content, cp) {
                self.view.set_cluster(r, owner, id);
                self.widen_for(r, owner, cp, s.base, s.cps());
                return self.marked(r);
            }
        }
        self.attach_before(cp, after)
    }

    /// Make the narrow character at `(r, o)` two cells wide, as its `U+FE0F`
    /// asks: in place when the row has room, with the cursor moved past the
    /// new tail; and at the right margin, moved whole to the next row as a
    /// wide character that cannot fit is, leaving its cell blank -- ghostty's
    /// rule. The margin case needs `DECAWM`, and asks nothing about it: only
    /// a write with autowrap on leaves the cursor past the last column, where
    /// `x + 1 == cx` can hold for a base on it.
    ///
    /// Only the write in progress: the character directly left of the
    /// cursor, and not one already wide -- a cursor moved onto a wide
    /// character's tail has that character directly left of it.
    #[cold]
    #[inline(never)]
    fn widen(&mut self, r: Row, o: Col) {
        self.refresh_fence();
        let x = o.index();
        if x + 1 != self.cx.col().index()
            || self
                .view
                .get(r, Col::at_index(x + 1))
                .content
                .is_wide_tail()
        {
            return;
        }
        let cell = self.view.get(r, o);
        let tail = PackedCell::wide_tail(cell.style, cell.flags.without_cluster());
        let blank = PackedCell::empty_with(self.style);
        if o < self.cold.fence {
            self.view.put_glyph(r, o, cell, Some(tail), blank);
            self.cells = self.cells.plus(1);
            self.overwrote(Col::at_index(x + 1), Col::at_index(x + 2));
            self.advance(Count::ONE);
            // The new tail, which a pinned cursor stops short of.
            self.row_written = self.row_written.max(Col::at_index(x + 2));
        } else if o == self.cold.fence && self.cold.fence > self.cold.left {
            self.view.put_glyph(r, o, blank, None, blank);
            self.wrap_bounded();
            let start = self.cx.raw();
            self.view.put_glyph(self.cy, start, cell, Some(tail), blank);
            self.cells = self.cells.plus(2);
            self.overwrote(start, Col::at_index(start.index() + 2));
            self.advance(Count::of(2));
        }
        self.row_written = self.row_written.max(self.cx.col());
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
        // **Never under IRM**: the one-by-one graph takes the text then
        // (`crate::variants::form_read`), each character opening its gap.
        // A run here wrote over the line, so a uniform run that fit lost
        // what it should have pushed on.
        debug_assert!(!self.inserting(), "a run under IRM opens no gap");
        let (style, flags) = (self.style, self.decoded_pen());
        match w == Width::WIDE {
            true => self
                .view
                .put_wide_run(self.cy, self.cx.raw(), cps, style, flags),
            false => self
                .view
                .put_narrow_run(self.cy, self.cx.raw(), cps, style, flags),
        };
        let n = cps.len() * (1 + usize::from(w == Width::WIDE));
        self.cells = self.cells.plus(n);
        let start = self.cx.raw();
        let end = Col::at_index(start.index() + n);
        self.cx = self.end_at(end);
        self.row_written = self.row_written.max(end);
        self.overwrote(start, end);
        // What `REP` repeats, once a run, as `piece` records an ASCII run's:
        // the run never set it, so sixteen UTF-8 `Ä` and `CSI 2 b` wrote
        // no cell (`rep_repeats_the_last_character_of_every_writer`).
        if let Some(&last) = cps.last() {
            self.last_cp = Some((last, w, Pen::Decoded));
        }
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
    ///
    /// **None when the cursor is past the right margin**, which `CUP` can
    /// put it since the side margins do not confine it. This subtracted
    /// past zero there -- a panic in a debug build, and in release a run
    /// taken as fitting: written on from the cursor past the margin, and
    /// clipped at the screen's edge with the rest dropped when it reached
    /// it. No room sends the run to `wrapped`, which writes it on to the
    /// screen's edge from there ([`Paint::past_margin`]), and a tagged
    /// value, past every column, has none either.
    #[inline(always)]
    pub(crate) fn room(&self) -> Count {
        Count::of((self.cold.fence.index() + 1).saturating_sub(self.cx.raw().index()))
    }

    /// Move the cursor anywhere on the screen.
    ///
    /// **Through [`Paint::set_row`], which re-reads `row_written` from the
    /// grid**: `CUP` lands on a row holding text, and a first write there on
    /// the fresh path would orphan whatever wide character it half-covers.
    /// This was the one move that re-read the mark, and the others kept the
    /// row they left (`tests/wide_halves.rs`).
    ///
    /// **Not placed**: its callers land at a margin or the origin, and the
    /// ones that can land one past the right margin -- `CUP`, the VT52
    /// address, `DECRC` and the `?1049 h` keep -- place their own
    /// ([`Paint::goto_at`], [`Paint::place`]).
    pub(crate) fn goto(&mut self, r: Row, c: Col) {
        let r = Row::from_index(r.index().min(self.rows.index() - 1));
        self.set_col(CursorCol::moved(Col::at_index(
            c.index().min(self.cols.index() - 1),
        )));
        self.deferred_reset();
        self.set_row(r);
    }

    /// [`Paint::goto`] to a column already on the screen and already
    /// encoded, which `CUP` places in [`Paint::address_col`].
    pub(crate) fn goto_at(&mut self, r: Row, c: CursorCol) {
        let r = Row::from_index(r.index().min(self.rows.index() - 1));
        self.set_col(c);
        self.deferred_reset();
        self.set_row(r);
    }

    /// Where a move that may land one past the right margin leaves the
    /// cursor: [`CursorCol::placed`], tagged there under side margins
    /// narrower than the screen.
    ///
    /// **One bit inline**, as for `CR` and `CUF`: only `?69` admits a right
    /// margin left of the last column -- `CSI s` is `DECSLRM` only under it,
    /// and `?69l`, `DECSTR` and `DECCOLM` put the margins back with it -- so
    /// with it reset no move lands one past the margin. **The encode is
    /// inline too**: out of line, the call gave `goto`, a leaf, a frame,
    /// about nine instructions a `CUP` that `vim.raw` paid 7,120 times a
    /// rep.
    #[inline(always)]
    pub(crate) fn place(&self, c: Col) -> CursorCol {
        debug_assert!(self.cold.modes.is_set(ModeBit::SideMargins) || !self.fenced());
        match self.cold.modes.is_set(ModeBit::SideMargins) {
            false => CursorCol::moved(c),
            true => CursorCol::placed(c, self.cold.right, self.cols),
        }
    }

    /// Which state the cursor is in, under the margins in force.
    pub(crate) fn cursor_at(&self) -> CursorAt {
        self.cx.at(self.line_fence(), self.cols)
    }

    /// Erase within the cursor's row. 0 to the end, 1 from the start, 2 all.
    ///
    /// **To the line's end**, which on a line not single-width is its half:
    /// `x <= end_of_line(line)` (`[[source: dec-std-070]]` EL, p. 5-155).
    /// So the rendition stays, as EL's note 1 has it: a fill short of the
    /// row's end keeps the mark's flags, and on a single-width line there
    /// is no rendition to keep.
    pub(crate) fn erase_line(&mut self, e: EraseExtent) {
        if self.cold.modes.is_set(ModeBit::PresentationEdits) && self.erase_line_presented(e) {
            return;
        }
        let (cy, end) = (self.cy, self.line_cols());
        let (a, b) = match e {
            EraseExtent::ToStart => (Col::ZERO, Col::at_index(self.cx.col().index() + 1)),
            EraseExtent::All => (Col::ZERO, Col::at_index(end.index())),
            EraseExtent::ToEnd => (self.cx.col(), Col::at_index(end.index())),
        };
        // To its end, the line is being cleared: what it held past a
        // narrower edge goes first (`Paint::clear_erased_past_edge`).
        let blank = self.blank();
        if e != EraseExtent::ToStart && self.past_edge() {
            let at = self.view.rotation().advance(cy.index(), self.view.ring());
            self.clear_erased_past_edge(at, blank);
        }
        self.erase_span(cy, a, b, blank);
        self.wrote_rows(cy, cy);
    }

    /// Erase the screen. 0 below the cursor, 1 above it, 2 all of it.
    pub(crate) fn erase_display(&mut self, e: EraseExtent) {
        let (rows, cols) = (self.rows.index(), Col::at_index(self.cols.index()));
        let blank = self.blank();
        let (from, to) = match e {
            EraseExtent::ToStart => (0, self.cy.index()),
            EraseExtent::All => (0, rows),
            EraseExtent::ToEnd => (self.cy.index() + 1, rows),
        };
        // The rows covered whole are being cleared: what they held past a
        // narrower edge goes first (`Paint::clear_erased_rows_past_edge`).
        if self.past_edge() {
            self.clear_erased_rows_past_edge(Row::from_index(from), Row::from_index(to));
        }
        for y in from..to {
            self.erase_span(Row::from_index(y), Col::ZERO, cols, blank);
        }
        if from < to {
            // The lines it erases whole take the direction in force, the
            // draft's "spreads the current BiDi modes into the entire
            // cleared area" (`crate::path`).
            self.path_rows(Row::from_index(from), Row::from_index(to - 1));
            self.wrote_rows(Row::from_index(from), Row::from_index(to - 1));
        }
        // **ED 1 and ED 2 end the history's newest line**, which a wrap may
        // have left continuing onto the first row they erase: "the last
        // scrolled out line also needs to change its ending to hard newline
        // in modes 1 and 2", even with the cursor on the top row
        // (`[[source: terminal-wg-bidi-escape-sequences]]`), so the screen's
        // first line is a paragraph of its own.
        if e != EraseExtent::ToEnd
            && self.cold.screen == crate::term::Screen::Normal
            && !self.on_status()
            && self.book.hist.index() > 0
        {
            self.view.set_ended_behind(Rows::of_count(0));
        }
        // The cursor's own row is a partial erase either side of it. The whole
        // screen takes its kitty images with it (`Paint::kitty_clear`).
        match e {
            EraseExtent::All if self.images.any_placed() && !self.on_status() => self.kitty_clear(),
            EraseExtent::All => {}
            e => {
                self.erase_line(e);
                if self.line_fence() != self.cold.right {
                    self.erased_line_whole(e);
                }
            }
        }
    }

    /// ED on a sized cursor line: "The Line Renditions of all lines
    /// completely erased ... are set to single-width" (ED note 1,
    /// `[[source: dec-std-070]]` p. 5-156) -- the rows ED covers whole are
    /// filled to the row's end, which leaves them single-width, and the
    /// cursor's line is erased whole by ED 0 from its first column and by
    /// ED 1 through its last (pp. 5-157 and 5-158, `if (active_position.
    /// column == 1)` and `== end_of_line(line)`; a product that did not
    /// reset it for ED 1 was granted a waiver, p. C-40).
    #[cold]
    #[inline(never)]
    fn erased_line_whole(&mut self, e: EraseExtent) {
        let whole = match e {
            EraseExtent::ToEnd => self.cx.col() == Col::ZERO,
            EraseExtent::ToStart => self.cx.col() >= self.line_fence(),
            EraseExtent::All => true,
        };
        if whole {
            self.view.set_rendition(self.cy, LineRendition::Single);
            self.row_changed_to(self.cy);
            self.cold.fence = crate::bounds::limited(self.cold.right, self.cold, self.cols);
        }
    }

    /// Queue a report for the host.
    ///
    /// **Every report `feed` makes comes through here, and it is one of four
    /// writers to the reply buffer.** It was called the only one, and was
    /// not: `report_status` (`dcs.rs`) writes DECRQSS, whose body is the
    /// rendition rather than a `Report`; `Term::queue` writes the reports made
    /// outside `feed` -- focus, mouse, `?2048` after a resize or a cell-size
    /// change, `?2031`; and `Term::key` writes keys. All four copy one whole
    /// encoded answer under one check, so a reply is in the buffer whole or
    /// not at all. Here the shape is chosen by the [`Report`] variant rather
    /// than by the call site: a `DSR` cannot be answered with a `DA`'s bytes,
    /// which `docs/vt-coverage.md` calls the one thing a terminal must never
    /// do.
    ///
    /// **The introducers follow `S7C1T` and `S8C1T`.** `cold.replies` is what
    /// `ESC SP F` and `ESC SP G` last set, and `report_status` and
    /// `Term::queue` read it too. This said seven-bit always, on the grounds
    /// that the core implemented neither; it implements both. `Term::key`
    /// reads it as well, so a key and a reply open with the same introducer.
    #[cold]
    #[inline(never)]
    pub(crate) fn want_reply(&mut self, r: Report) {
        let bytes = r.encode(self.cold.replies.c1());
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
            self.refresh_fence();
            self.cx = CursorCol::written(self.cold.fence);
            return;
        }
        // Autoprint, a selected character path and side margins out of
        // line, one test.
        if self.cold.modes.any_of_three(
            ModeBit::AutoPrint,
            ModeBit::PathSelected,
            ModeBit::SideMargins,
        ) {
            return self.wrap_moded();
        }
        self.cx = CursorCol::moved(self.cold.left);
        // The row being left continues onto the next: the fact a reflow
        // needs, kept in the mark the write already touched.
        self.view.set_wrapped(self.cy);
        self.index();
    }

    /// [`Paint::wrap`] under autoprint, a selected character path or
    /// DECLRMM. Autoprint prints the line an autowrap leaves, ended by `LF`
    /// (070-7 § 7.3.7, "or LF if Auto Wrap"); the row the wrap brings in
    /// takes the direction of the paragraph it joins (`crate::path`). **A
    /// wrap at side margins narrower than the page marks the row it leaves**
    /// ([`baud_domain::WrappedAt::Margins`]): the next line starts at the
    /// left margin (`[[source: vt510-decslrm]]`), and the display keeps the
    /// paragraph at its cut, since by baud's reading the row's cells outside
    /// the margins are not the paragraph's.
    #[cold]
    #[inline(never)]
    fn wrap_moded(&mut self) {
        if self.cold.modes.is_set(ModeBit::AutoPrint) {
            self.autoprint(crate::print::LineEnd::LineFeed);
        }
        if self.cold.left != Col::ZERO || self.cold.right.index() + 1 < self.cols.index() {
            let b = self.view.bidi(self.cy);
            if b.wrapped_at() != baud_domain::WrappedAt::Margins {
                self.view
                    .set_bidi(self.cy, b.with_wrapped_at(baud_domain::WrappedAt::Margins));
            }
        }
        self.cx = CursorCol::moved(self.cold.left);
        self.view.set_wrapped(self.cy);
        self.index();
        if self.cold.modes.is_set(ModeBit::PathSelected) {
            self.wrapped_into_path();
        }
    }

    /// The `LF` of a `CR LF` the loop folds into one step, under autoprint
    /// or a selected character path: the line printed, then the line feed
    /// the path rules follow (`crate::path`).
    #[cold]
    #[inline(never)]
    pub(crate) fn cr_lf_moded(&mut self) {
        if self.cold.modes.is_set(ModeBit::AutoPrint) {
            self.autoprint(crate::print::LineEnd::LineFeed);
        }
        let kept = self.presented_col();
        match self.cold.modes.is_set(ModeBit::PathSelected) {
            true => self.line_fed_into_path(),
            false => {
                self.index();
            }
        }
        self.keep_presented(kept);
    }

    /// One row up, scrolling the region when the cursor is on its first row,
    /// at its presentation position under DCSM's PRESENTATION.
    pub(crate) fn reverse_index(&mut self) {
        self.effector_ends_string();
        // At the top margin the cursor stays where the write left it.
        self.end_left_edge();
        if self.cold.modes.is_set(ModeBit::PresentationEdits) {
            return self.reverse_index_presented();
        }
        if self.cy == self.cold.top {
            self.scroll_region_down(Count::ONE);
            self.row_changed_to(self.cy);
            return;
        }
        if self.cy.index() > 0 {
            self.set_row(Row::from_index(self.cy.index() - 1));
        }
    }

    /// [`Paint::reverse_index`] under DCSM's PRESENTATION, out of line:
    /// split into a second function in the line, `less` retired 0.5% more
    /// instructions.
    #[cold]
    #[inline(never)]
    fn reverse_index_presented(&mut self) {
        let kept = self.presented_col();
        if self.cy == self.cold.top {
            self.scroll_region_down(Count::ONE);
            self.row_changed_to(self.cy);
        } else if self.cy.index() > 0 {
            self.set_row(Row::from_index(self.cy.index() - 1));
        }
        self.keep_presented(kept);
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

    /// The cursor's row has just been blanked whole, so nothing on it can be
    /// cut: the rotation at the bottom and a region scrolled up under it.
    /// **Only there** -- a row the cursor moves onto may hold text, and that
    /// is [`Paint::set_row`].
    fn row_blanked(&mut self) {
        self.row_written = Col::ZERO;
    }

    /// Move the cursor to row `r`, and read how far `r` is written.
    ///
    /// **The one spelling of a row change.** `CUU`, `CUD`, `CNL`, `CPL`,
    /// `VPA`, `VPR`, `LF` and `RI` without a scroll, a reverse wrap, VT52's
    /// cursor keys and `DECSLRM`'s home each assigned `cy` and kept the mark
    /// of the row they left, so a write on the row they reached took the
    /// fresh path over text and orphaned the half it covered. `SU`, `SD`,
    /// the screen swap and `DECIC` keep the cursor's row and move its text,
    /// and re-read through [`Paint::row_changed_to`].
    ///
    /// **A row change ends a wrap a DECRLM write left waiting at the left
    /// margin** (`crate::leftward`), as every row change ends the Last
    /// Column Flag (`[[source: dec-std-070]]` 070-D p. D-13). A line feed
    /// takes [`Paint::step_row`], and under DECRLM [`Paint::line_feed`]'s
    /// moded arm, which ends it.
    pub(crate) fn set_row(&mut self, r: Row) {
        self.end_left_edge();
        self.step_row(r);
    }

    /// [`Paint::set_row`] for [`Paint::index`]: a line feed's step down,
    /// which under DECRLM has already ended the wrap waiting.
    #[inline(always)]
    fn step_row(&mut self, r: Row) {
        self.cy = r;
        self.enter_row(r, Arrival::Moved);
    }

    /// The cursor now sits on `r`, whose contents an operation has just
    /// changed. **`row_written` is a shortcut, not a fact**: past it the
    /// orphan probe is skipped because the cells are blank, and an operation
    /// that moves text within a row invalidates that. Re-reading the view's
    /// real extent is what keeps a wide character from being split in half by
    /// the next write.
    pub(crate) fn row_changed_to(&mut self, r: Row) {
        self.enter_row(r, Arrival::InPlace);
    }

    /// The cursor is on row `r`, by a move or with the row changed under it:
    /// read how far it is written, and **its line's rendition from the same
    /// word**: one test of a flag in a value already loaded, and a sized
    /// line out of line ([`Paint::enter_sized`]).
    ///
    /// **Arriving on a single-width line writes nothing**, not even the
    /// fence back to the right margin: the fence a sized line left is too
    /// low, which only ever sends the fit test out of line early, and the
    /// cold paths it reaches read the line's own ([`Paint::refresh_fence`],
    /// [`Paint::line_fence`]). A store here, and in the scroll's blank, was
    /// 0.3-0.6% more instructions retired on `cat`, `btop`, `vim` and
    /// `less`.
    #[inline(always)]
    fn enter_row(&mut self, r: Row, how: Arrival) {
        let e = self.view.extent(r);
        self.row_written = Col::at_index(e.index());
        if e.rendition().is_double() {
            self.enter_sized(e.rendition(), how);
        }
    }

    /// **The fence of the cursor's line**, for a reader that must not see
    /// one a sized line left behind: `right` while they agree, which is
    /// every single-width line the cursor reached from another, and the
    /// line's own otherwise, read from its mark.
    #[inline(always)]
    pub(crate) fn line_fence(&self) -> Col {
        match self.cold.fence == self.cold.right {
            true => self.cold.right,
            false => self.fence_of_line(),
        }
    }

    #[cold]
    #[inline(never)]
    fn fence_of_line(&self) -> Col {
        fence_of(self.view.rendition(self.cy), self.cold.right, self.cols)
    }

    /// The fence set to the cursor's line's, for the cold paths a fit test
    /// sends a write to: it may have read one a sized line left, too low.
    #[inline(always)]
    pub(crate) fn refresh_fence(&mut self) {
        if self.cold.fence != self.cold.right {
            self.cold.fence = self.write_fence_of_line();
        }
    }

    /// The fence for a line of rendition `r`, and on a line not single-width
    /// the cursor inside its half: every move onto one ends with
    /// `if (column > end_of_line(line)) column = end_of_line(line)`
    /// (`[[source: dec-std-070]]` CUU, CUD, CUP, IND, RI, LF, pp. 5-41 to
    /// 5-60). A move can land past the half; the sentinel a write left one
    /// past it is kept where the line changed under the cursor in place,
    /// and a move ends it, as a move resets the Last Column Flag (D.6.1,
    /// pp. D-13 and D-14).
    #[cold]
    #[inline(never)]
    fn enter_sized(&mut self, r: LineRendition, how: Arrival) {
        let own = fence_of(r, self.cold.right, self.cols);
        self.cold.fence = crate::bounds::limited(own, self.cold, self.cols);
        if !r.is_double() {
            return;
        }
        // **Inside the half, not the line limit**: SLL's limit bounds only
        // implicit movement (`crate::bounds`), so a cursor past it stays
        // where it is. Clamped to the limit, a cursor past it on a double
        // line went back to the limit when a read began on the status line,
        // which enters its row again.
        let last = match how {
            Arrival::Moved => own.index(),
            Arrival::InPlace => own.index() + 1,
        };
        if self.cx.raw().index() > last {
            self.set_col(CursorCol::moved(own));
        }
    }

    /// **The width of the cursor's line**, one past its last column: the
    /// screen's on a single-width line and the half on any other. `right`
    /// less the fence is zero on a single-width line and the hidden half on
    /// a sized one, where `right` is the screen's last column.
    #[inline(always)]
    pub(crate) fn line_cols(&self) -> Cols {
        Cols::of_index(self.cols.index() - (self.cold.right.index() - self.line_fence().index()))
    }

    /// A horizontal move's column, kept on the cursor's line: past a
    /// double line's last column it lands there, as `CUF`, `HPR`, `CHA` and
    /// `HPA` stop (`[[source: dec-std-070]]` p. 5-46, "character positions
    /// cannot be addressed which are more than half the current value of the
    /// right edge of the display", p. 5-95). Everything else is `c`.
    /// One compare inline: a column at or inside the fence is on the line
    /// whichever line it is, and only one past it asks the line's own.
    #[inline(always)]
    pub(crate) fn on_line(&self, c: CursorCol) -> CursorCol {
        match c.raw() > self.cold.fence {
            false => c,
            true => self.past_the_fence(c),
        }
    }

    #[cold]
    #[inline(never)]
    fn past_the_fence(&self, c: CursorCol) -> CursorCol {
        let f = self.line_fence();
        match f != self.cold.right && c.raw() > f {
            true => CursorCol::moved(f),
            false => c,
        }
    }

    /// `ESC # 3`, `# 4`, `# 5` and `# 6`: DECDHLT, DECDHLB, DECSWL and
    /// DECDWL, the cursor's line made `to` (`[[source: dec-std-070]]` §
    /// 5.6.1, pp. 5-95 to 5-100; `[[source: vt100-ug-ch3]]`;
    /// `[[source: vt510-decdhl]]`, `[[source: vt510-decdwl]]`,
    /// `[[source: vt510-decswl]]`).
    ///
    /// **Any rendition but single-width empties the line past its half and
    /// pulls the cursor back into it**: "data which is beyond the end of the
    /// Active Line after execution of the control [is] lost", the algorithm
    /// setting each cell past `end_of_line` to `EMPTY_CHARACTER`, and the
    /// column clamped to `end_of_line` (pp. 5-97 to 5-100) -- so DECSWL,
    /// which loses nothing (p. 5-96), brings nothing back. xterm's
    /// `repaint_line` keeps the cells (`[[source: xterm-doublechr]]`), and
    /// vttest's "Bug L" calls the loss a misfeature: practice departing from
    /// the text, and the text is followed. DECDHLB's algorithm assigns
    /// `DOUBLE_HEIGHT_TOP`; its description and purpose say the bottom half,
    /// and the bottom half is set.
    ///
    /// **Not under `DECLRMM`**: DECDWL note 3 and DECDHL note 4, "This
    /// control does not operate when DECLRMM (Left Right Margin Mode) is
    /// set", and xterm's `repaint_line` tests the same mode. DECSWL still
    /// does, on a line that is single-width already.
    ///
    /// A wrap pending before the control is ended on the column it was
    /// pending on, the old line's last column, because DECSWL leaves the
    /// active position where it is (p. 5-96) and DECDWL and DECDHL clamp it
    /// to their end of line.
    #[cold]
    #[inline(never)]
    pub(crate) fn line_rendition(&mut self, to: LineRendition) {
        // A wrap a DECRLM write left waiting ends (`Paint::end_left_edge`).
        self.end_left_edge();
        if to.is_double() && self.cold.modes.is_set(ModeBit::SideMargins) {
            return;
        }
        let (cy, cols) = (self.cy, self.cols.index());
        // Read against the old line's fence: DECSWL widens it, and the
        // sentinel one past the old half would read as a column after. A
        // wrap waiting at SLL's line limit is read against the limit, as a
        // report reads it (`crate::bounds`).
        let fence = self.report_fence();
        let pending = matches!(self.cx.at(fence, self.cols), CursorAt::Pending(_));
        let on = self.cx.reported(fence, self.cols);
        if to.is_double() {
            let eol = Col::at_index(to.end_of_line(self.cols).index());
            let n = self
                .view
                .fill(cy, eol, Col::at_index(cols), PackedCell::EMPTY);
            self.cells = self.cells.plus(n);
            self.overwrote_unprinted(eol, Col::at_index(cols));
        }
        self.view.set_rendition(cy, to);
        self.row_changed_to(cy);
        let own = fence_of(to, self.cold.right, self.cols);
        self.cold.fence = crate::bounds::limited(own, self.cold, self.cols);
        // **And none of the four leaves a wrap pending**: D.6.1 (pp. D-13
        // and D-14) lists SINGLE_WIDTH_LINE, DOUBLE_WIDTH_LINE and both
        // DOUBLE_HEIGHT_LINE controls among those that reset the Last
        // Column Flag, so the cursor is on the column the wrap was pending
        // on, and a double line's clamp below pulls it into the half.
        if pending {
            self.set_col(CursorCol::moved(on));
        }
        // Into the half, not to SLL's limit, which bounds only implicit
        // movement (`Paint::enter_sized`).
        if to.is_double() && self.cx.raw() > own {
            self.set_col(CursorCol::moved(own));
        }
        self.wrote_rows(cy, cy);
    }

    /// A write on the cursor's row over columns `[start, end)`: an image
    /// there is freed ([`Paint::overwrite_images`]) and the row told to the
    /// watch on a selection's rows ([`Paint::wrote_rows`]). Every writer of
    /// text calls it once a run, and once a character on the per-character
    /// path, so **both tests sit behind one compare** (`Paint::hooks`) and
    /// out of line.
    #[inline(always)]
    pub(crate) fn overwrote(&mut self, start: Col, end: Col) {
        if self.hooks == WriteHooks::Off {
            debug_assert!(
                WriteHooks::of(self.images, self.cold.watched, self.cold.bell.armed)
                    == WriteHooks::Off
            );
            return;
        }
        self.overwrote_hooked(start, end);
    }

    #[cold]
    #[inline(never)]
    fn overwrote_hooked(&mut self, start: Col, end: Col) {
        self.overwrite_images(start, end);
        self.wrote_rows(self.cy, self.cy);
        self.margin_bell(start, end);
        self.rehook();
    }

    /// [`Paint::overwrote`] for a write that prints no character -- the
    /// half a double line clears, a rectangle's row -- which neither rings
    /// the margin bell nor disarms it: STD 070 rings it for "Any character
    /// is printed to the screen which causes the cursor to move"
    /// (`margin.rs`). The armed line is set aside across the hook rather
    /// than the hook given a second body, so the text writers' hook keeps
    /// the one caller each of its tests has.
    pub(crate) fn overwrote_unprinted(&mut self, start: Col, end: Col) {
        let armed = self.cold.bell.armed.take();
        self.overwrote(start, end);
        self.cold.bell.armed = armed;
        self.rehook();
    }

    /// **The margin bell's test**, on a write while one is armed: rung when
    /// the write crossed RIGHT MARGIN - 8 on the line it was armed on, the
    /// line's own last column the right margin (`margin.rs`).
    fn margin_bell(&mut self, start: Col, end: Col) {
        if self.cold.bell.armed.is_none() {
            return;
        }
        let at = crate::margin::Line {
            epoch: self.cold.epoch,
            rot: self.view.rot(),
            row: self.cy,
        };
        let fence = self.line_fence();
        if self.cold.bell.wrote(at, start, end, fence) == crate::margin::Crossed::Rung {
            self.margin_bells += 1;
            let b = &mut self.cold.bell;
            *b.margin_rung.at_mut(b.margin) += 1;
        }
    }

    /// **The `BEL`s this read counted since the last fold, credited to the
    /// warning volume they rang at**: before DECSWBV or `RIS` changes it, and
    /// at the read's end ([`Paint::bells_read_out`]), so an embedder reading
    /// the counts after the read hears each at its own volume.
    pub(crate) fn fold_bells(&mut self) {
        let b = &mut self.cold.bell;
        *b.warning_rung.at_mut(b.warning) += self.bells - b.folded;
        b.folded = self.bells;
    }

    /// [`Paint::fold_bells`] at a read's end, which leaves nothing folded
    /// for the next.
    #[cold]
    #[inline(never)]
    pub(crate) fn bells_read_out(&mut self) {
        self.fold_bells();
        self.cold.bell.folded = 0;
    }

    /// [`Paint::hooks`] read again, after anything that may have placed an
    /// image and on the hooked path itself.
    pub(crate) fn rehook(&mut self) {
        self.hooks = WriteHooks::of(self.images, self.cold.watched, self.cold.bell.armed);
    }

    /// Rows `from` to `to` of the live screen written or erased, told to the
    /// watch on a selection's rows while it is armed and nothing has reached
    /// it ([`crate::term::Cold::test_watch`]): text, a mark joined to a
    /// cell, `EL`, `ED`, `ECH`, `ICH`, `DCH`, the selective erases, `DECIC`,
    /// `DECDC`, `DECALN` and a clear for a change of columns. Since
    /// 2026-10-17 a selection goes when a program writes or erases on its
    /// rows, whatever it writes, as xterm disowns one when a write or an
    /// erase lands on a selected row (`[[source: xterm-scroll]]`,
    /// `WriteText`, `ClearInLine`, `ClearScreen`).
    ///
    /// **One compare of the watch's state a call, and the test out of
    /// line**: `ingest2` arms no watch, so an erase pays the compare. Text
    /// reaches it only through [`Paint::overwrote`]'s gate, and a mark
    /// joined to a cell through the same byte.
    #[inline(always)]
    pub(crate) fn wrote_rows(&mut self, from: Row, to: Row) {
        if self.cold.watched == crate::term::Watched::Rows {
            self.watch_written(from, to);
        }
    }

    #[cold]
    #[inline(never)]
    fn watch_written(&mut self, from: Row, to: Row) {
        self.cold.test_watch(from, to, self.book.top);
    }

    /// `LF`, `VT` and `FF`: down one line, and a carriage return **only**
    /// when LNM is set.
    ///
    /// **The mode was recorded and never read**, which is the failure
    /// `docs/vt-coverage.md` rates worse than not implementing it: `CSI 20 h`
    /// set a bit `apply_ansi_mode` stored and nothing consulted, so `LF` moved
    /// down and left the column alone whatever the application asked for.
    /// `baud-core2` always read it here, and the ported
    /// `lf_returns_to_column_one_when_lnm_is_set` is what found the gap.
    ///
    /// **Autoprint is read with LNM, one test of the mode word**, and both
    /// are out of line behind it ([`Paint::line_feed_moded`]): `by` says
    /// which byte moved the cursor, which is the line's ending in print. A
    /// selected character path is a third bit of the same test, and DECRLM
    /// a fifth, whose wrap waiting at the left margin a line feed ends
    /// (`crate::leftward`). A string laid out against an aligned stop is a
    /// sixth, which the line feed ends (`crate::aligned`).
    #[inline(always)]
    pub(crate) fn line_feed(&mut self, by: Feed) {
        // A newline ends the row it leaves, however it was written before.
        self.view.set_ended(self.cy);
        if self.cold.modes.any_of_six([
            ModeBit::LineFeedNewLine,
            ModeBit::AutoPrint,
            ModeBit::PathSelected,
            ModeBit::PresentationEdits,
            ModeBit::CursorRightToLeft,
            ModeBit::Aligning,
        ]) {
            return self.line_feed_moded(by);
        }
        self.index();
    }

    /// [`Paint::line_feed`] under LNM or autoprint. **The line is printed
    /// before anything moves**, as 070-7 § 7.3.7 prints "the active line";
    /// then `CR` first: `[[source: dec-std-070]]` LF's algorithm (p. 5-60)
    /// calls `carriage_return()` before it moves, so the line it leaves
    /// decides where the next one starts under the margins, and SLH's home
    /// is the following line's, as `NEL`'s is (`[[source: ecma48]]`
    /// 8.3.86).
    #[cold]
    #[inline(never)]
    fn line_feed_moded(&mut self, by: Feed) {
        self.effector_ends_string();
        self.end_left_edge();
        if self.cold.modes.is_set(ModeBit::AutoPrint) {
            let end = match by {
                Feed::Received(at) if self.arena[at.index()] == 0x0c => {
                    Some(crate::print::LineEnd::FormFeed)
                }
                Feed::Received(_) => Some(crate::print::LineEnd::LineFeed),
                Feed::LineTab => Some(crate::print::LineEnd::LineTab),
                Feed::Unprinted => None,
            };
            if let Some(end) = end {
                self.autoprint(end);
            }
        }
        let lnm = self.cold.modes.is_set(ModeBit::LineFeedNewLine);
        if lnm {
            self.return_before_feed();
        }
        // **Under DCSM's PRESENTATION** the cursor keeps its presentation
        // position on the line it reaches (`crate::component`).
        let kept = self.presented_col();
        // **A selected character path** goes to the line the cursor reaches
        // (`crate::path`).
        match self.cold.modes.is_set(ModeBit::PathSelected) {
            true => self.line_fed_into_path(),
            false => {
                self.index();
            }
        }
        if lnm {
            self.fed_onto_line();
        }
        self.keep_presented(kept);
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
        //
        // **Full screen means both ends.** This tested only the bottom, so a
        // region ending on the last row but starting below the top --
        // `DECSTBM 5 ; 50` -- took the rotation and retired its header rows
        // into the history. foot scrolls the region at its bottom whatever
        // row that is (`[[source: foot-linefeed]]`, `term_linefeed`).
        //
        // **Side margins make it a rectangle, the whole screen included**,
        // which [`Paint::scroll_region_up`] scrolls in place: the third test
        // here, on a path taken once a line at the bottom.
        if self.cy == self.cold.bot {
            if self.cold.top != Row::ZERO || self.cold.bot.index() != last || self.fenced() {
                self.scroll_region_up(Count::ONE);
                self.row_changed_to(self.cy);
                return self.view.rot();
            }
        } else {
            // **Below the region on the last row there is nothing to do.**
            // The cursor is outside the region, so nothing scrolls, and it is
            // on the last row, so it cannot move: foot's `term_cursor_down`
            // clamps to the screen, and alacritty does the same. This fell
            // through to the rotation and scrolled the whole screen, status
            // line and all. **And the row keeps its mark**: it holds whatever
            // it held, where this zeroed it.
            if self.cy.index() < last {
                self.step_row(Row::from_index(self.cy.index() + 1));
            }
            return self.view.rot();
        }
        self.row_blanked();
        debug_assert!(
            self.cold.top == Row::ZERO && self.cold.bot.index() == last,
            "only a full-screen region rotates the whole screen"
        );
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
        let n = {
            let b = self.blank();
            self.view.fill_new_row(Row::from_index(last), b)
        };
        #[cfg(feature = "ceiling-no-blank")]
        let n = self.view.extent(Row::from_index(last));
        self.cells = self.cells.plus(n.index());
        rot
    }
}

/// What moved the cursor down a line ([`Paint::line_feed`]): a byte the
/// host sent, `LF` or `FF`, read back from the arena only when autoprint
/// needs the line's ending; `VT` with no line tabulation stop after it; or
/// no byte at all, `OSC 133`'s fresh line, which prints nothing.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Feed {
    Received(BytePos),
    LineTab,
    Unprinted,
}

/// How the cursor came to be on a row ([`Paint::enter_row`]): a move put it
/// there, or the row changed under it in place.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Arrival {
    Moved,
    InPlace,
}

/// The write path's fence on a line of rendition `r` under right margin
/// `right` on a screen `cols` wide: `right` on a single-width line, and the
/// last column of the half on any other, where `right` is the screen's last
/// column because a line is sized only with `DECLRMM` reset.
pub(crate) fn fence_of(r: LineRendition, right: Col, cols: Cols) -> Col {
    match r.is_double() {
        false => right,
        true => Col::at_index(r.end_of_line(cols).index() - 1).min(right),
    }
}

/// [`Paint::hooks`]: whether a write of text has an image, a watch or an
/// armed margin bell to tell.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum WriteHooks {
    Off,
    On,
}

impl WriteHooks {
    fn of(
        images: &ImageStore<'_>,
        watched: crate::term::Watched,
        armed: Option<crate::margin::Line>,
    ) -> WriteHooks {
        match images.any_placed() || watched == crate::term::Watched::Rows || armed.is_some() {
            true => WriteHooks::On,
            false => WriteHooks::Off,
        }
    }
}
