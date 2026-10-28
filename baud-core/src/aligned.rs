//! **TAC, TALE, TATE and TCC**, `CSI Pn SP 06/02`, `06/01`, `06/00` and
//! `CSI Pn1 ; Pn2 SP 06/03` (`[[source: ecma48]]` 8.3.151 to 8.3.153,
//! 8.3.155): a character tabulation stop "calling for" centring, leading
//! edge or trailing edge alignment, or the centring of a target character,
//! each replacing "any tabulation stop previously set at that character
//! position" and leaving the others.
//!
//! **The string is laid out again as each character arrives** (Jev's lean,
//! RELAYOUT, 0.45/0.73 then 0.77/0.82): a terminal receives it a byte at a
//! time, so after an `HT` lands on such a stop each graphic character is
//! put where the stop says, the string written so far moved back to make
//! room. The stop is the left edge of its cell, where TATE puts the first
//! character's trailing edge, so TATE's string starts in the stop's cell,
//! as an ordinary stop's; TALE's ends in the cell before it, the last
//! character's leading edge at the stop (Jev's lean, BEFORE_STOP,
//! 0.44/0.66 then 0.66/0.78); TAC's is centred on the same edge, an odd
//! length's extra column after it (Jev's lean, ON_BOUNDARY, 0.72/0.60);
//! TCC's starts in the stop's cell until the target arrives, which goes in
//! the stop's cell, the text before it moved back so it ends in the cell
//! before, and what follows goes on after it (Jev's lean, TARGET_IN_CELL,
//! 0.88/0.92).
//!
//! **A format effector or a cursor movement ends the string**; SGR and the
//! other controls that move nothing do not, so a number coloured with SGR
//! stays one string (Jev, FORMAT_EFFECTOR, 0.92/0.92): `CR`, `LF`, `VT`,
//! `FF`, `IND`, `NEL` and `RI` end it before they move anything, so a line
//! scrolled in under the cursor takes none, and the other format effectors
//! and the cursor movements end it as they execute
//! ([`Paint::effector_ends_string`]). **An erase or edit of its line ends
//! it too**, the decided rule's "and an erase or edit of the line", before
//! it acts ([`EditRows`]; Jev's lean, LINE_REACHED, 0.83/0.94 on a second
//! ask), and so does a resize that moves the cursor (Jev's lean,
//! CURSOR_MOVED; `Term::resize_ends_string`). A string that would move
//! back past the line's start, the left margin under DECLRMM, or on past
//! the line's end goes on as plain text (Jev's lean, AS_TEXT, 0.16/0.26
//! then 0.94/0.88).
//!
//! `HTS` and `CTC 0` set an ordinary stop where an aligned one was; `TBC`,
//! `TSR` and `CTC` clear one as any stop; DECST8C, `RIS` and a restored
//! report's stops are ordinary. DECTABSR reports an aligned stop as a stop.
//! With BDSM reset, on a line holding a string, the stop is found at the
//! cursor's presentation position, where `HT` counted it, and the string
//! moves by the positions as drawn, each cell keeping the string it is
//! stored in (Jev's lean, BUILD_DRAWN, 0.66/0.66 then 0.69/0.76).
//!
//! **What it costs while no stop calls for alignment: nothing on the text
//! path.** An `HT` maps through [`ModeBit::ReturnMapped`] once an aligned
//! stop has been set, and the string's characters go a character at a
//! time, the one-by-one graph [`ModeBit::Aligning`] chooses, laid out by
//! SAPV's writer's step out of line.

use baud_domain::{
    Align, Codepoint, Col, Count, CursorCol, EditExtent, EraseExtent, ModeBit, OneBased, Row,
    TargetChar,
};

use crate::MAX_COLS;
use crate::component::Drawn;
use crate::paint::Paint;

/// What a column's stop calls for, where the column has one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum StopKind {
    /// An ordinary stop, `HTS`'s, or TATE's, whose string an ordinary stop
    /// lays out alike.
    Plain,
    /// TAC, TALE or TCC.
    Aligned(Align),
}

