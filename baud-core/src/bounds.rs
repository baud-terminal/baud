//! **SLH and SLL, `CSI Pn SP U` and `CSI Pn SP V`** (`[[source: ecma48]]`
//! 8.3.122, 8.3.123): a line home and a line limit of their own, apart from
//! DEC's margins (Jev, OWN_POSITIONS, 0.96/0.98).
//!
//! The line home is "the position to which the active presentation position
//! will be moved by subsequent occurrences of CARRIAGE RETURN (CR), DELETE
//! LINE (DL), INSERT LINE (IL) or NEXT LINE (NEL)", in both DCSM states;
//! the line limit takes `CR` and `NEL` only "if the parameter value of
//! SELECT IMPLICIT MOVEMENT DIRECTION (SIMD) is equal to 1", which it never
//! is here, SIMD being counted. **Under DATA**, and with BDSM set, which
//! reads DCSM as DATA (7.3.4 c), the home is also "the position ahead of
//! which no implicit movement of the active data position shall occur" and
//! the limit "the position beyond which no implicit movement ... shall
//! occur": a character written at the limit wraps to the next line's home
//! under DECAWM, or stays there with DECAWM reset, as at a right margin.
//! Under PRESENTATION those clauses are a "device without data
//! component"'s, and baud has one, so the limit does nothing there and a
//! wrap goes to DEC's left margin. **A wrap waiting at the limit is
//! reported on it**: the active position never passed it, as DEC's Last
//! Column Flag leaves it on the last column (`[[source: dec-std-070]]`
//! D.6.1), so `CPR` names the limit ([`report_fence`]).
//!
//! **DEC's margins still bound ICH, DCH, scrolling and DECOM**, and where
//! both are set the inner bound holds: the home past the left margin, the
//! limit short of the right. `CNL` and `CPL` go to "the first character
//! position" (8.3.12, 8.3.13), DEC's line start, and `ICH` leaves the
//! cursor (the `CSI @` row). Under DECRLM `CR` keeps DECRLM's right margin
//! (Jev's lean, DECRLM_MARGIN, 0.74/0.88 then 0.46/0.42).
//!
//! **A position past the line is its last column** (Jev split: CLAMPED
//! 0.04/0.06 asked again, after IGNORED_COUNTED 0.10/0.18), and an omitted
//! or zero `Pn`, which has no default, is counted and changes nothing. A
//! resize keeps both, each acting as the line's last column while the line
//! is narrower (Jev, KEPT_CLAMPED, 1.00/1.00). `RIS` forgets them; `DECSTR`
//! keeps them, as no DEC or xterm soft reset names them (Jev, KEEP,
//! 0.96/0.96).
//!
//! **What it costs while nothing sets them: nothing on the text path.**
//! The writes the limit and the home change go a character at a time, the
//! one-by-one graph IRM's and SAPV's take, chosen by
//! [`ModeBit::LineBounded`]; the limit lowers the fence those writes test,
//! where a double line's half lowers it, and the home is read by the out of
//! line wraps. `CR` maps through [`ModeBit::ReturnMapped`], the bit it
//! already tests.

use baud_domain::{
    Align, BidiSupport, Col, Cols, Count, CursorCol, DeviceComponent, ModeBit, OneBased, PositionOp,
};

use crate::paint::Paint;
use crate::term::{Cold, Pinned};

/// SLH's line home and SLL's line limit, each a column once set.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct LineBounds {
    pub(crate) home: Option<Col>,
    pub(crate) limit: Option<Col>,
}

impl LineBounds {
    /// Neither set: power-on and after `RIS`.
    pub(crate) const NONE: LineBounds = LineBounds {
        home: None,
        limit: None,
    };
}

/// Whether the implicit-movement clauses apply: DCSM at DATA, or BDSM set.
fn data_rules(cold: &Cold) -> bool {
    cold.bidi == BidiSupport::Implicit || cold.dcsm == DeviceComponent::Data
}

/// **Whether a line limit bounds implicit movement**, so a write can leave
/// a wrap waiting at it: `HT` then maps through
/// [`ModeBit::ReturnMapped`], out of line, where the wait is ended on the
/// limit before the stops are counted ([`Paint::end_limit_wait`]).
pub(crate) fn limit_bounds(cold: &Cold) -> bool {
    cold.bounds.limit.is_some() && data_rules(cold)
}

