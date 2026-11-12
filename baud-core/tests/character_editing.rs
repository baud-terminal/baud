//! **HEM, `CSI 10 h` / `CSI 10 l`** (`[[source: ecma48]]` 7.2.9, 7.3.3):
//! whether a character insertion or deletion shifts the positions after
//! the active position or before it. RM 10 selects FOLLOWING, the state
//! at power-on, and SM 10 PRECEDING. Under PRECEDING `DCH` removes the
//! active position and the ones before it and closes the gap from the
//! line's start, `ICH` pushes the active position and the ones before it
//! toward the line's start, and a character written under IRM is inserted
//! at the cursor, the ones before it moving toward the line's start, and
//! "the active position does not move" (7.3.3 b). With BDSM reset and DCSM
//! at PRESENTATION the positions shifted are the ones drawn, on a line
//! holding an SDS or SRS string.

mod support;

use baud_core::*;
use baud_domain::{BidiDirection, StringId, order_strings};

const WS: WinSize = WinSize {
    ws_row: 4,
    ws_col: 10,
};

/// The characters of row 0's first `n` cells, `.` for an empty one and
/// `_` for a wide character's tail.
fn row(t: &Term<'_>, n: usize) -> String {
    t.viewport().row(Row::ZERO)[..n]
        .iter()
        .map(|c| match c.content.is_wide_tail() {
            true => '_',
            false => c
                .content
                .as_codepoint()
                .and_then(|cp| char::from_u32(cp.value()))
                .unwrap_or('.'),
        })
        .collect()
}

/// Row 0's first `n` cells as BDSM reset draws them, left to right on a
/// left-to-right line, each string in its own direction.
fn drawn(t: &Term<'_>, n: usize) -> String {
    let cells = row(t, n);
    let chars: Vec<char> = cells.chars().collect();
    let ids: Vec<StringId> = t.viewport().row(Row::ZERO)[..n]
        .iter()
        .map(|c| {
            t.styles()[c.style.index()]
                .map(|w| w.string())
                .unwrap_or(StringId::NONE)
        })
        .collect();
    let mut from = vec![0u16; n];
    let mut runs = vec![BidiDirection::Ltr; n];
    let entry = |id| t.string_entry(id);
    order_strings(
        &ids,
        |_| false,
        n,
        BidiDirection::Ltr,
        &entry,
        &mut from,
        &mut runs,
    );
    from.iter().map(|&c| chars[usize::from(c)]).collect()
}

/// `abcdef` on row 0, the cursor on `d`, then `then`: row 0 and the
/// cursor.
fn edited(then: &str) -> (String, (Row, Col)) {
    support::with_term(&WS, |t| {
        support::feed(t, format!("abcdef\x1b[1;4H{then}").as_bytes());
        (row(t, 8), t.cursor())
    })
}

/// The cursor on `d`, column 3.
fn at_d() -> (Row, Col) {
    (Row::ZERO, Col::at_index(3))
}

/// **`DCH` under PRECEDING removes the active position and the `n - 1`
/// before it** (`[[source: ecma48]]` 8.3.26: "the contents of the active
/// data position and, depending on the setting of the CHARACTER EDITING
/// MODE (HEM), the contents of the n-1 preceding or following character
/// positions"), and the gap closes from the line's start, where the blanks
/// go; the cursor stays. Under FOLLOWING, the state at power-on, the
/// positions after it.
#[test]
fn dch_under_preceding_closes_the_gap_from_the_line_start() {
    assert_eq!(edited("\x1b[10h\x1b[2P"), ("..abef..".into(), at_d()));
    assert_eq!(edited("\x1b[10h\x1b[9P"), ("....ef..".into(), at_d()));
    assert_eq!(edited("\x1b[2P"), ("abcf....".into(), at_d()));
    assert_eq!(
        edited("\x1b[10h\x1b[10l\x1b[2P"),
        ("abcf....".into(), at_d())
    );
}

