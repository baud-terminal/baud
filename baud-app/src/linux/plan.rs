//! **What a frame shows, read from the terminal before it is drawn**: the
//! cursor, the cursor guide, ReGIS's blink, the selection's highlight, the
//! write seal, where the input method's window opens and the overlays'
//! text -- the badge, the find field, the link under the pointer, the
//! lines under links, the tip, a held window's line and the composition --
//! each read once and handed to the frame, which reads none of them from
//! the terminal or the session itself. The trail starts from the planned
//! cursor, and the visual bell, the thumb, the trail's screen, the splash
//! and the fireworks are timed from what the plan read.

use baud_core::Term;
use baud_domain::{CursorGuide, RegisBlink, Row, Seal, SelectedSpan};
pub(crate) use baud_domain::{
    CursorPlan, CursorShown as Shown, Effects, ImeRect, PointerState, WindowEffect,
};
use baud_font::Px;
use baud_render::{Composition, FindLook, LinkHover, LinkLine, LinkLook, TipLook};

/// **The cursor a frame of `term` drawn now draws**, `composed` the
/// composition drawn at its cursor, if one is.
pub(crate) fn cursor_plan(term: &Term<'_>, composed: Option<&Composition>) -> CursorPlan {
    let on_line = term.status_line().and_then(|l| l.cursor);
    let at = match on_line {
        Some(c) => (Row::from_index(term.geometry().1.index()), c),
        None => composed
            .and_then(|c| c.cursor(c.width_on(&term.view(term.window_back()))))
            .unwrap_or(term.cursor()),
    };
    let shown = match term.cursor_visible() || on_line.is_some() {
        true => Shown::Yes,
        false => Shown::No,
    };
    // The trail starts where the cursor is drawn; a hidden one's is
    // where the program placed it, and none while the window looks back.
    let followed = match (term.window_back().index() != 0, shown) {
        (true, _) => None,
        (false, Shown::Yes) => Some(crate::effects::TrailCursor::Shown(at.0, at.1)),
        (false, Shown::No) => term
            .placed_cursor()
            .map(|(r, c)| crate::effects::TrailCursor::Placed(r, c)),
    };
    CursorPlan {
        at,
        style: term.cursor_style(),
        shown,
        presented: crate::caret::presented_cursor(term),
        followed,
    }
}

/// **Everything a frame shows that is read from the terminal or the
/// session**, read once before the frame is drawn.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FramePlan {
    pub(crate) cursor: CursorPlan,
    /// The cursor guide: the program's, else the configured one.
    pub(crate) guide: CursorGuide,
    pub(crate) regis_blink: RegisBlink,
    /// The selection's span on the window, or `?1001`'s region.
    pub(crate) highlight: Option<SelectedSpan>,
    /// The write generation this frame closed, before its first read.
    pub(crate) seal: Seal,
    /// Where the input method's candidate window is asked to open; `None`
    /// while it hides its cursor, so the last stands.
    pub(crate) ime: Option<ImeRect>,
    /// What the visual bell reads of the screen: the `?5` counts, the
    /// video and how a ring shows.
    pub(crate) bell: crate::bell::BellFacts,
    /// The synchronised update's state when the plan was read: the bell's
    /// flash shows what an outlived update held.
    pub(crate) update: crate::bell::Update,
    pub(crate) scrollbar: Option<crate::timed::ScrollbarPlan>,
    pub(crate) moved: crate::timed::ScreenMove,
    pub(crate) splash: crate::splash::SplashGate,
    pub(crate) fireworks: crate::fireworks::FireworksAsked,
}

/// `rect` as `[x, y, width, height]` in pixels from the grid's top-left
/// corner, for a cell `w` by `h` pixels.
pub(crate) fn ime_px(rect: ImeRect, w: Px, h: Px) -> [Px; 4] {
    crate::marked::cells_rect(rect.at, rect.width, w, h)
}

/// **A held window's line**, and the cursor's row it is placed against.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct HeldText {
    pub(crate) line: String,
    pub(crate) row: Row,
}

/// **The text a frame's overlays show**, read from the terminal and the
/// session before the frame, its buffers refilled in place each frame.
#[derive(Default)]
pub(crate) struct PlanText {
    /// The badge's text and colour.
    pub(crate) badge: Option<crate::effects::BadgeText>,
    /// The find field, with the composition typed in it, and the matches
    /// on the window drawn, while the field is open.
    pub(crate) find: Option<FindLook>,
    /// The input method's composition at the cursor, while the find field
    /// is closed.
    pub(crate) preedit: Option<Composition>,
    /// The link previewed under the pointer, and the spans it underlines.
    pub(crate) hover: Option<LinkLook>,
    pub(crate) hover_over: Option<LinkHover>,
    /// The struck lines the link under the pointer holds.
    pub(crate) hover_struck: Vec<LinkLine>,
    /// The lines under links at rest, and those struck through.
    pub(crate) lines: Vec<LinkLine>,
    pub(crate) struck: Vec<LinkLine>,
    /// The tip over the status line's lights.
    pub(crate) tip: Option<TipLook>,
    /// A held window's line.
    pub(crate) held: Option<HeldText>,
}

