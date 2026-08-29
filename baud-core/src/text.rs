use crate::*;

impl<'a> Term<'a> {
    #[inline(always)]
    pub(crate) fn line(&self) -> LineSlot {
        LineSlot::of(self.head.plus(self.cy, SB))
    }

    #[inline(always)]
    pub(crate) fn view(&mut self) -> View<'_> {
        let alt = self.screen == Screen::Alternate;
        let rot = self.active_rot();
        // Buffer and marks from one `alt`, for the reason `active_rot` exists:
        // a site that takes `s.alt` and `s.used_vp` compiles and corrupts.
        let (cells, used) = match alt {
            true => (&mut self.s.alt, &mut self.s.used_alt),
            false => (&mut self.s.vp, &mut self.s.used_vp),
        };
        View::new(
            cells,
            used,
            Cols::from_stride(MAX_COLS),
            self.cols,
            self.rows,
            rot,
        )
    }

    /// Read-only counterpart of [`Term::view`], for the questions that only
    /// the grid can answer -- which column owns a character, above all.
    #[inline(always)]
    pub(crate) fn view_ref(&self) -> ViewRef<'_> {
        let rot = self.active_rot();
        // Buffer and rotation from one `alt`, for the reason `view` says.
        let cells = match self.screen {
            Screen::Alternate => &self.s.alt,
            Screen::Normal => &self.s.vp,
        };
        View::new_ref(
            cells,
            Cols::from_stride(MAX_COLS),
            self.cols,
            self.rows,
            rot,
        )
    }

    /// Write one decoded codepoint at the cursor: the orphan repair, the
    /// glyph, and a wide character's tail, through **one** `View`.
    ///
    /// It was three. `clear_partners_counted`, `write_cell` and the tail's
    /// `put_counted` each built their own four-field borrow, per codepoint, on
    /// the path that carries every non-ASCII character -- and `CLAUDE.md`
    /// records constructing a view inside a per-cell loop as a mistake this
    /// project has already made once and measured.
    ///
    /// ASCII never arrives here; it goes through `blit` and `put_ascii_run`.
    /// So this is the CJK path, which `MEASUREMENTS.md` puts at six times the
    /// cost of the same workload in ASCII.
    #[inline(always)]
    pub(crate) fn write_cell(&mut self, cp: Codepoint, w: Width) {
        let (r, c, st, fl) = (self.cy, self.cx, self.style, self.flags);
        let wide = w == Width::WIDE;
        let n = match wide {
            true => 2,
            false => 1,
        };
        self.cells_written.add(n);
        #[cfg(feature = "instrument")]
        self.cells_single.add(n);
        let span_end = match wide {
            true => c.after().after(),
            false => c.after(),
        };
        let _ = span_end;
        let blank = PackedCell::empty_with(st);
        let tail = match wide {
            true => Some(PackedCell::wide_tail(st, fl)),
            false => None,
        };
        // **One pass**: the orphan repair, the glyph and the tail share one
        // address computation, one bounds check and one extent update. Three of
        // each is what this cost when it was `clear_partners` plus two `put`s.
        self.view()
            .put_glyph(r, c, PackedCell::glyph(cp, st, fl), tail, blank);
    }

    /// Place one decoded codepoint. A 2-wide character at the last column
    /// cannot be split: the cell is left blank and it moves to the next row.
    /// That decision is forced by `try_advance` returning `Option`.
    #[inline(always)]
    /// Write a run of ordinary double-width characters, skipping the
    /// per-character machinery.
    ///
    /// The bulk path for CJK, and the counterpart of `blit`/`put_ascii_run`.
    /// Every caller-supplied codepoint has already been checked to be
    /// `Width::WIDE`, of class `Other`, and in no lookback table; the cluster
    /// machine is in `ordinary_run_ready` state, so `is_break` would return
    /// `true` for each and change nothing.
    ///
    /// What that leaves is the work this does **once** instead of `n` times:
    /// the promotion check, the insert-mode test, the orphan repair, the
    /// address computation, the extent update and the cursor arithmetic. The
    /// cells go out eight to a store.
    ///
    /// Returns how many codepoints it took. A short return means the run hit
    /// the right margin, and the caller falls back to `put_cp` for the rest --
    /// which is where wrapping, insert mode and every other case still live.
    pub(crate) fn put_wide_run(&mut self, cps: &[Codepoint]) -> ByteLen {
        if cps.is_empty() || self.modes.is_set(ModeBit::InsertReplace) {
            return ByteLen::ZERO;
        }
        // The run must fit before the right margin; the remainder wraps, and
        // wrapping is `put_cp`'s job.
        let edge = self.right_margin.after().index().min(self.cols.index());
        let x = self.cx.index();
        if x + 2 > edge {
            return ByteLen::ZERO;
        }
        let n = cps.len().min((edge - x) / 2);
        if n == 0 {
            return ByteLen::ZERO;
        }
        self.promote_if_runs();
        let (r, st, fl) = (self.cy, self.style, self.flags);
        let wrote = self
            .view()
            .put_wide_run(r, Col::at_index(x), &cps[..n], st, fl)
            .index();
        let took = wrote / 2;
        self.cells_written.add(wrote);
        #[cfg(feature = "instrument")]
        self.cells_wide.add(wrote);
        self.cx = Col::at_index(x + wrote);
        if took > 0 {
            // `REP` repeats the last graphic character, so it must be the last
            // one this wrote rather than whatever preceded the run.
            self.last_cp = Some((cps[took - 1], Width::WIDE));
        }
        // Three bytes a codepoint: the collector only admits three-byte leads.
        ByteLen::from_index(took * 3)
    }

    /// A run of ordinary single-column non-ASCII characters, written in bulk.
    ///
    /// The narrow counterpart of `put_wide_run`. Returns the **bytes** it
    /// consumed, which is what the caller advances by; `step` is the run's
    /// common encoded length, which the collector holds constant so this can
    /// multiply rather than re-measure each character.
    pub(crate) fn put_narrow_run(&mut self, cps: &[Codepoint], step: ByteLen) -> ByteLen {
        if cps.is_empty() || self.modes.is_set(ModeBit::InsertReplace) {
            return ByteLen::ZERO;
        }
        let edge = self.right_margin.after().index().min(self.cols.index());
        let x = self.cx.index();
        if x + 1 > edge {
            return ByteLen::ZERO;
        }
        let n = cps.len().min(edge - x);
        if n == 0 {
            return ByteLen::ZERO;
        }
        self.promote_if_runs();
        let (r, st, fl) = (self.cy, self.style, self.flags);
        let wrote = self
            .view()
            .put_narrow_run(r, Col::at_index(x), &cps[..n], st, fl)
            .index();
        self.cells_written.add(wrote);
        #[cfg(feature = "instrument")]
        self.cells_narrow.add(wrote);
        self.cx = Col::at_index(x + wrote);
        if wrote > 0 {
            self.last_cp = Some((cps[wrote - 1], Width::NARROW));
        }
        ByteLen::from_index(wrote * step.index())
    }

    pub(crate) fn put_cp(&mut self, cp: Codepoint, w: Width) {
        // What is paying the per-codepoint path. See `Diag`.
        #[cfg(feature = "diag")]
        {
            if cp.value() < 0x80 {
                self.diag.cp_ascii.inc();
            } else if w == Width::WIDE {
                self.diag.cp_wide.inc();
            } else if w == Width::ZERO {
                self.diag.cp_zero.inc();
            } else {
                self.diag.cp_narrow.inc();
            }
        }
        // **Does this codepoint continue the cluster before it?**
        //
        // Width alone cannot answer that. A ZWJ emoji sequence is three
        // codepoints of width 2, 0 and 2, so measuring each on its own gives
        // `U+1F469 U+200D U+1F4BB` four columns for one glyph. So does a flag:
        // two regional indicators, two columns each. `[[source: uax29]]` says
        // where the boundaries are, and `ClusterBreak` implements it against
        // Unicode's own conformance suite.
        //
        // A continuation is attached to the cell before it, exactly as a
        // combining mark is -- the marks table *is* the cluster continuation
        // store, which is why this needs no room in `PackedCell`.
        // **Two probes that differ only in a dependency.**
        //
        // Character N's break decision reads the state character N-1 wrote, so
        // the cluster machine is a serial chain through the whole stream. The
        // obvious arm -- decide from a snapshot that is never advanced -- is
        // *contaminated*, and its counters say so loudly: the real machine then
        // never advances either, `ordinary_run_ready` stops firing, and the
        // bulk wide collector disengages entirely. `runs` went 21% to 0%,
        // `cells_written` 15,212,545 to 16,116,287, and `payload::cjk` fell
        // 2561 to 364. It measures the collectors, not the chain.
        //
        // These two do. Each adds exactly one `is_break` on a `Copy`, so the
        // instruction count is the same and the decision still comes from the
        // real machine -- `cells_written` is byte-identical to the default
        // build in both. The only difference is what the extra call's input
        // depends on: `dep-probe-serial` copies the live machine, so its
        // lookup cannot start until the previous character finished with it;
        // `dep-probe-free` copies a snapshot that is never written, so it can
        // issue immediately. The gap between them is the cost of the chain.
        #[cfg(feature = "dep-probe-serial")]
        {
            let mut c = self.cluster;
            core::hint::black_box(c.is_break(cp));
        }
        #[cfg(feature = "dep-probe-free")]
        {
            let mut c = self.cluster_snapshot;
            core::hint::black_box(c.is_break(cp));
        }
        // **Prices one trie probe pair, to size the merged-table idea before
        // building it.** `Width::of` and `GraphemeClass::of_with_lookback` are
        // two separate two-stage tries over the same `cp >> 8` stage-1 shape,
        // so every character does four table probes where two would do. This
        // adds one `Width::of`; what it costs is roughly what merging them
        // would save.
        #[cfg(feature = "width-probe")]
        core::hint::black_box(Width::of(cp, self.width.ambiguous));
        // **Calibration probe: one L1D miss a character, by construction.**
        // The counter export names nothing, so an index is identified by
        // adding a known quantity of the event and dividing. The arena is
        // 1 MiB against a 128 KB L1, and this index scatters across all of it,
        // so the load misses L1 essentially always while staying resident in
        // L2 -- a last-level miss would cost more than the entire 29.6-cycle
        // budget and swamp what it is meant to measure. Multiplying by the
        // 32-bit golden ratio decorrelates consecutive codepoints, which would
        // otherwise land in the same line.
        #[cfg(feature = "miss-probe")]
        {
            // Indexed from the cursor rather than the codepoint: `Col` and
            // `Row` are already `usize`-backed, so this needs no cast and the
            // type audit stays at five. It also scatters wider -- about ten
            // thousand distinct indices against a few thousand codepoints.
            let k = self.cx.index() * MAX_ROWS + self.cy.index();
            core::hint::black_box(self.s.arena[k.wrapping_mul(2_654_435_761) % ARENA]);
        }
        if !self.cluster.is_break(cp) && self.cx != Col::ZERO {
            self.attach_mark(cp);
            return;
        }
        if w == Width::ZERO {
            // **Attached, not discarded.** This returned early, so every
            // combining mark was silently thrown away: `e` + U+0301 stored `e`
            // and the accent was gone -- not mis-measured, absent, and
            // unrecoverable by a renderer, a selection or a copy.
            //
            // Zero-width codepoints are rare next to text, so this is off the
            // measured path: ASCII never reaches `put_cp` at all.
            self.attach_mark(cp);
            return;
        }
        self.put_glyph(cp, w);
    }

    /// Write a glyph whose cluster decision is already known to be a break.
    ///
    /// **The tail of `put_cp`, reachable without the cluster machine.** A
    /// codepoint that is `is_wide_ordinary` while `ordinary_run_ready` holds
    /// cannot continue the previous cluster: `decide` falls through to GB999
    /// and `advance` writes back the state that is already there. That is not
    /// a new claim -- it is exactly the precondition `put_wide_run` relies on
    /// to skip the machine for an entire run, applied to a run of one.
    ///
    /// It matters because runs of one are what `cat-cjk.raw` is made of. Its
    /// wide runs have a **median of 1**, the vectorised collector needs four
    /// characters to take a step, and everything shorter fell back to the full
    /// per-codepoint path and paid `is_break` -- 14% of this recording -- for
    /// a decision already proven.
    #[inline(always)]
    pub(crate) fn put_glyph(&mut self, cp: Codepoint, w: Width) {
        if self.cx.try_advance(w, self.cols).is_none() {
            if !self.modes.is_set(ModeBit::AutoWrap) {
                // DECAWM reset: the character overwrites the last column and the
                // cursor stays put. No wrap, and therefore no scroll.
                self.cx = Col::at_index(self.cols.index() - 1);
                self.promote_if_runs();
                // `Width::ONE`, deliberately, whatever `w` says: there is one
                // column left and a two-cell character cannot be written into
                // it. Passing `w` would put a lead in the last column with its
                // tail off the end of the row -- an orphan by construction, and
                // the invariant `no_operation_splits_a_double_width_character`
                // exists to rule out.
                self.write_cell(cp, Width::NARROW);
                return;
            }
            self.wrap();
        }
        self.promote_if_runs();
        if self.modes.is_set(ModeBit::InsertReplace) {
            let n = match w {
                Width::WIDE => Count::TWO,
                _ => Count::ONE,
            };
            let edge = self.right_margin.after();
            self.open_gap(n, edge);
        }
        self.write_cell(cp, w);
        self.last_cp = Some((cp, w));
        self.cx = self.cx.try_advance(w, self.cols).unwrap_or(Col::ZERO);
    }

    /// What identifies the cell at a screen position, right now.
    ///
    /// **Three things, and using fewer was three bugs.** The surface, because
    /// the two screens are separate buffers; the *physical* row, because the
    /// grid rotates under the screen row; and the row's generation, because
    /// the ring recycles physical rows. A key of `(Row, Col)` named a cell
    /// that had scrolled away, and named the primary screen's cell from the
    /// alternate one.
    ///
    /// A **fourth** was the column: a mark belongs to the character, and a
    /// double-width character's glyph is in its lead cell. Keying on
    /// `cx.before()` put every ZWJ sequence's continuations on the
    /// `WIDE_TAIL`, where they were stored, counted as attached, and
    /// unreachable -- so a family emoji rendered as its first person. Taking
    /// [`LeadCol`] rather than `Col` is why that cannot be spelled again.
    pub(crate) fn mark_key(&self, r: Row, c: LeadCol) -> MarkKey {
        let p = self.phys_row(r);
        MarkKey::new(
            match self.screen {
                Screen::Alternate => Surface::Alternate,
                Screen::Normal => Surface::Primary,
            },
            p,
            c.col(),
            self.s.mark_gen[p.index()],
        )
    }

    /// The screen row's position in the backing store. Mirrors `View::phys`:
    /// a conditional subtract, because both operands are below `rows`.
    #[inline(always)]
    pub(crate) fn phys_row(&self, r: Row) -> PhysRow {
        let rows = self.rows.index();
        let y = r.index() + self.active_rot().index();
        PhysRow::of(if y >= rows { y - rows } else { y })
    }

    /// Strand every mark on a row, because its contents are being replaced.
    ///
    /// One counter, not a table scan: finding the affected slots in a
    /// direct-mapped table of 512 would cost more per blank than the blanking.
    #[inline(always)]
    pub(crate) fn invalidate_marks(&mut self, r: Row) {
        // **Nothing to strand if nothing was ever attached.** Blanking runs on
        // every scroll -- 4,200 times a rep on a CJK recording -- and marks are
        // rare, so the common case must not pay for the rare one. One load and
        // a predictable branch, against the row arithmetic and a store.
        if self.marks_live.is_zero() {
            return;
        }
        // Indexed directly: `phys_row` returns a row below `self.rows`, and
        // `Term::new` bounds that by `MAX_ROWS`. Clamping or a modulo here
        // would hide a geometry bug rather than trip on it.
        self.s.mark_gen[self.phys_row(r).index()].bump();
    }

    /// Attach a zero-width codepoint to the cell the cursor last wrote.
    ///
    /// `#[inline(never)]`: `put_cp` is on the decode path and this is the rare
    /// arm, the same reason `open_gap` and `anywhere_c0` are out of line.
    ///
    /// This attribute and this comment spent a session attached to `mark_key`
    /// below, because an edit landed between them and the signature they were
    /// written for. The effect was the exact inverse of both: a four-load pure
    /// function forced out of line, called once per attached codepoint, and
    /// this arm inlined into the decode path it says it stays out of.
    #[inline(never)]
    fn attach_mark(&mut self, cp: Codepoint) {
        // A mark with nothing before it on the line has no base to attach to.
        // Dropping it is correct -- `[[source: uax29]]` GB1 puts a boundary at
        // start of text, so it is its own cluster and there is no cell for it.
        if self.cx == Col::ZERO {
            self.marks_dropped.inc();
            return;
        }
        // **Ask the grid which cell owns that column.** The cursor sits after
        // the character it just wrote, so one column back is that character --
        // unless it was double-width, where one column back is its tail.
        let r = self.cy;
        let owner = self.view_ref().lead(r, self.cx.before());
        let key = self.mark_key(r, owner);
        // **Prices the redundancy a per-cluster API would remove.** All six
        // continuations of a ZWJ family target the same cell and rebuild this
        // from unchanged inputs: a grid read to resolve the lead, the key from
        // screen, rotation and generation, and the hash. Doing it twice costs
        // what caching it once would save.
        #[cfg(feature = "attach-probe")]
        {
            let o2 = self.view_ref().lead(r, self.cx.before());
            let k2 = self.mark_key(r, o2);
            core::hint::black_box(k2.slot(NMARK));
        }
        let i = key.slot(NMARK);
        if !self.s.marks[i].is_for(key) {
            if !self.s.marks[i].is_empty() {
                // Someone else's marks. Evicting is the bounded-table
                // contract: the cell that loses them keeps its base character.
                self.marks_evicted.inc();
            }
            self.s.marks[i] = MarkSlot::for_cell(key);
        }
        self.marks_live.inc();
        if !self.s.marks[i].push(cp) {
            self.marks_dropped.inc();
        }
        // **The cell records that it has marks, so that overwriting it drops
        // them.** The key identifies a cell over time; it cannot see the cell
        // rewritten *in place*, which is neither a scroll nor a blank and so
        // bumps no generation. A space written over `e` plus a combining acute
        // kept the accent before this.
        self.view().set_marked(r, owner);
    }

    #[inline(always)]
    pub(crate) fn blit(&mut self, base: BytePos, off: ByteLen, room: Room) {
        let abs = base.index() + off.index();
        let (r, c, st) = (self.cy, self.cx, self.style);
        let (stride, cols, rows) = (Cols::from_stride(MAX_COLS), self.cols, self.rows);
        // Read before the split borrow below: `charset()` reads `self.g`, and
        // the destructuring holds `self.s` mutably.
        let set = self.charset();
        // Same reason as `charset` above: `active_rot` takes `&self`, and the
        // destructuring below holds `self.s` mutably. **It is read here rather
        // than spelled inline so the buffer choice and the rotation come from
        // one decision** -- picking `s.alt` while reading the primary's
        // rotation compiles and displaces every row.
        let rot = self.active_rot();
        let alt = self.screen == Screen::Alternate;
        let (arena, vp, used) = match alt {
            true => (&self.s.arena, &mut self.s.alt, &mut self.s.used_alt),
            false => (&self.s.arena, &mut self.s.vp, &mut self.s.used_vp),
        };
        let fl = self.flags;
        self.cells_written.add(room.index());
        #[cfg(feature = "instrument")]
        self.cells_bulk.add(room.index());
        View::new(vp, used, stride, cols, rows, rot).put_ascii_run(
            r,
            c,
            &arena[abs..abs + room.index()],
            st,
            fl,
            set,
        );
    }

    /// Shift the line right at the cursor to make room for `n` cells. IRM.
    ///
    /// **`#[inline(never)]` and deliberately so.** `put_run` is
    /// `#[inline(always)]` and is the measured ingest loop; inlining
    /// `insert_cells` into it cost ~9% of the cells path (1072.6 -> 963.6
    /// MB/s, ranges not overlapping, with ghostty stable across both runs).
    /// Insert mode is rare, so the call belongs behind a branch that costs one
    /// predictable test per run and nothing else.
    ///
    /// Insert mode also cannot use the run table: a run records "these bytes,
    /// at this position", and shifting the tail right invalidates every run
    /// after the cursor. Promote first, then shift, then write.
    #[inline(never)]
    pub(crate) fn open_gap(&mut self, n: Count, edge: Col) {
        self.promote_if_runs();
        let (r, at, b) = (self.cy, self.cx, PackedCell::empty_with(self.style));
        self.insert_cells_counted(r, at, n, edge, b);
    }

    #[inline(always)]
    pub(crate) fn put_run(&mut self, base: BytePos, mut off: ByteLen, mut n: ByteLen) {
        // Printable ASCII is blitted and never reaches `put_cp`, so
        // `ClusterBreak` does not see it. Reset, or an emoji after `a` would be
        // judged against the emoji before it. One store per *run*, not per
        // byte, so it is off the per-byte path this file is careful about.
        self.cluster = ClusterBreak::new();
        // Hoisted: a mode cannot change inside a run, and reading it per
        // iteration cost 12% of streaming throughput -- measured, not guessed.
        let wrap = self.modes.is_set(ModeBit::AutoWrap);
        // Hoisted for the same reason as `wrap`: DECSLRM cannot change inside a
        // run. With no side margins set this is one past the last column, so
        // the margin-aware form is also the fast form and there is no branch.
        let edge = self.right_margin.after();
        // IRM, hoisted for the same reason again. This is the *whole* cost of
        // insert mode on the replace path: one bool tested once per run, and
        // the branch below never taken. `[[source: ecma48]]` 8.3.64 defines IRM
        // as shifting the line right rather than overwriting, and it was
        // recorded and reported by DECRQM while nothing read it -- so an
        // application got overwrite and was told it had insert.
        let insert = self.modes.is_set(ModeBit::InsertReplace);
        while !n.is_zero() {
            let mut room = self.cols.room_until(self.cx, edge).min(n);
            if room.is_empty() {
                if wrap {
                    self.wrap();
                } else {
                    // DECAWM reset: pin to the last column and let each
                    // remaining byte overwrite it. The last one wins, and the
                    // screen never scrolls.
                    self.cx = self.right_margin;
                }
                room = self.cols.room_until(self.cx, edge).min(n);
            }
            if insert {
                self.open_gap(Count::from_room(room), edge);
            }
            // `promoted` and the run table belong to the primary screen. On the
            // alt screen every write is materialised, which is the same thing
            // "no scrollback" means one layer down.
            let promoted = self.row_is_promoted(self.cy);
            if self.mode == OpMode::Runs && !promoted {
                let i = self.line();
                let c = self.s.nrun[i.index()];
                if c.index() < MAXRUN {
                    self.s.runs[i.run(c, MAXRUN)] = Run {
                        pos: base.advance(off),
                        len: ByteLen::from_room(room),
                        style: self.style,
                        encoding: 0,
                        charset: self.charset().to_u8(),
                        flags: 0,
                        _pad: [0; 1],
                    }
                    .with_encoding(Encoding::Ascii)
                    .with_flags(self.flags);
                    self.s.nrun[i.index()] = c.inc();
                    self.runs_written.inc();
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
            if !wrap && self.cx.index() > self.right_margin.index() {
                self.cx = self.right_margin;
            }
            off = off.saturating_add(ByteLen::from_room(room));
            n = n.sub_room(room);
        }
    }

    pub(crate) fn promote(&mut self) {
        let y = self.cy.index();
        // `row_is_promoted`, not the raw flag: on the alternate screen there
        // are no spans to replay into the cells, so this is already done.
        if self.row_is_promoted(self.cy) {
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
                let (st, fl) = (run.style, run.flags());
                // The run's own set, not whatever is designated now: a line
                // written under German and promoted later must still be German.
                let set = Charset::from_u8(run.charset);
                let cell = PackedCell {
                    content: Content::EMPTY,
                    style: st,
                    flags: fl,
                }
                .with_charset(b, set);
                self.put_counted(rr, cc, cell);
                c += 1;
            }
        }
        let blank = PackedCell::empty_with(self.style);
        let (rr, from, to) = (
            Row::from_index(y),
            Col::at_index(c),
            Col::at_index(self.cols.index()),
        );
        self.fill_counted(rr, from, to, blank);
        self.s.promoted[y] = true;
        self.promotions.inc();
    }

    #[inline(always)]
    pub(crate) fn promote_if_runs(&mut self) {
        if self.mode == OpMode::Runs && self.screen == Screen::Normal {
            self.promote();
        }
    }
}

