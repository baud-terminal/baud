//! **DCSM, `CSI 9 h` / `CSI 9 l`** (`[[source: ecma48]]` 7.2.3): whether
//! "certain control functions are performed in the presentation component"
//! or "in the data component" ([`DeviceComponent`]).
//!
//! **baud keeps one store, the data component**, and the presentation is
//! what is drawn from it. With BDSM reset a line is drawn in cell order,
//! reversed on a right-to-left path, and inside it each SDS or SRS string
//! in its own direction ([`baud_domain::order_strings`]): presentation
//! position `n` along the line's path holds data position `n` except on a
//! line holding a string. **There, under PRESENTATION**, the functions
//! 7.2.3 lists that baud implements take the cursor's presentation
//! position and act on the cells drawn at the positions they reach (Jev's
//! lean, BUILD, 0.22/0.22 then 0.86/0.86): `CR` goes to the cell drawn at
//! the line's home, `LF`, `NEL` and `RI` keep the presentation position on
//! the line they reach, `DCH` deletes the cells drawn at the positions it
//! covers and `ECH`, `EL` and `ED` erase them, `ICH` opens its blanks at the
//! presentation position, joining the string there (TR/53 note A.1 ii),
//! and `CPR` reports the presentation column. `DL` and `IL` act on lines,
//! the same in both. With BDSM set DCSM "has no effect" (7.3.4 c).
//!
//! **What it costs while nothing selects it: one bit.** The three
//! conditions are one bit of the mode word ([`ModeBit::PresentationEdits`]),
//! which `CR` tests beside DECLRMM's and a line feed in the test it makes
//! for LNM.

use baud_domain::{
    BidiDirection, BidiSupport, CharacterPath, Col, Count, CursorCol, DeviceComponent, EraseExtent,
    ModeBit, ModeChange, Row, StringId, order_strings,
};

use crate::MAX_COLS;
use crate::paint::Paint;
use crate::strings::Begun;

/// **One line's presentation positions**, where they differ from its data
/// positions: the position along the line's path each cell is drawn at,
/// and the cell drawn at each position, over the line's `width`.
pub(crate) struct LineMap {
    to_presentation: [u16; MAX_COLS],
    to_data: [u16; MAX_COLS],
    runs: [BidiDirection; MAX_COLS],
    pub(crate) width: usize,
}

impl LineMap {
    /// The presentation position of data column `c`, as a column counted
    /// along the line's path.
    pub(crate) fn presentation(&self, c: Col) -> Col {
        match c.index() < self.width {
            true => Col::at_index(usize::from(self.to_presentation[c.index()])),
            false => c,
        }
    }
    /// The data column drawn at presentation position `p`.
    pub(crate) fn data(&self, p: Col) -> Col {
        match p.index() < self.width {
            true => Col::at_index(usize::from(self.to_data[p.index()])),
            false => p,
        }
    }
    /// The direction data column `c` is drawn in.
    fn runs(&self, c: Col) -> BidiDirection {
        self.runs
            .get(c.index())
            .copied()
            .unwrap_or(BidiDirection::Ltr)
    }
}

impl Paint<'_, '_> {
    /// **[`ModeBit::PresentationEdits`] set as its three conditions say**:
    /// called wherever BDSM, DCSM or the strings' first begin change.
    pub(crate) fn refresh_presentation_edits(&mut self) {
        let on = self.cold.bidi == BidiSupport::Explicit
            && self.cold.dcsm == DeviceComponent::Presentation
            && self.strs.strings.begun == Begun::Yes;
        self.cold.modes.apply(ModeBit::PresentationEdits, on);
        let mapped = on || self.cold.modes.is_set(ModeBit::CursorRightToLeft);
        self.cold.modes.apply(ModeBit::ReturnMapped, mapped);
    }

    /// **DECRLM set or reset** (`crate::leftward`).
    pub(crate) fn cursor_direction(&mut self, to: ModeChange) {
        self.cold
            .modes
            .apply(ModeBit::CursorRightToLeft, to.is_set());
        if self.cold.pinned == crate::term::Pinned::LeftEdge {
            self.cold.pinned = crate::term::Pinned::Before;
        }
        self.refresh_presentation_edits();
        self.reselect();
    }

