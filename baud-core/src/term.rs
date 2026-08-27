use crate::*;

/// Largest geometry the fixed viewport can hold.
///
/// Sized against real limits, not guessed: 8K (7680x4320) at a 4x8 px font is
/// 1920x540, which is the smallest font anyone can read on the largest display
/// sold. `TIOCSWINSZ` caps `ws_col`/`ws_row` at 65535, so this is a policy
/// choice, not a protocol one.
///
/// Costs nothing until touched. `Storage` is 13.9 MB *virtual*; measured RSS
/// after allocating it, initialising a 50x200 grid and feeding data was
/// **+0.3 MB total** — untouched pages are never faulted in. Throughput is
/// unchanged from the old 400x120 (203.5 vs 205.9 MB/s, ranges overlapping).
///
/// The previous values truncated 1440p at a small font *silently*, which is why
/// [`Term::resize`] now returns [`Resized`].
pub const MAX_COLS: usize = 2048;

pub const MAX_ROWS: usize = 640;

pub const SB: usize = 10_000;

pub const ARENA: usize = 1 << 20;

pub const MAXRUN: usize = 8;

/// Cells in one screen buffer.
///
/// Named because two buffers must be the same size -- switching screens is a
/// pointer swap and nothing rechecks the geometry -- and because `vp` and `alt`
/// having literally the same expression is the invariant, not a coincidence.
pub const GRID: usize = MAX_ROWS * MAX_COLS;

/// Which screen buffer is active.
///
/// `[[source: ctlseqs]]` calls them the Normal and Alternate Screen Buffers. A
/// `bool` at these call sites would read as `if self.alt`, which says nothing
/// about what the other branch is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Normal,
    Alternate,
}

/// Bytes of pending reply the core will hold for the caller.
///
/// Queries are rare and their answers are short -- the longest baud produces is
/// a CPR at 12 bytes -- but a stream can send many in one read, so this is sized
/// for a burst rather than for one. Overflow is counted, never silent.
pub const REPLY: usize = 256;

pub const NSTYLE: usize = 64;

/// Longest OSC payload the core keeps.
///
/// Every byte of an OSC is attacker-controlled -- a window title is whatever
/// the far end of an ssh session sent -- and a `no_std` core cannot grow a
/// buffer to meet it. Overlong payloads are **truncated, not rejected**: a
/// 4 KiB title should become a short title, not a dropped sequence, because
/// dropping it leaves the previous title showing and the user with no signal.
/// Sized well past any real title.
pub const OSC_MAX: usize = 512;

/// Distinct hyperlinks a screen can hold at once, and the longest URI kept.
///
/// **Bounded by construction, like the style table and for the same reason.**
/// `CLAUDE.md` records a growable style table as "the content-addressing
/// failure mode again": a structure keyed on attacker-supplied content that
/// grows without limit walks out of cache and then out of memory. A URI is
/// exactly that kind of key. `NLINK` slots evict on collision, so a page that
/// cycles through thousands of links costs the same as one with four.
///
/// 128 x 256 B is 32 KiB, which fits beside the style table rather than
/// competing with the grid.
pub const NLINK: usize = 128;

/// Combining marks attached to cells, and the most any one cell may carry.
///
/// **A cell has no room for these.** `PackedCell` is eight bytes and that
/// number is measured; `CellFlags` is fully allocated, two bits of flags and
/// fourteen of `LinkId`. So the marks live beside the grid rather than in it,
/// in a direct-mapped table keyed on the cell's position.
///
/// Bounded and evicting on collision, for the reason `NLINK` and `NSTYLE` are:
/// the key is attacker-supplied — a page can emit as many combining marks as it
/// likes — and a growable table under that pressure is the content-addressing
/// failure mode `CLAUDE.md` already refuses. A cell whose slot was taken loses
/// its marks; it does not lose its base character, and nothing grows without
/// bound.
///
pub const NMARK: usize = 512;

pub const LINK_MAX: usize = 256;

pub const READ_CHUNK: usize = baud_domain::SLOT_BYTES;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpMode {
    Computed,
    Templated,
    Runs,
}

/// Outcome of a [`Term::resize`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[must_use]
pub enum Resized {
    Ok,
    /// The window is larger than the fixed viewport. The grid was clamped to
    /// the geometry given; the caller should surface this rather than render a
    /// terminal that disagrees with the application about its own size.
    Truncated {
        cols: Cols,
        rows: Rows,
    },
}

