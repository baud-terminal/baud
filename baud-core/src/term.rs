//! The terminal's state, and the one borrow the parser holds.

use crate::graph::Node;
use crate::osc::StrKind;
use crate::storage::{LINK_MAX, MAX_COLS, MAX_ROWS, Storage};
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
    /// **The four slots and which one `GL` invokes, not the resolved set.**
    /// `[[source: vt510-decsc]]` lists "character sets G0-G3" among what DECSC
    /// saves, and a saved *resolved* set restores into slots that no longer
    /// agree with it -- the next `SI` would recompute from `g[gl]` and throw
    /// the restore away.
    pub(crate) g: [Charset; 4],
    pub(crate) gl: GSlot,
    /// DECAWM, because `[[source: vt510-decsc]]` lists "Wrap flag (autowrap or
    /// no autowrap)" among the seven things DECSC saves.
    pub(crate) autowrap: ModeChange,
    /// DECOM, likewise: "State of origin mode (DECOM)".
    ///
    /// **Restoring it is not cosmetic.** Origin mode decides where home *is*,
    /// so an application that sets it, saves, resets it and restores must get
    /// its own coordinate system back -- and until this was read off the
    /// manual rather than assumed, neither core restored either flag.
    pub(crate) origin: ModeChange,
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
    /// DECKPAM/DECKPNM, `ESC =` and `ESC >`.
    ///
    /// **Not a mode bit.** It has no `SM`/`RM` wire number to live under, and
    /// it is input-side: it decides what the keypad sends, which is why the
    /// core's part is to record it and expose it.
    /// **The four G slots, and which one `GL` reads through.**
    ///
    /// Here rather than beside `charset` on the hot path, and that split is
    /// the same one `style`/`style_raw` already makes: designation and the
    /// locking shifts are control sequences, and what the per-character loop
    /// needs is the *resolved* set. `Paint::charset` stays one byte and
    /// `Paint::invoke` re-derives it whenever a slot or `gl` moves, so
    /// `put_ascii_run` reads exactly what it read before.
    ///
    /// `baud-core` had only the resolved byte, which made designation and
    /// invocation the same act: `ESC ) 0` changed what G0 rendered and
    /// `SO`/`SI` did nothing at all.
    pub(crate) g: [Charset; 4],
    pub(crate) gl: GSlot,
    /// **Mouse reporting, all of it in `Cold`.** Every field here is written
    /// by a control sequence and read by an embedder calling
    /// [`Term::mouse_report`] -- never by the per-byte loop -- so it costs the
    /// ingest path nothing at all.
    pub(crate) mouse: MouseTracking,
    /// `?1006`. Resetting it returns to X10, which is the protocol's default
    /// rather than "off": there is no off.
    pub(crate) mouse_encoding: MouseEncoding,
    /// `?1001`. Recorded and exposed, because a core with no selection model
    /// has no other way to act on it -- and answering `DECRQM` truthfully
    /// requires knowing.
    pub(crate) highlight: HighlightTracking,
    pub(crate) highlight_region: HighlightRegion,
    /// Which control string is being accumulated, and how much of it.
    ///
    /// In `Cold` because a control string is not the write path: `Cold` is
    /// behind a pointer the per-byte loop never dereferences, so the ingest
    /// path pays nothing for either. They are state that outlives a `feed`,
    /// because a string spans reads.
    pub(crate) str_kind: StrKind,
    pub(crate) str_len: ByteLen,
    /// ANSI or VT52, and the row byte a half-finished `ESC Y` is holding.
    ///
    /// **Both land in existing padding**: `size_of::<Cold>()` does not change.
    /// And both are in `Cold` rather than on `Paint` because the per-byte loop
    /// must never read them -- the protocol is in the *node*, so the mode is
    /// only ever consulted when something asks which one is live.
    pub(crate) terminal: TerminalMode,
    pub(crate) vt52_row: Vt52Coord,
    /// `DECSCUSR`. Which cursor a renderer should draw, and whether it
    /// blinks. Recorded and exposed for the reason every renderer-side mode
    /// here is: an embedder cannot honour it without being told, and a shape
    /// parsed and dropped is worse than one not implemented.
    pub(crate) cursor_style: CursorStyle,
    pub(crate) keypad: KeypadMode,
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
            g: [Charset::Ascii; 4],
            gl: GSlot::G0,
            mouse: MouseTracking::Off,
            mouse_encoding: MouseEncoding::X10,
            highlight: HighlightTracking::Off,
            highlight_region: HighlightRegion::default(),
            terminal: TerminalMode::Ansi,
            vt52_row: Vt52Coord::ZERO,
            str_kind: StrKind::Other,
            str_len: ByteLen::ZERO,
            cursor_style: CursorStyle::BlinkingBlock,
            keypad: KeypadMode::Numeric,
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
    /// The UAX #29 cluster machine, carried between reads.
    ///
    /// **Beside `charset` on the hot path rather than in `Cold`**, because it
    /// is read once per non-ASCII character where `Cold` is read once per
    /// control sequence. It is the whole state: one table id and four
    /// lookback fields, which `CLUSTER_NEXT` collapses to a single indexed
    /// load.
    pub(crate) cluster: ClusterBreak,
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
    /// How many bytes of `store.reply` are pending.
    pub(crate) reply_len: ByteLen,
    /// Replies dropped because the buffer was full. Counted rather than
    /// silently lost: a truncated reply is a wrong answer, and this file's
    /// rule is that a wrong answer is worse than silence.
    pub reply_overflows: usize,
    /// How much of the title and icon buffers is live, and the open link.
    pub(crate) title_len: ByteLen,
    pub(crate) icon_len: ByteLen,
    pub(crate) link: LinkId,
    /// **Refused, and counted.** A control-string body clipped at `OSC_MAX`, a
    /// string recognised and not acted on, and the two halves of `OSC 52`.
    /// Silence is the right answer to a clipboard read -- any reply, empty
    /// included, tells a remote program the feature works and it will keep
    /// asking, and what it would read back is whatever the user last copied.
    /// Being unable to tell that one arrived is a different thing, and is what
    /// these turn from a bare `=> {}` into a number.
    pub vt52_unsupported: usize,
    /// How many `BEL`s arrived.
    ///
    /// A count rather than a callback, because the core cannot make a sound
    /// and an embedder can -- and a **field** rather than an accessor, which
    /// is the convention its neighbours already set: `reply_overflows` and
    /// `string_truncations` are read the same way. The audit is what settled
    /// it, refusing a `-> usize` return where a public field is not a
    /// signature at all.
    pub bells: usize,
    /// Character-set designations this core does not name. **Counted rather
    /// than recorded**: a name means the designation was understood, so
    /// storing an unlisted final as a set would make it look designated.
    pub charset_unsupported: usize,
    pub string_truncations: usize,
    pub strings_ignored: usize,
    pub clipboard_reads_refused: usize,
    pub clipboard_writes_dropped: usize,
    /// Zero-width codepoints received and **not stored**: a continuation with
    /// no base on the line, a cluster past `ClusterSlot::CAPACITY`, or a
    /// [`ClusterStore`] with no free slot for it.
    ///
    /// **Counted, because the glyph differential structurally cannot see the
    /// difference**: it compares cell content and a continuation writes no
    /// cell, so a core that stores them and one that discards them are the
    /// same to that gate. Same shape as `clipboard_reads_refused` -- baud
    /// refuses, and says how often.
    ///
    /// It is no longer "every mark": most now land in [`ClusterStore`]. What
    /// remains is that store's bound, stated as a number rather than implied.
    pub marks_dropped: usize,
    /// Mouse reports refused because the position had no encoding.
    ///
    /// X10 caps each ordinate at 223 against a 2048-column grid, so a report
    /// from column 400 has no byte -- and the one it would produce names a
    /// *different cell* rather than a truncated one, which is the wrong-shaped
    /// answer `docs/vt-coverage.md` rules out. Refused and counted, like
    /// `clipboard_reads_refused`.
    pub mouse_unencodable: usize,
    pub fast_try: usize,
    #[cfg(feature = "diag")]
    pub fast_hit: usize,
    #[cfg(all(feature = "diag", feature = "sgr-table"))]
    pub sgr_hit: usize,
}

