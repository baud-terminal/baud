//! The VT420 rectangular area operations, DECBI and DECFI.
//!
//! `[[source: dec-std-070]]` § 5.12.1 and § 5.12.2 (pp. 5-168 to 5-181) and
//! § 5.4.3's DECFI and DECBI (pp. 5-37 to 5-39). Every one is a control
//! function the write loop never reaches: `csi_final` and `esc_final`, both
//! `#[cold]`, call in here.
//!
//! **One cold entry, [`Paint::area_op`], which reads the parameters too.**
//! With a variant a rectangle each in `baud_domain::Csi`, read and passed
//! inside `csi_final`, its prologue saved six register pairs where it saved
//! three, on every `CSI` it dispatches.

use crate::paint::Paint;
use crate::storage::MAX_COLS;
use baud_domain::*;

/// Which cells a wide character cut at a rectangle's edge leaves alone.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Spare {
    Nothing,
    /// DECSERA: a protected character is not erased, so its other half stays.
    Protected,
}

/// How many distinct renditions one DECCARA or DECRARA remembers it has
/// already re-interned: a rectangle of one rendition interns once.
const MEMO: usize = 8;

impl Paint<'_, '_> {
    /// A rectangle operation: its parameters read, then performed.
    #[cold]
    #[inline(never)]
    pub(crate) fn area_op(&mut self, op: AreaOp, csi: &crate::write::Csi) {
        let p = csi.params();
        match op.with(&p) {
            AreaCall::Copy {
                from,
                to_top,
                to_left,
            } => self.copy_area(from, to_top, to_left),
            AreaCall::Fill { fill, area } => self.fill_area(fill, area),
            AreaCall::Erase(a) => self.erase_area(a),
            AreaCall::SelectiveErase(a) => self.selective_erase_area(a),
            AreaCall::Attrs(a, how) => self.change_area_attrs(a, how, &p),
            AreaCall::SelectExtent(e) => self.select_attr_extent(e),
            AreaCall::RequestChecksum { id, of } => self.request_area_checksum(id, of),
            AreaCall::ReportRendition(a) => self.report_area_rendition(a),
            AreaCall::SelectChecksum(ext) => self.select_checksum(ext),
        }
    }

    /// XTCHECKSUM, `CSI Ps # y`: how the next DECRQCRA sums.
    ///
    /// **Honoured whatever the checksum setting**, as xterm gates
    /// `SetChecksum` apart from `GetChecksum` and a default xterm allows
    /// the first and refuses the second (`[[source: xterm-man]]`,
    /// `disallowedWindowOps`; Jev's lean, ALWAYS, 0.34/0.44 after a second
    /// ask): with the setting off it changes no reply. **Ignored and counted
    /// below Level 4**, as xterm's `vtXX_level >= 4` (`CASE_XTERM_CHECKSUM`,
    /// `[[source: xterm-reset]]`'s file; Jev's lean, GATE, 0.10/0.04 after a
    /// second ask, UNGATED 0.24/0.34 on the first); DECRQCRA itself is not
    /// gated here.
    fn select_checksum(&mut self, ext: ChecksumExtension) {
        match self.cold.level {
            ConformanceLevel::Four => self.cold.checksum_ext = ext,
            _ => self.unsupported_total += 1,
        }
    }

    fn page_rect(&self, a: Area) -> Option<PageRect> {
        a.on_page(self.origin_row(), self.origin_col(), self.rows, self.cols)
    }

    /// The rendition a cell names.
    fn word_of(&self, id: StyleId) -> StyleWord {
        self.tmpl
            .get(id.index())
            .copied()
            .flatten()
            .unwrap_or(StyleWord::DEFAULT)
    }

    /// **`right` stopped at row `r`'s own end**, or `None` when the line
    /// ends before `left`: a rectangle's columns are character positions,
    /// "whether single or double width" (the note of DECFRA, DECERA,
    /// DECSERA, DECCARA and DECRARA, `[[source: dec-std-070]]` pp. 5-169
    /// to 5-176), and on a line not single-width "character positions
    /// cannot be addressed which are more than half the current value of
    /// the right edge of the display" (p. 5-95), which DECRQCRA, whose text
    /// (pp. 5-179 and 5-180) carries no such note, follows alone. xterm's `validRect` reads the screen's width for every row
    /// and so acts on a double line's hidden cells (`[[source:
    /// xterm-screen]]`); the text is followed. Cold: the operations' own
    /// loops read it once a row.
    fn line_right(&self, r: Row, left: Col, right: Col) -> Option<Col> {
        let end = self.line_end(r).index();
        match left.index() < end {
            true => Some(Col::at_index(right.index().min(end - 1))),
            false => None,
        }
    }

