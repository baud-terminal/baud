//! What the route mask names: the writers, and the byte path behind them.

use crate::storage::{MAX_COLS, STRIDE_MAX, Storage};
use baud_domain::*;

impl crate::term::Term<'_> {
    /// A run of printable ASCII, blitted.
    ///
    /// Eight cells to a `Simd<u64,8>` store, which is why the loop above works
    /// in runs rather than characters: the writer's cost is per *store*, and a
    /// run of one wastes seven eighths of it.
    #[inline(always)]
    pub(crate) fn put_text(&mut self, base: BytePos, at: ByteLen, n: ByteLen) {
        let from = base.advance(at).index();
        let (r, c) = (self.cy, self.cx);
        let (style, flags, set) = (self.style, self.flags, self.charset);
        let k = n.index().min(self.cols.index().saturating_sub(c.index()));
        if k == 0 {
            return;
        }
        // **Disjoint field borrows, not a copy.** The source is in the arena
        // and the destination is the cell array; both live in `Storage`, so
        // destructuring the one `&mut` is what lets a run be blitted in place.
        // Copying it to the stack first would put a `memcpy` on every run.
        let Storage { arena, vp, used } = &mut self.store;
        #[cfg(feature = "ceiling-no-write")]
        {
            let _ = (arena, vp, used, from, r, style, flags, set);
            self.cells_written += k;
            self.cx = Col::at_index(c.index() + k);
            return;
        }
        #[allow(unreachable_code)]
        let src = &arena[from..from + k];
        let mut v = View::new(
            vp,
            used,
            Cols::from_stride(MAX_COLS),
            self.cols,
            self.rows,
            self.rot,
        );
        v.put_ascii_run(r, c, src, style, flags, set);
        self.cells_written += k;
        self.cx = Col::at_index(c.index() + k);
    }

    /// A run of `k` three-byte characters, decoded and written in one pass.
    ///
    /// **Decoded first, then written by whichever writer fits.** A stride run
    /// is a run of one *encoded length*, which is a far weaker condition than
    /// a run of one character class -- but the vectored writers want one
    /// *width*, and that is a question about the decoded codepoints rather
    /// than about the cluster machine. Asking it after the decode costs five
    /// compares and lets `put_wide_run` write eight cells to one
    /// `Simd<u64,8>` store; asking it before, as a gate on parser state, is
    /// what refused 92% of candidates in the previous core.
    ///
    /// Each character is `1110xxxx 10yyyyyy 10zzzzzz` and the lead's position
    /// is known from the stride, so there is no boundary search: the offsets
    /// are `3i`, `3i+1`, `3i+2` and the compiler sees constants.
    #[inline(always)]
    pub(crate) fn put_run_of(&mut self, base: BytePos, at: ByteLen, len: ByteLen, count: Count) {
        let (from, len, k) = (base.advance(at).index(), len.index(), count.index());
        // Eight is all sixteen lanes can hold at the shortest length, so this
        // never spills.
        let mut cps = [Codepoint::REPLACEMENT; STRIDE_MAX];
        let mut all_wide = true;
        for (i, slot) in cps.iter_mut().enumerate().take(k) {
            let s = StreamByte::slice_from(&self.store.arena[from + len * i..from + len * i + len]);
            // **Unchecked on purpose.** The route map already established that
            // `s[0]` leads a `len`-byte character and that every byte after it
            // is a continuation -- one shuffle answered that for all sixteen
            // lanes -- so the checking forms would re-ask a settled question
            // once a byte.
            let mut raw = s[0].lead_bits(len);
            for b in &s[1..] {
                raw = (raw << 6) | b.continuation_bits();
            }
            // The route said the *shape* was right; this says the value is.
            // A non-shortest form is not a character -- `E0 80 AF` renders `/`
            // without this, which is the path-traversal overlong -- and
            // neither are the surrogates or anything past `U+10FFFF`.
            let floor = [0x80u32, 0x800, 0x10000][len - 2];
            // `Codepoint::decoded` is the surrogate and out-of-range test --
            // the only fallible constructor, so those two rules are spelled
            // once in the domain rather than again here. What is left is the
            // shortest-form floor, which depends on the encoded length and so
            // cannot live on the codepoint.
            let cp = match raw < floor {
                true => Codepoint::REPLACEMENT,
                false => Codepoint::decoded(raw),
            };
            // **The table, not four range compares.** `is_wide_ordinary`
            // answers without touching memory and was tried here; it measured
            // **414 against 492**, because it covers CJK, kana and the
            // fullwidth forms and refuses Hangul and everything else the table
            // calls wide. Every character it refuses drops the whole run to
            // the per-character path, and losing the vectored write costs far
            // more than the load it saves. A cheaper predicate that answers a
            // narrower question is not cheaper.
            #[cfg(not(feature = "ceiling-no-classify"))]
            {
                all_wide &= Classified::of(cp).width(AmbiguousWidth::Narrow) == Width::WIDE;
            }
            #[cfg(feature = "ceiling-no-classify")]
            {
                all_wide = false;
            }
            *slot = cp;
        }

        #[cfg(feature = "ceiling-no-write")]
        {
            let _ = &cps;
            self.cells_written += k;
            self.cx = Col::at_index((self.cx.index() + k).min(self.cols.index() - 1));
            return;
        }
        #[allow(unreachable_code)]
        let room = self.cols.index().saturating_sub(self.cx.index());
        if all_wide && k > 0 && 2 * k <= room {
            let (r, c, style, flags) = (self.cy, self.cx, self.style, self.flags);
            let Storage { vp, used, .. } = &mut self.store;
            let mut v = View::new(
                vp,
                used,
                Cols::from_stride(MAX_COLS),
                self.cols,
                self.rows,
                self.rot,
            );
            v.put_wide_run(r, c, &cps[..k], style, flags);
            self.cx = Col::at_index(c.index() + 2 * k);
            self.cells_written += 2 * k;
            return;
        }

        // Mixed widths, or the row is about to end: one at a time, which also
        // handles the wrap.
        for cp in cps.iter().take(k) {
            let w = Classified::of(*cp).width(AmbiguousWidth::Narrow);
            self.put_char(*cp, w);
        }
    }

    /// One decoded character into the cell under the cursor.
    #[inline(always)]
    pub(crate) fn put_char(&mut self, cp: Codepoint, w: Width) {
        let wide = w == Width::WIDE;
        let need = 1 + usize::from(wide);
        if self.cx.index() + need > self.cols.index() {
            self.wrap();
        }
        let (r, c, style, flags) = (self.cy, self.cx, self.style, self.flags);
        let tail = match wide {
            true => Some(PackedCell::wide_tail(style, flags)),
            false => None,
        };
        let blank = PackedCell::empty_with(style);
        self.view()
            .put_glyph(r, c, PackedCell::glyph(cp, style, flags), tail, blank);
        self.cells_written += need;
        self.cx = Col::at_index(c.index() + need);
    }

    /// End of line: to the left margin, and down.
    pub(crate) fn wrap(&mut self) {
        self.cx = Col::ZERO;
        self.index();
    }

    /// Down one row, scrolling when that would leave the screen.
    ///
    /// **The scroll is a rotation.** Advancing `rot` retires the top row in
    /// O(1); the only cells written are the incoming row's, and those stop at
    /// its used extent.
    pub(crate) fn index(&mut self) {
        let last = self.rows.index() - 1;
        if self.cy.index() < last {
            self.cy = Row::from_index(self.cy.index() + 1);
            return;
        }
        self.rot = self.rot.advance(1, self.rows);
        let blank = PackedCell::empty_with(self.style);
        let n = self.view().fill_row(Row::from_index(last), blank);
        self.cells_written += n.index();
    }
}