pub struct Term<'a> {
    pub(crate) s: &'a mut Storage,
    pub(crate) tail: ArenaTail,
    pub(crate) head: RingPos,
    pub(crate) cx: Col,
    pub(crate) cy: Row,
    pub(crate) cols: Cols,
    pub(crate) rows: Rows,
    pub(crate) st: State,
    pub(crate) params: [Option<Param>; ParamIdx::CAPACITY],
    /// How many parameter positions the current sequence has produced.
    ///
    /// **Not** how many are readable: it saturates one past
    /// [`ParamIdx::CAPACITY`], so "sixteen arrived" and "more than sixteen
    /// arrived" stay distinguishable. Read it through [`Term::nparams`], never
    /// directly -- `&self.params[..n]` with the raw value panics.
    pub(crate) nparam: ParamIdx,
    /// Bit `i` set when parameter `i` followed a colon rather than a semicolon.
    /// See `Params::is_sub` -- the colon form of direct colour carries an extra
    /// colour-space identifier, so the separator changes the meaning.
    pub(crate) psub: SubParams,
    /// Bytes of `s.osc` filled so far, and the lengths of the two strings.
    pub(crate) osc_len: ByteLen,
    /// OSC payloads that overflowed `OSC_MAX`.
    ///
    /// A counter rather than a flag, matching `reply_overflows`: truncation is
    /// the correct behaviour but it should never be *invisible*, and a count
    /// survives the next OSC where a flag would be cleared by it.
    pub osc_truncations: Stat,
    pub(crate) title_len: ByteLen,
    pub(crate) icon_len: ByteLen,
    /// Which terminator the current OSC arrived with. `[[source: ctlseqs]]`:
    /// when returning information xterm "uses the same terminator used in a
    /// query", so a reply must remember how the question ended.
    pub(crate) osc_term: StringTerm,
    /// The final byte of the DCS currently being collected.
    pub(crate) dcs_final: Option<FinalByte>,
    pub(crate) private: Marker,
    pub(crate) style: StyleId,
    pub(crate) style_raw: StyleWord,
    /// DECSCUSR. Not a rendition -- it describes the cursor, not the text.
    pub(crate) cursor_style: CursorStyle,
    /// Which buffer writes and reads go to.
    pub(crate) screen: Screen,
    /// The cursor DECSC-style state saved when switching *to* the alt screen.
    ///
    /// Separate from `saved`, which belongs to DECSC/DECRC: an application that
    /// saves the cursor, switches screens and restores must get its own saved
    /// cursor back, not the one the switch stashed.
    pub(crate) alt_saved: Option<(Row, Col, StyleId, StyleWord, CellFlags)>,
    /// DECKPAM/DECKPNM. Input-side, so the core records it and an embedder
    /// reads it -- the keypad sends different sequences in each mode, and
    /// without it an embedder has no way to know which.
    pub(crate) keypad: KeypadMode,
    /// ANSI or VT52. DECANM, `CSI ? 2 l` enters VT52.
    pub(crate) terminal_mode: TerminalMode,
    /// Which mouse events the application asked to hear about, `?1000/2/3`.
    pub(crate) mouse: MouseTracking,
    /// The negotiated encoding. Without one baud reports nothing -- see
    /// [`Report::MouseSgr`].
    pub(crate) mouse_encoding: MouseEncoding,
    /// `?1001`, and the region the application last gave with XTHIMOUSE.
    pub(crate) highlight: HighlightTracking,
    pub(crate) highlight_region: HighlightRegion,
    /// Mouse events dropped because no encoding was negotiated.
    ///
    /// Counted rather than silent: an application that enables tracking and
    /// gets nothing back should be diagnosable from the outside.
    pub mouse_unencodable: Stat,
    /// `OSC 52` clipboard reads, which baud refuses on purpose.
    ///
    /// **The silence is right and the invisibility was not.** Answering a
    /// clipboard query with anything, empty string included, tells a remote
    /// program the feature works and it will keep asking -- so no reply is the
    /// correct answer to the *application*. Being unable to tell afterwards
    /// that one was asked is a different thing, and it is the failure
    /// `docs/vt-coverage.md` refuses everywhere else. An embedder that wants to
    /// implement the clipboard needs to know the requests are arriving.
    pub clipboard_reads_refused: Stat,
    /// `OSC 52` clipboard writes, which baud drops: it has no clipboard, and
    /// the payload is not kept. Counted for the same reason.
    pub clipboard_writes_dropped: Stat,
    /// Sequences the parser recognised and did **not** act on.
    ///
    /// This is the `parsed` failure made countable. `docs/vt-coverage.md` calls
    /// a sequence that is accepted, does nothing, and gives the application no
    /// way to find out "worse than one that is not implemented" -- but until
    /// now nothing could observe it happening. `tests/conformance/` asserts
    /// this stays zero while replaying every output capability of the terminfo
    /// entries baud claims.
    pub unsupported: Stat,
    /// Operations baud **recognises** but with a parameter it does not
    /// implement: `CSI 1 h` is a real SM, and mode 1 is not one baud has.
    ///
    /// Separate from [`Term::unsupported`] because the two are different
    /// questions and conflating them broke both. "I do not know this sequence"
    /// is what the terminfo conformance suite asks; "I know it but not this
    /// parameter" is what an inventory against the specification asks. Folding
    /// the second into the first made rxvt's reset string --
    /// `CSI 1;3;4;5;6 l`, four modes baud does not implement -- look like a
    /// conformance failure, when what it really shows is baud declining four
    /// modes it never claimed.
    pub unsupported_param: Stat,
    /// Combining marks that could not be stored: no base character, or the
    /// cell already held [`MARKS_PER_CELL`].
    pub marks_dropped: Stat,
    /// Marks displaced from a slot another cell was using. Bounded tables
    /// evict; counting it is how the bound stays visible.
    pub marks_evicted: Stat,
    /// Whether replies use 7-bit or 8-bit C1 introducers. Set by S7C1T/S8C1T.
    pub(crate) c1: C1Transmit,
    /// Locale policy for East Asian Ambiguous width. Not a mode an application
    /// can set: `[[source: uax11]]` § 5 makes it a property of the *reader's*
    /// context, so it belongs to the embedder.
    pub(crate) width: WidthTable,
    /// Where cluster boundaries fall. Carried across `put_cp` calls because
    /// five of `[[source: uax29]]`'s rules need more than the previous
    /// codepoint -- see [`ClusterBreak`].
    pub(crate) cluster: ClusterBreak,
    /// How incoming bytes become characters. `ESC % G` / `ESC % @` switch it.
    pub(crate) encoding: InputEncoding,
    /// The four designatable character-set slots, and which one is invoked
    /// into GL.
    ///
    /// `[[source: ctlseqs]]`: `ESC ( C` designates G0, `ESC ) C` G1, `ESC * C`
    /// G2, `ESC + C` G3; SO invokes G1 into GL and SI invokes G0.
    pub(crate) g: [Charset; 4],
    pub(crate) gl: GSlot,
    /// The hyperlink `OSC 8` opened, if one is open.
    pub(crate) link: LinkId,
    /// DECSCA protection alone, before the charset is folded in.
    ///
    /// Kept apart from `flags` so that changing either input recomputes the
    /// combination in one place -- `flags` is read on the hot path and must
    /// stay a single load.
    pub(crate) protect: CellFlags,
    /// DECSCA, and anything else that is per-cell state rather than rendition.
    ///
    /// Stamped onto every character written, so protection survives style
    /// eviction -- `tmpl_key` is a direct-mapped cache with one slot per
    /// bucket, and reading protection back out of it would answer with
    /// whichever style displaced the original.
    pub(crate) flags: CellFlags,
    pub(crate) saved: Option<(Row, Col, StyleId, StyleWord, CellFlags)>,
    /// DECSTBM. Inclusive, and full-screen until `CSI r` says otherwise.
    pub(crate) scroll_top: Row,
    pub(crate) scroll_bot: Row,
    /// DECSLRM. Inclusive, and full-width until `CSI Pl ; Pr s` says otherwise
    /// -- and only then if DECLRMM is set.
    pub(crate) left_margin: Col,
    pub(crate) right_margin: Col,
    /// A bitset rather than a struct of bools -- see [`Modes`].
    pub(crate) modes: Modes,
    pub(crate) mode: OpMode,
    /// Bytes handed to `feed`, and cells and runs written because of them.
    ///
    /// Rule #1 is write amplification <= 1 byte written per byte ingested, and
    /// it is the first thing `README.md` says every result here is predicted
    /// by. It had no measurement: the old instrumentation printed
    /// `inf x amplification` from counters nothing fed and was removed rather
    /// than repaired, so the number the architecture is justified by has been
    /// unverifiable since.
    ///
    /// Counted in bulk -- one `add` per run, not per cell -- because the
    /// per-cell loop is the one a runtime flag cost 5% of.
    pub bytes_in: Stat,
    /// Cells written to the grid. Multiply by `size_of::<PackedCell>()`.
    pub cells_written: Stat,
    /// Of those, the ones written to *blank* rather than to carry text.
    ///
    /// Separated because the two answer different questions. Materialising a
    /// character is a floor -- one ASCII byte becomes an eight-byte
    /// `PackedCell`, so a cell mode cannot be under 8x and rule #1 was never
    /// about that. Blanking is not a floor: it is what scrolling costs, and on
    /// a streaming payload it turns out to be *all* of the cell traffic.
    pub cells_blanked: Stat,
    /// Runs stored in the span table. Multiply by `size_of::<Run>()`.
    ///
    /// Only `OpMode::Runs` writes these; the materialising modes write cells
    /// instead, so the two counters are alternatives rather than a sum.
    pub runs_written: Stat,
    pub promotions: Stat,
    pub tmpl_builds: Stat,
    pub tmpl_hits: Stat,
    pub maxrun_overflows: Stat,
    pub replacements: Stat,
    // why the fast CSI path declined, so the fallback rate is measurable
    pub fast_hits: Stat,
    pub bells: Stat,
    /// How much of `Storage::reply` is pending.
    pub(crate) reply_len: ByteLen,
    /// Replies dropped because the buffer was full. Counted so a caller that
    /// never drains can find out, instead of losing answers silently.
    pub reply_overflows: Stat,
    /// The intermediate byte of the `ESC` sequence being parsed.
    pub(crate) esc_inter: Option<StreamByte>,
    /// The **second** intermediate of an escape sequence.
    ///
    /// `[[source: williams]]`'s `escape_intermediate` collects intermediates,
    /// plural; baud kept only the last, so `ESC ( % 6` overwrote the `(` and
    /// the G-slot was lost before the final byte arrived. Every multi-byte
    /// charset final -- Portuguese, Greek, DEC Cyrillic and the rest -- was
    /// unreachable for that reason rather than by decision.
    pub(crate) esc_inter2: Option<StreamByte>,
    /// How far the primary grid is rotated. See [`RowRot`].
    pub(crate) rot: RowRot,
    /// The last graphic character written, for REP.
    ///
    /// `[[source: ctlseqs]]`: "Repeat the preceding graphic character Ps
    /// times". `None` when nothing has been printed yet, in which case REP has
    /// nothing to repeat and must do nothing rather than repeat a stale cell.
    pub(crate) last_cp: Option<(Codepoint, Width)>,
    /// The intermediate byte of the CSI being parsed, if any. `CSI ! p` is
    /// DECSTR and `CSI p` is not the same operation.
    pub(crate) csi_inter: Option<StreamByte>,
    pub fb_not_csi: Stat,
    pub fb_boundary: Stat,
    pub fb_odd_param: Stat,
}

