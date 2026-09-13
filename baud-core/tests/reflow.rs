//! **A change of width reflows the primary's lines, in place, once.** A
//! line is the rows the wrap flag joins; a narrower screen re-cuts it into
//! more rows and a wider one joins them back, on the screen and through the
//! history alike, and a line that ended at a newline stays its own. The
//! alternate screen is not reflowed -- a full-screen program redraws on
//! `SIGWINCH` -- and keeps the cell-addressed contract a resize had before.

mod support;

use baud_core::*;

fn text_of(cells: &[PackedCell]) -> String {
    cells
        .iter()
        .map(|c| match c.content.as_codepoint() {
            Some(cp) => char::from_u32(cp.value()).unwrap_or('?'),
            None => ' ',
        })
        .collect()
}

fn row_text(t: &Term<'_>, y: usize) -> String {
    text_of(t.row(Row::from_index(y))).trim_end().to_string()
}

fn wrapped(t: &Term<'_>, y: usize) -> Wrap {
    t.viewport().wrapped(Row::from_index(y))
}

fn back(t: &Term<'_>, n: usize) -> (String, Wrap) {
    let sb = t.scrollback();
    let (c, w) = sb.line(Back::of(n)).expect("a retained line");
    (text_of(c), w)
}

fn resize(t: &mut Term<'_>, cols: u16, rows: u16) {
    assert_eq!(
        t.resize(&WinSize {
            ws_row: rows,
            ws_col: cols
        }),
        Resized::Ok
    );
}

fn digits(n: usize) -> String {
    (0..n).map(|i| char::from(b"0123456789"[i % 10])).collect()
}

/// **A long line re-cuts to the new width and joins back to the old one**,
/// and the cursor stays on its cell throughout.
#[test]
fn narrowing_rewraps_a_long_line_and_widening_joins_it_again() {
    support::with_corpus(|t| {
        let line = digits(150);
        support::feed(t, line.as_bytes());
        support::feed(t, b"\r\nnext");
        assert_eq!(t.cursor(), (Row::from_index(1), Col::at_index(4)));

        resize(t, 100, 50);
        assert_eq!(row_text(t, 0), line[..100], "the first hundred");
        assert_eq!(wrapped(t, 0), Wrap::Continues);
        assert_eq!(row_text(t, 1), line[100..], "the last fifty");
        assert_eq!(wrapped(t, 1), Wrap::Ended);
        assert_eq!(row_text(t, 2), "next");
        assert_eq!(
            t.cursor(),
            (Row::from_index(2), Col::at_index(4)),
            "the cursor's cell moved down a row with its line"
        );

        resize(t, 200, 50);
        assert_eq!(row_text(t, 0), line, "joined again");
        assert_eq!(wrapped(t, 0), Wrap::Ended);
        assert_eq!(row_text(t, 1), "next");
        assert_eq!(row_text(t, 2), "");
        assert_eq!(t.cursor(), (Row::from_index(1), Col::at_index(4)));
    });
}

/// **A row that is exactly full and then ended by a newline is not joined
/// with the row below it when the screen widens.** The pending-wrap
/// position is not a wrap; only a character written past the edge is.
#[test]
fn a_line_that_ended_at_a_newline_is_not_joined() {
    support::with_corpus(|t| {
        resize(t, 100, 50);
        let full = digits(100);
        support::feed(t, full.as_bytes());
        support::feed(t, b"\r\nbbb");
        assert_eq!(wrapped(t, 0), Wrap::Ended, "full, and ended by the newline");
        resize(t, 200, 50);
        assert_eq!(row_text(t, 0), full);
        assert_eq!(row_text(t, 1), "bbb");
        assert_eq!(t.cursor(), (Row::from_index(1), Col::at_index(3)));
    });
}

