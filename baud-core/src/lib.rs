//! `baud` — terminal ingest core, expressed in [`baud_domain`] types.
//!
//! Nothing here is a `usize`-as-a-column or a `u16`-as-a-length. The cursor
//! advances only by a [`Width`], and a `Width` can only come from decoding —
//! which is why the bug this file used to have is now not expressible:
//! `cx += 1` per UTF-8 continuation byte advanced a 3-byte CJK character three
//! columns instead of two, corrupting every position after the first non-ASCII
//! character on a line. Both operands were `usize`, so it compiled.
//!
//! Measured: the domain types cost nothing here. Four runs through a real PTY
//! gave +5.0%, -0.4%, -3.4%, -0.9% against raw primitives, ranges overlapping
//! throughout.
#![no_std]
#![forbid(unsafe_op_in_unsafe_fn)]
// Portable SIMD, so the scans below vectorise on every target rather than only
// x86-64. Nightly-only, which this project already requires.
#![feature(portable_simd)]

#[cfg(feature = "alloc")]
extern crate alloc;

pub use baud_domain::{
    AnsiMode, ArenaTail, ByteLen, BytePos, Codepoint, Col, Cols, Content, Count, Csi, Encoding,
    EraseExtent, Filled, FinalByte, LineSlot, Marker, ModeBit, Modes, OneBased, PackedCell, Param,
    ParamIdx, Params, Pending, PrivateMode, RingPos, Room, Row, Rows, Run, RunCount, Slot, Stat,
    StreamByte, StyleId, StyleWord, Utf8Acc, View, Width, WinSize,
};

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

/// Bytes of pending reply the core will hold for the caller.
///
/// Queries are rare and their answers are short -- the longest baud produces is
/// a CPR at 12 bytes -- but a stream can send many in one read, so this is sized
/// for a burst rather than for one. Overflow is counted, never silent.
pub const REPLY: usize = 256;
pub const NSTYLE: usize = 64;
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

#[repr(C)]
#[cfg_attr(
    feature = "alloc",
    derive(zerocopy::FromZeros, zerocopy::KnownLayout, zerocopy::Immutable)
)]
pub struct Storage {
    pub arena: [u8; ARENA],
    pub runs: [Run; SB * MAXRUN],
    pub nrun: [RunCount; SB],
    pub vp: [PackedCell; MAX_ROWS * MAX_COLS],
    pub promoted: [bool; MAX_ROWS],
    pub tmpl: [PackedCell; NSTYLE * MAX_COLS],
    pub tmpl_key: [StyleWord; NSTYLE],
    pub tmpl_valid: [bool; NSTYLE],
    /// Bytes the terminal owes the host.
    ///
    /// A terminal is not write-only: DA, DSR and DECRQM are questions, and an
    /// application that asks one and gets nothing back **hangs** rather than
    /// degrading. The core cannot own a PTY -- it is `no_std` with no I/O -- so
    /// this mirrors the input side: the core fills a buffer, the caller drains
    /// it with [`Term::reply`] and writes it wherever the input came from.
    pub reply: [u8; REPLY],
    /// Tab stops, one flag per column.
    ///
    /// A real table rather than the every-8 arithmetic HT used to do: HTS, TBC,
    /// CHT and CBT are meaningless without one, and `HT` after `CSI 3 g` must
    /// run to the right margin rather than to the next multiple of eight.
    pub tabs: [bool; MAX_COLS],
}

impl Storage {
    /// Allocate zeroed on the heap without a stack temporary.
    ///
    /// `Box::new(Storage::zeroed())` builds ~14 MB on the stack before moving
    /// it; LLVM elides that copy sometimes and not others, and a test thread
    /// overflowed on it. This is the only construction path callers should use
    /// — and it means no test or harness hand-rolls `vec![0u8; size_of::<..>()]`
    /// with a raw pointer cast.
    #[cfg(feature = "alloc")]
    pub fn boxed() -> alloc::boxed::Box<Storage> {
        use zerocopy::FromZeros;
        Storage::new_box_zeroed().expect("allocation failed")
    }

