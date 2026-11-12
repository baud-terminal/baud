//! **Writing on a row outside the top and bottom margins reaches the
//! line's end**, `[[source: dec-std-070]]`'s INSERT OR REPLACE GRAPHIC
//! CHARACTER (pp. 5-139 and 5-140): "If the Active Position is within the
//! Scroll Area, it will advance to the Right Margin. If the Active Position
//! is outside the Scroll Area, it will advance to the last column of the
//! Active Line"; "If Auto Wrap Mode is on and the Active position is
//! outside the Vertical Scroll Margins, the Active Position will advance
//! from the last column of the Active Line, to the first column of the next
//! line ... No scrolling will occur"; and under IRM, "Data shifted past the
//! Right Margin within the Scroll Area, or the last column of the Active
//! Line outside the Scroll Area is lost".
//!
//! The scenarios are j4james's margins test (`[[source: wt-margins-py]]`,
//! `test_output`), at 80 by 24 with the top and bottom margins at rows 9
//! and 16 and the side margins at columns 21 and 60, each row filled with
//! its own letter; a VT420 and a VT520 logged every one of them as the text
//! gives it (`[[source: wt-margins-vt420-log]]` and
//! `[[source: wt-margins-vt520-log]]`, lines 250-259). xterm and ghostty
//! keep the right margin on those rows: a source conflict, recorded in
//! `docs/vt-coverage.md`.
//!
//! **Moves are not writes**: `CUF` on those rows still stops at the right
//! margin, STD 070's CUF note 1 (p. 5-45), "The Active Position will not
//! move beyond the Right Margin", where the same logs show a VT420 and a
//! VT520 passing it.

mod support;

use baud_core::*;

const WS: WinSize = WinSize {
    ws_row: 24,
    ws_col: 80,
};

/// `DECLRMM` set, then `DECSTBM 9;16` and `DECSLRM 21;60`, as the test's
/// `margins(9,16,21,60)` sends them.
const MARGINS: &str = "\x1b[?69h\x1b[9;16r\x1b[21;60s";

/// The test's `horizontal_test_pattern()`: row `r` (1-based) filled with
/// the `r`th capital letter.
fn horizontal() -> String {
    (1..=24)
        .map(|r| format!("\x1b[{r};1H{}", letter(r).to_string().repeat(80)))
        .collect()
}

/// Row `r`'s letter in [`horizontal`] (1-based).
fn letter(r: usize) -> char {
    char::from(b'A' + u8::try_from(r - 1).expect("a row of 24"))
}

/// The test's `vertical_test_pattern()` line: `A-Z a-z A-Z a-z`, cut at
/// eighty columns.
fn vertical() -> String {
    let up: String = ('A'..='Z').collect();
    let low: String = ('a'..='z').collect();
    format!("{up}{low}{up}{low}")[..80].to_string()
}

/// Every row as text, a blank cell read as a space, as the test's
/// `read_screen` treats NUL and SP alike.
fn rows(t: &Term<'_>) -> Vec<String> {
    (0..24)
        .map(|y| {
            t.viewport().row(Row::from_index(y))[..80]
                .iter()
                .map(|c| {
                    c.content
                        .as_codepoint()
                        .and_then(|cp| char::from_u32(cp.value()))
                        .unwrap_or(' ')
                })
                .collect()
        })
        .collect()
}

/// `n` characters of row `r` from column `c`, both 1-based: the test's
/// `row_is(r, ..., c)`.
fn at(rows: &[String], r: usize, c: usize, n: usize) -> String {
    rows[r - 1].chars().skip(c - 1).take(n).collect()
}

/// The screen and the `CPR` after `bytes` and `CSI 6 n`.
fn after(bytes: &str) -> (Vec<String>, String) {
    support::with_term(&WS, |t| {
        support::feed(t, format!("{bytes}\x1b[6n").as_bytes());
        (rows(t), String::from_utf8_lossy(t.reply()).into_owned())
    })
}

/// A `CPR` naming row `r` and column `c`, 1-based.
fn cpr(r: usize, c: usize) -> String {
    format!("\x1b[{r};{c}R")
}