    /// One past row `r`'s last position: the screen's width on a
    /// single-width line, its half on any other.
    fn line_end(&self, r: Row) -> Cols {
        self.view.rendition(r).end_of_line(self.cols)
    }

    /// The half of a wide character outside `[left, right]` on row `r`,
    /// where an edge cuts one: **the character goes whole**, the outside
    /// half a written space in its own rendition and protection (Jev
    /// 2026-10-18, ORPHAN_SPACE, the lean taken at 0.38 and 0.20 after a
    /// second ask; xterm's `Clear1Cell` on both halves, `[[source:
    /// xterm-screen]]`). It keeps the position occupied, as the character
    /// had it, and of the character's flags DECSCA's protection alone: the
    /// ECMA-48 guard and an `OSC 8` link go (Jev's lean, PROTECTION_ONLY,
    /// 2026-10-19), where xterm's `Clear1Cell` keeps every attribute.
    fn cut_wide_edges(&mut self, r: Row, left: Col, right: Col, spare: Spare) {
        let keep = |c: PackedCell| spare == Spare::Protected && c.is_protected();
        if left.index() > 0 {
            let inside = self.view.get(r, left);
            if inside.content.is_wide_tail() && !keep(inside) {
                self.orphan_to_space(r, left.before());
            }
        }
        let past = right.after();
        if past.index() < self.cols.index() {
            let inside = self.view.get(r, right);
            if self.view.get(r, past).content.is_wide_tail() && !keep(inside) {
                self.orphan_to_space(r, past);
            }
        }
    }

    fn orphan_to_space(&mut self, r: Row, c: Col) {
        let was = self.view.get(r, c);
        let flags = CellFlags::protection(was.is_protected());
        self.view
            .put(r, c, PackedCell::glyph(Codepoint::SPACE, was.style, flags));
        self.cells = self.cells.plus(1);
    }

    /// Free an image a write over columns `[from, to)` of row `r` lands on,
    /// as text written over an image frees it ([`Paint::overwrite_images`]).
    ///
    /// **Through the text path's own hook, on the row as the cursor's**:
    /// `ImageStore::overwrite` has that one caller, and a second, here, moved
    /// what LLVM inlines into it -- cat-cjk retired more instructions a rep,
    /// ranges apart, for a call it never makes.
    fn overwrite_images_on(&mut self, r: Row, from: Col, to: Col) {
        let cy = self.cy;
        self.cy = r;
        self.overwrote(from, to);
        self.cy = cy;
    }

    /// After a rectangle operation: the selection watch told of the rows
    /// written, and the cursor's row read again, since its cells changed
    /// under it.
    fn area_written(&mut self, top: Row, bottom: Row) {
        self.wrote_rows(top, bottom);
        self.row_changed_to(self.cy);
    }

    /// DECERA, `CSI Pt ; Pl ; Pb ; Pr $ z`.
    ///
    /// "All characters are replaced with the Space character (2/0). Any
    /// unoccupied character positions remain unoccupied", "clearing all
    /// character attributes" (`[[source: dec-std-070]]` p. 5-171): an
    /// occupied cell becomes U+0020 and an unoccupied one stays empty, both
    /// in the default rendition with no protection -- not the current
    /// rendition baud's ED and EL erase in, and not xterm's, which keeps the
    /// current SGR (decided by Jev 2026-10-18, DEFAULT, 0.98 and 0.98).
    fn erase_area(&mut self, a: Area) {
        let Some(rect) = self.page_rect(a) else {
            return;
        };
        let space = PackedCell::glyph(Codepoint::SPACE, StyleId::DEFAULT, CellFlags::NONE);
        for y in rect.top.index()..=rect.bottom.index() {
            let r = Row::from_index(y);
            let Some(right) = self.line_right(r, rect.left, rect.right) else {
                continue;
            };
            self.cut_wide_edges(r, rect.left, right, Spare::Nothing);
            for x in rect.left.index()..=right.index() {
                let c = Col::at_index(x);
                let new = match self.view.get(r, c).content.is_empty() {
                    true => PackedCell::EMPTY,
                    false => space,
                };
                self.view.put(r, c, new);
            }
            self.cells = self.cells.plus(right.index() + 1 - rect.left.index());
        }
        self.area_written(rect.top, rect.bottom);
    }

