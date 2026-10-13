//! **Under side margins the scrolling region is a rectangle**, and every
//! vertical scroll moves only what lies inside it.
//!
//! * `[[source: dec-std-070]]` § 5.4.3, p. 5-23: "A scrolling region is
//!   defined as the area bounded by the top, bottom, left, and right margins,
//!   which are set by DECSTBM and DECSLRM. All scrolling, both vertical and
//!   horizontal, is limited to this area." Its `scroll_up` and `scroll_down`
//!   (§ 5.14.2-3, pp. 5-210 and 5-211) copy `left margin` to `right margin`
//!   only -- revision note 28, "Modified routines scroll up and scroll down to
//!   operate within the left and right margins" -- and "Lines scrolled off the
//!   top of the scrolling region are lost."
//! * The table on p. 5-24 says which controls care where the cursor is. `IL`
//!   and `DL` read **X** for both margin pairs: they do "not work outside these
//!   margins", and STD 070's IL (p. 5-146) says "Only that portion of the
//!   display between the top, bottom, left, and right margins is affected.
//!   IL is ignored if the Active Position is outside the Scroll Area", DL
//!   alike (p. 5-148). VT510 words it two ways: "IL has no effect outside the
//!   page margins" (`[[source: vt510-il]]`), "DL has no effect outside the
//!   scrolling margins" (`[[source: vt510-dl]]`). `IND` and `RI` read **O** for the side
//!   margins: "Not affected. Continues to work outside these margins" -- and
//!   the `IND`, `RI` and `LF` algorithms (pp. 5-34, 5-36, 5-59) test only the
//!   line. So at the bottom margin `IND` scrolls the rectangle whatever column
//!   the cursor is in. xterm (`[[source: xterm-cursor-c]]`, `xtermIndex`,
//!   `RevIndex`) and ghostty (`[[source: ghostty-d9840f3-terminal]]`, `index`,
//!   `reverseIndex`) instead do nothing from outside the side margins: their
//!   departure, pinned in the V10 oracle, not followed.
//! * `SU` and `SD` are pans in the DEC texts (STD 070 pp. 5-91 and 5-92, "No
//!   movement of data within the Logical Display occurs"), so what they do to
//!   data is ECMA-48's -- SU "causes the data in the presentation component
//!   to be moved by n line positions" (`[[source: ecma48]]` § 8.3.147, SD
//!   § 8.3.113 alike) -- inside
//!   the area § 5.4.3 limits all scrolling to.
//!
//! **A wide character that straddles a side margin is erased whole**, both
//! halves, on every row the scroll moves, before anything moves: no text
//! settles it (the DEC terminals that defined margins had no wide cells), and
//! it is the rule baud's own range operations already keep ("we can't just
//! split a wide char", `View::snap`), as xterm's `scrollInMargins`
//! (`[[source: xterm-scroll]]`) and ghostty do. Listed for Jev in the record.
//!
//! The screen is 20 by 8, each row filled with a letter of its own by `CUP`,
//! the V10 oracle's fixture: a row that moved shows another letter, and a
//! column outside the fence that moved shows one where its row's should be.

use crate::support;
use baud_core::*;
use baud_domain::Count;

const ROWS: usize = 8;
const COLS: usize = 20;
const WS: WinSize = WinSize {
    ws_row: 8,
    ws_col: 20,
};
/// Row `y`'s letter in the fill.
const LETTERS: &[u8; ROWS] = b"abcdefgh";

/// Top and bottom margins at rows 2..6 (1-based); homes the cursor.
const TB: &str = "\x1b[2;6r";
/// `?69h`, then side margins at columns 5..10 (1-based); homes the cursor.
const LRM: &str = "\x1b[?69h\x1b[5;10s";

fn fill() -> String {
    let mut s = String::new();
    for (r, &letter) in LETTERS.iter().enumerate() {
        s.push_str(&format!("\x1b[{};1H", r + 1));
        s.extend(std::iter::repeat_n(char::from(letter), COLS));
    }
    s
}