/// **`ICH` under PRECEDING pushes the active position and the ones before
/// it toward the line's start** (8.3.64: "the previous contents of the
/// active data position and an adjacent string of character positions are
/// shifted away from the active data position"), the first `n` leaving
/// the line; the cursor stays.
#[test]
fn ich_under_preceding_pushes_toward_the_line_start() {
    assert_eq!(edited("\x1b[10h\x1b[2@"), ("cd..ef..".into(), at_d()));
    assert_eq!(edited("\x1b[10h\x1b[9@"), ("....ef..".into(), at_d()));
    assert_eq!(edited("\x1b[2@"), ("abc..def".into(), at_d()));
}

/// **A character written under IRM and PRECEDING goes in at the cursor and
/// the cursor stays** (7.3.3 b: "if it is set to PRECEDING, the active
/// position does not move"; 7.2.9: "a character insertion causes the
/// contents of the active data position and of preceding character
/// positions in the data component to be shifted in the direction opposite
/// to that of the character progression"): `X` then `Y` at `d` push `a`,
/// `b` and `c` toward the start and off it, `Y` last at the cursor. A wide
/// character takes the cursor's column and the one before it, and at the
/// line's start, with no column before it, the cursor's and the one after
/// it.
#[test]
fn irm_under_preceding_inserts_at_the_cursor_which_stays() {
    assert_eq!(
        edited("\x1b[1;1H\x1b[4h\x1b[10h\u{4e2d}"),
        ("\u{4e2d}_cdef..".into(), (Row::ZERO, Col::ZERO))
    );
    assert_eq!(edited("\x1b[4h\x1b[10hX"), ("bcdXef..".into(), at_d()));
    assert_eq!(edited("\x1b[4h\x1b[10hXY"), ("cdXYef..".into(), at_d()));
    assert_eq!(
        edited("\x1b[4h\x1b[10h\u{4e2d}"),
        ("cd\u{4e2d}_ef..".into(), at_d())
    );
    assert_eq!(
        edited("\x1b[4hX"),
        ("abcXdef.".into(), (Row::ZERO, Col::at_index(4)))
    );
    assert_eq!(
        edited("\x1b[10hX"),
        ("abcXef..".into(), (Row::ZERO, Col::at_index(4))),
        "IRM reset: replaced, the cursor on"
    );
}

/// **A mark written after an inserted character under PRECEDING joins it**,
/// at the cursor, where the last write put it down.
#[test]
fn a_mark_under_preceding_joins_the_character_at_the_cursor() {
    support::with_term(&WS, |t| {
        support::feed(t, "abcdef\x1b[1;4H\x1b[4h\x1b[10hX\u{301}".as_bytes());
        let cell = t.viewport().row(Row::ZERO)[3];
        assert_eq!(
            t.base_of(cell).as_codepoint().map(|c| c.value()),
            Some(u32::from('X'))
        );
        assert_eq!(&row(t, 8)[..3], "bcd");
        let marks: Vec<u32> = t.marks(Row::ZERO, Col::at_index(3)).collect();
        assert_eq!(marks, [0x301]);
        assert_eq!(t.cursor(), at_d());
    });
}

/// **Inside the side margins the shifted part starts at the left margin**,
/// as it ends at the right margin under FOLLOWING: with DECLRMM and the
/// margins at columns 2 to 6, `DCH 1` at `d` leaves `a` where it is.
#[test]
fn preceding_shifts_from_the_left_margin() {
    let margins = "\x1b[?69h\x1b[2;6s\x1b[1;4H";
    assert_eq!(
        edited(&format!("{margins}\x1b[10h\x1b[P")),
        ("a.bcef..".into(), at_d())
    );
    assert_eq!(
        edited(&format!("{margins}\x1b[10h\x1b[@")),
        ("acd.ef..".into(), at_d())
    );
}