/// **The fence a write tests**: `f`, a line's own, lowered to the line
/// limit while it bounds implicit movement.
pub(crate) fn limited(f: Col, cold: &Cold, cols: Cols) -> Col {
    match (cold.modes.is_set(ModeBit::LineBounded), cold.bounds.limit) {
        (true, Some(l)) => f.min(l.min(Col::at_index(cols.index() - 1))),
        _ => f,
    }
}

/// **The fence a report reads the cursor against**: `own`, the line's, or
/// the line limit while a write left a wrap waiting there
/// ([`Pinned::AtLimit`]), "the position beyond which no implicit movement
/// of the active data position shall occur" (8.3.123), as DEC's Last
/// Column Flag leaves the active position on the last column, "the Active
/// Position cannot be advanced" (`[[source: dec-std-070]]` D.6.1).
pub(crate) fn report_fence(own: Col, cold: &Cold, cols: Cols) -> Col {
    match cold.pinned == Pinned::AtLimit {
        true => limited(own, cold, cols),
        false => own,
    }
}

impl Paint<'_, '_> {
    /// The fence a report reads the cursor against ([`report_fence`]).
    pub(crate) fn report_fence(&self) -> Col {
        report_fence(self.line_fence(), self.cold, self.cols)
    }

    /// **The column a forward move counts from**: the cursor's, or the
    /// line limit where a wrap waits there, the active position a report
    /// names (Jev, FROM_LIMIT, 0.92/0.96): 8.3.123 bounds only implicit
    /// movement, DEC's Last Column Flag leaves "the Active Position" on
    /// the last column (`[[source: dec-std-070]]` D.6.1) and its CUF and
    /// HT algorithms add to that column, and xterm's `CursorForward` and
    /// `TabNext` count from the cursor column, which stays there while the wrap
    /// waits (`[[source: xterm-cursor-c]]` 211-241, `[[source:
    /// xterm-tabs-c]]` 109-124). So `CUF`, `HT`, `CHT`, `CBT`, `HTS`,
    /// `TBC 0` and HTJ count from the limit. `CUB` counts the wait as its
    /// first step, as at a right margin, and `HPB` from the reported column.
    pub(crate) fn moved_from(&self) -> Col {
        match self.cold.pinned == Pinned::AtLimit {
            true => self.cx.reported(self.report_fence(), self.cols),
            false => self.cx.col(),
        }
    }

    /// **A wrap waiting at the line limit ended on the limit**, before a
    /// move that counts from the cursor's column ([`Paint::moved_from`]).
    /// One test inline; the rest out of line.
    #[inline(always)]
    pub(crate) fn end_limit_wait(&mut self) {
        if self.cold.pinned == Pinned::AtLimit {
            self.end_limit_wait_on_limit();
        }
    }

    /// The cursor put on the limit it waited past, which ends the wait.
    #[cold]
    #[inline(never)]
    fn end_limit_wait_on_limit(&mut self) {
        let on = self.moved_from();
        self.set_col(CursorCol::moved(on));
    }

    /// **Before a character of `need` columns is written while the line is
    /// bounded**: one that fills the line to its limit, short of the line's
    /// own end, with DECAWM set, leaves a wrap waiting at the limit, and
    /// any other write ends one.
    #[cold]
    #[inline(never)]
    pub(crate) fn limit_step(&mut self, need: Count) {
        if !self.cold.modes.is_set(ModeBit::AutoWrap) {
            return;
        }
        let own = self.line_fence();
        let limit = limited(own, self.cold, self.cols);
        let end = self.cx.raw().index() + need.index();
        if limit < own && end == limit.index() + 1 {
            self.cold.pinned = Pinned::AtLimit;
        } else if self.cold.pinned == Pinned::AtLimit {
            self.cold.pinned = Pinned::Before;
        }
    }

    /// **TSR, SLH, SLL, the aligned stops or a page function**, out of
    /// line ([`baud_domain::Csi::Position`]).
    #[cold]
    #[inline(never)]
    pub(crate) fn set_position(
        &mut self,
        op: PositionOp,
        n: Option<OneBased>,
        csi: &crate::write::Csi,
    ) {
        // TSR and PPA, PPB and PPR are format effectors (8.2.4), and NP and
        // PP move the cursor: each ends an aligned string.
        if matches!(op, PositionOp::RemoveStop | PositionOp::Page(_)) {
            self.effector_ends_string();
        }
        match op {
            PositionOp::RemoveStop => self.remove_stop(n),
            PositionOp::LineHome => self.set_line_home(n),
            PositionOp::LineLimit => self.set_line_limit(n),
            PositionOp::Aligned(a) => self.set_aligned_stop(a, n),
            // TCC's target, its second parameter (8.3.155); one it does not
            // permit is counted.
            PositionOp::CentredOnTarget => {
                match baud_domain::TargetChar::of_params(&csi.params()) {
                    Some(t) => self.set_aligned_stop(Align::OnCharacter(t), n),
                    None => self.unsupported_param += 1,
                }
            }
            PositionOp::Page(p) => self.move_page(p, n),
        }
    }