/// What a test reads after the bytes: the screen as strings (`.` blank,
/// `]` a wide tail), the cursor, and the history's length.
#[derive(Debug, PartialEq, Eq, Clone)]
struct Seen {
    screen: Vec<String>,
    cursor: (Row, Col),
    history: Rows,
}

fn seen_at(bytes: &[u8], read: ReadLen) -> Seen {
    let mut s = Owned::new(&WS);
    let mut t = Term::new(s.storage(), &WS);
    for piece in bytes.chunks(read.index().max(1)) {
        t.write_at(BytePos::ZERO, piece);
        t.feed(BytePos::ZERO, ReadLen::from_index(piece.len()));
    }
    let screen = (0..ROWS)
        .map(|y| {
            t.row(Row::from_index(y))[..COLS]
                .iter()
                .map(|c| match c.content.is_wide_tail() {
                    true => ']',
                    false => c
                        .content
                        .as_codepoint()
                        .map_or('.', |cp| char::from_u32(cp.value()).unwrap_or('?')),
                })
                .collect()
        })
        .collect();
    Seen {
        screen,
        cursor: t.cursor(),
        history: Rows::of_count(t.scrollback().len()),
    }
}

/// The bytes whole, after requiring every read size to agree with them.
fn seen(bytes: &str) -> Seen {
    let b = bytes.as_bytes();
    let whole = seen_at(b, ReadLen::from_index(b.len()));
    for read in 1..b.len() {
        let at = seen_at(b, ReadLen::from_index(read));
        assert_eq!(at, whole, "reads of {read} differ: {bytes:?}");
    }
    whole
}

/// How far, and which way, [`model`] moves the rectangle.
#[derive(Clone, Copy)]
enum Shift {
    Up(Count),
    Down(Count),
}
use Shift::{Down, Up};

/// The model: the fill with the rows and columns given (0-based, inclusive)
/// moved by `shift`, blanking what opens.
fn model(
    rows: std::ops::RangeInclusive<usize>,
    cols: std::ops::RangeInclusive<usize>,
    shift: Shift,
) -> Vec<String> {
    let letter = |y: usize| char::from(LETTERS[y]);
    let mut out: Vec<Vec<char>> = (0..ROWS).map(|y| vec![letter(y); COLS]).collect();
    for y in rows.clone() {
        let from = match shift {
            Up(n) => Some(y + n.index()),
            Down(n) => y.checked_sub(n.index()),
        };
        for x in cols.clone() {
            out[y][x] = match from.filter(|f| rows.contains(f)) {
                Some(f) => letter(f),
                None => '.',
            };
        }
    }
    out.into_iter().map(|r| r.into_iter().collect()).collect()
}

fn lines(rows: &[&str]) -> Vec<String> {
    rows.iter().map(|s| s.to_string()).collect()
}

// ── SU and SD ───────────────────────────────────────────────────────────────

#[test]
fn su_moves_only_the_rectangle() {
    let s = seen(&format!("{}{TB}{LRM}\x1b[S", fill()));
    assert_eq!(
        s.screen,
        lines(&[
            "aaaaaaaaaaaaaaaaaaaa",
            "bbbbccccccbbbbbbbbbb",
            "ccccddddddcccccccccc",
            "ddddeeeeeedddddddddd",
            "eeeeffffffeeeeeeeeee",
            "ffff......ffffffffff",
            "gggggggggggggggggggg",
            "hhhhhhhhhhhhhhhhhhhh",
        ])
    );
    assert_eq!(
        s.cursor,
        (Row::from_index(0), Col::at_index(0)),
        "SU leaves the cursor"
    );
}

#[test]
fn sd_moves_only_the_rectangle() {
    let s = seen(&format!("{}{TB}{LRM}\x1b[2T", fill()));
    assert_eq!(s.screen, model(1..=5, 4..=9, Down(Count::of(2))));
}

#[test]
fn su_past_the_region_blanks_the_rectangle() {
    assert_eq!(
        seen(&format!("{}{TB}{LRM}\x1b[9S", fill())).screen,
        model(1..=5, 4..=9, Up(Count::of(9)))
    );
    assert_eq!(
        seen(&format!("{}{TB}{LRM}\x1b[9T", fill())).screen,
        model(1..=5, 4..=9, Down(Count::of(9)))
    );
}