/// **`DECRQM` answers HEM**: reset, `2`, at power-on; set, `1`, after
/// `SM 10`. `RIS` resets it and `DECSTR` keeps it, as it keeps DCSM.
#[test]
fn decrqm_answers_hem() {
    let reply = |then: &str| {
        support::with_term(&WS, |t| {
            support::feed(t, format!("{then}\x1b[10$p").as_bytes());
            String::from_utf8_lossy(t.reply()).into_owned()
        })
    };
    assert_eq!(reply(""), "\x1b[10;2$y");
    assert_eq!(reply("\x1b[10h"), "\x1b[10;1$y");
    assert_eq!(reply("\x1b[10h\x1b[10l"), "\x1b[10;2$y");
    assert_eq!(reply("\x1b[10h\x1bc"), "\x1b[10;2$y");
    assert_eq!(reply("\x1b[10h\x1b[!p"), "\x1b[10;1$y");
}

/// **`RIS` returns the write to FOLLOWING**: IRM and HEM set, then `RIS`,
/// then IRM set again, writes as at power-on.
#[test]
fn ris_returns_insertion_to_following() {
    assert_eq!(
        edited("\x1b[4h\x1b[10h\x1bcabcdef\x1b[1;4H\x1b[4hX"),
        ("abcXdef.".into(), (Row::ZERO, Col::at_index(4)))
    );
}

/// The reversed string `abc`, still open, on row 0, BDSM reset, then
/// `mode` and `then`: row 0 as stored, as drawn, and the cursor. Drawn
/// `cba`: `c` at position 0, `b` at 1, `a` at 2.
fn reversed(mode: &str, then: &str) -> (String, String, (Row, Col)) {
    support::with_term(&WS, |t| {
        support::feed(t, format!("\x1b[8l\x1b[1[abc{mode}{then}").as_bytes());
        (row(t, 6), drawn(t, 6), t.cursor())
    })
}

const PRESENTATION: &str = "\x1b[9l";
const DATA: &str = "\x1b[9h";

/// **Under PRESENTATION a character written under IRM is inserted at the
/// presentation position** (7.2.10: "inserted at the active presentation
/// position"; 7.2.9: "the contents of the active presentation position
/// and of the following character positions in the presentation component
/// to be shifted in the direction of the character path"): at `b`, drawn
/// at position 1, `X` goes after it in the cells, so it is drawn at 1 and
/// `b` at 2; under DATA before it in the cells, drawn at 2. The cursor
/// then moves on by the cells, to `c`, so a second character goes in at
/// `c`'s position, 0, and is drawn apart from the first.
#[test]
fn irm_under_presentation_inserts_at_the_presentation_position() {
    let (cells, shown, _) = reversed(PRESENTATION, "\x1b[1;2f\x1b[4hX");
    assert_eq!((cells.as_str(), shown.as_str()), ("abXc..", "cXba.."));
    let (cells, shown, _) = reversed(PRESENTATION, "\x1b[1;2f\x1b[4hXY");
    assert_eq!((cells.as_str(), shown.as_str()), ("abXcY.", "YcXba."));
    let (cells, shown, _) = reversed(DATA, "\x1b[1;2f\x1b[4hX");
    assert_eq!((cells.as_str(), shown.as_str()), ("aXbc..", "cbXa.."));
}

/// **And under PRECEDING the positions before it move toward the line's
/// home**, the cursor kept at its presentation position: `X` at `b`,
/// position 1, pushes `c` off the home and `b` to 0, and is drawn at 1;
/// under DATA it pushes `a` off the line's start, the cursor kept on its
/// cell.
#[test]
fn irm_under_presentation_and_preceding_pushes_toward_the_home() {
    let (cells, shown, at) = reversed(PRESENTATION, "\x1b[1;2f\x1b[4h\x1b[10hX");
    assert_eq!((cells.as_str(), shown.as_str()), ("aXb...", "bXa..."));
    assert_eq!(at, (Row::ZERO, Col::at_index(1)));
    let (cells, shown, at) = reversed(DATA, "\x1b[1;2f\x1b[4h\x1b[10hX");
    assert_eq!((cells.as_str(), shown.as_str()), ("bXc...", "cXb..."));
    assert_eq!(at, (Row::ZERO, Col::at_index(1)));
}