/// Outcome of a [`Term::resize`].
///
/// `#[must_use]` because the two answers are different facts about the
/// terminal, and a caller that drops the value renders a grid that disagrees
/// with the application about its own size.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[must_use]
pub enum Resized {
    Ok,
    /// The window is larger than this core can hold. `cols`/`rows` are what it
    /// was clamped to.
    ///
    /// **Two different ceilings, and the caller can tell them apart from the
    /// numbers.** `MAX_COLS`/`MAX_ROWS` are the core's, and are fixed; the row
    /// capacity is the *caller's*, because the grid is caller-provided storage
    /// -- an embedder that wants to grow provisions
    /// [`Storage::row_capacity`] rows up front.
    Truncated {
        cols: Cols,
        rows: Rows,
    },
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

    /// How many rows the caller provisioned. See [`Storage::row_capacity`].
    ///
    /// Exposed on `Term` because that is where an embedder asks: it holds the
    /// `Storage` and cannot reach past it, and this is the number that decides
    /// whether a [`Term::resize`] can grow.
    pub fn row_capacity(&self) -> Rows {
        self.store.row_capacity()
    }

    /// Take a new window size.
    ///
    /// **Geometry only: the allocation is the caller's and is untouched.** The
    /// grid is caller-provided, so this can shrink freely and can only grow
    /// into rows somebody provisioned -- [`Storage::row_capacity`] is that
    /// number, and asking for more returns `Truncated` rather than clamping in
    /// silence. Columns are free to `MAX_COLS`, because `vp` is strided for the
    /// maximum by construction.
    ///
    /// **The content does not survive.** Reflow is a renderer's problem and
    /// this core has no renderer; a grid whose rows are a different width is a
    /// different grid, and pretending otherwise means deciding where a wrapped
    /// line breaks now -- a decision with no right answer at this layer.
    ///
    /// Four things have to happen in an order that is not free.
    ///
    /// * **Both rotations go to `NONE` before anything blanks.** `Term.rot`
    ///   and the parked `AltBuf.rot` are indexes into a row count that is
    ///   about to change, and `viewport` un-rotates with `rotate_left_by` --
    ///   which **panics** when the rotation is larger than the slice. Scroll a
    ///   50-row screen, shrink to 10, ask for the viewport: `mid <= len`.
    /// * **DECSTBM is reset.** xterm does, and a stale bottom margin points
    ///   off the grid.
    /// * **The parked primary is blanked whole**, not to the new `live()`.
    ///   It is laid out at the *old* stride, so any partial blank keyed on the
    ///   new geometry leaves reachable garbage that `restore_from` would copy
    ///   back on the next `?1049 l`. Every full-screen program enters the
    ///   alternate screen, so "resized while parked" is the common case.
    /// * The cursor goes home, because there is no content to be relative to.
    ///
    /// **Two things core2 does here and this does not, deliberately.** It
    /// resets side margins, which this core does not record at all -- porting
    /// the call would be a line that does nothing. And it rebuilds the tab
    /// table, which is `MAX_COLS` long and stamped every eighth column at
    /// construction, so no width change can invalidate it.
    pub fn resize(&mut self, ws: &WinSize) -> Resized {
        let (want_cols, want_rows) = ws.geometry();
        let cap = self.store.row_capacity();
        let cols = want_cols.clamp(MAX_COLS);
        let rows = want_rows.clamp(MAX_ROWS).clamp(cap.index());
        let over = cols.index() != want_cols.index() || rows.index() != want_rows.index();

        self.rows = rows;
        self.cols = cols;
        self.stride = Cols::from_stride(cols.index());
        self.cy = Row::ZERO;
        self.cx = Col::ZERO;
        self.cold.top = Row::ZERO;
        self.cold.bot = Row::from_index(rows.index().saturating_sub(1));
        // Before the blanks, and both of them.
        self.rot = RowRot::NONE;
        self.store.alt.rot = RowRot::NONE;

        let mut v = View::new(
            self.store.vp,
            self.store.used,
            self.stride,
            cols,
            rows,
            self.rot,
        );
        for y in 0..rows.index() {
            v.fill(
                Row::from_index(y),
                Col::ZERO,
                Col::at_index(cols.index()),
                PackedCell::EMPTY,
            );
        }
        // Whole, at whatever stride it was written in.
        for c in self.store.alt.cells.iter_mut() {
            *c = PackedCell::EMPTY;
        }
        for u in self.store.alt.used.iter_mut() {
            *u = RowExtent::ZERO;
        }
        match over {
            true => Resized::Truncated { cols, rows },
            false => Resized::Ok,
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
            cluster: ClusterBreak::new(),
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
            reply_len: ByteLen::ZERO,
            reply_overflows: 0,
            title_len: ByteLen::ZERO,
            icon_len: ByteLen::ZERO,
            link: LinkId::NONE,
            vt52_unsupported: 0,
            bells: 0,
            charset_unsupported: 0,
            string_truncations: 0,
            strings_ignored: 0,
            clipboard_reads_refused: 0,
            clipboard_writes_dropped: 0,
            marks_dropped: 0,
            mouse_unencodable: 0,
            fast_try: 0,
            #[cfg(feature = "diag")]
            fast_hit: 0,
            #[cfg(all(feature = "diag", feature = "sgr-table"))]
            sgr_hit: 0,
        }
    }