    /// DECSERA, `CSI Pt ; Pl ; Pb ; Pr $ {`.
    ///
    /// "This control function erases unprotected characters ... When a
    /// character is erased, it is replaced with the Space character (2/0).
    /// Unoccupied character positions are not affected. This sequence does
    /// not change or clear any video character attributes (SGR or DECSCA)"
    /// (`[[source: dec-std-070]]` p. 5-172): an unprotected occupied cell
    /// becomes U+0020 in the rendition it had, and a protected or unoccupied
    /// one is left as it is. The protection is DECSCA's alone. The space is
    /// DECSED's ([`PackedCell::selectively_erased`]), so an ECMA-48 guard
    /// and an `OSC 8` link stay (Jev, AGREE, 1.00/0.98, 2026-10-19).
    fn selective_erase_area(&mut self, a: Area) {
        let Some(rect) = self.page_rect(a) else {
            return;
        };
        for y in rect.top.index()..=rect.bottom.index() {
            let r = Row::from_index(y);
            let Some(right) = self.line_right(r, rect.left, rect.right) else {
                continue;
            };
            self.cut_wide_edges(r, rect.left, right, Spare::Protected);
            for x in rect.left.index()..=right.index() {
                let c = Col::at_index(x);
                let was = self.view.get(r, c);
                if was.content.is_empty() || was.is_protected() {
                    continue;
                }
                self.view.put(r, c, PackedCell::selectively_erased(was));
                self.cells = self.cells.plus(1);
            }
        }
        self.area_written(rect.top, rect.bottom);
    }

    /// DECFRA, `CSI Pch ; Pt ; Pl ; Pb ; Pr $ x`.
    ///
    /// "replacing both the character and the rendition present in those
    /// character positions with the character defined by the decimal value
    /// of Pch and the current renditions set through the SGR command. ...
    /// The decimal value refers to the character in the current GL and GR
    /// 'in-use' character table" (`[[source: dec-std-070]]` p. 5-170). A GL
    /// value goes through the set GL holds, as a written byte does, and
    /// carries the line-drawing flag when that set is DEC Special Graphics;
    /// a GR value is the Latin-1 supplement ([`FillChar::gr`]). The cell
    /// takes DECSCA's protection, as a written character does, and not the
    /// open `OSC 8` link, which names text that was written. One character a
    /// position: the fill is never a wide character.
    fn fill_area(&mut self, fill: Option<FillChar>, a: Area) {
        let Some(fill) = fill else {
            self.unsupported_param += 1;
            return;
        };
        let Some(rect) = self.page_rect(a) else {
            return;
        };
        let cell = match (fill.gl(), fill.gr()) {
            (Some(b), _) => PackedCell::glyph(
                PackedCell::translate(b, self.charset),
                self.style,
                self.flags.without_link(),
            ),
            (None, Some(cp)) => PackedCell::glyph(cp, self.style, self.cold.protect),
            (None, None) => return,
        };
        for y in rect.top.index()..=rect.bottom.index() {
            let r = Row::from_index(y);
            let Some(right) = self.line_right(r, rect.left, rect.right) else {
                continue;
            };
            self.cut_wide_edges(r, rect.left, right, Spare::Nothing);
            for x in rect.left.index()..=right.index() {
                self.view.put(r, Col::at_index(x), cell);
            }
            self.overwrite_images_on(r, rect.left, right.after());
            self.cells = self.cells.plus(right.index() + 1 - rect.left.index());
        }
        self.area_written(rect.top, rect.bottom);
    }