/// **`DCH` and `ICH` under PRESENTATION and PRECEDING shift the positions
/// drawn before the cursor's** (8.3.26, 8.3.64): drawn `c b a y`, from
/// `a`, position 2, `DCH 2` removes `a` and `b` and `c` closes up to
/// position 2, the blanks at the home; `ICH 1` at `b`, position 1, pushes
/// `c` off the home and `b` to position 0, a blank drawn at 1. Under DATA
/// they shift the cells before the cursor's.
#[test]
fn dch_and_ich_under_presentation_and_preceding() {
    let edit = |mode: &str, then: &str| {
        support::with_term(&WS, |t| {
            let setup = format!("\x1b[8l\x1b[1[abc\x1b[0[y{mode}\x1b[10h{then}");
            support::feed(t, setup.as_bytes());
            (row(t, 6), drawn(t, 6), t.cursor())
        })
    };
    let (cells, shown, at) = edit(PRESENTATION, "\x1b[1;1f\x1b[2P");
    assert_eq!((cells.as_str(), shown.as_str()), ("c..y..", "..cy.."));
    assert_eq!(at, (Row::ZERO, Col::ZERO), "on c, at position 2");
    let (cells, shown, _) = edit(PRESENTATION, "\x1b[1;2f\x1b[@");
    assert_eq!((cells.as_str(), shown.as_str()), ("a.by..", "b.ay.."));
    let (cells, _, _) = edit(DATA, "\x1b[1;2f\x1b[2P");
    assert_eq!(cells, "..cy..");
    let (cells, _, _) = edit(DATA, "\x1b[1;2f\x1b[@");
    assert_eq!(cells, "b.cy..");
}

/// **A run of wide or of narrow multi-byte characters opens its gap under
/// IRM** (7.2.10: "inserted at the active presentation position"), as an
/// ASCII run and a single character do: four `世` before `ab` push it
/// eight columns on, four `─` four. The bulk writers wrote over the line
/// instead, so whether `ab` survived depended on whether the run was one
/// width and fit, and where the reads fell.
#[test]
fn irm_opens_a_gap_before_a_run_of_multibyte_characters() {
    let wide = |then: &str| {
        support::with_term(&WS, |t| {
            support::feed(t, format!("ab\r\x1b[4h{then}").as_bytes());
            (row(t, 10), t.cursor())
        })
    };
    assert_eq!(
        wide("\u{4e16}\u{4e16}\u{4e16}\u{4e16}"),
        (
            "\u{4e16}_\u{4e16}_\u{4e16}_\u{4e16}_ab".into(),
            (Row::ZERO, Col::at_index(8))
        )
    );
    assert_eq!(
        wide("\u{2500}\u{2500}\u{2500}\u{2500}"),
        (
            "\u{2500}\u{2500}\u{2500}\u{2500}ab....".into(),
            (Row::ZERO, Col::at_index(4))
        )
    );
}

/// `abcdefg` then the reversed string `xyz`, still open, on a line of
/// ten columns, so the string ends at the line's edge, drawn `zyx` at
/// positions 7, 8 and 9; then `then`. Row 0 as stored, as drawn, row 1,
/// and the cursor.
fn at_the_edge(then: &str) -> (String, String, String, (Row, Col)) {
    support::with_term(&WS, |t| {
        support::feed(t, format!("\x1b[8labcdefg\x1b[1[xyz{then}").as_bytes());
        let next: String = t.viewport().row(Row::from_index(1))[..10]
            .iter()
            .map(|c| {
                c.content
                    .as_codepoint()
                    .and_then(|cp| char::from_u32(cp.value()))
                    .unwrap_or('.')
            })
            .collect();
        (row(t, 10), drawn(t, 10), next, t.cursor())
    })
}

