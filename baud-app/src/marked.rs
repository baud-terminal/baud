//! **An input method's composition, for both drivers**: the text being
//! composed, drawn underlined at the cursor of the live screen until it is
//! committed or cancelled, and where the input method's candidate
//! window opens. The macOS driver hears it from AppKit's
//! `setMarkedText:` and answers `firstRectForCharacterRange:`; the Linux
//! driver from `text-input-v3`'s `preedit_string`, and it sends
//! `set_cursor_rectangle` (`baud_present_wayland::ime`). One rule here, so
//! a composition is drawn the same on both systems; only the Linux one is
//! handed hints, `text-input-v3`'s `preedit_hint`s, and the input method's
//! cursor in it, `preedit_string`'s `cursor_begin` and `cursor_end`
//! ([`caret`]). AppKit hands `setMarkedText:` a selected range too, which
//! the macOS window crate does not pass on yet, so there the cursor is
//! after the composition.

use baud_core::Term;
use baud_domain::Codepoint;
use baud_font::Px;
use baud_render::{Caret, Composition, Hints};

/// The composition's codepoints, from the text an input method hands over.
pub fn codepoints(text: &str) -> Vec<Codepoint> {
    text.chars()
        .filter_map(|c| Codepoint::new(u32::from(c)))
        .collect()
}

/// A byte offset into the composition's text, as an input method's hint
/// counts it.
#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct HintAt(pub u32);

/// The input method's cursor in the composition: a byte offset into its
/// text, or -1.
#[cfg(any(target_os = "linux", test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct CaretAt(pub i32);

/// **The hints on each codepoint of `text`**, from an input method's
/// hints over its bytes `start..end`: each on every codepoint whose first
/// byte is in its range, overlapping ones combined. A range that ends past
/// the text, is reversed, or starts or ends inside a character -- "Indices
/// must not point to middle bytes inside a code point" (`[[source:
/// wp-text-input-unstable-v3]]`) -- is dropped; with none left there are
/// no hints, and the composition is drawn as one that had none. Only the
/// Linux driver is handed any.
#[cfg(any(target_os = "linux", test))]
pub fn hints(text: &str, ranges: &[(HintAt, HintAt, Hints)]) -> Vec<Hints> {
    let at = |i: HintAt| {
        usize::try_from(i.0)
            .ok()
            .filter(|&i| text.is_char_boundary(i))
    };
    let mut out = vec![Hints::NONE; text.chars().count()];
    let mut any = false;
    for &(start, end, h) in ranges {
        let (Some(start), Some(end)) = (at(start), at(end)) else {
            continue;
        };
        if start > end {
            continue;
        }
        any = true;
        for (slot, (b, _)) in out.iter_mut().zip(text.char_indices()) {
            if (start..end).contains(&b) {
                *slot = slot.with(h);
            }
        }
    }
    match any {
        true => out,
        false => Vec::new(),
    }
}

/// **Where the input method puts the cursor in `text`**, from
/// `preedit_string`'s `cursor_begin` and `cursor_end`, "counted in bytes"
/// (`[[source: wp-text-input-unstable-v3]]`), as the codepoints of the
/// composition: both -1 hides it -- "Cursor should be hidden when both are
/// equal to -1"; equal, a cursor before the character starting there, or
/// after the last at the text's length; apart, the characters between
/// drawn as selected text, the protocol's "text highlight". A value that
/// is past the text, inside a character -- "Indices must not point to
/// middle bytes inside a code point" -- or negative but not both -1, or a
/// reversed pair, says nothing, and the cursor is after the composition.
#[cfg(any(target_os = "linux", test))]
pub fn caret(text: &str, cursor: (CaretAt, CaretAt)) -> Caret {
    let cp = |b: CaretAt| {
        usize::try_from(b.0)
            .ok()
            .filter(|&b| text.is_char_boundary(b))
            .map(|b| text[..b].chars().count())
    };
    match cursor {
        (CaretAt(-1), CaretAt(-1)) => Caret::Hidden,
        (begin, end) => match (cp(begin), cp(end)) {
            (Some(b), Some(e)) if b == e => Caret::At(b),
            (Some(b), Some(e)) if b < e => Caret::Range(b, e),
            _ => Caret::End,
        },
    }
}