impl<'a> Term<'a> {
    /// How many parameters are readable, clamped to the array's capacity.
    ///
    /// `nparam` counts positions *seen* and stands one past capacity when a
    /// sequence sent more than [`ParamIdx::CAPACITY`] of them, so slicing
    /// `params` with the raw count panics. Every consumer goes through here.
    pub(crate) fn nparams(&self) -> usize {
        self.nparam.count_at_least_one().min(ParamIdx::CAPACITY)
    }

    pub fn new(s: &'a mut Storage, mode: OpMode) -> Self {
        let (cols, rows) = WinSize {
            ws_row: 50,
            ws_col: 200,
        }
        .geometry();
        let mut t = Term {
            s,
            tail: ArenaTail::ZERO,
            head: RingPos::ZERO,
            cx: Col::ZERO,
            cy: Row::ZERO,
            cols: cols.clamp(MAX_COLS),
            rows: rows.clamp(MAX_ROWS),
            st: State::Ground,
            params: [None; ParamIdx::CAPACITY],
            nparam: ParamIdx::FIRST,
            psub: SubParams::NONE,
            osc_len: ByteLen::ZERO,
            osc_truncations: Stat::ZERO,
            title_len: ByteLen::ZERO,
            icon_len: ByteLen::ZERO,
            osc_term: StringTerm::St,
            dcs_final: None,
            private: Marker::Standard,
            style: StyleId::DEFAULT,
            style_raw: StyleWord::DEFAULT,
            cursor_style: CursorStyle::BlinkingBlock,
            screen: Screen::Normal,
            alt_saved: None,
            keypad: KeypadMode::Numeric,
            terminal_mode: TerminalMode::Ansi,
            mouse: MouseTracking::Off,
            mouse_encoding: MouseEncoding::X10,
            highlight: HighlightTracking::Off,
            highlight_region: HighlightRegion::default(),
            mouse_unencodable: Stat::ZERO,
            clipboard_reads_refused: Stat::ZERO,
            clipboard_writes_dropped: Stat::ZERO,
            unsupported: Stat::ZERO,
            unsupported_param: Stat::ZERO,
            marks_dropped: Stat::ZERO,
            marks_evicted: Stat::ZERO,
            c1: C1Transmit::SevenBit,
            width: WidthTable::default(),
            cluster: ClusterBreak::new(),
            encoding: InputEncoding::Utf8,
            g: [Charset::Ascii; 4],
            gl: GSlot::G0,
            link: LinkId::NONE,
            protect: CellFlags::NONE,
            flags: CellFlags::NONE,
            saved: None,
            scroll_top: Row::ZERO,
            scroll_bot: Row::from_index(rows.clamp(MAX_ROWS).index() - 1),
            left_margin: Col::ZERO,
            right_margin: Col::at_index(cols.clamp(MAX_COLS).index() - 1),
            modes: Modes::default(),
            mode,
            bytes_in: Stat::ZERO,
            cells_written: Stat::ZERO,
            cells_blanked: Stat::ZERO,
            runs_written: Stat::ZERO,
            promotions: Stat::ZERO,
            tmpl_builds: Stat::ZERO,
            tmpl_hits: Stat::ZERO,
            maxrun_overflows: Stat::ZERO,
            replacements: Stat::ZERO,
            fast_hits: Stat::ZERO,
            bells: Stat::ZERO,
            reply_len: ByteLen::ZERO,
            reply_overflows: Stat::ZERO,
            csi_inter: None,
            last_cp: None,
            esc_inter: None,
            esc_inter2: None,
            rot: RowRot::NONE,
            fb_not_csi: Stat::ZERO,
            fb_boundary: Stat::ZERO,
            fb_odd_param: Stat::ZERO,
        };
        t.reset_tabs();
        t.clear_screen();
        t
    }