/// **A string that ends at the line's edge is shifted on the line, as one
/// in its middle is** (7.2.9: "the contents of the active presentation
/// position and of the following character positions in the presentation
/// component to be shifted in the direction of the character path"). `Q`
/// written under IRM at `z`, data column 9 drawn at position 7, is drawn
/// there, `z` and `y` move on to 8 and 9 and `x` leaves the line, with
/// autowrap set or reset; nothing reaches the next line. At `y` and at `x`
/// the same, and `ICH` opens its blank where `Q` would go. A wide
/// character at `z` takes positions 7 and 8; at `x`, the last position,
/// it does not fit and wraps.
#[test]
fn a_string_at_the_line_edge_shifts_on_the_line() {
    let blank = ".".repeat(10);
    for wrap in ["", "\x1b[?7l"] {
        let (cells, shown, next, _) = at_the_edge(&format!("{wrap}\x1b[1;10f\x1b[4hQ"));
        assert_eq!(
            (cells.as_str(), shown.as_str(), next.as_str()),
            ("abcdefgyzQ", "abcdefgQzy", blank.as_str()),
            "{wrap:?}"
        );
    }
    let cases = [
        ("\x1b[1;9f\x1b[4hQ", "abcdefgyQz", "abcdefgzQy"),
        ("\x1b[1;8f\x1b[4hQ", "abcdefgQyz", "abcdefgzyQ"),
        ("\x1b[1;10f\x1b[@", "abcdefgyz.", "abcdefg.zy"),
        ("\x1b[1;10f\x1b[2@", "abcdefgz..", "abcdefg..z"),
        ("\x1b[1;8f\x1b[@", "abcdefg.yz", "abcdefgzy."),
    ];
    for (then, cells, shown) in cases {
        let got = at_the_edge(then);
        assert_eq!(
            (got.0.as_str(), got.1.as_str(), got.2.as_str()),
            (cells, shown, blank.as_str()),
            "{then:?}"
        );
    }
    let (cells, _, next, _) = at_the_edge("\x1b[1;10f\x1b[4h\u{4e00}");
    assert_eq!(
        (cells.as_str(), next.as_str()),
        ("abcdefgz\u{4e00}_", blank.as_str())
    );
    let (cells, _, next, cursor) = at_the_edge("\x1b[1;8f\x1b[4h\u{4e00}");
    assert_eq!(cells, "abcdefgxyz");
    assert_eq!(
        (next.chars().next(), cursor),
        (Some('\u{4e00}'), (Row::from_index(1), Col::at_index(2)))
    );
    // With autowrap reset it goes in at the last two positions, 8 and 9,
    // `y` and `x` pushed off and `z` kept at 7.
    let (cells, _, next, _) = at_the_edge("\x1b[?7l\x1b[1;8f\x1b[4h\u{4e00}");
    assert_eq!(
        (cells.as_str(), next.as_str()),
        ("abcdefg\u{4e00}_z", blank.as_str())
    );
}

/// The reversed string `abcdefgh` on row 0, closed with `SDS 0` or left
/// `open`, BDSM reset, DECLRMM's margins at columns 2 to 6, HEM's
/// PRECEDING and IRM set, the cursor put on column `col` with `HPA`, which
/// moves the active data position (8.3.57), then `then` and a `CPR`: row
/// 0 as stored and as drawn, and the report, which under PRESENTATION
/// names the presentation column. Drawn `hgfedcba`, so the string crosses
/// the right margin: `f`, on the margin, at position 2, and `g` and `h`,
/// right of it, at 1 and 0, the home.
fn across_the_margin(open: bool, col: usize, then: &str) -> (String, String, String) {
    support::with_term(&WS, |t| {
        let end = match open {
            true => "",
            false => "\x1b[0]",
        };
        let setup = format!(
            "\x1b[8l\x1b[2]abcdefgh{end}\x1b[?69h\x1b[2;6s\x1b[10h\x1b[4h\x1b[1;1H\x1b[{col}`{then}\x1b[6n"
        );
        support::feed(t, setup.as_bytes());
        let reply = String::from_utf8_lossy(t.reply()).into_owned();
        (row(t, 10), drawn(t, 10), reply)
    })
}