/// `SU` and `SD` scroll the region, not the cursor's surroundings, and
/// leave the cursor where it was -- read against the same bytes without the
/// scroll, because `Term::cursor` clamps a cursor right of the fence to the
/// right margin (note V's V10, step (d)).
#[test]
fn su_and_sd_ignore_where_the_cursor_is() {
    let at = format!("{}{TB}{LRM}\x1b[1;15H", fill());
    let s = seen(&format!("{at}\x1b[S"));
    assert_eq!(s.screen, model(1..=5, 4..=9, Up(Count::of(1))));
    assert_eq!(s.cursor, seen(&at).cursor);
    let s = seen(&format!("{at}\x1b[T"));
    assert_eq!(s.screen, model(1..=5, 4..=9, Down(Count::of(1))));
}

// ── IL and DL ───────────────────────────────────────────────────────────────

/// From the cursor's row to the bottom margin, inside the side margins.
#[test]
fn il_and_dl_move_the_rectangle_from_the_cursor_row() {
    let il = seen(&format!("{}{TB}{LRM}\x1b[3;6H\x1b[L", fill()));
    assert_eq!(il.screen, model(2..=5, 4..=9, Down(Count::of(1))));
    let dl = seen(&format!("{}{TB}{LRM}\x1b[3;6H\x1b[2M", fill()));
    assert_eq!(dl.screen, model(2..=5, 4..=9, Up(Count::of(2))));
}

/// STD 070 p. 5-24, X under L/R: "Does not work outside these margins."
///
/// **Not column 11, right + 1**: a cursor moved there reads as the pending
/// wrap the sentinel also spells, which is on the right margin, so `IL` acts
/// from it. That conflation is note V's V10 step (d), the corner; this pins
/// every column it does not touch.
#[test]
fn il_and_dl_outside_the_side_margins_do_nothing() {
    let none = seen(&format!("{}{TB}{LRM}", fill())).screen;
    for at in ["3;1", "3;2", "3;4", "3;12", "3;15", "3;20"] {
        for op in ["L", "M", "3L", "3M"] {
            let s = seen(&format!("{}{TB}{LRM}\x1b[{at}H\x1b[{op}", fill()));
            assert_eq!(s.screen, none, "CUP {at}, CSI {op}");
        }
    }
}

/// **At a pending wrap the cursor is on the right margin**, not outside it:
/// the sentinel sits one past it, and reading that as outside would make
/// `IL` at the end of a full row inert on every screen.
#[test]
fn il_at_a_pending_wrap_is_inside_the_margins() {
    let full = seen(&format!("{}\x1b[3;1Hxxxxxxxxxxxxxxxxxxxx\x1b[L", fill()));
    assert_eq!(full.screen[2], "....................", "full width");
    let fenced = seen(&format!("{}{TB}{LRM}\x1b[3;5Hxxxxxx\x1b[L", fill()));
    assert_eq!(fenced.screen[2], "cccc......cccccccccc", "under the fence");
    assert_eq!(fenced.screen[3], "ddddxxxxxxdddddddddd");
}

// ── LF, IND and RI ──────────────────────────────────────────────────────────

#[test]
fn lf_and_ind_at_the_bottom_margin_move_the_rectangle() {
    for op in ["\n", "\x1bD", "\x0b", "\x0c"] {
        let s = seen(&format!("{}{TB}{LRM}\x1b[6;7H{op}", fill()));
        assert_eq!(s.screen, model(1..=5, 4..=9, Up(Count::of(1))), "{op:?}");
        assert_eq!(
            s.cursor,
            (Row::from_index(5), Col::at_index(6)),
            "{op:?}: the cursor stays on the margin"
        );
    }
}

#[test]
fn ri_at_the_top_margin_moves_the_rectangle() {
    let s = seen(&format!("{}{TB}{LRM}\x1b[2;7H\x1bM", fill()));
    assert_eq!(s.screen, model(1..=5, 4..=9, Down(Count::of(1))));
    assert_eq!(s.cursor, (Row::from_index(1), Col::at_index(6)));
}

