use crate::*;

/// Codepoints collected before a bulk wide write goes out.
///
/// Sized so the buffer stays in registers-and-L1 and the tail loop is rare:
/// a 200-column row holds 100 double-width characters, so 64 covers most of a
/// line in one pass without making the fallback path common.
const WIDE_RUN: usize = 64;

/// How many single-column non-ASCII characters one `narrow_run` collects.
const NARROW_RUN: usize = 64;

impl<'a> Term<'a> {
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
    pub(crate) unsafe fn fast_esc<S: Scan + Decode>(
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
            let off = match unsafe { S::st(StreamByte::slice_from(&self.s.arena[b + s..b + n])) } {
                Some(o) => o.index(),
                None => {
                    self.fb_boundary.inc();
                    return None;
                }
            };
            // Any C0 in the payload sends this to the state machine, because
            // this path cannot reproduce what williams says about them and the
            // two feeds must be indistinguishable:
            //
            //   * `18` CAN and `1A` SUB *cancel* the string. Scanning straight
            //     to the terminator cannot see a cancel in between, so the same
            //     bytes dispatched or dropped depending on whether they fit in
            //     one read -- a chunk-invariance break, and the property found
            //     it within seconds.
            //   * every other C0 is *ignored* rather than stored, and this path
            //     copies the payload wholesale, so it would keep them.
            //
            // One comparison over the payload, not the text: OSC strings are
            // rare next to graphic characters.
            //
            // One pass over the payload, not the text: OSC strings are rare
            // next to graphic characters, and correctness here is not
            // negotiable -- `chunk_invariant_all_modes` found this within
            // seconds of the anywhere block landing.
            if self.s.arena[b + s..b + s + off].iter().any(|&x| x < 0x20) {
                self.fb_not_csi.inc();
                return None;
            }
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
            let (used, st) = if term == StreamByte::BEL {
                (s + off + 1 - i, StringTerm::Bel)
            } else if s + off + 1 >= n {
                self.fb_boundary.inc();
                return None;
            } else if StreamByte::from_stream(self.s.arena[b + s + off + 1]) == StreamByte::ST_FINAL
            {
                (s + off + 2 - i, StringTerm::St)
            } else {
                self.fb_not_csi.inc();
                return None;
            };
            // The payload is *dispatched* here, not skipped. Scanning to the
            // terminator and discarding is what this path used to do, and it
            // would now mean a title set through the fast path silently did
            // nothing while the same bytes split across two reads worked --
            // exactly the two-feeds divergence this file keeps having.
            self.osc_reset();
            self.osc_term = st;
            let payload = off.min(OSC_MAX);
            if off > OSC_MAX {
                self.osc_truncations.inc();
            }
            self.s.osc[..payload].copy_from_slice(&self.s.arena[b + s..b + s + payload]);
            self.osc_len = ByteLen::from_index(payload);
            self.dispatch_osc();
            self.fast_hits.inc();
            return Some(ByteLen::from_index(used));
        }