    /// One row of the grid, resolved through the rotation.
    ///
    /// **Takes `&self` where `viewport` takes `&mut self`.** `viewport`
    /// un-rotates the whole grid in place so a renderer gets one contiguous
    /// slice; a single row is already contiguous wherever it sits, so `phys`
    /// answers it with arithmetic and nothing moves.
    pub fn row(&self, y: Row) -> &[PackedCell] {
        View::new_ref(self.store.vp, self.stride, self.cols, self.rows, self.rot).row_slice(y)
    }

    /// The live geometry, which a resize can change.
    pub fn geometry(&self) -> (Cols, Rows) {
        (self.cols, self.rows)
    }

    /// The character set in force -- the resolved one, `g[gl]`.
    ///
    /// Designation and invocation are different things: `ESC ) 0` loads G1 and
    /// `SO` selects it. This is what the write path reads, which is neither
    /// slot nor shift but their resolution.
    pub fn charset(&self) -> Charset {
        self.charset
    }

    /// `DECSCUSR`. The cursor shape an application asked for.
    pub fn cursor_style(&self) -> CursorStyle {
        self.cold.cursor_style
    }

    /// `?25` DECTCEM. Whether the cursor should be drawn.
    pub fn cursor_visible(&self) -> bool {
        self.cold.modes.is_set(ModeBit::CursorVisible)
    }