    /// BEL characters seen. There is no UI here to ring one.
    pub fn bells(&self) -> u64 {
        self.bells.count()
    }

    /// Bytes written to the grid per byte ingested.
    ///
    /// Rule #1 is that this stays at or below 1. It is the first thing
    /// `README.md` says every result here is predicted by, and it has had no
    /// measurement since the old counters were removed for printing
    /// `inf x amplification` -- so the quantity the architecture is justified
    /// by has been an assertion rather than a reading.
    ///
    /// Cells and runs are alternatives, not a sum: `OpMode::Runs` stores spans
    /// and the materialising modes store cells, so on any one run only one of
    /// the two is non-zero. Both are included so the caller does not have to
    /// know which mode it asked for.
    ///
    /// Zero bytes in gives zero rather than a division by it -- an empty run
    /// has no amplification, and `inf` is what the last version printed.
    /// Bytes of *span table* written per byte ingested.
    ///
    /// **This is the one rule #1 is about.** A run costs `size_of::<Run>()`
    /// however many bytes it covers, so it amortises: on a streaming payload
    /// it measures about 0.08x. A cell mode cannot compete and is not trying
    /// to -- materialising one ASCII byte into an eight-byte `PackedCell` is a
    /// floor of 8x, which is the cost of having a grid rather than a log.
    pub fn span_amplification(&self) -> f64 {
        self.per_byte(self.runs_written.count() * core::mem::size_of::<Run>() as u64)
    }