/// **STD 070 p. 5-24, O under L/R, for `IND` and `RI`**: "Continues to work
/// outside these margins", and the algorithms test the line alone. xterm
/// and ghostty move nothing from here; the V10 oracle pins that.
#[test]
fn ind_lf_and_ri_outside_the_side_margins_still_scroll_the_rectangle() {
    for at in ["6;2", "6;15"] {
        for op in ["\x1bD", "\n"] {
            let s = seen(&format!("{}{TB}{LRM}\x1b[{at}H{op}", fill()));
            assert_eq!(
                s.screen,
                model(1..=5, 4..=9, Up(Count::of(1))),
                "CUP {at}, {op:?}"
            );
        }
    }
    let s = seen(&format!("{}{TB}{LRM}\x1b[2;15H\x1bM", fill()));
    assert_eq!(s.screen, model(1..=5, 4..=9, Down(Count::of(1))));
}

/// A write that wraps at the bottom margin scrolls the rectangle and goes on
/// at the left margin.
#[test]
fn a_wrap_at_the_bottom_margin_scrolls_the_rectangle() {
    let s = seen(&format!("{}{TB}{LRM}\x1b[6;5H0123456789AB", fill()));
    assert_eq!(s.screen[4], "eeee012345eeeeeeeeee");
    assert_eq!(s.screen[5], "ffff6789ABffffffffff");
    assert_eq!(s.screen[3], "ddddeeeeeedddddddddd");
    assert_eq!(s.cursor, (Row::from_index(5), Col::at_index(9)));
}

// ── what a rectangle keeps ──────────────────────────────────────────────────

/// **The whole screen under side margins is a rectangle too, and keeps no
/// history**: "Lines scrolled off the top of the scrolling region are lost",
/// and a line that is part of a row is not a line. At full width the same
/// bytes retire three lines, the control.
#[test]
fn the_whole_screen_under_side_margins_keeps_no_history() {
    let s = seen(&format!("{}{LRM}\x1b[8;7H\n\n\n", fill()));
    assert_eq!(s.screen, model(0..=7, 4..=9, Up(Count::of(3))));
    assert_eq!(s.history, Rows::ZERO);
    let full = seen(&format!("{}\x1b[8;7H\n\n\n", fill()));
    assert_eq!(
        full.history,
        Rows::of_count(3),
        "the control: full width retires"
    );
}

/// A region anchored at the top retires its line at full width; under side
/// margins it is a rectangle and keeps nothing.
#[test]
fn a_top_anchored_region_under_side_margins_keeps_no_history() {
    let s = seen(&format!("{}\x1b[1;6r{LRM}\x1b[6;7H\n\n", fill()));
    assert_eq!(s.screen, model(0..=5, 4..=9, Up(Count::of(2))));
    assert_eq!(s.history, Rows::ZERO);
    // Taller than the rows under it, which rotated whatever the margins.
    let s = seen(&format!("{}\x1b[1;7r{LRM}\x1b[7;7H\n", fill()));
    assert_eq!(s.screen, model(0..=6, 4..=9, Up(Count::of(1))));
    assert_eq!(s.history, Rows::ZERO);
    let full = seen(&format!("{}\x1b[1;6r\x1b[6;7H\n\n", fill()));
    assert_eq!(full.history, Rows::of_count(2), "the control");
}