/// **The string being laid out against an aligned stop**.
#[derive(Clone, Copy)]
pub(crate) struct Open {
    row: Row,
    /// The stop's column.
    stop: Col,
    align: Align,
    /// Where the string starts, and the columns it covers.
    start: Col,
    len: Count,
    /// Where the string left the cursor.
    at: Col,
}

/// **The lines an erase or edit reaches**, read only while a string is
/// open ([`Paint::edit_reaches`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum EditRows {
    /// The cursor's line: EL, ECH, EF and DECSEL.
    Line,
    /// `ICH`'s and `DCH`'s: the cursor's line, or under SEE's page or
    /// qualified area the rows the shift runs through within DEC's margins,
    /// from the top margin to the cursor under HEM's PRECEDING and from the
    /// cursor to the bottom margin under FOLLOWING (`crate::extent`).
    Chars,
    /// `IL`'s and `DL`'s: the cursor's line through the bottom margin.
    Lines,
    /// ED's and DECSED's: below the cursor and its line, above it and its
    /// line, or the page.
    Display(EraseExtent),
    /// The scrolling region: SU, SD, SL, SR, DECIC and DECDC.
    Region,
    /// Rows from the first through the second: EA's run and a rectangle.
    Span(Row, Row),
}

/// What each column's stop calls for, and the string being laid out.
pub(crate) struct Aligned {
    kinds: [StopKind; MAX_COLS],
    any: AnyAligned,
    open: Option<Open>,
    /// The last character a code's set was sent to write, and its code
    /// table position, for TCC's target ([`Paint::received`]).
    received: Option<(Codepoint, u8)>,
}

/// Whether a stop has called for alignment since the stops were last all
/// ordinary.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum AnyAligned {
    No,
    Yes,
}

impl Aligned {
    pub(crate) const POWER_ON: Aligned = Aligned {
        kinds: [StopKind::Plain; MAX_COLS],
        any: AnyAligned::No,
        open: None,
        received: None,
    };

    /// Every stop ordinary: DECST8C, `RIS`, `TBC` of them all and a
    /// restored report's stops.
    pub(crate) fn all_plain(&mut self) {
        if self.any == AnyAligned::Yes {
            self.kinds = [StopKind::Plain; MAX_COLS];
        }
        self.any = AnyAligned::No;
        self.open = None;
    }

    /// The stop at column `c` ordinary, or gone: `HTS`, `CTC 0`, `TBC 0`
    /// and `TSR`.
    pub(crate) fn plain_at(&mut self, c: Col) {
        if let Some(k) = self.kinds.get_mut(c.index()) {
            *k = StopKind::Plain;
        }
    }

    /// Column `c`'s kind.
    fn kind_at(&self, c: Col) -> StopKind {
        self.kinds
            .get(c.index())
            .copied()
            .unwrap_or(StopKind::Plain)
    }

    /// Column `c` calling for `align`.
    fn set(&mut self, c: Col, align: Align) {
        if let Some(k) = self.kinds.get_mut(c.index()) {
            *k = StopKind::Aligned(align);
            self.any = AnyAligned::Yes;
        }
    }

    /// Whether `cp` is TCC's target `t`: by the code table position it was
    /// received at, the one [`Paint::received`] noted for it, else by its
    /// scalar value, which UTF-8 and ISO 8859-1 decode a character to.
    fn is_target(&self, cp: Codepoint, t: TargetChar) -> bool {
        let at = match self.received {
            Some((c, b)) if c == cp => u32::from(b),
            _ => cp.value(),
        };
        at == t.value()
    }

    /// Whether `HT` must look at where it lands.
    pub(crate) fn any(&self) -> bool {
        self.any == AnyAligned::Yes
    }
}

impl Paint<'_, '_> {
    /// **Character `cp` received at code table position `code`**, or
    /// decoded (`None`), from the entries that write while a string is
    /// open: a GL code by a form's graph, a GR code, a single shift's and
    /// `DEL`'s. TCC's target is "the code table position (binary value) of
    /// the target character in the currently invoked code" (8.3.155),
    /// which a set's replacement changes: the UK set's `#` is drawn `£` at
    /// position 35, DEC Special Graphics' `q` drawn `─` at 113. Noted only
    /// while a string is open, and forgotten when one opens or ends.
    #[inline(always)]
    pub(crate) fn received(&mut self, cp: Codepoint, code: Option<u8>) {
        if self.cold.modes.is_set(ModeBit::Aligning) {
            self.strs.aligned.received = code.map(|b| (cp, b));
        }
    }