    /// Bytes of *grid* written per byte ingested, and the share of it that is
    /// blanking rather than text.
    ///
    /// The second number is the interesting one. Scrolling blanks the incoming
    /// row, which costs `cols` cells every time the screen moves up, and on a
    /// streaming payload that is the entire cell budget -- the text itself
    /// goes into spans. Blanking is therefore the thing to attack, not the
    /// storing.
    pub fn cell_amplification(&self) -> (f64, f64) {
        let w = core::mem::size_of::<PackedCell>() as u64;
        (
            self.per_byte(self.cells_written.count() * w),
            self.per_byte(self.cells_blanked.count() * w),
        )
    }

    /// Zero bytes in gives zero rather than dividing by it. `inf` is what the
    /// instrumentation this replaces printed for several sessions.
    fn per_byte(&self, written: u64) -> f64 {
        match self.bytes_in.count() {
            0 => 0.0,
            n => written as f64 / n as f64,
        }
    }

    pub fn fast_hits(&self) -> u64 {
        self.fast_hits.count()
    }

    /// Escapes that fell back because they were not `ESC [`.
    pub fn fb_not_csi(&self) -> u64 {
        self.fb_not_csi.count()
    }

    /// Escapes that fell back because the sequence crossed the read boundary.
    pub fn fb_boundary(&self) -> u64 {
        self.fb_boundary.count()
    }

