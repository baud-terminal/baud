//! The reports a VT420 and xterm answer that ride on no other operation:
//! the DEC private device status reports, DECREQTPARM, DECRQUPSS, DECRQDE,
//! XTQMODKEYS and XTQFMTKEYS. Each answers only what is true of baud -- no
//! printer unless the embedder has one, no user-defined keys, a DEC
//! locator that is a mouse, no macro memory, one page, one session, and a
//! pty with no line to fail -- in the shape its text gives
//! (`baud_domain::Report`'s variants carry each source).
//!
//! **One cold entry, [`Paint::answer_query`], which reads the parameters
//! too**, as [`Paint::area_op`] does: `csi_final` hands it the sequence and
//! keeps nothing of it live.

use crate::paint::Paint;
use baud_domain::*;

impl Paint<'_, '_> {
    /// A report: its parameters read, then answered.
    #[cold]
    #[inline(never)]
    pub(crate) fn answer_query(
        &mut self,
        op: QueryOp,
        csi: &crate::write::Csi,
        node: &mut crate::graph::Node,
    ) {
        let p = csi.params();
        // **Parameters that name no report are an unsupported sequence**,
        // counted where `csi_final` counts one: the parse leaves them to be
        // read here, out of line.
        let Some(call) = op.with(&p) else {
            self.unsupported_total += 1;
            #[cfg(feature = "diag")]
            {
                self.unsupported_by_final[op.final_byte().slot()] += 1;
            }
            return;
        };
        // **Each private report is its own function**, and one the level in
        // force does not define is not answered and is counted, as
        // `csi_final` counts a control (`DecStatus::function`).
        if let QueryCall::DecStatus(k) = call {
            if !k.function().introduced().admits(self.cold.level) {
                self.unsupported_total += 1;
                return;
            }
        }
        match call {
            QueryCall::DecStatus(DecStatus::Printer) if self.strs.print.hold == PrinterHold::On => {
                self.hold_printer_status(node)
            }
            QueryCall::DecStatus(k) => self.dec_status(k),
            QueryCall::TerminalParameters { sol } => {
                self.want_reply(Report::TerminalParameters { sol })
            }
            QueryCall::PreferredSupplemental => self.want_reply(Report::PreferredSupplemental),
            QueryCall::DisplayedExtent => self.want_reply(Report::DisplayedExtent {
                rows: self.rows,
                cols: self.cols,
            }),
            QueryCall::ModifyKeys => self.key_options(KeyOption::Modify, csi),
            QueryCall::FormatKeys => self.key_options(KeyOption::Format, csi),
        }
    }

    /// **DSR `?15` under [`PrinterHold::On`]: the read stops here and the
    /// answer is owed**, so the embedder can look at its printer, which
    /// may have come or gone since it last said, and answer through
    /// [`crate::Term::set_printer`] before the rest of the read is parsed.
    /// A real port's status is its printer's at the moment it is asked
    /// (070-7 § 7.3.2), and a list read off the read loop is not (Jev's
    /// lean, HOLD, 0.92/0.89).
    fn hold_printer_status(&mut self, node: &mut crate::graph::Node) {
        self.strs.print.asked = PrinterAsked::Owed;
        self.hold_read(node);
    }

    /// A DEC private status report, answered at once.
    fn dec_status(&mut self, k: DecStatus) {
        let r = match k {
            DecStatus::ExtendedCursor => {
                let (row, col) = self.reported_cursor();
                Report::ExtendedCursorPosition { row, col }
            }
            // "?10n" ready while the embedder has a printer, "?11n" not
            // ready while it had one since power-up or RIS, else "?13n",
            // "not connected" (070-7 p. 7-32): the answer and what a print
            // does agree, as § 7.3.2 ties them.
            DecStatus::Printer => Report::DecStatus(self.strs.print.reply()),
            DecStatus::UserKeys => Report::DecStatus(DecReply::NoUserKeys),
            DecStatus::Keyboard => Report::DecStatus(DecReply::KeyboardUnknown),
            DecStatus::LocatorStatus => Report::DecStatus(match self.cold.locator.device {
                LocatorDevice::Present => DecReply::LocatorReady,
                LocatorDevice::Absent => DecReply::NoLocator,
            }),
            DecStatus::LocatorType => Report::DecStatus(match self.cold.locator.device {
                LocatorDevice::Present => DecReply::LocatorMouse,
                LocatorDevice::Absent => DecReply::LocatorUnknown,
            }),
            DecStatus::MacroSpace => Report::MacroSpace,
            // The checksum of the macro definitions, and there are none:
            // the sum of nothing, negated, is 0.
            DecStatus::MemoryChecksum { id } => Report::AreaChecksum {
                id,
                sum: Checksum::new(),
            },
            // 070-4's algorithm reports the flag and clears it: "?73n" and
            // `power_up_detected_flag = FALSE`.
            DecStatus::DataIntegrity => {
                let now = self.cold.replies.integrity();
                self.cold.replies = self.cold.replies.with_integrity(Integrity::Ok);
                Report::DecStatus(DecReply::DataIntegrity(now))
            }
            DecStatus::Sessions => Report::DecStatus(DecReply::SingleSession),
        };
        self.want_reply(r);
    }