impl<'a> Term<'a> {
    /// Counted wrappers around the grid writes.
    ///
    /// Sixteen call sites write cells. Instrumenting each by hand is how one
    /// of them ends up counting the wrong span -- and an amplification figure
    /// that is quietly low is worse than none, which is what `inf x
    /// amplification` taught this project once already. The arithmetic lives
    /// here instead, once per shape, so a wrong count is wrong everywhere and
    /// therefore visible.
    #[inline(always)]
    pub(crate) fn put_counted(&mut self, r: Row, c: Col, cell: PackedCell) {
        self.cells_written.inc();
        #[cfg(feature = "instrument")]
        self.cells_single.inc();
        self.view().put(r, c, cell);
    }

    /// The same bound as `fill_row_counted`, for a range.
    ///
    /// `EL`, `ED` and the selective-erase paths all land here, and an erase to
    /// end-of-line on a short line is the same waste scrolling was: the range
    /// runs to the margin whatever the row holds. Compared against `cell` for
    /// the same reason -- the blank carries the current style, so a row that is
    /// already empty still needs writing under a background colour.
    #[inline(always)]
    pub(crate) fn fill_counted(&mut self, r: Row, from: Col, to: Col, cell: PackedCell) {
        // **Count what was written.** A fill running to the end of a row stops
        // at the used extent, so counting the request over-reports -- and did,
        // hiding that optimisation's effect completely on its first
        // measurement: the counters were identical across a change that
        // halved the work.
        // Blanking replaces the contents, so anything remembered about those
        // cells is now about text that is gone. One counter bump strands the
        // row's marks; see `invalidate_marks`.
        self.invalidate_marks(r);
        let n = self.view().fill(r, from, to, cell);
        self.cells_written.add(n);
        self.cells_blanked.add(n);
        #[cfg(feature = "instrument")]
        self.cells_fill.add(n);
    }