    /// **A format effector ends the string** (Jev, FORMAT_EFFECTOR):
    /// `CR`, `LF`, `VT`, `FF`, `IND`, `NEL` and `RI`, before they move
    /// anything, so a line scrolled in under the cursor, or one the cursor
    /// stays on, takes no string; and, as each executes, the other format
    /// effectors of 8.2.4 (`HPA`, `HPB`, `HPR`, `HVP`, `HTS`, `TBC`, `TSR`,
    /// `VPA`, `VPB`, `VPR`, `VTS`, PPA, PPB and PPR) and the cursor
    /// movements (`CUU`, `CUD`, `CUF`, `CUB`, `CUP`, `CNL`, `CPL`, `CHA`,
    /// `CHT`, `CBT` and `CVT`, routed by [`ModeBit::CursorRouted`] with the
    /// format effectors among the control sequences, and NP, PP, `DECRC`,
    /// `?1048`'s restore, `CSI u`, DECFI, VT52's moves and erases, and
    /// the controls that home the cursor, DECSTBM, DECSLRM, DECOM, DECCOLM
    /// and DECALN), so a move away and back, or one that leaves the cursor
    /// where the string left it, ends it; and DECSASD and a screen switch,
    /// which take the active position to another display or page buffer
    /// (Jev's lean, SWITCH_ENDS, 0.42/0.26 on a second ask).
    ///
    /// **A control that cannot leave the cursor there needs no test of its
    /// own**: `BS` and DECBI move it off the string's end, and HTJ's tab
    /// lands as `HT`'s does, so the next character is not written where
    /// the string left the cursor and the string ends then, and no control
    /// can bring the cursor back to the string's cell without ending it.
    /// `BS` at the left margin moves nothing, and a string can be open
    /// there only while it is empty, which lays out its next character as
    /// plain text does.
    #[inline(always)]
    pub(crate) fn effector_ends_string(&mut self) {
        if self.cold.modes.is_set(ModeBit::Aligning) {
            self.aligned_ends();
        }
    }

    /// **An erase or edit reaching `rows` ends a string open on one of
    /// them**, before it acts (Jev, FORMAT_EFFECTOR, 0.92/0.92: "an erase or
    /// edit of the line"; Jev's lean, LINE_REACHED, 0.83/0.94 on a second
    /// ask): one that moves or erases the string's cells under it would
    /// leave the layout moving what is no longer the string. One that
    /// reaches only other lines, and an attribute change, leave it open,
    /// and so does one the margins or the region make a no-op: `IL`, `DL`,
    /// `ICH`, `DCH`, `DECIC` and `DECDC` ask past the guards that ignore
    /// them.
    #[inline(always)]
    pub(crate) fn edit_reaches(&mut self, rows: EditRows) {
        if self.cold.modes.is_set(ModeBit::Aligning) {
            self.aligned_edited(rows);
        }
    }

    #[cold]
    #[inline(never)]
    fn aligned_edited(&mut self, rows: EditRows) {
        let Some(o) = self.strs.aligned.open else {
            return;
        };
        let last = Row::from_index(self.rows.index() - 1);
        let (top, bottom) = match rows {
            EditRows::Line => (self.cy, self.cy),
            EditRows::Chars => match (self.cold.extent, self.cold.modes) {
                (None | Some(EditExtent::Line | EditExtent::Field), _) => (self.cy, self.cy),
                (_, m) if m.is_set(ModeBit::EditPreceding) => (self.cold.top, self.cy),
                _ => (self.cy, self.cold.bot),
            },
            EditRows::Lines => (self.cy, self.cold.bot.max(self.cy)),
            EditRows::Display(EraseExtent::ToEnd) => (self.cy, last),
            EditRows::Display(EraseExtent::ToStart) => (Row::ZERO, self.cy),
            EditRows::Display(EraseExtent::All) => (Row::ZERO, last),
            EditRows::Region => (self.cold.top, self.cold.bot),
            EditRows::Span(a, b) => (a, b),
        };
        if o.row >= top && o.row <= bottom {
            self.aligned_ends();
        }
    }

