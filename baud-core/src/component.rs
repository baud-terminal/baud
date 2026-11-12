//! **DCSM, `CSI 9 h` / `CSI 9 l`** (`[[source: ecma48]]` 7.2.3): whether
//! "certain control functions are performed in the presentation component"
//! or "in the data component" ([`DeviceComponent`]).
//!
//! **baud keeps one store, the data component**, and the presentation is
//! what is drawn from it. With BDSM reset a line is drawn in cell order,
//! reversed on a right-to-left path, and inside it each SDS or SRS string in
//! its own direction ([`baud_domain::order_strings`]): presentation position
//! `n` along the line's path holds data position `n` except on a line
//! holding a string. **There, under PRESENTATION**, the functions 7.2.3
//! lists that baud implements take the cursor's presentation position and
//! act on the cells drawn at the positions they reach: `CR` goes to the cell
//! drawn at the line's home, `LF`, `NEL` and `RI` keep the presentation
//! position on the line they reach, `DCH` deletes the cells drawn at the
//! positions it covers and `ECH`, `EL` and `ED` erase them, `ICH` opens its
//! blanks at the presentation position, joining the string there (TR/53 note
//! A.1 ii), and `CPR` reports the presentation column. `DL` and `IL` act on
//! lines, the same in both. With BDSM set DCSM "has no effect" (7.3.4 c).
//!
//! **The cursor functions move "the active presentation position"**
//! (8.3.7-8.3.23, 8.3.60, 8.3.161) **in both DCSM states**: their
//! definitions have no DCSM paragraph, 7.2.3's NOTE does not list them and
//! Annex F F.7, informative, calls them "independent of the setting of"
//! DCSM. With BDSM reset `CUP`, `CHA`, `CUF`, `CUB`, `CUU`, `CUD`, `CNL`,
//! `CPL`, `HT`, `CHT`, `CBT`, `CVT` and `VT` move from the cursor's
//! presentation position and leave it on the cell drawn where they end
//! ([`Paint::presented`]), and `HTS`, `CTC` and `TBC` take the stop's
//! column there. TR/53's worked example 2 reaches data columns with `CUP`
//! and `CHA` on its DATA lines; `HVP` and `HPA`, which move the active
//! data position, by cell in both states, reach them here. `CUF` is
//! "rightwards in the presentation component" and `CUB` "leftwards", so on
//! a right-to-left line they run back and on along its path.
//!
//! **What it costs while nothing selects it: one bit.** The three
//! conditions are one bit of the mode word ([`ModeBit::PresentationEdits`]),
//! which `CR` tests beside DECLRMM's and a line feed in the test it makes
//! for LNM.
//! The cursor functions test another, [`ModeBit::CursorPresented`], which
//! a path selected sets as a string begun does, in one arm of the `CSI`
//! dispatch, out of the per-byte loop; `HT` tests `CR`'s,
//! [`ModeBit::ReturnMapped`], in place of DECRLM's, set with BDSM reset
//! and a string begun in either DCSM state, and `CR` maps only under
//! PRESENTATION, behind it.

use baud_domain::{
    BidiDirection, BidiSupport, CharacterPath, Col, Cols, Count, Csi, CursorAt, CursorCol,
    DeviceComponent, EraseExtent, ModeBit, ModeChange, PackedCell, Row, StringId, order_strings,
};

use crate::MAX_COLS;
use crate::paint::Paint;
use crate::strings::Begun;

/// **Which line start a return takes**: `CR`'s, the line home once SLH has
/// set one, or `CNL`'s and `CPL`'s "first character position".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum LineStart {
    Home,
    /// `CR` before a line feed's move, whose home is the following line's.
    Following,
    First,
}

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

/// **Where a shift along the cursor's line ends**, one past the last
/// column it moves: [`Paint::fence_end`] for the edits the side margins
/// bound, [`Paint::insert_end`] for a character written under IRM. Chosen
/// once, by the caller, for the cursor, and passed down, so the cells a
/// shift deletes and the gap it opens end at the same column.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct ShiftEnd(Col);

impl ShiftEnd {
    /// The column one past the last the shift moves.
    fn col(self) -> Col {
        self.0
    }

    /// **The columns from `at` to this end**, `None` where `at` lies past
    /// it: there a gap shifts nothing, and its length would be negative.
    fn from(self, at: Col) -> Option<Shifted> {
        (at <= self.0).then_some(Shifted { at, end: self.0 })
    }
}