    /// `?4` DECSCLM. Smooth scroll -- renderer policy, recorded and exposed.
    pub fn smooth_scroll(&self) -> bool {
        self.cold.modes.is_set(ModeBit::SmoothScroll)
    }

    /// `?8` DECARM. Auto-repeat -- keyboard policy, recorded and exposed.
    pub fn auto_repeat(&self) -> bool {
        self.cold.modes.is_set(ModeBit::AutoRepeat)
    }

    /// `?3` DECCOLM. Whether the application asked for 132 columns.
    ///
    /// **The width is not changed here.** This core's geometry is the
    /// embedder's -- it comes from a `WinSize` and moves through
    /// [`Term::resize`] -- so DECCOLM records the request and performs the
    /// side effects the specification attaches to it: the region resets, the
    /// cursor homes and the screen clears.
    pub fn column_mode(&self) -> bool {
        self.cold.modes.is_set(ModeBit::Column132)
    }

    /// `?5` DECSCNM, reverse video. Recorded and exposed, not drawn: which
    /// pixels invert is the renderer's.
    pub fn reverse_video(&self) -> bool {
        self.cold.modes.is_set(ModeBit::ReverseVideo)
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

    /// Where a renderer should draw the cursor.
    ///
    /// **Clamped to the last column, and that is the contract rather than a
    /// safety net.** After a character lands in the last column the cursor is
    /// in *pending wrap*: `cx` is `cols`, one past the margin, so the next
    /// graphic character wraps to the next row instead of overwriting what is
    /// there. That is the right internal state and it is not a position --
    /// there is no column `cols` to draw in, and a renderer indexing a row
    /// with it reads past the end of the row.
    ///
    /// So the accessor answers the question a renderer is asking. `baud-core2`
    /// takes `min(cols - 1)` here for the same reason, and
    /// `both_cores_put_the_cursor_in_the_same_place` is what found that this
    /// core did not: on a 200-column screen it returned **column 200** after
    /// `HT` ran to the right margin. Both cores' *internal* `cx` was 200 and
    /// they rendered identically, so neither pinned gate could see it -- the
    /// glyph gate reads content and the rendition gate reads the style word,
    /// and the cursor is in neither.
    pub fn cursor(&self) -> (Row, Col) {
        let last = Col::at_index(self.cols.index() - 1);
        (self.cy, Col::at_index(self.cx.index().min(last.index())))
    }
}

impl<'b> Term<'b> {
    /// Bytes the terminal owes the host, e.g. the answer to a `DA` or `DSR`.
    ///
    /// Write these back to wherever the input came from, then call
    /// [`Term::clear_reply`]. Empty when there is nothing to say.
    pub fn reply(&self) -> &[u8] {
        &self.store.reply[..self.reply_len.index()]
    }