        // ESC Fe: two bytes, no parameters.
        //
        // **Not in VT52 mode.** These bytes mean different things there --
        // `ESC D` is IND here and cursor-left in VT52 -- so this path must
        // decline and let the state machine, which knows the mode, dispatch it.
        // Without this the fast path silently answered in the wrong protocol
        // whenever a sequence happened to land inside one read: the exact
        // two-feeds divergence this file has had before.
        // Belt and braces: the caller already skips this path in VT52 mode.
        // Kept because `fast_esc` is `pub(crate)` and a future caller might
        // not.
        let known = self.terminal_mode == TerminalMode::Ansi
            && matches!(
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
    pub(crate) unsafe fn fast_csi<S: Scan + Decode>(
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

        self.params = [None; ParamIdx::CAPACITY];
        self.nparam = ParamIdx::FIRST;
        self.psub = SubParams::NONE;
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
        let off =
            match unsafe { S::final_byte(StreamByte::slice_from(&self.s.arena[b + s..b + n])) } {
                Some(o) => o.index(),
                None => {
                    self.fb_boundary.inc();
                    return None;
                }
            };

        let mut np = ParamIdx::FIRST;
        let mut cur = Param::ZERO;
        let mut any = false;
        let mut sub = SubParams::NONE;
        for k in 0..off {
            let d = StreamByte::from_stream(self.s.arena[b + s + k]);
            if let Some(dig) = d.as_ascii_digit() {
                cur = cur.push(dig);
                any = true;
            } else if d == StreamByte::SEMICOLON || d == StreamByte::COLON {
                if np.raw_index() < ParamIdx::CAPACITY {
                    self.params[np.raw_index()] = if any { Some(cur) } else { None };
                    np = np.next();
                    // Same rule as the portable path. Recording it in only one
                    // of the two feeds is the silent-divergence bug this file
                    // has had before -- see the intermediate-byte note below.
                    if d == StreamByte::COLON {
                        sub = sub.mark(np);
                    }
                }
                cur = Param::ZERO;
                any = false;
            } else if let Some(m) = Marker::from_private(d) {
                // `[[source: williams]]`: `3C-3F` is legal only in `csi_entry`,
                // which is the byte at k == 0. Past that the whole sequence is
                // ignored -- decline and let the state machine do it, so the
                // rule has one implementation rather than two that can drift.
                if k != 0 {
                    self.fb_odd_param.inc();
                    return None;
                }
                self.private = m;
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
        if np.raw_index() < ParamIdx::CAPACITY {
            self.params[np.raw_index()] = if any { Some(cur) } else { None };
            np = np.next();
        }
        self.nparam = np;
        self.psub = sub;

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
    pub(crate) unsafe fn feed_with<S: Scan + Decode>(&mut self, base: BytePos, n: ByteLen) {
        self.count_in(n);
        let (b, nn) = (base.index(), n.index());
        let mut i = 0usize;
        while i < nn {
            // **One compare before the vector.** `S::run` returns the count of
            // printable ASCII at the front, so it returns *zero* whenever the
            // current byte is a control, an `ESC` or a UTF-8 lead -- after a
            // full vector load, compare and movemask. Every CJK codepoint paid
            // one of those to learn there was no ASCII in front of it, and so
            // did every escape sequence.
            //
            // The byte is loaded either way, so this costs a compare on the
            // path that goes on to scan, and saves the whole scan setup on the
            // path that would have got nothing.
            //
            // `StreamByte::is_printable`, not a fourth hand-written range.
            // What the scanner will consume is already written out three times
            // -- the portable vector's `0x20..=0x7e`, AVX2's
            // `< 0x20 | >= 0x80 | == 0x7f` stop mask, and this predicate in the
            // scalar tail of both -- and they have disagreed before: `0x7f` was
            // missing from the AVX2 mask, so a `DEL` inside a 32-byte block
            // joined a run while one in the tail did not.
            // `every_scanner_agrees_on_what_is_printable` pins them together.
            let printable = self.st == State::Ground
                && StreamByte::from_stream(self.s.arena[b + i]).is_printable();
            if printable {
                // SAFETY: caller established whatever S requires.
                let run =
                    unsafe { S::run(StreamByte::slice_from(&self.s.arena[b + i..b + nn])) }.index();
                if run > 0 {
                    self.bytes_bulk.add(run);
                    self.put_run(base, ByteLen::from_index(i), ByteLen::from_index(run));
                    i += run;
                    continue;
                }
            }
            // complete CSI sequences are handled in bulk; anything straddling
            // the read boundary falls through to the state machine.
            // No VT52 check here, deliberately. VT52 has its own ground state
            // (`State::Vt52Ground`), so `st == Ground` already excludes it and
            // the ANSI path pays nothing. An explicit mode test in this loop
            // cost 4% of the cells path -- see `State::Vt52Ground`.
            if self.st == State::Ground
                && StreamByte::from_stream(self.s.arena[b + i]) == StreamByte::ESC
            {
                let (at, lim) = (ByteLen::from_index(i), ByteLen::from_index(nn));
                // SAFETY: caller established whatever S requires.
                if let Some(used) = unsafe { self.fast_csi::<S>(base, at, lim) } {
                    self.bytes_seq.add(used.index());
                    i += used.index();
                    continue;
                }
                // SAFETY: as above.
                if let Some(used) = unsafe { self.fast_esc::<S>(base, at, lim) } {
                    self.bytes_seq.add(used.index());
                    i += used.index();
                    continue;
                }
            }
            // **A whole multi-byte character, without a dispatch per byte.**
            // `S::run` stops at the first byte above 0x7f, so before this every
            // CJK or emoji codepoint took three or four separate trips through
            // `control` -- a state-machine match, a `State::Utf8` transition
            // and a re-entry, per byte. `Term::utf8_share` measures those bytes
            // at 14.5% of btop and 28.2% of vim.
            if self.st == State::Ground && self.encoding == InputEncoding::Utf8 {
                let (at, lim) = (ByteLen::from_index(i), ByteLen::from_index(nn));
                if let Some(used) = self.fast_utf8::<S>(base, at, lim) {
                    i += used.index();
                    continue;
                }
            }
            let byte = StreamByte::from_stream(self.s.arena[b + i]);
            self.bytes_slow.inc();
            self.control(byte);
            i += 1;
        }
    }

    /// One complete, well-formed multi-byte UTF-8 character, decoded in place.
    ///
    /// Returns the bytes consumed, or `None` -- in which case nothing has been
    /// mutated and the caller falls through to `control`, which is what makes
    /// this safe to add. **Every unusual case declines**, so the state machine
    /// remains the only implementation of the hard parts:
    ///
    /// * a sequence straddling the end of this read. `read(2)` splits wherever
    ///   it likes and `ingest_is_chunk_invariant` is the property the parser
    ///   rests on, so a partial sequence must leave decoder state behind --
    ///   which is exactly what `State::Utf8` is for.
    /// * a malformed continuation. `[[source: unicode-ch3]]` § 3.9.5 requires
    ///   that a successor byte which itself begins a well-formed sequence is
    ///   *not* consumed by the error, and that recovery lives in one place.
    /// * anything but `Ground`, and any encoding but UTF-8: under ISO 8859-1 a
    ///   byte above 0x7f is a Latin-1 character and `single_byte` claims it
    ///   before `utf8_lead` is ever reached.
    ///
    /// What remains is the common case, and it does exactly what the
    /// `State::Utf8` arm does -- same counters, same `finish`, same `Width::of`,
    /// same `put_cp` -- because divergence between two spellings of one
    /// operation is a failure this repo has already had in `feed_avx2`.
    /// Does the byte at `i` begin a `k`-byte sequence?
    ///
    /// **The narrow collector's admission test, and it has to be this cheap.**
    /// Without it the collector was entered 169,120 times on `cat-cjk` and
    /// admitted **120** characters -- the narrow text there is isolated
    /// punctuation between CJK -- and each wasted entry paid a decode, a width
    /// lookup and a class lookup before breaking. That measured **-10.5% on
    /// vim** while btop, which is box drawing and therefore long runs of
    /// three-byte narrow characters, gained 10%. One range compare on the next
    /// byte separates the two.
    #[inline(always)]
    fn lead_is(&self, at: BytePos, k: ByteLen) -> bool {
        // Through `utf8_lead` rather than a byte range: the encoded length is
        // what the run is keyed on, and the decoder already names it. Two
        // hand-written hex ranges here were two new raw-byte-range sites the
        // type audit refused, and it was right to -- `0xc2..0xe0` is the
        // two-byte lead range spelled out, which is `utf8_lead` saying `rem`
        // is 1.
        match self.s.arena.get(at.index()) {
            Some(&b) => match StreamByte::from_stream(b).utf8_lead() {
                Some((rem, _, _)) => ByteLen::from_index(usize::from(rem) + 1) == k,
                None => false,
            },
            None => false,
        }
    }

    /// A run of ordinary single-column non-ASCII characters, collected and
    /// written in bulk.
    ///
    /// The narrow twin of `wide_run`, and it exists because the cost map found
    /// an inversion: `opcost` prices this shape at **266 MB/s against a wide
    /// character's 1155**, half the cells for five times the cost, with no
    /// mechanism available except that it had no bulk path.
    ///
    /// **The run holds one encoded length, `step`.** Latin-1 and Greek are two
    /// bytes, box drawing and CJK punctuation three, and mixing them would put
    /// a variable stride in the inner loop; the run simply ends where the
    /// length changes, and the next entry picks the new one up. That is also
    /// what lets `put_narrow_run` multiply rather than re-measure.
    /// **Outlined.** `feed_with`'s size is something this repo has measured
    /// repeatedly, and inlined this cost 4% of `cat-scroll` -- a payload that
    /// is 0.1% non-ASCII and therefore essentially cannot execute it.
    #[inline(never)]
    fn narrow_run(&mut self, start: BytePos, lim: ByteLen, step: ByteLen) -> ByteLen {
        let from = start.index();
        let end = from + lim.index();
        let k = step.index();
        let mut buf = [Codepoint::REPLACEMENT; NARROW_RUN];
        let mut n = 0usize;
        let mut scan = 0usize;
        #[cfg(feature = "diag")]
        self.diag.narrow_entries.inc();
        while n < NARROW_RUN && from + scan + k <= end {
            let b0 = StreamByte::from_stream(self.s.arena[from + scan]);
            let Some((rem, bits, min)) = b0.utf8_lead() else {
                break;
            };
            if usize::from(rem) + 1 != k {
                break;
            }
            let mut acc = Utf8Acc::lead(bits);
            let mut ok = true;
            for j in 1..k {
                match StreamByte::from_stream(self.s.arena[from + scan + j]).utf8_continuation() {
                    Some(c) => acc = acc.push(c),
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if !ok {
                break;
            }
            let cp = acc.finish(min);
            if cp == Codepoint::REPLACEMENT || Width::of(cp, self.width.ambiguous) != Width::NARROW
            {
                break;
            }
            let (class, lb) = GraphemeClass::of_with_lookback(cp);
            if class != GraphemeClass::Other || !lb.is_none() {
                break;
            }
            buf[n] = cp;
            n += 1;
            scan += k;
        }
        #[cfg(feature = "diag")]
        self.diag.narrow_taken.add(n);
        if n > 0 {
            let took = self.put_narrow_run(&buf[..n], step).index();
            #[cfg(feature = "instrument")]
            {
                self.bytes_utf8.add(took);
                self.bytes_text.add(took);
            }
            return ByteLen::from_index(took);
        }
        ByteLen::ZERO
    }

    /// Collect a run of ordinary wide characters and write it in bulk.
    ///
    /// `#[inline(never)]`, and that is measured rather than tidiness: left
    /// inline it grew `fast_utf8` enough to cost `btop.raw` **2.5%** -- a
    /// recording with *zero* wide codepoints, which never enters this at all.
    /// Same finding as `open_gap`, `anywhere_c0` and the outlined Ground arms:
    /// what costs is the size of what the ingest loop carries, not the work
    /// inside a branch it does not take.
    ///
    /// `start` is the offset of the *next* character, past the one `fast_utf8`
    /// has already placed. Returns bytes consumed, three per codepoint taken.
    #[inline(never)]
    fn wide_run(&mut self, start: BytePos, lim: ByteLen) -> ByteLen {
        let from = start.index();
        let end = from + lim.index();
        let mut used = 0usize;
        let mut buf = [Codepoint::REPLACEMENT; WIDE_RUN];
        let mut n = 0usize;
        let mut scan = used;
        #[cfg(feature = "diag")]
        self.diag.wide_entries.inc();
        // **Four characters a step, decoded with vectors.**
        //
        // The ASCII side has been vectorised since the beginning -- `Scan::run`
        // classifies sixteen bytes at a time and `put_ascii_run` writes eight
        // cells a store -- while the UTF-8 side decoded one codepoint at a
        // time: a lead test, two continuation tests, two accumulator pushes, a
        // shortest-form check, a width trie and a cluster trie, per character.
        // This closes that gap for the case that dominates a CJK stream.
        //
        // Three bytes a character, so twelve bytes give four codepoints. The
        // load is sixteen wide because that is the vector; the last four bytes
        // are looked at and not consumed, and the scalar loop below finishes
        // whatever is left.
        //
        // **The range test is the validation.** Every admitted range sits
        // above the three-byte shortest form's U+800 floor and outside the
        // surrogate block, so a codepoint landing in one can be neither an
        // overlong encoding nor a surrogate. The one check that makes this
        // legal is the same one that makes it fast, and
        // `the_wide_ordinary_ranges_agree_with_the_tables` enumerates the
        // rest of the premise against the generated tables.
        while n + 4 <= WIDE_RUN && from + scan + 16 <= end {
            use core::simd::simd_swizzle;
            use core::simd::{Simd, cmp::SimdPartialEq, cmp::SimdPartialOrd, num::SimdUint};
            let chunk = Simd::<u8, 16>::from_slice(&self.s.arena[from + scan..from + scan + 16]);
            let b0 = simd_swizzle!(chunk, [0, 3, 6, 9]);
            let b1 = simd_swizzle!(chunk, [1, 4, 7, 10]);
            let b2 = simd_swizzle!(chunk, [2, 5, 8, 11]);
            let leads = (b0 & Simd::splat(0xf0)).simd_eq(Simd::splat(0xe0));
            let ok1 = (b1 & Simd::splat(0xc0)).simd_eq(Simd::splat(0x80));
            let ok2 = (b2 & Simd::splat(0xc0)).simd_eq(Simd::splat(0x80));
            if !(leads & ok1 & ok2).all() {
                break;
            }
            let cp = ((b0 & Simd::splat(0x0f)).cast::<u32>() << 12)
                | ((b1 & Simd::splat(0x3f)).cast::<u32>() << 6)
                | (b2 & Simd::splat(0x3f)).cast::<u32>();
            // The four ranges of `Codepoint::is_wide_ordinary`, as vectors.
            // Every one of them sits above the three-byte shortest form's
            // U+800 floor and outside the surrogate block, so admission here
            // is also proof the encoding was well formed.
            //
            // **The big range is tested alone first.** It holds 29,171 of the
            // 29,454 admissible codepoints, so on Han text the other three are
            // pure cost -- computing all four unconditionally measured **-9%**
            // on `payload::cjk` while buying the mixed recordings 2%. Asking
            // the common one first keeps both.
            let han = cp.simd_ge(Simd::splat(0x329a)) & cp.simd_le(Simd::splat(0xa48c));
            if !han.all() {
                let rest = (cp.simd_ge(Simd::splat(0x3041)) & cp.simd_le(Simd::splat(0x3096)))
                    | (cp.simd_ge(Simd::splat(0x309b)) & cp.simd_le(Simd::splat(0x30ff)))
                    | (cp.simd_ge(Simd::splat(0xff01)) & cp.simd_le(Simd::splat(0xff60)));
                if !(han | rest).all() {
                    break;
                }
            }
            for (k, v) in cp.to_array().into_iter().enumerate() {
                buf[n + k] = Codepoint::wide_ordinary(v);
            }
            n += 4;
            scan += 12;
        }
        while n < WIDE_RUN && from + scan + 3 <= end {
            let b0 = self.s.arena[from + scan];
            // Three-byte leads only. CJK, kana and Hangul are all here, and
            // fixing the length keeps this loop free of a second branch.
            if !(0xe0..0xf0).contains(&b0) {
                #[cfg(feature = "diag")]
                self.diag.brk_lead.inc();
                break;
            }
            let (Some(c1), Some(c2)) = (
                StreamByte::from_stream(self.s.arena[from + scan + 1]).utf8_continuation(),
                StreamByte::from_stream(self.s.arena[from + scan + 2]).utf8_continuation(),
            ) else {
                #[cfg(feature = "diag")]
                self.diag.brk_lead.inc();
                break;
            };
            let Some((rem, bits, min)) = StreamByte::from_stream(b0).utf8_lead() else {
                #[cfg(feature = "diag")]
                self.diag.brk_lead.inc();
                break;
            };
            if rem != 2 {
                #[cfg(feature = "diag")]
                self.diag.brk_lead.inc();
                break;
            }
            let cp = Utf8Acc::lead(bits).push(c1).push(c2).finish(min);
            // **One range compare instead of two trie lookups.** Han is the
            // bulk of what this collector is for, and for Han both answers are
            // constant -- see `Codepoint::is_wide_ordinary`, whose premise is
            // enumerated against the tables rather than asserted. Everything
            // else still asks.
            if !cp.is_wide_ordinary() {
                if cp == Codepoint::REPLACEMENT
                    || Width::of(cp, self.width.ambiguous) != Width::WIDE
                {
                    #[cfg(feature = "diag")]
                    self.diag.brk_width.inc();
                    break;
                }
                let (class, lb) = GraphemeClass::of_with_lookback(cp);
                if class != GraphemeClass::Other || !lb.is_none() {
                    #[cfg(feature = "diag")]
                    self.diag.brk_class.inc();
                    break;
                }
            }
            buf[n] = cp;
            n += 1;
            scan += 3;
        }
        // The two exits the loop condition takes rather than a `break`, so the
        // six reasons sum to one per entry -- the positive control that says
        // this map is complete rather than merely plausible.
        #[cfg(feature = "diag")]
        {
            self.diag.wide_taken.add(n);
            if n == WIDE_RUN {
                self.diag.brk_cap.inc();
            } else if from + scan + 3 > end {
                self.diag.brk_end.inc();
            }
        }
        if n > 0 {
            let took = self.put_wide_run(&buf[..n]).index();
            #[cfg(feature = "instrument")]
            {
                self.bytes_utf8.add(took);
                self.bytes_text.add(took);
            }
            used += took;
        }
        ByteLen::from_index(used)
    }

    /// Collect a run through the block decoder and write it with the existing
    /// bulk path.
    ///
    /// **A like-for-like comparison of the two decoders.** Everything after
    /// the decode is identical to `wide_run` -- the same `put_wide_run`, the
    /// same eight cells a store -- so the difference measured here is the
    /// decode and nothing else. `wide_run` decodes with a constant stride of
    /// three and compile-time swizzle indices; this decodes any mix of lengths
    /// with a table lookup and two vector steps.
    #[cfg(feature = "block-decode")]
    #[inline(never)]
    fn decoded_run<S: Decode>(&mut self, start: BytePos, lim: ByteLen) -> ByteLen {
        let from = start.index();
        let end = from + lim.index();
        let mut cps = [Codepoint::REPLACEMENT; WIDE_RUN];
        let (mut k, mut scan) = (0usize, 0usize);
        while k + 4 <= WIDE_RUN && from + scan + baud_domain::LANE_COUNT <= end {
            // SAFETY: the caller established this scanner's target features.
            let got = unsafe { S::block(StreamByte::slice_from(&self.s.arena[from + scan..end])) };
            if got.count.is_zero() {
                break;
            }
            let mut n = 0usize;
            while n < got.count.index() {
                let cp = got.cps[n];
                // Wide-ordinary only: the cluster machine provably does not
                // need these, and they are all three bytes.
                if !cp.is_wide_ordinary() {
                    break;
                }
                cps[k + n] = cp;
                n += 1;
            }
            k += n;
            scan += n * 3;
            if n < got.count.index() {
                break;
            }
        }
        if k == 0 {
            return ByteLen::ZERO;
        }
        let took = self.put_wide_run(&cps[..k]).index();
        #[cfg(feature = "instrument")]
        {
            self.bytes_utf8.add(took);
            self.bytes_text.add(took);
        }
        ByteLen::from_index(took)
    }

    /// Write the heterogeneous stretch that no bulk collector covers.
    ///
    /// **This is the other half of the split `Decode`'s doc comment
    /// describes**, and it was missing. `decoded_run` calls the same
    /// class-agnostic decoder and then keeps only the `is_wide_ordinary`
    /// prefix, so a block that decodes four codepoints and finds a kana in
    /// lane two throws lanes two through four away and lets the scalar loop
    /// decode them again. That is the "decides by scanning" waste this repo
    /// already records, and it is why `block-decode` measures **-5% on
    /// cat-cjk and -12% on `payload::cjk`**.
    ///
    /// What is left once the specialised paths take their cases is exactly
    /// what `cat-cjk.raw` is made of: narrow two-byte characters, which
    /// `opcost` prices at 6.5 ns and which have no bulk path at all; combining
    /// marks; and emoji. The recording's wide runs have a **median of 1**, so
    /// the homogeneous collectors decline constantly and this stretch is most
    /// of the payload.
    ///
    /// Two classes are handed back rather than written here, and both are
    /// measured rather than assumed:
    ///
    /// * **ASCII**, because `put_ascii_run` writes eight cells to one store
    ///   and reaches 2450 MB/s, which nothing here approaches.
    /// * **A wide-ordinary run**, because `wide_run`'s const-stride collector
    ///   beats this decoder on homogeneous text -- 2570 against 2250 on
    ///   `payload::cjk`, measured as the `block-decode` arm above.
    ///
    /// The cost of handing them back is one wasted block decode at the start
    /// of each run. The gain is that every character in between stops paying
    /// a scalar `utf8_lead` and an accumulate loop.
    #[cfg(feature = "mixed-decode")]
    fn mixed_run<S: Decode>(&mut self, start: BytePos, lim: ByteLen) -> ByteLen {
        let from = start.index();
        let end = from + lim.index();
        if from + baud_domain::LANE_COUNT > end {
            return ByteLen::ZERO;
        }
        // SAFETY: the caller established this scanner's target features.
        let got = unsafe { S::block(StreamByte::slice_from(&self.s.arena[from..end])) };
        let mut used = 0usize;
        for k in 0..got.count.index() {
            let cp = got.cps[k];
            if cp.is_ascii() {
                break;
            }
            let w = Width::of(cp, self.width.ambiguous);
            if w == Width::WIDE && cp.is_wide_ordinary() && self.cluster.ordinary_run_ready() {
                break;
            }
            // Mirrors the scalar path exactly: a literal `EF BF BD` counts as
            // a replacement there too, so both arms report the same figure.
            if cp == Codepoint::REPLACEMENT {
                self.replacements.inc();
            }
            self.put_cp(cp, w);
            used += cp.utf8_len().index();
        }
        #[cfg(feature = "instrument")]
        {
            self.bytes_utf8.add(used);
            self.bytes_text.add(used);
        }
        ByteLen::from_index(used)
    }

    // `S` is used only by the `block-decode` arm. Keeping the parameter in
    // both spellings means the two differ by one call site rather than by a
    // signature, which is what makes them comparable.
    #[cfg_attr(
        not(feature = "block-decode"),
        allow(clippy::extra_unused_type_parameters)
    )]
    fn fast_utf8<S: Decode>(
        &mut self,
        base: BytePos,
        at: ByteLen,
        lim: ByteLen,
    ) -> Option<ByteLen> {
        let from = base.index() + at.index();
        let end = base.index() + lim.index();
        let (rem, bits, min) = StreamByte::from_stream(self.s.arena[from]).utf8_lead()?;
        let need = usize::from(rem) + 1;
        if from + need > end {
            return None;
        }
        let mut acc = Utf8Acc::lead(bits);
        for k in 1..need {
            acc = acc.push(StreamByte::from_stream(self.s.arena[from + k]).utf8_continuation()?);
        }
        // Past here nothing can decline, so the mutations begin.
        #[cfg(feature = "instrument")]
        self.bytes_utf8.add(need);
        #[cfg(feature = "instrument")]
        self.bytes_text.add(need);
        // `finish` rejects a non-shortest form: without it `E0 80 AF` decoded
        // to `/`, the path-traversal overlong.
        let c = acc.finish(min);
        if c == Codepoint::REPLACEMENT {
            self.replacements.inc();
        }
        let w = Width::of(c, self.width.ambiguous);
        self.put_cp(c, w);

        // **A run of ordinary wide characters goes out in bulk.**
        //
        // `cat-cjk.raw` is 81% three-byte characters with no escape sequences
        // at all, and every one of them was paying the full per-codepoint path:
        // a promotion check, an insert-mode test, an orphan repair, an address
        // computation, an extent update and two individual cell stores. The
        // ASCII side has had `put_ascii_run` writing eight cells a store since
        // the beginning; this is its counterpart.
        //
        // A codepoint joins the run only if it is `Width::WIDE`, of class
        // `Other`, and in no lookback table -- which together mean `is_break`
        // would return `true` and change nothing. The cluster machine has to
        // be in that state to begin with, and each such character leaves it
        // there, so the whole run is provably a no-op for it.
        //
        // Anything else -- an emoji, a combining mark, a Hangul syllable, an
        // ASCII byte, a narrow character, a malformed sequence, the right
        // margin -- ends the run and falls back to the one-at-a-time path
        // below, which is unchanged.
        //
        // **Gated on the character just written being wide**, which is already
        // known and costs nothing to test. Without it, `btop.raw` -- 14.5%
        // multi-byte and **zero** wide codepoints, because box drawing is
        // three bytes and one column -- entered the collector on every
        // character, decoded it, looked its width up, failed the test and left
        // the real path to do all of that again. That measured **-4%** on btop
        // and vim before the gate and +3% after.
        let mut used = need;
        #[cfg(feature = "diag")]
        if w == Width::WIDE {
            self.diag.gate_wide.inc();
            match self.cluster.ordinary_run_blocker() {
                baud_domain::RunBlocker::Ready => {}
                baud_domain::RunBlocker::Prev => self.diag.gate_prev.inc(),
                baud_domain::RunBlocker::Regional => self.diag.gate_regional.inc(),
                baud_domain::RunBlocker::Pictographic => self.diag.gate_pict.inc(),
                baud_domain::RunBlocker::Indic => self.diag.gate_indic.inc(),
            }
        }
        if w == Width::WIDE && self.cluster.ordinary_run_ready() {
            #[cfg(feature = "block-decode")]
            {
                used += self
                    .decoded_run::<S>(
                        BytePos::from_index(from + used),
                        ByteLen::from_index(end - from - used),
                    )
                    .index();
            }
            #[cfg(not(feature = "block-decode"))]
            {
                used += self
                    .wide_run(
                        BytePos::from_index(from + used),
                        ByteLen::from_index(end - from - used),
                    )
                    .index();
            }
        } else if w == Width::NARROW
            && need > 1
            && self.cluster.ordinary_run_ready()
            && self.lead_is(BytePos::from_index(from + used), ByteLen::from_index(need))
        {
            used += self
                .narrow_run(
                    BytePos::from_index(from + used),
                    ByteLen::from_index(end - from - used),
                    ByteLen::from_index(need),
                )
                .index();
        }
        while from + used < end {
            let b0 = StreamByte::from_stream(self.s.arena[from + used]);
            let Some((rem2, bits2, min2)) = b0.utf8_lead() else {
                break;
            };
            let need2 = usize::from(rem2) + 1;
            if from + used + need2 > end {
                break;
            }
            let mut acc2 = Utf8Acc::lead(bits2);
            let mut ok = true;
            for k in 1..need2 {
                match StreamByte::from_stream(self.s.arena[from + used + k]).utf8_continuation() {
                    Some(cont) => acc2 = acc2.push(cont),
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if !ok {
                break;
            }
            #[cfg(feature = "instrument")]
            self.bytes_utf8.add(need2);
            #[cfg(feature = "instrument")]
            self.bytes_text.add(need2);
            let c2 = acc2.finish(min2);
            if c2 == Codepoint::REPLACEMENT {
                self.replacements.inc();
            }
            let w2 = Width::of(c2, self.width.ambiguous);
            self.put_cp(c2, w2);
            used += need2;

            // **The bulk path re-arms here, and not doing so was most of what
            // it was leaving on the table.** The gate above fires once, on the
            // first character this call decodes; every wide run *after* the
            // first non-wide character stayed on the per-codepoint path
            // because nothing brought it back. Real text alternates -- a CJK
            // recording is 39% ASCII, so a line is runs of wide characters
            // separated by spaces, digits and punctuation, and only the first
            // of them was going out in bulk.
            //
            // It is the same gate for the same reason: the character just
            // written is known to be wide, so the collector is only entered
            // where it has a run to find, and btop -- multi-byte with zero
            // wide codepoints -- never reaches it.
            #[cfg(feature = "diag")]
            if w2 == Width::WIDE {
                self.diag.gate_wide.inc();
                match self.cluster.ordinary_run_blocker() {
                    baud_domain::RunBlocker::Ready => {}
                    baud_domain::RunBlocker::Prev => self.diag.gate_prev.inc(),
                    baud_domain::RunBlocker::Regional => self.diag.gate_regional.inc(),
                    baud_domain::RunBlocker::Pictographic => self.diag.gate_pict.inc(),
                    baud_domain::RunBlocker::Indic => self.diag.gate_indic.inc(),
                }
            }
            if w2 == Width::WIDE && self.cluster.ordinary_run_ready() {
                used += self
                    .wide_run(
                        BytePos::from_index(from + used),
                        ByteLen::from_index(end - from - used),
                    )
                    .index();
            } else if w2 == Width::NARROW
                && need2 > 1
                && self.cluster.ordinary_run_ready()
                && self.lead_is(BytePos::from_index(from + used), ByteLen::from_index(need2))
            {
                used += self
                    .narrow_run(
                        BytePos::from_index(from + used),
                        ByteLen::from_index(end - from - used),
                        ByteLen::from_index(need2),
                    )
                    .index();
            } else {
                // **Both collectors declined, so the stretch is heterogeneous.**
                // Entering here rather than at the top of the loop is the whole
                // difference between a block decode that is used and one that
                // is wasted: at the top, a block beginning with ASCII or with a
                // wide run paid a full decode and returned nothing.
                #[cfg(feature = "mixed-decode")]
                {
                    used += self
                        .mixed_run::<S>(
                            BytePos::from_index(from + used),
                            ByteLen::from_index(end - from - used),
                        )
                        .index();
                }
            }
        }
        Some(ByteLen::from_index(used))
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

    /// Bytes ingested, for the amplification denominator.
    ///
    /// Counted here rather than inside `feed_with` so both entry points get it
    /// once per read, not once per scanned block.
    #[inline(always)]
    pub(crate) fn count_in(&mut self, n: ByteLen) {
        self.bytes_in.add(n.index());
    }

    /// Ingest with the aarch64 scanner.
    ///
    /// Same 16-byte step as `feed`, one instruction different: the lane mask
    /// comes out of `shrn` rather than the `uzp1`/`zip1`/`addv` sequence
    /// `core::simd`'s `to_bitmask` lowers to here. See `baud-isa-aarch64`.
    ///
    /// NEON is baseline on aarch64, so unlike `feed_avx2` there is nothing for
    /// the caller to have detected -- the target is the guarantee.
    #[cfg(target_arch = "aarch64")]
    pub fn feed_neon(&mut self, base: BytePos, n: ByteLen) {
        // SAFETY: NEON is baseline on aarch64.
        unsafe { self.feed_with::<baud_isa_aarch64::Neon>(base, n) }
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
        unsafe { self.feed_with::<baud_isa_x86_64::Avx2>(base, n) }
    }
}