/// **Before the top margin and beyond the bottom one, a write from inside
/// the side margins runs past the right margin to the line's end**: "If
/// the Active Position is outside the Scroll Area, it will advance to the
/// last column of the Active Line" (p. 5-139). The test's `Output across
/// right margin, before top` and `beyond bottom`, logged `(8, 66)
/// 'H0123456789H'` and `(17, 66) 'Q0123456789Q'` on both terminals. Until
/// now baud wrapped at the right margin, as xterm does.
#[test]
fn a_write_outside_the_vertical_margins_runs_past_the_right_margin() {
    for r in [8, 17] {
        let (s, reply) = after(&format!("{}{MARGINS}\x1b[{r};56H0123456789", horizontal()));
        let l = letter(r);
        assert_eq!(
            (at(&s, r, 55, 12), reply),
            (format!("{l}0123456789{l}"), cpr(r, 66)),
            "row {r}"
        );
    }
}

/// **A wrap from the line's end on a row outside the vertical margins goes
/// to the first column of the next line**, not the left margin: "the Active
/// Position will advance from the last column of the Active Line, to the
/// first column of the next line" (p. 5-139). The test's `Output across
/// end of line, before top`, `at top margin` and `beyond bottom`, logged
/// `(6, 6)`, `(9, 6)` and `(21, 6)`; the line reached at the top margin is
/// inside the region, and the wrap still goes to its first column, because
/// where the Active Position was decides. Until now the wrap went to the
/// left margin, column 21.
#[test]
fn a_wrap_outside_the_vertical_margins_goes_to_the_first_column() {
    for r in [5, 8, 20] {
        let (s, reply) = after(&format!("{}{MARGINS}\x1b[{r};76H0123456789", horizontal()));
        let (l, next) = (letter(r), letter(r + 1));
        assert_eq!(
            (at(&s, r, 75, 6), at(&s, r + 1, 1, 6), reply),
            (format!("{l}01234"), format!("56789{next}"), cpr(r + 1, 6)),
            "from row {r}"
        );
    }
}

/// **On the last row, below the region, the wrap scrolls nothing**: "No
/// scrolling will occur" (p. 5-139). The test's `Output across end of
/// line, at bottom of page`, logged `(24, 6) 'X01234' '56789X'`: the wrap
/// takes the line's own first column, and row 23 keeps its letter.
#[test]
fn a_wrap_on_the_last_row_below_the_region_does_not_scroll() {
    let (s, reply) = after(&format!("{}{MARGINS}\x1b[24;76H0123456789", horizontal()));
    assert_eq!(
        (at(&s, 24, 75, 6), at(&s, 24, 1, 6), reply),
        ("X01234".to_string(), "56789X".to_string(), cpr(24, 6))
    );
    assert_eq!(s[22], "W".repeat(80), "row 23 stayed");
}

/// **Inside the vertical margins the side margins still bound a write**:
/// the test's `Output across right margin, within vertical margins` and
/// `at bottom margin`, and `Output across end of line` on the same rows,
/// logged `(13, 26) 'L01234L' 'M56789M'`, `(16, 26) 'P01234O' 'P56789_'`,
/// `(13, 26) 'L01234' 'M56789M'` and `(16, 26) 'P01234' 'P56789_'`.
/// Unchanged, and the control for the rows outside.
#[test]
fn a_write_inside_the_vertical_margins_keeps_the_side_margins() {
    let cases = [
        (12, 56, (12, 55, "L01234L"), (13, 20, "M56789M"), (13, 26)),
        (16, 56, (15, 55, "P01234O"), (16, 20, "P56789 "), (16, 26)),
        (12, 76, (12, 75, "L01234"), (13, 20, "M56789M"), (13, 26)),
        (16, 76, (16, 75, "P01234"), (16, 20, "P56789 "), (16, 26)),
    ];
    for (r, c, one, two, to) in cases {
        let (s, reply) = after(&format!("{}{MARGINS}\x1b[{r};{c}H0123456789", horizontal()));
        assert_eq!(
            (
                at(&s, one.0, one.1, one.2.len()),
                at(&s, two.0, two.1, two.2.len()),
                reply
            ),
            (one.2.to_string(), two.2.to_string(), cpr(to.0, to.1)),
            "from {r};{c}"
        );
    }
}