    /// **Row `r`'s map**, `None` where every presentation position holds
    /// its own data position: a line with no string.
    pub(crate) fn line_map(&self, r: Row) -> Option<LineMap> {
        let width = self
            .view
            .rendition(r)
            .end_of_line(self.cols)
            .index()
            .min(MAX_COLS);
        let mut ids = [StringId::NONE; MAX_COLS];
        let mut strung = false;
        for (c, id) in ids.iter_mut().enumerate().take(width) {
            *id = self.string_of(self.view.get(r, Col::at_index(c)).style);
            strung |= !id.is_none();
        }
        if !strung {
            return None;
        }
        let base = match self.view.bidi(r).path() {
            CharacterPath::RightToLeft => BidiDirection::Rtl,
            _ => BidiDirection::Ltr,
        };
        let mut from = [0u16; MAX_COLS];
        let mut map = LineMap {
            to_presentation: [0; MAX_COLS],
            to_data: [0; MAX_COLS],
            runs: [BidiDirection::Ltr; MAX_COLS],
            width,
        };
        let entry = |id| self.strs.strings.entry(id);
        let tail =
            |c: usize| c < width && self.view.get(r, Col::at_index(c)).content.is_wide_tail();
        order_strings(
            &ids[..width],
            tail,
            width,
            base,
            &entry,
            &mut from[..width],
            &mut map.runs[..width],
        );
        for (v, &l) in from.iter().enumerate().take(width) {
            let p = match base {
                BidiDirection::Ltr => v,
                BidiDirection::Rtl => width - 1 - v,
            };
            map.to_data[p] = l;
            map.to_presentation[usize::from(l)] = u16::try_from(p).unwrap_or(u16::MAX);
        }
        Some(map)
    }

    /// The cursor's line's map, while presentation edits may apply.
    fn cursor_map(&self) -> Option<LineMap> {
        match self.cold.modes.is_set(ModeBit::PresentationEdits) {
            true => self.line_map(self.cy),
            false => None,
        }
    }

    /// **`CR` under PRESENTATION**: the column `CR` just chose is the
    /// line's home in the presentation, and the cursor goes to the cell
    /// drawn there.
    #[cold]
    #[inline(never)]
    pub(crate) fn return_presented(&mut self) {
        match (
            self.cold.modes.is_set(ModeBit::CursorRightToLeft),
            self.cold.modes.is_set(ModeBit::SideMargins),
        ) {
            (true, _) => self.return_leftward(),
            (false, false) => self.set_col(CursorCol::moved(Col::ZERO)),
            (false, true) => self.return_fenced(),
        }
        if let Some(m) = self.cursor_map() {
            let home = self.cx.col();
            self.set_col(CursorCol::moved(m.data(home)));
        }
    }

    /// **The cursor's presentation position before a line feed, `NEL` or
    /// `RI` moves it**, for [`Paint::keep_presented`] to put it back at on
    /// the line it reaches.
    pub(crate) fn presented_col(&self) -> Option<Col> {
        if !self.cold.modes.is_set(ModeBit::PresentationEdits) {
            return None;
        }
        let c = self.cx.col();
        Some(self.line_map(self.cy).map_or(c, |m| m.presentation(c)))
    }

    /// The cursor at presentation position `p` of its line.
    pub(crate) fn keep_presented(&mut self, p: Option<Col>) {
        if let Some(p) = p {
            let c = self.line_map(self.cy).map_or(p, |m| m.data(p));
            self.set_col(CursorCol::moved(c));
        }
    }