/// **The columns `[at, end)` a shift moves**, built only by
/// [`ShiftEnd::from`], where `at` is at or before `end`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Shifted {
    at: Col,
    end: Col,
}

impl Shifted {
    /// How many columns the shift moves.
    fn len(self) -> Cols {
        Cols::of_index(self.end.index() - self.at.index())
    }
}

/// **The columns HEM's PRECEDING shifts within**, `[lo, hi]` of the
/// cursor's line, on the side of the right margin the cursor is on: from
/// the left margin, or the line's start for a cursor left of it, to the
/// fence for a cursor at or inside the fence; from one past the fence to
/// the line's last column for a cursor a move put right of it, the mirror
/// of the line's end DEC's insert-or-replace shifts to there
/// ([`Paint::insert_end`]). Built only by [`Paint::preceding_zone`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct PrecedingZone {
    lo: Col,
    hi: Col,
}

impl PrecedingZone {
    /// The zone's first column, where the shifted part starts on a line
    /// with no string.
    pub(crate) fn lo(self) -> Col {
        self.lo
    }
    /// The zone's last column.
    pub(crate) fn hi(self) -> Col {
        self.hi
    }
    /// Whether column `c` is in the zone.
    fn holds(self, c: Col) -> bool {
        c >= self.lo && c <= self.hi
    }
    /// Where a shift inside the zone ends, one past its last column.
    pub(crate) fn end(self) -> ShiftEnd {
        ShiftEnd(self.hi.after())
    }
}

/// **The positions HEM's PRECEDING shifts under PRESENTATION**, `[home,
/// cursor]` along the line's path: back from the cursor's presentation
/// position toward the line's home while the cell drawn at each is in the
/// [`PrecedingZone`], so a string drawn across a side margin is shifted
/// only on the cursor's side of it. Built only by
/// [`Paint::preceding_drawn`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct DrawnPart {
    home: Col,
    cursor: Col,
}

impl DrawnPart {
    /// How many positions the part holds, the cursor's included.
    pub(crate) fn len(self) -> Cols {
        Cols::of_index(self.cursor.index() + 1 - self.home.index())
    }
}

/// **The lines a walk over positions reads**, `EA`'s and `EF`'s: in the
/// presentation component, while [`ModeBit::PresentationEdits`] is set,
/// each line's map, the last one read kept; in the data component every
/// position its own cell.
pub(crate) struct Drawn {
    component: DeviceComponent,
    row: Option<Row>,
    map: Option<LineMap>,
}

impl Drawn {
    /// The walk for the edits the mode word says: PRESENTATION's while
    /// they take the positions as drawn.
    pub(crate) fn of(modes: baud_domain::Modes) -> Drawn {
        let component = match modes.is_set(ModeBit::PresentationEdits) {
            true => DeviceComponent::Presentation,
            false => DeviceComponent::Data,
        };
        Drawn {
            component,
            row: None,
            map: None,
        }
    }

    /// The walk for the functions that act in the presentation component
    /// in both DCSM states, as the cursor functions do: HTJ and the aligned
    /// stops (`crate::aligned`).
    pub(crate) fn presented(modes: baud_domain::Modes) -> Drawn {
        let component = match modes.is_set(ModeBit::CursorPresented) {
            true => DeviceComponent::Presentation,
            false => DeviceComponent::Data,
        };
        Drawn {
            component,
            row: None,
            map: None,
        }
    }

    /// Whether the walk takes the positions as drawn.
    pub(crate) fn is_drawn(&self) -> bool {
        self.component == DeviceComponent::Presentation
    }

    /// Row `r`'s map, `None` in the data component or where the row holds
    /// no string.
    fn read(&mut self, p: &Paint<'_, '_>, r: Row) -> Option<&LineMap> {
        if self.component == DeviceComponent::Data {
            return None;
        }
        if self.row != Some(r) {
            self.map = p.line_map(r);
            self.row = Some(r);
        }
        self.map.as_ref()
    }

    /// Row `r`'s map, taken out for an erase that writes the row.
    pub(crate) fn map_of(&mut self, p: &Paint<'_, '_>, r: Row) -> Option<LineMap> {
        self.read(p, r)?;
        self.row = None;
        self.map.take()
    }
}