/// **Back inside the region the side margins bound a write again**, however
/// the cursor came there from a row outside it: `LF` from row 8 onto row 9,
/// the region's top; `CUP` after a write on row 8; `RI` from row 17 onto
/// row 16, the region's bottom; and `LF` from row 8 after `ED 0` from row 8,
/// `EL 2` on row 9, `DECERA` over row 9, or `?1049h` onto the alternate
/// screen, each of which erases row 9 whole. Every write wraps at the right
/// margin to the left margin of the next line, as the rows inside do
/// above. A fence left at the line's end would carry the write past the
/// margin inside the region.
#[test]
fn a_write_back_inside_the_region_keeps_the_side_margins() {
    let fed = |then: &str| after(&format!("{}{MARGINS}{then}0123456789", horizontal()));
    let cases = [
        ("\x1b[8;56H\n", "I01234I", "J56789J", (10, 26)),
        ("\x1b[8;1Hx\x1b[12;56H", "L01234L", "M56789M", (13, 26)),
        ("\x1b[8;56H\x1b[J\n", " 01234 ", " 56789 ", (10, 26)),
        (
            "\x1b[9;1H\x1b[2K\x1b[8;56H\n",
            " 01234 ",
            "J56789J",
            (10, 26),
        ),
        (
            "\x1b[8;56H\x1b[9;1;9;80$z\n",
            " 01234 ",
            "J56789J",
            (10, 26),
        ),
        ("\x1b[8;56H\x1b[?1049h\n", " 01234 ", " 56789 ", (10, 26)),
    ];
    for (then, one, two, to) in cases {
        let (s, reply) = fed(then);
        let r = to.0 - 1;
        assert_eq!(
            (at(&s, r, 55, 7), at(&s, r + 1, 20, 7), reply),
            (one.to_string(), two.to_string(), cpr(to.0, to.1)),
            "{then:?}"
        );
    }
    let (s, reply) = fed("\x1b[17;56H\x1bM");
    assert_eq!(
        (at(&s, 15, 55, 7), at(&s, 16, 20, 7), reply),
        ("P01234O".to_string(), "P56789 ".to_string(), cpr(16, 26)),
        "RI from row 17"
    );
}

/// **A row outside the region erased whole still writes to its end**:
/// `EL 2` on row 8 leaves it blank, and after the cursor has been inside
/// the region and come back, ten characters from column 56 land on 56 to
/// 65, as on a row never erased.
#[test]
fn a_row_outside_the_region_erased_whole_still_writes_to_the_line_end() {
    let (s, reply) = after(&format!(
        "{}{MARGINS}\x1b[8;1H\x1b[2K\x1b[12;1H\x1b[8;56H0123456789",
        horizontal()
    ));
    assert_eq!(
        (at(&s, 8, 55, 12), reply),
        (" 0123456789 ".to_string(), cpr(8, 66))
    );
}