/// **What the plan finds**: no frame while a synchronised update holds the
/// screen, else what the next frame shows.
#[derive(Clone, Copy, Debug)]
#[expect(
    clippy::large_enum_variant,
    reason = "returned once a frame and taken apart at once; a box would allocate every frame"
)]
pub(crate) enum Planned {
    /// An update holds the screen: no frame is drawn.
    Held,
    /// What the next frame shows.
    Frame(FramePlan),
}

#[cfg(test)]
mod tests {
    /// **An effect pushed is applied once**: the drain hands each over in
    /// the order pushed and leaves none for the next frame.
    #[test]
    fn an_effect_pushed_is_drained_once() {
        use super::{Effects, PointerState, WindowEffect};
        let mut owed = Effects::default();
        let title = WindowEffect::Title("main.rs - vim".to_string());
        let pointer = WindowEffect::Pointer(PointerState::Shown(None));
        owed.push(title.clone());
        owed.push(pointer.clone());
        assert_eq!(owed.drain().collect::<Vec<_>>(), [title, pointer]);
        assert_eq!(owed.drain().count(), 0, "applied once");
    }

    use super::*;
    use baud_core::Owned;
    use baud_domain::WinSize;
    use baud_render::{Blend, Damage, PxSize, Renderer};

    /// **The planned cursor is the cell the renderer draws it in**, on a
    /// whole frame, after a scroll, on the alternate screen, on DEC's
    /// status line and at an input method's composition: the cell is read
    /// back from the pixels a shown phase changes against a hidden one, and
    /// the trail starts from that cell on each. A lending is not a case: it
    /// changes the cells' style ids, never the cursor's cell. The control: a
    /// hidden cursor changes no pixel and plans none.
    #[test]
    fn the_planned_cursor_is_the_cell_the_renderer_draws() {
        use baud_core::{BytePos, ReadLen};
        use baud_domain::{Codepoint, Col, Row};
        use baud_render::{Composition, CursorPhase, Stack, Target, Theme};
        let ws = WinSize {
            ws_col: 20,
            ws_row: 6,
        };
        let faces = [baud_font::Face::parse(crate::embedded::REGULAR.bytes).expect("the face")];
        let drawn = |t: &mut Term<'_>, composed: Option<&Composition>| -> Option<(Row, Col)> {
            let (cols, rows) = t.geometry();
            let mut r = Renderer::new(
                Stack::new(&faces).expect("a stack"),
                PxSize::of(14.0).expect("a size"),
                Theme::dark(),
                Blend::Native,
                cols,
                rows,
            )
            .expect("a renderer");
            r.fit(&*t);
            r.set_composition(composed.cloned());
            let mut paint = |phase: CursorPhase| {
                r.set_cursor_blink(phase);
                let (w, h) = r.pixel_size();
                let len = usize::try_from(w.value() * h.value()).expect("a size");
                let mut px = vec![0u32; len];
                let mut target = Target::new(&mut px, w, w, h).expect("a target");
                let seal = t.seal_writes();
                r.render(&*t, seal, &mut target, Damage::Full);
                (px, usize::try_from(w.value()).expect("a width"))
            };
            let (shown, width) = paint(CursorPhase::SHOWN);
            let (hidden, _) = paint(CursorPhase::HIDDEN);
            let changed = shown.iter().zip(&hidden).position(|(a, b)| a != b)?;
            let m = r.metrics();
            let cell = |v: usize, off: Px, side: Px| {
                (v - usize::try_from(off.value()).expect("an offset"))
                    / usize::try_from(side.value()).expect("a side")
            };
            Some((
                Row::from_index(cell(changed / width, r.grid_top(), m.cell_h)),
                Col::at_index(cell(changed % width, r.strip(), m.cell_w)),
            ))
        };
        let kana: Vec<Codepoint> = "ab"
            .chars()
            .filter_map(|c| Codepoint::new(u32::from(c)))
            .collect();
        let lines = b"line\r\n".repeat(9);
        let cases: [(&str, &[u8], bool); 6] = [
            ("whole", b"\x1b[1 q\x1b[3;5Hab", false),
            ("scroll", &lines, false),
            ("alternate screen", b"\x1b[?1049h\x1b[2;3Hq", false),
            ("status line", b"\x1b[2$~\x1b[1$}st", false),
            ("composition", b"\x1b[2;2H", true),
            ("hidden", b"\x1b[?25l", false),
        ];
        for (name, bytes, composing) in cases {
            let mut store = Owned::new(&ws);
            let mut t = Term::new(store.storage(), &ws);
            let bytes = [b"\x1b[1 q".as_slice(), bytes].concat();
            t.write_at(BytePos::ZERO, &bytes);
            t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
            let composed = match composing {
                true => crate::marked::composition(&t, &kana, &[], baud_render::Caret::End),
                false => None,
            };
            assert_eq!(composed.is_some(), composing, "{name}: composed");
            let planned = cursor_plan(&t, composed.as_ref());
            assert_eq!(drawn(&mut t, composed.as_ref()), planned.drawn(), "{name}");
            let trail = planned.followed.map(crate::effects::TrailCursor::cell);
            assert_eq!(trail, planned.drawn(), "{name}: the trail");
        }
    }
}
