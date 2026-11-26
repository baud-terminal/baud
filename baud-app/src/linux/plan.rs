//! **What a frame shows, read from the terminal before it is drawn**: the
//! cursor, the cursor guide, ReGIS's blink, the selection's highlight and
//! the write seal, each read once and handed to the frame, which reads
//! none of them from the terminal itself.

use baud_core::{CursorStyle, Term};
use baud_domain::{Col, CursorGuide, RegisBlink, Row, Seal};
use baud_render::{Composition, Highlight};

/// Whether a cursor is on the screen being drawn: the live screen's while
/// `DECTCEM` shows it, or the status line's while DECSASD selects the line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Shown {
    Yes,
    No,
}

/// **The cursor a frame draws**: the cell it is drawn in, its style, whether
/// one is on the screen, and the cursor as the caret compares it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct CursorPlan {
    /// The cell the cursor is drawn in: the status line's row while
    /// DECSASD selects the line, else the input method's caret in its
    /// composition while one is drawn at the cursor, else the cursor's.
    pub(crate) at: (Row, Col),
    pub(crate) style: CursorStyle,
    pub(crate) shown: Shown,
    pub(crate) presented: crate::caret::PresentedCursor,
}

impl CursorPlan {
    /// The cursor a frame of `term` drawn now draws, `composed` the
    /// composition drawn at its cursor, if one is.
    pub(crate) fn of(term: &Term<'_>, composed: Option<&Composition>) -> CursorPlan {
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
        CursorPlan {
            at,
            style: term.cursor_style(),
            shown,
            presented: crate::caret::PresentedCursor::of(term),
        }
    }

    /// The cell the cursor is drawn in, `None` where none is on the screen.
    pub(crate) fn drawn(self) -> Option<(Row, Col)> {
        match self.shown {
            Shown::Yes => Some(self.at),
            Shown::No => None,
        }
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
    /// The selection's share of the window, or `?1001`'s region.
    pub(crate) highlight: Option<Highlight>,
    /// The write generation this frame closed, before its first read.
    pub(crate) seal: Seal,
}

#[cfg(test)]
mod tests {
    use super::*;
    use baud_core::Owned;
    use baud_domain::WinSize;
    use baud_font::Px;
    use baud_render::{Blend, Damage, PxSize, Renderer};

    /// **The planned cursor is the cell the renderer draws it in**, on a
    /// whole frame, after a scroll, on the alternate screen, on DEC's
    /// status line and at an input method's composition: the cell is read
    /// back from the pixels a shown phase changes against a hidden one. The
    /// trail follows the screen's own cursor, which is the planned cell but
    /// on the status line and under a composition. The control: a hidden
    /// cursor changes no pixel and plans none.
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
        let cases: [(&str, &[u8], bool, bool); 6] = [
            ("whole", b"\x1b[1 q\x1b[3;5Hab", false, true),
            ("scroll", &lines, false, true),
            ("alternate screen", b"\x1b[?1049h\x1b[2;3Hq", false, true),
            ("status line", b"\x1b[2$~\x1b[1$}st", false, false),
            ("composition", b"\x1b[2;2H", true, false),
            ("hidden", b"\x1b[?25l", false, true),
        ];
        for (name, bytes, composing, follows) in cases {
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
            let planned = CursorPlan::of(&t, composed.as_ref());
            assert_eq!(drawn(&mut t, composed.as_ref()), planned.drawn(), "{name}");
            let trail = crate::effects::trail_cursor(&t).map(crate::effects::TrailCursor::cell);
            match follows {
                true => assert_eq!(trail, planned.drawn(), "{name}: the trail"),
                false => assert_ne!(trail, planned.drawn(), "{name}: the trail"),
            }
        }
    }
}