/// **Under IRM on a row outside the vertical margins the shift runs to the
/// line's end**, from a cursor inside the side margins: "Data shifted past
/// the Right Margin within the Scroll Area, or the last column of the
/// Active Line outside the Scroll Area is lost" (p. 5-140). Row 8 holds
/// the test's vertical pattern; `X` on column 56 moves columns 56 to 79 one
/// right and column 80's `b` is lost, as it was before this change: the
/// gap a write opens under DATA already ignored the side margins there, as
/// ICH does (the `CSI @` row).
///
/// **Under DCSM's PRESENTATION too**, on a line holding the reversed string
/// `abcdefgh` (BDSM reset, `SDS 2`), at 4 by 10 with the scroll area at
/// rows 2 and 3 and the side margins at columns 2 to 6: from `d`, inside
/// the side margins, on row 1 above the region and row 4 below it, the row
/// and the report are the ones the same write leaves with no margins set.
/// Until now that shift stopped at the right margin: the shift's end read
/// the margin's fence on every row.
#[test]
fn irm_on_a_row_outside_the_vertical_margins_shifts_to_the_line_end() {
    let v = vertical();
    let (s, reply) = after(&format!(
        "{}\x1b[8;1H{v}{MARGINS}\x1b[4h\x1b[8;56HX",
        horizontal()
    ));
    let want = format!("{}X{}", &v[..55], &v[55..79]);
    assert_eq!((s[7].clone(), reply), (want, cpr(8, 57)));
    let small = WinSize {
        ws_row: 4,
        ws_col: 10,
    };
    let presented = |margins: &str, row: usize| {
        support::with_term(&small, |t| {
            let setup = format!(
                "\x1b[{row}H\x1b[8l\x1b[2]abcdefgh\x1b[0]\x1b[9l{margins}\x1b[4h\x1b[{row}H\x1b[4`X\x1b[6n"
            );
            support::feed(t, setup.as_bytes());
            let text: String = t.viewport().row(Row::from_index(row - 1))[..10]
                .iter()
                .map(|c| {
                    c.content
                        .as_codepoint()
                        .and_then(|cp| char::from_u32(cp.value()))
                        .unwrap_or('.')
                })
                .collect();
            (text, String::from_utf8_lossy(t.reply()).into_owned())
        })
    };
    for row in [1, 4] {
        assert_eq!(
            presented("\x1b[2;3r\x1b[?69h\x1b[2;6s", row),
            presented("", row),
            "PRESENTATION, row {row}"
        );
    }
}

/// **With DECAWM reset a write outside the vertical margins stops at the
/// line's end, not the right margin**: "If Auto Wrap mode is off, the
/// Active Position will not advance beyond the Right Margin or last column
/// of the Active Line" (p. 5-139), the last column on a row outside the
/// Scroll Area. Ten characters from column 56 land on 56 to 65; thirty
/// land on 56 to 80, the last six overwriting column 80. Until now they
/// stopped at the margin, column 60 taking the fifth character on.
#[test]
fn with_decawm_reset_a_write_outside_the_vertical_margins_stops_at_the_line_end() {
    let (s, reply) = after(&format!(
        "{}{MARGINS}\x1b[?7l\x1b[8;56H0123456789",
        horizontal()
    ));
    assert_eq!(
        (at(&s, 8, 55, 12), reply),
        ("H0123456789H".to_string(), cpr(8, 66))
    );
    let text = "abcdefghijklmnopqrstuvwxyz0123";
    let (s, reply) = after(&format!(
        "{}{MARGINS}\x1b[?7l\x1b[8;56H{text}",
        horizontal()
    ));
    assert_eq!(
        (at(&s, 8, 55, 26), reply),
        ("Habcdefghijklmnopqrstuvwx3".to_string(), cpr(8, 80))
    );
}

/// **A write that ends on the right margin of a row outside the vertical
/// margins leaves the cursor one past it, on a column**: the margin is not
/// the row's last column, so the Active Position advances (p. 5-139) and no
/// wrap waits. `CPR` names column 61, and `EL 0` from there keeps the `4`
/// written on the margin. Until now the write left a wrap waiting on the
/// margin: `CPR` named column 60, and `EL 0` erased the `4`.
#[test]
fn a_write_ending_on_the_margin_outside_the_vertical_margins_is_one_past_it() {
    let (s, reply) = after(&format!("{}{MARGINS}\x1b[8;56H01234", horizontal()));
    assert_eq!(
        (at(&s, 8, 55, 7), reply),
        ("H01234H".to_string(), cpr(8, 61))
    );
    let (s, _) = after(&format!("{}{MARGINS}\x1b[8;56H01234\x1b[K", horizontal()));
    assert_eq!(
        at(&s, 8, 55, 26),
        format!("H01234{}", " ".repeat(20)),
        "EL 0 from column 61"
    );
}