/// **Under PRESENTATION and PRECEDING a character written under IRM shifts
/// only the positions inside the side margins** (7.2.10: "inserted at the
/// active presentation position"; 7.2.9, the contents of the active
/// presentation position shifted "in the direction opposite to that of
/// the character path"; 8.3.64: "the contents of n character positions at
/// the other end of the shifted part are removed"), the side margins
/// bounding the shifted part as they bound FOLLOWING's at the right
/// margin, so a cell right of the margin is neither shifted nor written
/// over. On `f`, the margin's cell, at position 2: the part is position 2
/// alone, `f` leaves it and `X` is drawn there, `g` and `h` kept; on `e`,
/// at 3, `e` moves to 2 and `f` leaves. The cursor stays at its
/// presentation position (7.3.3 b). The same at the left margin, the cells
/// left of it neither shifted nor deleted. A wide character on `f` takes
/// positions 2 and 3, as one at the shifted part's start takes the column
/// after it on a line with no string. With the string closed the cells are
/// the same. Until now the shift took the line's first positions, `h` and
/// `g`, which the margin kept, so nothing left and `X` went in over `g`:
/// `abcdefXh..` from `f`, `abcdeXgh..` from `e`, and from `f` a wide
/// character over `h`, `abcdefg中_`.
#[test]
fn irm_under_preceding_and_presentation_shifts_only_inside_the_margins() {
    assert_eq!(
        across_the_margin(true, 6, "X"),
        ("abcdeXgh..".into(), "hgXedcba..".into(), "\x1b[1;3R".into())
    );
    assert_eq!(
        across_the_margin(true, 5, "X"),
        ("abcdXegh..".into(), "hgeXdcba..".into(), "\x1b[1;4R".into())
    );
    assert_eq!(across_the_margin(false, 6, "X").0, "abcdeXgh..");
    assert_eq!(across_the_margin(false, 5, "X").0, "abcdXegh..");
    // **Mirrored at the left margin**: `xy` left of it at columns 1 and 2,
    // the string `abcdef` after them drawn `fedcba` from position 2, the
    // margins at columns 3 to 10, and on `f`, at 2, the part is position 2
    // alone, `x` and `y` kept.
    let left = support::with_term(&WS, |t| {
        let setup = "\x1b[8lxy\x1b[2]abcdef\x1b[?69h\x1b[3;10s\x1b[10h\x1b[4h\x1b[8`X\x1b[6n";
        support::feed(t, setup.as_bytes());
        let reply = String::from_utf8_lossy(t.reply()).into_owned();
        (row(t, 10), drawn(t, 10), reply)
    });
    assert_eq!(
        left,
        ("xyabcdeX..".into(), "xyXedcba..".into(), "\x1b[1;3R".into())
    );
    assert_eq!(
        across_the_margin(true, 6, "\u{4e00}").0,
        "abcd\u{4e00}_gh.."
    );
    assert_eq!(
        across_the_margin(false, 6, "\u{4e00}").0,
        "abcd\u{4e00}_gh.."
    );
}

/// **`ICH` under PRESENTATION and PRECEDING the same** (8.3.64): on `f` a
/// blank is drawn at position 2 and `f` leaves the line, on `e` the blank
/// at 3, `e` at 2; the blank joins the string of the cell it pushed (TR/53
/// note A.1). Until now `ICH` on `f` changed nothing, and on `e` put the
/// blank at 2 and kept `e` at 3, `abcde.gh..`.
#[test]
fn ich_under_preceding_and_presentation_shifts_only_inside_the_margins() {
    assert_eq!(
        across_the_margin(true, 6, "\x1b[@"),
        ("abcde.gh..".into(), "hg.edcba..".into(), "\x1b[1;3R".into())
    );
    assert_eq!(
        across_the_margin(true, 5, "\x1b[@"),
        ("abcd.egh..".into(), "hge.dcba..".into(), "\x1b[1;4R".into())
    );
}

