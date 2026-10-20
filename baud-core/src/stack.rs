//! xterm's stack controls (`[[source: ctlseqs]]`), out of the dispatch's
//! line: XTTITLEPOS, which reports the title stack `crate::window` keeps.
//!
//! **One cold entry, [`Paint::stack_op`], which reads the parameters too**,
//! as `crate::rect`'s `area_op` does: no byte of the write loop reaches it.

use baud_domain::StackOp;

use crate::paint::Paint;
use crate::write::Csi;

impl Paint<'_, '_> {
    /// One stack control, its parameters read here.
    #[cold]
    #[inline(never)]
    pub(crate) fn stack_op(&mut self, op: StackOp, _csi: &Csi) {
        match op {
            StackOp::TitlePosition => self.report_title_position(),
        }
    }
}
