//! **HEM, `CSI 10 h` / `CSI 10 l`** (`[[source: ecma48]]` 7.2.9): which
//! way a character insertion or deletion shifts the line. FOLLOWING, RM
//! 10, the state at power-on: "the contents of the active presentation
//! position and of the following character positions" move on along the
//! character path, as `ICH`, `DCH` and IRM always did here. PRECEDING, SM
//! 10: "the contents of the active presentation position and of preceding
//! character positions" move back toward the line's start, a deletion
//! closes its gap from there, and under IRM "the active position does not
//! move" (7.3.3 b). "Control functions affected are: DCH, ICH", and the
//! write under IRM (7.3.3 b).
//!
//! **The shifted part** runs from the line's start, the left margin under
//! DECLRMM for a cursor inside it, to the cursor, as FOLLOWING's runs from
//! the cursor to the right margin, before any SEE and under SEE 1
//! (`crate::extent` for the other extents). **A wide character stays
//! whole**, as under SEE's shifts: on its lead the part runs on through
//! its tail, so an edit on either half takes the whole character, and one
//! the part's ends or the shift would cut is erased, both halves. A wide
//! character written under PRECEDING takes the cursor's column and the one
//! before it, the positions after the cursor kept; on a wide character's
//! lead, that character's two columns.
//!
//! **Under DCSM's PRESENTATION**, on a line holding a string, the
//! positions shifted are the ones drawn ([`crate::component`]): the cells
//! drawn at the positions that leave are deleted and blanks opened where
//! the gap belongs, by the two moves `DCH` and `ICH` make there, so each
//! cell keeps its string. A character written under IRM is inserted at the
//! cursor's presentation position in either state of HEM (7.2.10,
//! "inserted at the active presentation position").
//!
//! **What it costs while nothing selects it: nothing on the text path.**
//! `DCH` and `ICH` test HEM's bit beside [`ModeBit::PresentationEdits`] in
//! one test; under IRM the text goes one character at a time, the
//! one-by-one graph SAPV's forms take, and SAPV's writer sends a character
//! here while [`ModeBit::InsertShifted`] is set, so the text path's
//! writers never read it.

use baud_domain::{Codepoint, Col, Count, ModeBit, PackedCell, StringId, Width};

use crate::component::LineMap;
use crate::paint::{Paint, Pen, Writer};

/// **`DCH` or `ICH`**, for [`Paint::edit_moded`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum CharEdit {
    Delete,
    Insert,
}

/// The writer for a character whose gap is open already: a SAPV form's
/// step, and no gap.
pub(crate) struct Gapless;

impl Writer for Gapless {
    #[inline(always)]
    fn before(p: &mut Paint<'_, '_>, cp: Codepoint, _: Count) {
        p.form_step(cp);
    }
}

/// SAPV's writer without the dispatch to this module: the form's step,
/// then the gap IRM opens after the cursor.
pub(crate) struct Gapped;

impl Writer for Gapped {
    #[inline(always)]
    fn before(p: &mut Paint<'_, '_>, cp: Codepoint, need: Count) {
        p.write_slowly(cp, need);
    }
}

impl Paint<'_, '_> {
    /// **[`ModeBit::InsertShifted`] set as its conditions say**, and the
    /// graph chosen again: called wherever IRM, HEM or
    /// [`ModeBit::PresentationEdits`] change.
    pub(crate) fn refresh_insert(&mut self) {
        let m = self.cold.modes;
        let on = m.is_set(ModeBit::InsertReplace)
            && m.either_set(ModeBit::EditPreceding, ModeBit::PresentationEdits);
        self.cold.modes.apply(ModeBit::InsertShifted, on);
        self.reselect();
    }

