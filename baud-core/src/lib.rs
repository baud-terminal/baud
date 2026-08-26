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

#[cfg(feature = "alloc")]
extern crate alloc;

pub use baud_domain::{
    ByteLen, BytePos, Col, Cols, Count, Csi, Encoding, EraseExtent, FinalByte, Marker,
    Content, Filled, PackedCell, Param, ParamIdx, Params, RingPos, Room, Row, Rows, Run, RunCount,
    ArenaTail, Codepoint, LineSlot, Pending, Slot, Stat, StreamByte, StyleId, StyleWord,
    Utf8Acc,
    View, Width, WinSize,
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
pub const NSTYLE: usize = 64;
pub const READ_CHUNK: usize = baud_domain::SLOT_BYTES;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpMode { Computed, Templated, Runs }

/// Outcome of a [`Term::resize`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[must_use]
pub enum Resized {
    Ok,
    /// The window is larger than the fixed viewport. The grid was clamped to
    /// the geometry given; the caller should surface this rather than render a
    /// terminal that disagrees with the application about its own size.
    Truncated { cols: Cols, rows: Rows },
}

#[repr(C)]
#[cfg_attr(feature = "alloc", derive(zerocopy::FromZeros, zerocopy::KnownLayout, zerocopy::Immutable))]
pub struct Storage {
    pub arena: [u8; ARENA],
    pub runs: [Run; SB * MAXRUN],
    pub nrun: [RunCount; SB],
    pub vp: [PackedCell; MAX_ROWS * MAX_COLS],
    pub promoted: [bool; MAX_ROWS],
    pub tmpl: [PackedCell; NSTYLE * MAX_COLS],
    pub tmpl_key: [StyleWord; NSTYLE],
    pub tmpl_valid: [bool; NSTYLE],
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
        const R: Run = Run { pos: BytePos::ZERO, len: ByteLen::ZERO,
                             style: StyleId::DEFAULT, encoding: 0, _pad: [0; 3] };
        const C: PackedCell = PackedCell::EMPTY;
        Storage { arena: [0; ARENA], runs: [R; SB * MAXRUN], nrun: [RunCount::ZERO; SB],
                  vp: [C; MAX_ROWS * MAX_COLS], promoted: [false; MAX_ROWS],
                  tmpl: [C; NSTYLE * MAX_COLS], tmpl_key: [StyleWord::DEFAULT; NSTYLE],
                  tmpl_valid: [false; NSTYLE] }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Ground, Escape, CsiParams, OscString,
    /// Mid multi-byte sequence: continuation bytes remaining, and bits so far.
    Utf8 { remaining: Pending, acc: Utf8Acc },
}

pub struct Term<'a> {
    s: &'a mut Storage,
    tail: ArenaTail, head: RingPos,
    cx: Col, cy: Row, cols: Cols, rows: Rows,
    st: State, params: [Option<Param>; 16], nparam: ParamIdx, private: Marker,
    style: StyleId, style_raw: StyleWord,
    saved: Option<(Row, Col, StyleId, StyleWord)>,
    mode: OpMode,
    pub promotions: Stat,
    pub tmpl_builds: Stat,
    pub tmpl_hits: Stat,
    pub maxrun_overflows: Stat,
    pub replacements: Stat,
    // why the fast CSI path declined, so the fallback rate is measurable
    pub fast_hits: Stat,
    pub fb_not_csi: Stat,
    pub fb_boundary: Stat,
    pub fb_odd_param: Stat,
}

