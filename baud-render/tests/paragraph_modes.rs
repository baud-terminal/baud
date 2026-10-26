//! **What terminal-wg's paragraph modes draw**: `?2501`, direction
//! autodetection (`[[source: terminal-wg-bidi-basic-modes]]`), and `?2500`,
//! box mirroring (`[[source: terminal-wg-bidi-misc]]`), each a property of
//! a paragraph the core keeps on its rows (`baud-core/tests/paragraph_modes.rs`).
//!
//! Every expectation is a visual order worked by hand from UAX #9's rules;
//! `.` is an erased cell.

use baud_core::{BidiDirection, Owned, Term};
use baud_domain::{BytePos, Col, PackedCell, ReadLen, Row, Rows, WinSize};
use baud_render::{CursorSide, Reorder};

const WS: WinSize = WinSize {
    ws_row: 4,
    ws_col: 12,
};

const HEBREW: &str = "\u{5D0}\u{5D1}\u{5D2}!";

fn feed(t: &mut Term<'_>, bytes: &str) {
    t.write_at(BytePos::ZERO, bytes.as_bytes());
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
}

fn row_of(t: &Term<'_>, o: &mut Reorder, r: usize) -> String {
    o.lay_out(t, Rows::ZERO);
    let vp = t.view(Rows::ZERO);
    let row = Row::from_index(r);
    let mut out = vec![PackedCell::EMPTY; vp.cols().index()];
    o.orders().permute(row, vp.row(row), &mut out);
    out.iter()
        .map(|c| match c.content {
            x if x.is_empty() => '.',
            x if x.is_wide_tail() => '~',
            x => x
                .as_codepoint()
                .and_then(|cp| char::from_u32(cp.value()))
                .unwrap_or('?'),
        })
        .collect()
}

/// Every row of the screen after `bytes`, from power-on.
fn screen(bytes: &str) -> Vec<String> {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    feed(&mut t, bytes);
    let mut o = Reorder::new();
    (0..usize::from(WS.ws_row))
        .map(|r| row_of(&t, &mut o, r))
        .collect()
}

/// **With autodetection on, as at power-on, SCP 2 is a fallback**: an
/// English line stays left to right, its first strong letter L, and a line
/// with no strong letter runs right to left (terminal-wg: "including of
/// course the fallback direction of implicit paragraphs with
/// autodetection"; Jev's lean, SET_FALLBACK). Off, the path is the level
/// outright, and an unset Hebrew line runs left to right.
#[test]
fn a_path_is_the_fallback_while_autodetection_is_on() {
    let s = screen("\x1b[2 kabc!\r\n123\r\n\x1b[?2501labc!\r\n\x1b[1 k");
    assert_eq!(s[0], "abc!........", "English: first strong");
    assert_eq!(s[1], ".........123", "digits: the fallback");
    assert_eq!(s[2], "........!abc", "autodetection off: the path");
    let s = screen(&format!("\x1b[?2501l{HEBREW}"));
    assert_eq!(
        s[0], "\u{5D2}\u{5D1}\u{5D0}!........",
        "no path, no detection"
    );
    let s = screen(HEBREW);
    assert_eq!(
        s[0], "........!\u{5D2}\u{5D1}\u{5D0}",
        "the control: detected"
    );
}

/// **`?2500` draws a box drawn right to left as its counterpart**: on an
/// explicit right-to-left line (BDSM reset, SCP 2), reversed cell by cell,
/// each corner is drawn as the other side's, so the box closes
/// (`[[source: terminal-wg-bidi-misc]]`, "mirror the U+2500 .. U+257F
/// glyphs in RTL"). Without it the corners face out.
#[test]
fn box_mirroring_closes_a_box_drawn_right_to_left() {
    let boxed = "\u{250C}\u{2500}\u{2500}\u{2510}\r\n\u{2502}ab\u{2502}\r\n\u{2514}\u{2500}\u{2500}\u{2518}";
    let s = screen(&format!("\x1b[8l\x1b[?2500h\x1b[2 k{boxed}"));
    assert_eq!(s[0], "........\u{250C}\u{2500}\u{2500}\u{2510}");
    assert_eq!(s[1], "........\u{2502}ba\u{2502}");
    assert_eq!(s[2], "........\u{2514}\u{2500}\u{2500}\u{2518}");
    let s = screen(&format!("\x1b[8l\x1b[2 k{boxed}"));
    assert_eq!(
        s[0], "........\u{2510}\u{2500}\u{2500}\u{250C}",
        "the control"
    );
}