    /// **TAC, TALE, TATE or TCC at position `n`**: the stop set, calling
    /// for `align`; `Pn` has no default, so an omitted or zero one is
    /// counted. A position past the line holds no stop.
    pub(crate) fn set_aligned_stop(&mut self, align: Align, n: Option<OneBased>) {
        let Some(n) = n else {
            self.unsupported_param += 1;
            return;
        };
        let Some(c) = n.col_within(self.cols) else {
            return;
        };
        if align == Align::TrailingEdge {
            self.tabs[c.index()] = true;
            return self.strs.aligned.plain_at(c);
        }
        self.strs.aligned.set(c, align);
        self.tabs[c.index()] = true;
        self.refresh_presentation_edits();
    }

    /// **After a tab moved the cursor**: on a stop calling for alignment a
    /// string opens there, and its characters go a character at a time.
    #[cold]
    #[inline(never)]
    pub(crate) fn tab_landed(&mut self) {
        if !self.strs.aligned.any() {
            return;
        }
        // The stop at the cursor's presentation position, where `HT`
        // counted it (`crate::component`).
        let c = self.presented_cursor();
        let kind = match self.tabs.get(c.index()) {
            Some(&true) => self.strs.aligned.kind_at(c),
            _ => StopKind::Plain,
        };
        self.strs.aligned.open = match kind {
            StopKind::Aligned(align) => Some(Open {
                row: self.cy,
                stop: c,
                align,
                start: c,
                len: Count::of(0),
                at: c,
            }),
            StopKind::Plain => None,
        };
        self.strs.aligned.received = None;
        self.cold
            .modes
            .apply(ModeBit::Aligning, self.strs.aligned.open.is_some());
        self.refresh_routed();
        self.reselect();
    }

    /// **[`ModeBit::CursorRouted`] set as its two bits say**, wherever
    /// either changes: the cursor functions go out of line while a string
    /// is open, and end it ([`Paint::cursor_presented`]).
    pub(crate) fn refresh_routed(&mut self) {
        let on = self.cold.modes.is_set(ModeBit::CursorPresented)
            || self.cold.modes.is_set(ModeBit::Aligning);
        self.cold.modes.apply(ModeBit::CursorRouted, on);
    }

    /// The string ends: what follows is plain text.
    #[cold]
    #[inline(never)]
    fn aligned_ends(&mut self) {
        self.strs.aligned.open = None;
        self.strs.aligned.received = None;
        self.cold.modes.apply(ModeBit::Aligning, false);
        self.refresh_routed();
        self.reselect();
    }

    /// **Before a character of `need` columns is written while a string is
    /// open**: the string moved back and the cursor put where the character
    /// goes, or the string ended where the cursor is not where it left it.
    pub(crate) fn align_step(&mut self, cp: Codepoint, need: Count) {
        let Some(o) = self.strs.aligned.open else {
            return self.aligned_ends();
        };
        if self.cy != o.row || self.presented_cursor() != o.at {
            return self.aligned_ends();
        }
        let need = need.index();
        let floor = self.cold.left.index();
        let end = self.line_fence().index() + 1;
        let (s, start, len) = (o.stop.index(), o.start.index(), o.len.index());
        let placed = match o.align {
            Align::LeadingEdge => (s >= floor + len + need).then(|| (s - len - need, len)),
            Align::Centred => {
                let n = len + need;
                (s >= floor + n / 2 && s - n / 2 + n <= end).then(|| (s - n / 2, len))
            }
            Align::OnCharacter(t) if self.strs.aligned.is_target(cp, t) => {
                let back = start + len - s;
                match start >= floor + back {
                    true => {
                        self.move_back(o.row, o.start, o.len, Count::of(back));
                        self.cursor_to_position(o.stop);
                        return self.aligned_ends();
                    }
                    false => None,
                }
            }
            Align::OnCharacter(_) | Align::TrailingEdge => Some((start, len)),
        };
        let Some((to, len)) = placed else {
            return self.aligned_ends();
        };
        if to < start {
            self.move_back(o.row, o.start, o.len, Count::of(start - to));
        }
        let at = to + len;
        self.cursor_to_position(Col::at_index(at));
        self.strs.aligned.open = Some(Open {
            start: Col::at_index(to),
            len: Count::of(len + need),
            at: Col::at_index(at + need),
            ..o
        });
    }