/// **`DECSTBM` homes the cursor, and the line it homes to is outside the
/// new region**: note 1, "Execution of this control causes the Active
/// position to be set to the page origin obeying Origin Mode (DECOM):
/// First column of first line if Origin Mode is in the reset (Absolute)
/// state" (p. 5-25), and the algorithm's `active_position.line = 1;
/// active_position.column = 1` (p. 5-26). With the side margins set
/// first, row 1 is inside the scroll area until `DECSTBM 9;16`, which puts
/// the cursor on it outside: sixty-five characters then run to column 65.
/// Until now they wrapped at the right margin.
#[test]
fn decstbm_homes_onto_a_row_outside_the_region_whose_writes_run_to_the_line_end() {
    let text: String = "0123456789".repeat(7)[..65].to_string();
    let (s, reply) = after(&format!(
        "{}\x1b[?69h\x1b[21;60s\x1b[9;16r{text}",
        horizontal()
    ));
    assert_eq!(
        (s[0].clone(), reply),
        (format!("{text}{}", "A".repeat(15)), cpr(1, 66))
    );
}

/// **`DECSLRM` homes the cursor too**: note 1, the same sentence (p.
/// 5-27), and the algorithm's `active_position.line = 1;
/// active_position.column = 1` (p. 5-28). With `DECSTBM 9;16` set first,
/// the cursor is on row 1 outside the region when `DECSLRM 21;60` narrows
/// the side margins, and a write from there runs to column 65. Until now it
/// wrapped at the right margin.
#[test]
fn decslrm_homes_onto_a_row_outside_the_region_whose_writes_run_to_the_line_end() {
    let text: String = "0123456789".repeat(7)[..65].to_string();
    let (s, reply) = after(&format!(
        "{}\x1b[?69h\x1b[9;16r\x1b[21;60s{text}",
        horizontal()
    ));
    assert_eq!(
        (s[0].clone(), reply),
        (format!("{text}{}", "A".repeat(15)), cpr(1, 66))
    );
}

/// **`?69` reset, then set again**: with DECLRMM reset there are no side
/// margins (p. 5-29, "set to be at the extremes of the page"), so from a
/// cursor on row 8 a write wraps at the line's end to the first column of
/// row 9; set again, and `DECSLRM 21;60` sent again, a write on row 8 runs
/// past the margin to the line's end once more, and one on row 12, inside
/// the region, wraps at the margin.
#[test]
fn declrmm_reset_and_set_again_keeps_the_rows_outside_writing_to_the_line_end() {
    let text = "abcdefghijklmnopqrstuvwxyz0123";
    let (s, reply) = after(&format!(
        "{}{MARGINS}\x1b[8;56H\x1b[?69l{text}",
        horizontal()
    ));
    assert_eq!(
        (at(&s, 8, 55, 26), at(&s, 9, 1, 6), reply),
        (
            format!("H{}", &text[..25]),
            format!("{}I", &text[25..]),
            cpr(9, 6)
        ),
        "reset"
    );
    let again = format!("{}{MARGINS}\x1b[?69l\x1b[?69h\x1b[21;60s", horizontal());
    let (s, reply) = after(&format!("{again}\x1b[8;56H0123456789"));
    assert_eq!(
        (at(&s, 8, 55, 12), reply),
        ("H0123456789H".to_string(), cpr(8, 66)),
        "set again, row 8"
    );
    let (s, reply) = after(&format!("{again}\x1b[12;56H0123456789"));
    assert_eq!(
        (at(&s, 12, 55, 7), reply),
        ("L01234L".to_string(), cpr(13, 26)),
        "set again, row 12"
    );
}

/// **`?69` set again while it is set, with the cursor on a row outside the
/// region, keeps that row writing to its end**: the set makes every line
/// single-width (p. 5-29) and moves no margin, so ten characters from
/// column 56 of row 8 still land on 56 to 65; on row 12, inside, they wrap
/// at the margin.
#[test]
fn declrmm_set_while_set_keeps_the_row_outside_writing_to_the_line_end() {
    let (s, reply) = after(&format!(
        "{}{MARGINS}\x1b[8;56H\x1b[?69h0123456789",
        horizontal()
    ));
    assert_eq!(
        (at(&s, 8, 55, 12), reply),
        ("H0123456789H".to_string(), cpr(8, 66))
    );
    let (s, reply) = after(&format!(
        "{}{MARGINS}\x1b[12;56H\x1b[?69h0123456789",
        horizontal()
    ));
    assert_eq!(
        (at(&s, 12, 55, 7), reply),
        ("L01234L".to_string(), cpr(13, 26))
    );
}