/// **The composition the renderer draws**, at the cursor of the live
/// view, with its `hints` ([`hints`]) and the input method's cursor in it
/// ([`caret`]): none while nothing is composed, and none while the window
/// is over the history, where the cursor is not.
pub fn composition(
    term: &Term<'_>,
    composing: &[Codepoint],
    hints: &[Hints],
    caret: Caret,
) -> Option<Composition> {
    (!composing.is_empty() && term.window_back().index() == 0)
        .then(|| Composition::at_cursor(term, composing, hints, caret))
}

/// **The cursor's cell**, as `[x, y, width, height]` in pixels from the
/// grid's top-left corner, for a cell `w` by `h` pixels: where the
/// candidate window is asked to open.
pub fn cursor_cell(term: &Term<'_>, w: Px, h: Px) -> [Px; 4] {
    let (r, c) = term.cursor();
    let at = |i: usize, size: Px| {
        Px::of(
            u32::try_from(i)
                .unwrap_or(u32::MAX)
                .saturating_mul(size.value()),
        )
    };
    [at(c.index(), w), at(r.index(), h), w, h]
}

/// **The find field's caret cell**, as [`cursor_cell`] gives the cursor's:
/// where the candidate window opens while the field takes the keys.
pub fn field_cell(field: &baud_render::Field, w: Px, h: Px) -> [Px; 4] {
    let at = |i: usize, size: Px| {
        Px::of(
            u32::try_from(i)
                .unwrap_or(u32::MAX)
                .saturating_mul(size.value()),
        )
    };
    [at(field.caret.index(), w), at(field.row.index(), h), w, h]
}

/// **Where the Linux input method's candidate window is asked to open**,
/// as `[x, y, width, height]` in pixels from the grid's top-left corner:
/// "an area around the cursor" (`[[source: wp-text-input-unstable-v3]]`),
/// the cursor being the input method's own inside the composition, as
/// baud draws it -- a caret's cell, or a range's cells
/// ([`Composition::caret_cells`]), at the doubled size on a double-width
/// line. With no composition, the terminal cursor's cell
/// ([`cursor_cell`]). `None` while the input method hides its cursor: the
/// last rectangle stands, as foot leaves it (`[[source:
/// foot-2705e36-render]]` 1839-1872).
#[cfg(any(target_os = "linux", test))]
pub fn candidate_rect(
    term: &Term<'_>,
    composition: Option<&Composition>,
    w: Px,
    h: Px,
) -> Option<[Px; 4]> {
    let Some(c) = composition else {
        return Some(cursor_cell(term, w, h));
    };
    let view = term.view(baud_domain::Rows::ZERO);
    let cols = c.width_on(&view);
    let (row, col, n) = c.caret_cells(cols)?;
    let doubled = row.index() < view.rows().index()
        && view.rendition(row) != baud_domain::LineRendition::Single;
    let cw = match doubled {
        true => w.value().saturating_mul(2),
        false => w.value(),
    };
    let at =
        |i: usize, size: u32| Px::of(u32::try_from(i).unwrap_or(u32::MAX).saturating_mul(size));
    Some([
        at(col.index(), cw),
        at(row.index(), h.value()),
        at(n.index(), cw),
        h,
    ])
}

/// **A rectangle of the grid, in the frame's pixels**: `rect`, grid
/// pixels, moved by `origin`, where the grid begins -- past the exit-status
/// strip, and on macOS the padding too -- so an input method's window opens
/// at the cursor it is for.
pub fn in_frame(rect: [Px; 4], origin: (Px, Px)) -> [Px; 4] {
    let [x, y, w, h] = rect;
    [
        Px::of(x.value().saturating_add(origin.0.value())),
        Px::of(y.value().saturating_add(origin.1.value())),
        w,
        h,
    ]
}

#[cfg(test)]
mod tests {

    /// **The candidate window opens where the grid's cell is**: a grid
    /// rectangle moved by where the grid begins, the strip and the
    /// padding, its size kept.
    #[test]
    fn a_grid_rectangle_moves_by_where_the_grid_begins() {
        let r = [Px::of(30), Px::of(40), Px::of(10), Px::of(20)];
        assert_eq!(in_frame(r, (Px::of(0), Px::of(0))), r);
        assert_eq!(
            in_frame(r, (Px::of(17), Px::of(8))),
            [Px::of(47), Px::of(48), Px::of(10), Px::of(20)]
        );
    }
    use super::*;
    use baud_core::{BytePos, Owned, ReadLen, WinSize};
    use baud_domain::Rows;