    /// Drop the pending reply after sending it.
    pub fn clear_reply(&mut self) {
        self.reply_len = ByteLen::ZERO;
    }

    /// The codepoints attached to the cell at `(r, c)`, after its base.
    ///
    /// **Resolved through the grid, not from a position key.** The cell holds
    /// the cluster index, so this is one load and a bounds check, and it is
    /// correct after a scroll, after `viewport` and on either screen -- the
    /// index moved when the cell moved. `baud-core2` answers the same question
    /// from a table keyed on surface, physical row, column and generation, and
    /// an adversarial review reproduced what that costs: one `viewport()` call
    /// and the answer is empty, then a stranger's.
    ///
    /// Empty when the cell has no continuations, and empty rather than wrong
    /// when the store refused the cluster -- the count is
    /// [`Term::marks_dropped`].
    pub fn marks(&self, r: Row, c: Col) -> impl Iterator<Item = u32> + '_ {
        let cell =
            View::new_ref(self.store.vp, self.stride, self.cols, self.rows, self.rot).get(r, c);
        self.store
            .clusters
            .get(cell.flags.cluster(), cell.content)
            .into_iter()
            .flat_map(|s| s.marks())
    }

    /// What tracking the application asked for: `?1000`, `?1002`, `?1003`.
    pub fn mouse_tracking(&self) -> MouseTracking {
        self.cold.mouse
    }

    /// Whether `?1001` highlight tracking is on.
    ///
    /// An embedder with a selection model can implement it; this core records
    /// the request and does nothing else. `[[source: ctlseqs]]`'s warning that
    /// it "requires a cooperating program, else xterm will hang" describes an
    /// event loop blocking on a reply, and this core has none -- so whether to
    /// wait is a decision it does not make.
    pub fn highlight_tracking(&self) -> HighlightTracking {
        self.cold.highlight
    }

    /// The region from the last `CSI Ps ; Ps ; Ps ; Ps ; Ps T`.
    pub fn highlight_region(&self) -> HighlightRegion {
        self.cold.highlight_region
    }