    /// **One graphic character under IRM while [`ModeBit::InsertShifted`]
    /// is set**: under PRECEDING at the cursor, which stays; under
    /// FOLLOWING at the cursor's presentation position, the cursor going on
    /// as after any write.
    #[cold]
    #[inline(never)]
    pub(crate) fn glyph_shifted(&mut self, cp: Codepoint, w: Width, pen: Pen) {
        if self.cold.modes.is_set(ModeBit::EditPreceding) {
            return self.glyph_preceding(cp, w, pen);
        }
        let need = 1 + usize::from(w == Width::WIDE);
        let mut map = self.edit_map();
        let raw = self.cx.raw().index();
        let fits = raw + need <= self.cold.fence.index() + 1;
        // Under the map the room is the presentation position's: a string
        // drawn to the line's edge holds the cell at its far end at the
        // string's start. A pending wrap, or a cursor past the fence, is
        // the cells' as before.
        let to = self.line_fence().index() + 1;
        let drawn = match &map {
            Some(m) if raw < to => {
                let p = m.presentation(self.cx.col()).index();
                (p < to).then_some(p + need <= to)
            }
            _ => None,
        };
        if !drawn.unwrap_or(fits) {
            // With autowrap reset a wide character too wide for the last
            // position goes in at the two positions ending at the margin,
            // the cells drawn there pushed on (Jev's lean, LAST_TWO); a line
            // with no string writes its lead at the last column instead.
            let pin = map.as_ref().filter(|_| fits).and_then(|m| {
                (!self.cold.modes.is_set(ModeBit::AutoWrap))
                    .then(|| m.data(Col::at_index(to.saturating_sub(need))))
            });
            match (fits, pin) {
                (false, _) => {
                    self.overflow(Count::of(need));
                }
                (true, Some(c)) => self.set_col(baud_domain::CursorCol::moved(c)),
                (true, None) => self.wrap_bounded(),
            }
            map = self.edit_map();
        }
        match map {
            Some(m) => {
                let p = m.presentation(self.cx.col());
                let at = self.open_following_drawn(&m, p, Count::of(need));
                self.row_changed_to(self.cy);
                self.set_col(baud_domain::CursorCol::moved(at));
                self.glyph_by::<Gapless>(cp, w, pen);
            }
            None => self.glyph_by::<Gapped>(cp, w, pen),
        }
    }

    /// **One graphic character under IRM and PRECEDING**: written at the
    /// cursor, a wide one taking the column before it too, the positions
    /// before them pushed toward the line's start, and the cursor where it
    /// was, at its presentation position under DCSM's PRESENTATION. On a
    /// wide character's lead it ends where that character's tail is, the
    /// shift taking the character whole.
    fn glyph_preceding(&mut self, cp: Codepoint, w: Width, pen: Pen) {
        let need = 1 + usize::from(w == Width::WIDE);
        self.refresh_fence();
        let fence = self.line_fence();
        let c = self.cx.col().min(fence);
        let left = self.left_limit(c);
        // A wide character at the line's start takes the column after it.
        let c = Col::at_index(c.index().max(left.index() + need - 1));
        if c > fence {
            return;
        }
        self.form_step(cp);
        let blank = self.blank();
        let (lead_col, kept) = match self.edit_map() {
            Some(m) => {
                let p = m.presentation(c);
                (self.open_preceding_drawn(&m, p, Count::of(need)), Some(p))
            }
            // Under an extent SEE chose other than the line, along it
            // (`crate::extent`). On a wide character's lead the gap opens
            // after its tail, the shift taking the whole character.
            None if self.extent_chosen() => {
                let to = crate::extent::Toward::Start;
                let x = self.presented_at(c);
                let start = self.shift_extent(x, Count::of(need), CharEdit::Insert, to);
                let end = start.filter(|&s| s > c).unwrap_or(c).after();
                (Col::at_index(end.index() - need), None)
            }
            None => {
                let (_, end) = self.open_preceding_line(c, Count::of(need), blank);
                (Col::at_index(end.index() - need), None)
            }
        };
        let style = self.style;
        let flags = match pen {
            Pen::Gl => self.flags,
            Pen::Decoded => self.decoded_pen(),
            Pen::Kept(set) => self.flags.with_received(set),
        };
        let tail = match w == Width::WIDE {
            true => Some(PackedCell::wide_tail(style, flags)),
            false => None,
        };
        let lead = PackedCell::glyph(cp, style, flags);
        self.last_cp = Some((cp, w, pen));
        self.view
            .put_glyph(self.cy, lead_col, lead, tail, PackedCell::empty_with(style));
        self.cells = self.cells.plus(need);
        let end = Col::at_index(lead_col.index() + need);
        self.overwrote(lead_col, end);
        self.row_written = self.row_written.max(end);
        self.keep_presented(kept);
        self.row_changed_to(self.cy);
    }

