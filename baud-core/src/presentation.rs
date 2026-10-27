//! DECRQPSR's two reports, DECCIR and DECTABSR, and DECRSPS, which restores
//! either.
//!
//! `[[source: dec-std-070]]` pp. 5-198 to 5-205; `[[source: vt510-decrqpsr]]`,
//! `[[source: vt510-deccir]]`, `[[source: vt510-dectabsr]]`, `[[source:
//! vt510-decrsps]]`. Answered whatever level DA claims, as DECRQSS and the
//! rectangle operations are (Jev's lean UNGATED, 0.52/0.56 then 0.64/0.62,
//! with the owner's standing rule quoted); at the levels DECSCL selects,
//! from Level 2, "Levels: 2x (TSI), 3-4" (`baud_domain::LEVELS`).

use crate::osc::StrKind;
use crate::paint::Paint;
use crate::write::Csi;
use baud_domain::*;

impl Paint<'_, '_> {
    /// DECRQPSR, `CSI Ps $ w`: "0 ignored, no report sent", 1 DECCIR, 2
    /// DECTABSR (p. 5-198); any other `Ps` is ignored as 0 is.
    #[cold]
    #[inline(never)]
    pub(crate) fn request_presentation_state(&mut self, csi: &Csi) {
        match PresentationState::from_param(csi.params().raw(ParamIdx::FIRST, 0)) {
            Some(PresentationState::CursorInformation) => self.report_cursor_information(),
            Some(PresentationState::TabStops) => self.report_tab_stops(),
            None => {}
        }
    }

    /// The cursor state DECCIR reports: the current one, of the kinds DECSC
    /// saves (`[[source: vt510-deccir]]`, "DECCIR reports the status of the
    /// cursor position, including visual attributes and character protection
    /// attributes"). The position is the screen's, from 1, and a pending
    /// wrap reports the column it is pending on with Sflag's bit 4, as CPR
    /// reports it: the line's last, the right margin under DECSLRM, the
    /// end of a double-width line ([`CursorCol::reported`]) and SLL's line
    /// limit (`crate::bounds`). Srend reports
    /// any underline style as underline and either blink as blink, the one
    /// bit each DECCIR has. The single-shift bits are 0: the `CSI` that asks
    /// has cancelled any shift (070-3 p. 3-81).
    pub(crate) fn cursor_information(&self) -> CursorInformation {
        let a = self.style_raw.attrs();
        let on = |b: bool| ModeChange::of(b);
        let rendition = CirBits::NONE
            .with(CirBits::BIT1, on(a.has(Attrs::BOLD)))
            .with(CirBits::BIT2, on(a.bits() & Attrs::UNDERLINES.bits() != 0))
            .with(CirBits::BIT3, on(a.bits() & Attrs::BLINKS.bits() != 0))
            .with(CirBits::BIT4, on(a.has(Attrs::REVERSE)));
        let fence = self.report_fence();
        let pending = !self.cx.is_placed() && self.cx.col().index() == fence.index() + 1;
        let flags = CirBits::NONE
            .with(CirBits::BIT1, on(self.cold.modes.is_set(ModeBit::Origin)))
            .with(CirBits::BIT4, on(pending));
        let col = self.cx.reported(fence, self.cols);
        CursorInformation {
            row: self.cy.one_based(),
            col: col.one_based(),
            rendition,
            attribute: CirBits::NONE.with(CirBits::BIT1, on(self.cold.protect.is_protected())),
            flags,
            gl: self.cold.gl,
            gr: self.cold.gr,
            designations: self.cold.desig,
        }
    }

    /// DECCIR, `DCS 1 $ u D...D ST`, in the controls `S7C1T`/`S8C1T` chose.
    fn report_cursor_information(&mut self) {
        let mut data = [0u8; CIR_DATA_MAX];
        let n = self.cursor_information().encode(&mut data);
        let c1 = self.cold.replies.c1();
        self.push_reply(&[c1.dcs(), b"1$u", &data[..n], c1.st()]);
    }