    /// Queue a mouse report, if the application asked for this kind of event.
    ///
    /// Returns whether anything was queued, so an embedder can tell "the
    /// application does not want this" from "it was sent".
    ///
    /// **Two gates, and both are refusals rather than omissions.** The
    /// tracking level decides which *kinds* are reported: motion is dropped
    /// under `?1000`, and motion with no button held under anything below
    /// `?1003`, because sending it anyway floods an application that asked
    /// only for clicks. And the *position* has to be encodable -- X10 caps
    /// each ordinate at 223 against a 2048-column grid, so a report from
    /// column 400 has no byte, and the one it would produce names a
    /// **different cell** rather than a truncated one. Those are refused and
    /// counted in [`Term::mouse_unencodable`]; everything that fits is sent,
    /// which is every position on a terminal anyone actually runs.
    pub fn mouse_report(&mut self, e: MouseEvent) -> bool {
        let wanted = match (self.cold.mouse, e.kind) {
            (MouseTracking::Off, _) => false,
            (MouseTracking::Normal, MouseKind::Motion) => false,
            (MouseTracking::ButtonEvent, MouseKind::Motion) => e.button != MouseButton::None,
            _ => true,
        };
        if !wanted {
            return false;
        }
        if let Some(max) = self.cold.mouse_encoding.max_ordinate() {
            let (r, c) = (e.row.one_based().get(), e.col.one_based().get());
            if r > max || c > max {
                self.mouse_unencodable += 1;
                return false;
            }
            self.queue(Report::MouseX10(e));
            return true;
        }
        self.queue(Report::MouseSgr(e));
        true
    }

    /// Put a report in the reply buffer.
    ///
    /// The same rule `Paint::want_reply` follows: a truncated reply is a wrong
    /// answer, so it is dropped whole and counted rather than half-sent.
    fn queue(&mut self, r: Report) {
        let bytes = r.encode(C1Transmit::SevenBit);
        let b = bytes.as_bytes();
        let at = self.reply_len.index();
        match at + b.len() <= self.store.reply.len() {
            true => {
                self.store.reply[at..at + b.len()].copy_from_slice(b);
                self.reply_len = ByteLen::from_index(at + b.len());
            }
            false => self.reply_overflows += 1,
        }
    }

    /// ANSI or VT52.
    ///
    /// **The protocol lives in the parser node and this is the read-out.** An
    /// embedder needs it because the two answer keys differently -- the VT52
    /// keyboard sends `ESC A` where ANSI sends `CSI A` -- and `Term::key`
    /// reads the same field for the same reason.
    pub fn terminal_mode(&self) -> TerminalMode {
        self.cold.terminal
    }

    /// The window title, as the application set it. Bounded by `OSC_MAX`.
    pub fn title(&self) -> &[u8] {
        &self.store.strs.title[..self.title_len.index()]
    }

    /// The icon name. `OSC 1` sets this, and `OSC 0` sets both.
    pub fn icon_name(&self) -> &[u8] {
        &self.store.strs.icon[..self.icon_len.index()]
    }

    /// A palette override from `OSC 4`, if the application set one.
    ///
    /// `None` means "use your own colour": this core deliberately holds no
    /// default palette, because the mapping from index to pixels belongs to
    /// the renderer and a second copy here would be one to keep in step. It is
    /// also why a query for an entry nothing overrode gets no reply -- naming
    /// a colour the renderer does not use is a wrong-shaped answer.
    pub fn palette(&self, index: Chan) -> Option<(u8, u8, u8)> {
        self.store.strs.palette[index.index()].as_rgb()
    }

    /// The URI a cell's link index resolves to, if any.
    ///
    /// The index comes off the cell -- `CellFlags::link` -- rather than from a
    /// position, which is what makes it survive a scroll: the grid rotates and
    /// the cell carries its link with it. The table evicts on collision, so a
    /// live index can resolve to nothing; that renders as an ordinary cell,
    /// the same tolerable degradation an evicted style has.
    pub fn link_uri(&self, l: LinkId) -> Option<&[u8]> {
        if l.is_none() {
            return None;
        }
        let n = self.store.strs.link_len[l.index()].index();
        (n > 0).then(|| &self.store.strs.links[l.index() * LINK_MAX..][..n])
    }