    /// DECCRA, `CSI Pts ; Pls ; Pbs ; Prs ; Pps ; Ptd ; Pld ; Ppd $ v`.
    ///
    /// "the terminal copies the character values and attributes in the
    /// specified rectangular area ... to the area specified by the
    /// coordinate Ptd, Pld ... If the destination rectangle specified is
    /// partially off of the Page, clipping of information will occur ... The
    /// Active Position does not change" (`[[source: dec-std-070]]` p. 5-169).
    /// The destination is placed from the origin as the source is and
    /// clamped to the page; the page parameters all name baud's one page.
    ///
    /// **A copy, as if the source were read whole first**, which xterm gets
    /// from a heap buffer and this core, with no allocator, from the order:
    /// rows from the far end when the destination is below the source, and
    /// each row read into one row's buffer on the stack before it is written.
    /// A wide character the source's edge cuts is copied as a space in its
    /// own rendition, as xterm's `saveCellData` copies it, and one the
    /// destination's edge cuts goes whole ([`Paint::cut_wide_edges`]).
    ///
    /// **Each row to its own line's end** ([`Paint::line_right`]): the
    /// destination's positions past a double line's end are not written,
    /// as "only information which will appear in valid character positions
    /// in the page will be copied", and a source position past its line's
    /// end is copied as unoccupied, which is what a DEC terminal's copy
    /// gives, its cells there always empty (decided by Jev 2026-10-19,
    /// UNOCCUPIED, 1.00 and 1.00 on a re-ask; xterm copies what the row
    /// stores).
    fn copy_area(&mut self, from: Area, to_top: OneBased, to_left: OneBased) {
        let Some(src) = self.page_rect(from) else {
            return;
        };
        let top = page_row(to_top, self.origin_row(), self.rows);
        let left = page_col(to_left, self.origin_col(), self.cols);
        let h = src.rows().index().min(self.rows.index() - top.index());
        let w = src.cols().index().min(self.cols.index() - left.index());
        let mut buf = [PackedCell::EMPTY; MAX_COLS];
        let down = top > src.top;
        for i in 0..h {
            let k = match down {
                true => h - 1 - i,
                false => i,
            };
            let (sr, dr) = (
                Row::from_index(src.top.index() + k),
                Row::from_index(top.index() + k),
            );
            // The destination line's own positions, and no more: "only
            // information which will appear in valid character positions in
            // the page will be copied", and the text "take[s] on the line
            // attributes of that area".
            let Some(right) = self.line_right(dr, left, Col::at_index(left.index() + w - 1)) else {
                continue;
            };
            let n = right.index() + 1 - left.index();
            // A source position past its own line's end is no character
            // position: it is read as unoccupied.
            let src_end = self.line_end(sr).index();
            for (j, b) in buf.iter_mut().enumerate().take(n) {
                let x = src.left.index() + j;
                *b = match x < src_end {
                    true => self.view.get(sr, Col::at_index(x)),
                    false => PackedCell::EMPTY,
                };
            }
            let space = |c: PackedCell| {
                PackedCell::glyph(
                    Codepoint::SPACE,
                    c.style,
                    CellFlags::protection(c.is_protected()),
                )
            };
            // The source's edges: a tail first, a lead whose tail is past
            // the last column copied.
            if buf[0].content.is_wide_tail() {
                buf[0] = space(buf[0]);
            }
            let after = Col::at_index(src.left.index() + n);
            if after.index() <= src_end
                && after.index() < self.cols.index()
                && self.view.get(sr, after).content.is_wide_tail()
            {
                buf[n - 1] = space(buf[n - 1]);
            }
            self.cut_wide_edges(dr, left, right, Spare::Nothing);
            for (j, b) in buf.iter().enumerate().take(n) {
                self.view.put(dr, Col::at_index(left.index() + j), *b);
            }
            self.overwrite_images_on(dr, left, right.after());
            self.cells = self.cells.plus(n);
        }
        if h > 0 {
            let bottom = Row::from_index(top.index() + h - 1);
            self.area_written(top, bottom);
        }
    }

