//! xterm's stack controls (`[[source: ctlseqs]]`), out of the dispatch's
//! line: XTTITLEPOS, which reports the title stack `crate::window` keeps,
//! and XTPUSHSGR and XTPOPSGR, the rendition's stack, kept here.
//!
//! **One cold entry, [`Paint::stack_op`], which reads the parameters too**,
//! as `crate::rect`'s `area_op` does: no byte of the write loop reaches it.

use baud_domain::{SGR_STACK_DEPTH, SgrGroups, StackOp, StyleWord, restore_rendition};

use crate::paint::Paint;
use crate::write::Csi;

impl Paint<'_, '_> {
    /// One stack control, its parameters read here.
    #[cold]
    #[inline(never)]
    pub(crate) fn stack_op(&mut self, op: StackOp, csi: &Csi) {
        match op {
            StackOp::TitlePosition => self.report_title_position(),
            StackOp::PushRendition => self.push_rendition(csi),
            StackOp::PopRendition => self.pop_rendition(),
        }
    }

    /// XTPUSHSGR: the rendition and the groups to restore, as xterm's
    /// `CASE_XTERM_PUSH_SGR` and `xtermPushSGR` read them (`[[source:
    /// xterm-reset]]`'s file, `[[source: xterm-misc]]`): no parameter, or one
    /// omitted, saves every aspect; a list saves the groups it names, so
    /// `CSI 0 # {` saves none. **A push past the tenth level stores nothing
    /// and still counts**, so its pop restores nothing and the pops below it
    /// still pair with their pushes.
    fn push_rendition(&mut self, csi: &Csi) {
        let (mut listed, mut groups) = (0, SgrGroups::NONE);
        csi.for_each_param(|v| {
            listed += 1;
            if let Some(n) = v {
                listed += 1;
                groups = groups.with(n);
            }
        });
        // One parameter, omitted, is xterm's `nparam == 1 && GetParam(0) ==
        // DEFAULT`, a bare push; anything more is a list.
        if listed <= 1 {
            groups = SgrGroups::ALL;
        }
        let s = &mut self.strs.renditions;
        let used = s.used;
        if let Some(level) = s.saved.get_mut(used.0) {
            *level = (self.style_raw, groups);
        }
        s.used = used.pushed();
    }

    /// XTPOPSGR: the level the last push took, its groups restored into
    /// the rendition and interned. A pop of an empty stack changes nothing
    /// and is counted.
    fn pop_rendition(&mut self) {
        let s = &mut self.strs.renditions;
        let Some(used) = s.used.popped() else {
            self.unsupported_param += 1;
            return;
        };
        s.used = used;
        if let Some(&(saved, groups)) = s.saved.get(used.0) {
            self.style_raw = restore_rendition(saved, self.style_raw, groups);
            self.style = self.intern_style(self.style_raw);
        }
    }
}

/// XTPUSHSGR's ten levels, each a rendition and the groups its pop
/// restores, and the pushes not yet popped, which may pass ten as xterm's
/// `saved_sgr.used` does.
#[derive(Clone, Copy)]
pub(crate) struct RenditionStack {
    saved: [(StyleWord, SgrGroups); SGR_STACK_DEPTH],
    used: Depth,
}

impl RenditionStack {
    pub(crate) const EMPTY: RenditionStack = RenditionStack {
        saved: [(StyleWord::DEFAULT, SgrGroups::NONE); SGR_STACK_DEPTH],
        used: Depth(0),
    };
}

/// A count of pushes not yet popped.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Depth(usize);

impl Depth {
    fn pushed(self) -> Depth {
        Depth(self.0.saturating_add(1))
    }
    fn popped(self) -> Option<Depth> {
        self.0.checked_sub(1).map(Depth)
    }
}
