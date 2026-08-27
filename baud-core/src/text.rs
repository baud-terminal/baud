use crate::*;

impl<'a> Term<'a> {
    #[inline(always)]
    pub(crate) fn line(&self) -> LineSlot {
        LineSlot::of(self.head.plus(self.cy, SB))
    }

    #[inline(always)]
    pub(crate) fn view(&mut self) -> View<'_> {
        View::new(
            if self.screen == Screen::Alternate {
                &mut self.s.alt
            } else {
                &mut self.s.vp
            },
            Cols::from_stride(MAX_COLS),
            self.cols,
            self.rows,
        )
    }

    #[inline(always)]
    pub(crate) fn write_cell(&mut self, cp: Codepoint) {
        let (r, c, st) = (self.cy, self.cx, self.style);
        let fl = self.flags;
        self.view().put(r, c, PackedCell::glyph(cp, st, fl));
    }

    /// Place one decoded codepoint. A 2-wide character at the last column
    /// cannot be split: the cell is left blank and it moves to the next row.
    /// That decision is forced by `try_advance` returning `Option`.
    #[inline(always)]
    pub(crate) fn put_cp(&mut self, cp: Codepoint, w: Width) {
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
            let fl = self.flags;
            self.view().put(r, c, PackedCell::wide_tail(st, fl));
        }
        self.cx = self.cx.try_advance(w, self.cols).unwrap_or(Col::ZERO);
    }

    #[inline(always)]
    pub(crate) fn blit(&mut self, base: BytePos, off: ByteLen, room: Room) {
        let abs = base.index() + off.index();
        let (r, c, st) = (self.cy, self.cx, self.style);
        let (stride, cols, rows) = (Cols::from_stride(MAX_COLS), self.cols, self.rows);
        let (arena, vp) = (
            &self.s.arena,
            if self.screen == Screen::Alternate {
                &mut self.s.alt
            } else {
                &mut self.s.vp
            },
        );
        let fl = self.flags;
        View::new(vp, stride, cols, rows).put_ascii_run(
            r,
            c,
            &arena[abs..abs + room.index()],
            st,
            fl,
        );
    }

    #[inline(always)]
    pub(crate) fn put_run(&mut self, base: BytePos, mut off: ByteLen, mut n: ByteLen) {
        // Hoisted: a mode cannot change inside a run, and reading it per
        // iteration cost 12% of streaming throughput -- measured, not guessed.
        let wrap = self.modes.is_set(ModeBit::AutoWrap);
        // Hoisted for the same reason as `wrap`: DECSLRM cannot change inside a
        // run. With no side margins set this is one past the last column, so
        // the margin-aware form is also the fast form and there is no branch.
        let edge = self.right_margin.after();
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
                        flags: 0,
                        _pad: [0; 2],
                    }
                    .with_encoding(Encoding::Ascii)
                    .with_flags(self.flags);
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
                self.view().put(rr, cc, PackedCell::from_ascii(b, st, fl));
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
    pub(crate) fn promote_if_runs(&mut self) {
        if self.mode == OpMode::Runs && self.screen == Screen::Normal {
            self.promote();
        }
    }
}