impl Paint<'_, '_> {
    /// The cell drawn at presentation position `p` of row `r`.
    pub(crate) fn drawn_at(&self, d: &mut Drawn, r: Row, p: Col) -> Col {
        d.read(self, r).map_or(p, |m| m.data(p))
    }

    /// The presentation position data column `c` of row `r` is drawn at.
    pub(crate) fn drawn_from(&self, d: &mut Drawn, r: Row, c: Col) -> Col {
        d.read(self, r).map_or(c, |m| m.presentation(c))
    }

    /// **The cells drawn at presentation positions `[from, to)` of row `r`
    /// erased**, each where it stands.
    pub(crate) fn erase_drawn_span(
        &mut self,
        r: Row,
        m: &LineMap,
        (from, to): (Col, Col),
        blank: PackedCell,
    ) {
        for q in from.index()..to.index().min(m.width) {
            let c = m.data(Col::at_index(q));
            self.erase_span(r, c, c.after(), blank);
        }
    }

    /// **[`ModeBit::PresentationEdits`] set as its three conditions say**:
    /// called wherever BDSM, DCSM or the strings' first begin change.
    pub(crate) fn refresh_presentation_edits(&mut self) {
        let on = self.cold.bidi == BidiSupport::Explicit
            && self.cold.dcsm == DeviceComponent::Presentation
            && self.strs.strings.begun == Begun::Yes;
        self.cold.modes.apply(ModeBit::PresentationEdits, on);
        // The cursor functions take the presentation position in both DCSM
        // states, so `HT` maps wherever a string was begun.
        let strung =
            self.cold.bidi == BidiSupport::Explicit && self.strs.strings.begun == Begun::Yes;
        // And once SLH has set a line home, which `CR` goes to
        // (`crate::bounds`).
        let mapped = strung
            || self.cold.modes.is_set(ModeBit::CursorRightToLeft)
            || self.cold.bounds.home.is_some()
            || crate::bounds::limit_bounds(self.cold)
            || self.strs.aligned.any();
        self.cold.modes.apply(ModeBit::ReturnMapped, mapped);
        let presented = self.cold.bidi == BidiSupport::Explicit
            && (self.strs.strings.begun == Begun::Yes
                || self.cold.modes.is_set(ModeBit::PathSelected));
        self.cold.modes.apply(ModeBit::CursorPresented, presented);
        self.refresh_routed();
        self.refresh_bounds();
        self.refresh_insert();
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
    pub(crate) fn cursor_map(&self) -> Option<LineMap> {
        match self.cold.modes.is_set(ModeBit::PresentationEdits) {
            true => self.line_map(self.cy),
            false => None,
        }
    }

    /// **`CR` under PRESENTATION**: the column `CR` just chose is the
    /// line's home in the presentation, and the cursor goes to the cell
    /// drawn there. Once SLH has set a line home, `CR` goes no further back
    /// than it (`crate::bounds`).
    #[cold]
    #[inline(never)]
    pub(crate) fn return_presented(&mut self) {
        self.return_mapped(LineStart::Home);
    }

    /// **`CR` before a line feed, `NEL`'s and LNM's**, whose home is "the
    /// line home position of the following line" (`[[source: ecma48]]`
    /// 8.3.86): bounded by the screen here, and by the line the feed
    /// reaches in [`Paint::fed_onto_line`]. Bounded by the line it left, a
    /// double line's half was carried onto a single line below it.
    pub(crate) fn return_before_feed(&mut self) {
        match self.cold.modes.is_set(ModeBit::ReturnMapped) {
            true => self.return_mapped(LineStart::Following),
            false => self.carriage_return(),
        }
    }

    /// **`CNL`'s and `CPL`'s line start**: "the first character position"
    /// (8.3.12, 8.3.13), `CR`'s without SLH's line home. Out of line, as
    /// the rest of a mapped `CR` is.
    #[cold]
    #[inline(never)]
    pub(crate) fn line_start(&mut self) {
        match self.cold.modes.is_set(ModeBit::ReturnMapped) {
            true => self.return_mapped(LineStart::First),
            false => self.carriage_return(),
        }
    }

    /// `CR`'s column, the line home's once SLH has set one for
    /// [`LineStart::Home`], mapped to the cell drawn there under
    /// PRESENTATION.
    fn return_mapped(&mut self, to: LineStart) {
        self.effector_ends_string();
        self.line_home();
        let homed = match to {
            LineStart::Home => Some(self.homed(self.cx.col())),
            LineStart::Following => Some(self.homed_within(self.cx.col(), self.cols)),
            LineStart::First => None,
        };
        if let Some(c) = homed {
            self.set_col(CursorCol::moved(c));
        }
        if let Some(m) = self.cursor_map() {
            let home = self.cx.col();
            self.set_col(CursorCol::moved(m.data(home)));
        }
        // **Past column 0 the cluster machine is told a control went past**,
        // as `return_fenced` tells it: `CR` leads to the ground because at
        // column 0 a write restarts the machine itself, and under DECRLM the
        // home is the right margin, where a mark after `CR LF` in one read
        // joined the cluster before the return and, in two, took a cell.
        if self.cx.col() != Col::ZERO {
            self.cluster = baud_domain::ClusterBreak::after_control();
        }
    }

    /// The cursor at the column `CR` chooses, before any map: the line's
    /// home, the right margin under DECRLM.
    fn line_home(&mut self) {
        match (
            self.cold.modes.is_set(ModeBit::CursorRightToLeft),
            self.cold.modes.is_set(ModeBit::SideMargins),
        ) {
            (true, _) => self.return_leftward(),
            (false, false) => self.set_col(CursorCol::moved(Col::ZERO)),
            (false, true) => self.return_fenced(),
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

    /// **The cursor's presentation position for a cursor function**:
    /// `VT`'s, and the column `HTS`, `CTC` and `TBC` set or clear a stop
    /// at, which take "the active presentation position" in both DCSM
    /// states (8.3.161, 8.3.62, 8.3.17, 8.3.154).
    pub(crate) fn cursor_presented_col(&self) -> Option<Col> {
        if !self.cold.modes.is_set(ModeBit::CursorPresented) {
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

    /// **A cursor function with BDSM reset** (`[[source: ecma48]]`
    /// 8.3.7-8.3.23: each moves "the active presentation position", in both
    /// DCSM states): `f`
    /// moves the cursor from its presentation position, counted along the
    /// line's path, and the cursor goes to the cell drawn where `f` leaves
    /// it, on the line it reaches. A line with no string, a cursor at a
    /// pending wrap or placed past the margin are taken as they stand.
    #[cold]
    #[inline(never)]
    pub(crate) fn presented(&mut self, f: impl FnOnce(&mut Self)) {
        self.map_cursor(LineMap::presentation);
        self.landed(f);
    }

    /// **An absolute move**, `CUP`'s, `CHA`'s, `CNL`'s or `CPL`'s: `f`
    /// names the position, and the cursor goes to the cell drawn there; the
    /// position it left is not read.
    fn landed(&mut self, f: impl FnOnce(&mut Self)) {
        f(self);
        self.map_cursor(LineMap::data);
    }

    /// The cursor's column through `to` of its line's map, where it stands
    /// on a column of a line holding a string.
    fn map_cursor(&mut self, to: fn(&LineMap, Col) -> Col) {
        if !self.cold.modes.is_set(ModeBit::CursorPresented) {
            return;
        }
        if let (CursorAt::Column(c), Some(m)) = (self.cursor_at(), self.line_map(self.cy)) {
            if c.index() < m.width {
                self.set_col(CursorCol::moved(to(&m, c)));
            }
        }
    }

    /// **A cursor function while [`ModeBit::CursorPresented`] is set**,
    /// each the move its arm in `csi_final` makes, through
    /// [`Paint::presented`].
    ///
    /// **Reached by [`ModeBit::CursorRouted`]**, which an open aligned
    /// string sets too: the string ends at the move ("or other cursor
    /// movement"), wherever the move leaves the cursor, and with
    /// [`ModeBit::CursorPresented`] reset the move is the plain arm's
    /// ([`Paint::cursor_plain`]). The string's own bit, tested before the
    /// dispatch's match, read 0.15-0.55% more instructions on `less`,
    /// `asciiquarium`, `btop` and `vim`; this bit replaces the one the arm
    /// tested.
    #[cold]
    #[inline(never)]
    pub(crate) fn cursor_presented(&mut self, op: Csi) {
        self.effector_ends_string();
        if !self.cold.modes.is_set(ModeBit::CursorPresented) {
            return self.cursor_plain(op);
        }
        match op {
            Csi::CursorPosition { row, col } => self.landed(|p| {
                let base = p.origin_row().index();
                let want = Row::from_index(base + row.to_row(p.rows).index());
                p.goto_at(p.confine(want), p.address_col(col.to_col(p.cols)));
            }),
            Csi::CursorUp(c) => self.presented(|p| {
                p.end_wait();
                p.set_row(p.up_in_margins(c))
            }),
            Csi::CursorDown(c) => self.presented(|p| {
                p.end_wait();
                p.set_row(p.down_in_margins(c))
            }),
            Csi::CursorForward(c) => {
                self.end_limit_wait();
                self.rightward_presented(c)
            }
            Csi::CursorBack(c) => self.leftward_presented(c),
            Csi::ColumnAbsolute(c) => {
                self.landed(|p| p.set_col(p.on_line(p.place(p.confine_col(c.to_col(p.cols))))))
            }
            // "The first character position of the n-th following line in
            // the presentation component" (8.3.12, 8.3.13): `CR`'s home,
            // mapped here whatever DCSM says, where `CR`'s map follows it.
            Csi::CursorNextLine(c) => self.landed(|p| {
                p.set_row(p.down_in_margins(c));
                p.line_home();
            }),
            Csi::CursorPrevLine(c) => self.landed(|p| {
                p.set_row(p.up_in_margins(c));
                p.line_home();
            }),
            Csi::CursorForwardTab(c) => {
                self.end_limit_wait();
                self.presented(|p| p.tab_forward(c));
                self.tab_landed();
            }
            Csi::CursorBackTab(c) => {
                self.end_wait_before_back_tab();
                self.presented(|p| p.tab_back(c))
            }
            Csi::CursorLineTab(c) => self.presented(|p| p.line_tab(c)),
            // The data-position moves and `TBC`, routed here only to end a
            // string, go as their plain arms go.
            op => self.cursor_plain(op),
        }
    }

    /// **A routed cursor function as its plain arm in `csi_dispatch`
    /// makes it**, for [`Paint::cursor_presented`] where the presentation
    /// does not take it: the same calls, kept beside the arm's.
    fn cursor_plain(&mut self, op: Csi) {
        match op {
            Csi::CursorPosition { row, col } | Csi::CharacterLinePosition { row, col } => {
                let base = self.origin_row().index();
                let want = Row::from_index(base + row.to_row(self.rows).index());
                self.goto_at(self.confine(want), self.address_col(col.to_col(self.cols)));
            }
            Csi::CursorUp(c) => {
                self.end_wait();
                self.set_row(self.up_in_margins(c))
            }
            Csi::CursorDown(c) => {
                self.end_wait();
                self.set_row(self.down_in_margins(c))
            }
            Csi::CursorForward(c) => self.set_col(self.forward_in_margins(c)),
            Csi::CursorBack(c) => match (self.wraps_back_extended(), self.wraps_back()) {
                (true, _) => self.back_wrapping(c),
                (false, true) => self.back_reverse_wrapping(c),
                (false, false) => self.set_col(self.back_in_margins(c)),
            },
            Csi::CursorNextLine(c) => {
                self.set_row(self.down_in_margins(c));
                self.line_start();
            }
            Csi::CursorPrevLine(c) => {
                self.set_row(self.up_in_margins(c));
                self.line_start();
            }
            Csi::ColumnAbsolute(c) | Csi::CharacterPositionAbsolute(c) => {
                self.set_col(self.on_line(self.place(self.confine_col(c.to_col(self.cols)))))
            }
            Csi::LineAbsolute(r) => {
                let base = self.origin_row().index();
                let want = Row::from_index(base + r.to_row(self.rows).index());
                self.set_row(self.confine(want));
            }
            Csi::ColumnRelative(c) => {
                let from = self.cx.reported(self.report_fence(), self.cols);
                self.set_col(self.on_line(self.place(self.confine_col(from.forward(c, self.cols)))))
            }
            Csi::LineRelative(c) => self.set_row(self.confine(self.cy.down(c, self.rows))),
            Csi::ColumnBackward(c) => {
                let from = self.cx.reported(self.report_fence(), self.cols);
                self.set_col(self.on_line(self.place(self.confine_col(from.back(c)))))
            }
            Csi::LineBackward(c) => self.set_row(self.confine(self.cy.up(c))),
            Csi::CursorForwardTab(c) => {
                self.end_limit_wait();
                self.tab_forward(c);
                self.tab_landed();
            }
            Csi::CursorBackTab(c) => {
                self.end_wait_before_back_tab();
                self.tab_back(c)
            }
            Csi::CursorLineTab(c) => self.line_tab(c),
            Csi::TabClear(w) => self.tab_clear(w),
            // The arm that calls this admits only the functions above.
            _ => {}
        }
    }

    /// Whether the cursor's line runs right to left.
    fn runs_leftward(&self) -> bool {
        self.view.bidi(self.cy).path() == CharacterPath::RightToLeft
    }

    /// **`CUF` with BDSM reset**: "rightwards in the presentation
    /// component" (8.3.20), which on a right-to-left line is back along its
    /// path, toward its home.
    pub(crate) fn rightward_presented(&mut self, n: Count) {
        self.presented(|p| {
            let to = match p.runs_leftward() {
                true => p.back_in_margins(n),
                false => p.forward_in_margins(n),
            };
            p.set_col(to);
        });
    }

    /// **`CUB` with BDSM reset**: "leftwards" (8.3.18), on a
    /// right-to-left line on along its path; on a left-to-right line as
    /// `CUB` is, `?1045` and `?45` included.
    pub(crate) fn leftward_presented(&mut self, n: Count) {
        self.presented(
            |p| match (p.runs_leftward(), p.wraps_back_extended(), p.wraps_back()) {
                (true, _, _) => p.set_col(p.forward_in_margins(n)),
                (false, true, _) => p.back_wrapping(n),
                (false, false, true) => p.back_reverse_wrapping(n),
                (false, false, false) => p.set_col(p.back_in_margins(n)),
            },
        );
    }

    /// **`HT` while `CR` is mapped** ([`ModeBit::ReturnMapped`]): under
    /// DECRLM to the left (`crate::leftward`), and with BDSM reset and a
    /// string begun from the cursor's presentation position, the stops
    /// counted along the line's path (8.3.60), in both DCSM states.
    #[cold]
    #[inline(never)]
    pub(crate) fn tab_mapped(&mut self) {
        self.end_limit_wait();
        let leftward = self.cold.modes.is_set(ModeBit::CursorRightToLeft);
        self.presented(|p| match leftward {
            true => p.tab_leftward(),
            false => p.tab_forward(Count::ONE),
        });
        self.tab_landed();
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
        self.edit_reaches(crate::aligned::EditRows::Chars);
        let p = m.presentation(self.cx.col());
        self.delete_drawn(&m, p, n, self.fence_end());
        self.keep_presented(Some(p));
        self.row_changed_to(self.cy);
        self.wrote_rows(self.cy, self.cy);
        true
    }

    /// **The cells drawn at the `n` presentation positions from `p` of the
    /// cursor's line deleted**, each where it is stored and the rest of the
    /// line closing up after it, from the rightmost, so each one's place in
    /// the presentation closes as it goes, the line closing up to `end`.
    pub(crate) fn delete_drawn(&mut self, m: &LineMap, p: Col, n: Count, end: ShiftEnd) {
        let to = end.col();
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
        self.edit_reaches(crate::aligned::EditRows::Chars);
        let p = m.presentation(self.cx.col());
        self.open_following_drawn(&m, p, n, self.fence_end());
        self.keep_presented(Some(p));
        self.row_changed_to(self.cy);
        self.wrote_rows(self.cy, self.cy);
        true
    }

    /// **`n` blanks opened at presentation position `p` of the cursor's
    /// line**, mapped by `m`, the cell drawn there pushed on in the
    /// presentation and the blanks joining its string (TR/53 note A.1 ii);
    /// with no map, at column `p`. The line keeps the cells it holds past
    /// the gap only where it has room for them: the callers, which make
    /// that room first, are PRECEDING's. The cells shift on to `end`. The
    /// column the blanks start at, past `end` where none fits.
    pub(crate) fn insert_drawn(
        &mut self,
        m: Option<&LineMap>,
        p: Col,
        n: Count,
        end: ShiftEnd,
    ) -> Col {
        let (c, at) = match m {
            Some(m) => self.gap_of(m, p),
            None => (p, p),
        };
        let Some(part) = end.from(at) else {
            return at;
        };
        let s = match m {
            Some(_) => self.string_of(self.view.get(self.cy, c).style),
            None => StringId::NONE,
        };
        self.open_gap_at(part, n, s);
        at
    }

    /// **`n` blanks opened under FOLLOWING at presentation position `p`**
    /// of the cursor's line, mapped by `m` (7.2.9: "the contents of the
    /// active presentation position and of the following character
    /// positions in the presentation component to be shifted in the
    /// direction of the character path"): the cells drawn at the last `n`
    /// positions before `end`, the ones the shift takes off the line,
    /// deleted first, and the gap opened where the cell drawn at `p` stood
    /// in the data, so the blanks are drawn at `p` and after it. A string
    /// that ends at the line's edge is shifted as one in its middle is. The
    /// column the blanks start at.
    ///
    /// **The gap opens at or before `end` wherever the caller chose `end`
    /// for the cursor**: the cell drawn at `p` is the cursor's, and the gap
    /// opens at its column or the one after it, so a shift ending at the
    /// right margin, chosen for a cursor at or inside it, reaches it, as
    /// one ending at the line's end does from anywhere. Until 2026-11-11 a
    /// character written under IRM right of the right margin shifted to
    /// the margin, before the gap: a debug build panicked, and a release
    /// build deleted a cell inside the margins and wrote over the cursor's.
    pub(crate) fn open_following_drawn(
        &mut self,
        m: &LineMap,
        p: Col,
        n: Count,
        end: ShiftEnd,
    ) -> Col {
        let room = end.from(p).map_or(0, |part| part.len().index());
        if room == 0 {
            return self.insert_drawn(Some(m), p, n, end);
        }
        let k = n.at_least_one().min(room);
        let (c, at) = self.gap_of(m, p);
        let s = self.string_of(self.view.get(self.cy, c).style);
        let to = end.col();
        let last = Col::at_index(to.index() - k);
        let gone = (last.index()..to.index().min(m.width))
            .filter(|&q| m.data(Col::at_index(q)) < at)
            .count();
        let at = Col::at_index(at.index() - gone);
        let Some(part) = end.from(at) else {
            return at;
        };
        self.delete_drawn(m, last, Count::of(k), end);
        self.open_gap_at(part, Count::of(k), s);
        at
    }

    /// **Where a gap at presentation position `p` opens in the data**: the
    /// cell drawn there, and the column before it where it runs with the
    /// line's path, after it where it runs against.
    fn gap_of(&self, m: &LineMap, p: Col) -> (Col, Col) {
        let base = match self.view.bidi(self.cy).path() {
            CharacterPath::RightToLeft => BidiDirection::Rtl,
            _ => BidiDirection::Ltr,
        };
        let c = m.data(p);
        match m.runs(c) == base {
            true => (c, c),
            false => (c, c.after()),
        }
    }

    /// **The columns PRECEDING shifts within for a cursor on column `c`**
    /// ([`PrecedingZone`]).
    pub(crate) fn preceding_zone(&self, c: Col) -> PrecedingZone {
        let fence = self.line_fence();
        match c > fence {
            true => PrecedingZone {
                lo: fence.after(),
                hi: Col::at_index(self.line_cols().index() - 1),
            },
            false => PrecedingZone {
                lo: self.left_limit(c),
                hi: fence,
            },
        }
    }

    /// **The part PRECEDING shifts from presentation position `p`** of the
    /// cursor's line, mapped by `m`, inside `zone` ([`DrawnPart`]). Where
    /// it holds fewer than `need` positions it runs on past `p`, away from
    /// the home, while the cells drawn there are in the zone, as a wide
    /// character at the shifted part's start takes the column after it on
    /// a line with no string (`crate::editing`); the caller writes nothing
    /// where it still holds fewer.
    pub(crate) fn preceding_drawn(
        &self,
        m: &LineMap,
        p: Col,
        zone: PrecedingZone,
        need: Count,
    ) -> DrawnPart {
        let inside = |q: usize| zone.holds(m.data(Col::at_index(q)));
        let mut home = p.index();
        while home > 0 && inside(home - 1) {
            home -= 1;
        }
        let mut cursor = p.index();
        while cursor + 1 - home < need.index() && inside(cursor + 1) {
            cursor += 1;
        }
        DrawnPart {
            home: Col::at_index(home),
            cursor: Col::at_index(cursor),
        }
    }

    /// **`n` blanks opened under PRECEDING at the cursor's end of `part`**
    /// (7.2.9: "the contents of the active presentation position and of
    /// the following character positions in the presentation component to
    /// be shifted in the direction opposite to that of the character
    /// path", read as its DATA paragraph's "preceding"; 8.3.64: "the
    /// contents of n character positions at the other end of the shifted
    /// part are removed"): the cells drawn at the part's first `n`
    /// positions deleted, and the gap opened beside the cell drawn at the
    /// part's cursor end, on its far side from the home, so the blanks are
    /// drawn there and at the `n - 1` positions before it. Both read `m`,
    /// the map before the edit, as [`Paint::open_following_drawn`] does.
    /// The blanks join the string of the cell they pushed (TR/53 note A.1).
    /// The column the blanks start at; `None` where the gap would open past
    /// `end`, which a part inside the zone `end` closes never does.
    ///
    /// **Until 2026-11-11 the deletion took the line's first positions**,
    /// whatever the margins: on a string drawn across the right margin they
    /// were cells right of it, which the shift's end kept, so nothing was
    /// deleted, the gap opened past the fence with nothing to shift, and
    /// the character was written over the cell right of the margin.
    pub(crate) fn open_preceding_drawn(
        &mut self,
        m: &LineMap,
        part: DrawnPart,
        n: Count,
        end: ShiftEnd,
    ) -> Option<Col> {
        let k = n.at_least_one().min(part.len().index());
        let (c, at) = match self.gap_of(m, part.cursor) {
            (c, at) if at == c => (c, c.after()),
            (c, _) => (c, c),
        };
        let s = self.string_of(self.view.get(self.cy, c).style);
        let home = part.home.index();
        let gone = (home..home + k)
            .filter(|&q| m.data(Col::at_index(q)) < at)
            .count();
        let at = Col::at_index(at.index() - gone);
        let gap = end.from(at)?;
        self.delete_drawn(m, part.home, Count::of(k), end);
        self.open_gap_at(gap, Count::of(k), s);
        Some(at)
    }

    /// `n` blanks opened at the start of `part` of the cursor's line, the
    /// cells in it shifted on to its end, the first `n` joining string `s`.
    fn open_gap_at(&mut self, part: Shifted, n: Count, s: StringId) {
        let Shifted { at, end: to } = part;
        let blank = self.blank();
        let k = n.at_least_one().min(part.len().index());
        self.view.insert_cells(self.cy, at, n, to, blank);
        let end = Col::at_index(at.index() + k);
        self.strings_kept(self.cy, (at, end), blank, |_| s);
        self.erase_images(self.cy, at, to);
        self.cells = self.cells.plus(part.len().index());
    }

    /// **Where `ICH`, `DCH` and PRECEDING's shifts end**: one past the
    /// fence, the side margins bounding them on every row, as
    /// [`Paint::char_edit_end`] ignores them outside the margins.
    pub(crate) fn fence_end(&self) -> ShiftEnd {
        ShiftEnd(self.line_fence().after())
    }

    /// **Where a character written under IRM and FOLLOWING shifts the
    /// line to**: one past the fence for a cursor at or inside it, and the
    /// line's end for one a move put right of it. `[[source: dec-std-070]]`'s
    /// insert-or-replace takes the right margin as the shift's boundary
    /// only for an Active Position at or left of it and `end_of_line(line)`
    /// otherwise, and "Data shifted past the Right Margin within the Scroll
    /// Area, or the last column of the Active Line outside the Scroll Area
    /// is lost" (pp. 5-139, 5-140); the write
    /// itself runs on to the line's last column there ([`Paint::overflow`]),
    /// and under DATA its gap opens to the line's end ([`Paint::open_gap`]).
    /// A row outside the scroll area keeps the right margin for a cursor
    /// inside it, as the write does: open, the `CSI s` row.
    pub(crate) fn insert_end(&self) -> ShiftEnd {
        match self.cx.raw() > self.line_fence() {
            true => ShiftEnd(Col::at_index(self.line_cols().index())),
            false => self.fence_end(),
        }
    }

    /// **The cells drawn at presentation positions `[from, to)` of the
    /// cursor's line erased**, each where it stands.
    fn erase_presented_span(&mut self, m: &LineMap, span: (Col, Col)) {
        let blank = self.blank();
        self.erase_drawn_span(self.cy, m, span, blank);
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