    const WS: WinSize = WinSize {
        ws_row: 5,
        ws_col: 20,
    };

    fn feed(t: &mut Term<'_>, b: &[u8]) {
        t.write_at(BytePos::ZERO, b);
        t.feed(BytePos::ZERO, ReadLen::from_index(b.len()));
    }

    /// **A composition is drawn at the cursor, and only on the live
    /// screen**: none while nothing is composed; at the cursor's cell
    /// with the text and the core's ambiguous width; none once the window
    /// is over the history.
    #[test]
    fn a_composition_is_drawn_at_the_cursor_of_the_live_screen() {
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        assert_eq!(composition(&t, &[], &[], Caret::End), None);
        feed(&mut t, b"ab\r\ncd");
        let kana = codepoints("かな");
        let marks = [Hints::SELECTION, Hints::WHOLE];
        let c = composition(&t, &kana, &marks, Caret::At(1)).expect("composed");
        assert_eq!((c.row.index(), c.col.index()), (1, 2));
        assert_eq!(c.text, kana);
        assert_eq!(c.hints, marks);
        assert_eq!(c.measure, baud_render::Measure::of(&t));
        assert_eq!(c.caret, Caret::At(1), "the input method's cursor");
        for _ in 0..8 {
            feed(&mut t, b"\r\nline");
        }
        t.set_window_back(Rows::of_count(2));
        assert!(
            t.window_back().index() > 0,
            "the window is over the history"
        );
        assert_eq!(composition(&t, &kana, &[], Caret::End), None);
    }

    /// **An input method's byte ranges land on the codepoints they
    /// cover**: overlapping ones combined, a range that ends inside a
    /// character, past the text or before it starts dropped, and none
    /// left meaning no hints at all.
    #[test]
    fn hints_over_bytes_land_on_codepoints() {
        let (s, w, p) = (Hints::SELECTION, Hints::WHOLE, Hints::PREDICTION);
        // `か` and `な` are three bytes each, `a` one.
        let text = "かなa";
        assert_eq!(
            hints(
                text,
                &[
                    (HintAt(0), HintAt(3), s),
                    (HintAt(0), HintAt(7), w),
                    (HintAt(6), HintAt(7), p)
                ]
            ),
            [s.with(w), w, w.with(p)]
        );
        assert_eq!(
            hints(text, &[(HintAt(3), HintAt(6), s)]),
            [Hints::NONE, s, Hints::NONE],
            "text no hint covers is unhinted"
        );
        for bad in [(1, 3), (0, 4), (0, 8), (6, 3)] {
            assert_eq!(
                hints(
                    text,
                    &[(HintAt(bad.0), HintAt(bad.1), s), (HintAt(6), HintAt(7), w)]
                ),
                [Hints::NONE, Hints::NONE, w],
                "{bad:?} dropped"
            );
            assert_eq!(
                hints(text, &[(HintAt(bad.0), HintAt(bad.1), s)]),
                [],
                "{bad:?}: none"
            );
        }
        assert_eq!(hints(text, &[]), []);
    }

    /// **The input method's cursor lands on the codepoints its bytes
    /// count**: both -1 hidden; equal offsets a cursor before the character
    /// starting there, after the last at the length; apart, a range; and a
    /// value inside a character, past the text, negative alone or reversed
    /// says nothing. Perturbations: read the offsets as codepoints -- the
    /// three-byte kana's assertion fails; take -1 alone as hidden -- its
    /// assertion fails.
    #[test]
    fn the_input_methods_cursor_lands_on_codepoints() {
        // `か` and `な` are three bytes each, `a` one.
        let text = "かなa";
        assert_eq!(caret(text, (CaretAt(-1), CaretAt(-1))), Caret::Hidden);
        assert_eq!(caret(text, (CaretAt(0), CaretAt(0))), Caret::At(0));
        assert_eq!(caret(text, (CaretAt(3), CaretAt(3))), Caret::At(1));
        assert_eq!(
            caret(text, (CaretAt(7), CaretAt(7))),
            Caret::At(3),
            "after the last"
        );
        assert_eq!(caret(text, (CaretAt(3), CaretAt(7))), Caret::Range(1, 3));
        for bad in [(1, 1), (8, 8), (-1, 3), (3, -1), (6, 3), (-2, -2)] {
            assert_eq!(
                caret(text, (CaretAt(bad.0), CaretAt(bad.1))),
                Caret::End,
                "{bad:?}"
            );
        }
        assert_eq!(caret("", (CaretAt(0), CaretAt(0))), Caret::At(0));
    }

