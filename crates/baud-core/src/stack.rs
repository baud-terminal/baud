//! xterm's stack controls (`[[source: ctlseqs]]`), out of the dispatch's
//! line: XTTITLEPOS, which reports the title stack `crate::window` keeps,
//! XTPUSHSGR and XTPOPSGR, the rendition's stack, and XTPUSHCOLORS,
//! XTPOPCOLORS and XTREPORTCOLORS, the palette's, kept here.
//!
//! **One cold entry, [`Paint::stack_op`], which reads the parameters too**,
//! as `crate::rect`'s `area_op` does: no byte of the write loop reaches it.

use baud_domain::{
    Chan, ColorWord, DynamicColor, Param, Report, SGR_STACK_DEPTH, SgrGroups, SpecialColor,
    StackOp, StyleWord, restore_rendition,
};

use crate::storage::{
    COLOR_SLOT, COLOR_STACK_PALETTE, CURSOR_TEXT, EMBEDDER_PALETTE, SPECIAL_END, SPECIAL_PALETTE,
    VISUAL_BELL,
};

use crate::paint::Paint;
use crate::write::Csi;

impl<M: crate::bands::BandMode> Paint<'_, '_, M> {
    /// One stack control, its parameters read here.
    #[cold]
    #[inline(never)]
    pub(crate) fn stack_op(&mut self, op: StackOp, csi: &Csi) {
        match op {
            StackOp::TitlePosition => self.report_title_position(),
            StackOp::PushRendition => self.push_rendition(csi),
            StackOp::PopRendition => self.pop_rendition(),
            // Each parameter in turn, as xterm's loop takes them, and a
            // bare control as one omitted.
            StackOp::PushColors | StackOp::PopColors => {
                let mut any = false;
                csi.for_each_param(|v| {
                    any = true;
                    self.palette_stack(op, Slot::of(v));
                });
                if !any {
                    self.palette_stack(op, Slot::Next);
                }
            }
            StackOp::ReportColors => self.report_colors(),
        }
    }

    fn palette_stack(&mut self, op: StackOp, slot: Slot) {
        match op {
            StackOp::PushColors => self.push_colors(slot),
            _ => self.pop_colors(slot),
        }
    }

    /// XTPUSHCOLORS, one parameter of it, as xterm's `xtermPushColors`
    /// stores it (`[[source: xterm-misc]]`): omitted or 0, the palette goes
    /// to the current entry and the entry moves up one, refused at ten; 1
    /// to 10 stores it into that palette "without pushing", the entry left
    /// where it is, as the title stack's direct slots are. xterm makes it
    /// the current entry, a departure from the text, which is built here.
    /// One past 10, or a push of a full stack, changes nothing and is
    /// counted.
    fn push_colors(&mut self, slot: Slot) {
        let used = self.strs.colors.used.0;
        let (at, next) = match slot {
            Slot::Next if used < SGR_STACK_DEPTH => (used, used + 1),
            Slot::Direct(n) => (n - 1, used),
            Slot::Next | Slot::Past => {
                self.unsupported_param += 1;
                return;
            }
        };
        let base = COLOR_STACK_PALETTE + at * COLOR_SLOT;
        let p = &mut *self.strs.palette;
        p.copy_within(..EMBEDDER_PALETTE, base);
        p.copy_within(SPECIAL_PALETTE..SPECIAL_END, base + EMBEDDER_PALETTE);
        let dynamic = base + EMBEDDER_PALETTE + SpecialColor::ALL.len();
        p[dynamic..dynamic + DynamicColor::COUNT].copy_from_slice(&self.cold.dynamic);
        p[dynamic + DynamicColor::COUNT] = p[CURSOR_TEXT];
        p[dynamic + DynamicColor::COUNT + 1] = p[VISUAL_BELL];
        self.strs.colors.filled = self.strs.colors.filled.with(Entry(at));
        self.strs.colors.used = Entry(next);
    }

