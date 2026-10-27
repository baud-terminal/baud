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
//! `FF`, `IND`, `NEL` and `RI` end it themselves, so a line scrolled in
//! under the cursor takes none, and after any other control it ends at the
//! first character written anywhere but where it left the cursor. A
//! string that would move back past the line's start, the left margin
//! under DECLRMM, or on past the line's end goes on as plain text (Jev's
//! lean, AS_TEXT, 0.16/0.26 then 0.94/0.88).
//!
//! `HTS` and `CTC 0` set an ordinary stop where an aligned one was; `TBC`,
//! `TSR` and `CTC` clear one as any stop; DECST8C, `RIS` and a restored
//! report's stops are ordinary. DECTABSR reports an aligned stop as a stop.
//! With BDSM reset, on a line holding a string, the cells are moved, not
//! the positions as drawn.
//!
//! **What it costs while no stop calls for alignment: nothing on the text
//! path.** An `HT` maps through [`ModeBit::ReturnMapped`] once an aligned
//! stop has been set, and the string's characters go a character at a
//! time, the one-by-one graph [`ModeBit::Aligning`] chooses, laid out by
//! SAPV's writer's step out of line.

use baud_domain::{Align, Codepoint, Col, Count, CursorCol, ModeBit, OneBased, Row, TargetChar};

use crate::MAX_COLS;
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
    /// stays on, takes no string.
    #[inline(always)]
    pub(crate) fn effector_ends_string(&mut self) {
        if self.cold.modes.is_set(ModeBit::Aligning) {
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
        let c = self.cx.col();
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
        self.reselect();
    }

    /// The string ends: what follows is plain text.
    #[cold]
    #[inline(never)]
    fn aligned_ends(&mut self) {
        self.strs.aligned.open = None;
        self.strs.aligned.received = None;
        self.cold.modes.apply(ModeBit::Aligning, false);
        self.reselect();
    }

    /// **Before a character of `need` columns is written while a string is
    /// open**: the string moved back and the cursor put where the character
    /// goes, or the string ended where the cursor is not where it left it.
    pub(crate) fn align_step(&mut self, cp: Codepoint, need: Count) {
        let Some(o) = self.strs.aligned.open else {
            return self.aligned_ends();
        };
        if self.cy != o.row || self.cx.col() != o.at {
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
                        self.set_col(CursorCol::moved(o.stop));
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
        self.set_col(CursorCol::moved(Col::at_index(at)));
        self.strs.aligned.open = Some(Open {
            start: Col::at_index(to),
            len: Count::of(len + need),
            at: Col::at_index(at + need),
            ..o
        });
    }

    /// The `len` columns from `start` of row `r` moved back `by`, the cells
    /// they leave erased.
    fn move_back(&mut self, r: Row, start: Col, len: Count, by: Count) {
        if by.index() == 0 {
            return;
        }
        let to = Col::at_index(start.index() - by.index());
        let end = Col::at_index(start.index() + len.index());
        let b = self.blank();
        self.view.delete_cells(r, to, by, end, b);
        self.erase_images(r, to, end);
        self.cells = self.cells.plus(end.index() - to.index());
        self.row_changed_to(r);
        self.wrote_rows(r, r);
    }
}