    /// DECCARA and DECRARA, `CSI Pt ; Pl ; Pb ; Pr ; Ps... $ r` and `$ t`.
    ///
    /// The positions are DECSACE's: under `STREAM` "the stream of character
    /// positions beginning with the first character position specified in
    /// the command, and ending with the second", wrapping at the page's
    /// edges, and "unoccupied positions are not affected"; under `RECTANGLE`
    /// every position in the rectangle, and "these character positions are
    /// changed to blanks and are affected" (`[[source: dec-std-070]]`
    /// p. 5-177). **Only the cells inside change**, a wide character cut at
    /// an edge included (Jev 2026-10-18, INSIDE_ONLY, 0.98 and 0.98).
    ///
    /// **A selection on the rows stays**, as xterm's `ScrnMarkRectangle`
    /// leaves it: the owner's B23 drops one for text written or erased, and
    /// these change neither.
    ///
    /// **A cell's rendition is interned again, never edited in place**: other
    /// cells, outside the rectangle, name the same slot. The colours, the
    /// font and every aspect but the four are untouched -- "This command does
    /// not change the current graphic rendition" either.
    fn change_area_attrs(&mut self, a: Area, how: AreaAttrs, p: &Params<'_>) {
        if !how.acts(p) {
            return;
        }
        let Some(rect) = self.page_rect(a) else {
            return;
        };
        let extent = self.cold.attr_extent;
        let last = Col::at_index(self.cols.index() - 1);
        let blank = Content::codepoint(Codepoint::SPACE);
        let mut memo = [(StyleId::DEFAULT, StyleId::DEFAULT); MEMO];
        let mut known = 0;
        for y in rect.top.index()..=rect.bottom.index() {
            let r = Row::from_index(y);
            let (from, to) = match extent {
                AttrExtent::Rectangle => (rect.left, rect.right),
                AttrExtent::Stream => (
                    if r == rect.top { rect.left } else { Col::ZERO },
                    if r == rect.bottom { rect.right } else { last },
                ),
            };
            // Each line to its own end, a stream's rows between the corners
            // included.
            let Some(to) = self.line_right(r, from, to) else {
                continue;
            };
            for x in from.index()..=to.index() {
                let c = Col::at_index(x);
                let mut cell = self.view.get(r, c);
                if cell.content.is_empty() {
                    match extent {
                        AttrExtent::Stream => continue,
                        AttrExtent::Rectangle => cell.content = blank,
                    }
                }
                let hit = memo[..known.min(MEMO)].iter().find(|m| m.0 == cell.style);
                cell.style = match hit {
                    Some(m) => m.1,
                    None => {
                        let w = self.word_of(cell.style);
                        let new = self.intern_style(w.with_attrs(how.apply(p, w.attrs())));
                        memo[known % MEMO] = (cell.style, new);
                        known += 1;
                        new
                    }
                };
                self.view.put(r, c, cell);
                self.cells = self.cells.plus(1);
            }
        }
        self.row_changed_to(self.cy);
    }

    /// DECSACE, `CSI Ps * x`. A value the text does not give changes
    /// nothing and is counted.
    fn select_attr_extent(&mut self, e: Option<AttrExtent>) {
        match e {
            Some(e) => self.cold.attr_extent = e,
            None => self.unsupported_param += 1,
        }
    }

    /// DECRQCRA, `CSI Pid ; Pp ; Pt ; Pl ; Pb ; Pr * y`, answered with
    /// DECCKSR ([`Report::AreaChecksum`], the algorithm at [`Checksum`]).
    ///
    /// "Pt must be less than or equal to Pb, and Pl must be less than or
    /// equal to Pr or the sequence is ignored" (`[[source: dec-std-070]]`
    /// p. 5-179): no reply then. xterm answers such a request with the
    /// checksum of nothing.
    ///
    /// **With the checksum off** (`Term::set_checksum_report`) no request
    /// is answered, whatever its corners, and each is counted in
    /// `checksums_refused`: xterm with `GetChecksum` disallowed sends
    /// nothing (`CASE_DECRQCRA`, `[[source: xterm-reset]]`'s file).
    fn request_area_checksum(&mut self, id: Param, of: ChecksumOf) {
        if self.cold.checksum_report == ChecksumReport::Off {
            self.checksums_refused += 1;
            return;
        }
        let rect = match of {
            ChecksumOf::Page => Some(PageRect {
                top: Row::ZERO,
                left: Col::ZERO,
                bottom: Row::from_index(self.rows.index() - 1),
                right: Col::at_index(self.cols.index() - 1),
            }),
            ChecksumOf::Area(a) => self.page_rect(a),
        };
        let Some(rect) = rect else {
            return;
        };
        let mut sum = Checksum::with(self.cold.checksum_ext);
        for y in rect.top.index()..=rect.bottom.index() {
            let r = Row::from_index(y);
            let Some(right) = self.line_right(r, rect.left, rect.right) else {
                continue;
            };
            for x in rect.left.index()..=right.index() {
                let cell = self.view.get(r, Col::at_index(x));
                let base = self.clusters.base_of(cell.content);
                sum.add(base, cell.flags, self.word_of(cell.style).attrs());
                if sum.wants_marks() {
                    for m in self.clusters.marks_of(cell.content) {
                        sum.mark(m);
                    }
                }
            }
        }
        self.want_reply(Report::AreaChecksum { id, sum });
    }