    /// XTPOPCOLORS, one parameter of it, as xterm's `xtermPopColors`
    /// restores it: omitted or 0, the entry below the current one, which
    /// becomes the current entry; 1 to 10, that palette "without popping",
    /// the entry left where it is (xterm moves it, not followed, as above).
    /// Either way a palette stored there becomes the colours. A palette
    /// never stored, an empty
    /// stack or one past 10 changes nothing and is counted. **Each colour
    /// goes through its setter**, so the colour epoch moves only where one
    /// changed and the renderer repaints what a pop recoloured.
    fn pop_colors(&mut self, slot: Slot) {
        let used = self.strs.colors.used.0;
        let (at, next) = match (slot, used) {
            (Slot::Next, used) if used > 0 => (used - 1, used - 1),
            (Slot::Direct(n), _) => (n - 1, used),
            _ => {
                self.unsupported_param += 1;
                return;
            }
        };
        if !self.strs.colors.filled.has(Entry(at)) {
            self.unsupported_param += 1;
            return;
        }
        let base = COLOR_STACK_PALETTE + at * COLOR_SLOT;
        let mut saved = [ColorWord::DEFAULT; COLOR_SLOT];
        saved.copy_from_slice(&self.strs.palette[base..base + COLOR_SLOT]);
        for (i, &w) in (0..=u8::MAX).zip(&saved[..EMBEDDER_PALETTE]) {
            self.set_palette(Chan::from_u8(i), w);
        }
        let specials = &saved[EMBEDDER_PALETTE..EMBEDDER_PALETTE + SpecialColor::ALL.len()];
        for (&which, &w) in SpecialColor::ALL.iter().zip(specials) {
            self.set_special(which, w);
        }
        let dynamic = &saved[EMBEDDER_PALETTE + SpecialColor::ALL.len()..];
        for (&which, &w) in DYNAMIC.iter().zip(dynamic) {
            self.set_dynamic(which, w);
        }
        self.strs.palette[CURSOR_TEXT] = saved[COLOR_SLOT - 2];
        self.strs.palette[VISUAL_BELL] = saved[COLOR_SLOT - 1];
        self.strs.colors.used = Entry(next);
    }

    /// XTREPORTCOLORS: the current entry and the palettes stored,
    /// `CSI ? Pu ; Ps # Q`. **The palettes stored are counted**, the
    /// text's "number of palettes stored on the stack", which is xterm's
    /// `last` wherever only bare pushes were made; xterm's does not count a
    /// palette a direct push stored.
    fn report_colors(&mut self) {
        let c = self.strs.colors;
        self.want_reply(Report::ColorStack {
            current: u16::try_from(c.used.0).unwrap_or(u16::MAX),
            stored: u16::try_from(c.filled.0.count_ones()).unwrap_or(u16::MAX),
        });
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
        let kept = self.kept_rendition();
        let s = &mut self.strs.renditions;
        let used = s.used;
        if let Some(level) = s.saved.get_mut(used.0) {
            *level = (kept, groups);
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
            self.style_raw = self.keeping_string(restore_rendition(saved, self.style_raw, groups));
            self.style = self.intern_style(self.style_raw);
            self.rendition_changed();
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

/// The dynamic colours in the order [`DynamicColor::index`] numbers them.
const DYNAMIC: [DynamicColor; DynamicColor::COUNT] = [
    DynamicColor::Foreground,
    DynamicColor::Background,
    DynamicColor::Cursor,
    DynamicColor::HighlightBg,
    DynamicColor::HighlightFg,
];

/// Which palette a parameter of XTPUSHCOLORS or XTPOPCOLORS names: "the
/// default 0 will push" (or pop), and 1 to 10 a palette directly; `Past`
/// beyond.
#[derive(Clone, Copy)]
enum Slot {
    Next,
    Direct(usize),
    Past,
}

impl Slot {
    fn of(v: Option<Param>) -> Slot {
        match v.map_or(0, Param::value) {
            0 => Slot::Next,
            n @ 1..=10 => Slot::Direct(usize::from(n)),
            _ => Slot::Past,
        }
    }
}

/// The colour stack: which palettes hold one, one bit each, and the
/// current entry, xterm's `saved_colors.used`.
#[derive(Clone, Copy)]
pub(crate) struct ColorStack {
    filled: Filled,
    used: Entry,
}

impl ColorStack {
    pub(crate) const EMPTY: ColorStack = ColorStack {
        filled: Filled(0),
        used: Entry(0),
    };
}

/// Which of the ten palettes a push has stored, one bit each: xterm never
/// frees one, so a pop may read any stored once.
#[derive(Clone, Copy)]
struct Filled(u16);

impl Filled {
    fn with(self, at: Entry) -> Filled {
        Filled(self.0 | 1 << at.0)
    }
    fn has(self, at: Entry) -> bool {
        self.0 & (1 << at.0) != 0
    }
}

/// The colour stack's current entry: the palette the next bare push takes.
#[derive(Clone, Copy)]
struct Entry(usize);