    /// DECTABSR ([`encode_tab_report`]), in the controls `S7C1T`/`S8C1T`
    /// chose, whole or not at all: the reply buffer holds the longest one, a
    /// stop at every column of the widest screen (`storage::REPLY_MOST`).
    fn report_tab_stops(&mut self) {
        let cols = self.cols.index().min(self.tabs.len());
        let at = self.reply_len.index();
        match encode_tab_report(
            &self.tabs[..cols],
            self.cold.replies.c1(),
            &mut self.reply[at..],
        ) {
            Some(n) => self.reply_len = ByteLen::from_index(at + n.index()),
            None => self.reply_overflows += 1,
        }
    }

    /// Append `parts` to the reply buffer whole, or count an overflow: a
    /// truncated reply is a wrong answer (`Paint::want_reply`).
    pub(crate) fn push_reply(&mut self, parts: &[&[u8]]) {
        let len: usize = parts.iter().map(|p| p.len()).sum();
        let at = self.reply_len.index();
        if at + len > self.reply.len() {
            self.reply_overflows += 1;
            return;
        }
        let mut w = at;
        for p in parts {
            self.reply[w..w + p.len()].copy_from_slice(p);
            w += p.len();
        }
        self.reply_len = ByteLen::from_index(w);
    }

    /// DECRSPS, `DCS Ps $ t`: which report's format the body is in, as the
    /// string kind its bytes go to. "0 error, restore ignored" (p. 5-205),
    /// and so is a `Ps` neither text defines: the body is consumed and
    /// dropped. **A tab-stop restore takes effect at its terminator**, as
    /// every other control string here does: each column is kept as a bit
    /// as it arrives ([`TAB_BITS`] bytes of the string accumulator, which a
    /// tab-stop body does not otherwise use), so a body of any length
    /// restores whole, and one a `CAN`, an `ESC` or a C1 cancels changes
    /// nothing.
    #[cold]
    #[inline(never)]
    pub(crate) fn restore_start(&mut self, csi: &Csi) -> StrKind {
        match PresentationState::from_param(csi.params().raw(ParamIdx::FIRST, 0)) {
            Some(PresentationState::CursorInformation) => StrKind::RestoreCursor,
            Some(PresentationState::TabStops) => {
                self.strs.pending[..TAB_BITS].fill(0);
                self.cold.tab_restore = TabRestore::START;
                StrKind::RestoreTabs
            }
            None => StrKind::Other,
        }
    }

    /// One byte of a tab-stop restore's body ([`TabRestore`]); a column
    /// outside the screen is an invalid value too (Jev's lean STOP: SKIP at
    /// confidence 0.28 and STOP at 0.06, then STOP at 0.36 and 0.34 after
    /// research). Every C0 and a `DEL` are dropped first, and a GR byte read
    /// as its 7-bit byte, as for DECRQSS (`Paint::status_data_put`).
    pub(crate) fn restore_tab_put(&mut self, b: StreamByte) {
        self.cold.tab_restore = match self.cold.tab_restore.put(b) {
            TabRestore::Ended(c) => self.restore_tab_stop(c),
            next => next,
        };
    }

    /// The end of a tab-stop restore: its last column, then the stops
    /// replaced by the columns the body listed. "If there is an invalid
    /// value in the DECRSPS sequence, then the terminal ignores the rest of
    /// the sequence. This action may leave the terminal in a partially
    /// restored state" (`[[source: vt510-decrsps]]`): the columns before an
    /// invalid value are restored.
    pub(crate) fn restore_tab_end(&mut self) {
        if let Some(c) = self.cold.tab_restore.last() {
            self.restore_tab_stop(c);
        }
        self.cold.tab_restore = TabRestore::Stopped;
        let cols = self.cols.index().min(self.tabs.len());
        for (i, t) in self.tabs[..cols].iter_mut().enumerate() {
            *t = self.strs.pending[i / 8] & (1 << (i % 8)) != 0;
        }
        self.strs.aligned.all_plain();
    }