    /// Resize. Changes geometry only; the allocation is untouched.
    ///
    /// Returns [`Resized::Truncated`] when the requested geometry exceeds
    /// [`MAX_COLS`]/[`MAX_ROWS`]. Silently clamping was the old behaviour and it
    /// produces a terminal that renders a smaller grid than the application
    /// believes it has — the cursor drifts and nothing says why.
    #[must_use = "Resized::Truncated means the geometry exceeded MAX_COLS/MAX_ROWS; ignoring it is the silent-clamp bug this return value replaced"]
    pub fn resize(&mut self, ws: &WinSize) -> Resized {
        let (c, r) = ws.geometry();
        let over = c.index() > MAX_COLS || r.index() > MAX_ROWS;
        self.cols = c.clamp(MAX_COLS);
        self.rows = r.clamp(MAX_ROWS);
        self.cx = Col::ZERO;
        self.cy = Row::ZERO;
        // A scrolling region is expressed in rows that may no longer exist.
        // xterm resets DECSTBM to the full screen on resize; keeping a stale
        // bottom row would leave the region pointing off the grid.
        self.scroll_top = Row::ZERO;
        self.scroll_bot = Row::from_index(self.rows.index() - 1);
        self.reset_side_margins();
        // **Before `clear_screen`, and not optional.** `rot` is bounded by the
        // *old* row count, so shrinking the window can leave it at or past the
        // new one -- and `View::phys` is a single conditional subtract,
        // deliberately, because both operands are normally below `rows`. Past
        // that point it returns a row outside the grid, so `clear_screen`
        // blanks rows nobody can see and `viewport`'s `rotate_left` panics
        // outright: 41 rows of rotation against a ten-row slice.
        //
        // Scroll a screenful, then make the window smaller: that is a
        // full-screen application and a user dragging a corner. The grid is
        // cleared here anyway, so there is nothing for a rotation to mean.
        self.rot = RowRot::NONE;
        // **Both buffers, not just the one on screen.**
        //
        // Discarding the rotation is only sound if the buffer it described is
        // cleared too. `clear_screen` clears whichever screen is current, so
        // resizing while on the alternate screen left the *primary* holding
        // rotated content with `rot` back at zero -- every row displaced, and
        // only for applications that resize while in full-screen mode, which
        // is most of them.
        let showing = self.screen;
        for s in [Screen::Normal, Screen::Alternate] {
            self.screen = s;
            self.clear_screen();
        }
        self.screen = showing;
        if over {
            Resized::Truncated {
                cols: self.cols,
                rows: self.rows,
            }
        } else {
            Resized::Ok
        }
    }

    /// Where the cursor is, as the protocol understands it.
    ///
    /// Clamped to the last column, which matters in exactly one state. With
    /// autowrap on, `self.cx` sits one *past* the last column after a character
    /// lands there -- the deferred-wrap sentinel the next write consumes. That
    /// position is an implementation device, not a place the protocol has:
    /// `[[source: vt510-decawm]]` describes the wrap as happening to "graphic
    /// characters received when the cursor is at the right border of the page",
    /// so the cursor is *at* the border, not beyond it. A 200-column page has
    /// no column 201.
    ///
    /// This is the difference between an internal flag and an answer. CPR goes
    /// through here, and reporting a column that does not exist is the
    /// wrong-shaped reply that `docs/vt-coverage.md` rules out -- an
    /// application that asks where the cursor is and moves it back gets a
    /// one-column drift, silently, only on full lines. An embedder drawing the
    /// cursor would put it off-screen for the same reason.
    pub fn cursor(&self) -> (Row, Col) {
        let last = Col::at_index(self.cols.index() - 1);
        (self.cy, Col::at_index(self.cx.index().min(last.index())))
    }