/// **`CUF` on a row outside the vertical margins stops at the right
/// margin**: STD 070's CUF note 1, "If the Active Position is at or inside
/// the Right Margin when the CUF control is executed, and an attempt is
/// made to move the Active Position beyond the Right Margin, the control
/// will be executed until the Active Position reaches the Right Margin"
/// (p. 5-45), which names no row. The test's `CUF across right margin,
/// before top` and `beyond bottom` expect `(5, 60)` and `(20, 60)`, and a
/// VT420 and a VT520 logged `(5, 66)` and `(20, 66)`: a conflict, the text
/// followed. A write there runs to the line's end; a move does not.
#[test]
fn cuf_on_a_row_outside_the_vertical_margins_stops_at_the_margin() {
    for r in [5, 20] {
        let (_, reply) = after(&format!("{}{MARGINS}\x1b[{r};56H\x1b[10C", horizontal()));
        assert_eq!(reply, cpr(r, 60), "row {r}");
    }
}

/// **The other moves keep the right margin there too**: DECFI from column
/// 56 steps one right, `CHT` from it reaches the stop at 57, and a `CUB`
/// from 56 goes back ten; `ICH` from column 56 shifts to the line's end, as
/// it did before (the `CSI @` row). Each as on the same row with no scroll
/// area, so none reads a fence the row's write runs on to.
#[test]
fn moves_on_a_row_outside_the_vertical_margins_keep_the_margin() {
    for (then, to) in [
        ("\x1b9", (5, 57)),
        ("\x1b[I", (5, 57)),
        ("\x1b[10D", (5, 46)),
        ("\x1b[@", (5, 56)),
    ] {
        let (_, reply) = after(&format!("{}{MARGINS}\x1b[5;56H{then}", horizontal()));
        assert_eq!(reply, cpr(to.0, to.1), "{then:?}");
    }
}

/// **No move reads the write fence**: the bodies of `CUF`'s, `CUB`'s,
/// `HT` and `CHT`'s, DECFI's and DECBI's functions name neither the stored
/// fence nor the write fence's accessors; each reads the move fence or the
/// right margin, and the cursor's column through a report's reading.
/// Positive controls: the probe finds `write_fence(` in `line_fence`, and
/// `move_stop` in `forward_in_margins`.
#[test]
fn no_move_reads_the_write_fence() {
    let read = |f: &str| {
        let p = format!("{}/src/{f}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{p}: {e}"))
    };
    let (ops, rect, paint) = (read("ops.rs"), read("rect.rs"), read("paint.rs"));
    assert!(body_of(&paint, "line_fence").contains("write_fence("));
    assert!(body_of(&ops, "forward_in_margins").contains("move_stop"));
    let moves: [(&str, &[&str]); 2] = [
        (
            ops.as_str(),
            &[
                "forward_in_margins",
                "forward_to_line_end",
                "forward_fenced",
                "back_in_margins",
                "back_past_the_fence",
                "back_fenced",
                "tab_forward",
                "tab_forward_fenced",
                "tab_end",
            ],
        ),
        (rect.as_str(), &["back_index", "forward_index"]),
    ];
    let mut found = Vec::new();
    for (src, names) in moves {
        for name in names {
            let body = body_of(src, name);
            for banned in ["cold.fence", "line_fence(", "write_fence(", "WriteFence"] {
                if body.contains(banned) {
                    found.push(format!("{name}: {banned}"));
                }
            }
        }
    }
    assert!(found.is_empty(), "{found:#?}");
}

/// The body of `fn name(` in `src`, from its first `{` to the brace that
/// closes it.
fn body_of<'a>(src: &'a str, name: &str) -> &'a str {
    let sig = format!("fn {name}(");
    let at = src.find(&sig).unwrap_or_else(|| panic!("no `fn {name}(`"));
    let open = at + src[at..].find('{').expect("a body");
    let mut depth = 0usize;
    for (i, ch) in src[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &src[open..open + i + 1];
                }
            }
            _ => {}
        }
    }
    panic!("the body of `fn {name}(` is not closed")
}