impl crate::term::Term<'_> {
    /// One byte, off the block path. Returns how many bytes it consumed, which
    /// is **never zero** -- the block loop relies on that to make progress.
    #[inline(always)]
    pub(crate) fn one(&mut self, base: BytePos, at: ByteLen) -> ByteLen {
        let arena = &self.store.arena;
        let from = base.advance(at).index();
        let b = StreamByte::from_stream(arena[from]);

        // Mid-character: this byte continues one, or it does not and the
        // partial character becomes a replacement.
        if !self.node.is_ground() {
            return match b.utf8_continuation() {
                Some(bits) => {
                    self.utf8 = self.utf8.push(bits);
                    self.node = self.node.one_fewer();
                    if self.node.is_ground() {
                        self.finish_char();
                    }
                    ByteLen::from_index(1)
                }
                None => {
                    // `[[source: unicode-ch3]]` 3.9.5: a converter "must not
                    // consume the successor bytes as part of the ill-formed
                    // subsequence whenever those successor bytes themselves
                    // constitute part of a well-formed" one. So the partial
                    // character becomes a replacement and this byte is then
                    // re-dispatched from ground rather than swallowed.
                    self.node = crate::graph::Node::Ground;
                    self.put_char(Codepoint::REPLACEMENT, Width::NARROW);
                    self.one(base, at)
                }
            };
        }

        if b.is_c0() {
            self.execute(b);
            return ByteLen::from_index(1);
        }
        if b.is_printable_ascii() {
            let cp = PackedCell::translate(b, self.charset);
            self.put_char(cp, Width::NARROW);
            return ByteLen::from_index(1);
        }
        match b.utf8_lead() {
            Some((rem, bits, min)) => {
                self.utf8 = Utf8Acc::lead(bits);
                self.utf8_min = min;
                self.node = crate::graph::Node::owing(Pending::new(rem));
                ByteLen::from_index(1)
            }
            // A stray continuation or an illegal lead: one replacement, and
            // `[[source: unicode-ch3]]` 3.9.5 says the *next* byte is not
            // consumed with it.
            None => {
                self.put_char(Codepoint::REPLACEMENT, Width::NARROW);
                ByteLen::from_index(1)
            }
        }
    }

    /// The accumulator is complete: reject a non-shortest form, then write.
    #[inline(always)]
    fn finish_char(&mut self) {
        let cp = self.utf8.finish(self.utf8_min);
        let cl = Classified::of(cp);
        let w = cl.width(AmbiguousWidth::Narrow);
        self.put_char(cp, w);
    }

    /// A C0 control at a known offset, for the block walk.
    #[inline(always)]
    pub(crate) fn control(&mut self, base: BytePos, at: ByteLen) {
        let b = StreamByte::from_stream(self.store.arena[base.advance(at).index()]);
        self.execute(b);
    }

    /// A C0 control.
    #[inline(always)]
    fn execute(&mut self, b: StreamByte) {
        if b == StreamByte::LF || b == StreamByte::VT || b == StreamByte::FF {
            self.index();
        } else if b == StreamByte::CR {
            self.cx = Col::ZERO;
        } else if b == StreamByte::BS && self.cx.index() > 0 {
            self.cx = Col::at_index(self.cx.index() - 1);
        }
    }
}