impl<'a> Term<'a> {
    pub fn new(s: &'a mut Storage, mode: OpMode) -> Self {
        let (cols, rows) = WinSize { ws_row: 50, ws_col: 200 }.geometry();
        let mut t = Term { s, tail: ArenaTail::ZERO, head: RingPos::ZERO, cx: Col::ZERO, cy: Row::ZERO,
               cols: cols.clamp(MAX_COLS), rows: rows.clamp(MAX_ROWS),
               st: State::Ground, params: [None; 16], nparam: ParamIdx::FIRST, private: Marker::Standard,
               style: StyleId::DEFAULT, style_raw: StyleWord::DEFAULT, saved: None, mode,
               promotions: Stat::ZERO, tmpl_builds: Stat::ZERO, tmpl_hits: Stat::ZERO,
               maxrun_overflows: Stat::ZERO, replacements: Stat::ZERO,
               fast_hits: Stat::ZERO, fb_not_csi: Stat::ZERO,
               fb_boundary: Stat::ZERO, fb_odd_param: Stat::ZERO };
        t.clear_screen();
        t
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
    #[must_use]
    pub fn resize(&mut self, ws: &WinSize) -> Resized {
        let (c, r) = ws.geometry();
        let over = c.index() > MAX_COLS || r.index() > MAX_ROWS;
        self.cols = c.clamp(MAX_COLS);
        self.rows = r.clamp(MAX_ROWS);
        self.cx = Col::ZERO; self.cy = Row::ZERO;
        self.clear_screen();
        if over { Resized::Truncated { cols: self.cols, rows: self.rows } }
        else { Resized::Ok }
    }

    pub fn cursor(&self) -> (Row, Col) { (self.cy, self.cx) }
    pub fn geometry(&self) -> (Cols, Rows) { (self.cols, self.rows) }
    pub fn stride(&self) -> Cols { Cols::from_stride(MAX_COLS) }
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
    pub fn commit_read(&mut self, n: ByteLen) { self.tail = self.tail.advance(n); }

    #[inline(always)] fn line(&self) -> LineSlot { LineSlot::of(self.head.plus(self.cy, SB)) }

    #[inline(always)]
    fn newline(&mut self) {
        match self.cy.next(self.rows) {
            Some(r) => self.cy = r,
            None => {
                self.head = self.head.advance(SB);
                let last = self.head.plus(Row::from_index(self.rows.index() - 1), SB).index();
                self.s.nrun[last] = RunCount::ZERO;
                self.s.promoted[self.rows.index() - 1] = false;
                let b = PackedCell::empty_with(self.style);
                let y = Row::from_index(self.rows.index() - 1);
                self.view().fill_row(y, b);
            }
        }
        self.cx = Col::ZERO;
        let i = self.line();
        self.s.nrun[i.index()] = RunCount::ZERO;
    }

    #[inline(always)]
    fn intern_style(&mut self, raw: StyleWord) -> StyleId {
        let k = StyleId::bucket(raw, NSTYLE);
        if self.s.tmpl_valid[k] && self.s.tmpl_key[k] == raw { self.tmpl_hits.inc(); }
        else {
            self.s.tmpl_valid[k] = true; self.s.tmpl_key[k] = raw; self.tmpl_builds.inc();
            let c = PackedCell::empty_with(StyleId::from_index(k));
            let a = k * MAX_COLS;
            self.s.tmpl[a..a + MAX_COLS].fill(c);
        }
        StyleId::from_index(k)
    }

    #[inline(always)]
    fn view(&mut self) -> View<'_> {
        View::new(&mut self.s.vp, Cols::from_stride(MAX_COLS), self.cols, self.rows)
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
        if w == Width::ZERO { return; }
        if self.cx.try_advance(w, self.cols).is_none() { self.newline(); }
        self.promote_if_runs();
        self.write_cell(cp);
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
        View::new(vp, stride, cols, rows)
            .put_ascii_run(r, c, &arena[abs..abs + room.index()], st);
    }