/// **A table row's rules are drawn as written under `?2500`**: baud keeps
/// them where they were written, at the left-to-right level, so none is
/// drawn right to left and none is mirrored -- L4's resolved R, which HL6
/// widens to more characters but not to characters drawn left to right
/// (Jev's lean, RESOLVED_RTL).
#[test]
fn box_mirroring_leaves_a_table_row_as_written() {
    let row = "\u{250C}\u{2500}\u{252C}\u{2500}\u{2510}\r\n\u{2502}\u{5D0}\u{5D1}\u{2502}";
    let on = screen(&format!("\x1b[?2500h\x1b[2 k{row}"));
    let off = screen(&format!("\x1b[2 k{row}"));
    assert_eq!(on, off);
    assert_eq!(on[0], "\u{250C}\u{2500}\u{252C}\u{2500}\u{2510}.......");
}

/// **The core's direction for the arrow keys is the one drawn**: on each
/// screen, `Term::paragraph_direction` at a row's last cell, past its text,
/// against the side the renderer draws a bar cursor on there, the side of
/// the paragraph's own level -- first strong, the fallback, autodetection
/// off, a wrapped paragraph's second row, a paragraph begun in the
/// history, BDSM reset, and past a table row's text, which belongs to no
/// cell and is drawn left to right where it is.
#[test]
fn the_direction_the_core_reads_is_the_one_drawn() {
    let cases = [
        (HEBREW.to_string(), 0, BidiDirection::Rtl),
        ("abc".to_string(), 0, BidiDirection::Ltr),
        ("\x1b[2 kabc".to_string(), 0, BidiDirection::Ltr),
        ("\x1b[2 k123".to_string(), 0, BidiDirection::Rtl),
        (format!("\x1b[?2501l{HEBREW}"), 0, BidiDirection::Ltr),
        ("\x1b[?2501l\x1b[2 kabc".to_string(), 0, BidiDirection::Rtl),
        (format!("{HEBREW}abcdefghijkl"), 1, BidiDirection::Rtl),
        (
            format!("{HEBREW}abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJ"),
            3,
            BidiDirection::Rtl,
        ),
        ("\x1b[8l\x1b[2 kabc".to_string(), 0, BidiDirection::Rtl),
        (
            "\u{2502}ab\u{2502}\u{5D0}\u{5D1}".to_string(),
            0,
            BidiDirection::Ltr,
        ),
        (
            "\u{2502}\u{5D0}\u{5D1}\u{2502}ab".to_string(),
            0,
            BidiDirection::Ltr,
        ),
    ];
    for (bytes, r, want) in cases {
        let mut owned = Owned::new(&WS);
        let mut t = Term::new(owned.storage(), &WS);
        feed(&mut t, &bytes);
        let mut o = Reorder::new();
        o.lay_out(&t, Rows::ZERO);
        let (row, col) = (
            Row::from_index(r),
            Col::at_index(usize::from(WS.ws_col) - 1),
        );
        let got = t.paragraph_direction(row, col);
        assert_eq!(got, want, "{bytes:?}");
        let side = match got {
            BidiDirection::Rtl => CursorSide::Right,
            BidiDirection::Ltr => CursorSide::Left,
        };
        assert_eq!(o.orders().side(row, col), side, "{bytes:?}: drawn");
    }
}