    /// **`DCH` under PRESENTATION**: the cells drawn at the `n` positions
    /// from the cursor's, deleted from the right, the cursor kept at its
    /// presentation position. `false` where the line has no string.
    #[cold]
    #[inline(never)]
    pub(crate) fn delete_presented(&mut self, n: Count) -> bool {
        let Some(m) = self.cursor_map() else {
            return false;
        };
        let p = m.presentation(self.cx.col());
        let to = self.line_fence().after();
        let mut cols = [Col::ZERO; MAX_COLS];
        let mut k = 0;
        for q in p.index()..(p.index() + n.at_least_one()).min(m.width) {
            let c = m.data(Col::at_index(q));
            if c < to {
                cols[k] = c;
                k += 1;
            }
        }
        cols[..k].sort_unstable_by(|a, b| b.cmp(a));
        let blank = self.blank();
        for &at in &cols[..k] {
            self.view.delete_cells(self.cy, at, Count::ONE, to, blank);
            self.erase_images(self.cy, at, to);
        }
        self.cells = self.cells.plus(k);
        self.keep_presented(Some(p));
        self.row_changed_to(self.cy);
        self.wrote_rows(self.cy, self.cy);
        true
    }

    /// **`ICH` under PRESENTATION**: `n` blanks opened at the cursor's
    /// presentation position, the cell there pushed on in the presentation:
    /// before it in the data where it runs with the line's path, after it
    /// where it runs against. `false` where the line has no string.
    #[cold]
    #[inline(never)]
    pub(crate) fn insert_presented(&mut self, n: Count) -> bool {
        let Some(m) = self.cursor_map() else {
            return false;
        };
        let c = self.cx.col();
        let p = m.presentation(c);
        let base = match self.view.bidi(self.cy).path() {
            CharacterPath::RightToLeft => BidiDirection::Rtl,
            _ => BidiDirection::Ltr,
        };
        let at = match m.runs(c) == base {
            true => c,
            false => c.after(),
        };
        let to = self.line_fence().after();
        if at > to {
            return true;
        }
        let s = self.string_of(self.view.get(self.cy, c).style);
        let blank = self.blank();
        let k = n.at_least_one().min(to.index() - at.index());
        self.view.insert_cells(self.cy, at, n, to, blank);
        let end = Col::at_index(at.index() + k);
        self.strings_kept(self.cy, (at, end), blank, |_| s);
        self.erase_images(self.cy, at, to);
        self.cells = self.cells.plus(to.index() - at.index());
        self.keep_presented(Some(p));
        self.row_changed_to(self.cy);
        self.wrote_rows(self.cy, self.cy);
        true
    }

    /// **The cells drawn at presentation positions `[from, to)` of the
    /// cursor's line erased**, each where it stands.
    fn erase_presented_span(&mut self, m: &LineMap, (from, to): (Col, Col)) {
        let blank = self.blank();
        for q in from.index()..to.index().min(m.width) {
            let c = m.data(Col::at_index(q));
            self.erase_span(self.cy, c, c.after(), blank);
        }
    }

    /// **`ECH` under PRESENTATION.** `false` where the line has no string.
    #[cold]
    #[inline(never)]
    pub(crate) fn erase_chars_presented(&mut self, n: Count) -> bool {
        let Some(m) = self.cursor_map() else {
            return false;
        };
        let p = m.presentation(self.cx.col());
        let to = Col::at_index(p.index() + n.at_least_one());
        self.erase_presented_span(&m, (p, to));
        self.wrote_rows(self.cy, self.cy);
        true
    }

    /// **`EL 0` and `EL 1` under PRESENTATION**, and the cursor's line of
    /// `ED 0` and `ED 1`: from the cursor's presentation position to the
    /// line's end, or from its start through it. `EL 2` erases the line,
    /// the same in both. `false` where the line has no string.
    #[cold]
    #[inline(never)]
    pub(crate) fn erase_line_presented(&mut self, e: EraseExtent) -> bool {
        if e == EraseExtent::All {
            return false;
        }
        let Some(m) = self.cursor_map() else {
            return false;
        };
        let p = m.presentation(self.cx.col());
        let span = match e {
            EraseExtent::ToEnd => (p, Col::at_index(m.width)),
            _ => (Col::ZERO, p.after()),
        };
        self.erase_presented_span(&m, span);
        self.wrote_rows(self.cy, self.cy);
        true
    }

    /// **The column `CPR` reports under PRESENTATION**: data column `col`'s
    /// presentation position on the cursor's line.
    pub(crate) fn reported_presented(&self, col: Col) -> Col {
        match self.cursor_map() {
            Some(m) => m.presentation(col),
            None => col,
        }
    }
}
