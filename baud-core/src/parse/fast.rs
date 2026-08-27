use crate::*;

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
    pub(crate) unsafe fn fast_esc<S: Scan>(
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
    pub(crate) unsafe fn fast_csi<S: Scan>(
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
    pub(crate) unsafe fn feed_with<S: Scan>(&mut self, base: BytePos, n: ByteLen) {
        self.count_in(n);
        let (b, nn) = (base.index(), n.index());
        let mut i = 0usize;
        while i < nn {
            if self.st == State::Ground {
                // SAFETY: caller established whatever S requires.
                let run = unsafe { S::run(&self.s.arena[b + i..b + nn]) }.index();
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
            let byte = StreamByte::from_stream(self.s.arena[b + i]);
            self.bytes_slow.inc();
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

    /// Bytes ingested, for the amplification denominator.
    ///
    /// Counted here rather than inside `feed_with` so both entry points get it
    /// once per read, not once per scanned block.
    #[inline(always)]
    pub(crate) fn count_in(&mut self, n: ByteLen) {
        self.bytes_in.add(n.index());
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