    /// Encode a key press into the reply buffer.
    ///
    /// Returns whether anything was written; `false` is "this key sends
    /// nothing", which is a real answer rather than a failure -- ctlseqs gives
    /// no modified form for `SS3 j`, and a codepoint above `U+00FF` has no
    /// ISO 8859-1 byte. Inventing a sequence for either would be answering
    /// with a shape no application asked for, which this core already refuses
    /// to do on the output side.
    ///
    /// **The encoding is `baud_domain::encode_key`, shared with
    /// `baud-core2`.** What is here is the part that belongs to a terminal
    /// rather than to the protocol: reading the modes off `Term` and putting
    /// the bytes where the replies already go. The alternative was porting 485
    /// lines and having two spellings of the input protocol, which is what a
    /// hand-written CSI recogniser beside the table cost this repo once.
    ///
    /// **One of the five inputs is a constant here, and it used to be two.**
    ///
    /// * **`C1Transmit::SevenBit`**, because this core does not implement
    ///   `S8C1T` -- the same constant `want_reply` already passes, so a key
    ///   and a `DA` cannot disagree about how wide a `CSI` is.
    /// * `terminal` was `TerminalMode::Ansi` on the grounds that "this core
    ///   has no VT52 sub-graph and no DECANM to leave ANSI with". It has both
    ///   now, so it reads the mode. Leaving the constant would have made every
    ///   arrow key send `CSI A` inside VT52, where the keyboard sends
    ///   `ESC A` -- a gap that opened the moment the *parser* gained a feature
    ///   the *encoder* had carried all along.
    pub fn key(&mut self, e: KeyEvent) -> bool {
        let s = KeyState {
            modes: self.cold.modes,
            keypad: self.cold.keypad,
            c1: C1Transmit::SevenBit,
            terminal: self.cold.terminal,
            encoding: self.encoding,
        };
        let Some(k) = encode_key(e, s) else {
            return false;
        };
        let b = k.as_bytes();
        let at = self.reply_len.index();
        // A truncated key is a wrong answer, so it is dropped whole and
        // counted rather than half-sent -- the same rule `want_reply` follows,
        // and for the same reason: an application reads the first byte as the
        // start of a sequence that never arrives.
        match at + b.len() <= self.store.reply.len() {
            true => {
                self.store.reply[at..at + b.len()].copy_from_slice(b);
                self.reply_len = ByteLen::from_index(at + b.len());
                true
            }
            false => {
                self.reply_overflows += 1;
                false
            }
        }
    }
}

impl Term<'_> {
    /// DECCKM `?1`. Whether the arrow keys send `SS3 A` rather than `CSI A`.
    ///
    /// **Exposed rather than acted on internally, and that is the acting.** An
    /// embedder cannot encode a key press without knowing this, and being told
    /// is the whole of the terminal's part -- the same contract
    /// `bracketed_paste` already has.
    pub fn cursor_keys_application(&self) -> bool {
        self.cold.modes.is_set(ModeBit::CursorKeys)
    }

    /// DECKPAM/DECKPNM. Whether the keypad sends `SS3`-prefixed sequences.
    pub fn keypad(&self) -> KeypadMode {
        self.cold.keypad
    }

    /// `?1036` metaSendsEscape. Whether Meta prefixes `ESC`.
    pub fn meta_sends_escape(&self) -> bool {
        self.cold.modes.is_set(ModeBit::MetaSendsEscape)
    }

    /// `?1039` altSendsEscape. Whether Alt prefixes `ESC`.
    pub fn alt_sends_escape(&self) -> bool {
        self.cold.modes.is_set(ModeBit::AltSendsEscape)
    }

    /// `?1034` eightBitInput. Whether Alt/Meta add 128 instead of prefixing.
    pub fn eight_bit_input(&self) -> bool {
        self.cold.modes.is_set(ModeBit::EightBitInput)
    }
}