/// **The history reflows with the screen**, newest nearest, and the screen
/// stays anchored to its bottom row so the cursor keeps its distance from
/// the edge the shell writes at.
#[test]
fn the_history_reflows_with_the_screen() {
    support::with_corpus(|t| {
        // Sixty lines of 150 characters, each numbered in its first five.
        let lines: Vec<String> = (0..60).map(|i| format!("{i:05}") + &digits(145)).collect();
        for l in &lines {
            support::feed(t, l.as_bytes());
            support::feed(t, b"\r\n");
        }
        assert_eq!(
            t.scrollback().len(),
            11,
            "60 lines and the cursor's row on 50"
        );
        assert_eq!(t.cursor(), (Row::from_index(49), Col::ZERO));

        resize(t, 100, 50);
        // Each line is two rows now: the cursor's blank row is the screen's
        // last, the 24 lines above it fill the other 49 rows (one of them
        // split by the top), and everything older is behind the window.
        assert_eq!(t.cursor(), (Row::from_index(49), Col::ZERO));
        assert_eq!(row_text(t, 48), lines[59][100..]);
        assert_eq!(row_text(t, 47), lines[59][..100]);
        assert_eq!(wrapped(t, 47), Wrap::Continues);
        assert_eq!(
            row_text(t, 0),
            lines[35][100..],
            "the top row is a line's tail"
        );
        assert_eq!(back(t, 0), (lines[35][..100].to_string(), Wrap::Continues));
        assert_eq!(back(t, 1), (lines[34][100..].to_string(), Wrap::Ended));
        assert_eq!(back(t, 2), (lines[34][..100].to_string(), Wrap::Continues));
        assert_eq!(
            t.scrollback().len(),
            71,
            "35 lines of two rows, and one head"
        );

        resize(t, 200, 50);
        assert_eq!(t.cursor(), (Row::from_index(49), Col::ZERO));
        for y in 0..49 {
            assert_eq!(
                row_text(t, y),
                lines[y + 11],
                "row {y} after the round trip"
            );
            assert_eq!(wrapped(t, y), Wrap::Ended);
        }
        assert_eq!(t.scrollback().len(), 11);
        assert_eq!(back(t, 10), (lines[0].clone(), Wrap::Ended));
    });
}

/// **A wide character that would straddle the new edge moves whole to the
/// next row**, and the column it leaves behind is blank.
#[test]
fn a_wide_character_does_not_straddle_the_new_edge() {
    support::with_corpus(|t| {
        let head = "a".repeat(99);
        support::feed(t, head.as_bytes());
        support::feed(t, "漢字end".as_bytes());
        resize(t, 100, 50);
        let r0 = t.row(Row::from_index(0));
        assert_eq!(text_of(&r0[..99]), head);
        assert!(
            r0[99].content.is_empty(),
            "the column the wide character could not take"
        );
        assert_eq!(wrapped(t, 0), Wrap::Continues);
        let r1 = t.row(Row::from_index(1));
        assert_eq!(
            r1[0]
                .content
                .as_codepoint()
                .and_then(|c| char::from_u32(c.value())),
            Some('漢')
        );
        assert!(r1[1].content.is_wide_tail());
        assert_eq!(text_of(&r1[4..7]), "end");
        assert_eq!(t.cursor(), (Row::from_index(1), Col::at_index(7)));
    });
}

/// **Trailing blanks are not content**: a short line narrowed is still one
/// row, not the width's worth of blanks re-cut into several.
#[test]
fn trailing_blanks_are_not_carried_across_a_rewrap() {
    support::with_corpus(|t| {
        support::feed(t, b"ab\r\ncd");
        resize(t, 100, 50);
        assert_eq!(row_text(t, 0), "ab");
        assert_eq!(row_text(t, 1), "cd");
        assert_eq!(t.cursor(), (Row::from_index(1), Col::at_index(2)));
        assert!(t.scrollback().is_empty());
    });
}

/// **A styled blank past the text survives the re-cut**: the tail of a
/// line keeps the fill the erase left, so a red-cleared line is red to the
/// new edge and not the default past the old text.
#[test]
fn the_tail_fill_survives_a_rewrap() {
    support::with_corpus(|t| {
        support::feed(t, b"ab\x1b[41m\x1b[K\x1b[0m");
        let red = t.style_of(t.row(Row::from_index(0))[50]).bg();
        resize(t, 100, 50);
        assert_eq!(row_text(t, 0), "ab");
        assert_eq!(t.style_of(t.row(Row::from_index(0))[50]).bg(), red);
        assert_eq!(t.style_of(t.row(Row::from_index(0))[99]).bg(), red);
    });
}