/// **A margin on the screen's edge still makes a rectangle** when the other
/// one is inside it: margins at the left edge (`1;10s`) and at the right
/// edge (`11;20s`) each scroll only their columns, through `SU`, `SD`, `LF`
/// at the bottom margin and `LF` on the whole screen, which keeps no history.
/// Full width is both margins on the edges, not either.
#[test]
fn a_side_margin_on_the_screens_edge_still_makes_a_rectangle() {
    for (lrm, cols, at) in [
        ("\x1b[?69h\x1b[1;10s", 0..=9, 3),
        ("\x1b[?69h\x1b[11;20s", 10..=19, 15),
    ] {
        let s = seen(&format!("{}{TB}{lrm}\x1b[S", fill()));
        assert_eq!(
            s.screen,
            model(1..=5, cols.clone(), Up(Count::of(1))),
            "{lrm:?} SU"
        );
        let s = seen(&format!("{}{TB}{lrm}\x1b[2T", fill()));
        assert_eq!(
            s.screen,
            model(1..=5, cols.clone(), Down(Count::of(2))),
            "{lrm:?} SD"
        );
        let s = seen(&format!("{}{TB}{lrm}\x1b[6;{at}H\n", fill()));
        assert_eq!(
            s.screen,
            model(1..=5, cols.clone(), Up(Count::of(1))),
            "{lrm:?} LF"
        );
        let s = seen(&format!("{}{lrm}\x1b[8;{at}H\n", fill()));
        assert_eq!(
            s.screen,
            model(0..=7, cols.clone(), Up(Count::of(1))),
            "{lrm:?} whole"
        );
        assert_eq!(s.history, Rows::ZERO, "{lrm:?}: no history");
    }
}

/// **A resize puts the margins back, and the full-width scroll with them**:
/// a line feed at the bottom of the whole screen retires its line to the
/// history again, where under the margins it kept none.
#[test]
fn a_resize_puts_the_margins_back() {
    let mut s = Owned::new(&WS);
    let mut t = Term::new(s.storage(), &WS);
    support::feed(&mut t, format!("top{LRM}").as_bytes());
    let _ = t.resize(&WinSize {
        ws_row: 8,
        ws_col: 15,
    });
    let before = t.scrollback().len();
    support::feed(&mut t, b"\x1b[8;1H\n");
    assert_eq!(t.scrollback().len(), before + 1, "full width retires");
}

#[test]
fn the_alternate_screen_scrolls_the_rectangle() {
    let s = seen(&format!("\x1b[?1049h{}{LRM}\x1b[8;7H\n", fill()));
    assert_eq!(s.screen, model(0..=7, 4..=9, Up(Count::of(1))));
    let s = seen(&format!("\x1b[?1049h{}\x1b[1;6r{LRM}\x1b[S", fill()));
    assert_eq!(s.screen, model(0..=5, 4..=9, Up(Count::of(1))));
}

/// **Every way out of `DECLRMM` puts the margins back**, so no scroll is a
/// rectangle with the mode off: `?69l`, `DECSTR` (xterm's `ReallyReset`
/// clears `DECLRMM`, and `[[source: dec-std-070]]`'s reset table gives
/// "Left Margin 1, Right Margin 80"), `RIS`, and XTRESTORE of a `?69` saved
/// off. `Paint::fenced` reads the margins, not the mode, so this is what
/// keeps the two from disagreeing.
#[test]
fn without_declrmm_the_scrolls_move_whole_rows() {
    for off in ["\x1b[?69l", "\x1b[!p", "\x1b[?69r"] {
        let save = match off {
            "\x1b[?69r" => "\x1b[?69s",
            _ => "",
        };
        let s = seen(&format!("{}{save}{TB}{LRM}{off}{TB}\x1b[S", fill()));
        assert_eq!(s.screen, model(1..=5, 0..=19, Up(Count::of(1))), "{off:?}");
        // A rectangle as wide as the screen draws the same screen, so the
        // history is what tells them apart: a whole row retires.
        let s = seen(&format!("{}{save}{LRM}{off}\x1b[8;1H\n", fill()));
        assert_eq!(s.history, Rows::of_count(1), "{off:?}: the line retires");
    }
    let s = seen(&format!("{TB}{LRM}\x1bc{}{TB}\x1b[S", fill()));
    assert_eq!(s.screen, model(1..=5, 0..=19, Up(Count::of(1))), "RIS");
    let s = seen(&format!("{LRM}\x1bc{}\x1b[8;1H\n", fill()));
    assert_eq!(s.history, Rows::of_count(1), "RIS: the line retires");
}

// ── wide characters across a margin ─────────────────────────────────────────