    /// **SLH**: the line home at position `n`, the last column for one
    /// past the line.
    fn set_line_home(&mut self, n: Option<OneBased>) {
        let Some(n) = n else {
            self.unsupported_param += 1;
            return;
        };
        self.cold.bounds.home = Some(n.to_col(self.cols));
        self.refresh_presentation_edits();
    }

    /// **SLL**: the line limit at position `n`, the last column for one
    /// past the line.
    fn set_line_limit(&mut self, n: Option<OneBased>) {
        let Some(n) = n else {
            self.unsupported_param += 1;
            return;
        };
        self.cold.bounds.limit = Some(n.to_col(self.cols));
        self.refresh_presentation_edits();
    }

    /// **[`ModeBit::LineBounded`] set as its conditions say**, and the fence
    /// the cursor's line has under it: called from
    /// [`Paint::refresh_presentation_edits`], wherever BDSM, DCSM, SLH or
    /// SLL change, before the graph is chosen again.
    pub(crate) fn refresh_bounds(&mut self) {
        let b = self.cold.bounds;
        let on = (b.home.is_some() || b.limit.is_some()) && data_rules(self.cold);
        self.cold.modes.apply(ModeBit::LineBounded, on);
        self.cold.fence = self.write_fence_of_line();
        // **A wrap waiting at a limit that moved ends on the position it
        // waited on**, the cursor one back from the sentinel, as DECSWL,
        // DECDWL and DECDHL end one when a line's end changes (D.6.1;
        // `Paint::line_rendition`): an SLL elsewhere, or BDSM or DCSM
        // making the limit bound nothing (Jev's lean, END_ON_POSITION, by
        // mean probability 0.44 against END_AFTER's 0.30 and KEEP_WAIT's
        // 0.27 on a second ask). An SLL naming the same position keeps it.
        if self.cold.pinned == Pinned::AtLimit {
            let waited = self.cx.raw().before();
            if self.cold.fence != waited {
                self.set_col(CursorCol::moved(waited));
            }
        }
    }

    /// The fence a write on the cursor's line tests: its own, lowered to
    /// the line limit while it bounds implicit movement. Out of line, as
    /// the line's own fence is, for [`Paint::refresh_fence`].
    #[cold]
    #[inline(never)]
    pub(crate) fn write_fence_of_line(&self) -> Col {
        let own = crate::paint::fence_of(self.view.rendition(self.cy), self.cold.right, self.cols);
        limited(own, self.cold, self.cols)
    }

    /// **The column `CR`, `NEL`, `IL` and `DL` take**: `dec`, the line start
    /// DEC's margins give, moved on to the line home once SLH has set one,
    /// as the inner of the two; under DECRLM DEC's alone.
    pub(crate) fn homed(&self, dec: Col) -> Col {
        match self.cold.bounds.home {
            Some(h) if !self.cold.modes.is_set(ModeBit::CursorRightToLeft) => {
                dec.max(h.min(Col::at_index(self.cols.index() - 1)))
            }
            _ => dec,
        }
    }

    /// **A wrap, then the line home under DATA**: the next line starts no
    /// further back than the home, "the position ahead of which no implicit
    /// movement ... shall occur". For the out of line writers, which the
    /// one-by-one graph sends every character to while
    /// [`ModeBit::LineBounded`] is set. The wrap inline, as those writers
    /// had it, and the home one bit and a call out of line.
    #[inline(always)]
    pub(crate) fn wrap_bounded(&mut self) {
        self.wrap();
        if self.cold.modes.is_set(ModeBit::LineBounded) {
            self.home_after_wrap();
        }
    }

    #[cold]
    #[inline(never)]
    fn home_after_wrap(&mut self) {
        if self.cold.modes.is_set(ModeBit::AutoWrap) {
            let c = self.homed(self.cx.col());
            self.cx = CursorCol::moved(c);
        }
    }
}