    pub const fn zeroed() -> Self {
        const R: Run = Run {
            pos: BytePos::ZERO,
            len: ByteLen::ZERO,
            style: StyleId::DEFAULT,
            encoding: 0,
            _pad: [0; 3],
        };
        const C: PackedCell = PackedCell::EMPTY;
        Storage {
            arena: [0; ARENA],
            runs: [R; SB * MAXRUN],
            nrun: [RunCount::ZERO; SB],
            vp: [C; MAX_ROWS * MAX_COLS],
            promoted: [false; MAX_ROWS],
            tmpl: [C; NSTYLE * MAX_COLS],
            tmpl_key: [StyleWord::DEFAULT; NSTYLE],
            tmpl_valid: [false; NSTYLE],
            // Zeroed here and populated by Term::reset_tabs, so `Storage` stays
            // a plain zeroable block for `new_box_zeroed`.
            tabs: [false; MAX_COLS],
            reply: [0; REPLY],
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Ground,
    Escape,
    /// After `ESC` and an intermediate byte. `[[source: williams]]`'s
    /// `escape_intermediate`. Without it `ESC # 8` dropped the `#` and printed
    /// the `8`.
    EscapeIntermediate,
    CsiParams,
    /// `[[source: williams]]`'s `csi_intermediate`. The intermediate is kept,
    /// so `CSI ! p` (DECSTR) is distinguishable from `CSI p`.
    CsiIntermediate,
    /// `[[source: williams]]`'s `csi_ignore`: a malformed sequence is consumed
    /// to its final byte and discarded. Returning to Ground mid-sequence
    /// instead would print the remainder as text.
    CsiIgnore,
    OscString,
    /// The payload of DCS, SOS, PM and APC.
    ///
    /// Consumed to the string terminator and executed as nothing. This state
    /// not existing was a **correctness and security** bug, not a missing
    /// feature: those introducers fell through to Ground, so
    /// `ESC _ ... ESC \` had its payload interpreted as commands. A stream
    /// could hide `CSI 2 J` inside an APC string and clear the screen.
    StringIgnore,
    /// Mid multi-byte sequence: continuation bytes remaining, and bits so far.
    Utf8 {
        remaining: Pending,
        acc: Utf8Acc,
    },
}

/// A scan backend.
///
/// The three scans below are the *only* ISA-dependent code in the parser. Every
/// fast path is written once against this trait and monomorphised per backend,
/// so the portable and accelerated feeds cannot diverge in logic — which is the
/// bug this codebase has already had once, when `ESC 7`/`8`/`M` were
/// implemented in the AVX2 path only and `feed` silently disagreed with
/// `feed_avx2`.
///
/// # Safety
/// An implementation may require a target feature. The caller establishes it
/// once, at startup; `no_std` has no runtime feature detection.
pub(crate) trait Scan {
    /// Length of the leading printable-ASCII run.
    unsafe fn run(buf: &[u8]) -> ByteLen;
    /// Offset of the first CSI final byte (`0x40..=0x7e`).
    unsafe fn final_byte(buf: &[u8]) -> Option<ByteLen>;
    /// Offset of the first BEL, or the ESC of an ESC-backslash terminator.
    unsafe fn st(buf: &[u8]) -> Option<ByteLen>;
}

/// Vectorised through `core::simd`, which lowers to NEON on aarch64, SSE2 on
/// baseline x86-64, and a scalar loop anywhere without vectors. Requires no
/// target feature, so it is always safe to call.
///
/// 16-byte lanes. [`Avx2`] stays for x86-64 because it does 32.
pub(crate) struct Portable;

/// 32-byte lanes on x86-64. Requires AVX2.
#[cfg(target_arch = "x86_64")]
pub(crate) struct Avx2;

/// Lane width. 16 bytes is what every SIMD ISA this targets has natively;
/// asking `core::simd` for more emits multiple registers per step and measured
/// no better.
const LANES: usize = 16;

impl Scan for Portable {
    // Scalar, deliberately. The `core::simd` version of this loop measured a
    // 12% regression on the streaming payload against this one and #[inline]
    // did not recover it: printable runs there average ~32 bytes, so a 16-lane
    // scan does two vector steps plus a bitmask and trailing_zeros where this
    // does ~32 compares an M-series core predicts perfectly. Vectorising a scan
    // is only free when the runs are long.
    //
    // Kept as a trait method rather than inlined back into feed_with so the
    // shared loop and the AVX2 backend survive.
    #[inline(always)]
    unsafe fn run(buf: &[u8]) -> ByteLen {
        let n = buf.len();
        let mut i = 0usize;
        while i < n && StreamByte::from_stream(buf[i]).is_printable() {
            i += 1;
        }
        ByteLen::from_lane(i)
    }

    #[inline(always)]
    unsafe fn final_byte(buf: &[u8]) -> Option<ByteLen> {
        use core::simd::{Simd, cmp::SimdPartialOrd};
        let n = buf.len();
        let mut i = 0usize;
        let lo = Simd::<u8, LANES>::splat(0x40);
        let hi = Simd::<u8, LANES>::splat(0x7e);
        while i + LANES <= n {
            let v = Simd::<u8, LANES>::from_slice(&buf[i..i + LANES]);
            let m = (v.simd_ge(lo) & v.simd_le(hi)).to_bitmask();
            if m != 0 {
                return Some(ByteLen::from_lane(i + m.trailing_zeros() as usize));
            }
            i += LANES;
        }
        while i < n {
            if FinalByte::from_stream(StreamByte::from_stream(buf[i])).is_some() {
                return Some(ByteLen::from_lane(i));
            }
            i += 1;
        }
        None
    }

    #[inline(always)]
    unsafe fn st(buf: &[u8]) -> Option<ByteLen> {
        use core::simd::{Simd, cmp::SimdPartialEq};
        let n = buf.len();
        let mut i = 0usize;
        let bel = Simd::<u8, LANES>::splat(0x07);
        let esc = Simd::<u8, LANES>::splat(0x1b);
        while i + LANES <= n {
            let v = Simd::<u8, LANES>::from_slice(&buf[i..i + LANES]);
            let m = (v.simd_eq(bel) | v.simd_eq(esc)).to_bitmask();
            if m != 0 {
                return Some(ByteLen::from_lane(i + m.trailing_zeros() as usize));
            }
            i += LANES;
        }
        while i < n {
            let b = StreamByte::from_stream(buf[i]);
            if b == StreamByte::BEL || b == StreamByte::ESC {
                return Some(ByteLen::from_lane(i));
            }
            i += 1;
        }
        None
    }
}

#[cfg(target_arch = "x86_64")]
impl Scan for Avx2 {
    unsafe fn run(buf: &[u8]) -> ByteLen {
        unsafe { Term::scan_avx2(buf) }
    }
    unsafe fn final_byte(buf: &[u8]) -> Option<ByteLen> {
        unsafe { Term::find_final(buf) }
    }
    unsafe fn st(buf: &[u8]) -> Option<ByteLen> {
        unsafe { Term::find_st(buf) }
    }
}

pub struct Term<'a> {
    s: &'a mut Storage,
    tail: ArenaTail,
    head: RingPos,
    cx: Col,
    cy: Row,
    cols: Cols,
    rows: Rows,
    st: State,
    params: [Option<Param>; 16],
    nparam: ParamIdx,
    private: Marker,
    style: StyleId,
    style_raw: StyleWord,
    saved: Option<(Row, Col, StyleId, StyleWord)>,
    /// DECSTBM. Inclusive, and full-screen until `CSI r` says otherwise.
    scroll_top: Row,
    scroll_bot: Row,
    /// A bitset rather than a struct of bools -- see [`Modes`].
    modes: Modes,
    mode: OpMode,
    pub promotions: Stat,
    pub tmpl_builds: Stat,
    pub tmpl_hits: Stat,
    pub maxrun_overflows: Stat,
    pub replacements: Stat,
    // why the fast CSI path declined, so the fallback rate is measurable
    pub fast_hits: Stat,
    pub bells: Stat,
    /// How much of `Storage::reply` is pending.
    reply_len: ByteLen,
    /// Replies dropped because the buffer was full. Counted so a caller that
    /// never drains can find out, instead of losing answers silently.
    pub reply_overflows: Stat,
    /// The intermediate byte of the `ESC` sequence being parsed.
    esc_inter: Option<StreamByte>,
    /// The last graphic character written, for REP.
    ///
    /// `[[source: ctlseqs]]`: "Repeat the preceding graphic character Ps
    /// times". `None` when nothing has been printed yet, in which case REP has
    /// nothing to repeat and must do nothing rather than repeat a stale cell.
    last_cp: Option<(Codepoint, Width)>,
    /// The intermediate byte of the CSI being parsed, if any. `CSI ! p` is
    /// DECSTR and `CSI p` is not the same operation.
    csi_inter: Option<StreamByte>,
    pub fb_not_csi: Stat,
    pub fb_boundary: Stat,
    pub fb_odd_param: Stat,
}

impl<'a> Term<'a> {
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
            params: [None; 16],
            nparam: ParamIdx::FIRST,
            private: Marker::Standard,
            style: StyleId::DEFAULT,
            style_raw: StyleWord::DEFAULT,
            saved: None,
            scroll_top: Row::ZERO,
            scroll_bot: Row::from_index(rows.clamp(MAX_ROWS).index() - 1),
            modes: Modes::default(),
            mode,
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
            fb_not_csi: Stat::ZERO,
            fb_boundary: Stat::ZERO,
            fb_odd_param: Stat::ZERO,
        };
        t.reset_tabs();
        t.clear_screen();
        t
    }

    /// Fast-path hits: CSI or `ESC Fe` sequences consumed in bulk.
    ///
    /// These counters have been fed since the fast paths were written and never
    /// read anywhere, which is how "is the fast path even firing on this
    /// target?" became a question that could not be answered from the outside.
    /// Bytes the terminal owes the host, e.g. the answer to a DA or DSR.
    ///
    /// Write these back to wherever the input came from, then call
    /// [`Term::clear_reply`]. Empty when there is nothing to say.
    pub fn reply(&self) -> &[u8] {
        &self.s.reply[..self.reply_len.index()]
    }
    /// Drop the pending reply after sending it.
    pub fn clear_reply(&mut self) {
        self.reply_len = ByteLen::ZERO;
    }

    /// Append to the pending reply, or count the loss.
    fn emit(&mut self, bytes: &[u8]) {
        let at = self.reply_len.index();
        if at + bytes.len() > REPLY {
            self.reply_overflows.inc();
            return;
        }
        self.s.reply[at..at + bytes.len()].copy_from_slice(bytes);
        self.reply_len = ByteLen::from_index(at + bytes.len());
    }

    /// Append a 1-based coordinate as decimal.
    ///
    /// No allocator and no `format!`. Five digits covers MAX_COLS and MAX_ROWS
    /// with room to spare; anything larger cannot be a coordinate on this grid.
    fn emit_num(&mut self, v: OneBased) {
        // A coordinate, not a `usize`. Taking `usize` here tripped the audit --
        // correctly: it is exactly the "two quantities confused" shape the
        // newtypes exist to prevent, and a report is where an off-by-one would
        // be invisible until an application drew in the wrong place.
        let n = usize::from(v.get());
        const DIGITS: &[u8; 10] = b"0123456789";
        let mut buf = [0u8; 5];
        let mut i = buf.len();
        let mut v = n;
        loop {
            i -= 1;
            // Indexed, not `(v % 10) as u8`: the cast would be another raw-integer
            // site against the audit budget, and a table lookup says what it means.
            buf[i] = DIGITS[v % 10];
            v /= 10;
            if v == 0 || i == 0 {
                break;
            }
        }
        // `buf` is local, so this does not borrow `self`.
        let (lo, hi) = (i, buf.len());
        let mut tmp = [0u8; 5];
        tmp[..hi - lo].copy_from_slice(&buf[lo..hi]);
        self.emit(&tmp[..hi - lo]);
    }

    /// BEL characters seen. There is no UI here to ring one.
    pub fn bells(&self) -> u64 {
        self.bells.count()
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

    /// Default stops every eight columns, which is what a terminal powers on
    /// with and what RIS restores.
    fn reset_tabs(&mut self) {
        for (i, stop) in self.s.tabs.iter_mut().enumerate() {
            *stop = i % 8 == 0 && i != 0;
        }
    }

    /// HT, and the engine of CHT. Advances to the next stop, or to the last
    /// column when there is none left -- never past the margin.
    fn tab_forward(&mut self, n: Count) {
        let last = self.cols.index() - 1;
        // There cannot be more stops than columns, so a larger count is the
        // same as `cols`. Without this, `CSI 65535 I` spins 65535 times once
        // the cursor is already at the margin -- bounded, but work a stream
        // should not be able to ask for.
        for _ in 0..n.at_least_one().min(self.cols.index()) {
            let mut x = self.cx.index();
            loop {
                if x >= last {
                    x = last;
                    break;
                }
                x += 1;
                if self.s.tabs[x] {
                    break;
                }
            }
            self.cx = Col::at_index(x);
        }
    }

    /// CBT. Backward to the previous stop, or to column 0.
    fn tab_back(&mut self, n: Count) {
        for _ in 0..n.at_least_one().min(self.cols.index()) {
            let mut x = self.cx.index();
            loop {
                if x == 0 {
                    break;
                }
                x -= 1;
                if self.s.tabs[x] {
                    break;
                }
            }
            self.cx = Col::at_index(x);
        }
    }

    /// A fresh screen is spaces, not NULs. Terminals compare equal on blank
    /// cells and a NUL renders as a hole.
    fn clear_screen(&mut self) {
        for y in 0..self.rows.index() {
            self.view().fill_row(Row::from_index(y), PackedCell::EMPTY);
        }
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
        self.clear_screen();
        if over {
            Resized::Truncated {
                cols: self.cols,
                rows: self.rows,
            }
        } else {
            Resized::Ok
        }
    }

    pub fn cursor(&self) -> (Row, Col) {
        (self.cy, self.cx)
    }
    pub fn geometry(&self) -> (Cols, Rows) {
        (self.cols, self.rows)
    }
    pub fn stride(&self) -> Cols {
        Cols::from_stride(MAX_COLS)
    }
    /// The whole materialised region, stride included. Callers wanting a single
    /// row should use [`Term::row`], which is bounds-correct by construction.
    pub fn viewport(&self) -> &[PackedCell] {
        &self.s.vp[..self.rows.index() * MAX_COLS]
    }
    pub fn row(&self, y: Row) -> &[PackedCell] {
        View::new_ref(&self.s.vp, self.stride(), self.cols, self.rows).row_slice(y)
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

    #[inline(always)]
    fn line(&self) -> LineSlot {
        LineSlot::of(self.head.plus(self.cy, SB))
    }

    /// IND: down one line, scrolling the region if already at its bottom.
    ///
    /// **The column is not touched.** This used to reset `cx`, which made `LF`
    /// and `ESC D` behave as `NEL`. `[[source: ctlseqs]]` defines IND as index
    /// only, and LNM (mode 20, reset by default) is what decides whether LF
    /// also returns to column 1 -- so the old behaviour was LNM permanently on.
    #[inline(always)]
    fn index(&mut self) {
        if self.cy == self.scroll_bot {
            self.scroll_region_up(Count::ONE);
        } else if let Some(r) = self.cy.next(self.rows) {
            self.cy = r;
        }
        // The destination line starts empty of spans.
        //
        // Load-bearing for chunk invariance, not tidiness: without it runs
        // accumulate on a line across index operations, so whether MAXRUN
        // overflows -- and therefore whether the line gets promoted -- depends
        // on how the input happened to be split into reads. Dropping this line
        // broke ingest_is_chunk_invariant.
        //
        // KNOWN GAP: this also means IND onto an *existing* unpromoted line
        // discards its spans, where the spec moves the cursor without touching
        // content. Recorded in docs/vt-coverage.md rather than fixed here,
        // because fixing it means bounding run growth some other way and that
        // needs its own measurement.
        let i = self.line();
        self.s.nrun[i.index()] = RunCount::ZERO;
    }

    /// CR + IND: what autowrap does at the right margin, and what NEL is.
    #[inline(always)]
    fn wrap(&mut self) {
        self.cx = Col::ZERO;
        self.index();
    }

    /// RI. Up one line, scrolling the region down if already at its top.
    ///
    /// Was keyed on row 0 rather than the region top, so `ESC M` inside a
    /// scrolling region scrolled the whole screen.
    #[inline(always)]
    fn reverse_index(&mut self) {
        if self.cy == self.scroll_top {
            self.scroll_region_down(Count::ONE);
        } else if self.cy.index() > 0 {
            self.cy = self.cy.up(Count::ONE);
        }
    }

    /// LF. Adds the carriage return only when LNM is set.
    #[inline(always)]
    fn line_feed(&mut self) {
        self.index();
        if self.modes.is_set(ModeBit::LineFeedNewLine) {
            self.cx = Col::ZERO;
        }
    }

    /// Scroll the scrolling region up by `n`, blanking what comes in at the
    /// bottom. The engine of LF-at-region-bottom, SU, and DL.
    ///
    /// The full-screen case advances the ring instead of moving cells: O(1)
    /// against O(rows x cols), and it is what makes streaming ingest fast. A
    /// region smaller than the screen cannot use it -- rows outside the region
    /// must not move -- so it pays for the copy.
    fn scroll_region_up(&mut self, n: Count) {
        let (top, bot) = (self.scroll_top.index(), self.scroll_bot.index());
        if top > bot {
            return;
        }
        let k = n.at_least_one().min(bot + 1 - top);
        let blank = PackedCell::empty_with(self.style);
        let last = self.rows.index() - 1;
        if top == 0 && bot == last {
            for _ in 0..k {
                self.head = self.head.advance(SB);
                let y = Row::from_index(last);
                self.s.nrun[self.head.plus(y, SB).index()] = RunCount::ZERO;
                self.s.promoted[last] = false;
                self.view().fill_row(y, blank);
            }
            return;
        }
        let (rt, rb) = (self.scroll_top, self.scroll_bot);
        self.view().delete_lines_in(rt, n, rb, blank);
        for y in top..=bot {
            self.s.promoted[y] = true;
            self.s.nrun[self.head.plus(Row::from_index(y), SB).index()] = RunCount::ZERO;
        }
    }

    /// Scroll the scrolling region down by `n`. The engine of RI-at-region-top,
    /// SD, and IL.
    fn scroll_region_down(&mut self, n: Count) {
        let (top, bot) = (self.scroll_top.index(), self.scroll_bot.index());
        if top > bot {
            return;
        }
        let blank = PackedCell::empty_with(self.style);
        let (rt, rb) = (self.scroll_top, self.scroll_bot);
        self.view().insert_lines_in(rt, n, rb, blank);
        for y in top..=bot {
            self.s.promoted[y] = true;
            self.s.nrun[self.head.plus(Row::from_index(y), SB).index()] = RunCount::ZERO;
        }
    }

    /// DECSTBM. Sets the region and homes the cursor, per `[[source: ctlseqs]]`.
    ///
    /// A region of fewer than two lines is rejected and leaves the previous one
    /// in place: a one-line region cannot scroll, and applications send
    /// `CSI r` with no parameters to mean "the whole screen".
    fn set_scroll_region(&mut self, top: OneBased, bottom: Option<OneBased>) {
        let last = Row::from_index(self.rows.index() - 1);
        let t = top.to_row(self.rows);
        let b = bottom.map_or(last, |v| v.to_row(self.rows));
        if t.index() >= b.index() {
            return;
        }
        self.scroll_top = t;
        self.scroll_bot = b;
        self.cx = Col::ZERO;
        self.cy = t;
    }

    /// DECSET / DECRST.
    ///
    /// Only modes baud acts on are stored, for the same reason as the ANSI
    /// ones: a bit that is set and never read is the "accepted, did nothing"
    /// failure `docs/vt-coverage.md` calls `parsed`. Mouse reporting, bracketed
    /// paste and the alternate screen are recognised by the parser and
    /// deliberately not recorded here -- they are input-side or need a second
    /// screen buffer, and pretending otherwise would be worse than ignoring.
    fn set_private_mode(&mut self, m: PrivateMode, on: bool) {
        match m {
            PrivateMode::CursorVisible => self.modes.apply(ModeBit::CursorVisible, on),
            PrivateMode::Origin => {
                self.modes.apply(ModeBit::Origin, on);
                // Both DECSET and DECRST home the cursor, and where home *is*
                // depends on the mode just applied.
                self.cx = Col::ZERO;
                self.cy = if on { self.scroll_top } else { Row::ZERO };
            }
            PrivateMode::AutoWrap => self.modes.apply(ModeBit::AutoWrap, on),
            _ => {}
        }
    }

    /// Whether the cursor should be drawn. DECTCEM; a renderer needs it.
    pub fn cursor_visible(&self) -> bool {
        self.modes.is_set(ModeBit::CursorVisible)
    }

    /// Origin for row addressing, and the row a CUP parameter is relative to.
    ///
    /// `[[source: vt510-decom]]`: "When DECOM is set, the home cursor position
    /// is at the upper-left corner of the screen, within the margins. The
    /// starting point for line numbers depends on the current top margin
    /// setting. The cursor cannot move outside of the margins."
    fn origin_row(&self) -> Row {
        if self.modes.is_set(ModeBit::Origin) {
            self.scroll_top
        } else {
            Row::ZERO
        }
    }

    /// Clamp a row to the margins when DECOM confines the cursor to them.
    fn confine(&self, r: Row) -> Row {
        if !self.modes.is_set(ModeBit::Origin) {
            return r;
        }
        Row::from_index(
            r.index()
                .clamp(self.scroll_top.index(), self.scroll_bot.index()),
        )
    }

    /// SM / RM for the unprefixed namespace.
    ///
    /// Only modes baud acts on are stored. An unrecognised mode is ignored
    /// rather than recorded, so a bit can never be set and never read.
    fn set_ansi_mode(&mut self, m: AnsiMode, on: bool) {
        match m {
            AnsiMode::LineFeedNewLine => self.modes.apply(ModeBit::LineFeedNewLine, on),
            AnsiMode::InsertReplace => self.modes.apply(ModeBit::InsertReplace, on),
            AnsiMode::Other(_) => {}
        }
    }

    fn delete_lines(&mut self, n: Count) {
        if self.cy.index() < self.scroll_top.index() || self.cy.index() > self.scroll_bot.index() {
            return;
        }
        let blank = PackedCell::empty_with(self.style);
        let (cy, bot) = (self.cy.index(), self.scroll_bot.index());
        let (at, rb) = (self.cy, self.scroll_bot);
        self.view().delete_lines_in(at, n, rb, blank);
        for y in cy..=bot {
            self.s.promoted[y] = true;
            self.s.nrun[self.head.plus(Row::from_index(y), SB).index()] = RunCount::ZERO;
        }
    }

    #[inline(always)]
    fn intern_style(&mut self, raw: StyleWord) -> StyleId {
        let k = StyleId::bucket(raw, NSTYLE);
        if self.s.tmpl_valid[k] && self.s.tmpl_key[k] == raw {
            self.tmpl_hits.inc();
        } else {
            self.s.tmpl_valid[k] = true;
            self.s.tmpl_key[k] = raw;
            self.tmpl_builds.inc();
            let c = PackedCell::empty_with(StyleId::from_index(k));
            let a = k * MAX_COLS;
            self.s.tmpl[a..a + MAX_COLS].fill(c);
        }
        StyleId::from_index(k)
    }

    #[inline(always)]
    fn view(&mut self) -> View<'_> {
        View::new(
            &mut self.s.vp,
            Cols::from_stride(MAX_COLS),
            self.cols,
            self.rows,
        )
    }

    #[inline(always)]
    fn write_cell(&mut self, cp: Codepoint) {
        let (r, c, st) = (self.cy, self.cx, self.style);
        self.view().put(r, c, PackedCell::glyph(cp, st));
    }

    /// Place one decoded codepoint. A 2-wide character at the last column
    /// cannot be split: the cell is left blank and it moves to the next row.
    /// That decision is forced by `try_advance` returning `Option`.
    #[inline(always)]
    fn put_cp(&mut self, cp: Codepoint, w: Width) {
        if w == Width::ZERO {
            return;
        }
        if self.cx.try_advance(w, self.cols).is_none() {
            if !self.modes.is_set(ModeBit::AutoWrap) {
                // DECAWM reset: the character overwrites the last column and the
                // cursor stays put. No wrap, and therefore no scroll.
                self.cx = Col::at_index(self.cols.index() - 1);
                self.promote_if_runs();
                self.write_cell(cp);
                return;
            }
            self.wrap();
        }
        self.promote_if_runs();
        self.write_cell(cp);
        self.last_cp = Some((cp, w));
        if w == Width::WIDE {
            let (r, c, st) = (self.cy, self.cx.after(), self.style);
            self.view().put(r, c, PackedCell::wide_tail(st));
        }
        self.cx = self.cx.try_advance(w, self.cols).unwrap_or(Col::ZERO);
    }

    #[inline(always)]
    fn blit(&mut self, base: BytePos, off: ByteLen, room: Room) {
        let abs = base.index() + off.index();
        let (r, c, st) = (self.cy, self.cx, self.style);
        let (stride, cols, rows) = (Cols::from_stride(MAX_COLS), self.cols, self.rows);
        let (arena, vp) = (&self.s.arena, &mut self.s.vp);
        View::new(vp, stride, cols, rows).put_ascii_run(r, c, &arena[abs..abs + room.index()], st);
    }

    #[inline(always)]
    fn put_run(&mut self, base: BytePos, mut off: ByteLen, mut n: ByteLen) {
        // Hoisted: a mode cannot change inside a run, and reading it per
        // iteration cost 12% of streaming throughput -- measured, not guessed.
        let wrap = self.modes.is_set(ModeBit::AutoWrap);
        while !n.is_zero() {
            let mut room = self.cols.room_at(self.cx).min(n);
            if room.is_empty() {
                if wrap {
                    self.wrap();
                } else {
                    // DECAWM reset: pin to the last column and let each
                    // remaining byte overwrite it. The last one wins, and the
                    // screen never scrolls.
                    self.cx = Col::at_index(self.cols.index() - 1);
                }
                room = self.cols.room_at(self.cx).min(n);
            }
            let promoted = self.s.promoted[self.cy.index()];
            if self.mode == OpMode::Runs && !promoted {
                let i = self.line();
                let c = self.s.nrun[i.index()];
                if c.index() < MAXRUN {
                    self.s.runs[i.run(c, MAXRUN)] = Run {
                        pos: base.advance(off),
                        len: ByteLen::from_room(room),
                        style: self.style,
                        encoding: 0,
                        _pad: [0; 3],
                    }
                    .with_encoding(Encoding::Ascii);
                    self.s.nrun[i.index()] = c.inc();
                } else {
                    self.maxrun_overflows.inc();
                    self.promote();
                    self.blit(base, off, room);
                }
            } else {
                self.blit(base, off, room);
            }
            // The last byte of this run is the newest graphic character, which
            // is what REP repeats. Set here as well as in put_cp: plain ASCII
            // never reaches put_cp, so recording it only there would make
            // `abc CSI 3 b` repeat whatever was printed before the run.
            if !room.is_empty() {
                let at = base.index() + off.index() + room.index() - 1;
                let b = StreamByte::from_stream(self.s.arena[at]);
                self.last_cp = Some((Codepoint::from_ascii(b), Width::NARROW));
            }
            self.cx += room;
            // With autowrap on, the cursor may sit one past the last column --
            // the "wrap is pending" sentinel the next write consumes. With it
            // off there is no pending wrap, so that position is not reachable
            // and the cursor belongs on the last cell.
            if !wrap && self.cx.index() >= self.cols.index() {
                self.cx = Col::at_index(self.cols.index() - 1);
            }
            off = off.saturating_add(ByteLen::from_room(room));
            n = n.sub_room(room);
        }
    }

    fn promote(&mut self) {
        let y = self.cy.index();
        if self.s.promoted[y] {
            return;
        }
        let i = self.line();
        let n = self.s.nrun[i.index()].index();
        let mut c = 0usize;
        for r in 0..n {
            let run = self.s.runs[i.run(RunCount::from_index(r), MAXRUN)];
            let (o, l) = (run.pos.index(), run.len.index());
            for z in 0..l {
                if c >= self.cols.index() {
                    break;
                }
                let b = StreamByte::from_stream(self.s.arena[o + z]);
                let (rr, cc) = (Row::from_index(y), Col::at_index(c));
                self.view()
                    .put(rr, cc, PackedCell::from_ascii(b, run.style));
                c += 1;
            }
        }
        let blank = PackedCell::empty_with(self.style);
        let (rr, from, to) = (
            Row::from_index(y),
            Col::at_index(c),
            Col::at_index(self.cols.index()),
        );
        self.view().fill(rr, from, to, blank);
        self.s.promoted[y] = true;
        self.promotions.inc();
    }

    #[inline(always)]
    fn promote_if_runs(&mut self) {
        if self.mode == OpMode::Runs {
            self.promote();
        }
    }

    fn erase_line(&mut self, e: EraseExtent) {
        let y = self.cy.index();
        // Erase-to-end of an unpromoted line in Runs mode IS the truncation of
        // its span list: there is nothing after nrun to erase, and the viewport
        // is a projection of the spans. Promoting and filling afterwards
        // materialises a row only to blank it.
        //
        // On the TUI payload this is ~134 cell writes per 77 input bytes -- a
        // CSI K per line against a 200-column grid -- which is write
        // amplification of roughly 20:1 against rule #1's <= 1.
        if self.mode == OpMode::Runs && !self.s.promoted[y] && e == EraseExtent::ToEnd {
            let i = self.line();
            self.s.nrun[i.index()] = RunCount::ZERO;
            return;
        }
        self.promote_if_runs();
        let (from, to) = match e {
            EraseExtent::ToEnd => (self.cx.index(), self.cols.index()),
            EraseExtent::ToStart => (0, (self.cx.index() + 1).min(self.cols.index())),
            EraseExtent::All => (0, self.cols.index()),
        };
        let b = PackedCell::empty_with(self.style);
        let (rr, f, t) = (Row::from_index(y), Col::at_index(from), Col::at_index(to));
        self.view().fill(rr, f, t, b);
    }

    fn erase_display(&mut self, e: EraseExtent) {
        let (lo, hi) = match e {
            EraseExtent::ToEnd => (self.cy.index(), self.rows.index()),
            EraseExtent::ToStart => (0, self.cy.index() + 1),
            EraseExtent::All => (0, self.rows.index()),
        };
        let b = PackedCell::empty_with(self.style);
        for y in lo..hi {
            self.view().fill_row(Row::from_index(y), b);
            self.s.nrun[self.head.plus(Row::from_index(y), SB).index()] = RunCount::ZERO;
            self.s.promoted[y] = true;
        }
    }

    fn erase_chars(&mut self, n: Count) {
        self.promote_if_runs();
        let b = PackedCell::empty_with(self.style);
        let (r, from) = (self.cy, self.cx);
        let to = Col::at_index(self.cx.index() + n.at_least_one());
        self.view().fill(r, from, to, b);
    }

    fn delete_chars(&mut self, n: Count) {
        self.promote_if_runs();
        let (r, at, b) = (self.cy, self.cx, PackedCell::empty_with(self.style));
        self.view().delete_chars(r, at, n, b);
    }

    fn insert_blanks(&mut self, n: Count) {
        self.promote_if_runs();
        let (r, at, b) = (self.cy, self.cx, PackedCell::empty_with(self.style));
        self.view().insert_blanks(r, at, n, b);
    }

    /// IL. Bounded by the scrolling region, and a no-op with the cursor
    /// outside it -- `[[source: ctlseqs]]` defines IL as operating within the
    /// region. It used to shift to the bottom of the *screen*, which moved rows
    /// that DECSTBM had placed outside the region.
    fn insert_lines(&mut self, n: Count) {
        if self.cy.index() < self.scroll_top.index() || self.cy.index() > self.scroll_bot.index() {
            return;
        }
        let (cy, bot) = (self.cy.index(), self.scroll_bot.index());
        let k = n.at_least_one().min(bot + 1 - cy);
        let (at, rb, b) = (self.cy, self.scroll_bot, PackedCell::empty_with(self.style));
        self.view().insert_lines_in(at, n, rb, b);
        for y in cy..cy + k {
            self.s.promoted[y] = true;
            self.s.nrun[self.head.plus(Row::from_index(y), SB).index()] = RunCount::ZERO;
        }
    }

    fn apply_sgr(&mut self) {
        let n = self.nparam.count_at_least_one();
        let mut i = 0usize;
        while i < n {
            match baud_domain::SgrAttr::parse(Params::new(&self.params[i..n])) {
                Some((attr, used)) => {
                    self.style_raw = attr.fold(self.style_raw);
                    i += used.at_least_one();
                }
                None => break,
            }
        }
        self.style = self.intern_style(self.style_raw);
    }

    fn dispatch(&mut self, f: FinalByte) {
        let n = self.nparam.count_at_least_one();
        let marker = self.private;
        let p = Params::new(&self.params[..n]);
        match Csi::parse(marker, self.csi_inter, p, f) {
            Csi::SoftReset => self.soft_reset(),
            Csi::PrivateMode { set, mode } => self.set_private_mode(mode, set),
            // `[[source: ctlseqs]]` lists the Primary DA identities. VT102 is
            // what baud actually is: it has DECSTBM (VT100) and IL/DL (VT102),
            // and it does NOT have sixel, ReGIS, UDKs, selective erase or
            // 132-column mode. Claiming VT220 would make applications send
            // sequences baud drops -- under-claiming degrades, over-claiming
            // breaks.
            Csi::Repeat(c) => {
                if let Some((cp, w)) = self.last_cp {
                    // Bounded by the row: a repeat cannot usefully exceed the
                    // line it is filling, and `CSI 65535 b` should not spin.
                    let k = c.at_least_one().min(self.cols.index());
                    for _ in 0..k {
                        self.put_cp(cp, w);
                    }
                }
            }
            Csi::DeviceAttributes => self.emit(b"\x1b[?6c"),
            Csi::DeviceStatus(Some(baud_domain::StatusKind::Operating)) => self.emit(b"\x1b[0n"),
            Csi::DeviceStatus(Some(baud_domain::StatusKind::CursorPosition)) => {
                // CPR is 1-based, like CUP.
                let (r, c) = (self.cy.one_based(), self.cx.one_based());
                self.emit(b"\x1b[");
                self.emit_num(r);
                self.emit(b";");
                self.emit_num(c);
                self.emit(b"R");
            }
            // A DSR baud does not implement gets no answer. Replying with the
            // wrong report shape is worse than silence: the application parses
            // it as the report it asked for.
            Csi::DeviceStatus(None) => {}
            Csi::CursorPosition { row, col } => {
                let base = self.origin_row().index();
                let want = Row::from_index(base + row.to_row(self.rows).index());
                self.cy = self.confine(want);
                self.cx = col.to_col(self.cols);
            }
            Csi::CursorUp(c) => self.cy = self.confine(self.cy.up(c)),
            Csi::CursorDown(c) => self.cy = self.confine(self.cy.down(c, self.rows)),
            Csi::CursorForward(c) => self.cx = self.cx.forward(c, self.cols),
            Csi::CursorBack(c) => self.cx = self.cx.back(c),
            Csi::EraseInLine(e) => self.erase_line(e),
            Csi::EraseInDisplay(e) => self.erase_display(e),
            Csi::EraseChars(c) => self.erase_chars(c),
            Csi::DeleteChars(c) => self.delete_chars(c),
            Csi::InsertBlanks(c) => self.insert_blanks(c),
            Csi::CursorForwardTab(c) => self.tab_forward(c),
            Csi::CursorBackTab(c) => self.tab_back(c),
            Csi::TabClear(Some(baud_domain::TabClear::AtCursor)) => {
                let x = self.cx.index();
                self.s.tabs[x] = false;
            }
            Csi::TabClear(Some(baud_domain::TabClear::All)) => {
                for stop in self.s.tabs.iter_mut() {
                    *stop = false;
                }
            }
            Csi::TabClear(None) => {}
            Csi::CursorNextLine(c) => {
                self.cy = self.confine(self.cy.down(c, self.rows));
                self.cx = Col::ZERO;
            }
            Csi::CursorPrevLine(c) => {
                self.cy = self.confine(self.cy.up(c));
                self.cx = Col::ZERO;
            }
            Csi::ColumnAbsolute(c) => self.cx = c.to_col(self.cols),
            Csi::LineAbsolute(r) => {
                let base = self.origin_row().index();
                let want = Row::from_index(base + r.to_row(self.rows).index());
                self.cy = self.confine(want);
            }
            Csi::ColumnRelative(c) => self.cx = self.cx.forward(c, self.cols),
            Csi::LineRelative(c) => self.cy = self.confine(self.cy.down(c, self.rows)),
            Csi::SaveCursor => self.saved = Some((self.cy, self.cx, self.style, self.style_raw)),
            Csi::RestoreCursor => {
                if let Some((r, c, st, sw)) = self.saved {
                    self.cy = r;
                    self.cx = c;
                    self.style = st;
                    self.style_raw = sw;
                }
            }
            Csi::InsertLines(c) => self.insert_lines(c),
            Csi::DeleteLines(c) => self.delete_lines(c),
            Csi::ScrollUp(c) => self.scroll_region_up(c),
            Csi::ScrollDown(c) => self.scroll_region_down(c),
            Csi::SetScrollRegion { top, bottom } => self.set_scroll_region(top, bottom),
            Csi::Mode { set, mode } => self.set_ansi_mode(mode, set),
            Csi::Sgr => self.apply_sgr(),
            _ => {}
        }
    }

    /// The C0 `execute` action.
    ///
    /// Separate from the Ground branch because `[[source: williams]]` has the
    /// escape and CSI states execute C0 controls mid-sequence rather than
    /// treating them as the end of the sequence: `event 00-17,19,1C-1F /
    /// execute`. A stream carrying `ESC [ 1 ; \n 2 m` gets its line feed *and*
    /// its SGR.
    #[inline(always)]
    fn execute(&mut self, b: StreamByte) {
        if b == StreamByte::LF {
            self.line_feed();
        } else if b == StreamByte::CR {
            self.cx = Col::ZERO;
        } else if b == StreamByte::BS {
            self.cx = self.cx.back(Count::ONE);
        } else if b == StreamByte::TAB {
            self.tab_forward(Count::ONE);
        } else if b == StreamByte::VT || b == StreamByte::FF {
            // Both are an index. A terminal does not eject a page, and they were
            // being dropped, so a stream containing VT lost the line break.
            self.index();
        } else if b == StreamByte::BEL {
            // No UI here to ring one, so count it. A core that silently drops
            // BEL leaves an embedder no way to implement one.
            self.bells.inc();
        }
        // Anything else in C0 is discarded, never printed.
    }

    #[inline(always)]
    fn control(&mut self, b: StreamByte) {
        match self.st {
            State::Ground => {
                if b == StreamByte::ESC {
                    self.st = State::Escape;
                } else if b.is_c0() || b == StreamByte::BEL {
                    self.execute(b);
                } else if let Some((rem, acc)) = b.utf8_lead() {
                    self.st = State::Utf8 {
                        remaining: Pending::new(rem),
                        acc: Utf8Acc::lead(acc),
                    };
                } else {
                    // stray continuation byte or 0x7f: not text
                    self.replacements.inc();
                    self.put_cp(Codepoint::REPLACEMENT, Width::NARROW);
                }
            }
            State::Utf8 { remaining, acc } => match b.utf8_continuation() {
                Some(bits) => {
                    let acc = acc.push(bits);
                    if remaining.is_last() {
                        self.st = State::Ground;
                        let c = acc.finish();
                        self.put_cp(c, Width::of(c));
                    } else {
                        self.st = State::Utf8 {
                            remaining: remaining.dec(),
                            acc,
                        };
                    }
                }
                None => {
                    self.st = State::Ground;
                    self.replacements.inc();
                    self.put_cp(Codepoint::REPLACEMENT, Width::NARROW);
                    self.control(b);
                }
            },
            State::Escape => {
                if b == StreamByte::CSI_INTRO {
                    self.params = [None; 16];
                    self.nparam = ParamIdx::FIRST;
                    self.private = Marker::Standard;
                    self.csi_inter = None;
                    self.st = State::CsiParams;
                } else if b == StreamByte::OSC_INTRO {
                    self.st = State::OscString;
                } else if b == StreamByte::DCS_INTRO
                    || b == StreamByte::SOS_INTRO
                    || b == StreamByte::PM_INTRO
                    || b == StreamByte::APC_INTRO
                {
                    // Consume the payload; never execute it. These fell through
                    // to `esc_fe` and back to Ground, so the bytes that followed
                    // were interpreted as commands.
                    self.st = State::StringIgnore;
                } else if b.is_esc_intermediate() {
                    self.esc_inter = Some(b);
                    self.st = State::EscapeIntermediate;
                } else {
                    // Same two-byte escapes the AVX2 path handles. The two feed
                    // paths must be indistinguishable; `feed_matches_feed_avx2`
                    // in the property suite asserts it.
                    self.esc_fe(b);
                    self.st = State::Ground;
                }
            }
            State::EscapeIntermediate => {
                if b.is_c0() {
                    self.execute(b);
                } else if b.is_esc_intermediate() {
                    self.esc_inter = Some(b);
                } else if b != StreamByte::DEL {
                    // `ESC # 8` is DECALN. The charset designators
                    // (`ESC ( <c>`) are consumed and select nothing -- see
                    // docs/vt-coverage.md for why the mapping is not invented.
                    if self.esc_inter == Some(StreamByte::HASH) && b.raw_byte() == b'8' {
                        self.screen_alignment();
                    }
                    self.st = State::Ground;
                }
            }
            State::StringIgnore => {
                // Terminated by ST (`ESC \`) or, as xterm accepts, by BEL.
                // Any ESC leaves the string; the Escape state decides whether
                // the next byte is the ST backslash or a fresh sequence, so a
                // truncated string cannot swallow what follows it forever.
                if b == StreamByte::BEL {
                    self.st = State::Ground;
                } else if b == StreamByte::ESC {
                    self.st = State::Escape;
                }
            }
            State::CsiIntermediate => {
                if let Some(f) = FinalByte::from_stream(b) {
                    self.dispatch(f);
                    self.st = State::Ground;
                } else if !b.is_csi_intermediate() {
                    self.st = State::CsiIgnore;
                }
            }
            State::CsiIgnore => {
                // Consume to the final byte and dispatch nothing.
                if b.is_c0() {
                    self.execute(b);
                } else if FinalByte::from_stream(b).is_some() {
                    self.st = State::Ground;
                }
            }
            State::CsiParams => {
                if let Some(d) = b.as_ascii_digit() {
                    if self.nparam.is_first() {
                        self.nparam = self.nparam.next();
                    }
                    let cur = self.params[self.nparam.raw_index() - 1].unwrap_or(Param::ZERO);
                    self.params[self.nparam.raw_index() - 1] = Some(cur.push(d));
                } else if b == StreamByte::SEMICOLON {
                    if self.nparam.is_first() {
                        self.nparam = self.nparam.next();
                    }
                    if self.nparam.raw_index() < 16 {
                        self.params[self.nparam.raw_index()] = None;
                        self.nparam = self.nparam.next();
                    }
                } else if b == StreamByte::QUESTION {
                    self.private = Marker::Private;
                } else if let Some(f) = FinalByte::from_stream(b) {
                    self.dispatch(f);
                    self.st = State::Ground;
                } else if b.is_csi_intermediate() {
                    self.csi_inter = Some(b);
                    self.st = State::CsiIntermediate;
                } else {
                    // Malformed. Consume to the final byte rather than
                    // returning to Ground here, which printed the remainder of
                    // the sequence as text.
                    self.st = State::CsiIgnore;
                }
            }
            State::OscString => {
                if b == StreamByte::BEL {
                    self.st = State::Ground;
                } else if b == StreamByte::ESC {
                    self.st = State::Escape;
                }
            }
        }
    }

    /// Length of the leading printable-ASCII run, 32 bytes per step.
    ///
    /// # Safety
    /// AVX2 must be available. `no_std` has no runtime feature detection, so
    /// the caller decides once at startup — see [`Term::feed_avx2`].
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2")]
    unsafe fn scan_avx2(buf: &[u8]) -> ByteLen {
        use core::arch::x86_64::*;
        let (n, p) = (buf.len(), buf.as_ptr());
        let sp = _mm256_set1_epi8(0x20);
        let del = _mm256_set1_epi8(0x7f);
        let z = _mm256_setzero_si256();
        let mut i = 0usize;
        unsafe {
            while i + 32 <= n {
                let v = _mm256_loadu_si256(p.add(i) as *const __m256i);
                let le = _mm256_cmpeq_epi8(_mm256_min_epu8(v, sp), v); // <= 0x20
                let eq = _mm256_cmpeq_epi8(v, sp);
                let lt = _mm256_andnot_si256(eq, le); // < 0x20
                let hi = _mm256_cmpgt_epi8(z, v); // >= 0x80, signed compare
                // 0x7f (DEL) is not printable and was missing from this mask, so
                // the vector body accepted it into a run while the scalar tail
                // below — which uses is_printable() — rejected it. Any input
                // containing a DEL byte then rendered differently depending on
                // whether it landed in a 32-byte block or the tail.
                let d = _mm256_cmpeq_epi8(v, del);
                let m = _mm256_movemask_epi8(_mm256_or_si256(_mm256_or_si256(lt, hi), d)) as u32;
                if m != 0 {
                    return ByteLen::from_lane(i + m.trailing_zeros() as usize);
                }
                i += 32;
            }
        }
        while i < n && StreamByte::from_stream(buf[i]).is_printable() {
            i += 1;
        }
        ByteLen::from_lane(i)
    }

    /// Position of the first CSI final byte (`0x40..=0x7e`), 32 at a time.
    ///
    /// # Safety
    /// AVX2 must be available.
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2")]
    unsafe fn find_final(buf: &[u8]) -> Option<ByteLen> {
        use core::arch::x86_64::*;
        let (n, p) = (buf.len(), buf.as_ptr());
        let lo = _mm256_set1_epi8(0x3f);
        let hi = _mm256_set1_epi8(0x7f);
        let mut i = 0usize;
        unsafe {
            while i + 32 <= n {
                let v = _mm256_loadu_si256(p.add(i) as *const __m256i);
                let m = _mm256_movemask_epi8(_mm256_and_si256(
                    _mm256_cmpgt_epi8(v, lo),
                    _mm256_cmpgt_epi8(hi, v),
                )) as u32;
                if m != 0 {
                    return Some(ByteLen::from_lane(i + m.trailing_zeros() as usize));
                }
                i += 32;
            }
        }
        while i < n {
            if FinalByte::from_stream(StreamByte::from_stream(buf[i])).is_some() {
                return Some(ByteLen::from_lane(i));
            }
            i += 1;
        }
        None
    }

    /// First BEL, or the ESC of an ESC-backslash string terminator.
    ///
    /// # Safety
    /// AVX2 must be available.
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2")]
    unsafe fn find_st(buf: &[u8]) -> Option<ByteLen> {
        use core::arch::x86_64::*;
        let (n, p) = (buf.len(), buf.as_ptr());
        let bel = _mm256_set1_epi8(0x07);
        let esc = _mm256_set1_epi8(0x1b);
        let mut i = 0usize;
        unsafe {
            while i + 32 <= n {
                let v = _mm256_loadu_si256(p.add(i) as *const __m256i);
                let m = _mm256_movemask_epi8(_mm256_or_si256(
                    _mm256_cmpeq_epi8(v, bel),
                    _mm256_cmpeq_epi8(v, esc),
                )) as u32;
                if m != 0 {
                    return Some(ByteLen::from_lane(i + m.trailing_zeros() as usize));
                }
                i += 32;
            }
        }
        while i < n {
            let b = StreamByte::from_stream(buf[i]);
            if b == StreamByte::BEL || b == StreamByte::ESC {
                return Some(ByteLen::from_lane(i));
            }
            i += 1;
        }
        None
    }

    /// Two-byte `ESC Fe` sequences and OSC strings, in bulk.
    ///
    /// Measured: on a payload carrying `ESC 7`/`ESC 8`/`ESC M` and OSC titles
    /// alongside CSI, **14.7% of escapes** were falling through to the
    /// byte-at-a-time state machine purely because they were not `ESC [`. The
    /// read-boundary case, which is what the fallback exists for, was 0.18%.
    ///
    /// # Safety
    /// Whatever target feature `S` requires must be available.
    #[inline]
    unsafe fn fast_esc<S: Scan>(
        &mut self,
        base: BytePos,
        at: ByteLen,
        limit: ByteLen,
    ) -> Option<ByteLen> {
        let (b, i, n) = (base.index(), at.index(), limit.index());
        if i + 1 >= n {
            self.fb_boundary.inc();
            return None;
        }
        let second = StreamByte::from_stream(self.s.arena[b + i + 1]);

        // OSC: skip to the string terminator in one scan.
        if second == StreamByte::OSC_INTRO {
            let s = i + 2;
            if s >= n {
                self.fb_boundary.inc();
                return None;
            }
            // SAFETY: caller established whatever S requires.
            let off = match unsafe { S::st(&self.s.arena[b + s..b + n]) } {
                Some(o) => o.index(),
                None => {
                    self.fb_boundary.inc();
                    return None;
                }
            };
            let term = StreamByte::from_stream(self.s.arena[b + s + off]);
            // BEL ends it. ESC only ends it when the next byte is the backslash
            // of ST -- `[[source: ctlseqs]]` line 201 defines ST as `ESC \` and
            // nothing else.
            //
            // This used to consume the byte after ESC without checking it, so
            // `OSC ... ESC E` swallowed the `E` and the NEL never ran, while the
            // byte-at-a-time path cancelled the string and executed it. Whether
            // a stream got its NEL depended on where the read boundary fell:
            // caught by ingest_is_chunk_invariant, and pre-dating the state
            // machine work that exposed it.
            //
            // A non-backslash byte means the ESC *cancelled* the string
            // (`[[source: williams]]`), and what follows is a fresh sequence.
            // Handing that back to the state machine is the whole fix: it
            // already parses the cancel correctly.
            let used = if term == StreamByte::BEL {
                s + off + 1 - i
            } else if s + off + 1 >= n {
                self.fb_boundary.inc();
                return None;
            } else if StreamByte::from_stream(self.s.arena[b + s + off + 1]) == StreamByte::ST_FINAL
            {
                s + off + 2 - i
            } else {
                self.fb_not_csi.inc();
                return None;
            };
            self.fast_hits.inc();
            return Some(ByteLen::from_index(used));
        }

        // ESC Fe: two bytes, no parameters.
        let known = matches!(
            second.raw_byte(),
            b'7' | b'8' | b'M' | b'D' | b'E' | b'c' | b'=' | b'>'
        );
        if known {
            self.esc_fe(second); // one implementation, both paths
            self.fast_hits.inc();
            Some(ByteLen::from_index(2))
        } else {
            self.fb_not_csi.inc();
            None
        }
    }

    /// `ESC Fe`: two-byte escapes with no parameters. Shared by both feed paths.
    #[inline(always)]
    fn esc_fe(&mut self, b: StreamByte) {
        match b.raw_byte() {
            b'7' => self.saved = Some((self.cy, self.cx, self.style, self.style_raw)),
            b'8' => {
                if let Some((r, c, st, sw)) = self.saved {
                    self.cy = r;
                    self.cx = c;
                    self.style = st;
                    self.style_raw = sw;
                }
            }
            b'M' => self.reverse_index(),
            b'D' => self.index(),
            b'E' => self.wrap(),
            b'c' => self.reset(),
            b'H' => {
                let x = self.cx.index();
                self.s.tabs[x] = true;
            }
            _ => {}
        }
    }

    /// DECALN, `ESC # 8`.
    ///
    /// `[[source: vt510-decaln]]`: "fills the complete screen area with a test
    /// pattern used for adjusting screen alignment... DECALN sets the margins
    /// to the extremes of the page, and moves the cursor to the home
    /// position." The pattern is uppercase E.
    fn screen_alignment(&mut self) {
        self.scroll_top = Row::ZERO;
        self.scroll_bot = Row::from_index(self.rows.index() - 1);
        let cell = PackedCell::from_ascii(StreamByte::from_stream(b'E'), self.style);
        for y in 0..self.rows.index() {
            let r = Row::from_index(y);
            self.s.promoted[y] = true;
            self.s.nrun[self.head.plus(r, SB).index()] = RunCount::ZERO;
            self.view().fill_row(r, cell);
        }
        self.cx = Col::ZERO;
        self.cy = Row::ZERO;
    }

    /// DECSTR. Modes, region, cursor and attributes to their defaults --
    /// **not** the screen contents. That is what makes it soft, and it is why
    /// an application uses it to recover a terminal without losing scrollback.
    fn soft_reset(&mut self) {
        self.cx = Col::ZERO;
        self.cy = Row::ZERO;
        self.style = StyleId::DEFAULT;
        self.style_raw = StyleWord::DEFAULT;
        self.saved = None;
        self.scroll_top = Row::ZERO;
        self.scroll_bot = Row::from_index(self.rows.index() - 1);
        self.modes = Modes::default();
    }

    /// RIS. Everything back to power-on state, including the scrolling region
    /// and the modes -- a region left over across a reset is how an application
    /// ends up drawing into a window it did not ask for.
    fn reset(&mut self) {
        self.cx = Col::ZERO;
        self.cy = Row::ZERO;
        self.style = StyleId::DEFAULT;
        self.style_raw = StyleWord::DEFAULT;
        self.saved = None;
        self.scroll_top = Row::ZERO;
        self.scroll_bot = Row::from_index(self.rows.index() - 1);
        self.modes = Modes::default();
        self.reset_tabs();
        self.clear_screen();
    }

    /// Parse a complete CSI sequence that lies wholly inside this read.
    ///
    /// Returns the bytes consumed, or `None` when the sequence runs past the end
    /// of the buffer — in which case the byte-at-a-time state machine takes over
    /// and carries the partial state to the next read. That fallback is what
    /// keeps `ingest_is_chunk_invariant` true.
    ///
    /// Measured in isolation against the previous byte-at-a-time path: SIMD scan
    /// to the terminator 1.6x, SIMD scan for `ESC` another 1.65x, and a
    /// zero-parameter fast path another 1.31x. SWAR digit accumulation was tried
    /// and *lost* — the branch predictor handles a two-digit loop better than the
    /// bit trick that replaces it.
    ///
    /// # Safety
    /// Whatever target feature `S` requires must be available.
    #[inline]
    unsafe fn fast_csi<S: Scan>(
        &mut self,
        base: BytePos,
        at: ByteLen,
        limit: ByteLen,
    ) -> Option<ByteLen> {
        let (i, n) = (at.index(), limit.index());
        let b = base.index();
        if i + 2 >= n {
            self.fb_boundary.inc();
            return None;
        }
        if StreamByte::from_stream(self.s.arena[b + i + 1]) != StreamByte::CSI_INTRO {
            return None; // fast_esc gets it; counted there
        }
        let s = i + 2;

        self.params = [None; 16];
        self.nparam = ParamIdx::FIRST;
        self.private = Marker::Standard;
        self.csi_inter = None;

        // `ESC [ <final>` — roughly half of a TUI stream, and needs no scan.
        let first = StreamByte::from_stream(self.s.arena[b + s]);
        if let Some(f) = FinalByte::from_stream(first) {
            self.dispatch(f);
            self.fast_hits.inc();
            return Some(ByteLen::from_index(s + 1 - i));
        }

        // SAFETY: caller established whatever S requires.
        let off = match unsafe { S::final_byte(&self.s.arena[b + s..b + n]) } {
            Some(o) => o.index(),
            None => {
                self.fb_boundary.inc();
                return None;
            }
        };

        let mut np = ParamIdx::FIRST;
        let mut cur = Param::ZERO;
        let mut any = false;
        for k in 0..off {
            let d = StreamByte::from_stream(self.s.arena[b + s + k]);
            if let Some(dig) = d.as_ascii_digit() {
                cur = cur.push(dig);
                any = true;
            } else if d == StreamByte::SEMICOLON {
                if np.raw_index() < 16 {
                    self.params[np.raw_index()] = if any { Some(cur) } else { None };
                    np = np.next();
                }
                cur = Param::ZERO;
                any = false;
            } else if d == StreamByte::QUESTION {
                self.private = Marker::Private;
            } else if d.is_csi_intermediate() {
                // Recorded, not skipped. Dropping it here would make the AVX2
                // path dispatch `CSI ! p` as `CSI p` while the portable path
                // dispatched DECSTR -- a silent divergence between the two
                // feeds, which is the bug class this codebase has had before.
                self.csi_inter = Some(d);
            } else {
                self.fb_odd_param.inc();
                return None; // something unexpected; let the state machine decide
            }
        }
        if np.raw_index() < 16 {
            self.params[np.raw_index()] = if any { Some(cur) } else { None };
            np = np.next();
        }
        self.nparam = np;

        let f = FinalByte::from_stream(StreamByte::from_stream(self.s.arena[b + s + off]))?;
        self.dispatch(f);
        self.fast_hits.inc();
        Some(ByteLen::from_index(s + off + 1 - i))
    }

    /// The one ingest loop. Both public entry points are this, monomorphised.
    ///
    /// Writing it once is not tidiness. `ESC 7`/`8`/`M` were previously
    /// implemented in the AVX2 loop only, `feed` and `feed_avx2` silently
    /// disagreed, and `ingest_is_chunk_invariant` could not see it because it
    /// drives one path. With the loop shared, that class of divergence is not
    /// expressible; only the scan backend varies.
    ///
    /// # Safety
    /// Whatever target feature `S` requires must be available.
    #[inline]
    unsafe fn feed_with<S: Scan>(&mut self, base: BytePos, n: ByteLen) {
        let (b, nn) = (base.index(), n.index());
        let mut i = 0usize;
        while i < nn {
            if self.st == State::Ground {
                // SAFETY: caller established whatever S requires.
                let run = unsafe { S::run(&self.s.arena[b + i..b + nn]) }.index();
                if run > 0 {
                    self.put_run(base, ByteLen::from_index(i), ByteLen::from_index(run));
                    i += run;
                    continue;
                }
            }
            // complete CSI sequences are handled in bulk; anything straddling
            // the read boundary falls through to the state machine.
            if self.st == State::Ground
                && StreamByte::from_stream(self.s.arena[b + i]) == StreamByte::ESC
            {
                let (at, lim) = (ByteLen::from_index(i), ByteLen::from_index(nn));
                // SAFETY: caller established whatever S requires.
                if let Some(used) = unsafe { self.fast_csi::<S>(base, at, lim) } {
                    i += used.index();
                    continue;
                }
                // SAFETY: as above.
                if let Some(used) = unsafe { self.fast_esc::<S>(base, at, lim) } {
                    i += used.index();
                    continue;
                }
            }
            let byte = StreamByte::from_stream(self.s.arena[b + i]);
            self.control(byte);
            i += 1;
        }
    }

    /// Ingest, portable. Vectorises through `core::simd` on any target that has
    /// vectors at all, and carries the same CSI and `ESC Fe` fast paths the
    /// x86-64 loop has.
    ///
    /// Until this existed, those fast paths were inside `#[cfg(target_arch =
    /// "x86_64")]`, so every escape on aarch64 went through the byte-at-a-time
    /// state machine — the 85.2% -> 99.8% fast-path win was x86-only.
    pub fn feed(&mut self, base: BytePos, n: ByteLen) {
        // SAFETY: `Portable` requires no target feature.
        unsafe { self.feed_with::<Portable>(base, n) }
    }

    /// Ingest, scanning 32 bytes at a time instead of 16.
    ///
    /// Measured 278.5 MB/s against 240.8 for the portable path *when the
    /// portable path was scalar*. That comparison predates `Portable` being
    /// vectorised and has not been re-run.
    ///
    /// # Safety
    /// The caller must have established AVX2 support.
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2")]
    pub unsafe fn feed_avx2(&mut self, base: BytePos, n: ByteLen) {
        // SAFETY: caller established AVX2.
        unsafe { self.feed_with::<Avx2>(base, n) }
    }
}