/// Both halves of a character a side margin cuts are erased, on every row of
/// the region, before the rectangle moves -- the moved rows and the row the
/// scroll writes into alike. The four screens are what ghostty draws too.
#[test]
fn a_wide_character_across_a_side_margin_is_erased_whole() {
    let cases = [
        // Lead outside the left margin, tail inside, on a row that moves.
        (
            "\x1b[3;4H\u{4e2d}",
            "\x1b[S",
            ["bbbb.cccccbbbbbbbbbb", "ccc.ddddddcccccccccc"],
            1,
        ),
        // Lead inside the right margin, tail outside.
        (
            "\x1b[4;10H\u{4e2d}",
            "\x1b[S",
            ["ccccddddd.cccccccccc", "ddddeeeeee.ddddddddd"],
            2,
        ),
        // On the top row, which only takes the row below's cells.
        (
            "\x1b[2;4H\u{4e2d}",
            "\x1b[S",
            ["bbb.ccccccbbbbbbbbbb", "ccccddddddcccccccccc"],
            1,
        ),
        // On the bottom row, scrolling down, which it only takes into.
        (
            "\x1b[6;10H\u{4e2d}",
            "\x1b[T",
            ["eeeeddddddeeeeeeeeee", "ffffeeeeee.fffffffff"],
            4,
        ),
    ];
    for (put, op, want, at) in cases {
        let s = seen(&format!("{}{put}{TB}{LRM}{op}", fill()));
        assert_eq!(&s.screen[at..at + 2], &want, "{put:?} then {op:?}");
        for row in &s.screen {
            assert!(!row.contains('\u{4e2d}') && !row.contains(']'), "{row}");
        }
    }
}

// ── the cells it writes ─────────────────────────────────────────────────────

/// The blank the scroll opens carries the current background, as every
/// scroll's here does, and only inside the rectangle.
#[test]
fn a_coloured_blank_fills_only_the_rectangle() {
    let bytes = format!("{}{TB}{LRM}\x1b[44m\x1b[S", fill());
    let mut s = Owned::new(&WS);
    let mut t = Term::new(s.storage(), &WS);
    support::feed(&mut t, bytes.as_bytes());
    let row = t.row(Row::from_index(5));
    assert_eq!(row[4].content, Content::EMPTY);
    assert_ne!(row[4].style, row[3].style, "blue inside");
    assert_eq!(row[9].style, row[4].style);
    assert_eq!(row[10].style, row[3].style, "untouched outside");
}

/// **Every row the rectangle writes is marked to the right margin**: a row
/// blank to its mark of zero that takes cells inside the fence is no longer
/// uniform from there, and a later blank that stops at its mark would leave
/// them standing.
#[test]
fn the_marks_cover_what_the_rectangle_wrote() {
    for op in ["\x1b[2T", "\x1b[S", "\x1b[3;6H\x1b[L", "\x1b[3;6H\x1b[M"] {
        let bytes = format!("\x1b[4;5Hxxxxxx\x1b[44m{TB}{LRM}{op}");
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        support::feed(&mut t, bytes.as_bytes());
        for y in 0..ROWS {
            let row = t.row(Row::from_index(y));
            let last = row[COLS - 1];
            let uniform = row[..COLS]
                .iter()
                .rposition(|c| *c != last)
                .map_or(0, |p| p + 1);
            let mark = t.row_extent(Row::from_index(y)).index();
            assert!(
                mark >= uniform,
                "{op:?}: row {y} uniform from {uniform}, marked {mark}"
            );
        }
    }
}

/// **`DL` re-reads the cursor's row after the rectangle moves**: a blank row
/// that takes a wide character from below is no longer blank where the
/// write path's shortcut says it is, and a write over the tail with no move
/// in between must still erase the lead. `SU` and `SD` re-read in their
/// dispatch and `LF` in `index`; `IL` and `DL` only through
/// `Paint::scroll_rect`.
#[test]
fn dl_that_brings_a_wide_character_onto_the_cursor_row_rereads_it() {
    let s = seen(&format!("\x1b[4;6H\u{4e2d}{TB}{LRM}\x1b[3;7H\x1b[Mx"));
    assert_eq!(s.screen[2], "......x.............", "the lead erased");
    assert_eq!(s.screen[3], "....................");
}