    /// Keep column `c` as a stop to restore, or stop the restore there.
    fn restore_tab_stop(&mut self, c: OneBased) -> TabRestore {
        let cols = Cols::of_index(self.cols.index().min(self.tabs.len()));
        match c.col_within(cols) {
            Some(col) => {
                let i = col.index();
                self.strs.pending[i / 8] |= 1 << (i % 8);
                TabRestore::START
            }
            None => TabRestore::Stopped,
        }
    }

    /// The end of a cursor-information restore: each field up to the first
    /// invalid one ([`CirRestore`]), in the report's order.
    ///
    /// The position is the screen's, as DECCIR reports it, confined to the
    /// margins when the restored origin mode is set, as DECRC confines it
    /// (`Paint::restore`). Srend sets or clears the four renditions it
    /// carries and keeps the rest of the rendition (Jev's lean FOUR_ONLY,
    /// 0.80/0.70 then 0.84/0.86); an underline or blink already on stays the
    /// style it is. A pending wrap is restored where the column is the one a
    /// wrap is pending on. A single-shift bit leaves the shift pending after
    /// the string (Jev's lean RESTORE_PENDING, four asks averaging 0.63).
    pub(crate) fn restore_cursor_information(&mut self, data: &[u8]) {
        let r = CirRestore::parse(data, self.rows, self.cols);
        if let (Some(row), Some(col)) = (r.row, r.col) {
            self.goto(row.to_row(self.rows), col.to_col(self.cols));
        }
        if let Some(rend) = r.rendition {
            let mut a = self.style_raw.attrs();
            let keep_or = |a: Attrs, family: Attrs, one: Attrs, on: bool| match (
                on,
                a.bits() & family.bits() != 0,
            ) {
                (true, true) => a,
                (true, false) => a.with(one),
                (false, _) => a.without(family),
            };
            a = keep_or(a, Attrs::BOLD, Attrs::BOLD, rend.has(CirBits::BIT1));
            a = keep_or(
                a,
                Attrs::UNDERLINES,
                Attrs::UNDERLINE,
                rend.has(CirBits::BIT2),
            );
            a = keep_or(a, Attrs::BLINKS, Attrs::BLINK, rend.has(CirBits::BIT3));
            a = keep_or(a, Attrs::REVERSE, Attrs::REVERSE, rend.has(CirBits::BIT4));
            self.style_raw.set_attrs(a);
            self.style = self.intern_style(self.style_raw);
        }
        if let Some(att) = r.attribute {
            self.cold.protect = match att.has(CirBits::BIT1) {
                true => CellFlags::PROTECTED,
                false => CellFlags::NONE,
            };
            self.refresh_flags();
        }
        if let Some(f) = r.flags {
            self.cold.modes.apply(ModeBit::Origin, f.has(CirBits::BIT1));
            let c = self.cx.col();
            let r0 = self.cy;
            self.goto(self.confine(r0), self.confine_col(c));
            if f.has(CirBits::BIT4) && self.cx.col() == self.line_fence() {
                self.cx = CursorCol::written(Col::at_index(self.line_fence().index() + 1));
            }
            self.cold.restored_shift = match (f.has(CirBits::BIT2), f.has(CirBits::BIT3)) {
                (_, true) => Some(GSlot::G3),
                (true, false) => Some(GSlot::G2),
                (false, false) => None,
            };
            if let Some(s) = self.cold.restored_shift {
                self.cold.single = s;
            }
        }
        if let Some(g) = r.gl {
            self.cold.gl = g;
        }
        if let Some(g) = r.gr {
            self.cold.gr = g;
        }
        for (i, d) in r.designations.iter().enumerate() {
            if let Some(d) = *d {
                self.cold.g[i] = d.charset();
                self.cold.desig[i] = d;
            }
        }
        self.invoke();
    }
}

/// The bytes of the string accumulator a tab-stop restore keeps its columns
/// in, a bit each for every column of the widest screen.
const TAB_BITS: usize = crate::storage::MAX_COLS.div_ceil(8);
const _: () = assert!(TAB_BITS <= crate::storage::OSC_MAX);