    /// The candidate window's cell is the cursor's, in pixels.
    #[test]
    fn the_candidate_window_opens_at_the_cursors_cell() {
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        feed(&mut t, b"\r\n\r\nabc");
        let px = Px::of;
        assert_eq!(
            cursor_cell(&t, px(9), px(18)),
            [px(27), px(36), px(9), px(18)]
        );
    }

    /// **The candidate window opens at the input method's cursor**: at a
    /// caret, its cell, both of a wide character's; past the clusters, the
    /// cell after; over a range, the range's cells; hidden, nothing new,
    /// so the last stands; with no composition, the terminal cursor's
    /// cell; on a double-width line, at the doubled cells. Each assertion
    /// with a composition fails for the rule before, its first cell.
    #[test]
    fn the_candidate_window_opens_at_the_input_methods_cursor() {
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        feed(&mut t, b"\r\nab");
        let px = Px::of;
        let (w, h) = (px(9), px(18));
        // `か` and `な` are wide, `a` narrow: cells 2-3, 4-5 and 6.
        let text = codepoints("かなa");
        let rect = |t: &Term<'_>, caret| {
            let c = composition(t, &text, &[], caret).expect("composed");
            candidate_rect(t, Some(&c), w, h)
        };
        assert_eq!(rect(&t, Caret::At(1)), Some([px(36), px(18), px(18), h]));
        assert_eq!(rect(&t, Caret::At(2)), Some([px(54), px(18), px(9), h]));
        assert_eq!(rect(&t, Caret::End), Some([px(63), px(18), px(9), h]));
        assert_eq!(
            rect(&t, Caret::Range(1, 3)),
            Some([px(36), px(18), px(27), h])
        );
        assert_eq!(rect(&t, Caret::Hidden), None);
        assert_eq!(
            candidate_rect(&t, None, w, h),
            Some([px(18), px(18), px(9), h]),
            "no composition: the cursor's cell"
        );
        feed(&mut t, b"\x1b#6");
        assert_eq!(
            rect(&t, Caret::At(1)),
            Some([px(72), px(18), px(36), h]),
            "a double-width line doubles the cells"
        );
    }

    /// **Both drivers draw through this**: the macOS driver's frame and
    /// its `marked_text`, and the Linux driver's frame and its preedit
    /// event, so a composition looks the same on both -- and the Linux
    /// driver hands the input method's hints to the find field too, so a
    /// composition there is drawn in them as at the cursor.
    #[test]
    fn both_drivers_draw_the_composition_through_this_rule() {
        let app = crate::app_source::app_source();
        let linux = include_str!("linux.rs");
        for (name, src) in [("app.rs", app), ("linux.rs", linux)] {
            assert!(
                src.contains("crate::marked::composition("),
                "{name} draws it"
            );
            assert!(
                src.contains("crate::marked::codepoints("),
                "{name} reads it"
            );
        }
        assert!(
            app.contains("crate::marked::cursor_cell("),
            "app.rs places it"
        );
        assert!(
            linux.contains("crate::marked::candidate_rect("),
            "linux.rs places it at the input method's cursor"
        );
        assert!(
            linux.contains("Event::Preedit("),
            "linux.rs hears the preedit"
        );
        assert!(
            linux.contains("set_text_cursor("),
            "linux.rs sends the cursor"
        );
        let field = linux
            .split_once("f.look_typing(")
            .and_then(|(_, rest)| rest.split_once("self.composing_caret"))
            .map(|(args, _)| args);
        assert!(
            field.is_some_and(|a| a.contains("&self.composing_hints")),
            "linux.rs draws the field's composition in its hints: {field:?}"
        );
    }
}