    /// XTQMODKEYS and XTQFMTKEYS: one answer a parameter, as xterm's
    /// `for (value = 0; value < nparam; ++value)` gives, every resource
    /// with its value, as `report_mod_fkeys` and `report_fmt_fkeys` write
    /// it. A `Pp` naming none is answered as xterm answers it, with its
    /// `DEFAULT` written unsigned, 65535 (Jev's lean, XTERM_65535), and
    /// counted; a disabled one, -1, the same 65535, as `report_mod_fkeys`
    /// writes it; an omitted one gets nothing and is counted.
    fn key_options(&mut self, which: KeyOption, csi: &crate::write::Csi) {
        let mut any = false;
        csi.for_each_param(|pp| {
            any = true;
            let Some(pp) = pp.map(|v| v.value()) else {
                self.unsupported_param += 1;
                return;
            };
            let value = match (KeyResource::from_param(pp), which) {
                (Some(r), KeyOption::Modify) => self.cold.keys.modify(r).reported(),
                (Some(r), KeyOption::Format) => u16::from(self.cold.keys.format(r).code()),
                (None, _) => {
                    self.unsupported_param += 1;
                    ModifyValue::DISABLED.reported()
                }
            };
            self.want_reply(Report::KeyOption { which, pp, value });
        });
        if !any {
            self.unsupported_param += 1;
        }
    }

    /// XTMODKEYS, XTFMTKEYS or `CSI > Ps n`: its parameters read here, out
    /// of `csi_final`, as a report's are, and acted on; a shape or value the
    /// texts do not give is counted where `csi_final` counts one.
    #[cold]
    #[inline(never)]
    pub(crate) fn key_control(&mut self, op: KeyControl, csi: &crate::write::Csi) {
        match key_change(&csi.params(), op) {
            Some(KeyChange::Modify(set)) => self.cold.keys.apply(set),
            Some(KeyChange::Format(set)) => self.cold.keys.apply_format(set),
            None => {
                self.unsupported_total += 1;
                #[cfg(feature = "diag")]
                {
                    self.unsupported_by_final[op.final_byte().slot()] += 1;
                }
            }
        }
    }

    /// The cell `CPR` and `DECXCPR` report, 1-based, so the two cannot
    /// disagree.
    ///
    /// **Relative to the origin when DECOM is set**, which is what
    /// `[[source: vt510-decom]]` requires: an application inside a margin
    /// asks where it is and must be told in its own coordinates.
    #[inline]
    pub(crate) fn reported_cursor(&self) -> (OneBased, OneBased) {
        let base = self.origin_row().index();
        let row = Row::from_index(self.cy.index().saturating_sub(base));
        // **The last writable column, not the sentinel one past it.** With
        // `DECAWM` on, the cursor parks one past the right margin until the
        // next character wraps it, and `CPR` reported that -- a column the
        // application cannot address, off the end of its own row. The same
        // clamp `Term::cursor` carries, which is where this was found the
        // first time.
        //
        // **A cursor placed one past it, or right of the fence, reports its
        // own column**: only the sentinel waits on a column it is not on
        // ([`CursorCol::reported`]). **And a wrap waiting at SLL's line
        // limit is on the limit** (`crate::bounds`).
        let col = self.cx.reported(self.report_fence(), self.cols);
        // **Its presentation position under DCSM's PRESENTATION**, on a line
        // holding a string (`crate::component`).
        let col = match self.cold.modes.is_set(ModeBit::PresentationEdits) {
            true => self.reported_presented(col),
            false => col,
        };
        // **And the column from the left margin under DECOM**:
        // `active_position.column - left_margin + 1` (`[[source:
        // dec-std-070]]` CPR, p. 5-54, and DECXCPR, p. 5-56), so a `CUP` and
        // the report agree. The subtraction cannot saturate: under DECOM
        // every move lands inside the margins (`Paint::confine` and
        // `Paint::confine_col`), and the record, "DECOM's origin is the left
        // margin too", lists every place the column is set.
        let col = Col::at_index(col.index().saturating_sub(self.origin_col().index()));
        (row.one_based(), col.one_based())
    }
}
