//! **NP, PP, PPA, PPR and PPB on baud's one page** (`[[source: ecma48]]`
//! 8.3.87, 8.3.95 to 8.3.98; `[[source: dec-std-070]]` pp. 5-77 to 5-83).
//!
//! baud's page memory is one page, its screen, as xterm's "single page
//! (which corresponds to its visible window)" (`[[source: ctlseqs]]`;
//! Jev, ONE_PAGE, 0.94/0.94). STD 070's algorithms clamp the page to
//! `number_of_pages`, "the highest numbered page is used" and "the first
//! page is used", and NP and PP then take "the home position in the new
//! page obeying Origin Mode" whichever page that is, so on one page NP and
//! PP home the cursor and PPA, PPR and PPB, which keep "the corresponding
//! line and column", move nothing (Jev, STD070_HOME, 0.92/0.98). The
//! VT420's and VT510's "If there is only one page, then the terminal
//! ignores NP" (`[[source: vt510-np]]`) is the products' departure, not
//! followed. A control asking for a page past the first is counted
//! ([`crate::Term::pages_missing`]).
//!
//! **Off the write path**: one arm of `Csi::Position`, out of line, so the
//! per-byte loop and the control dispatch carry nothing for it.

use baud_domain::{OneBased, PageOp, PageSought};

use crate::paint::Paint;

impl Paint<'_, '_> {
    /// **A page function**: page 1 is the only page, so the active position
    /// stays on it; NP and PP go home obeying DECOM, as their algorithms
    /// do after the clamp (pp. 5-78, 5-80).
    pub(crate) fn move_page(&mut self, op: PageOp, n: Option<OneBased>) {
        if let PageSought::Other = op.sought(n) {
            self.cold.pages_missing = self.cold.pages_missing.next();
        }
        match op {
            PageOp::Next | PageOp::Preceding => self.home(),
            PageOp::Absolute | PageOp::Forward | PageOp::Backward => {}
        }
    }
}