    pub fn geometry(&self) -> (Cols, Rows) {
        (self.cols, self.rows)
    }

    pub fn stride(&self) -> Cols {
        Cols::from_stride(MAX_COLS)
    }

    /// The whole materialised region, stride included. Callers wanting a single
    /// row should use [`Term::row`], which is bounds-correct by construction.
    /// The visible grid, as one contiguous slice.
    ///
    /// **Takes `&mut self`, and that is the whole trick.** A full-screen scroll
    /// rotates an index rather than moving cells, so ingest never pays for it;
    /// the rotation has to be undone exactly once before the cells can be
    /// handed out as a slice, and that happens here. A renderer calls this once
    /// a frame, where the old code copied the grid once a *scroll* -- which on
    /// a streaming payload is every screenful, and cost 45% of cell ingest.
    ///
    /// `rotate_left` is in-place and allocation-free, which `no_std` requires.
    pub fn viewport(&mut self) -> &[PackedCell] {
        if self.screen == Screen::Normal && self.rot != RowRot::NONE {
            let n = self.rot.index() * self.stride().index();
            let len = self.rows.index() * self.stride().index();
            self.s.vp[..len].rotate_left(n);
            self.rot = RowRot::NONE;
        }
        let b = if self.screen == Screen::Alternate {
            &self.s.alt
        } else {
            &self.s.vp
        };
        RowBlock::leading(self.rows, self.stride()).of(b)
    }

    /// One row of the grid.
    ///
    /// **`y` must be below `geometry().1`.** The buffer is `MAX_ROWS` deep and
    /// the grid is `rows` deep, so an index between them is allocated memory
    /// that is not part of the screen -- it returns cells rather than an error,
    /// which is the wrong-shape failure this project rules out for replies.
    /// `viewport` is the bounded accessor and does not have this edge.
    ///
    /// Left as a precondition rather than an `Option` because that is a
    /// published signature; `row_past_the_grid_is_a_documented_sharp_edge`
    /// pins the current behaviour so a change to it is deliberate. Use
    /// [`Term::get_row`] where the index is not already known to be in range.
    pub fn row(&self, y: Row) -> &[PackedCell] {
        let b = if self.screen == Screen::Alternate {
            &self.s.alt
        } else {
            &self.s.vp
        };
        let rot = if self.screen == Screen::Alternate {
            RowRot::NONE
        } else {
            self.rot
        };
        View::new_ref(b, self.stride(), self.cols, self.rows, rot).row_slice(y)
    }

    /// One row of the grid, or `None` if `y` is not on it.
    ///
    /// The `get`/index pair `slice` already trains everyone on: [`Term::row`]
    /// takes the index on trust, this one checks it. Worth having because the
    /// unchecked form does not fail loudly -- the buffer is `MAX_ROWS` deep and
    /// the grid is `rows` deep, so an index between them is allocated memory
    /// that is not on screen and comes back looking like cells.
    ///
    /// An embedder that caches a row count across a resize is the case this
    /// exists for, and it is not hypothetical: a stale bound is what turned a
    /// shrink into a panic in `viewport` and an out-of-grid cursor in DECRC.
    pub fn get_row(&self, y: Row) -> Option<&[PackedCell]> {
        match y.index() < self.rows.index() {
            true => Some(self.row(y)),
            false => None,
        }
    }

    /// Is the alternate screen showing? `CSI ? 1049`.
    pub fn screen(&self) -> Screen {
        self.screen
    }

    #[inline(always)]
    /// Hand out the next slot for a [`baud_domain::Source`] to fill.
    ///
    /// A [`Slot`], not a `&mut [u8]`: the length is [`READ_CHUNK`] rather than
    /// whatever the caller passes, and reporting how much arrived goes through
    /// [`Filled`], which cannot exceed the slot it came from.
    pub fn read_slot(&mut self) -> Slot<'_> {
        self.tail = self.tail.next_slot(ARENA);
        let base = self.tail.index();
        let pos = self.tail.pos();
        Slot::new(&mut self.s.arena[base..base + READ_CHUNK], pos)
    }

    #[inline(always)]
    pub fn commit_read(&mut self, n: ByteLen) {
        self.tail = self.tail.advance(n);
    }
}
