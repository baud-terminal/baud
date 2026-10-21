//! **An input method's composition, for both drivers**: the text being
//! composed, drawn underlined at the cursor of the live screen until it is
//! committed or cancelled, and the cell the input method's candidate
//! window opens beside. The macOS driver hears it from AppKit's
//! `setMarkedText:` and answers `firstRectForCharacterRange:`; the Linux
//! driver from `text-input-v3`'s `preedit_string`, and it sends
//! `set_cursor_rectangle` (`baud_present_wayland::ime`). One rule here, so
//! a composition is drawn the same on both systems; only the Linux one is
//! handed hints, `text-input-v3`'s `preedit_hint`s.

use baud_core::Term;
use baud_domain::Codepoint;
use baud_font::Px;
use baud_render::{Composition, Hints};

/// The composition's codepoints, from the text an input method hands over.
pub fn codepoints(text: &str) -> Vec<Codepoint> {
    text.chars()
        .filter_map(|c| Codepoint::new(u32::from(c)))
        .collect()
}

/// **The hints on each codepoint of `text`**, from an input method's
/// hints over its bytes `start..end`: each on every codepoint whose first
/// byte is in its range, overlapping ones combined. A range that ends past
/// the text, is reversed, or starts or ends inside a character -- "Indices
/// must not point to middle bytes inside a code point" (`[[source:
/// wp-text-input-unstable-v3]]`) -- is dropped; with none left there are
/// no hints, and the composition is drawn as one that had none. Only the
/// Linux driver is handed any.
#[cfg(any(target_os = "linux", test))]
pub fn hints(text: &str, ranges: &[(u32, u32, Hints)]) -> Vec<Hints> {
    let at = |i: u32| {
        usize::try_from(i)
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

/// **The composition the renderer draws**, at the cursor of the live
/// view, with its `hints` ([`hints`]): none while nothing is composed,
/// and none while the window is over the history, where the cursor is
/// not.
pub fn composition(
    term: &Term<'_>,
    composing: &[Codepoint],
    hints: &[Hints],
) -> Option<Composition> {
    (!composing.is_empty() && term.window_back().index() == 0).then(|| {
        let (row, col) = term.cursor();
        Composition {
            row,
            col,
            text: composing.to_vec(),
            ambiguous: term.ambiguous_width(),
            hints: hints.to_vec(),
        }
    })
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

#[cfg(test)]
mod tests {
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
        assert_eq!(composition(&t, &[], &[]), None);
        feed(&mut t, b"ab\r\ncd");
        let kana = codepoints("かな");
        let marks = [Hints::SELECTION, Hints::WHOLE];
        let c = composition(&t, &kana, &marks).expect("composed");
        assert_eq!((c.row.index(), c.col.index()), (1, 2));
        assert_eq!(c.text, kana);
        assert_eq!(c.hints, marks);
        assert_eq!(c.ambiguous, t.ambiguous_width());
        for _ in 0..8 {
            feed(&mut t, b"\r\nline");
        }
        t.set_window_back(Rows::of_count(2));
        assert!(
            t.window_back().index() > 0,
            "the window is over the history"
        );
        assert_eq!(composition(&t, &kana, &[]), None);
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
            hints(text, &[(0, 3, s), (0, 7, w), (6, 7, p)]),
            [s.with(w), w, w.with(p)]
        );
        assert_eq!(
            hints(text, &[(3, 6, s)]),
            [Hints::NONE, s, Hints::NONE],
            "text no hint covers is unhinted"
        );
        for bad in [(1, 3), (0, 4), (0, 8), (6, 3)] {
            assert_eq!(
                hints(text, &[(bad.0, bad.1, s), (6, 7, w)]),
                [Hints::NONE, Hints::NONE, w],
                "{bad:?} dropped"
            );
            assert_eq!(hints(text, &[(bad.0, bad.1, s)]), [], "{bad:?}: none");
        }
        assert_eq!(hints(text, &[]), []);
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

    /// **Both drivers draw through this**: the macOS driver's frame and
    /// its `marked_text`, and the Linux driver's frame and its preedit
    /// event, so a composition looks the same on both.
    #[test]
    fn both_drivers_draw_the_composition_through_this_rule() {
        let app = include_str!("app.rs");
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
            assert!(
                src.contains("crate::marked::cursor_cell("),
                "{name} places it"
            );
        }
        assert!(
            linux.contains("Event::Preedit("),
            "linux.rs hears the preedit"
        );
        assert!(
            linux.contains("set_text_cursor("),
            "linux.rs sends the cursor"
        );
    }
}
