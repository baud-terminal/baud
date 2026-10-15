//! **A change of width reflows the primary's lines, in place, once.** A
//! line is the rows the wrap flag joins; a narrower screen re-cuts it into
//! more rows and a wider one joins them back, on the screen and through the
//! history alike, and a line that ended at a newline stays its own. The
//! alternate screen is not reflowed -- a full-screen program redraws on
//! `SIGWINCH` -- and a widen blanks the columns it reveals rather than
//! showing the stale frame under the program's redraw.

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
    text_of(t.row(Row::from_index(y)).expect("a row on the grid"))
        .trim_end()
        .to_string()
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
        let r0 = t.row(Row::from_index(0)).expect("a row on the grid");
        assert_eq!(text_of(&r0[..99]), head);
        assert!(
            r0[99].content.is_empty(),
            "the column the wide character could not take"
        );
        assert_eq!(wrapped(t, 0), Wrap::Continues);
        let r1 = t.row(Row::from_index(1)).expect("a row on the grid");
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
        let red = t
            .style_of(t.row(Row::from_index(0)).expect("a row on the grid")[50])
            .bg();
        resize(t, 100, 50);
        assert_eq!(row_text(t, 0), "ab");
        assert_eq!(
            t.style_of(t.row(Row::from_index(0)).expect("a row on the grid")[50])
                .bg(),
            red
        );
        assert_eq!(
            t.style_of(t.row(Row::from_index(0)).expect("a row on the grid")[99])
                .bg(),
            red
        );
    });
}

/// **The alternate screen is cell-addressed, not reflowed, and a widen shows
/// blank rather than the old frame.** A program that painted a full-screen
/// frame is told the size changed and redraws; a narrower screen hides its
/// tail past the edge rather than re-cutting it into a second row (that is
/// the not-reflowed half). A wider screen does *not* bring the hidden tail
/// back under the program's redraw -- which lands a frame or two later -- but
/// blanks the revealed columns, so the grow shows the overlap plus empty
/// space, not two frames at once. The tail is gone; the program owns the
/// screen and repaints it.
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
        assert_eq!(
            row_text(t, 0),
            line[..100],
            "the revealed columns are blank, not the stale tail"
        );
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
        assert!(
            ((2000 - 1)..=2000).contains(&len),
            "the ring is full: {len}"
        );
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
        assert_eq!(newest, (String::from("01075") + &digits(145))[..100]);
        assert_eq!(w, Wrap::Continues);
        assert_eq!(row_text(t, 0), digits(145)[95..]);
        assert_eq!(
            row_text(t, 47),
            (String::from("01099") + &digits(145))[..100]
        );
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
        assert!(((2000 - 1)..=2000).contains(&len), "still full: {len}");
        let (oldest, w) = back(t, len - 1);
        assert_eq!(w, Wrap::Continues);
        assert!(
            oldest.starts_with(|c: char| c.is_ascii_digit()),
            "{oldest:?}"
        );
        assert_eq!(
            row_text(t, 47),
            (String::from("02099") + &digits(145))[..100]
        );
        assert_eq!(row_text(t, 48), digits(145)[95..]);
        assert_eq!(t.cursor(), (Row::from_index(49), Col::ZERO));
        // What the narrowing dropped is gone: it kept lines 1076 to 2099
        // and the cursor's row, 2,049 rows, which join back into 1,025
        // rows, 975 of them behind the screen.
        resize(t, 200, 50);
        assert_eq!(t.scrollback().len(), 975);
        assert_eq!(row_text(t, 48), String::from("02099") + &digits(145));
        let (oldest, w) = back(t, 974);
        assert_eq!(w, Wrap::Ended);
        assert_eq!(oldest, String::from("01076") + &digits(145), "a whole line");
    });
}

// **A window over the history holds its line through a re-cut.** The app
// anchored a scrolled-back window by how far the line at the window's top
// moved, and a re-cut renumbers that line by the growth of every line above
// it -- the lines above the one the window shows included, which do not move
// it at all. `Term::resize` maps the terminal's own window's first row
// through the re-cut the way an image's row is mapped (note V, V2).

/// The line at the top of a view `back` lines up.
fn viewed(t: &Term<'_>, back: Rows) -> String {
    text_of(t.view(back).row(Row::ZERO)).trim_end().to_string()
}

/// The terminal's window put `back` lines up, the screen resized, and where
/// the window is after it.
fn hold(t: &mut Term<'_>, cols: u16, rows: u16, back: Rows) -> Rows {
    t.set_window_back(back);
    assert_eq!(
        t.window_back(),
        back,
        "the control: the window is where it was put"
    );
    let r = t.resize(&WinSize {
        ws_row: rows,
        ws_col: cols,
    });
    assert_eq!(
        r,
        Resized::Ok,
        "{cols}x{rows} is inside the provisioned grid"
    );
    t.window_back()
}

/// How far back the line `want` starts, looking from the live screen.
fn find(t: &Term<'_>, want: &str) -> Rows {
    (1..=t.scrollback().len())
        .map(Rows::of_count)
        .find(|&b| viewed(t, b) == want)
        .unwrap_or_else(|| panic!("{want:?} is in the history"))
}

fn long(i: usize) -> String {
    format!("long {i} {}\r\n", "x".repeat(22))
}

