//! The reports a VT420 and xterm answer that ride on no other operation:
//! the DEC private device status reports, DECREQTPARM, DECRQUPSS, DECRQDE,
//! XTQMODKEYS and XTQFMTKEYS. Each answers only what is true of baud -- no
//! printer, no user-defined keys, no DEC locator, no macro memory, one page,
//! one session, and a pty with no line to fail -- in the shape its text
//! gives (`baud_domain::Report`'s variants carry each source).
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
    pub(crate) fn answer_query(&mut self, op: QueryOp, csi: &crate::write::Csi) {
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
        match call {
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

    fn dec_status(&mut self, k: DecStatus) {
        let r = match k {
            DecStatus::ExtendedCursor => {
                let (row, col) = self.reported_cursor();
                Report::ExtendedCursorPosition { row, col }
            }
            DecStatus::Printer => Report::DecStatus(DecReply::NoPrinter),
            DecStatus::UserKeys => Report::DecStatus(DecReply::NoUserKeys),
            DecStatus::Keyboard => Report::DecStatus(DecReply::KeyboardUnknown),
            DecStatus::LocatorStatus => Report::DecStatus(DecReply::NoLocator),
            DecStatus::LocatorType => Report::DecStatus(DecReply::UnknownLocator),
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
    /// `for (value = 0; value < nparam; ++value)` gives, for the one
    /// resource baud holds, `Pp = 4`. Any other resource is counted: baud
    /// sends the cursor, function and keypad keys from ctlseqs' tables
    /// rather than from a resource, so it has no value to report for them.
    fn key_options(&mut self, which: KeyOption, csi: &crate::write::Csi) {
        let mut any = false;
        csi.for_each_param(|pp| {
            any = true;
            match pp.map(|v| v.value()) {
                Some(4) => {
                    let value = match which {
                        KeyOption::Modify => self.cold.other_keys.level(),
                        KeyOption::Format => self.cold.other_keys_format.code(),
                    };
                    self.want_reply(Report::KeyOption { which, value });
                }
                _ => self.unsupported_param += 1,
            }
        });
        if !any {
            self.unsupported_param += 1;
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
        // ([`CursorCol::reported`]).
        let col = self.cx.reported(self.line_fence(), self.cols);
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