    /// XTREPORTSGR, `CSI Pt ; Pl ; Pb ; Pr # |`: the rendition every cell
    /// of the rectangle shares (`common_rendition`), answered as an SGR
    /// sequence through DECRQSS's encoder, so the two answers spell an
    /// attribute alike. The rectangle is read as DECRQCRA's is, from the
    /// origin and clamped to the page, each line to its own end; one that
    /// holds no cell answers `CSI 0 m`, as xterm's answers with no cell
    /// read (Jev, ANSWER_ZERO, 0.96/0.98).
    fn report_area_rendition(&mut self, a: Area) {
        let mut common: Option<StyleWord> = None;
        if let Some(rect) = self.page_rect(a) {
            for y in rect.top.index()..=rect.bottom.index() {
                let r = Row::from_index(y);
                let Some(right) = self.line_right(r, rect.left, rect.right) else {
                    continue;
                };
                for x in rect.left.index()..=right.index() {
                    let w = self.word_of(self.view.get(r, Col::at_index(x)).style);
                    common = Some(common.map_or(w, |c| common_rendition(c, w)));
                }
            }
        }
        self.report_rendition(common.unwrap_or(StyleWord::DEFAULT));
    }

    /// DECBI, `ESC 6`.
    ///
    /// `[[source: dec-std-070]]` p. 5-39: "If the active position was
    /// already at the left margin, the contents of the Logical Display within
    /// the left, right, top and bottom margin shifts right one column. The
    /// column shifted beyond the right margin is deleted. A new column is
    /// inserted at the left margin with all attributes turned off ... If the
    /// active position is outside the left or right margin when the command
    /// is received the active position moves backwards one column. If the
    /// active position was at the left edge of the page, the command is
    /// ignored." Its algorithm tests the column alone, so the rectangle
    /// moves whatever row the cursor is on, as IND and RI do from outside
    /// the margins (p. 5-24, "Continues to work"); xterm moves nothing from
    /// a row outside them (`xtermColScroll`, `[[source: xterm-scroll]]`).
    #[cold]
    #[inline(never)]
    pub(crate) fn back_index(&mut self) {
        let x = self.cx.reported(self.cold.right, self.cols);
        if x == self.cold.left && !self.cx.is_placed() {
            let left = self.cold.left;
            self.shift_columns_at(left, Count::ONE, ColumnShift::Insert, PackedCell::EMPTY);
            self.set_col(CursorCol::moved(left));
            return;
        }
        if x.index() > 0 {
            self.set_col(CursorCol::moved(x.before()));
        }
    }

    /// DECFI, `ESC 9`: the mirror (p. 5-37), shifting left at the right
    /// margin, the new column at the right margin "and the cursor appears in
    /// this column"; at the page's right edge it is ignored. **A step stops
    /// at the line's end**, a double line's half: "active line may not be
    /// columns_per_page long", and the column is cut back to
    /// `end_of_line` (p. 5-38). It stepped into the half past it, a wrap
    /// pending there.
    #[cold]
    #[inline(never)]
    pub(crate) fn forward_index(&mut self) {
        let x = self.cx.reported(self.cold.right, self.cols);
        if x == self.cold.right && !self.cx.is_placed() {
            let (left, right) = (self.cold.left, self.cold.right);
            self.shift_columns_at(left, Count::ONE, ColumnShift::Delete, PackedCell::EMPTY);
            self.set_col(CursorCol::moved(right));
            return;
        }
        if x.index() + 1 < self.line_cols().index() {
            self.set_col(self.place(x.after()));
        }
    }
}