fn short(i: usize) -> String {
    format!("s {i:02}\r\n")
}

/// A terminal provisioned at twenty columns and started at `cols`.
fn at<R>(cols: u16, rows: u16, f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    let ws = |c: u16| WinSize {
        ws_col: c,
        ws_row: rows,
    };
    let mut owned = Owned::new(&ws(20));
    let mut t = Term::new(owned.storage(), &ws(cols));
    f(&mut t)
}

#[test]
fn a_window_over_the_history_holds_its_line_through_a_re_cut() {
    let above: String = (0..5).map(long).chain((0..30).map(short)).collect();
    let between: String = (0..10)
        .map(short)
        .chain((0..5).map(long))
        .chain((10..30).map(short))
        .collect();
    for (stream, arm) in [
        (&between, "long lines between it and the screen"),
        (&above, "long lines above it"),
    ] {
        for (from, to) in [(20, 12), (12, 20), (20, 7)] {
            for n in [1, 4096] {
                at(from, 10, |t| {
                    for c in stream.as_bytes().chunks(n) {
                        support::feed(t, c);
                    }
                    let back = find(t, "s 05");
                    let held = hold(t, to, 10, back);
                    assert_eq!(
                        viewed(t, held),
                        "s 05",
                        "{arm}, {from} to {to} columns, read size {n}"
                    );
                });
            }
        }
    }
}

/// A live window stays live, even when the re-cut slides the window down to
/// keep the cursor on the screen -- where the window's old top row lands
/// above the new one, and mapping it would have left the live screen.
#[test]
fn a_live_window_stays_live_through_a_re_cut() {
    at(20, 10, |t| {
        let s: String = (0..20).map(long).collect();
        support::feed(t, s.as_bytes());
        let top = row_text(t, 0);
        assert_eq!(hold(t, 12, 10, Rows::ZERO), Rows::ZERO);
        assert_ne!(row_text(t, 0), top, "the control: the window slid");
        assert_eq!(hold(t, 20, 10, Rows::ZERO), Rows::ZERO);
    });
}

/// A window that a taller screen brings back to the live screen, in the
/// same resize as a re-cut, is live for the re-cut: the growth pulls two
/// rows back from the history and a window two lines up is on the live
/// screen by the time the lines are re-cut.
#[test]
fn a_window_a_growth_brings_to_the_live_screen_stays_live_through_the_re_cut() {
    let ws = |c: u16, r: u16| WinSize {
        ws_col: c,
        ws_row: r,
    };
    let mut owned = Owned::new(&ws(20, 10));
    let mut t = Term::new(owned.storage(), &ws(20, 8));
    let s: String = (0..20).map(long).collect();
    support::feed(&mut t, s.as_bytes());
    t.set_window_back(Rows::of_count(2));
    assert_eq!(t.window_back().index(), 2, "the control: two lines up");
    assert_eq!(t.resize(&ws(12, 10)), Resized::Ok);
    assert_eq!(t.window_back(), Rows::ZERO);
}

/// The re-cut drops whole lines from the oldest end when the ring cannot
/// hold them, and a window on one of those lines holds the oldest line
/// kept.
#[test]
fn a_window_whose_line_the_re_cut_dropped_holds_the_oldest_line_kept() {
    let ws = |c: u16| WinSize {
        ws_col: c,
        ws_row: 10,
    };
    let mut owned = Owned::with_history(&ws(20), &CacheGeometry::UNKNOWN, Depth::of(20));
    let mut t = Term::new(owned.storage(), &ws(20));
    let s: String = (0..40).map(long).collect();
    support::feed(&mut t, s.as_bytes());
    let deepest = Rows::of_count(t.scrollback().len());
    assert_eq!(deepest.index(), 20, "the control: the ring is full");
    let dropped_line = viewed(&t, deepest);
    let held = hold(&mut t, 12, 10, deepest);
    assert_eq!(held.index(), t.scrollback().len(), "the oldest line kept");
    assert_ne!(
        viewed(&t, held),
        dropped_line,
        "the control: the line the window showed is gone"
    );
}

/// A change of height and width in one step: the height retires rows the
/// cursor needs, which moves the window's lines up by that many, and then
/// the re-cut. The window holds its line through both.
#[test]
fn a_change_of_height_and_width_in_one_step_holds_the_line() {
    let s: String = (0..5).map(long).chain((0..30).map(short)).collect();
    for (to_cols, to_rows) in [(12, 6), (7, 4)] {
        at(20, 10, |t| {
            support::feed(t, s.as_bytes());
            let back = find(t, "s 05");
            let held = hold(t, to_cols, to_rows, back);
            assert_eq!(viewed(t, held), "s 05", "{to_cols}x{to_rows}");
        });
    }
}

/// At a prompt a change of width re-cuts nothing, and the window stays
/// where it was.
#[test]
fn at_a_prompt_the_window_holds_without_a_re_cut() {
    let s: String = (0..5).map(long).chain((0..30).map(short)).collect();
    at(20, 10, |t| {
        support::feed(t, s.as_bytes());
        support::feed(t, b"\x1b]133;P;k=i\x07$ ");
        let back = find(t, "s 05");
        let held = hold(t, 12, 10, back);
        assert_eq!(held, back, "nothing was re-cut");
        assert_eq!(viewed(t, held), "s 05");
    });
}