/// **The alternate screen keeps the cell-addressed contract**: a program
/// that painted a full-screen frame is told the size changed and redraws,
/// and until it does its cells stay where they were, hidden past the edge
/// and shown again when it widens.
#[test]
fn the_alternate_screen_is_not_reflowed() {
    support::with_corpus(|t| {
        support::feed(t, b"\x1b[?1049h");
        let line = digits(150);
        support::feed(t, line.as_bytes());
        resize(t, 100, 50);
        assert_eq!(row_text(t, 0), line[..100]);
        assert_eq!(row_text(t, 1), "", "the tail is hidden, not re-cut");
        resize(t, 200, 50);
        assert_eq!(row_text(t, 0), line);
        support::feed(t, b"\x1b[?1049l");
        assert_eq!(row_text(t, 0), "", "the primary was blank throughout");
    });
}

/// **A ring that cannot hold the re-cut history drops whole lines from the
/// oldest end**, so the oldest retained line always starts at its start.
#[test]
fn a_ring_that_cannot_hold_the_rewrapped_history_drops_the_oldest_lines() {
    support::with_corpus(|t| {
        let n = 1100;
        for i in 0..n {
            support::feed(t, (format!("{i:05}") + &digits(145)).as_bytes());
            support::feed(t, b"\r\n");
        }
        let depth = t.scrollback().len();
        assert_eq!(depth, n + 1 - 50, "every line but the screen's is retained");
        resize(t, 100, 50);
        let len = t.scrollback().len();
        assert!(len >= 2000 - 1 && len <= 2000, "the ring is full: {len}");
        let (oldest, w) = back(t, len - 1);
        assert_eq!(
            w,
            Wrap::Continues,
            "the oldest retained row is a line's head"
        );
        assert!(
            oldest.starts_with(|c: char| c.is_ascii_digit()),
            "and it starts where the line starts: {oldest:?}"
        );
        // The window holds the cursor's row and the 49 rows above it: 24
        // whole lines and the tail of line 1075, whose head is the newest
        // retained line.
        let (newest, w) = back(t, 0);
        assert_eq!(newest, (format!("01075") + &digits(145))[..100]);
        assert_eq!(w, Wrap::Continues);
        assert_eq!(row_text(t, 0), digits(145)[95..]);
        assert_eq!(row_text(t, 47), (format!("01099") + &digits(145))[..100]);
    });
}

/// **A ring that is already full reflows too**, and keeps as many whole
/// lines as the ring holds: the destination wraps onto the rows the
/// dropped lines held, which is the case the in-place order has to get
/// right when there is no free row at all.
#[test]
fn a_full_ring_narrows_and_keeps_what_fits() {
    support::with_corpus(|t| {
        for i in 0..2100 {
            support::feed(t, (format!("{i:05}") + &digits(145)).as_bytes());
            support::feed(t, b"\r\n");
        }
        assert_eq!(t.scrollback().len(), 2000, "the ring is full");
        resize(t, 100, 50);
        let len = t.scrollback().len();
        assert!(len >= 2000 - 1 && len <= 2000, "still full: {len}");
        let (oldest, w) = back(t, len - 1);
        assert_eq!(w, Wrap::Continues);
        assert!(
            oldest.starts_with(|c: char| c.is_ascii_digit()),
            "{oldest:?}"
        );
        assert_eq!(row_text(t, 47), (format!("02099") + &digits(145))[..100]);
        assert_eq!(row_text(t, 48), digits(145)[95..]);
        assert_eq!(t.cursor(), (Row::from_index(49), Col::ZERO));
        // What the narrowing dropped is gone: it kept lines 1076 to 2099
        // and the cursor's row, 2,049 rows, which join back into 1,025
        // rows, 975 of them behind the screen.
        resize(t, 200, 50);
        assert_eq!(t.scrollback().len(), 975);
        assert_eq!(row_text(t, 48), format!("02099") + &digits(145));
        let (oldest, w) = back(t, 974);
        assert_eq!(w, Wrap::Ended);
        assert_eq!(oldest, format!("01076") + &digits(145), "a whole line");
    });
}
