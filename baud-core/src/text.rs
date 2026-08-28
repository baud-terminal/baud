use crate::*;

impl<'a> Term<'a> {
    #[inline(always)]
    pub(crate) fn line(&self) -> LineSlot {
        LineSlot::of(self.head.plus(self.cy, SB))
    }

    #[inline(always)]
    pub(crate) fn view(&mut self) -> View<'_> {
        let alt = self.screen == Screen::Alternate;
        let rot = if alt { RowRot::NONE } else { self.rot };
        View::new(
            if alt { &mut self.s.alt } else { &mut self.s.vp },
            Cols::from_stride(MAX_COLS),
            self.cols,
            self.rows,
            rot,
        )
    }

    #[inline(always)]
    pub(crate) fn write_cell(&mut self, cp: Codepoint) {
        let (r, c, st) = (self.cy, self.cx, self.style);
        let fl = self.flags;
        self.put_counted(r, c, PackedCell::glyph(cp, st, fl));
    }

    /// Place one decoded codepoint. A 2-wide character at the last column
    /// cannot be split: the cell is left blank and it moves to the next row.
    /// That decision is forced by `try_advance` returning `Option`.
    #[inline(always)]
    pub(crate) fn put_cp(&mut self, cp: Codepoint, w: Width) {
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
        if self.modes.is_set(ModeBit::InsertReplace) {
            let n = match w {
                Width::WIDE => Count::TWO,
                _ => Count::ONE,
            };
            let edge = self.right_margin.after();
            self.open_gap(n, edge);
        }
        self.write_cell(cp);
        self.last_cp = Some((cp, w));
        if w == Width::WIDE {
            let (r, c, st) = (self.cy, self.cx.after(), self.style);
            let fl = self.flags;
            self.put_counted(r, c, PackedCell::wide_tail(st, fl));
        }
        self.cx = self.cx.try_advance(w, self.cols).unwrap_or(Col::ZERO);
    }

    /// Attach a zero-width codepoint to the cell the cursor last wrote.
    ///
    /// `#[inline(never)]`: `put_cp` is on the decode path and this is the rare
    /// arm, the same reason `open_gap` and `anywhere_c0` are out of line.
    #[inline(never)]
    fn attach_mark(&mut self, cp: Codepoint) {
        // A mark with nothing before it on the line has no base to attach to.
        // Dropping it is correct -- `[[source: uax29]]` GB1 puts a boundary at
        // start of text, so it is its own cluster and there is no cell for it.
        if self.cx == Col::ZERO {
            self.marks_dropped.inc();
            return;
        }
        let (r, c) = (self.cy, self.cx.before());
        let i = MarkSlot::index_of(r, c, NMARK);
        if !self.s.marks[i].is_for(r, c) {
            if !self.s.marks[i].is_empty() {
                // Someone else's marks. Evicting is the bounded-table
                // contract: the cell that loses them keeps its base character.
                self.marks_evicted.inc();
            }
            self.s.marks[i] = MarkSlot::for_cell(r, c);
        }
        if !self.s.marks[i].push(cp) {
            self.marks_dropped.inc();
        }
    }

    #[inline(always)]
    pub(crate) fn blit(&mut self, base: BytePos, off: ByteLen, room: Room) {
        let abs = base.index() + off.index();
        let (r, c, st) = (self.cy, self.cx, self.style);
        let (stride, cols, rows) = (Cols::from_stride(MAX_COLS), self.cols, self.rows);
        // Read before the split borrow below: `charset()` reads `self.g`, and
        // the destructuring holds `self.s` mutably.
        let set = self.charset();
        let (arena, vp) = (
            &self.s.arena,
            if self.screen == Screen::Alternate {
                &mut self.s.alt
            } else {
                &mut self.s.vp
            },
        );
        let fl = self.flags;
        let rot = if self.screen == Screen::Alternate {
            RowRot::NONE
        } else {
            self.rot
        };
        self.cells_written.add(room.index());
        View::new(vp, stride, cols, rows, rot).put_ascii_run(
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
            let promoted = self.screen == Screen::Alternate || self.s.promoted[self.cy.index()];
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
        self.view().put(r, c, cell);
    }

    #[inline(always)]
    pub(crate) fn fill_counted(&mut self, r: Row, from: Col, to: Col, cell: PackedCell) {
        let n = to.index().saturating_sub(from.index());
        self.cells_written.add(n);
        self.cells_blanked.add(n);
        self.view().fill(r, from, to, cell);
    }

    /// **Variant A: scan, then blank only what differs.**
    ///
    /// Blanking writes the full width whether or not the row was that wide,
    /// and `how_much_of_a_row_is_actually_used` measures the waste: vim uses
    /// 70 columns of 200, asciiquarium uses all 200. This trades a read for
    /// the writes it avoids.
    ///
    /// Compared against `cell`, **not** `PackedCell::EMPTY`. The blank carries
    /// the current style -- `empty_with(self.style)` -- so a row already empty
    /// still needs writing when a background colour is set, and comparing
    /// against EMPTY would skip exactly that case.
    #[inline(always)]
    pub(crate) fn fill_row_counted(&mut self, r: Row, cell: PackedCell) {
        let n = self
            .row(r)
            .iter()
            .rposition(|c| *c != cell)
            .map_or(0, |p| p + 1);
        if n == 0 {
            return;
        }
        self.cells_written.add(n);
        self.cells_blanked.add(n);
        self.view().fill(r, Col::ZERO, Col::at_index(n), cell);
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