    #[inline(always)]
    fn put_run(&mut self, base: BytePos, mut off: ByteLen, mut n: ByteLen) {
        while !n.is_zero() {
            let mut room = self.cols.room_at(self.cx).min(n);
            if room.is_empty() { self.newline(); room = self.cols.room_at(self.cx).min(n); }
            let promoted = self.s.promoted[self.cy.index()];
            if self.mode == OpMode::Runs && !promoted {
                let i = self.line();
                let c = self.s.nrun[i.index()];
                if c.index() < MAXRUN {
                    self.s.runs[i.run(c, MAXRUN)] = Run {
                        pos: base.advance(off), len: ByteLen::from_room(room),
                        style: self.style, encoding: 0, _pad: [0; 3] }
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
            self.cx += room;
            off = off.add(ByteLen::from_room(room));
            n = n.sub_room(room);
        }
    }

    fn promote(&mut self) {
        let y = self.cy.index();
        if self.s.promoted[y] { return; }
        let i = self.line();
        let n = self.s.nrun[i.index()].index();
        let mut c = 0usize;
        for r in 0..n {
            let run = self.s.runs[i.run(RunCount::from_index(r), MAXRUN)];
            let (o, l) = (run.pos.index(), run.len.index());
            for z in 0..l {
                if c >= self.cols.index() { break; }
                let b = StreamByte::from_stream(self.s.arena[o + z]);
                let (rr, cc) = (Row::from_index(y), Col::at_index(c));
                self.view().put(rr, cc, PackedCell::from_ascii(b, run.style));
                c += 1;
            }
        }
        let blank = PackedCell::empty_with(self.style);
        let (rr, from, to) = (Row::from_index(y), Col::at_index(c), Col::at_index(self.cols.index()));
        self.view().fill(rr, from, to, blank);
        self.s.promoted[y] = true;
        self.promotions.inc();
    }

    #[inline(always)]
    fn promote_if_runs(&mut self) { if self.mode == OpMode::Runs { self.promote(); } }

    fn erase_line(&mut self, e: EraseExtent) {
        let y = self.cy.index();
        if self.mode == OpMode::Runs && !self.s.promoted[y] && e == EraseExtent::ToEnd {
            let i = self.line();
            self.s.nrun[i.index()] = RunCount::ZERO;
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

    fn insert_lines(&mut self, n: Count) {
        let (cy, rows) = (self.cy.index(), self.rows.index());
        let k = n.at_least_one().min(rows - cy);
        let (at, b) = (self.cy, PackedCell::empty_with(self.style));
        self.view().insert_lines(at, n, b);
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
        match Csi::parse(marker, p, f) {
            Csi::CursorPosition { row, col } => {
                self.cy = row.to_row(self.rows); self.cx = col.to_col(self.cols);
            }
            Csi::CursorUp(c) => self.cy = self.cy.up(c),
            Csi::CursorDown(c) => self.cy = self.cy.down(c, self.rows),
            Csi::CursorForward(c) => self.cx = self.cx.forward(c, self.cols),
            Csi::CursorBack(c) => self.cx = self.cx.back(c),
            Csi::EraseInLine(e) => self.erase_line(e),
            Csi::EraseInDisplay(e) => self.erase_display(e),
            Csi::EraseChars(c) => self.erase_chars(c),
            Csi::DeleteChars(c) => self.delete_chars(c),
            Csi::InsertBlanks(c) => self.insert_blanks(c),
            Csi::InsertLines(c) => self.insert_lines(c),
            Csi::Sgr => self.apply_sgr(),
            _ => {}
        }
    }

    #[inline(always)]
    fn control(&mut self, b: StreamByte) {
        match self.st {
            State::Ground => {
                if b == StreamByte::LF { self.newline(); }
                else if b == StreamByte::CR { self.cx = Col::ZERO; }
                else if b == StreamByte::ESC { self.st = State::Escape; }
                else if b == StreamByte::BS { self.cx = self.cx.back(Count::ONE); }
                else if b == StreamByte::TAB { self.cx = self.cx.next_tab(self.cols); }
                else if let Some((rem, acc)) = b.utf8_lead() {
                    self.st = State::Utf8 { remaining: Pending::new(rem), acc: Utf8Acc::lead(acc) };
                } else if !b.is_c0() {
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
                        self.st = State::Utf8 { remaining: remaining.dec(), acc };
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
                    self.params = [None; 16]; self.nparam = ParamIdx::FIRST; self.private = Marker::Standard;
                    self.st = State::CsiParams;
                } else if b == StreamByte::OSC_INTRO {
                    self.st = State::OscString;
                } else {
                    // Same two-byte escapes the AVX2 path handles. The two feed
                    // paths must be indistinguishable; `feed_matches_feed_avx2`
                    // in the property suite asserts it.
                    self.esc_fe(b);
                    self.st = State::Ground;
                }
            }
            State::CsiParams => {
                if let Some(d) = b.as_ascii_digit() {
                    if self.nparam.is_first() { self.nparam = self.nparam.next(); }
                    let cur = self.params[self.nparam.raw_index() - 1].unwrap_or(Param::ZERO);
                    self.params[self.nparam.raw_index() - 1] = Some(cur.push(d));
                } else if b == StreamByte::SEMICOLON {
                    if self.nparam.is_first() { self.nparam = self.nparam.next(); }
                    if self.nparam.raw_index() < 16 { self.params[self.nparam.raw_index()] = None; self.nparam = self.nparam.next(); }
                } else if b == StreamByte::QUESTION {
                    self.private = Marker::Private;
                } else if let Some(f) = FinalByte::from_stream(b) {
                    self.dispatch(f); self.st = State::Ground;
                } else if !b.is_csi_intermediate() {
                    self.st = State::Ground;
                }
            }
            State::OscString => {
                if b == StreamByte::BEL { self.st = State::Ground; }
                else if b == StreamByte::ESC { self.st = State::Escape; }
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
                let m = _mm256_movemask_epi8(
                    _mm256_or_si256(_mm256_or_si256(lt, hi), d)) as u32;
                if m != 0 { return ByteLen::from_lane(i + m.trailing_zeros() as usize); }
                i += 32;
            }
        }
        while i < n && StreamByte::from_stream(buf[i]).is_printable() { i += 1; }
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
                    _mm256_cmpgt_epi8(v, lo), _mm256_cmpgt_epi8(hi, v))) as u32;
                if m != 0 { return Some(ByteLen::from_lane(i + m.trailing_zeros() as usize)); }
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
                    _mm256_cmpeq_epi8(v, bel), _mm256_cmpeq_epi8(v, esc))) as u32;
                if m != 0 { return Some(ByteLen::from_lane(i + m.trailing_zeros() as usize)); }
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
    /// AVX2 must be available.
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2")]
    unsafe fn fast_esc(&mut self, base: BytePos, at: ByteLen, limit: ByteLen)
        -> Option<ByteLen>
    {
        let (b, i, n) = (base.index(), at.index(), limit.index());
        if i + 1 >= n { self.fb_boundary.inc(); return None; }
        let second = StreamByte::from_stream(self.s.arena[b + i + 1]);

        // OSC: skip to the string terminator in one scan.
        if second == StreamByte::OSC_INTRO {
            let s = i + 2;
            if s >= n { self.fb_boundary.inc(); return None; }
            // SAFETY: caller established AVX2.
            let off = match unsafe { Self::find_st(&self.s.arena[b + s..b + n]) } {
                Some(o) => o.index(),
                None => { self.fb_boundary.inc(); return None; }
            };
            let term = StreamByte::from_stream(self.s.arena[b + s + off]);
            // BEL ends it; ESC needs the following byte to confirm ESC-backslash
            let used = if term == StreamByte::BEL { s + off + 1 - i }
                       else if s + off + 1 < n { s + off + 2 - i }
                       else { self.fb_boundary.inc(); return None; };
            self.fast_hits.inc();
            return Some(ByteLen::from_index(used));
        }

        // ESC Fe: two bytes, no parameters.
        let known = matches!(second.raw_byte(),
                             b'7' | b'8' | b'M' | b'D' | b'E' | b'c' | b'=' | b'>');
        if known {
            self.esc_fe(second);          // one implementation, both paths
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
                    self.cy = r; self.cx = c; self.style = st; self.style_raw = sw;
                }
            }
            b'M' => {
                if self.cy.index() == 0 { self.scroll_down(); }
                else { self.cy = self.cy.up(Count::ONE); }
            }
            b'D' => self.newline(),
            b'E' => { self.newline(); self.cx = Col::ZERO; }
            b'c' => self.reset(),
            _ => {}
        }
    }

    /// Scroll the viewport down one row, opening a blank line at the top.
    fn scroll_down(&mut self) {
        let b = PackedCell::empty_with(self.style);
        let rows = self.rows.index();
        let mut y = rows;
        while y > 1 {
            y -= 1;
            for x in 0..self.cols.index() {
                let (src, dst) = (Row::from_index(y - 1), Row::from_index(y));
                let cell = self.view().get(src, Col::at_index(x));
                self.view().put(dst, Col::at_index(x), cell);
            }
        }
        self.view().fill_row(Row::ZERO, b);
    }

    fn reset(&mut self) {
        self.cx = Col::ZERO;
        self.cy = Row::ZERO;
        self.style = StyleId::DEFAULT;
        self.style_raw = StyleWord::DEFAULT;
        self.saved = None;
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
    /// AVX2 must be available.
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2")]
    unsafe fn fast_csi(&mut self, base: BytePos, at: ByteLen, limit: ByteLen)
        -> Option<ByteLen>
    {
        let (i, n) = (at.index(), limit.index());
        let b = base.index();
        if i + 2 >= n { self.fb_boundary.inc(); return None; }
        if StreamByte::from_stream(self.s.arena[b + i + 1]) != StreamByte::CSI_INTRO {
            return None; // fast_esc gets it; counted there
        }
        let s = i + 2;

        self.params = [None; 16];
        self.nparam = ParamIdx::FIRST;
        self.private = Marker::Standard;

        // `ESC [ <final>` — roughly half of a TUI stream, and needs no scan.
        let first = StreamByte::from_stream(self.s.arena[b + s]);
        if let Some(f) = FinalByte::from_stream(first) {
            self.dispatch(f);
            self.fast_hits.inc();
            return Some(ByteLen::from_index(s + 1 - i));
        }

        // SAFETY: caller established AVX2.
        let off = match unsafe { Self::find_final(&self.s.arena[b + s..b + n]) } {
            Some(o) => o.index(),
            None => { self.fb_boundary.inc(); return None; }
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
            } else if !d.is_csi_intermediate() {
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

    /// Same as [`Term::feed`], scanning 32 bytes at a time.
    ///
    /// Measured 278.5 MB/s against 240.8 for the portable path. The port to the
    /// domain types dropped this method entirely and nothing noticed, because
    /// the benchmark was still *printing* "scan path: AVX2" from a stale flag.
    ///
    /// # Safety
    /// The caller must have established AVX2 support.
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2")]
    pub unsafe fn feed_avx2(&mut self, base: BytePos, n: ByteLen) {
        let (b, nn) = (base.index(), n.index());
        let mut i = 0usize;
        while i < nn {
            if self.st == State::Ground {
                // SAFETY: caller established AVX2.
                let run = unsafe { Self::scan_avx2(&self.s.arena[b + i..b + nn]) }.index();
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
                // SAFETY: caller established AVX2.
                let (at, lim) = (ByteLen::from_index(i), ByteLen::from_index(nn));
                // SAFETY: caller established AVX2.
                if let Some(used) = unsafe { self.fast_csi(base, at, lim) } {
                    i += used.index();
                    continue;
                }
                // SAFETY: as above.
                if let Some(used) = unsafe { self.fast_esc(base, at, lim) } {
                    i += used.index();
                    continue;
                }
            }
            let byte = StreamByte::from_stream(self.s.arena[b + i]);
            self.control(byte);
            i += 1;
        }
    }

    pub fn feed(&mut self, base: BytePos, n: ByteLen) {
        let (b, nn) = (base.index(), n.index());
        let mut i = 0usize;
        while i < nn {
            if self.st == State::Ground {
                let mut run = 0usize;
                while i + run < nn {
                    if !StreamByte::from_stream(self.s.arena[b + i + run]).is_printable() { break; }
                    run += 1;
                }
                if run > 0 {
                    self.put_run(base, ByteLen::from_index(i), ByteLen::from_index(run));
                    i += run; continue;
                }
            }
            let byte = StreamByte::from_stream(self.s.arena[b + i]);
            self.control(byte);
            i += 1;
        }
    }
}