    /// The cursor's position along its line: the one drawn, with BDSM
    /// reset on a line holding a string, where `HT` counts its stops.
    fn presented_cursor(&self) -> Col {
        let mut d = Drawn::presented(self.cold.modes);
        self.drawn_from(&mut d, self.cy, self.cx.col())
    }

    /// The cursor on the cell drawn at position `p` of its line.
    fn cursor_to_position(&mut self, p: Col) {
        let mut d = Drawn::presented(self.cold.modes);
        let c = self.drawn_at(&mut d, self.cy, p);
        self.set_col(CursorCol::moved(c));
    }

    /// The `len` positions from `start` of row `r` moved back `by`, the
    /// positions they leave erased; on a line drawn as edited the content
    /// moves between the cells drawn there (`crate::extent`).
    fn move_back(&mut self, r: Row, start: Col, len: Count, by: Count) {
        if by.index() == 0 {
            return;
        }
        let to = Col::at_index(start.index() - by.index());
        let end = Col::at_index(start.index() + len.index());
        let mut d = Drawn::presented(self.cold.modes);
        if d.is_drawn() && self.line_map(r).is_some() {
            let run = crate::extent::Run {
                from: (r, to),
                to: (r, end.before()),
                left: to,
                right: end.before(),
            };
            let del = crate::editing::CharEdit::Delete;
            return self.shift_run(&run, by, del, crate::extent::Toward::End, &mut d);
        }
        let b = self.blank();
        self.view.delete_cells(r, to, by, end, b);
        self.erase_images(r, to, end);
        self.cells = self.cells.plus(end.index() - to.index());
        self.row_changed_to(r);
        self.wrote_rows(r, r);
    }
}