    #[inline(always)]
    pub(crate) fn fill_row_counted(&mut self, r: Row, cell: PackedCell) -> RowExtent {
        // **Count what was written, not what was asked for.** `fill_row` stops
        // at the used extent when it soundly can, so the old unconditional
        // `add(cols)` would keep reporting full-width blanking after the write
        // got shorter -- an instrument that cannot show the effect it exists to
        // measure, which is the failure this repo already records twice.
        self.invalidate_marks(r);
        #[cfg(not(feature = "ceiling-no-blank"))]
        let n = self.view().fill_row(r, cell);
        // The ceiling arm writes nothing and still counts the full row, so both
        // arms report identical attribution and only throughput differs.
        #[cfg(feature = "ceiling-no-blank")]
        let n = {
            let _ = (r, cell);
            RowExtent::of(self.cols.index())
        };
        self.cells_written.add(n.index());
        self.cells_blanked.add(n.index());
        #[cfg(feature = "instrument")]
        self.cells_fill.add(n.index());
        n
    }

    /// Insert and delete rewrite everything from `at` to the margin: the
    /// shifted cells *and* the blanks that replace them. Counting only `n`
    /// would miss the shift, which is the expensive half.
    #[inline(always)]
    pub(crate) fn insert_cells_counted(
        &mut self,
        r: Row,
        at: Col,
        n: Count,
        right: Col,
        blank: PackedCell,
    ) {
        self.cells_written
            .add(right.index().saturating_sub(at.index()));
        self.view().insert_cells(r, at, n, right, blank);
    }

    #[inline(always)]
    pub(crate) fn delete_cells_counted(
        &mut self,
        r: Row,
        at: Col,
        n: Count,
        right: Col,
        blank: PackedCell,
    ) {
        self.cells_written
            .add(right.index().saturating_sub(at.index()));
        self.view().delete_cells(r, at, n, right, blank);
    }
}