    /// **`n` blanks opened under PRECEDING at presentation position `p`**
    /// of the cursor's line, mapped by `m`: the cells drawn at the line's
    /// first `n` positions deleted, and the blanks opened at the position
    /// `p` has moved back to, so they are drawn at `p` and the `n - 1`
    /// before it. The column the blanks start at.
    fn open_preceding_drawn(&mut self, m: &LineMap, p: Col, n: Count) -> Col {
        let k = Count::of(n.at_least_one().min(p.index() + 1));
        self.delete_drawn(m, Col::ZERO, k);
        let at = Col::at_index(p.index() + 1 - k.index());
        let after = self.line_map(self.cy);
        self.insert_drawn(after.as_ref(), at, k)
    }

    /// **`DCH` or `ICH` under PRECEDING, or under DCSM's PRESENTATION**,
    /// out of line: `false` where neither applies, which leaves the cells'
    /// shift to the caller.
    #[cold]
    #[inline(never)]
    pub(crate) fn edit_moded(&mut self, n: Count, edit: CharEdit) -> bool {
        match (self.cold.modes.is_set(ModeBit::EditPreceding), edit) {
            (true, CharEdit::Delete) => self.delete_preceding(n),
            (true, CharEdit::Insert) => self.insert_preceding(n),
            (false, CharEdit::Delete) => return self.delete_presented(n),
            (false, CharEdit::Insert) => return self.insert_presented(n),
        }
        true
    }

    /// **`DCH` under PRECEDING** (`[[source: ecma48]]` 8.3.26): the active
    /// position and the `n - 1` before it removed, the positions before
    /// them shifted on to close the gap, and `n` blanks at the line's
    /// start. Right of the fence it is ignored, as FOLLOWING's is.
    fn delete_preceding(&mut self, n: Count) {
        let c = self.cx.col();
        if c > self.line_fence() {
            return;
        }
        self.edit_reaches(crate::aligned::EditRows::Chars);
        match self.cursor_map() {
            Some(m) => {
                let p = m.presentation(c);
                let k = Count::of(n.at_least_one().min(p.index() + 1));
                self.delete_drawn(&m, Col::at_index(p.index() + 1 - k.index()), k);
                let after = self.line_map(self.cy);
                self.insert_drawn(after.as_ref(), Col::ZERO, k);
                self.keep_presented(Some(p));
            }
            None => {
                let blank = self.blank();
                let (left, end, took) = self.preceding_part(c, blank);
                // The `n` positions from the cursor's own, and the half the
                // tail took in (8.3.26); a span whose last position is a
                // wide character's tail takes its lead, as
                // `Grid::delete_cells` widens one.
                let len = end.index() - left.index();
                let mut k = (n.at_least_one() + usize::from(took)).min(len);
                let last = Col::at_index(end.index() - k);
                if last > left && self.is_tail(last) {
                    k += 1;
                }
                self.view
                    .insert_cells(self.cy, left, Count::of(k), end, blank);
                self.erase_images(self.cy, left, end);
                self.cells = self.cells.plus(len);
            }
        }
        self.row_changed_to(self.cy);
        self.wrote_rows(self.cy, self.cy);
    }