/// **Right of the right margin a character written under IRM and
/// PRECEDING goes in at the cursor** (7.2.10: "inserted at the active
/// presentation position"; 7.3.3 b: "the active position does not move"),
/// the shifted part running from one past the margin to the cursor, the
/// mirror of `[[source: dec-std-070]]`'s insert-or-replace, which takes
/// the right margin as the shift's boundary only for an Active Position at
/// or left of it and `end_of_line(line)` otherwise (pp. 5-139, 5-140):
/// nothing inside the margins moves. Under PRESENTATION on `g`, at
/// position 1, `g` moves to 0 and `h` leaves; on `h`, at 0, `h` leaves.
/// The cursor stays at its presentation position, on column 7 one past the
/// margin as a move put it, which `CPR` names, not a wrap waiting on the
/// margin. On a line with no string the same at the cursor's column. Until
/// now the character went in on the margin's column, two columns from a
/// cursor on column 8: `abcdefXh..` under PRESENTATION, `acdefXgh..` on
/// a plain line. A wrap waiting on the margin still writes on the
/// margin's column.
#[test]
fn irm_under_preceding_right_of_the_right_margin_goes_in_at_the_cursor() {
    assert_eq!(
        across_the_margin(true, 7, "X"),
        ("abcdefXg..".into(), "gXfedcba..".into(), "\x1b[1;2R".into())
    );
    assert_eq!(
        across_the_margin(true, 8, "X"),
        ("abcdefgX..".into(), "Xgfedcba..".into(), "\x1b[1;1R".into())
    );
    let plain = |then: &str| {
        support::with_term(&WS, |t| {
            let setup = format!("abcdefgh\x1b[?69h\x1b[2;6s\x1b[10h{then}\x1b[6n");
            support::feed(t, setup.as_bytes());
            (row(t, 10), String::from_utf8_lossy(t.reply()).into_owned())
        })
    };
    assert_eq!(
        plain("\x1b[4h\x1b[1;8HX"),
        ("abcdefhX..".into(), "\x1b[1;8R".into())
    );
    assert_eq!(
        plain("\x1b[4h\x1b[1;7HX"),
        ("abcdefXh..".into(), "\x1b[1;7R".into())
    );
    assert_eq!(
        plain("\x1b[1;2HPQRST\x1b[4hX"),
        ("aQRSTXgh..".into(), "\x1b[1;6R".into())
    );
}

/// **A wrap waiting at the line's end right of the right margin, under IRM
/// and PRECEDING, writes on the line's last column**: `XY` written from
/// column 9 fills the line to its last column, past the margin at 6, and
/// leaves the wrap waiting there, on the column DEC's Last Column Flag
/// leaves the Active Position on (`[[source: dec-std-070]]` D.6.1); `Z`
/// goes in there, the part from one past the margin to it shifting toward
/// the home and `g` lost, as a wrap waiting on the margin writes on the
/// margin's column. The cursor stays, the report naming column 10. Until
/// 2026-11-11 `Z` went in on the margin's column, the part from the left
/// margin shifting and `b` lost: `acdefZghXY`, the exception the
/// `CSI 10 h` row named.
#[test]
fn irm_under_preceding_with_a_wrap_waiting_at_the_line_end_right_of_the_margin() {
    let s = support::with_term(&WS, |t| {
        let setup = "abcdefgh\x1b[?69h\x1b[2;6s\x1b[10h\x1b[1;9HXY\x1b[4hZ\x1b[6n";
        support::feed(t, setup.as_bytes());
        (row(t, 10), String::from_utf8_lossy(t.reply()).into_owned())
    });
    assert_eq!(s, ("abcdefhXYZ".into(), "\x1b[1;10R".into()));
}