impl crate::term::Term<'_> {
    /// **A resize that moves the cursor ends an open string** (Jev's lean,
    /// CURSOR_MOVED, split first, then 0.79/0.89): one that leaves it on
    /// another row or column, by the width's clamp, rows retired above it
    /// or its line re-cut, is a cursor movement under FORMAT_EFFECTOR, as
    /// DEC's page-size controls move the active position only when it no
    /// longer fits (`[[source: dec-std-070]]` DECSCPP p. 5-73, DECSLPP p.
    /// 5-75), and DECSCPP's request is executed by the embedder's resize.
    /// One that leaves the cursor where it was keeps the string's cells
    /// where they were, since a re-cut keeps a line's order and the string
    /// ends at the cursor on its row, so the string goes on.
    pub(crate) fn resize_ends_string(&mut self, before: (Row, CursorCol)) {
        if self.cold.modes.is_set(ModeBit::Aligning) && (self.cy, self.cx) != before {
            self.store.strs.aligned.open = None;
            self.store.strs.aligned.received = None;
            self.cold.modes.apply(ModeBit::Aligning, false);
            let routed = self.cold.modes.is_set(ModeBit::CursorPresented);
            self.cold.modes.apply(ModeBit::CursorRouted, routed);
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use baud_domain::ModeBit;

    use crate::{BytePos, Owned, ReadLen, Term, WinSize};

    const WS: WinSize = WinSize {
        ws_row: 4,
        ws_col: 20,
    };

    /// Whether a string is open after `bytes`: the string, and the two
    /// bits it sets.
    fn open_after(bytes: &[u8]) -> [bool; 3] {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
        [
            t.store.strs.aligned.open.is_some(),
            t.cold.modes.is_set(ModeBit::Aligning),
            t.cold.modes.is_set(ModeBit::CursorRouted),
        ]
    }

    /// **A control that homes the cursor ends the string as it executes**
    /// (Jev, FORMAT_EFFECTOR, 0.92/0.92: "or other cursor movement"):
    /// DECSTBM, DECSLRM, DECOM set and reset, DECCOLM, DECALN, SPD and SCP
    /// with `Ps2` 1, DECRSPS's cursor restore and RIS, from the string's line,
    /// row 4, where none of them leaves the cursor. No
    /// character written after one can show it, since the home is never
    /// where a string can go on being laid out, so the string's state is
    /// read here. **SGR, and a DECSTBM that is ignored and moves nothing,
    /// keep it**, the controls.
    #[test]
    fn a_control_that_homes_the_cursor_ends_the_string() {
        let string = "\x1b[3g\x1b[9 a\x1b[4;1H\tab";
        for keep in ["", "\x1b[31m", "\x1b[3;2r"] {
            let bytes = [string, keep].concat();
            assert_eq!(open_after(bytes.as_bytes()), [true; 3], "{keep:?}");
        }
        let open: usize = [
            "\x1b[r",
            "\x1b[2;4r",
            "\x1b[?69h\x1b[3;12s",
            "\x1b[?6h",
            "\x1b[?6l",
            "\x1b[?40h\x1b[?3h",
            "\x1b#8",
            "\x1b[0;1 S",
            "\x1b[1;1 k",
            "\x1bP1$t1;1;1;@;@;@;0;2;@;BBBB\x1b\\",
            "\x1bc",
        ]
        .iter()
        .filter(|home| open_after([string, home].concat().as_bytes()) != [false; 3])
        .inspect(|home| std::eprintln!("open after {home:?}"))
        .count();
        assert_eq!(open, 0, "of 11");
    }

    /// Whether a string is open after `bytes` and a resize to `to`, on
    /// storage with room for 8 by 40.
    fn open_after_resize(bytes: &[u8], to: WinSize) -> [bool; 3] {
        let big = WinSize {
            ws_row: 8,
            ws_col: 40,
        };
        let mut owned = Owned::new(&big);
        let mut t = Term::new(owned.storage(), &WS);
        t.write_at(BytePos::ZERO, bytes);
        t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
        assert_eq!(t.resize(&to), crate::Resized::Ok);
        [
            t.store.strs.aligned.open.is_some(),
            t.cold.modes.is_set(ModeBit::Aligning),
            t.cold.modes.is_set(ModeBit::CursorRouted),
        ]
    }

    /// **A resize that moves the cursor ends the string** (Jev's lean,
    /// CURSOR_MOVED): the string on TALE's stop at 9, row 1 or row 4. A
    /// narrowing to 6 columns re-cuts the line and to 8 clamps the cursor,
    /// and a shortening to 2 rows retires the rows above the string's row
    /// 4, each leaving the cursor elsewhere. No character written after
    /// one shows it, since the next character is not where the string left
    /// the cursor, so the string's state is read here. **A resize that
    /// leaves the cursor where it was keeps it**, the controls: to the same
    /// geometry, to 30 columns, and to 3 rows cut below the cursor on row 1.
    #[test]
    fn a_resize_that_moves_the_cursor_ends_the_string() {
        let row1 = "\x1b[3g\x1b[9 a\tab";
        let row4 = "\x1b[3g\x1b[9 a\x1b[4;1H\tab";
        let to = |ws_row, ws_col| WinSize { ws_row, ws_col };
        for (s, rows, cols) in [(row1, 4, 20), (row1, 4, 30), (row1, 3, 20), (row4, 4, 20)] {
            let got = open_after_resize(s.as_bytes(), to(rows, cols));
            assert_eq!(got, [true; 3], "{s:?} to {rows} by {cols}");
        }
        for (s, rows, cols) in [(row1, 4, 6), (row1, 4, 8), (row4, 2, 20), (row4, 4, 6)] {
            let got = open_after_resize(s.as_bytes(), to(rows, cols));
            assert_eq!(got, [false; 3], "{s:?} to {rows} by {cols}");
        }
    }
}