    /// **`ICH` under PRECEDING** (8.3.64): `n` blanks at
    /// the active position and the `n - 1` before it, what stood there and
    /// before it shifted toward the line's start and the first `n` leaving
    /// the line. The blanks join the string of the character they pushed
    /// (TR/53 note A.1 i). Right of the fence it is ignored.
    fn insert_preceding(&mut self, n: Count) {
        let c = self.cx.col();
        if c > self.line_fence() {
            return;
        }
        self.edit_reaches(crate::aligned::EditRows::Chars);
        match self.cursor_map() {
            Some(m) => {
                let p = m.presentation(c);
                self.open_preceding_drawn(&m, p, n);
                self.keep_presented(Some(p));
            }
            None => {
                let blank = self.blank();
                let (left, after) = self.open_preceding_line(c, n, blank);
                let k = n.at_least_one().min(after.index() - left.index());
                let from = Col::at_index(after.index() - k);
                if self.strs.strings.begun == crate::strings::Begun::Yes {
                    let s = match from > left {
                        true => self.string_of(self.view.get(self.cy, from.before()).style),
                        false => StringId::NONE,
                    };
                    self.strings_kept(self.cy, (from, after), blank, |_| s);
                }
            }
        }
        self.row_changed_to(self.cy);
        self.wrote_rows(self.cy, self.cy);
    }

    /// **`n` blanks opened under PRECEDING on the cursor's line**, its
    /// cells' shift where no map draws it: the shifted part's first `n`
    /// positions leave the line and the blanks end where it does. A wide
    /// character whose lead would leave and whose tail stay is erased,
    /// both halves, as SEE's shift erases one it pushes off. The shifted
    /// part, as [`Paint::preceding_part`] gives it.
    fn open_preceding_line(&mut self, c: Col, n: Count, blank: PackedCell) -> (Col, Col) {
        let (left, end, _) = self.preceding_part(c, blank);
        let k = n.at_least_one().min(end.index() - left.index());
        let kept = Col::at_index(left.index() + k);
        if kept < end && self.is_tail(kept) {
            self.erase_whole(kept.before(), blank);
        }
        self.view
            .delete_cells(self.cy, left, Count::of(k), end, blank);
        self.erase_images(self.cy, left, end);
        self.cells = self.cells.plus(end.index() - left.index());
        (left, end)
    }

    /// **The shifted part of the cursor's line under PRECEDING**, `[left,
    /// end)`: from the line's start to the cursor, and on to a wide
    /// character's tail where the cursor is on its lead, so an edit on
    /// either half takes the whole character, as SEE's shifts do
    /// (`crate::extent`); and whether it took the tail in. A character the
    /// part's ends would cut is erased first, both halves: a tail past the
    /// fence, or one at the line's start whose lead is before it.
    fn preceding_part(&mut self, c: Col, blank: PackedCell) -> (Col, Col, bool) {
        let left = self.left_limit(c);
        if left.index() > 0 && self.is_tail(left) {
            self.erase_whole(left.before(), blank);
        }
        let after = c.after();
        if !self.is_tail(after) {
            return (left, after, false);
        }
        if after <= self.line_fence() {
            return (left, after.after(), true);
        }
        self.erase_whole(c, blank);
        (left, after, false)
    }

    /// Whether column `x` of the cursor's line holds a wide character's
    /// tail; `false` past the line.
    fn is_tail(&self, x: Col) -> bool {
        x.index() < self.cols.index() && self.view.get(self.cy, x).content.is_wide_tail()
    }

    /// **The wide character whose lead is at `lead` erased**, both halves,
    /// each position kept in the string it was in.
    fn erase_whole(&mut self, lead: Col, blank: PackedCell) {
        let (r, to) = (self.cy, lead.after().after());
        let was = match self.strs.strings.begun == crate::strings::Begun::Yes {
            true => [lead, lead.after()].map(|c| self.string_of(self.view.get(r, c).style)),
            false => [StringId::NONE; 2],
        };
        self.view.put(r, lead, blank);
        self.view.put(r, lead.after(), blank);
        self.strings_kept(r, (lead, to), blank, |c| was[c.index() - lead.index()]);
        self.erase_images(r, lead, to);
        self.cells = self.cells.plus(2);
    }
}