/// **The cells a rectangle scroll writes are its rows times its width**,
/// plus the two halves of each wide character a margin cuts: `SU` on rows
/// 2..6 between columns 5 and 10 writes 5 x 6 = 30, and 32 with one cut.
#[test]
fn a_rectangle_scroll_counts_the_cells_it_writes() {
    for (put, want) in [("", 30), ("\x1b[3;4H\u{4e2d}", 32)] {
        let mut s = Owned::new(&WS);
        let mut t = Term::new(s.storage(), &WS);
        support::feed(&mut t, format!("{}{put}{TB}{LRM}", fill()).as_bytes());
        let (_, before) = t.amplification();
        support::feed(&mut t, b"\x1b[S");
        let (_, after) = t.amplification();
        assert_eq!(after - before, want, "{put:?}");
    }
}

// ── images ──────────────────────────────────────────────────────────────────

/// **An image anchored inside the rectangle moves with its text, and one
/// anchored beside it on the same rows stays with its own.** By the anchor's
/// column as by its row -- no text says what an image does here, and ghostty
/// moves only one wholly inside; listed for Jev in the record.
#[test]
fn an_image_moves_only_when_its_anchor_is_inside_the_rectangle() {
    use baud_domain::ImageBudget;
    // A 1x6 red Sixel, one row at a sixteen-pixel cell.
    const TINY: &str = "\x1bP0;1;0q#0;2;100;0;0~\x1b\\";
    let lines = |op: &str| {
        let mut s = Owned::new(&WS).with_images(ImageBudget::of(4096, 4096));
        let mut t = Term::new(s.storage(), &WS);
        t.set_cell_size(ImagePx::of(8), ImagePx::of(16));
        let bytes = format!("{}\x1b[4;6H{TINY}\x1b[4;15H{TINY}{TB}{LRM}{op}", fill());
        support::feed(&mut t, bytes.as_bytes());
        let mut v: Vec<(usize, u64)> = t
            .images()
            .placements()
            .map(|(_, p)| (p.col.index(), p.line.value()))
            .collect();
        v.sort();
        v
    };
    let before = lines("");
    assert_eq!(before.len(), 2, "both placed: {before:?}");
    let (inside, beside) = (before[0], before[1]);
    assert_eq!((inside.0, beside.0), (5, 14));
    assert_eq!(lines("\x1b[S"), vec![(5, inside.1 - 1), beside], "SU");
    assert_eq!(lines("\x1b[T"), vec![(5, inside.1 + 1), beside], "SD");
    assert_eq!(
        lines("\x1b[4;6H\x1b[M"),
        vec![beside],
        "DL at its row deletes it"
    );
}

/// **An image anchored on a margin column is inside the rectangle**: the
/// left margin (column 5) and the right (column 10) are both in it, so both
/// move with `SU`, and the columns either side of them (4 and 11) stay.
#[test]
fn an_image_anchored_on_a_margin_column_moves() {
    use baud_domain::ImageBudget;
    const TINY: &str = "\x1bP0;1;0q#0;2;100;0;0~\x1b\\";
    let placed = |op: &str| {
        let mut s = Owned::new(&WS).with_images(ImageBudget::of(4096, 4096));
        let mut t = Term::new(s.storage(), &WS);
        t.set_cell_size(ImagePx::of(8), ImagePx::of(16));
        let mut bytes = fill();
        for col in [4, 5, 10, 11] {
            bytes.push_str(&format!("\x1b[4;{col}H{TINY}"));
        }
        bytes.push_str(&format!("{TB}{LRM}{op}"));
        support::feed(&mut t, bytes.as_bytes());
        let mut v: Vec<(usize, u64)> = t
            .images()
            .placements()
            .map(|(_, p)| (p.col.index(), p.line.value()))
            .collect();
        v.sort();
        v
    };
    let before = placed("");
    let cols: Vec<usize> = before.iter().map(|p| p.0).collect();
    assert_eq!(cols, vec![3, 4, 9, 10], "all four placed: {before:?}");
    let line = before[0].1;
    assert!(before.iter().all(|p| p.1 == line), "{before:?}");
    assert_eq!(
        placed("\x1b[S"),
        vec![(3, line), (4, line - 1), (9, line - 1), (10, line)],
        "SU moves the two on the margins and no other"
    );
}
